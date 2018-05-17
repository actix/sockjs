use std::cell::RefCell;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::rc::Rc;

use actix::{Actor, Addr, Syn};
use actix_web::dev::{AsyncResult, Handler, Resource};
use actix_web::{HttpMessage, HttpRequest, HttpResponse};
use futures::future::Either;
use http::{header, Method};
use md5;
use rand::{self, Rng, ThreadRng};

use context::SockJSContext;
use manager::SessionManager;
use protocol;
use session::Session;
use transports;
use utils::{Info, SockjsHeaders};

/// Sockjs application
///
/// Sockjs application implements sockjs protocol.
pub struct SockJS<A, SM, S = ()>
where
    A: Actor<Context = SockJSContext<A>> + Session,
    SM: SessionManager<A>,
{
    manager: Rc<Addr<Syn, SM>>,
    act: PhantomData<A>,
    state: PhantomData<S>,
    rng: RefCell<ThreadRng>,
    patterns: Vec<Resource>,
    iframe_html: Rc<String>,
    iframe_html_md5: String,
    disabled_transports: HashSet<String>,
    max_size: usize,
    cookie_needed: bool,
}

const ROUTES: [RouteType; 5] = [
    RouteType::Info,
    RouteType::Transport,
    RouteType::RawWebsocket,
    RouteType::IFrame,
    RouteType::IFrame,
];

const PATTERNS: [&str; 5] = [
    "info",
    "{server}/{session}/{transport}",
    "websocket",
    "iframe.html",
    "iframe{version}.html",
];

impl<A, SM, S> SockJS<A, SM, S>
where
    A: Actor<Context = SockJSContext<A>> + Session,
    SM: SessionManager<A>,
    S: 'static,
{
    /// Create new sockjs application. Sockjs application requires
    /// Session manager's address.
    pub fn new(manager: Addr<Syn, SM>) -> Self {
        let html = protocol::IFRAME_HTML.to_owned();
        let digest = md5::compute(&html);
        let patterns: Vec<_> = PATTERNS
            .iter()
            .map(|s| Resource::with_prefix("", s, "", false))
            .collect();

        SockJS {
            patterns,
            act: PhantomData,
            state: PhantomData,
            rng: RefCell::new(rand::thread_rng()),
            manager: Rc::new(manager),
            iframe_html: Rc::new(html),
            iframe_html_md5: format!("{:x}", digest),
            disabled_transports: HashSet::new(),
            max_size: transports::MAXSIZE,
            cookie_needed: false,
        }
    }

    /// Disable specific transports
    pub fn disable_transports<T, I>(mut self, disabled: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
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
    Info,
    Transport,
    IFrame,
    RawWebsocket,
}

impl<A, SM, S> Handler<S> for SockJS<A, SM, S>
where
    A: Actor<Context = SockJSContext<A>> + Session,
    SM: SessionManager<A>,
    S: 'static,
{
    type Result = AsyncResult<HttpResponse>;

    fn handle(&mut self, req: HttpRequest<S>) -> AsyncResult<HttpResponse> {
        let idx = if let Some(path) = req.match_info().get("tail") {
            if path.is_empty() {
                return HttpResponse::Ok()
                    .content_type("text/plain; charset=UTF-8")
                    .body("Welcome to SockJS!\n")
                    .into();
            }

            let mut i = None;
            let mut req2 = req.clone();
            for (idx, pattern) in self.patterns.iter().enumerate() {
                if pattern.match_with_params(path, req2.match_info_mut()) {
                    i = Some(idx)
                }
            }

            if let Some(idx) = i {
                idx
            } else {
                return HttpResponse::NotFound().finish().into();
            }
        } else {
            return HttpResponse::NotFound().finish().into();
        };

        match ROUTES[idx] {
            RouteType::Info => {
                if *req.method() == Method::GET {
                    HttpResponse::Ok()
                        .content_type("application/json;charset=UTF-8")
                        .sockjs_no_cache()
                        .sockjs_cors_headers(req.headers())
                        .json(Info::new(
                            self.rng.borrow_mut().gen::<u32>(),
                            !self.disabled_transports.contains("websocket"),
                            self.cookie_needed,
                        ))
                        .into()
                } else if *req.method() == Method::OPTIONS {
                    HttpResponse::NoContent()
                        .content_type("application/json;charset=UTF-8")
                        .sockjs_cache_headers()
                        .sockjs_allow_methods()
                        .sockjs_cors_headers(req.headers())
                        .sockjs_session_cookie(&req)
                        .finish()
                        .into()
                } else {
                    HttpResponse::MethodNotAllowed().finish().into()
                }
            }
            RouteType::IFrame => {
                if req.headers().contains_key(header::IF_NONE_MATCH) {
                    HttpResponse::NotModified()
                        .content_type("")
                        .sockjs_cache_headers()
                        .finish()
                        .into()
                } else {
                    HttpResponse::Ok()
                        .content_type("text/html;charset=UTF-8")
                        .header(header::ETAG, self.iframe_html_md5.as_str())
                        .sockjs_cache_headers()
                        .body(&self.iframe_html)
                        .into()
                }
            }
            RouteType::Transport => {
                let req2 = req.change_state(Rc::clone(&self.manager));
                let tr = req.match_info().get("transport").unwrap().to_owned();
                if self.disabled_transports.contains(&tr) {
                    return HttpResponse::NotFound().finish().into();
                }

                // check valid session and server params
                {
                    let sid = req.match_info().get("session").unwrap();
                    let server = req.match_info().get("server").unwrap();
                    if sid.is_empty() || sid.contains('.') || server.contains('.') {
                        return HttpResponse::NotFound().finish().into();
                    }
                    trace!(
                        "sockjs transport: {}, session: {}, srv: {}",
                        tr,
                        sid,
                        server
                    );
                }

                if tr == "websocket" {
                    transports::Websocket::<A, _>::init(req2).into()
                } else if tr == "xhr_streaming" {
                    transports::XhrStreaming::<A, _>::init(req2, self.max_size).into()
                } else if tr == "xhr" {
                    transports::Xhr::<A, _>::init(req2).into()
                } else if tr == "xhr_send" {
                    match transports::XhrSend(req2) {
                        Either::A(resp) => resp.into(),
                        Either::B(fut) => fut.into(),
                    }
                } else if tr == "htmlfile" {
                    transports::HTMLFile::<A, _>::init(req2, self.max_size).into()
                } else if tr == "eventsource" {
                    transports::EventSource::<A, _>::init(req2, self.max_size).into()
                } else if tr == "jsonp" {
                    transports::JSONPolling::<A, _>::init(req2).into()
                } else if tr == "jsonp_send" {
                    match transports::JSONPollingSend(req2) {
                        Either::A(resp) => resp.into(),
                        Either::B(fut) => fut.into(),
                    }
                } else {
                    HttpResponse::NotFound().finish().into()
                }
            }
            RouteType::RawWebsocket => {
                transports::RawWebsocket::init(req.change_state(Rc::clone(&self.manager))).into()
            }
        }
    }
}
