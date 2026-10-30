use std::fmt;

/// An error with a message fit to show the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub fn new(msg: impl Into<String>) -> Error {
        Error(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(s: String) -> Error {
        Error(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Error {
        Error(s.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error(e.to_string())
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.0
    }
}

/// The wire form is the bare message, as the `Result<_, String>` commands had.
impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
