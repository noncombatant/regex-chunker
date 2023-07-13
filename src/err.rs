/**!
Error types.
*/
use std::{
    error::Error,
    fmt::Display,
};

#[derive(Debug)]
pub enum RcErr {
    /// Error returned during creation of a regex.
    Regex(regex::Error),
    /// Error returned during reading from a *Chunker's source.
    Read(std::io::Error),
}

impl Display for RcErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RcErr::Regex(e) => write!(f, "regex error: {}", &e),
            RcErr::Read(e) => write!(f, "read error: {}", &e),
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

impl Error  for RcErr {
    fn source<'a>(&'a self) -> Option<&(dyn Error + 'static)> {
        match self {
            RcErr::Regex(e) => Some(e),
            RcErr::Read(e) => Some(e),
        }
    }
}