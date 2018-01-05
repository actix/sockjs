//! Simple chat server
extern crate actix;
extern crate actix_web;
extern crate sockjs;
extern crate env_logger;

use std::fs::File;
use std::io::Read;

use actix_web::*;
use actix::prelude::*;
use sockjs::{Message, Session, CloseReason, SockJSManager, SockJSContext};

struct Chat;

/// Sockjs session has to use `SockJSContext` context
impl Actor for Chat {
    type Context = SockJSContext<Self>;
}

/// Session has to implement `Default` trait
impl Default for Chat {
    fn default() -> Chat { Chat }
}

/// Sockjs session trait implementation
impl Session for Chat {
    fn opened(&mut self, ctx: &mut SockJSContext<Self>) {
        ctx.broadcast("Someone joined.")
    }
    fn closed(&mut self, ctx: &mut SockJSContext<Self>, _: CloseReason) {
        ctx.broadcast("Someone left.")
    }
}

/// Session has to be able to handle `sockjs::Message` messages
impl Handler<Message> for Chat {
    type Result = ();

    fn handle(&mut self, msg: Message, ctx: &mut SockJSContext<Self>) {
        ctx.broadcast(msg);
    }
}


fn main() {
    std::env::set_var("RUST_LOG", "actix_web=info");
    let _ = env_logger::init();

    let sys = actix::System::new("sockjs-chat");

    // Sockjs sessions manager
    let sm: SyncAddress<_> = SockJSManager::<Chat>::start_default();

    HttpServer::new(
        move || {
            let s = sm.clone();
            Application::new()
                .middleware(middleware::Logger::default())
                .handler(
                    // Sockjs route handler
                    "/sockjs/", sockjs::SockJS::new(s.clone()))
                .resource("/", |r| r.method(Method::GET).f(|_| -> Result<HttpResponse> {
                    let mut file = File::open("examples/chat.html")?;
                    let mut content = String::new();
                    file.read_to_string(&mut content)?;

                Ok(httpcodes::HTTPOk
                   .build()
                   .body(content)?)
                }))})
        .bind("127.0.0.1:8080").unwrap()
        .start();

    let _ = sys.run();
}
