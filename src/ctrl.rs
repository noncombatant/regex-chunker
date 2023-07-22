/*!
A bunch of enums that control the behavior of chunkers.
*/
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ErrorStatus {
    Ok,
    Errored,
    Continue,
    Ignore,
}
impl Eq for ErrorStatus {}

/// Type for specifying a Chunker's behavior upon encountering an error.
#[derive(Clone, Copy, Debug)]
pub enum ErrorResponse {
    /// Return `Some(Err(error))` once then None thereafter. This is
    /// the default behavior.
    Halt,
    /// Return `Some(Err(error))` but attempt to recover and continue.
    /// This may result in an endless stream of errors.
    Continue,
    /// Attempt to recover and continue until it's possible to return
    /// another `Some(Ok())`. This may result in a deadlock.
    Ignore,
}

/// Specify what the chunker should do with the matched text.
#[derive(Clone, Copy, Debug, Default)]
pub enum MatchDisposition {
    /// Discard the matched text; only return the text
    /// _between_ matches. This is the default behavior.
    #[default]
    Drop,
    /// Treat the matched text like the end of the preceding chunk.
    Append,
    /// Treat the matched text like the beginning of the
    /// following chunk.
    Prepend,
}

/// Type for specifying a [`StringAdapter`](crate::StringAdapter)'s
/// behavior upon encountering non-UTF-8 data.
#[derive(Clone, Copy, Debug, Default)]
pub enum Utf8FailureMode {
    /// Lossily convert to UTF-8 (with
    /// [`String::from_utf8_lossy`](std::string::String::from_utf8_lossy)).
    Lossy,
    /// Report an error and stop reading (return `Some(Err(RcErr))` once
    /// and then `None` thereafter.
    #[default]
    Fatal,
    /// Report an error but attempt to continue (keep returning
    /// `Some(Err(RcErr))` until the it starts reading UTF-8 from the
    /// `source` again.
    Continue,
}