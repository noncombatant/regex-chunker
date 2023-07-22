/*!
The custom chunker type.
*/
use std::io::Read;

use crate::{Adapter, ByteChunker, RcErr, SimpleAdapter};

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