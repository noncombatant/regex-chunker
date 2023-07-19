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

/**
Simpler, less flexible, version of the [`Adapter`] trait.

Can be used in situations where it suffices to just pass `None` and `Err()`
values through and only operate when the inner [`ByteChunker`]'s `.next()`
returns `Some(Ok(vec))`.
*/
pub trait SimpleAdapter {
    type Item;

    fn adapt(&mut self, v: Vec<u8>) -> Self::Item;
}

pub struct CustomChunker<R, A> {
    chunker: ByteChunker<R>,
    adapter: A,
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

impl<R, A> Iterator for CustomChunker<R, A>
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