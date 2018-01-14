use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use http::header::ACCESS_CONTROL_ALLOW_METHODS;

use context::ChannelItem;
use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Broadcast, Record, SessionManager};

use super::{Transport, SendResult, Flags};


const OPEN_SEQ: &str =
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
    flags: Flags,
    rec: Option<Record>,
}

impl<S, SM> XhrStreaming<S, SM> where S: Session, SM: SessionManager<S> {

    pub fn init(req: HttpRequest<SyncAddress<SM>>, maxsize: usize) -> Result<HttpResponse> {
        if *req.method() == Method::OPTIONS {
            return Ok(
                httpcodes::HTTPNoContent
                    .build()
                    .content_type("application/jsonscript; charset=UTF-8")
                    .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, POST")
                    .sockjs_cache_headers()
                    .sockjs_cors_headers(req.headers())
                    .sockjs_session_cookie(&req)
                    .finish()?)
        } else if *req.method() != Method::POST {
            return Ok(httpcodes::HTTPNotFound.into())
        }

        let session = req.match_info().get("session").unwrap().to_owned();
        let mut resp = httpcodes::HTTPOk.build()
            .content_type("application/javascript; charset=UTF-8")
            .force_close()
            .sockjs_no_cache()
            .sockjs_session_cookie(&req)
            .sockjs_cors_headers(req.headers())
            .take();

        let mut ctx = HttpContext::new(
            req, XhrStreaming{s: PhantomData,
                              sm: PhantomData,
                              size: 0,
                              maxsize: maxsize,
                              flags: Flags::empty(),
                              rec: None});
        ctx.write(OPEN_SEQ);

        // init transport, but aftre prelude only
        ctx.drain().map(move |_, _, ctx| {
            ctx.run_later(Duration::new(0, 1_200_000), move |act, ctx| {
                act.init_transport(session, ctx);
            });
        }).wait(&mut ctx);

        Ok(resp.body(ctx)?)
    }
}

// Http actor implementation
impl<S, SM> Actor for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self, SyncAddress<SM>>;

    fn stopping(&mut self, ctx: &mut Self::Context) -> bool {
        self.release(ctx);
        true
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut Self::Context, msg: &Frame, record: &mut Record) -> SendResult
    {
        self.size += match *msg {
            Frame::Heartbeat => {
                ctx.write("h\n");
                2
            },
            Frame::Message(ref s) => {
                let s = format!("a[{:?}]\n", s);
                let size = s.len();
                ctx.write(s);
                size
            }
            Frame::MessageVec(ref s) => {
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

    fn send_heartbeat(&mut self, ctx: &mut Self::Context) {
        ctx.write("h\n");
    }

    fn send_close(&mut self, ctx: &mut Self::Context, code: CloseCode) {
        ctx.write(format!("c[{},{:?}]\n", code.num(), code.reason()));
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }

    fn flags(&mut self) -> &mut Flags {
        &mut self.flags
    }
}

impl<S, SM> Handler<ChannelItem> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: ChannelItem, ctx: &mut Self::Context) {
        self.handle_message(msg, ctx)
    }
}

impl<S, SM> Handler<Broadcast> for XhrStreaming<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: Broadcast, ctx: &mut Self::Context) {
        self.handle_broadcast(msg, ctx)
    }
}
