#![feature(doc_cfg)]

/*!
The centerpiece of this crate is the [`ByteChunker`], which takes a regular
expression and wraps a [`Read`] type, becoming an iterator over the bytes
read from the wrapped type, yielding chunks delimited by the supplied
regular expression.

The example program below uses a `ByteChunker` to do a crude word
tally on text coming in on the standard input.

```rust
use std::{collections::BTreeMap, error::Error};
use regex_chunker::ByteChunker;
  
fn main() -> Result<(), Box<dyn Error>> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let stdin = std::io::stdin();
    
    // The regex is a stab at something matching strings of
    // "between-word" characters in general English text.
    let chunker = ByteChunker::new(stdin, r#"[ "\r\n.,!?:;/]+"#)?;
    for chunk in chunker {
        let word = String::from_utf8_lossy(&chunk?).to_lowercase();
        *counts.entry(word).or_default() += 1;
    }

    println!("{:#?}", &counts);
    Ok(())
}
```

Enabling the `async` feature also exposes the [`stream`] module, which
features an async version of the `ByteChunker`, wrapping an
[`AsyncRead`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html)
and implementing
[`Stream`](https://docs.rs/futures-core/0.3.28/futures_core/stream/trait.Stream.html).

(This also pulls in several crates of
[`tokio`](https://docs.rs/tokio/latest/tokio/index.html) machinery, which is why
it's behind a feature flag.)
*/

mod adapter;
pub use adapter::*;
pub mod err;
#[cfg(any(feature = "async", doc))]
#[doc(cfg(feature = "async"))]
pub mod stream;

pub(crate) mod stream_adapter;

use std::{
    fmt::{Debug, Formatter},
    hint::spin_loop,
    io::{ErrorKind, Read},
};

use regex::bytes::Regex;

pub use crate::err::RcErr;

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
            source,
            fence,
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
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.read_buff.resize(size, 0);
        self.read_buff.shrink_to_fit();
        self
    }

    /**
    Builder-pattern method for controlling how the chunker behaves when
    encountering an error in the course of its operation. Default value
    is [`ErrorResponse::Halt`].
     */
    pub fn on_error(mut self, response: ErrorResponse) -> Self {
        self.error_status = match response {
            ErrorResponse::Halt => {
                if self.error_status != ErrorStatus::Errored {
                    ErrorStatus::Ok
                } else {
                    ErrorStatus::Errored
                }
            }
            ErrorResponse::Continue => ErrorStatus::Continue,
            ErrorResponse::Ignore => ErrorStatus::Ignore,
        };
        self
    }

    /**
    Builder-pattern method for controlling what the chunker does with the
    matched text. Default value is [`MatchDisposition::Drop`].
     */
    pub fn with_match(mut self, behavior: MatchDisposition) -> Self {
        self.match_dispo = behavior;
        if matches!(behavior, MatchDisposition::Drop | MatchDisposition::Append) {
            // If we swtich to one of these two dispositions, we
            // need to be sure we reset the scan_start_offset, or
            // else we'll never scan the beginning of our buffer.
            self.scan_start_offset = 0;
        }
        self
    }

    /**
    Consumes the [`ByteChunker`] and returns its wrapped `Read`er.
    The `ByteChunker` may have read some data from its source that may not
    yet have been returned or successfully matched; this data may be lost.
    To retrieve that data, see [`ByteChunker::into_innards`].
    */
    pub fn into_inner(self) -> R {
        self.source
    }

    /**
    Consumes the [`ByteChunker`] and returns its wrapped `Read`er, as well
    as any not-yet-processed data that has been read. If this unprocessed
    data is unimportant, and you just want the reader back, use the more
    traditional [`ByteChunker::into_inner`].
    */
    pub fn into_innards(self) -> (R, Vec<u8>) {
        (self.source, self.search_buff)
    }

    /**
    Creates a [`CustomChunker`] by combining this `ByteChunker` with an
    `Adapter` type.
    */
    pub fn with_adapter<A>(self, adapter: A) -> CustomChunker<R, A> {
        CustomChunker {
            chunker: self,
            adapter,
        }
    }

    /*
    Search the search_buffer for a match; if found, return the next chunk
    of bytes to be returned from ]`Iterator::next`].
    */
    fn scan_buffer(&mut self) -> Option<Vec<u8>> {
        let (start, end) = match self
            .fence
            .find_at(&self.search_buff, self.scan_start_offset)
        {
            Some(m) => {
                self.last_scan_matched = true;
                (m.start(), m.end())
            }
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
            }
            MatchDisposition::Append => {
                new_buff = self.search_buff.split_off(end);
            }
            MatchDisposition::Prepend => {
                new_buff = self.search_buff.split_off(start);
                self.scan_start_offset = end - start;
            }
        }

        std::mem::swap(&mut new_buff, &mut self.search_buff);
        Some(new_buff)
    }

    // Function for wrapping types that need this information.
    #[allow(dead_code)]
    #[inline(always)]
    fn buff_size(&self) -> usize {
        return self.read_buff.len();
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

/**
The [`ByteChunker`] specifically doesn't supply an implementation of
[`Iterator::size_hint`] because, in general, it's impossible to tell
how much data is left in a reader.
*/
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
                        ErrorKind::WouldBlock | ErrorKind::Interrupted => {
                            spin_loop();
                            continue;
                        }
                        _ => match self.error_status {
                            ErrorStatus::Ok | ErrorStatus::Errored => {
                                self.error_status = ErrorStatus::Errored;
                                return Some(Err(e.into()));
                            }
                            ErrorStatus::Continue => {
                                return Some(Err(e.into()));
                            }
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
                    }
                    Ok(n) => {
                        self.search_buff.extend_from_slice(&self.read_buff[..n]);
                        match self.scan_buffer() {
                            Some(v) => return Some(Ok(v)),
                            None => {
                                spin_loop();
                                continue;
                            }
                        }
                    }
                }
            } else {
                match self.scan_buffer() {
                    Some(v) => return Some(Ok(v)),
                    None => {
                        spin_loop();
                        continue;
                    }
                }
            }
        }
    }
}

/// Type for specifying a [`StringChunker`]'s behavior upon encountering
/// non-UTF-8 data.
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Utf8ErrorStatus {
    #[default]
    Ok,
    Errored,
    Lossy,
    Continue,
}
impl Eq for Utf8ErrorStatus {}

#[derive(Debug)]
struct StringAdapter {
    status: Utf8ErrorStatus,
}

impl Adapter for StringAdapter {
    type Item = Result<String, RcErr>;

    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item> {
        match (self.status, v) {
            (Utf8ErrorStatus::Errored, _) => None,
            (_, None) => None,
            (_, Some(Err(e))) => Some(Err(e)),
            (Utf8ErrorStatus::Lossy, Some(Ok(v))) =>
                Some(Ok(String::from_utf8_lossy(&v).into())),
            (Utf8ErrorStatus::Ok, Some(Ok(v))) => match String::from_utf8(v) {
                Ok(s) => Some(Ok(s)),
                Err(e) => {
                    self.status = Utf8ErrorStatus::Errored;
                    Some(Err(e.into()))
                },
            },
            (Utf8ErrorStatus::Continue, Some(Ok(v))) => match String::from_utf8(v) {
                Ok(s) => Some(Ok(s)),
                Err(e) => Some(Err(e.into())),
            }
        }
    }
}

/**
The `StringChunker` operates like the [`ByteChunker`], except instead of
returning `Vec<u8>`s, it returns [`String`]s. It also has an extra parameter
of operation, [`Utf8FailureMode`], which controls how it reacts when
reading data that is not valid UTF-8.

```rust
use regex_chunker::StringChunker;
use std::io::Cursor;

# fn main() -> Result<(), regex_chunker::RcErr> {
let text = b"One, two, three, four. Can I have a little more?";
let c = Cursor::new(text);

let chunks: Vec<String> = StringChunker::new(c, "[ .,?]+")?
    .map(|res| res.unwrap())
    .collect();

assert_eq!(
    &chunks,
    &["One", "two", "three", "four",
    "Can", "I", "have", "a", "little", "more"].clone()
);
# Ok(())
# }
```

Several of the builder-pattern methods function identically to their
[`ByteChunker`] counterparts.

*/
pub struct StringChunker<R> {
    chunker: CustomChunker<R, StringAdapter>,
}

impl<R> StringChunker<R> {
    pub fn new(source: R, delimiter: &str) -> Result<StringChunker<R>, RcErr> {
        Ok(StringChunker {
            chunker: ByteChunker::new(source, delimiter)?
                .with_adapter(StringAdapter { status: Utf8ErrorStatus::default() })
        })
    }

    pub fn with_buffer_size(self, size: usize) -> Self {
        let (mut c, a) = self.chunker.into_innards();
        c = c.with_buffer_size(size);
        StringChunker { chunker: c.with_adapter(a) }
    }

    pub fn on_error(self, response: ErrorResponse) -> Self {
        let (mut c, a) = self.chunker.into_innards();
        c = c.on_error(response);
        StringChunker { chunker: c.with_adapter(a) }
    }

    pub fn with_match(self, behavior: MatchDisposition) -> Self {
        let (mut c, a) = self.chunker.into_innards();
        c = c.with_match(behavior);
        StringChunker { chunker: c.with_adapter(a) }
    }

    /**
    Builder-pattern method for controlling how the chunker responds when
    encountering non-UTF-8 data. The default value is
    [`Utf8FailureMode::Fatal`].
     */
    pub fn on_utf8_error(self, response: Utf8FailureMode) -> Self {
        let (c, mut adapter) = self.chunker.into_innards();
        adapter.status = match response {
            Utf8FailureMode::Fatal => {
                if adapter.status != Utf8ErrorStatus::Errored {
                    Utf8ErrorStatus::Ok
                } else {
                    Utf8ErrorStatus::Errored
                }
            },
            Utf8FailureMode::Lossy => Utf8ErrorStatus::Lossy,
            Utf8FailureMode::Continue => Utf8ErrorStatus::Continue,
        };
        StringChunker { chunker: c.with_adapter(adapter) }
    }
}

impl<R: Read> Iterator for StringChunker<R> {
    type Item = Result<String, RcErr>;

    fn next(&mut self) -> Option<Self::Item> {
        self.chunker.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        fs::File,
        io::{Cursor, Write},
    };

    pub static TEST_PATH: &str = "test/cessen_issue.txt";
    pub static TEST_PATT: &str = r#"[A-Z]"#;
    pub static PASSWD_PATH: &str = "test/passwd.txt";
    pub static PASSWD_PATT: &str = r#"[:\r\n]+"#;
    pub static HTTP_URL: &str = "https://www.zombo.com";
    pub static HTTP_PATT: &str = r#">[^<]*"#;

    pub fn chunk_vec<'a>(re: &Regex, v: &'a [u8], mode: MatchDisposition) -> Vec<&'a [u8]> {
        let mut u: Vec<&[u8]> = Vec::new();
        let mut offs: usize = 0;
        let mut prev_offs: usize = 0;
        while let Some(m) = re.find_at(v, offs) {
            let (start, end) = match mode {
                MatchDisposition::Drop => {
                    let start = offs;
                    offs = m.end();
                    (start, m.start())
                }
                MatchDisposition::Append => {
                    let start = offs;
                    offs = m.end();
                    (start, m.end())
                }
                MatchDisposition::Prepend => {
                    let start = prev_offs;
                    offs = m.end();
                    prev_offs = m.start();
                    (start, m.start())
                }
            };

            u.push(&v[start..end]);
        }

        match mode {
            MatchDisposition::Drop | MatchDisposition::Append => {
                u.push(&v[offs..]);
            }
            MatchDisposition::Prepend => {
                u.push(&v[prev_offs..]);
            }
        }

        u
    }

    pub fn ref_slice_cmp<R, S>(a: &[R], b: &[S])
    where
        R: AsRef<[u8]> + Debug,
        S: AsRef<[u8]> + Debug,
    {
        for (aref, bref) in a.iter().zip(b.iter()) {
            assert_eq!(
                aref.as_ref(),
                bref.as_ref(),
                "left: {:?}\nright: {:?}\n",
                &String::from_utf8_lossy(aref.as_ref()),
                &String::from_utf8_lossy(bref.as_ref())
            );
        }
    }

    #[test]
    fn basic_bytes() {
        let byte_vec = std::fs::read(TEST_PATH).unwrap();
        let re = Regex::new(TEST_PATT).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec, MatchDisposition::Drop);

        let f = File::open(TEST_PATH).unwrap();
        let chunker = ByteChunker::new(f, TEST_PATT).unwrap();
        let vec_vec: Vec<Vec<u8>> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn bytes_append_prepend() {
        let byte_vec = std::fs::read(PASSWD_PATH).unwrap();
        let re = Regex::new(PASSWD_PATT).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec, MatchDisposition::Append);

        let vec_vec: Vec<Vec<u8>> = ByteChunker::new(File::open(PASSWD_PATH).unwrap(), PASSWD_PATT)
            .unwrap()
            .with_match(MatchDisposition::Append)
            .map(|res| res.unwrap())
            .collect();

        ref_slice_cmp(&vec_vec, &slice_vec);

        let slice_vec = chunk_vec(&re, &byte_vec, MatchDisposition::Prepend);

        let vec_vec: Vec<Vec<u8>> = ByteChunker::new(File::open(PASSWD_PATH).unwrap(), PASSWD_PATT)
            .unwrap()
            .with_match(MatchDisposition::Prepend)
            .map(|res| res.unwrap())
            .collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn bytes_http_request() {
        use reqwest::blocking::Client;

        let re = Regex::new(HTTP_PATT).unwrap();
        let client = Client::new();
        let re_response = client.get(HTTP_URL).send().unwrap().bytes().unwrap();
        let slice_vec = chunk_vec(&re, &re_response, MatchDisposition::Drop);

        let ch_response = client.get(HTTP_URL).send().unwrap();
        let chunker = ByteChunker::new(ch_response, HTTP_PATT).unwrap();
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
            let mut dest = File::create(file_path).unwrap();
            dest.write_all(&buff).unwrap();
            dest.flush().unwrap();
            buff
        };

        let re = Regex::new(re_text).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec, MatchDisposition::Drop);

        let f = File::open(file_path).unwrap();
        let chunker = ByteChunker::new(f, re_text).unwrap();
        let vec_vec: Vec<Vec<u8>> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn basic_string() {
        let byte_vec = std::fs::read(TEST_PATH).unwrap();
        let re = Regex::new(TEST_PATT).unwrap();
        let slice_vec = chunk_vec(&re, &byte_vec, MatchDisposition::Drop);

        let f = File::open(TEST_PATH).unwrap();
        let chunker = StringChunker::new(f, TEST_PATT).unwrap();
        let vec_vec: Vec<String> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn string_utf8_error() {
        let bytes: &[u8] = &[130, 15];
        let mut chunker = StringChunker::new(Cursor::new(bytes), TEST_PATT).unwrap();
        assert!(matches!(chunker.next(), Some(Err(RcErr::Utf8(_)))));

        let bytes = b"test one two";
        let mut chunker = StringChunker::new(Cursor::new(bytes), TEST_PATT).unwrap();
        assert!(matches!(chunker.next(), Some(Ok(_))));
    }
}
