use time;
use actix_web::HttpRequest;
use actix_web::headers::Cookie;
use actix_web::dev::HttpResponseBuilder;
use http::header::HeaderMap;
use http::header::{EXPIRES, ORIGIN, CACHE_CONTROL,
                   ACCESS_CONTROL_ALLOW_ORIGIN,
                   ACCESS_CONTROL_ALLOW_HEADERS,
                   ACCESS_CONTROL_ALLOW_METHODS,
                   ACCESS_CONTROL_ALLOW_CREDENTIALS,
                   ACCESS_CONTROL_MAX_AGE,
                   ACCESS_CONTROL_REQUEST_HEADERS};

const CACHE_CONTROL_VAL: &str =
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
            entropy,
            websocket,
            cookie_needed: cookie_needed,
            origins: vec!["*:*"],
        }
    }
}


pub(crate) trait SockjsHeaders {

    fn sockjs_allow_methods(&mut self) -> &mut Self;

    fn sockjs_no_cache(&mut self) -> &mut Self;

    fn sockjs_cache_headers(&mut self) -> &mut Self;

    fn sockjs_cors_headers(&mut self, headers: &HeaderMap) -> &mut Self;

    fn sockjs_session_cookie<S>(&mut self, req: &HttpRequest<S>) -> &mut Self;

}


impl SockjsHeaders for HttpResponseBuilder {

    fn sockjs_session_cookie<S>(&mut self, req: &HttpRequest<S>) -> &mut Self {
        let builder = if let Some(cookie) = req.cookie("JSESSIONID") {
            Cookie::build("JSESSIONID", cookie.value().to_owned())
        } else {
            Cookie::build("JSESSIONID", "dummy")
        };
        self.cookie(builder.path("/").finish());
        self
    }

    fn sockjs_allow_methods(&mut self) -> &mut Self {
        self.header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, GET")
    }

    fn sockjs_no_cache(&mut self) -> &mut Self {
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
        const TD365_SECONDS: &str = "31536000";
        const TD365_SECONDS_CC: &str  = "max-age=31536000, public";

        let d = time::now() + time::Duration::days(365);

        self.header(CACHE_CONTROL, TD365_SECONDS_CC);
        self.header(ACCESS_CONTROL_MAX_AGE, TD365_SECONDS);
        self.header(EXPIRES, time::strftime("%a, %d %b %Y %H:%M:%S", &d).unwrap().as_str());

        self
    }
}
