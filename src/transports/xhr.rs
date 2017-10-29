use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;
use http::header::{self, ACCESS_CONTROL_ALLOW_METHODS};

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::Session;
use manager::{Release, Record, SessionManager};

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
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        if let Some(rec) = self.rec.take() {
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for Xhr<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult
    {
        match msg {
            Frame::Heartbeat => {
                ctx.write("h\n");
            },
            Frame::Message(s) => {
                ctx.write("a[");
                ctx.write(serde_json::to_string(&s).unwrap());
                ctx.write("]\n");
            }
            Frame::MessageVec(s) => {
                ctx.write(format!("a{}\n", s));
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
        ctx.stop();
        SendResult::Stop
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>)
    {
        ctx.write("h\n");
        ctx.write_eof();
        ctx.stop();
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode)
    {
        ctx.write(format!("c[{},{:?}]\n", code.num(), code.reason()));
        ctx.write_eof();
        ctx.stop();
    }

    fn set_session_record(&mut self, record: Record) {
        self.rec = Some(record);
    }
}

impl<S, SM> Route for Xhr<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, _: Payload, ctx: &mut HttpContext<Self>)
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
                    .sockjs_session_cookie(&req)
                    .finish())
        }
        else if *req.method() != Method::POST {
            return Reply::reply(httpcodes::HTTPNotFound)
        }

        let _ = req.load_cookies();
        ctx.start(httpcodes::HTTPOk
                  .builder()
                  .header(header::CONTENT_TYPE, "application/javascript; charset=UTF-8")
                  .force_close()
                  .sockjs_no_cache()
                  .sockjs_session_cookie(req)
                  .sockjs_cors_headers(req.headers())
                  .body(Body::Streaming));

        let session = req.match_info().get("session").unwrap().to_owned();
        let mut transport = Xhr{s: PhantomData,
                                sm: PhantomData,
                                rec: None};
        // init transport
        transport.init_transport(session, ctx);

        Reply::async(transport)
    }
}

impl<S, SM> StreamHandler<Frame> for Xhr<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for Xhr<S, SM>
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
