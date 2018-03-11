use std::collections::HashMap;
use std::fmt;
use fibers::sync::oneshot;

use {Error, Result};
use codec::Decode;
use frame::{Frame, HandleFrame};
use message::MessageSeqNo;

pub struct IncomingFrameHandler {
    handlers: HashMap<MessageSeqNo, BoxResponseHandler>,
}
impl IncomingFrameHandler {
    pub fn new() -> Self {
        IncomingFrameHandler {
            handlers: HashMap::new(),
        }
    }

    pub fn register_response_handler(&mut self, seqno: MessageSeqNo, handler: BoxResponseHandler) {
        self.handlers.insert(seqno, handler);
    }
}
impl HandleFrame for IncomingFrameHandler {
    type Item = ();
    fn handle_frame(&mut self, frame: &Frame) -> Result<Option<Self::Item>> {
        let seqno = frame.seqno();
        if let Some(mut handler) = self.handlers.remove(&seqno) {
            if track!(handler.handle_frame(frame))?.is_some() {
                Ok(Some(()))
            } else {
                self.handlers.insert(seqno, handler);
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    fn handle_error(&mut self, seqno: MessageSeqNo, error: Error) {
        if let Some(mut handler) = self.handlers.remove(&seqno) {
            handler.handle_error(seqno, error);
        }
    }
}
impl fmt::Debug for IncomingFrameHandler {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "IncomingFrameHandler {{ handlers.len: {} }}",
            self.handlers.len()
        )
    }
}

pub type BoxResponseHandler = Box<HandleFrame<Item = ()> + Send + 'static>;

#[derive(Debug)]
pub struct ResponseHandler<T, D> {
    decoder: D,
    reply_tx: Option<oneshot::Monitored<T, Error>>,
}
impl<T, D: Decode<T>> ResponseHandler<T, D> {
    pub fn new(decoder: D, reply_tx: oneshot::Monitored<T, Error>) -> Self {
        ResponseHandler {
            decoder,
            reply_tx: Some(reply_tx),
        }
    }
}
impl<T, D: Decode<T>> HandleFrame for ResponseHandler<T, D> {
    type Item = ();
    fn handle_frame(&mut self, frame: &Frame) -> Result<Option<Self::Item>> {
        track!(self.decoder.decode(frame.data()))?;
        if frame.is_end_of_message() {
            let response = track!(self.decoder.finish())?;
            let reply_tx = self.reply_tx.take().expect("Never fails");
            let _ = reply_tx.exit(Ok(response));
            Ok(Some(()))
        } else {
            Ok(None)
        }
    }
    fn handle_error(&mut self, _seqno: MessageSeqNo, error: Error) {
        let reply_tx = self.reply_tx.take().expect("Never fails");
        let _ = reply_tx.exit(Err(error));
    }
}
