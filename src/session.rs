use std::fmt::Debug;
use bytes::Bytes;
use serde_json;
use actix::*;

use protocol::Frame;
use context::SockJSContext;

#[derive(PartialEq, Debug)]
pub enum SessionState {
    /// Newly create session
    New,
    /// Transport is connected
    Running,
    /// Session interrupted
    Interrupted,
    /// Session is closed
    Closed,
}

#[derive(Debug)]
pub enum Message {
    Str(String),
    Bin(Bytes),
    Strs(Vec<String>),
}

impl ResponseType for Message {
    type Item = ();
    type Error = ();
}

impl From<Message> for Frame {
    fn from(m: Message) -> Frame {
        match m {
            Message::Str(s) => Frame::Message(s),
            Message::Bin(s) => Frame::MessageBlob(s),
            Message::Strs(s) => Frame::MessageVec(serde_json::to_string(&s).unwrap()),
        }
    }
}

impl From<Vec<String>> for Message {
    fn from(msgs: Vec<String>) -> Message {
        Message::Strs(msgs)
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
