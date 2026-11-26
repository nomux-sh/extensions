//! The failure of one method.

use std::fmt;

use nomux_extension::{CallError, ErrorCode};

/// The result of a method or of one of its steps.
pub type Result<T> = std::result::Result<T, Error>;

/// The failure of one method: a code of the extension protocol and a message
/// in English for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The reason for the failure.
    pub code: ErrorCode,
    /// A message in English for the user.
    pub message: String,
}

impl Error {
    /// The request has a value that the method cannot use.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    /// The user did not approve what the request needs.
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, message)
    }

    /// The item that the request names does not exist.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    /// The method failed, and no other code gives the reason.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Failed, message)
    }

    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<gix::Error> for Error {
    fn from(error: gix::Error) -> Self {
        Self::failed(error.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::failed(error.to_string())
    }
}

impl From<Error> for CallError {
    fn from(error: Error) -> Self {
        Self::new(error.code, error.message)
    }
}
