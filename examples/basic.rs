#![allow(dead_code, unused_imports, unused_variables)]
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use std::net;
use std::str::FromStr;
use actix_web::*;
use actix::prelude::*;

use sockjs::{Session, SessionManager};

#[derive(Debug)]
struct MyApp;

impl Actor for MyApp {
    type Context = sockjs::SockJSContext<Self>;
}

impl Default for MyApp {
    fn default() -> MyApp {
        MyApp
    }
}

impl Session for MyApp {
    type State = ();
    type Message = ();

    fn handle(&mut self) {
    }
}


fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let sm: SyncAddress<_> = SessionManager::new().start();

    let http = HttpServer::new(
        Application::default("/")
            .middleware(Logger::new(None))
            .route_handler("/", sockjs::SockJS::<MyApp, _>::new(sm)))
        .serve::<_, ()>("127.0.0.1:9080").unwrap();

    let _ = sys.run();
}
