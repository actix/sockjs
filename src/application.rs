use std::rc::Rc;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::collections::HashSet;

use md5;
use rand::{self, Rng, ThreadRng};
use http::{header, Method};
use actix::dev::*;
use actix_web::dev::*;

use protocol;
use transports;
use context::SockJSContext;
use session::Session;
use manager::SessionManager;
use utils::{Info, SockjsHeaders};

/// Sockjs application
///
/// Sockjs application implements sockjs protocol.
pub struct SockJS<A, SM, S=()>
    where A: Actor<Context=SockJSContext<A>> + Session,
          SM: SessionManager<A>,
{
    manager: Rc<SyncAddress<SM>>,
    act: PhantomData<A>,
    state: PhantomData<S>,
    prefix: usize,
    rng: RefCell<ThreadRng>,
    router: RouteRecognizer<RouteType>,
    iframe_html: Rc<String>,
    iframe_html_md5: String,
    disabled_transports: HashSet<String>,
    max_size: usize,
    cookie_needed: bool,
}

impl<A, SM, S> SockJS<A, SM, S>
    where A: Actor<Context=SockJSContext<A>> + Session,
          SM: SessionManager<A>,
          S: 'static,
{
    /// Create new sockjs application. Sockjs application requires
    /// Session manager's address.
    pub fn new(manager: SyncAddress<SM>) -> Self
    {
        let routes = vec![
            ("/", RouteType::Greeting),
            ("/info", RouteType::Info),
            ("/websocket", RouteType::Websocket),
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
            disabled_transports: HashSet::new(),
            max_size: transports::MAXSIZE,
            cookie_needed: false,
        }
    }

    /// Disable specific transports
    pub fn disable_transports<T, I>(mut self, disabled: I) -> Self
        where T: Into<String>, I: IntoIterator<Item = T>
    {
        for i in disabled {
            self.disabled_transports.insert(i.into());
        }
        self
    }

    /// Set max size for single streaming request (EventSource, XhrStreamimng).
    pub fn maxsize(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Set cookie needed param
    pub fn cookie_needed(mut self, val: bool) -> Self {
        self.cookie_needed = val;
        self
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

#[doc(hidden)]
impl<A, SM, S> RouteHandler<S> for SockJS<A, SM, S>
    where A: Actor<Context=SockJSContext<A>> + Session,
          SM: SessionManager<A>,
          S: 'static
{
    fn handle(&self, req: &mut HttpRequest, payload: Payload, _: Rc<S>) -> Task {
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
                    return if *req.method() == Method::GET {
                        Task::reply(
                            httpcodes::HTTPOk
                                .builder()
                                .content_type("application/json;charset=UTF-8")
                                .sockjs_no_cache()
                                .sockjs_cors_headers(req.headers())
                                .json_body(Info::new(
                                    self.rng.borrow_mut().gen::<u32>(),
                                    !self.disabled_transports.contains("websocket"),
                                    self.cookie_needed)))
                    } else if *req.method() == Method::OPTIONS {
                        let _ = req.load_cookies();
                        Task::reply(
                            httpcodes::HTTPNoContent
                                .builder()
                                .content_type("application/json;charset=UTF-8")
                                .sockjs_cache_headers()
                                .sockjs_allow_methods()
                                .sockjs_cors_headers(req.headers())
                                .sockjs_session_cookie(req)
                                .finish()
                        )
                    } else {
                        Task::reply(httpcodes::HTTPMethodNotAllowed)
                    };
                },
                RouteType::IFrame => {
                    let resp = if req.headers().contains_key(header::IF_NONE_MATCH) {
                        httpcodes::HTTPNotModified
                            .builder()
                            .content_type("")
                            .sockjs_cache_headers()
                            .finish()
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
                RouteType::Transport => {
                    if let Some(params) = params {
                        req.set_match_info(params);
                    }

                    let tr = req.match_info().get("transport").unwrap().to_owned();
                    if self.disabled_transports.contains(&tr) {
                        return Task::reply(httpcodes::HTTPNotFound)
                    }

                    // check valid session and server params
                    {
                        let sid = req.match_info().get("session").unwrap();
                        let server = req.match_info().get("server").unwrap();
                        if sid.is_empty() || sid.contains('.') || server.contains('.') {
                            return Task::reply(httpcodes::HTTPNotFound)
                        }
                    }

                    let res = {
                        if tr == "websocket" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::Websocket::<A, _>::request(req, payload, &mut ctx)
                                .map(|r| r.into(ctx))
                        } else if tr == "xhr_streaming" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::XhrStreaming::<A, _>
                                ::handle(req, &mut ctx, self.max_size)
                                .map(|r| r.into(ctx))
                        } else if tr == "xhr" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::Xhr::<A, _>::request(req, payload, &mut ctx)
                                .map(|r| r.into(ctx))
                        } else if tr == "xhr_send" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::XhrSend::<A, _>::request(req, payload, &mut ctx)
                                .map(|r| r.into(ctx))
                        } else if tr == "htmlfile" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::HTMLFile::<A, _>
                                ::handle(req, &mut ctx, self.max_size)
                                .map(|r| r.into(ctx))
                        } else if tr == "eventsource" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::EventSource::<A, _>
                                ::handle(req, &mut ctx, self.max_size)
                                .map(|r| r.into(ctx))
                        } else if tr == "jsonp" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::JSONPolling::<A, _>::request(req, payload, &mut ctx)
                                .map(|r| r.into(ctx))
                        } else if tr == "jsonp_send" {
                            let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                            transports::JSONPollingSend::<A, _>::request(req, payload, &mut ctx)
                                .map(|r| r.into(ctx))
                        } else {
                            return Task::reply(httpcodes::HTTPNotFound)
                        }
                    };
                    match res {
                        Ok(resp) => return resp,
                        Err(err) => return Task::reply(err),
                    }
                },
                RouteType::Websocket => {
                    let mut ctx = HttpContext::new(Rc::clone(&self.manager));
                    match transports::RawWebsocket::request(req, payload, &mut ctx)
                        .map(|r| r.into(ctx))
                    {
                        Ok(resp) => return resp,
                        Err(err) => return Task::reply(err),
                    }
                },
            }
        }
        Task::reply(httpcodes::HTTPNotFound)
    }

    fn set_prefix(&mut self, prefix: String) {
        self.prefix = prefix.len();
        self.router.set_prefix(prefix);
    }
}
