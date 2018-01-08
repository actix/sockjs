//! [`SockJS`](https://github.com/sockjs/sockjs-client) server
//! for [Actix](https://github.com/actix/actix)

#[macro_use]
extern crate log;
extern crate time;
extern crate bytes;
extern crate md5;
extern crate rand;
extern crate regex;
extern crate futures;
#[macro_use]
extern crate lazy_static;
extern crate percent_encoding;
#[macro_use]
extern crate bitflags;

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

extern crate http;
#[macro_use]
extern crate actix;
extern crate actix_web;

mod context;
mod application;
mod manager;
mod session;
mod utils;
mod protocol;
mod transports;

pub use application::SockJS;
pub use context::SockJSContext;
pub use manager::SockJSManager;
pub use session::{Message, Session, CloseReason};
