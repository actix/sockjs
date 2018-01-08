use std::sync::Arc;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use serde_json;

use context::ChannelItem;
use protocol::{Frame, CloseCode};
use session::{Message, Session};
use manager::{Broadcast, Release, Record, SessionManager, SessionMessage};

use super::{Transport, SendResult, Flags};


pub struct Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
    flags: Flags,
}

impl<S, SM> Websocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    pub fn init(req: HttpRequest<SyncAddress<SM>>) -> Result<HttpResponse>
    {
        let mut resp = ws::handshake(&req)?;
        let stream = ws::WsStream::new(req.payload().readany());
        let session = req.match_info().get("session").unwrap().to_owned();

        let mut ctx = HttpContext::from_request(req);
        ctx.add_message_stream(stream);

        // init transport
        let mut tr = Websocket{s: PhantomData,
                               sm: PhantomData,
                               rec: None,
                               flags: Flags::empty()};
        tr.init_transport(session, &mut ctx);

        Ok(resp.body(ctx.actor(tr))?)
    }
}

// Http actor implementation
impl<S, SM> Actor for Websocket<S, SM> where S: Session, SM: SessionManager<S>
{
    type Context = HttpContext<Self, SyncAddress<SM>>;

    fn stopping(&mut self, ctx: &mut Self::Context) -> bool {
        if let Some(mut rec) = self.rec.take() {
            rec.close();
            ctx.state().send(Release{ses: rec});
        }
        true
    }
}

// Transport implementation
impl<S, SM> Transport<S, SM> for Websocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut Self::Context, msg: &Frame, record: &mut Record) -> SendResult
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

    fn send_heartbeat(&mut self, ctx: &mut Self::Context) {
        ws::WsWriter::text(ctx, "h");
    }

    fn send_close(&mut self, ctx: &mut Self::Context, code: CloseCode) {
        ws::WsWriter::text(ctx, &format!("c[{},{:?}]", code.num(), code.reason()));
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }

    fn flags(&mut self) -> &mut Flags {
        &mut self.flags
    }
}

impl<S, SM> Handler<ChannelItem> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: ChannelItem, ctx: &mut Self::Context) {
        self.handle_message(msg, ctx)
    }
}

impl<S, SM> Handler<Broadcast> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: Broadcast, ctx: &mut Self::Context) {
        if let Some(mut rec) = self.rec.take() {
            self.send(ctx, &msg.msg, &mut rec);
            self.rec = Some(rec);
        }
    }
}

impl<S, SM> Handler<ws::Message> for Websocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        // process websocket messages
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, &msg),
            ws::Message::Text(text) => {
                if text.is_empty() {
                    return
                }
                let msg: String = if text.starts_with('[') {
                    if text.len() <= 2 {
                        return
                    }
                    match serde_json::from_slice(text[1..text.len()-1].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            ws::WsWriter::close(
                                ctx, ws::CloseCode::Invalid,"Broken JSON encoding");
                            if let Some(mut rec) = self.rec.take() {
                                rec.interrupted();
                                ctx.state().send(Release{ses: rec});
                            }
                            ctx.stop();
                            return
                        }
                    }
                } else {
                    match serde_json::from_slice(text[..].as_ref()) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            ws::WsWriter::close(
                                ctx, ws::CloseCode::Invalid,"Broken JSON encoding");
                            if let Some(mut rec) = self.rec.take() {
                                rec.interrupted();
                                ctx.state().send(Release{ses: rec});
                            }
                            ctx.stop();
                            return
                        }
                    }
                };

                if let Some(ref rec) = self.rec {
                    ctx.state().send(
                        SessionMessage {
                            sid: Arc::clone(&rec.sid),
                            msg: Message(msg)
                        });
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
    }
}
