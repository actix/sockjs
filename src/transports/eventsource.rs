use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use http::header;
use serde_json;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Broadcast, Record, SessionManager};

use super::{Transport, SendResult};


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
    fn hb(&self, ctx: &mut HttpContext<Self, SyncAddress<SM>>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            act.send_heartbeat(ctx);
            act.hb(ctx);
        });
    }

    pub fn init(req: HttpRequest<SyncAddress<SM>>, maxsize: usize) -> Result<HttpResponse>
    {
        let session = req.match_info().get("session").unwrap().to_owned();
        let mut resp = httpcodes::HTTPOk.build()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .force_close()
            .sockjs_no_cache()
            .sockjs_session_cookie(&req)
            .take();

        let mut ctx = HttpContext::new(
            req, EventSource{s: PhantomData,
                             sm: PhantomData,
                             size: 0, rec: None, maxsize: maxsize});
        ctx.write("\r\n");

        // init transport, but aftre prelude only
        ctx.drain().map(move |_, _, ctx| {
            ctx.run_later(Duration::new(0, 1_200_000), move |act, ctx| {
                act.hb(ctx);
                act.init_transport(session, ctx);
            });
        }).wait(&mut ctx);

        Ok(resp.body(ctx)?)
    }
}

// Http actor implementation
impl<S, SM> Actor for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self, SyncAddress<SM>>;

    fn stopping(&mut self, ctx: &mut Self::Context) {
        self.stop(ctx);
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut Self::Context, msg: &Frame, rec: &mut Record)
            -> SendResult
    {
        self.size += match *msg {
            Frame::Heartbeat => {
                ctx.write("data: h\r\n\r\n");
                11
            },
            Frame::Message(ref s) => {
                let blob = serde_json::to_string(&s).unwrap();
                let size = blob.len();
                ctx.write("data: a[");
                ctx.write(blob);
                ctx.write("]\r\n\r\n");
                size + 13
            }
            Frame::MessageVec(ref s) => {
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
                return SendResult::Stop
            }
        };

        if self.size > self.maxsize {
            ctx.write_eof();
            SendResult::Stop
        } else {
            SendResult::Continue
        }
    }

    fn send_close(&mut self, ctx: &mut Self::Context, code: CloseCode) {
        let blob = format!("data: c[{}, {:?}]\r\n\r\n", code.num(), code.reason());
        ctx.write(blob);
    }

    fn send_heartbeat(&mut self, ctx: &mut Self::Context) {
        ctx.write("data: h\r\n\r\n");
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }
}

impl<S, SM> StreamHandler<Frame> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: Frame, ctx: &mut Self::Context) {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, &msg, &mut rec);
            self.rec = Some(rec);
        } else if let Some(ref mut rec) = self.rec {
            rec.add(msg);
        }
    }
}

impl<S, SM> Handler<Broadcast> for EventSource<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: Broadcast, ctx: &mut Self::Context) {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, &msg.msg, &mut rec);
            self.rec = Some(rec);
        } else if let Some(ref mut rec) = self.rec {
            rec.add(msg);
        }
    }
}
