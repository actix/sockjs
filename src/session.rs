use actix::*;

use protocol::Frame;
use context::SockJSContext;

/// Session state
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

#[derive(Debug, Message)]
pub struct Message(pub String);

impl From<Message> for Frame {
    fn from(m: Message) -> Frame {
        Frame::Message(m.0)
    }
}

impl From<&'static str> for Message {
    fn from(s: &'static str) -> Message {
        Message(s.to_owned())
    }
}

impl From<String> for Message {
    fn from(s: String) -> Message {
        Message(s)
    }
}

#[doc(hidden)]
pub enum SessionError {
    Acquired,
    Interrupted,
    Closing,
    InternalError,
}

#[derive(Debug)]
/// Reason for closing session
pub enum CloseReason {
    /// Session closed session
    Normal,
    /// Session expired
    Expired,
    /// Peer get disconnected
    Interrupted,
}

/// This trait defines sockjs session
#[allow(unused_variables)]
pub trait Session: Actor<Context=SockJSContext<Self>> + Default + Handler<Message> {

    /// Method get called when session get opened
    fn opened(&mut self, ctx: &mut SockJSContext<Self>) {}

    /// Method get called when transport acquires this session
    fn acquired(&mut self, ctx: &mut SockJSContext<Self>) {}

    /// Method get called when transport releases this session
    fn released(&mut self, ctx: &mut SockJSContext<Self>) {}

    /// Method get called when session get closed
    fn closed(&mut self, ctx: &mut SockJSContext<Self>, reason: CloseReason) {}
}
