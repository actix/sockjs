use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;

use protocol::{Frame, CloseCode};
use session::{Message, Session};
use manager::{Broadcast, Release, Record, SessionManager, SessionMessage};

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
        if let Some(mut rec) = self.rec.take() {
            rec.close();
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for Websocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: &Frame, record: &mut Record)
            -> SendResult
    {
        match *msg {
            Frame::Heartbeat => {
                ws::WsWriter::text(ctx, "h");
            },
            Frame::Message(ref s) => {
                ws::WsWriter::text(ctx, &format!("a[{:?}]", s));
            }
            Frame::MessageVec(ref s) => {
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

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
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

        let mut tr = Websocket{s: PhantomData,
                               sm: PhantomData,
                               rec: None};
        // init transport
        let session = req.match_info().get("session").unwrap().to_owned();
        tr.init_transport(session, ctx);

        Reply::async(tr)
    }
}

impl<S, SM> StreamHandler<Frame> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: Frame, ctx: &mut HttpContext<Self>) -> Response<Self, Frame> {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, &msg, &mut rec);
            self.rec = Some(rec);
        } else if let Some(ref mut rec) = self.rec {
            rec.buffer.push_back(msg.into());
        }
        Self::empty()
    }
}

impl<S, SM> Handler<Broadcast> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: Broadcast, ctx: &mut HttpContext<Self>)
              -> Response<Self, Broadcast>
    {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, &msg.msg, &mut rec);
            self.rec = Some(rec);
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
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, &msg),
            ws::Message::Text(text) => {
                if text.is_empty() {
                    return Self::empty();
                }
                let msg: String = if text.starts_with('[') {
                    if text.len() <= 2 {
                        return Self::empty();
                    }
                    match serde_json::from_slice(text[1..text.len()-1].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            ws::WsWriter::close(
                                ctx, ws::CloseCode::Invalid,"Broken JSON encoding");
                            ctx.stop();
                            return Self::empty()
                        }
                    }
                } else {
                    match serde_json::from_slice(text[..].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            ws::WsWriter::close(
                                ctx, ws::CloseCode::Invalid,"Broken JSON encoding");
                            ctx.stop();
                            return Self::empty()
                        }
                    }
                };

                if let Some(ref rec) = self.rec {
                    ctx.state().send(
                        SessionMessage {
                            sid: rec.sid.clone(),
                            msg: Message(msg)});
                }
            }
            ws::Message::Binary(_) => {
                error!("Binary messages are not supported");
            },
            ws::Message::Closed => {
                if let Some(mut rec) = self.rec.take() {
                    rec.close();
                    ctx.state().send(Release{ses: rec});
                }
                ctx.stop();
            },
            ws::Message::Error => {
                if let Some(mut rec) = self.rec.take() {
                    rec.interrupted();
                    ctx.state().send(Release{ses: rec});
                }
                ctx.stop();
            },
            _ => (),
        }
        Self::empty()
    }
}
