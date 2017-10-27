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

use sockjs::{Message, Session, SockJSManager};

#[derive(Debug)]
struct MyApp;

impl Actor for MyApp {
    type Context = sockjs::SockJSContext<Self>;

    fn started(&mut self, ctx: &mut sockjs::SockJSContext<Self>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            ctx.send("TEST".to_owned());
        });
    }
}

impl Default for MyApp {
    fn default() -> MyApp {
        MyApp
    }
}

impl Session for MyApp {}

impl Handler<Message> for MyApp {
    fn handle(&mut self, msg: Message, ctx: &mut sockjs::SockJSContext<Self>)
              -> Response<Self, Message>
    {
        println!("MESSAGE: {:?}", msg);
        ctx.send(msg);
        Self::empty()
    }
}


fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let sm: SyncAddress<_> = SockJSManager::<MyApp>::new().start();

    let http = HttpServer::new(
        Application::default("/")
            .middleware(Logger::new(None))
            .route_handler("/", sockjs::SockJS::<MyApp, _>::new(sm)))
        .serve::<_, ()>("127.0.0.1:9080").unwrap();

    let _ = sys.run();
}
