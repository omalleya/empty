#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Misconfiguration the caller can fix (missing env, no login yet).
    #[error("{0}")]
    Config(String),

    /// Kroger replied with a non-2xx status.
    #[error("Kroger API error ({status})")]
    KrogerHttp { status: u16 },

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Like requests' raise_for_status, but keeps the status code around so the
/// server layer can map 401/403 on the cart call to a friendly message.
pub async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        Err(Error::KrogerHttp {
            status: resp.status().as_u16(),
        })
    }
}
