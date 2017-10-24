use actix_web::{Body, Cookie, HttpRequest, HttpResponse, HttpResponseBuilder};
use http::Error;
use http::header::{self, HeaderMap, HeaderValue};
use http::header::{ORIGIN,
                   ACCESS_CONTROL_ALLOW_ORIGIN,
                   ACCESS_CONTROL_ALLOW_HEADERS,
                   ACCESS_CONTROL_ALLOW_METHODS,
                   ACCESS_CONTROL_REQUEST_HEADERS,
                   ACCESS_CONTROL_ALLOW_CREDENTIALS};
use serde::Serialize;
use serde_json;


const CACHE_CONTROL: &'static str =
    "no-store, no-cache, no-transform, must-revalidate, max-age=0";

#[derive(Serialize)]
pub(crate) struct Info {
    entropy: u32,
    websocket: bool,
    cookie_needed: bool,
    origins: Vec<&'static str>,
}

impl Info {

    pub fn new(entropy: u32, websocket: bool, cookie_needed: bool) -> Info {
        Info {
            entropy: entropy,
            websocket: websocket,
            cookie_needed: cookie_needed,
            origins: vec!["*:*"],
        }
    }
}


pub(crate) trait SockjsHeaders {

    fn json_body<T: Serialize>(&mut self, body: T) -> Result<HttpResponse, Error>;

    fn sockjs_allow_methods(&mut self) -> &mut Self;

    fn sockjs_cache_control(&mut self) -> &mut Self;

    fn sockjs_cors_headers(&mut self, headers: &HeaderMap) -> &mut Self;

    fn sockjs_session_cookie(&mut self, req: &HttpRequest) -> &mut Self;

}


impl SockjsHeaders for HttpResponseBuilder {

    fn json_body<T: Serialize>(&mut self, body: T) -> Result<HttpResponse, Error> {
        let serialized = serde_json::to_string(&body).unwrap();
        self.body(Body::Binary(serialized.into_bytes().into()))
    }

    fn sockjs_session_cookie(&mut self, req: &HttpRequest) -> &mut Self {
        if req.cookie("JSESSIONID").is_none() {
            self.cookie(Cookie::build("JSESSIONID", "dummy")
                        .path("/")
                        .finish());
        }
        self
    }

    fn sockjs_allow_methods(&mut self) -> &mut Self {
        self.header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, GET")
    }

    fn sockjs_cache_control(&mut self) -> &mut Self {
        self.header(header::CACHE_CONTROL, CACHE_CONTROL)
    }

    fn sockjs_cors_headers(&mut self, headers: &HeaderMap) -> &mut Self {
        if let Some(origin) = headers.get(ORIGIN) {
            self.header(ACCESS_CONTROL_ALLOW_ORIGIN, origin.as_ref());
            if origin != "*" {
                self.header(ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
            }
        } else {
            self.header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");
        }

        if let Some(ac) = headers.get(ACCESS_CONTROL_REQUEST_HEADERS) {
            self.header(ACCESS_CONTROL_ALLOW_HEADERS, ac.as_ref());
        }

        self
    }
}
