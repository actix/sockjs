

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

pub trait Session {

}
