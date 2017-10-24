#![allow(dead_code, unused_variables)]
use std::rc::Rc;
use std::cell::RefCell;
use std::marker::PhantomData;

use md5;
use rand::{self, Rng, ThreadRng};
use bytes::Bytes;
use http::Method;
use http::header::{self, HeaderValue, CONTENT_TYPE};
use actix::dev::*;
use actix_web::*;
use actix_web::dev::*;

use protocol;
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
    router: RouteRecognizer<RouteType>,
    iframe_html: Rc<String>,
    iframe_html_md5: String,
    // factory: RouteFactory<A, S>,
}

impl<A, T, S> SockJS<A, T, S>
    where A: Actor<Context=SockJSContext<A>> + Handler<Message>,
          T: SessionManager,
          S: 'static,
{
    pub fn new(manager: T) -> Self
    {
        let routes = vec![
            ("/", RouteType::Greeting),
            ("/info", RouteType::Info),
            ("/{server}/{session}/{transport}", RouteType::Transport),
            ("/websocket", RouteType::Websocket),
            ("/iframe.html", RouteType::IFrame),
            ("/iframe{version}.html", RouteType::IFrame),
        ].into_iter().map(|(s, t)| (s.to_owned(), t));

        let html = protocol::IFRAME_HTML.to_owned();
        let digest = md5::compute(&html);

        SockJS {
            act: PhantomData,
            state: PhantomData,
            prefix: 0,
            rng: RefCell::new(rand::thread_rng()),
            manager: Rc::new(manager),
            router: RouteRecognizer::new("/", routes),
            iframe_html: Rc::new(html),
            iframe_html_md5: format!("{:x}", digest),
        }
    }
}

#[derive(Debug)]
enum RouteType {
    Greeting,
    Info,
    Transport,
    Websocket,
    IFrame,
}

impl<A, T: 'static, S: 'static> RouteHandler<S> for SockJS<A, T, S>
    where A: Actor<Context=SockJSContext<A>> + Handler<Message>,
          T: SessionManager,
{
    fn handle(&self, req: &mut HttpRequest, payload: Payload, state: Rc<S>) -> Task {
        if let Some((params, route)) = self.router.recognize(req.path()) {
            match *route {
                RouteType::Greeting => {
                    return Task::reply(
                        httpcodes::HTTPOk
                            .builder()
                            .content_type("text/plain; charset=UTF-8")
                            .body("Welcome to SockJS!\n"))
                },
                RouteType::Info => {
                    let resp = if *req.method() == Method::GET {
                        httpcodes::HTTPOk
                            .builder()
                            .content_type("application/json;charset=UTF-8")
                            .sockjs_cache_control()
                            .sockjs_cors_headers(req.headers())
                            .json_body(Info::new(
                                self.rng.borrow_mut().gen::<u32>(), true, true)).unwrap()
                    } else if *req.method() == Method::OPTIONS {
                        let _ = req.load_cookies();
                        httpcodes::HTTPNoContent
                            .builder()
                            .content_type("application/json;charset=UTF-8")
                            .sockjs_cache_control()
                            .sockjs_allow_methods()
                            .sockjs_cors_headers(req.headers())
                            .sockjs_session_cookie(&req)
                            .finish().unwrap()
                    } else {
                        httpcodes::HTTPMethodNotAllowed.response()
                    };
                    return Task::reply(resp)
                },
                RouteType::IFrame => {
                    let resp = if req.headers().contains_key(header::IF_NONE_MATCH) {
                        httpcodes::HTTPNotModified
                            .builder()
                            .content_type("")
                            .sockjs_cache_headers()
                            .finish().unwrap()
                    } else {
                        httpcodes::HTTPOk
                            .builder()
                            .content_type("text/html;charset=UTF-8")
                            .header(header::ETAG, self.iframe_html_md5.as_str())
                            .sockjs_cache_headers()
                            .body(&self.iframe_html)
                    };
                    return Task::reply(resp)
                },
                _ => ()
            }
        }

        return Task::reply(httpcodes::HTTPMethodNotAllowed.response())
    }

    fn set_prefix(&mut self, prefix: String) {
        self.prefix = prefix.len();
        self.router.set_prefix(prefix);
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

    fn request(req: &mut HttpRequest, payload: Payload,
               ctx: &mut HttpContext<Self>) -> RouteResult<Self> {
        let resp = ws::handshake(&req)?;
        ctx.start(resp);
        ctx.add_stream(ws::WsStream::new(payload));
        Reply::async(SockJSRoute{state: PhantomData})
    }
}


impl<S: 'static> StreamHandler<ws::Message> for SockJSRoute<S> {}

impl<S: 'static> Handler<ws::Message> for SockJSRoute<S> {
    fn handle(&mut self, msg: ws::Message, ctx: &mut HttpContext<Self>)
              -> Response<Self, ws::Message>
    {
        println!("WS: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, msg),
            ws::Message::Text(text) => ws::WsWriter::text(ctx, &text),
            ws::Message::Binary(bin) => ws::WsWriter::binary(ctx, bin),
            _ => (),
        }
        Self::empty()
    }
}
