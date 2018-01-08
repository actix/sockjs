use std::rc::Rc;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::collections::HashSet;

use md5;
use regex::RegexSet;
use rand::{self, Rng, ThreadRng};
use http::{header, Method};
use futures::future::Either;
use actix::{Actor, SyncAddress};
use actix_web::dev::{Pattern, Handler};
use actix_web::{httpcodes, HttpRequest, Reply};

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
    rng: RefCell<ThreadRng>,
    router: RegexSet,
    patterns: Vec<Pattern>,
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
    RouteType::IFrame];

const PATTERNS: [&'static str; 5] = [
    "info",
    "{server}/{session}/{transport}",
    "^websocket",
    "iframe.html",
    "iframe{version}.html"];


impl<A, SM, S> SockJS<A, SM, S>
    where A: Actor<Context=SockJSContext<A>> + Session,
          SM: SessionManager<A>,
          S: 'static,
{
    /// Create new sockjs application. Sockjs application requires
    /// Session manager's address.
    pub fn new(manager: SyncAddress<SM>) -> Self
    {
        let html = protocol::IFRAME_HTML.to_owned();
        let digest = md5::compute(&html);
        let patterns: Vec<_> = PATTERNS.iter().map(|s| Pattern::new("", s, "")).collect();
        let regset = RegexSet::new(patterns.iter().map(|p| p.pattern())).unwrap();

        SockJS { act: PhantomData,
                 state: PhantomData,
                 rng: RefCell::new(rand::thread_rng()),
                 manager: Rc::new(manager),
                 router: regset,
                 patterns: patterns,
                 iframe_html: Rc::new(html),
                 iframe_html_md5: format!("{:x}", digest),
                 disabled_transports: HashSet::new(),
                 max_size: transports::MAXSIZE,
                 cookie_needed: false }
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
    Info,
    Transport,
    IFrame,
    RawWebsocket,
}

impl<A, SM, S> Handler<S> for SockJS<A, SM, S>
    where A: Actor<Context=SockJSContext<A>> + Session,
          SM: SessionManager<A>,
          S: 'static
{
    type Result = Reply;

    fn handle(&mut self, mut req: HttpRequest<S>) -> Reply {
        let idx = if let Some(path) = req.match_info().get("tail") {
            if path.is_empty() {
                return httpcodes::HTTPOk
                    .build()
                    .content_type("text/plain; charset=UTF-8")
                    .body("Welcome to SockJS!\n").into()
            }

            if let Some(idx) = self.router.matches(path).into_iter().next() {
                idx
            } else {
                return httpcodes::HTTPNotFound.into()
            }
        } else {
            return httpcodes::HTTPNotFound.into()
        };

        match ROUTES[idx] {
            RouteType::Info => {
                if *req.method() == Method::GET {
                    httpcodes::HTTPOk
                        .build()
                        .content_type("application/json;charset=UTF-8")
                        .sockjs_no_cache()
                        .sockjs_cors_headers(req.headers())
                        .json(Info::new(
                            self.rng.borrow_mut().gen::<u32>(),
                            !self.disabled_transports.contains("websocket"),
                            self.cookie_needed)).into()
                } else if *req.method() == Method::OPTIONS {
                    httpcodes::HTTPNoContent
                        .build()
                        .content_type("application/json;charset=UTF-8")
                        .sockjs_cache_headers()
                        .sockjs_allow_methods()
                        .sockjs_cors_headers(req.headers())
                        .sockjs_session_cookie(&req)
                        .finish().into()
                } else {
                    httpcodes::HTTPMethodNotAllowed.into()
                }
            },
            RouteType::IFrame => {
                if req.headers().contains_key(header::IF_NONE_MATCH) {
                    httpcodes::HTTPNotModified
                        .build()
                        .content_type("")
                        .sockjs_cache_headers()
                        .finish()
                        .into()
                } else {
                    httpcodes::HTTPOk
                        .build()
                        .content_type("text/html;charset=UTF-8")
                        .header(header::ETAG, self.iframe_html_md5.as_str())
                        .sockjs_cache_headers()
                        .body(&self.iframe_html)
                        .into()
                }
            },
            RouteType::Transport => {
                let req2 = req.change_state(self.manager.clone());
                self.patterns[idx].update_match_info(&mut req, req2.prefix_len());

                let tr = req.match_info().get("transport").unwrap().to_owned();
                if self.disabled_transports.contains(&tr) {
                    return httpcodes::HTTPNotFound.into()
                }

                // check valid session and server params
                {
                    let sid = req.match_info().get("session").unwrap();
                    let server = req.match_info().get("server").unwrap();
                    if sid.is_empty() || sid.contains('.') || server.contains('.') {
                        return httpcodes::HTTPNotFound.into()
                    }
                    trace!("sockjs transport: {}, session: {}, srv: {}", tr, sid, server);
                }

                if tr == "websocket" {
                    transports::Websocket::<A, _>::init(req2).into()
                } else if tr == "xhr_streaming" {
                    transports::XhrStreaming::<A, _>::init(req2, self.max_size).into()
                } else if tr == "xhr" {
                    transports::Xhr::<A, _>::init(req2).into()
                } else if tr == "xhr_send" {
                    match transports::XhrSend(req2).into() {
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
                    httpcodes::HTTPNotFound.into()
                }
            },
            RouteType::RawWebsocket =>
                transports::RawWebsocket::init(req.change_state(self.manager.clone())).into(),
        }
    }
}
