use thiserror::Error;

#[derive(Debug, Error)]
pub enum CalDavError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    #[error("xml parse error: {0}")]
    Xml(#[from] roxmltree::Error),

    #[error("ical parse error: {0}")]
    Ical(String),

    #[error("authentication rejected by {url}")]
    Unauthorized { url: String },

    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("etag mismatch (precondition failed) on {0}")]
    PreconditionFailed(String),

    #[error("{method} {url} failed with {status}: {body}")]
    Status {
        method: String,
        url: String,
        status: u16,
        body: String,
    },

    #[error("server response is missing {0}")]
    MissingField(&'static str),
}

pub type Result<T> = std::result::Result<T, CalDavError>;
