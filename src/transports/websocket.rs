use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;

use protocol::{Frame, CloseCode};
use session::{Message, Session};
use manager::{Release, Record, SessionManager, SessionMessage};

use super::{Transport, SendResult};


pub struct Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
}

// Http actor implementation
impl<S, SM> Actor for Websocket<S, SM> where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        println!("STOPPING");
        if let Some(rec) = self.rec.take() {
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for Websocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult
    {
        match msg {
            Frame::Heartbeat => {
                ws::WsWriter::text(ctx, "h");
            },
            Frame::Message(s) => {
                ws::WsWriter::text(ctx, &format!("a[{:?}]", s));
            }
            Frame::MessageVec(s) => {
                ws::WsWriter::text(ctx, &format!("a{}", s));
            }
            Frame::MessageBlob(_) => {
                // ctx.write(format!("a{}\n", s));
            }
            Frame::Open => {
                ws::WsWriter::text(ctx, "o");
            },
            Frame::Close(code) => {
                record.close();
                ws::WsWriter::text(ctx, &format!("c[{},{:?}]\n", code.num(), code.reason()));
            }
        };

        SendResult::Continue
    }

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>) {
        ws::WsWriter::text(ctx, "h");
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode) {
        ws::WsWriter::text(ctx, &format!("c[{},{:?}]", code.num(), code.reason()));
    }

    fn set_session_record(&mut self, record: Record) {
        self.rec = Some(record);
    }
}

impl<S, SM> Route for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        ctx.start(ws::handshake(req)?);
        ctx.add_stream(ws::WsStream::new(payload));

        // init transport, but aftre prelude only
        let session = req.match_info().get("session").unwrap().to_owned();
        ctx.drain().map(move |_, _, ctx| {
            ctx.run_later(Duration::new(0, 800000), move |act, ctx| {
                act.init_transport(session, ctx);
            });
        }).wait(ctx);

        Reply::async(
            Websocket{s: PhantomData,
                      sm: PhantomData,
                      rec: None})
    }
}

impl<S, SM> StreamHandler<Frame> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for Websocket<S, SM>
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

impl<S, SM> StreamHandler<ws::Message> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<ws::Message> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: ws::Message, ctx: &mut HttpContext<Self>)
              -> Response<Self, ws::Message>
    {
        // process websocket messages
        println!("WS: {:?}", msg);
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, msg),
            ws::Message::Text(text) => {
                let mut msgs: Vec<String> = if text.starts_with('[') {
                    match serde_json::from_slice(text[1..text.len()-2].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            // ws::WsWriter::close();
                            //"Broken JSON encoding."
                            ctx.stop();
                            return Self::empty()
                        }
                    }
                } else {
                    match serde_json::from_slice(text[..].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            // ws::WsWriter::close();
                            //"Broken JSON encoding."
                            ctx.stop();
                            return Self::empty()
                        }
                    }
                };

                if msgs.is_empty() {
                    return Self::empty()
                }
                let msg = if msgs.len() == 1 {
                    Message::Str(msgs.pop().unwrap())
                } else {
                    msgs.into()
                };

                if let Some(ref rec) = self.rec {
                    ctx.state().send(
                        SessionMessage {
                            sid: rec.sid.clone(),
                            msg: msg});
                }
            }
            ws::Message::Binary(_) => unimplemented!(),
            ws::Message::Closed | ws::Message::Error => {
                if let Some(rec) = self.rec.take() {
                    ctx.state().send(Release{ses: rec});
                }
                ctx.stop();
            }
            _ => (),
        }
        Self::empty()
    }
}
