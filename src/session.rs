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

pub trait Session {

}
