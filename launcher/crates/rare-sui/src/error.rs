//! One error type for every read: gRPC status, HTTP, decode and input errors.

use std::fmt;

#[derive(Debug, Clone)]
pub enum Error {
    /// The fullnode answered with a gRPC error (`code` is the gRPC status code, 5 = not found).
    Grpc { code: i32, message: String },
    /// An HTTP API (rare.market, the Trade API) failed or answered non-2xx.
    Http { status: Option<u16>, message: String },
    /// The answer did not have the expected shape.
    Decode(String),
    /// The caller passed something unusable (bad address, bad type tag).
    Invalid(String),
    /// Transaction building failed.
    Build(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// gRPC status codes this crate looks at.
pub const GRPC_UNKNOWN: i32 = 2;
pub const GRPC_NOT_FOUND: i32 = 5;

impl Error {
    pub fn decode(m: impl Into<String>) -> Self {
        Error::Decode(m.into())
    }
    pub fn invalid(m: impl Into<String>) -> Self {
        Error::Invalid(m.into())
    }
    pub fn http(status: Option<u16>, m: impl Into<String>) -> Self {
        Error::Http { status, message: m.into() }
    }
    /// The node does not have it (pruned transaction, deleted object, no such field).
    pub fn is_not_found(&self) -> bool {
        match self {
            Error::Grpc { code, message } => *code == GRPC_NOT_FOUND || message.to_lowercase().contains("not found"),
            Error::Http { status, .. } => *status == Some(404),
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Grpc { code, message } => write!(f, "sui grpc {code}: {message}"),
            Error::Http { status: Some(s), message } => write!(f, "http {s}: {message}"),
            Error::Http { status: None, message } => write!(f, "http: {message}"),
            Error::Decode(m) => write!(f, "decode: {m}"),
            Error::Invalid(m) => write!(f, "invalid: {m}"),
            Error::Build(m) => write!(f, "build: {m}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(feature = "chain")]
impl From<tonic::Status> for Error {
    fn from(s: tonic::Status) -> Self {
        Error::Grpc { code: s.code() as i32, message: s.message().to_string() }
    }
}

#[cfg(feature = "chain")]
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http { status: e.status().map(|s| s.as_u16()), message: e.to_string() }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Decode(e.to_string())
    }
}

#[cfg(feature = "chain")]
impl From<sui_transaction_builder::Error> for Error {
    fn from(e: sui_transaction_builder::Error) -> Self {
        Error::Build(e.to_string())
    }
}
