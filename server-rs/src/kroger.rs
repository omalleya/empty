//! Thin wrapper over the Kroger public API.
//!
//! Base URL:  https://api.kroger.com/v1
//! Endpoints used:
//!   GET  /locations            find a store by zip
//!   GET  /products             search by term, scoped to a location
//!   PUT  /cart/add             add UPCs to the authenticated user's cart

use std::time::Duration;

use serde_json::{json, Value};

use crate::auth::KrogerAuth;
use crate::error::{check, Error, Result};

const BASE: &str = "https://api.kroger.com/v1";

/// A Kroger-family store (Fred Meyer, Ralphs, King Soopers, ...).
#[derive(Clone, serde::Serialize)]
pub struct Location {
    pub location_id: String,
    pub name: String,
    pub chain: String,
    pub address: String,
}

impl Location {
    fn from_api(raw: &Value) -> Option<Self> {
        let addr = &raw["address"];
        let address = ["addressLine1", "city", "state"]
            .iter()
            .filter_map(|k| addr[k].as_str().filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            location_id: raw["locationId"].as_str()?.to_string(),
            name: raw["name"].as_str().unwrap_or("").to_string(),
            chain: raw["chain"].as_str().unwrap_or("").to_string(),
            address,
        })
    }
}

/// One product hit from the Products API.
#[derive(Clone)]
pub struct Product {
    pub upc: String,
    pub description: String,
    pub brand: String,
    pub size: String,
    pub price: Option<f64>,
}

impl Product {
    fn from_api(raw: &Value) -> Option<Self> {
        // The Products API nests price/size under items[0].
        let item0 = raw["items"].get(0).cloned().unwrap_or(Value::Null);
        let price_block = &item0["price"];
        // Kroger returns promo + regular; prefer promo when non-zero.
        let nonzero = |v: &Value| v.as_f64().filter(|p| *p != 0.0);
        let price = nonzero(&price_block["promo"]).or_else(|| nonzero(&price_block["regular"]));
        Some(Self {
            upc: raw["upc"].as_str()?.to_string(),
            description: raw["description"].as_str().unwrap_or("").to_string(),
            brand: raw["brand"].as_str().unwrap_or("").to_string(),
            size: item0["size"].as_str().unwrap_or("").to_string(),
            price,
        })
    }

    pub fn label(&self) -> String {
        let mut text = [&self.brand, &self.description, &self.size]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(price) = self.price {
            text += &format!(" — ${price:.2}");
        }
        text
    }
}

/// A line to push into the cart.
pub struct CartItem {
    pub upc: String,
    pub quantity: u32,
}

pub struct KrogerClient {
    pub auth: KrogerAuth,
    location_id: Option<String>,
    http: reqwest::Client,
}

impl KrogerClient {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            auth: KrogerAuth::from_env()?,
            location_id: std::env::var("KROGER_LOCATION_ID")
                .ok()
                .filter(|v| !v.is_empty()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(Error::Http)?,
        })
    }

    // ------------------------------------------------------------------ //
    // Locations
    // ------------------------------------------------------------------ //
    pub async fn find_location(&self, zip_code: &str, limit: u32) -> Result<Vec<Location>> {
        let resp = self
            .http
            .get(format!("{BASE}/locations"))
            .bearer_auth(self.auth.app_token().await?)
            .query(&[
                ("filter.zipCode.near", zip_code),
                ("filter.limit", &limit.to_string()),
            ])
            .send()
            .await?;
        let body: Value = check(resp).await?.json().await?;
        Ok(data_array(&body).iter().filter_map(Location::from_api).collect())
    }

    // ------------------------------------------------------------------ //
    // Products
    // ------------------------------------------------------------------ //
    pub async fn search_products(&self, term: &str, limit: u32) -> Result<Vec<Product>> {
        let loc = self.location_id.as_deref().ok_or_else(|| {
            Error::Config(
                "No location_id set. Call /stores/{zip} and set KROGER_LOCATION_ID.".into(),
            )
        })?;
        let resp = self
            .http
            .get(format!("{BASE}/products"))
            .bearer_auth(self.auth.app_token().await?)
            .query(&[
                ("filter.term", term),
                ("filter.locationId", loc),
                ("filter.limit", &limit.to_string()),
            ])
            .send()
            .await?;
        let body: Value = check(resp).await?.json().await?;
        Ok(data_array(&body).iter().filter_map(Product::from_api).collect())
    }

    // ------------------------------------------------------------------ //
    // Cart
    // ------------------------------------------------------------------ //
    pub async fn add_to_cart(&self, items: &[CartItem]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let body = json!({
            "items": items.iter().map(|i| json!({
                "upc": i.upc,
                "quantity": i.quantity,
                "modality": "PICKUP",
            })).collect::<Vec<_>>(),
        });
        let resp = self
            .http
            .put(format!("{BASE}/cart/add"))
            .bearer_auth(self.auth.user_token().await?)
            .json(&body)
            .send()
            .await?;
        // Success is 204 No Content.
        check(resp).await?;
        Ok(())
    }
}

fn data_array(body: &Value) -> Vec<Value> {
    body["data"].as_array().cloned().unwrap_or_default()
}
