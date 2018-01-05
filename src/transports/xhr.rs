use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;
use http::header::{self, ACCESS_CONTROL_ALLOW_METHODS};

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Broadcast, Record, SessionManager};

use super::{Transport, SendResult};


pub struct Xhr<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
}

// Http actor implementation
impl<S, SM> Actor for Xhr<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self, SyncAddress<SM>>;

    fn stopping(&mut self, ctx: &mut Self::Context) {
        self.stop(ctx)
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for Xhr<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut Self::Context, msg: &Frame, record: &mut Record) -> SendResult
    {
        match *msg {
            Frame::Heartbeat => {
                ctx.write("h\n");
            },
            Frame::Message(ref s) => {
                ctx.write("a[");
                ctx.write(serde_json::to_string(s).unwrap());
                ctx.write("]\n");
            }
            Frame::MessageVec(ref s) => {
                ctx.write("a");
                ctx.write(s);
                ctx.write("\n");
            }
            Frame::MessageBlob(_) => {
                unimplemented!()
            }
            Frame::Open => {
                ctx.write("o\n");
            },
            Frame::Close(code) => {
                record.close();
                let blob = format!("c[{},{:?}]\n", code.num(), code.reason());
                ctx.write(blob);
            }
        };

        ctx.write_eof();
        SendResult::Stop
    }

    fn send_heartbeat(&mut self, ctx: &mut Self::Context) {
        ctx.write("h\n");
        ctx.write_eof();
    }

    fn send_close(&mut self, ctx: &mut Self::Context, code: CloseCode) {
        ctx.write(format!("c[{},{:?}]\n", code.num(), code.reason()));
        ctx.write_eof();
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }
}

impl<S, SM> Xhr<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    pub fn init(req: HttpRequest<SyncAddress<SM>>) -> Result<HttpResponse>
    {
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
        }
        else if *req.method() != Method::POST {
            return Ok(httpcodes::HTTPNotFound.into())
        }

        let session = req.match_info().get("session").unwrap().to_owned();
        let mut resp = httpcodes::HTTPOk
            .build()
            .header(header::CONTENT_TYPE, "application/javascript; charset=UTF-8")
            .force_close()
            .sockjs_no_cache()
            .sockjs_session_cookie(&req)
            .sockjs_cors_headers(req.headers())
            .take();

        let mut ctx = HttpContext::from_request(req);

        // init transport
        let mut transport = Xhr{s: PhantomData,
                                sm: PhantomData,
                                rec: None};
        transport.init_transport(session, &mut ctx);

        Ok(resp.body(ctx.actor(transport))?)
    }
}

impl<S, SM> StreamHandler<Frame> for Xhr<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for Xhr<S, SM>
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

impl<S, SM> Handler<Broadcast> for Xhr<S, SM>
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
