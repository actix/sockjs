#![allow(dead_code, unused_imports, unused_variables)]
extern crate actix;
extern crate actix_http;
extern crate sockjs;
extern crate env_logger;

use std::net;
use std::str::FromStr;
use actix::prelude::*;
use actix_http::*;


struct MyApp;

impl Actor for MyApp {
    type Context = sockjs::SockJSContext<Self>;
}

impl ResponseType<sockjs::Message> for MyApp {
    type Item = ();
    type Error = ();
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

    let mut routes = RoutingMap::default();

    let mut app = Application::default();
    app.add_handler("/", sockjs::SockJS::<MyApp, _, _>::new(sockjs::SM::new()));

    routes.add("/ws/", app);

    let http = HttpServer::new(routes);
    http.serve::<()>(
        &net::SocketAddr::from_str("127.0.0.1:9080").unwrap()).unwrap();

    let _ = sys.run();
}
