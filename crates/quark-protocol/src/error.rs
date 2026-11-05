use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(reqwest::Error),

    #[error("invalid response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("quark api error {status}/{code}: {message}")]
    Api {
        status: i64,
        code: i64,
        message: String,
        retryable: bool,
    },

    #[error("login session is missing or expired")]
    AuthRequired,

    #[error("invalid cookie: {0}")]
    InvalidCookie(String),

    #[error("upload session expired")]
    UploadExpired,

    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // Strip the request URL: signed download URLs and QR tokens can appear
        // in query strings, and this error is surfaced to logs and the UI.
        Self::Http(error.without_url())
    }
}

impl Error {
    pub fn api(status: i64, code: i64, message: impl Into<String>) -> Self {
        let message = message.into();
        let retryable = status == 429 || status >= 500 || is_retryable_code(code);
        Self::Api {
            status,
            code,
            message,
            retryable,
        }
    }

    pub fn is_auth(&self) -> bool {
        match self {
            Self::AuthRequired => true,
            Self::Api {
                status,
                code,
                message,
                ..
            } => {
                *status == 401
                    || *status == 403 && message.to_ascii_lowercase().contains("login")
                    || matches!(*code, 11001 | 14001 | 31001 | 31002 | 31003)
            }
            _ => false,
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Api { retryable, .. } => *retryable,
            Self::Http(err) => err.is_timeout() || err.is_connect() || err.is_request(),
            Self::Io(_) => true,
            _ => false,
        }
    }

    pub fn is_expired_upload(&self) -> bool {
        matches!(self, Self::UploadExpired)
            || matches!(
                self,
                Self::Api {
                    status: 403 | 404 | 410,
                    ..
                }
            )
    }
}

fn is_retryable_code(code: i64) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn network_errors_do_not_expose_request_urls() {
        let error = reqwest::Client::new()
            .get("http://127.0.0.1:1/private?ticket=secret")
            .send()
            .await
            .expect_err("connection should fail");
        let error: Error = error.into();
        assert!(!error.to_string().contains("ticket=secret"));
    }
}
