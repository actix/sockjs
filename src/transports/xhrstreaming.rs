use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use http::header::ACCESS_CONTROL_ALLOW_METHODS;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Record, SessionManager};

use super::{MAXSIZE, Transport, SendResult};


const OPEN_SEQ: &'static str =
    "hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\
     hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\n";

pub struct XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    size: usize,
    maxsize: usize,
    rec: Option<Record>,
}

impl<S, SM> XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>
{
    pub fn handle(req: &mut HttpRequest, ctx: &mut HttpContext<Self>, maxsize: usize)
                  -> RouteResult<Self>
    {
        if *req.method() == Method::OPTIONS {
            let _ = req.load_cookies();
            return Reply::reply(
                httpcodes::HTTPNoContent
                    .builder()
                    .content_type("application/jsonscript; charset=UTF-8")
                    .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, POST")
                    .sockjs_cache_headers()
                    .sockjs_cors_headers(req.headers())
                    .sockjs_session_cookie(req)
                    .finish())
        }
        else if *req.method() != Method::POST {
            return Reply::reply(httpcodes::HTTPNotFound)
        }

        let _ = req.load_cookies();
        ctx.start(
            httpcodes::HTTPOk.builder()
                .content_type("application/javascript; charset=UTF-8")
                .force_close()
                .sockjs_no_cache()
                .sockjs_session_cookie(req)
                .sockjs_cors_headers(req.headers())
                .if_true(
                    req.version() == Version::HTTP_11, |builder| {builder.enable_chunked();})
                .body(Body::Streaming));
        ctx.write(OPEN_SEQ);

        // init transport, but aftre prelude only
        let session = req.match_info().get("session").unwrap().to_owned();
        ctx.drain().map(move |_, _, ctx| {
            ctx.run_later(Duration::new(0, 800_000), move |act, ctx| {
                act.init_transport(session, ctx);
            });
        }).wait(ctx);

        Reply::async(
            XhrStreaming{s: PhantomData,
                         sm: PhantomData,
                         size: 0,
                         maxsize: maxsize,
                         rec: None})
    }
}

// Http actor implementation
impl<S, SM> Actor for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        self.stop(ctx);
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult
    {
        self.size += match msg {
            Frame::Heartbeat => {
                ctx.write("h\n");
                2
            },
            Frame::Message(s) => {
                let s = format!("a[{:?}]\n", s);
                let size = s.len();
                ctx.write(s);
                size
            }
            Frame::MessageVec(s) => {
                let s = format!("a{}\n", s);
                let size = s.len();
                ctx.write(s);
                size
            }
            Frame::MessageBlob(_) => {
                // ctx.write(format!("a{}\n", s));
                0
            }
            Frame::Open => {
                ctx.write("o\n");
                2
            },
            Frame::Close(code) => {
                record.close();
                ctx.write(format!("c[{},{:?}]\n", code.num(), code.reason()));
                ctx.write_eof();
                return SendResult::Stop;
            }
        };

        if self.size > self.maxsize {
            ctx.write_eof();
            SendResult::Stop
        } else {
            SendResult::Continue
        }
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>) {
        ctx.write("h\n");
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode) {
        ctx.write(format!("c[{},{:?}]\n", code.num(), code.reason()));
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }
}

impl<S, SM> Route for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, _: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        XhrStreaming::handle(req, ctx, MAXSIZE)
    }
}

impl<S, SM> StreamHandler<Frame> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: Frame, ctx: &mut HttpContext<Self>) -> Response<Self, Frame> {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, msg, &mut rec);
            self.rec = Some(rec);
        } else if let Some(ref mut rec) = self.rec {
            rec.buffer.push_back(msg);
        }
        Self::empty()
    }
}
