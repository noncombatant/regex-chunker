pub mod err;

use std::{
    fmt::{Debug, Formatter},
    hint::spin_loop,
    io::{ErrorKind, Read},
};

use regex::bytes::Regex;

pub use crate::err::RcErr;

//const DEBUG: bool = true;

// By default the `read_buffer` size is 1 KiB.
const DEFAULT_BUFFER_SIZE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
enum ErrorStatus {
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

/**
The `ByteChunker` takes a
[`bytes::Regex`](https://docs.rs/regex/latest/regex/bytes/struct.Regex.html),
wraps a byte source (that is, a type that implements [`std::io::Read`])
and iterates over chunks of bytes from that source that are delimited by
the regular expression. It operates very much like
[`bytes::Regex::split`](https://docs.rs/regex/latest/regex/bytes/struct.Regex.html#method.split),
except that it works on an incoming stream of bytes instead of a
necessarily-already-in-memory slice.

```
use regex_chunker::ByteChunker;
use std::io::Cursor;

# fn main() -> Result<(), regex_chunker::RcErr> {
let text = b"One, two, three, four. Can I have a little more?";
let c = Cursor::new(text);

let chunks: Vec<String> = ByteChunker::new(c, "[ .,?]+")?
    .map(|res| {
        let v = res.unwrap();
        String::from_utf8(v).unwrap()
    }).collect();

assert_eq!(
    &chunks,
    &["One", "two", "three", "four",
    "Can", "I", "have", "a", "little", "more"].clone()
);
# Ok(())
# }
```

It's also slightly more flexible, in that the the matched bytes can be
optionally added to the beginnings or endings of the returned chunks.
(By default they are just dropped.)

```
use regex_chunker::{ByteChunker, MatchDisposition};
use std::io::Cursor;

# fn main() -> Result<(), regex_chunker::RcErr> {
let text = b"One, two, three, four. Can I have a little more?";
let c = Cursor::new(text);

let chunks: Vec<String> = ByteChunker::new(c, "[ .,?]+")?
    .with_match(MatchDisposition::Append)
    .map(|res| {
        let v = res.unwrap();
        String::from_utf8(v).unwrap()
    }).collect();

assert_eq!(
    &chunks,
    &["One, ", "two, ", "three, ", "four. ",
    "Can ", "I ", "have ", "a ", "little ", "more?"].clone()
);

# Ok(())
# }

*/
pub struct ByteChunker<R> {
    source: R,
    fence: Regex,
    read_buff: Vec<u8>,
    search_buff: Vec<u8>,
    error_status: ErrorStatus,
    match_dispo: MatchDisposition,
    /* Whether the last search of the search buffer found a match. If it did,
    then the next call to `.next()` should start by searching the search
    buffer again; otherwise we should start by trying to pull more bytes
    from our source. */
    last_scan_matched: bool,
    /* If the MatchDisposition is Prepend, we need to keep the match in the
    scan buffer so we can return it with the next chunk. This means we need
    to start our next scan of the buffer from _after_ the match, or we'll
    just match the very beginning of the scan buffer again. */
    scan_start_offset: usize,
}

impl<R> ByteChunker<R> {
    /**
    Return a new [`ByteChunker`] wrapping the given writer that will chunk its
    output by delimiting it with the supplied regex pattern.
    */
    pub fn new(source: R, delimiter: &str) -> Result<Self, RcErr> {
        let fence = Regex::new(delimiter)?;
        Ok(Self {
            source, fence,
            read_buff: vec![0u8; DEFAULT_BUFFER_SIZE],
            search_buff: Vec::new(),
            error_status: ErrorStatus::Ok,
            match_dispo: MatchDisposition::default(),
            last_scan_matched: false,
            scan_start_offset: 0,
        })
    }

    /**
    Builder-pattern method for setting the read buffer size.
    Default size is 1024 bytes.
     */
    pub fn with_buffer_size(&mut self, size: usize) -> &mut Self {
        self.read_buff.resize(size, 0);
        self.read_buff.shrink_to_fit();
        self
    }

    /**
    Builder-pattern method for controlling how the chunker behaves when
    encountering an error in the course of its operation. Default value
    is [`ErrorResponse::Halt`].
     */
    pub fn on_error(&mut self, response: ErrorResponse) -> &mut Self {
        self.error_status = match response {
            ErrorResponse::Halt => if self.error_status != ErrorStatus::Errored {
                ErrorStatus::Ok
            } else {
                ErrorStatus::Errored
            },
            ErrorResponse::Continue => ErrorStatus::Continue,
            ErrorResponse::Ignore => ErrorStatus::Ignore,
        };
        self
    }

    /**
    Builder-pattern method for controlling what the chunker does with the
    matched text. Default value is [`MatchDisposition::Drop`].
     */
    pub fn with_match(&mut self, behavior: MatchDisposition) -> &mut Self {
        self.match_dispo = behavior;
        if matches!(
            behavior,
            MatchDisposition::Drop | MatchDisposition::Append
        ) {
            // If we swtich to one of these two dispositions, we
            // need to be sure we reset the scan_start_offset, or
            // else we'll never scan the beginning of our buffer.
            self.scan_start_offset = 0;
        }
        self
    }

    /*
    Search the search_buffer for a match; if found, return the next chunk
    of bytes to be returned from ]`Iterator::next`].
    */
    fn scan_buffer(&mut self) -> Option<Vec<u8>> {
        let (start, end) = match self.fence.find_at(
            &self.search_buff, self.scan_start_offset
        ) {
            Some(m) => {
                self.last_scan_matched = true;
                (m.start(), m.end())
            },
            None => {
                self.last_scan_matched = false;
                return None;
            }
        };

        let mut new_buff;
        match self.match_dispo {
            MatchDisposition::Drop => {
                new_buff = self.search_buff.split_off(end);
                self.search_buff.resize(start, 0);
            },
            MatchDisposition::Append => {
                new_buff = self.search_buff.split_off(end);
            },
            MatchDisposition::Prepend => {
                new_buff = self.search_buff.split_off(start);
                self.scan_start_offset = end - start;
            }
        }

        std::mem::swap(&mut new_buff, &mut self.search_buff);
        Some(new_buff)
    }
}

impl<R> Debug for ByteChunker<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ByteChunker")
            .field("source", &std::any::type_name::<R>())
            .field("fence", &self.fence)
            .field("read_buff", &String::from_utf8_lossy(&self.read_buff))
            .field("search_buff", &String::from_utf8_lossy(&self.search_buff))
            .field("error_status", &self.error_status)
            .field("match_dispo", &self.match_dispo)
            .field("last_scan_matched", &self.last_scan_matched)
            .field("scan_start_offset", &self.scan_start_offset)
            .finish()
    }
}

impl<R: Read> Iterator for ByteChunker<R> {
    type Item = Result<Vec<u8>, RcErr>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.error_status == ErrorStatus::Errored {
            return None;
        }

        loop {
            if !self.last_scan_matched {
                match self.source.read(&mut self.read_buff) {
                    Err(e) => match e.kind() {
                        ErrorKind::WouldBlock |
                        ErrorKind::Interrupted => {
                            spin_loop();
                            continue;
                        },
                        _ => match self.error_status {
                            ErrorStatus::Ok |
                            ErrorStatus::Errored => {
                                self.error_status = ErrorStatus::Errored;
                                return Some(Err(e.into()));
                            },
                            ErrorStatus::Continue => {
                                return Some(Err(e.into()));
                            },
                            ErrorStatus::Ignore => {
                                continue;
                            }
                        },
                    },
                    Ok(0) => {
                        if self.search_buff.is_empty() {
                            return None;
                        } else {
                            let mut new_buff: Vec<u8> = Vec::new();
                            std::mem::swap(&mut self.search_buff, &mut new_buff);
                            return Some(Ok(new_buff));
                        }
                    },
                    Ok(n) => {
                        self.search_buff.extend_from_slice(&self.read_buff[..n]);
                        match self.scan_buffer() {
                            Some(v) => return Some(Ok(v)),
                            None => {
                                spin_loop();
                                continue;
                            }
                        }
                    },
                }
            } else {
                match self.scan_buffer() {
                    Some(v) => return Some(Ok(v)),
                    None => {
                        spin_loop();
                        continue;
                    },
                }
            }
        }
    }
}

/// Type for specifying a `StringChunker`'s behavior upon encountering
/// non-UTF-8 data.
#[derive(Clone, Copy, Debug)]
pub enum Utf8FailureMode {
    /// Lossily convert to UTF-8 (with
    /// [`std::string::String::from_utf8_lossy`]).
    Lossy,
    /// Report an error and stop reading (return `Some(Err(RcErr))` once
    /// and then `None` thereafter.
    Fatal,
    /// Report an error but attempt to continue (keep returning
    /// `Some(Err(RcErr))` until the it starts reading UTF-8 from the
    /// `source` again.
    Continue,
    /// Ignore the error and continue trying to read from the `source`
    /// until it encounters UTF-8 again.
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Utf8ErrorStatus {
    Ok,
    Errored,
    Lossy,
    Continue,
    Ignore,
}
impl Eq for Utf8ErrorStatus {}

#[derive(Debug)]
pub struct StringChunker<R> {
    chunker: ByteChunker<R>,
    utf8_error_status: Utf8ErrorStatus,
}

impl<R> StringChunker<R> {
    pub fn new(source: R, delimiter: &str) -> Result<Self, RcErr> {
        let chunker = ByteChunker::new(source, delimiter)?;
        Ok(Self {
            chunker,
            utf8_error_status: Utf8ErrorStatus::Ok,
        })
    }

    pub fn with_buffer_size(&mut self, size: usize) -> &mut Self {
        self.chunker.with_buffer_size(size);
        self
    }

    pub fn on_error(&mut self, response: ErrorResponse) -> &mut Self {
        self.chunker.on_error(response);
        self
    }

    pub fn with_match(&mut self, behavior: MatchDisposition) -> &mut Self {
        self.chunker.with_match(behavior);
        self
    }

    pub fn on_utf8_error(&mut self, response: Utf8FailureMode) -> &mut Self {
        self.utf8_error_status = match response {
            Utf8FailureMode::Fatal => {
                if self.utf8_error_status != Utf8ErrorStatus::Errored {
                    Utf8ErrorStatus::Ok
                } else {
                    Utf8ErrorStatus::Errored
                }
            },
            Utf8FailureMode::Lossy => Utf8ErrorStatus::Lossy,
            Utf8FailureMode::Continue => Utf8ErrorStatus::Continue,
            Utf8FailureMode::Ignore => Utf8ErrorStatus::Ignore,
        };
        self
    }
}

impl<R: Read> Iterator for StringChunker<R> {
    type Item = Result<String, RcErr>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.utf8_error_status == Utf8ErrorStatus::Errored {
            return None;
        }

        loop {
            let v = match self.chunker.next()? {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            match self.utf8_error_status {
                Utf8ErrorStatus::Errored => return None, // Shouldn't happen.
                Utf8ErrorStatus::Lossy => return Some(Ok(
                    String::from_utf8_lossy(&v).into()
                )),
                Utf8ErrorStatus::Ok => match String::from_utf8(v) {
                    Ok(s) => return Some(Ok(s)),
                    Err(e) => {
                        self.utf8_error_status = Utf8ErrorStatus::Errored;
                        return Some(Err(e.into()));
                    },
                },
                Utf8ErrorStatus::Continue => return Some(
                    String::from_utf8(v).map_err(|e| e.into())
                ),
                Utf8ErrorStatus::Ignore => match String::from_utf8(v) {
                    Ok(s) => return Some(Ok(s)),
                    Err(_) => continue,
                },
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        io::Write,
        fs::File,
    };

    static TEST_PATH: &str = "test/cessen_issue.txt";
    static TEST_PATT: &str = r#"[A-Z]"#;

    fn chunk_vec<'a>(re: &Regex, v: &'a [u8]) -> Vec<&'a [u8]> {
        re.split(v).collect()
    }

    fn ref_slice_cmp<T, R, S>(a: &[R], b: &[S])
    where
        T: PartialEq + Debug + ?Sized,
        R: AsRef<T> + Debug,
        S: AsRef<T> + Debug,
    {
        for (aref, bref) in a.iter().zip(b.iter()) {
            assert_eq!(aref.as_ref(), bref.as_ref());
        }
    }

    #[test]
    fn basic_bytes() {
        let byte_vec = std::fs::read(TEST_PATH).unwrap();
        let re = Regex::new(TEST_PATT).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec);

        let f = File::open(TEST_PATH).unwrap();
        let chunker = ByteChunker::new(f, TEST_PATT).unwrap();
        let vec_vec: Vec<Vec<u8>> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[cfg(unix)]
    #[test]
    fn random_bytes() {
        let re_text = r#"[0-9]"#;
        let source_path = "/dev/urandom";
        const N_BYTES: usize = 1024 * 1024;
        let file_path = "test/random.dat";

        let byte_vec = {
            let mut source = File::open(source_path).unwrap();
            let mut buff: Vec<u8> = vec![0; N_BYTES];
            source.read_exact(&mut buff).unwrap();
            let mut dest =  File::create(file_path).unwrap();
            dest.write_all(&buff).unwrap();
            dest.flush().unwrap();
            buff
        };

        let re = Regex::new(re_text).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec);

        let f = File::open(file_path).unwrap();
        let chunker = ByteChunker::new(f, re_text).unwrap();
        let vec_vec: Vec<Vec<u8>> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn basic_string() {
        let byte_vec = std::fs::read(TEST_PATH).unwrap();
        let re = Regex::new(TEST_PATT).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec);

        let f = File::open(TEST_PATH).unwrap();
        let chunker = StringChunker::new(f, TEST_PATT).unwrap();
        let vec_vec: Vec<String> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

}
