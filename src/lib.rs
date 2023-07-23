#![cfg_attr(docsrs, feature(doc_cfg))]

/*!
The centerpiece of this crate is the [`ByteChunker`], which takes a regular
expression and wraps a [`Read`](std::io::Read) type, becoming an iterator
over the bytes read from the wrapped type, yielding chunks delimited by
the supplied regular expression.

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

pub(crate) mod adapter;
pub use adapter::*;
mod base;
pub use base::*;
pub(crate) mod ctrl;
pub use ctrl::*;
mod custom;
pub use custom::*;
mod err;
pub use err::RcErr;
#[cfg(any(feature = "async", docsrs))]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub mod stream;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::{
        fmt::Debug,
        fs::File,
        io::{Cursor, Read, Write},
    };

    use regex::bytes::Regex;

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
        let chunker = ByteChunker::new(f, TEST_PATT)
            .unwrap()
            .with_adapter(StringAdapter::default());
        let vec_vec: Vec<String> = chunker.map(|res| res.unwrap()).collect();

        ref_slice_cmp(&vec_vec, &slice_vec);
    }

    #[test]
    fn string_utf8_error() {
        let bytes: &[u8] = &[130, 15];
        let mut chunker = ByteChunker::new(Cursor::new(bytes), TEST_PATT)
            .unwrap()
            .with_adapter(StringAdapter::default());
        assert!(matches!(chunker.next(), Some(Err(RcErr::Utf8(_)))));

        let bytes = b"test one two";
        let mut chunker = ByteChunker::new(Cursor::new(bytes), TEST_PATT)
            .unwrap()
            .with_adapter(StringAdapter::default());
        assert!(matches!(chunker.next(), Some(Ok(_))));
    }
}