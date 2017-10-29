use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use http::header;
use serde_json;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Release, Record, SessionManager};

use super::{MAXSIZE, Transport, SendResult};


pub struct EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    size: usize,
    rec: Option<Record>,
    maxsize: usize,
}

impl<S, SM> EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn hb(&self, ctx: &mut HttpContext<Self>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            act.send_heartbeat(ctx);
            act.hb(ctx);
        });
    }

    pub fn handle(req: &mut HttpRequest,ctx: &mut HttpContext<Self>, maxsize: usize)
                  -> RouteResult<Self>
    {
        let _ = req.load_cookies();
        ctx.start(httpcodes::HTTPOk
                  .builder()
                  .header(header::CONTENT_TYPE, "text/event-stream")
                  .force_close()
                  .sockjs_no_cache()
                  .sockjs_session_cookie(req)
                  .body(Body::Streaming));
        ctx.write("\r\n");

        // init transport, but aftre prelude only
        let session = req.match_info().get("session").unwrap().to_owned();
        ctx.drain().map(move |_, _, ctx| {
            ctx.run_later(Duration::new(0, 800000), move |act, ctx| {
                act.hb(ctx);
                act.init_transport(session, ctx);
            });
        }).wait(ctx);

        Reply::async(
            EventSource{s: PhantomData,
                        sm: PhantomData,
                        size: 0, rec: None, maxsize: maxsize})
    }
}

// Http actor implementation
impl<S, SM> Actor for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>
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
impl<S, SM> Transport<S, SM> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, rec: &mut Record) -> SendResult {
        self.size += match msg {
            Frame::Heartbeat => {
                ctx.write("data: h\r\n\r\n");
                11
            },
            Frame::Message(s) => {
                let blob = serde_json::to_string(&s).unwrap();
                let size = blob.len();
                ctx.write("data: a[");
                ctx.write(blob);
                ctx.write("]\r\n\r\n");
                size + 13
            }
            Frame::MessageVec(s) => {
                let size = s.len();
                ctx.write("data: a");
                ctx.write(s);
                ctx.write("\r\n\r\n");
                size + 11
            }
            Frame::MessageBlob(_) => {
                unimplemented!()
            }
            Frame::Open => {
                ctx.write("data: o\r\n\r\n");
                11
            },
            Frame::Close(code) => {
                rec.close();
                ctx.write(format!("data: c[{}, {:?}]\r\n\r\n", code.num(), code.reason()));
                ctx.write_eof();
                ctx.stop();
                return SendResult::Stop
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
        let blob = format!("data: c[{}, {:?}]\r\n\r\n", code.num(), code.reason());
        ctx.write(blob);
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>) {
        ctx.write("data: h\r\n\r\n");
    }

    fn set_session_record(&mut self, rec: Record) {
        self.rec = Some(rec)
    }
}

impl<S, SM> Route for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, _: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self> {
        EventSource::handle(req, ctx, MAXSIZE)
    }
}

impl<S, SM> StreamHandler<Frame> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for EventSource<S, SM>
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
