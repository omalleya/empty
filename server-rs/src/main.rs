//! Cart import server (Rust port of the FastAPI app in ../server).
//!
//! Endpoints:
//!   POST /cart/import      {"items": ["2% milk", ...]}  -> resolve + add to cart
//!   GET  /stores/{zip}     nearby store locationIds (to fill KROGER_LOCATION_ID)
//!   GET  /health
//!
//! This is what the iOS app's "Send to cart" / Siri intent talks to. Running it
//! server-side keeps the family's Kroger OAuth refresh token off the phone — the
//! app only ever sends plain item-name strings.
//!
//! Run:
//!   cargo run                          # serve on :8000 (PORT overrides)
//!   cargo run -- --login               # one-time Kroger cart authorization
//!   cargo run -- --find-store 97232    # nearby store locationIds
//!
//! Needs the same env as the Python version: KROGER_CLIENT_ID/SECRET,
//! KROGER_LOCATION_ID, ANTHROPIC_API_KEY (see ../.env.example). The cart push
//! needs the one-time `--login` to mint a refresh token; after that
//! KROGER_TOKEN_STORE holds it for unattended use.

mod auth;
mod error;
mod kroger;
mod resolver;

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use error::Error;
use kroger::{CartItem, KrogerClient};
use resolver::{ResolvedItem, Resolver};

struct AppState {
    client: KrogerClient,
    // One import at a time keeps the UPC cache writes simple; fine for a
    // family-sized workload.
    resolver: tokio::sync::Mutex<Resolver>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--login") => {
            let client = KrogerClient::from_env()?;
            client.auth.login().await?;
            Ok(())
        }
        Some("--find-store") => {
            let zip = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("usage: kroger-cart-server --find-store ZIP"))?;
            let client = KrogerClient::from_env()?;
            for loc in client.find_location(zip, 5).await? {
                println!(
                    "{}  {:12} {}  ({})",
                    loc.location_id, loc.chain, loc.name, loc.address
                );
            }
            println!("\nCopy the locationId you want into KROGER_LOCATION_ID in .env");
            Ok(())
        }
        None => serve().await,
        Some(other) => anyhow::bail!("unknown argument {other:?} (try --login or --find-store ZIP)"),
    }
}

async fn serve() -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        client: KrogerClient::from_env()?,
        resolver: tokio::sync::Mutex::new(Resolver::from_env()?),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/stores/{zip}", get(stores))
        .route("/cart/import", post(cart_import))
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("kroger-cart-server listening on :{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// --------------------------------------------------------------------------- //
// Schemas (wire-compatible with the FastAPI version)
// --------------------------------------------------------------------------- //
#[derive(serde::Deserialize)]
struct ImportRequest {
    items: Vec<String>,
}

#[derive(serde::Serialize)]
struct ProductOut {
    upc: String,
    label: String,
}

#[derive(serde::Serialize)]
struct ItemResult {
    raw_text: String,
    quantity: u32,
    // added | added_from_cache | needs_confirmation | no_match
    status: String,
    chosen: Option<ProductOut>,
    candidates: Vec<ProductOut>,
    note: String,
}

#[derive(serde::Serialize)]
struct ImportResponse {
    added: usize,
    needs_attention: usize,
    message: String,
    items: Vec<ItemResult>,
}

fn to_result(item: &ResolvedItem) -> ItemResult {
    let (status, chosen) = match &item.chosen {
        Some(product) => (
            if item.from_cache { "added_from_cache" } else { "added" },
            Some(ProductOut {
                upc: product.upc.clone(),
                label: if product.description.is_empty() {
                    product.label()
                } else {
                    product.description.clone()
                },
            }),
        ),
        None => (
            if item.candidates.is_empty() { "no_match" } else { "needs_confirmation" },
            None,
        ),
    };
    ItemResult {
        raw_text: item.raw_text.clone(),
        quantity: item.quantity,
        status: status.to_string(),
        chosen,
        candidates: item
            .candidates
            .iter()
            .take(5)
            .map(|p| ProductOut { upc: p.upc.clone(), label: p.label() })
            .collect(),
        note: item.note.clone(),
    }
}

// --------------------------------------------------------------------------- //
// Routes
// --------------------------------------------------------------------------- //
async fn health() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn stores(
    State(state): State<Arc<AppState>>,
    Path(zip): Path<String>,
) -> Result<Json<Vec<Value>>, ApiError> {
    let locations = state
        .client
        .find_location(&zip, 5)
        .await
        .map_err(|e| ApiError::bad_gateway(format!("Kroger error: {e}")))?;
    Ok(Json(
        locations
            .into_iter()
            .map(|loc| {
                json!({
                    "location_id": loc.location_id,
                    "name": loc.name,
                    "chain": loc.chain,
                    "address": loc.address,
                })
            })
            .collect(),
    ))
}

async fn cart_import(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportResponse>, ApiError> {
    if req.items.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "No items provided."));
    }

    let mut resolver = state.resolver.lock().await;
    let resolved = resolver
        .resolve(&req.items, &state.client)
        .await
        .map_err(import_error)?;
    drop(resolver);

    // Push the confident ones to the cart.
    let cart: Vec<CartItem> = resolved
        .iter()
        .filter_map(|item| {
            item.chosen.as_ref().map(|p| CartItem {
                upc: p.upc.clone(),
                quantity: item.quantity,
            })
        })
        .collect();
    if !cart.is_empty() {
        state.client.add_to_cart(&cart).await.map_err(import_error)?;
    }

    let items: Vec<ItemResult> = resolved.iter().map(to_result).collect();
    let added = items.iter().filter(|i| i.status.starts_with("added")).count();
    let needs = items.len() - added;
    let mut message = format!("{added} added");
    if needs > 0 {
        message += &format!(", {needs} need attention");
    }
    Ok(Json(ImportResponse { added, needs_attention: needs, message, items }))
}

fn import_error(err: Error) -> ApiError {
    match err {
        // e.g. missing KROGER_LOCATION_ID or no saved refresh token
        Error::Config(msg) => ApiError::new(StatusCode::BAD_REQUEST, msg),
        Error::KrogerHttp { status: 401 | 403 } => ApiError::bad_gateway(
            "Cart authorization failed. The server needs a one-time Kroger login \
             to save a refresh token, and the app must have Cart API access.",
        ),
        Error::KrogerHttp { status } => {
            ApiError::bad_gateway(format!("Kroger API error ({status})."))
        }
        other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

// FastAPI-style error body: {"detail": "..."} — keeps the iOS client happy.
struct ApiError {
    status: StatusCode,
    detail: String,
}

impl ApiError {
    fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self { status, detail: detail.into() }
    }

    fn bad_gateway(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, detail)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"detail": self.detail}))).into_response()
    }
}
