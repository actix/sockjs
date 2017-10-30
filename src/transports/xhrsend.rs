use std::sync::Arc;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use bytes::BytesMut;
use serde_json;
use http::header::ACCESS_CONTROL_ALLOW_METHODS;

use utils::SockjsHeaders;
use session::{Message, Session};
use manager::{SessionManager, SessionMessage};

use super::MAXSIZE;


pub struct XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    buf: BytesMut,
    sid: Option<String>,
    resp: Option<HttpResponse>,
}

// Http actor implementation
impl<S, SM> Actor for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self>;
}

impl<S, SM> Route for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>,
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
                    .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, POST")
                    .sockjs_cache_headers()
                    .sockjs_cors_headers(req.headers())
                    .sockjs_session_cookie(req)
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
                .sockjs_no_cache()
                .sockjs_cors_headers(req.headers())
                .sockjs_session_cookie(req)
                .finish()?;
            ctx.add_stream(payload);

            Reply::async(XhrSend{
                s: PhantomData,
                sm: PhantomData,
                buf: BytesMut::new(),
                sid: Some(req.match_info().get("session").unwrap().to_owned()),
                resp: Some(resp),
            })
        }
    }
}

impl<S, SM> StreamHandler<PayloadItem, PayloadError> for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn finished(&mut self, ctx: &mut Self::Context) {
        if let Some(sid) = self.sid.take() {
            let sid = Arc::new(sid);

            // empty message
            if self.buf.is_empty() {
                ctx.start(httpcodes::HTTPInternalServerError.with_body("Payload expected."));
                ctx.write_eof();
                return
            } else if self.buf == "[]" {
                ctx.start(self.resp.take().unwrap());
                ctx.write_eof();
                return
            }

            // deserialize json
            let mut msgs: Vec<String> = match serde_json::from_slice(&self.buf) {
                Ok(msgs) => msgs,
                Err(_) => {
                    ctx.start(
                        httpcodes::HTTPInternalServerError.with_body("Broken JSON encoding."));
                    ctx.write_eof();
                    return
                }
            };

            if !msgs.is_empty() {
                let last = msgs.pop().unwrap();
                for msg in msgs {
                    ctx.state().send(
                        SessionMessage {
                            sid: sid.clone(),
                            msg: Message(msg)});
                }

                ctx.state().call(
                    self, SessionMessage {
                        sid: sid.clone(),
                        msg: Message(last)})
                    .map(|res, act, ctx| {
                        match res {
                            Ok(_) => ctx.start(act.resp.take().unwrap()),
                            Err(_) => ctx.start(httpcodes::HTTPNotFound),
                        }
                        ctx.write_eof();
                    })
                    .map_err(|_, _, ctx| {
                        ctx.start(httpcodes::HTTPNotFound);
                        ctx.write_eof();
                    })
                    .wait(ctx);
            }
        }
    }
}

impl<S, SM> Handler<PayloadItem, PayloadError> for XhrSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn error(&mut self, _: PayloadError, ctx: &mut HttpContext<Self>) {
        ctx.terminate();
    }

    fn handle(&mut self, msg: PayloadItem, ctx: &mut HttpContext<Self>)
              -> Response<Self, PayloadItem>
    {
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
