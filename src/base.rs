/*!
The base ByteChunker types.
*/
use std::{
    fmt::{Debug, Formatter},
    hint::spin_loop,
    io::{ErrorKind, Read},
};

use regex::bytes::Regex;

use crate::{ctrl::*, CustomChunker, RcErr};

// By default the `read_buffer` size is 1 KiB.
const DEFAULT_BUFFER_SIZE: usize = 1024;

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