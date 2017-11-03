//! Test server for sockjs-protcol functiona tests
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use actix_web::*;
use actix::prelude::*;

use sockjs::{Message, Session, SockJSManager, SockJSContext};

#[derive(Debug)]
struct Echo;

impl Actor for Echo {
    type Context = SockJSContext<Self>;
}

impl Default for Echo {
    fn default() -> Echo {
        Echo
    }
}

impl Session for Echo {}

impl Handler<Message> for Echo {
    fn handle(&mut self, msg: Message, ctx: &mut SockJSContext<Self>)
              -> Response<Self, Message>
    {
        ctx.send(msg);
        Self::empty()
    }
}

#[derive(Debug)]
struct Close;

impl Actor for Close {
    type Context = sockjs::SockJSContext<Self>;

    fn started(&mut self, ctx: &mut SockJSContext<Self>) {
        ctx.close()
    }
}

impl Default for Close {
    fn default() -> Close {
        Close
    }
}

impl Session for Close {}

impl Handler<Message> for Close {
    fn handle(&mut self, _: Message, _: &mut sockjs::SockJSContext<Self>)
              -> Response<Self, Message>
    {
        Self::empty()
    }
}


fn main() {
    if ::std::env::var("RUST_LOG").is_err() {
        ::std::env::set_var("RUST_LOG", "actix_web=info");
    }
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let sm: SyncAddress<_> = SockJSManager::<Echo>::start_default();
    let cl: SyncAddress<_> = SockJSManager::<Close>::start_default();

    HttpServer::new(
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
                sockjs::SockJS::<Echo, _>::new(sm).cookie_needed(true))
            .resource("/exit.html", |r| r.handler(Method::GET, |_, _, _| {
                Arbiter::system().send(msgs::SystemExit(0));
                Ok(httpcodes::HTTPOk)})))
        .serve::<_, ()>("127.0.0.1:52081").unwrap();

    let _ = sys.run();
}
