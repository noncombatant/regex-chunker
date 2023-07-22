/*!
Async adapter trait and structs that use it.
*/
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::AsyncRead;
use tokio_stream::Stream;

use crate::{RcErr, stream::*};

pub trait Adapter {
    type Item;

    fn adapt(&mut self, v: Option<Result<Vec<u8>, RcErr>>) -> Option<Self::Item>;
}

pub struct CustomChunker<R: AsyncRead, A> {
    chunker: ByteChunker<R>,
    adapter: A,
}

impl<R: AsyncRead, A> CustomChunker<R, A> {
    pub fn into_innards(self) -> (ByteChunker<R>, A) {
        (self.chunker, self.adapter)
    }

    pub fn get_adapter(&self) -> &A { &self.adapter }

    pub fn get_adapter_mut(&mut self) -> &mut A { &mut self.adapter }
}

impl<R: AsyncRead, A> Unpin for CustomChunker<R, A> {}

impl<R, A> Stream for CustomChunker<R, A>
where
    R: AsyncRead + Unpin,
    A: Adapter
{
    type Item = A::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let p = Pin::new(&mut self.chunker).poll_next(cx);
        match p {
            Poll::Pending => Poll::Pending,
            Poll::Ready(x) => Poll::Ready(self.adapter.adapt(x)),
        }
    }
}

