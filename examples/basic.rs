#![allow(dead_code, unused_imports, unused_variables)]
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use std::net;
use std::str::FromStr;
use std::time::Duration;
use actix_web::*;
use actix::prelude::*;

use sockjs::{Message, Session, CloseCode, SockJSManager};

#[derive(Debug)]
struct Echo;

impl Actor for Echo {
    type Context = sockjs::SockJSContext<Self>;
}

impl Default for Echo {
    fn default() -> Echo {
        Echo
    }
}

impl Session for Echo {}

impl Handler<Message> for Echo {
    fn handle(&mut self, msg: Message, ctx: &mut sockjs::SockJSContext<Self>)
              -> Response<Self, Message>
    {
        println!("MESSAGE: {:?}", msg);
        ctx.send(msg);
        Self::empty()
    }
}

#[derive(Debug)]
struct Close;

impl Actor for Close {
    type Context = sockjs::SockJSContext<Self>;

    fn started(&mut self, ctx: &mut sockjs::SockJSContext<Self>) {
        ctx.close(CloseCode::GoAway)
    }
}

impl Default for Close {
    fn default() -> Close {
        Close
    }
}

impl Session for Close {}

impl Handler<Message> for Close {
    fn handle(&mut self, msg: Message, ctx: &mut sockjs::SockJSContext<Self>)
              -> Response<Self, Message>
    {
        Self::empty()
    }
}


fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let sm: SyncAddress<_> = SockJSManager::<Echo>::new().start();
    let cl: SyncAddress<_> = SockJSManager::<Close>::new().start();

    let http = HttpServer::new(
        Application::default("/")
            .middleware(Logger::new(None))
            .route_handler(
                "/echo", sockjs::SockJS::<Echo, _>::new(sm.clone()).maxsize(4096))
            .route_handler(
                "/close", sockjs::SockJS::<Close, _>::new(cl))
            .route_handler(
                "/disabled_websocket_echo",
                sockjs::SockJS::<Echo, _>::new(sm.clone()).disable_transports(vec!["websocket"]))
            .route_handler(
                "/cookie_needed_echo",
                sockjs::SockJS::<Echo, _>::new(sm).cookie_needed(true)))
        .serve::<_, ()>("127.0.0.1:8081").unwrap();

    let _ = sys.run();
}
