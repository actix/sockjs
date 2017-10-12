#![allow(dead_code, unused_variables)]
use std::rc::Rc;
use std::cell::RefCell;
use std::marker::PhantomData;

use rand::{self, Rng, ThreadRng};
use bytes::Bytes;
use http::Method;
use http::header::{self, HeaderValue, CONTENT_TYPE};
use actix::dev::*;
use actix_http::dev::*;

use context::SockJSContext;
use manager::SessionManager;
use session::Message;
use utils::{Info, SockjsHeaders};


pub struct SockJS<A, T, S=()> where A: Actor<Context=SockJSContext<A>> {
    manager: Rc<T>,
    act: PhantomData<A>,
    state: PhantomData<S>,
    prefix: usize,
    rng: RefCell<ThreadRng>,
    // factory: RouteFactory<A, S>,
}

impl<A, T, S> SockJS<A, T, S>
    where A: Actor<Context=SockJSContext<A>> + Handler<Message>,
          T: SessionManager,
          S: 'static,
{
    pub fn new(manager: T) -> Self
    {
        SockJS {
            act: PhantomData,
            state: PhantomData,
            prefix: 0,
            rng: RefCell::new(rand::thread_rng()),
            manager: Rc::new(manager),
        }
    }
}

enum RouteType {
    Greeting,
    Info,
    InfoOptions,
    Unknown,
}

impl<A, T: 'static, S: 'static> RouteHandler<S> for SockJS<A, T, S>
    where A: Actor<Context=SockJSContext<A>> + Handler<Message>,
          T: SessionManager,
{
    fn handle(&self, req: HttpRequest, payload: Payload, state: Rc<S>) -> Task {
        let route = {
            let path = &req.path()[self.prefix..];
            println!("====== {:?}", path);
            if path.is_empty() {
                RouteType::Greeting
            } else if path == "info" {
                match req.method() {
                    &Method::GET => RouteType::Info,
                    &Method::OPTIONS => RouteType::InfoOptions,
                    _ => RouteType::Unknown,
                }
            } else {
                RouteType::Unknown
            }
        };
        match route {
            RouteType::Greeting => {
                Task::reply(
                    req, httpcodes::HTTPOk
                        .builder()
                        .header(CONTENT_TYPE,
                                HeaderValue::from_static("text/plain; charset=UTF-8"))
                        .body(Body::Binary(
                            Bytes::from_static(b"Welcome to SockJS!\n".as_ref()))))
            },
            RouteType::Info => {
                let resp = httpcodes::HTTPOk
                    .builder()
                    .header(CONTENT_TYPE, "application/json;charset=UTF-8")
                    .sockjs_cache_control()
                    .sockjs_cors_headers(req.headers())
                    .json_body(Info::new(self.rng.borrow_mut().gen::<u32>(), true, true));
                Task::reply(req, resp)
            },
            RouteType::InfoOptions => {
                let resp = httpcodes::HTTPNoContent
                    .builder()
                    .header(CONTENT_TYPE, "application/json;charset=UTF-8")
                    .sockjs_cache_control()
                    .sockjs_allow_methods()
                    .sockjs_cors_headers(req.headers())
                    .body(Body::Empty);
                Task::reply(req, resp)
            },
            RouteType::Unknown => {
                Task::reply(req, httpcodes::HTTPNotFound)
            }
        }
    }

    fn set_prefix(&mut self, prefix: String) {
        self.prefix = prefix.len();
        println!("PREFIX: {:?}", prefix);
    }
}

struct SockJSRoute<S> {
    state: PhantomData<S>,
}

impl<S: 'static> Actor for SockJSRoute<S> {
    type Context = HttpContext<Self>;
}

impl<S: 'static> Route for SockJSRoute<S> {
    type State = S;

    fn request(req: HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>) -> Reply<Self> {
        match ws::handshake(&req) {
            Ok(resp) => {
                ctx.start(req, resp);
                ctx.add_stream(ws::WsStream::new(payload));
                Reply::stream(SockJSRoute{state: PhantomData})
            },
            Err(err) =>
                Reply::reply(req, err)
        }
    }
}


impl<S: 'static> ResponseType<ws::Message> for SockJSRoute<S> {
    type Item = ();
    type Error = ();
}

impl<S: 'static> StreamHandler<ws::Message> for SockJSRoute<S> {}

impl<S: 'static> Handler<ws::Message> for SockJSRoute<S> {
    fn handle(&mut self, msg: ws::Message, ctx: &mut HttpContext<Self>)
              -> Response<Self, ws::Message>
    {
        println!("WS: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, msg),
            ws::Message::Text(text) => ws::WsWriter::text(ctx, text),
            ws::Message::Binary(bin) => ws::WsWriter::binary(ctx, bin),
            _ => (),
        }
        Self::empty()
    }
}
