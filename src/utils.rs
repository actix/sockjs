use std::time::Duration;
use actix_web::{Body, Cookie, HttpRequest, HttpResponse, HttpResponseBuilder};
use http::Error;
use http::header::{self, HeaderMap, HeaderValue};
use http::header::{EXPIRES, ORIGIN, CACHE_CONTROL,
                   ACCESS_CONTROL_ALLOW_ORIGIN,
                   ACCESS_CONTROL_ALLOW_HEADERS,
                   ACCESS_CONTROL_ALLOW_METHODS,
                   ACCESS_CONTROL_ALLOW_CREDENTIALS,
                   ACCESS_CONTROL_MAX_AGE,
                   ACCESS_CONTROL_REQUEST_HEADERS};
use serde::Serialize;
use serde_json;
use time;


const CACHE_CONTROL_VAL: &'static str =
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

    fn sockjs_cache_headers(&mut self) -> &mut Self;

    fn sockjs_cors_headers(&mut self, headers: &HeaderMap) -> &mut Self;

    fn sockjs_session_cookie(&mut self, req: &HttpRequest) -> &mut Self;

}


impl SockjsHeaders for HttpResponseBuilder {

    fn json_body<T: Serialize>(&mut self, body: T) -> Result<HttpResponse, Error> {
        let serialized = serde_json::to_string(&body).unwrap();
        self.body(serialized.into_bytes())
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
        self.header(CACHE_CONTROL, CACHE_CONTROL_VAL)
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

    fn sockjs_cache_headers(&mut self) -> &mut Self {
        const TD365_SECONDS: &'static str = "31536000";
        const TD365_SECONDS_CC: &'static str  = "max-age=31536000, public";

        let d = time::now() + time::Duration::days(365);

        self.header(CACHE_CONTROL, TD365_SECONDS_CC);
        self.header(ACCESS_CONTROL_MAX_AGE, TD365_SECONDS);
        self.header(EXPIRES, time::strftime("%a, %d %b %Y %H:%M:%S", &d).unwrap().as_str());

        self
    }
}
