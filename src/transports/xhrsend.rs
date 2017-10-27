use std::io;
use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use actix_web::dev::*;
use bytes::BytesMut;
use http::header::{self, ACCESS_CONTROL_ALLOW_METHODS};

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use context::SockJSContext;
use session::{Message, Session, SessionState};
use manager::{Acquire, Release, Record, SessionManager, SessionMessage};

use super::{MAXSIZE, Transport, SendResult};


pub struct XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    buf: BytesMut,
    sid: Option<String>,
}

// Http actor implementation
impl<S, SM> Actor for XhrSend<S, SM>
    where S: Session,
          SM: SessionManager<S>,
          SM::Context: ToEnvelope<SM>
{
    type Context = HttpContext<Self>;
}

impl<S, SM> Route for XhrSend<S, SM>
    where S: Session,
          SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        if *req.method() == Method::OPTIONS {
            let _ = req.load_cookies();
            Reply::reply(
                httpcodes::HTTPNoContent
                    .builder()
                    .content_type("application/jsonscript; charset=UTF-8")
                    .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, GET")
                    .sockjs_cache_control()
                    .sockjs_cors_headers(req.headers())
                    .sockjs_session_cookie(&req)
                    .finish())
        }
        else if *req.method() != Method::GET && *req.method() != Method::POST {
            Reply::reply(
                httpcodes::HTTPForbidden.with_reason("Method is not allowed"))
        } else {
            let _ = req.load_cookies();
            let resp = httpcodes::HTTPNoContent
                .builder()
                .content_type("text/plain; charset=UTF-8")
                .sockjs_cache_control()
                .sockjs_cors_headers(req.headers())
                .sockjs_session_cookie(&req)
                .finish()?;
            ctx.start(resp);
            ctx.add_stream(payload);

            Reply::async(XhrSend{
                s: PhantomData,
                sm: PhantomData,
                buf: BytesMut::new(),
                sid: Some(req.match_info().get("session").unwrap().to_owned()),
            })
        }
    }
}

impl<S, SM> StreamHandler<PayloadItem, PayloadError> for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn finished(&mut self, ctx: &mut Self::Context) {
        if let Some(sid) = self.sid.take() {
            let s = String::from_utf8_lossy(&self.buf).into_owned();
            ctx.state().send(
                SessionMessage {
                    sid: sid,
                    msg: Message::Str(s)
                });
        }
        ctx.write_eof()
    }
}

impl<S, SM> Handler<PayloadItem, PayloadError> for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn error(&mut self, err: PayloadError, ctx: &mut HttpContext<Self>) {
        ctx.terminate();
    }

    fn handle(&mut self, msg: PayloadItem, ctx: &mut HttpContext<Self>)
              -> Response<Self, PayloadItem>
    {
        println!("msg {:?}", msg.0);
        let size = self.buf.len() + msg.0.len();
        if size >= MAXSIZE {
            ctx.start(
                httpcodes::HTTPBadRequest
                    .builder()
                    .force_close()
                    .finish());
            ctx.write_eof();
        } else {
            self.buf.extend(msg.0);
        }
        Self::empty()
    }
}
