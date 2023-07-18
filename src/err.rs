/*!
Error types returned by the various chunkers.
*/
use std::{error::Error, fmt::Display, string::FromUtf8Error};

/**
Wraps various types of errors that can happen in the internals of a
Chunker. The way Chunkers respond to and report these errors can be
controlled through builder-pattern methods that take the
[`ErrorResponse`](crate::ErrorResponse) and
[`Utf8FailureMode`](crate::Utf8FailureMode) types.
*/
#[derive(Debug)]
pub enum RcErr {
    /// Error returned during creation of a regex.
    Regex(regex::Error),
    /// Error returned during reading from a `*Chunker`'s source.
    Read(std::io::Error),
    /// Error returned by [`StringChunker`](crate::StringChunker) upon encountering
    /// non-UTF-8 data.
    Utf8(FromUtf8Error),
}

impl Display for RcErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RcErr::Regex(e) => write!(f, "regex error: {}", &e),
            RcErr::Read(e) => write!(f, "read error: {}", &e),
            RcErr::Utf8(e) => write!(f, "UTF-8 decoding error: {}", &e),
        }
    }
}

impl From<regex::Error> for RcErr {
    fn from(e: regex::Error) -> Self {
        RcErr::Regex(e)
    }
}

impl From<std::io::Error> for RcErr {
    fn from(e: std::io::Error) -> Self {
        RcErr::Read(e)
    }
}

impl From<FromUtf8Error> for RcErr {
    fn from(e: FromUtf8Error) -> Self {
        RcErr::Utf8(e)
    }
}

impl Error for RcErr {
    fn source<'a>(&'a self) -> Option<&(dyn Error + 'static)> {
        match self {
            RcErr::Regex(e) => Some(e),
            RcErr::Read(e) => Some(e),
            RcErr::Utf8(e) => Some(e),
        }
    }
}
