use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;
use regex::Regex;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Release, Record, SessionManager};

use super::{MAXSIZE, Transport, SendResult};

const PRELUDE1: &'static str = r#"
<!doctype html>
<html><head>
  <meta http-equiv="X-UA-Compatible" content="IE=edge" />
  <meta http-equiv="Content-Type" content="text/html; charset=UTF-8" />
</head><body><h2>Don't panic!</h2>
  <script>
    document.domain = document.domain;
    var c = parent."#;

const PRELUDE2: &'static str = r#";
    c.start();
    function p(d) {c.message(d);};
    window.onload = function() {c.stop();};
  </script>"#;

const PRELUDE3: [u8; 1024] = [b' '; 1024];


pub struct HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    size: usize,
    maxsize: usize,
    rec: Option<Record>,
}

impl<S, SM> HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn hb(&self, ctx: &mut HttpContext<Self>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            act.send_heartbeat(ctx);
            act.hb(ctx);
        });
    }

    fn write(&mut self, s: &str, ctx: &mut HttpContext<Self>) {
        let b = serde_json::to_string(s).unwrap();
        self.size += b.len() + 25;
        ctx.write("<script>\np(");
        ctx.write(b);
        ctx.write(");\n</script>\r\n");
    }

    pub fn handle(req: &mut HttpRequest,ctx: &mut HttpContext<Self>, maxsize: usize)
                  -> RouteResult<Self>
    {
        lazy_static! {
            static ref CHECK: Regex = Regex::new(r"^[a-zA-Z0-9_\.]+$").unwrap();
        }
        if *req.method() != Method::GET {
            return Reply::reply(httpcodes::HTTPNotFound)
        }

        if let Some(callback) = req.query().remove("c") {
            if !CHECK.is_match(&callback) {
                return Reply::reply(httpcodes::HTTPInternalServerError.with_body(
                    "invalid \"callback\" parameter"))
            }

            let _ = req.load_cookies();
            ctx.start(httpcodes::HTTPOk
                      .builder()
                      .force_close()
                      .content_type("text/html; charset=UTF-8")
                      .sockjs_no_cache()
                      .sockjs_session_cookie(req)
                      .body(Body::Streaming));
            ctx.write(PRELUDE1);
            ctx.write(callback);
            ctx.write(PRELUDE2);
            ctx.write(&PRELUDE3[..]);

            // init transport, but aftre prelude only
            let session = req.match_info().get("session").unwrap().to_owned();
            ctx.drain().map(move |_, _, ctx| {
                ctx.run_later(Duration::new(0, 800000), move |act, ctx| {
                    act.hb(ctx);
                    act.init_transport(session, ctx);
                });
            }).wait(ctx);

            Reply::async(
                HTMLFile{s: PhantomData,
                         sm: PhantomData,
                         size: 0, rec: None, maxsize: maxsize})
        } else {
            Reply::reply(
                httpcodes::HTTPInternalServerError.with_body("\"callback\" parameter required")
            )
        }
    }
}

// Http actor implementation
impl<S, SM> Actor for HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>, SM::Context: ToEnvelope<SM>
{
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        println!("STOPPING, {:?}", self.rec.is_some());
        if let Some(rec) = self.rec.take() {
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, rec: &mut Record) -> SendResult {
        match msg {
            Frame::Heartbeat => {
                self.write("h", ctx);
            },
            Frame::Message(s) => {
                let blob = format!("a[{}]", serde_json::to_string(&s).unwrap());
                self.write(&blob, ctx);
            }
            Frame::MessageVec(s) => {
                self.write(&s, ctx);
            }
            Frame::MessageBlob(_) => {
                unimplemented!()
            }
            Frame::Open => {
                self.write("o", ctx);
            },
            Frame::Close(code) => {
                rec.close();
                let blob = format!("c[{},{:?}]", code.num(), code.reason());
                self.write(&blob, ctx);
            }
        };

        if self.size > self.maxsize {
            ctx.write_eof();
            ctx.stop();
            SendResult::Stop
        } else {
            SendResult::Continue
        }
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode) {
        self.write(&format!("c[{},{:?}]", code.num(), code.reason()), ctx);
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>) {
        self.write("h", ctx);
    }

    fn set_session_record(&mut self, rec: Record) {
        self.rec = Some(rec)
    }
}

impl<S, SM> Route for HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, _: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self> {
        HTMLFile::handle(req, ctx, MAXSIZE)
    }
}

impl<S, SM> StreamHandler<Frame> for HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for HTMLFile<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: Frame, ctx: &mut HttpContext<Self>) -> Response<Self, Frame> {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, msg, &mut rec);
            self.rec = Some(rec);
        } else {
            if let Some(ref mut rec) = self.rec {
                rec.buffer.push_back(msg);
            };
        }
        Self::empty()
    }
}
