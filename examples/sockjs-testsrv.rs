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
    type Result = ();

    fn handle(&mut self, msg: Message, ctx: &mut SockJSContext<Self>) {
        ctx.send(msg);
    }
}

#[derive(Debug)]
struct Close;

impl Actor for Close {
    type Context = sockjs::SockJSContext<Self>;
}

impl Default for Close {
    fn default() -> Close {
        Close
    }
}

impl Session for Close {

    fn opened(&mut self, ctx: &mut SockJSContext<Self>) {
        ctx.close()
    }
}

impl Handler<Message> for Close {
    type Result = ();

    fn handle(&mut self, _: Message, ctx: &mut sockjs::SockJSContext<Self>) {
        ctx.close()
    }
}


fn main() {
    if ::std::env::var("RUST_LOG").is_err() {
        ::std::env::set_var("RUST_LOG", "actix_web=info");
    }
    env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let sm: Addr<Syn, _> = SockJSManager::<Echo>::start_default();
    let cl: Addr<Syn, _> = SockJSManager::<Close>::start_default();

    server::new(
        move || App::new()
            .middleware(middleware::Logger::default())
            .handler(
                "/echo", sockjs::SockJS::new(sm.clone()).maxsize(4096))
            .handler(
                "/close", sockjs::SockJS::new(cl.clone()))
            .handler(
                "/disabled_websocket_echo",
                sockjs::SockJS::new(sm.clone()).disable_transports(vec!["websocket"]))
            .handler(
                "/cookie_needed_echo",
                sockjs::SockJS::new(sm.clone()).cookie_needed(true))
            .resource("/exit.html", |r| r.f(|_| {
                Arbiter::system().do_send(actix::msgs::SystemExit(0));
                HttpResponse::Ok()})))
        .bind("127.0.0.1:52081").unwrap()
        .start();

    let _ = sys.run();
}
