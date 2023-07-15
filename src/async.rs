/*!
Asynchronous analogs to the base `*Chunker` types that wrap
[`AsyncRead`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html)
types and implement
[`Stream`](https://docs.rs/futures/latest/futures/stream/trait.Stream.html).
*/

use bytes::BytesMut;
use regex::bytes::Regex;
use tokio::io::AsyncRead;
use tokio_util::codec::{Decoder, FramedRead};

use crate::{ErrorResponse, ErrorStatus, MatchDisposition, RcErr};

struct ByteDecoder {
    fence: Regex,
    error_status: ErrorStatus,
    match_dispo: MatchDisposition,
    scan_offset: usize,
}

impl Decoder for ByteDecoder {
    type Item = Vec<u8>;
    type Error = RcErr;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(Some(vec![]))
    }
}

pub struct ByteChunker<A: AsyncRead> {
    freader: FramedRead<A, ByteDecoder>,
}

impl<A: AsyncRead> ByteChunker<A> {
    pub fn new(source: A, pattern: &str) -> Result<Self, RcErr> {
        let fence = Regex::new(pattern)?;
        let decoder = ByteDecoder {
            fence,
            error_status: ErrorStatus::Ok,
            match_dispo: MatchDisposition::default(),
            scan_offset: 0,
        };

        let freader = FramedRead::new(source, decoder);
        Ok(Self { freader })
    }

    pub fn on_error(mut self, response: ErrorResponse) -> Self {
        let mut d = self.freader.decoder_mut();
        d.error_status = match response {
            ErrorResponse::Halt => {
                if d.error_status != ErrorStatus::Errored {
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

    pub fn with_match(mut self, behavior: MatchDisposition) -> Self {
        let mut d = self.freader.decoder_mut();
        d.match_dispo = behavior;
        if matches!(behavior, MatchDisposition::Drop | MatchDisposition::Append) {
            d.scan_offset = 0;
        }
        self
    }
}
