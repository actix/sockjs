use std::fmt::Debug;
use actix::*;


pub enum State {
    New,
    Opened,
    Closing,
    Closed,
}

pub enum Message {
    Open,
    Message,
    Closed,
}

impl ResponseType for Message {
    type Item = ();
    type Error = ();
}

pub enum SessionError {
    Acquired,
    Interrupted,
    Closing,
}

pub trait Session: Default + Debug + 'static {
    type Message: Send;
    type State: Default + Send + 'static;

    fn acquired(&mut self) {}

    fn released(&mut self) {}

    fn handle(&mut self) {}
}
