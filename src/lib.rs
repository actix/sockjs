//! SockJS server for [Actix](https://github.com/fafhrd91/actix)
#![allow(unused_imports)]

#[macro_use]
extern crate log;
extern crate time;
extern crate bytes;
extern crate sha1;
extern crate url;
extern crate rand;
#[macro_use]
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_proto;

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

extern crate http;
extern crate actix;
extern crate actix_http;

mod context;
mod application;
mod manager;
mod session;
mod utils;

pub use application::SockJS;
pub use context::SockJSContext;
pub use manager::{SM, SessionManager};
pub use session::{Message, State, Session};
