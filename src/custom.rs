/*!
The custom chunker type.
*/
use std::io::Read;

use crate::{Adapter, ByteChunker, RcErr, SimpleAdapter};

/**
A chunker that has additionally been supplied with an [`Adapter`], so it
can produce arbitrary types. The `CustomChunker`s does not have a separate
constructor; it is built by combining a `ByteChunker` with an `Adapter`
using [`ByteChunker::with_adapter`].

Here's the example using the [`StringAdapter`](crate::StringAdapter) type
to yield [`String`]s instead of byte vectors.

```rust
# use std::error::Error;
# fn main() -> Result<(), Box<dyn Error>> {
    use regex_chunker::{ByteChunker, CustomChunker, StringAdapter};
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

pub struct CustomChunker<R, A> {
    chunker: ByteChunker<R>,
    adapter: A,
}

impl<R, A> CustomChunker<R, A> {
    /// Consume this `CustomChunker` and return the underlying
    /// [`ByteChunker`] and [`Adapter`].
    pub fn into_innards(self) -> (ByteChunker<R>, A) {
        (self.chunker, self.adapter)
    }

    /// Get a reference to the underlying [`Adapter`].
    pub fn get_adapter(&self) -> &A { &self.adapter }

    /// Get a mutable reference to the underlying [`Adapter`].
    pub fn get_adapter_mut(&mut self) -> &mut A { &mut self.adapter }

}

impl<R, A> From<(ByteChunker<R>, A)> for CustomChunker<R, A> {
    fn from((chunker, adapter): (ByteChunker<R>, A)) -> Self {
        Self { chunker, adapter }
    }
}

impl<R, A> Iterator for CustomChunker<R, A>
where
    R: Read,
    A: Adapter,
{
    type Item = A::Item;

    fn next(&mut self) -> Option<A::Item> {
        let opt = self.chunker.next();
        self.adapter.adapt(opt)
    }
}

/**
A version of [`CustomChunker`] that takes a [`SimpleAdapter`] type.

This type will disappear once
[Issue #1672](https://github.com/rust-lang/rfcs/pull/1672) is resolved.
As it is, if `CustomChunker` tries to implement `Iterator` for _both_
`Adapter` and `SimpleAdapter` types, these implementations conflict
(though they shouldn't). Once the compiler is capable of figuring this
out, `CustomChunker` will work with types that implement both of
these traits.
*/
pub struct SimpleCustomChunker<R, A> {
    chunker: ByteChunker<R>,
    adapter: A,
}

impl<R, A> SimpleCustomChunker<R, A> {
    /// Consume this `SimpleCustomChunker` and return the underlying
    /// [`ByteChunker`] and [`Adapter`].
    pub fn into_innards(self) -> (ByteChunker<R>, A) {
        (self.chunker, self.adapter)
    }

    /// Get a reference to the underlying [`SimpleAdapter`].
    pub fn get_adapter(&self) -> &A { &self.adapter }

    /// Get a mutable reference to the underlying [`SimpleAdapter`].
    pub fn get_adapter_mut(&mut self) -> &mut A { &mut self.adapter }
}

impl<R, A> From<(ByteChunker<R>, A)> for SimpleCustomChunker<R, A> {
    fn from((chunker, adapter): (ByteChunker<R>, A)) -> Self {
        Self { chunker, adapter }
    }
}

impl<R, A> Iterator for SimpleCustomChunker<R, A>
where
    R: Read,
    A: SimpleAdapter,
{
    type Item = Result<A::Item, RcErr>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.chunker.next()? {
            Ok(v) => Some(Ok(self.adapter.adapt(v))),
            Err(e) => Some(Err(e)),
        }
    }
}