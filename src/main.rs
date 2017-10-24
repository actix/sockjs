#![allow(dead_code, unused_imports, unused_variables)]
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use std::net;
use std::str::FromStr;
use actix_web::*;
use actix::prelude::*;


struct MyApp;

impl Actor for MyApp {
    type Context = sockjs::SockJSContext<Self>;
}

impl Handler<sockjs::Message> for MyApp {

    fn handle(&mut self, msg: sockjs::Message, ctx: &mut sockjs::SockJSContext<Self>)
              -> Response<Self, sockjs::Message>
    {
        Self::empty()
    }
}


fn main() {
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-example");

    let http = HttpServer::new(
        Application::default("/")
            .route_handler("/", sockjs::SockJS::<MyApp, _, _>::new(sockjs::SM::new())))
        .serve::<_, ()>("127.0.0.1:9080").unwrap();

    let _ = sys.run();
}
