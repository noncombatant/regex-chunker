/*!
The trait used for types that transform the output of a Chunker.
*/
use crate::{
    ctrl::Utf8FailureMode,
    RcErr,
};

/**
Trait used to implement a [`CustomChunker`](crate::CustomChunker) by
transforming the output of a [`ByteChunker`](crate::ByteChunker).

This is more powerful than simply calling 
[`.map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map),
[`.map_while()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map_while),
or [`.filter_map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.filter_map)
on a `ByteChunker` because the type implementing `Adapter` can be _stateful_.

The example below shows a struct implementing `Adapter` to count the number of
chunks returned so far.

```rust
use regex_chunker::{Adapter, ByteChunker, RcErr};
use std::io::Cursor;

struct ChunkCounter {
    lines: usize,
}

impl Adapter for ChunkCounter {
    type Item = Result<Vec<u8>, RcErr>;

    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item> {
        match v {
            Some(Ok(v)) => {
                self.lines += 1;
                Some(Ok(v))
            },
            x => x,
        }
    }
}

let text =
br#"What's he that wishes so?
My cousin Westmoreland? No, my fair cousin:
If we are mark'd to die, we are enow
To do our country loss; and if to live,
The fewer men, the greater share of honour."#;

let c = Cursor::new(text);

let mut chunker = ByteChunker::new(c, r#"\r?\n"#)?
    .with_adapter(ChunkCounter { lines: 0 });

let _: Vec<String> = (&mut chunker).map(|res| {
    let v: Vec<u8> = res.unwrap();
    String::from_utf8(v).unwrap()
}).collect();

// Prints "5".
println!("{}", &chunker.get_adapter().lines);
# Ok::<(), RcErr>(())
```

*/
pub trait Adapter {
    /// The type into which it transforms the values returned by the
    /// [`ByteChunker`](crate::ByteChunker)'s `Iterator` implementation.
    type Item;

    /// Convert the `ByteChunker`'s output.
    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item>;
}

/**
Simpler, less flexible, version of the [`Adapter`] trait.

Can be used in situations where it suffices to just pass `None` and `Err()`
values through and only operate when the inner
[`ByteChunker`](crate::ByteChunker)'s `.next()` returns `Some(Ok(vec))`.

This is less powerful than just using
[`.map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map),
_et. al._, but simpler because there's no error handling required by
the custom type.

The [`StringAdapter`] type tracks error status, but we can implement a
simpler type that just performs lossy UTF-8 conversion.

```rust
# use regex_chunker::RcErr;
use regex_chunker::{ByteChunker, SimpleAdapter};
use std::io::Cursor;

struct LossyStringAdapter {}

impl SimpleAdapter for LossyStringAdapter {
    type Item = String;

    fn adapt(&mut self, v: Vec<u8>) -> Self::Item {
        String::from_utf8_lossy(&v).into()
    }
}

let text = b"One, two, three four. Can I have a little more?";
let c = Cursor::new(text);

let chunks: Vec<_> = ByteChunker::new(c, "[ .,?]+")?
    .with_simple_adapter(LossyStringAdapter{})
    .map(|res| res.unwrap())
    .collect();

assert_eq!(
    &chunks,
    &["One", "two", "three", "four", "Can", "I", "have", "a", "little", "more"].clone()
);
# Ok::<(), RcErr>(())
```
}
*/
pub trait SimpleAdapter {
    /// The type into which it converts the `Vec<u8>`s successfully produced
    /// by the underlying [`ByteChunker`](crate::ByteChunker)'s  `Iterator`
    /// implementation.
    type Item;

    /// Convert the `ByteChunker`'s output when _successful_.
    fn adapt(&mut self, v: Vec<u8>) -> Self::Item;
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

/**
An example [`Adapter`] type for producing a chunker that yields `String`s.

```rust
# use std::error::Error;
# fn main() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{ByteChunker, StringAdapter};
    use std::io::Cursor;

    let text = b"One, two, three four. Can I have a little more?";
    let c = Cursor::new(text);

    let chunks: Vec<_> = ByteChunker::new(c, "[ .,?]+")?
        .with_adapter(StringAdapter::default())
        .map(|res| res.unwrap())
        .collect();

    assert_eq!(
        &chunks,
        &[
            "One", "two", "three", "four",
            "Can", "I", "have", "a", "little", "more"
        ].clone()
    );
#   Ok(()) }
```

*/
#[derive(Debug, Default)]
pub struct StringAdapter {
    status: Utf8ErrorStatus,
}

impl StringAdapter {
    pub fn new(mode: Utf8FailureMode) -> Self {
        let status = match mode {
            Utf8FailureMode::Fatal => Utf8ErrorStatus::Ok,
            Utf8FailureMode::Lossy => Utf8ErrorStatus::Lossy,
            Utf8FailureMode::Continue => Utf8ErrorStatus::Continue,
        };

        Self { status }
    }
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