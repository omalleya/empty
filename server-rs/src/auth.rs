//! Kroger OAuth2.
//!
//! Two grant types, two purposes:
//!
//!   * client_credentials  -> Products + Locations APIs. App-level, no user login.
//!                            Scope: "product.compact". Short-lived, fetched on demand.
//!
//!   * authorization_code  -> Cart API. Needs the *user* (the family account) to
//!                            authorize once in a browser. Scope: "cart.basic:write".
//!                            The refresh_token is persisted so login happens once;
//!                            every later run silently refreshes the access token.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use url::Url;

use crate::error::{check, Error, Result};

const TOKEN_URL: &str = "https://api.kroger.com/v1/connect/oauth2/token";
const AUTHORIZE_URL: &str = "https://api.kroger.com/v1/connect/oauth2/authorize";

pub const CART_SCOPE: &str = "cart.basic:write profile.compact";
pub const PRODUCT_SCOPE: &str = "product.compact";

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

impl CachedToken {
    fn valid(&self) -> bool {
        // 30s safety margin.
        Instant::now() + Duration::from_secs(30) < self.expires_at
    }
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    refresh_token: Option<String>,
}

fn default_expires_in() -> u64 {
    1800
}

impl TokenResponse {
    fn cached(&self) -> CachedToken {
        CachedToken {
            access_token: self.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(self.expires_in),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredToken {
    refresh_token: String,
    saved_at: f64,
}

pub struct KrogerAuth {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    token_store: PathBuf,
    http: reqwest::Client,
    app_token: Mutex<Option<CachedToken>>,
    user_token: Mutex<Option<CachedToken>>,
}

impl KrogerAuth {
    pub fn from_env() -> Result<Self> {
        let client_id = require_env("KROGER_CLIENT_ID")?;
        let client_secret = require_env("KROGER_CLIENT_SECRET")?;
        let redirect_uri = std::env::var("KROGER_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8088/callback".into());
        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            token_store: token_store_path(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(Error::Http)?,
            app_token: Mutex::new(None),
            user_token: Mutex::new(None),
        })
    }

    fn basic_auth_header(&self) -> String {
        format!(
            "Basic {}",
            BASE64.encode(format!("{}:{}", self.client_id, self.client_secret))
        )
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .header("Authorization", self.basic_auth_header())
            .form(form)
            .send()
            .await?;
        Ok(check(resp).await?.json().await?)
    }

    // ------------------------------------------------------------------ //
    // client_credentials — products & locations
    // ------------------------------------------------------------------ //
    pub async fn app_token(&self) -> Result<String> {
        let mut cached = self.app_token.lock().await;
        if let Some(tok) = cached.as_ref().filter(|t| t.valid()) {
            return Ok(tok.access_token.clone());
        }
        let resp = self
            .token_request(&[("grant_type", "client_credentials"), ("scope", PRODUCT_SCOPE)])
            .await?;
        let tok = resp.cached();
        let access = tok.access_token.clone();
        *cached = Some(tok);
        Ok(access)
    }

    // ------------------------------------------------------------------ //
    // authorization_code — cart (the family account)
    // ------------------------------------------------------------------ //
    pub async fn user_token(&self) -> Result<String> {
        let mut cached = self.user_token.lock().await;
        if let Some(tok) = cached.as_ref().filter(|t| t.valid()) {
            return Ok(tok.access_token.clone());
        }
        let stored = self.load_stored().ok_or_else(|| {
            Error::Config(
                "No Kroger cart authorization. Run `kroger-cart-server --login` once \
                 to save a refresh token."
                    .into(),
            )
        })?;
        let resp = self
            .token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &stored.refresh_token),
            ])
            .await?;
        // Kroger may rotate the refresh token; persist whichever is current.
        self.persist(resp.refresh_token.as_deref().unwrap_or(&stored.refresh_token))?;
        let tok = resp.cached();
        let access = tok.access_token.clone();
        *cached = Some(tok);
        Ok(access)
    }

    // ------------------------------------------------------------------ //
    // one-time interactive login (browser + loopback redirect capture)
    // ------------------------------------------------------------------ //
    pub async fn login(&self) -> Result<()> {
        let state = format!("{:032x}", rand::random::<u128>());
        let url = Url::parse_with_params(
            AUTHORIZE_URL,
            &[
                ("scope", CART_SCOPE),
                ("response_type", "code"),
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("state", state.as_str()),
            ],
        )
        .context("building authorize URL")?;

        println!("Open this URL in a browser to authorize your Kroger account:\n\n  {url}\n");
        let code = capture_redirect_code(&self.redirect_uri, &state).await?;

        let resp = self
            .token_request(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", &self.redirect_uri),
            ])
            .await?;
        let refresh = resp
            .refresh_token
            .ok_or_else(|| anyhow!("token response had no refresh_token"))?;
        self.persist(&refresh)?;
        println!("Authorized. Refresh token saved — you won't need to log in again.");
        Ok(())
    }

    // ------------------------------------------------------------------ //
    // token store
    // ------------------------------------------------------------------ //
    fn persist(&self, refresh_token: &str) -> Result<()> {
        if let Some(parent) = self.token_store.parent() {
            std::fs::create_dir_all(parent).context("creating token store dir")?;
        }
        let stored = StoredToken {
            refresh_token: refresh_token.to_string(),
            saved_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        };
        std::fs::write(
            &self.token_store,
            serde_json::to_string_pretty(&stored).context("serializing token")?,
        )
        .context("writing token store")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.token_store, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    fn load_stored(&self) -> Option<StoredToken> {
        let raw = std::fs::read_to_string(&self.token_store).ok()?;
        let stored: StoredToken = serde_json::from_str(&raw).ok()?;
        (!stored.refresh_token.is_empty()).then_some(stored)
    }
}

fn require_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::Config(format!("{name} is not set")))
}

fn token_store_path() -> PathBuf {
    let raw = std::env::var("KROGER_TOKEN_STORE").unwrap_or_else(|_| "~/.kroger/token.json".into());
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(raw)
}

// ---------------------------------------------------------------------- //
// Tiny one-shot loopback server to catch the OAuth redirect.
// ---------------------------------------------------------------------- //
async fn capture_redirect_code(redirect_uri: &str, expected_state: &str) -> Result<String> {
    let parsed = Url::parse(redirect_uri).context("parsing redirect URI")?;
    let port = parsed.port().unwrap_or(8088);
    // Bind all interfaces so the redirect also reaches us inside a container.
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("binding loopback listener on port {port}"))?;

    let deadline = Duration::from_secs(300);
    let capture = async {
        loop {
            let (mut stream, _) = listener.accept().await.context("accepting redirect")?;
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.context("reading redirect")?;
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
            let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            let code = params.iter().find(|(k, _)| k == "code").map(|(_, v)| v.clone());
            let state = params.iter().find(|(k, _)| k == "state").map(|(_, v)| v.clone());

            let (status, body) = if code.is_some() {
                ("200 OK", "Authorized. You can close this tab.")
            } else {
                ("400 Bad Request", "No authorization code in redirect.")
            };
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;

            // Browsers often probe /favicon.ico — keep listening until the
            // actual callback arrives.
            if let Some(code) = code {
                if state.as_deref() != Some(expected_state) {
                    return Err(anyhow!("OAuth state mismatch — possible CSRF, aborting.").into());
                }
                return Ok::<String, Error>(code);
            }
        }
    };

    tokio::time::timeout(deadline, capture)
        .await
        .map_err(|_| Error::Other(anyhow!("Timed out waiting for Kroger authorization redirect.")))?
}
