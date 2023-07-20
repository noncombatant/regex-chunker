/*!
The trait used for types that transform the output of a Chunker.
*/
use super::*;

/**
Trait used to implement a [`CustomChunker`] by transforming the
output of a [`ByteChunker`].

This is more powerful than simply calling 
[`.map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map),
[`.map_while()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map_while),
or [`.filter_map()`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.filter_map)
on a `ByteChunker` because the type implementing `Adapter` can be _stateful_.
*/
pub trait Adapter {
    type Item;

    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item>;
}

pub struct CustomChunker<R, A> {
    chunker: ByteChunker<R>,
    adapter: A,
}

impl<R, A> CustomChunker<R, A> {
    /// Return a reference to this struct's [`Adapter`].
    pub fn adapter(&self) -> &A { &self.adapter }

    /// Return a mutable reference to this struct's `Adapter`.
    pub fn adapter_mut(&mut self) -> &mut A { &mut self.adapter }
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
    type Item;

    fn adapt(&mut self, v: Vec<u8>) -> Self::Item;
}

pub struct SimpleCustomChunker<R, S> {
    chunker: ByteChunker<R>,
    adapter: S,
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