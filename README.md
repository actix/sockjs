# SockJS server [![Build Status](https://travis-ci.org/actix/sockjs.svg?branch=master)](https://travis-ci.org/actix/sockjs) [![codecov](https://codecov.io/gh/actix/sockjs/branch/master/graph/badge.svg)](https://codecov.io/gh/actix/sockjs) [![crates.io](http://meritbadge.herokuapp.com/sockjs)](https://crates.io/crates/sockjs)

[SockJS](https://github.com/sockjs) server for for [Actix framework](https://github.com/actix/actix).

* [API Documentation](http://actix.github.io/sockjs/sockjs/)
* Cargo package: [sockjs](https://crates.io/crates/sockjs)
* SockJS is built with [Actix web](https://github.com/actix/actix-web)
* Minimum supported Rust version: 1.21 or later

---

Actix SockJS is licensed under the [Apache-2.0 license](http://opensource.org/licenses/APACHE-2.0).

## Usage

To use `sockjs`, add this to your `Cargo.toml`:

```toml
[dependencies]
sockjs = "0.2"
```

## Supported transports

* [websocket](http://tools.ietf.org/html/draft-ietf-hybi-thewebsocketprotocol-10)
* [xhr-streaming](https://secure.wikimedia.org/wikipedia/en/wiki/XMLHttpRequest#Cross-domain_requests)
* [xhr-polling](https://secure.wikimedia.org/wikipedia/en/wiki/XMLHttpRequest#Cross-domain_requests)
* [iframe-xhr-polling](https://developer.mozilla.org/en/DOM/window.postMessage)
* iframe-eventsource ([EventSource](http://dev.w3.org/html5/eventsource/) used from an [iframe via
  postMessage](https://developer.mozilla.org/en/DOM/window.postMessage>))
* iframe-htmlfile ([HtmlFile](http://cometdaily.com/2007/11/18/ie-activexhtmlfile-transport-part-ii/)
  used from an [iframe via postMessage](https://developer.mozilla.org/en/DOM/window.postMessage>).)
* [jsonp-polling](https://secure.wikimedia.org/wikipedia/en/wiki/JSONP)


## Simple chat example

```rust
extern crate actix;
extern crate actix_web;
extern crate sockjs;

use actix_web::*;
use actix::prelude::*;
use sockjs::{Message, Session, CloseReason, SockJSManager, SockJSContext};

struct Chat;

/// `SockJSContext` context is required for sockjs session
impl Actor for Chat {
    type Context = SockJSContext<Self>;
}

/// Session has to implement `Default` trait
impl Default for Chat {
    fn default() -> Chat { Chat }
}

/// Sockjs session trait
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

    fn handle(&mut self, msg: Message, ctx: &mut SockJSContext<Self>)
    {
        // broadcast message to all sessions
        ctx.broadcast(msg);
    }
}


fn main() {
    let sys = actix::System::new("sockjs-chat");

    // SockJS sessions manager
    let sm: Addr<Syn, _> = SockJSManager::<Chat>::start_default();

    HttpServer::new(move || {
        let manager = sm.clone();
        Application::new()
            // register SockJS application
            .handler(
                "/sockjs/", sockjs::SockJS::new(manager.clone()))})
        .bind("127.0.0.1:8080").unwrap()
        .start();

    // let _ = sys.run();
}
```

[Full chat example](https://github.com/actix/actix-sockjs/blob/master/examples/chat.rs)
