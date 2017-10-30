//! Test server for sockjs-protcol functiona tests
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use std::fs::File;
use std::io::Read;

use actix_web::*;
use actix::prelude::*;
use sockjs::{Message, Session, SockJSManager, SockJSContext};

struct Chat;

impl Actor for Chat {
    type Context = SockJSContext<Self>;
}

impl Default for Chat {
    fn default() -> Chat { Chat }
}

impl Session for Chat {
    fn opened(&mut self, ctx: &mut SockJSContext<Self>) {
        ctx.broadcast("Someone joined.")
    }
    fn closed(&mut self, ctx: &mut SockJSContext<Self>, _: bool) {
        ctx.broadcast("Someone left.")
    }
}

impl Handler<Message> for Chat {
    fn handle(&mut self, msg: Message, ctx: &mut SockJSContext<Self>)
              -> Response<Self, Message>
    {
        ctx.broadcast(msg);
        Self::empty()
    }
}


fn main() {
    std::env::set_var("RUST_LOG", "actix_web=info");
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-chat");

    let sm: SyncAddress<_> = SockJSManager::<Chat>::start_default();

    HttpServer::new(
        Application::default("/")
            .middleware(Logger::new(None))
            .route_handler(
                "/sockjs/", sockjs::SockJS::<Chat, _>::new(sm.clone()))
            .resource("/", |r| r.handler(Method::GET, |_, _, _| {
                let mut file = File::open("examples/chat.html")?;
                let mut content = String::new();
                file.read_to_string(&mut content)?;

                Ok(httpcodes::HTTPOk
                   .builder()
                   .body(content)?)
            })))
        .serve::<_, ()>("127.0.0.1:8080").unwrap();

    let _ = sys.run();
}
