#![allow(dead_code, unused_variables)]
use std::marker::PhantomData;
use std::time::Duration;

use actix::*;
use actix_web::*;
use actix::dev::ToEnvelope;
use http::header;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use context::SockJSContext;
use session::{Message, Session, SessionState};
use manager::{Acquire, Release, Record, SessionManager};

mod xhrsend;
mod eventsource;

pub use self::xhrsend::XhrSend;
pub use self::eventsource::EventSource;


pub const MAXSIZE: usize = 131_072;  // 128K bytes

/// Result of `Transport::send` method
pub enum SendResult {
    /// continue transport event loop
    Continue,
    /// stop transport, ask client to reconnect
    Stop,
}

trait Transport: Actor<Context=HttpContext<Self>> + Route {
    /// Send sockjs frame
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame) -> SendResult;
}
