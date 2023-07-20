/*!
The trait used for types that transform the output of a Chunker.
*/
use super::*;

/**
Trait used to implement a [`CustomChunker`] by transforming the
output of a [`ByteChunker`]. The [`StringChunker`] is implemented this way.
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
    /// [`ByteChunker`]'s `Iterator` implementation.
    type Item;

    /// Convert the `ByteChunker`'s output.
    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item>;
}

pub struct CustomChunker<R, A> {
    pub(super) chunker: ByteChunker<R>,
    pub(super) adapter: A,
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
Simpler, less flexible, version of the [`Adapter`] trait.

Can be used in situations where it suffices to just pass `None` and `Err()`
values through and only operate when the inner [`ByteChunker`]'s `.next()`
returns `Some(Ok(vec))`.

This is less powerful than just using
[`.map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map),
_et. al._, but simpler because there's no error handling required by
the custom type.
*/
pub trait SimpleAdapter {
    /// The type into which it converts the `Vec<u8>`s successfully produced
    /// by the underlying [`ByteChunker`]'s  `Iterator` implementation.
    type Item;

    /// Convert the `ByteChunker`'s output when _successful_.
    fn adapt(&mut self, v: Vec<u8>) -> Self::Item;
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
pub struct SimpleCustomChunker<R, S> {
    chunker: ByteChunker<R>,
    adapter: S,
}

impl<R, A> SimpleCustomChunker<R, A> {
    /// Consume this `SimpleCustomChunker` and return the underlying
    /// [`ByteChunker`] and [`Adapter`].
    pub fn into_innards(self) -> (ByteChunker<R>, A) {
        (self.chunker, self.adapter)
    }
}

impl<R, S> Iterator for SimpleCustomChunker<R, S>
where
    R: Read,
    S: SimpleAdapter,
{
    type Item = Result<S::Item, RcErr>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.chunker.next()? {
            Ok(v) => Some(Ok(self.adapter.adapt(v))),
            Err(e) => Some(Err(e)),
        }
    }
}