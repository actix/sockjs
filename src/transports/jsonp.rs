use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use regex::Regex;
use serde_json;
use bytes::BytesMut;
use percent_encoding::percent_decode;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use session::{Message, Session};
use manager::{Record, SessionManager, SessionMessage};

use super::{MAXSIZE, Transport, SendResult};


pub struct JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
    callback: String,
}

// Http actor implementation
impl<S, SM> Actor for JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        self.stop(ctx);
    }
}

impl<S, SM>  JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn write(&self, s: &str, ctx: &mut HttpContext<Self>) {
        ctx.write(format!("/**/{}({});\r\n",
                          self.callback, serde_json::to_string(s).unwrap()))
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult
    {
        match msg {
            Frame::Heartbeat => {
                self.write("h", ctx);
            },
            Frame::Message(s) => {
                self.write(&format!("a[{:?}]", s), ctx);
            }
            Frame::MessageVec(s) => {
                self.write(&format!("a{}", s), ctx);
            }
            Frame::MessageBlob(_) => {
                unimplemented!()
            }
            Frame::Open => {
                self.write("o", ctx);
            },
            Frame::Close(code) => {
                record.close();
                let blob = format!("c[{},{:?}]", code.num(), code.reason());
                self.write(&blob, ctx);
            }
        };
        ctx.write_eof();
        SendResult::Stop
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>)
    {
        self.write("h\n", ctx);
        ctx.write_eof();
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode)
    {
        self.write(&format!("c[{},{:?}]", code.num(), code.reason()), ctx);
        ctx.write_eof();
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }
}

impl<S, SM> Route for JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, _: Payload, ctx: &mut HttpContext<Self>)
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
                      .content_type("application/javascript; charset=UTF-8")
                      .force_close()
                      .sockjs_no_cache()
                      .sockjs_session_cookie(req)
                      .sockjs_cors_headers(req.headers())
                      .body(Body::Streaming));

            let session = req.match_info().get("session").unwrap().to_owned();
            let mut transport = JSONPolling{s: PhantomData,
                                            sm: PhantomData,
                                            rec: None,
                                            callback: callback};
            // init transport
            transport.init_transport(session, ctx);

            Reply::async(transport)
        } else {
            Reply::reply(
                httpcodes::HTTPInternalServerError.with_body("\"callback\" parameter required")
            )
        }
    }
}

impl<S, SM> StreamHandler<Frame> for JSONPolling<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for JSONPolling<S, SM>
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


pub struct JSONPollingSend<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    buf: BytesMut,
    sid: Option<String>,
    resp: Option<HttpResponse>,
    urlencoded: bool,
}

// Http actor implementation
impl<S, SM> Actor for JSONPollingSend<S, SM>
    where S: Session,
          SM: SessionManager<S>,
{
    type Context = HttpContext<Self>;
}

impl<S, SM> Route for JSONPollingSend<S, SM>
    where S: Session,
          SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        if *req.method() != Method::POST {
            Reply::reply(httpcodes::HTTPBadRequest.with_reason("Method is not allowed"))
        } else {
            let _ = req.load_cookies();
            let resp = httpcodes::HTTPOk
                .builder()
                .content_type("text/plain; charset=UTF-8")
                .sockjs_no_cache()
                .sockjs_session_cookie(req)
                .body("ok")?;
            ctx.add_stream(payload);

            Reply::async(JSONPollingSend{
                s: PhantomData,
                sm: PhantomData,
                buf: BytesMut::new(),
                sid: Some(req.match_info().get("session").unwrap().to_owned()),
                resp: Some(resp),
                urlencoded: req.content_type() == "application/x-www-form-urlencoded",
            })
        }
    }
}

impl<S, SM> StreamHandler<PayloadItem, PayloadError> for JSONPollingSend<S, SM>
    where S: Session, SM: SessionManager<S>
{
    fn finished(&mut self, ctx: &mut Self::Context) {
        if let Some(sid) = self.sid.take() {
            // empty message
            if self.buf.is_empty() {
                ctx.start(httpcodes::HTTPInternalServerError.with_body("Payload expected."));
                ctx.write_eof();
                return
            }

            // deserialize json
            let mut msgs: Vec<String> = if self.urlencoded {
                if self.buf.len() <= 2 || &self.buf[..2] != b"d=" {
                    ctx.start(
                        httpcodes::HTTPInternalServerError.with_body("Payload expected."));
                    ctx.write_eof();
                    return
                }

                if let Ok(data) = percent_decode(&self.buf[2..]).decode_utf8() {
                    match serde_json::from_slice(data.as_ref().as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            ctx.start(
                                httpcodes::HTTPInternalServerError.with_body(
                                    "Broken JSON encoding."));
                            ctx.write_eof();
                            return
                        }
                    }
                } else {
                    ctx.start(httpcodes::HTTPInternalServerError.with_body("Payload expected."));
                    ctx.write_eof();
                    return
                }
            } else {
                match serde_json::from_slice(&self.buf) {
                    Ok(msgs) => msgs,
                    Err(_) => {
                        ctx.start(
                            httpcodes::HTTPInternalServerError.with_body(
                                "Broken JSON encoding."));
                        ctx.write_eof();
                        return
                    }
                }
            };

            // do nothing
            if msgs.is_empty() {
                ctx.start(self.resp.take().unwrap());
                ctx.write_eof();
                return
            } else {
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

impl<S, SM> Handler<PayloadItem, PayloadError> for JSONPollingSend<S, SM>
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
