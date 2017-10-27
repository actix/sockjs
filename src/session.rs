use std::fmt::Debug;
use bytes::Bytes;
use actix::*;
use context::SockJSContext;

#[derive(PartialEq, Debug)]
pub enum SessionState {
    /// Newly create session
    New,
    /// Session is running, but transport is not connected
    Idle,
    /// Transport is connected
    Running,
    /// Closing session
    Closing,
    /// Session is closed, transport is dropped.
    Closed,
}

#[derive(Debug)]
pub enum Message {
    Str(String),
    Bin(Bytes),
}

impl ResponseType for Message {
    type Item = ();
    type Error = ();
}

impl From<String> for Message {
    fn from(s: String) -> Message {
        Message::Str(s)
    }
}

impl From<Bytes> for Message {
    fn from(s: Bytes) -> Message {
        Message::Bin(s)
    }
}

pub enum SessionError {
    Acquired,
    Interrupted,
    Closing,
    InternalError,
}

#[allow(unused_variables)]
pub trait Session: Actor<Context=SockJSContext<Self>> + Default + Debug + Handler<Message> {

    fn acquired(&mut self, ctx: &mut SockJSContext<Self>) {
        println!("acquired");
    }

    fn released(&mut self, ctx: &mut SockJSContext<Self>) {
        println!("released");
    }
}
