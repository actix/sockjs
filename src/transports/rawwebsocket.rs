use std::sync::Arc;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use rand;

use context::ChannelItem;
use protocol::{Frame, CloseCode};
use session::{Message, Session};
use manager::{Broadcast, Release, Record, SessionManager, SessionMessage};

use super::{Transport, SendResult, Flags};


pub struct RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
    flags: Flags,
}

impl<S, SM> RawWebsocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    pub fn init(req: HttpRequest<SyncAddress<SM>>) -> Result<HttpResponse>
    {
        let mut resp = ws::handshake(&req)?;
        let stream = ws::WsStream::new(req.payload().readany());

        // session
        let sid = format!("{}", rand::random::<u32>());

        let mut ctx = HttpContext::from_request(req);
        ctx.add_message_stream(stream);

        let mut tr = RawWebsocket{s: PhantomData,
                                  sm: PhantomData,
                                  rec: None,
                                  flags: Flags::empty()};
        // init transport
        tr.init_transport(sid, &mut ctx);

        Ok(resp.body(ctx.actor(tr))?)
    }
}

// Http actor implementation
impl<S, SM> Actor for RawWebsocket<S, SM> where S: Session, SM: SessionManager<S>
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
impl<S, SM> Transport<S, SM> for RawWebsocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut Self::Context, msg: &Frame, record: &mut Record)
            -> SendResult
    {
        match *msg {
            Frame::Heartbeat => {
                ws::WsWriter::ping(ctx, "");
            },
            Frame::Message(ref s) | Frame::MessageVec(ref s) => {
                ws::WsWriter::text(ctx, s);
            }
            Frame::MessageBlob(ref b) => {
                ws::WsWriter::binary(ctx, Vec::from(b.as_ref()));
            }
            Frame::Open => (),
            Frame::Close(_) => {
                record.close();
                ws::WsWriter::close(ctx, ws::CloseCode::Normal, "Go away!");
            }
        };

        SendResult::Continue
    }

    fn send_heartbeat(&mut self, ctx: &mut Self::Context) {
        ws::WsWriter::ping(ctx, "");
    }

    fn send_close(&mut self, ctx: &mut Self::Context, _: CloseCode) {
        ws::WsWriter::close(ctx, ws::CloseCode::Normal, "Go away!");
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }

    fn flags(&mut self) -> &mut Flags {
        &mut self.flags
    }
}

impl<S, SM> Handler<ChannelItem> for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: ChannelItem, ctx: &mut Self::Context) {
        self.handle_message(msg, ctx)
    }
}

impl<S, SM> Handler<Broadcast> for RawWebsocket<S, SM>
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

impl<S, SM> Handler<ws::Message> for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type Result = ();

    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        // process websocket messages
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, &msg),
            ws::Message::Text(text) => {
                if !text.is_empty() {
                    if let Some(ref rec) = self.rec {
                        ctx.state().send(
                            SessionMessage {
                                sid: Arc::clone(&rec.sid),
                                msg: Message(text)});
                    }
                }
            }
            ws::Message::Binary(_) => {
                error!("Not supported!");
            }
            ws::Message::Closed | ws::Message::Error => {
                if let Some(rec) = self.rec.take() {
                    ctx.state().send(Release{ses: rec});
                }
                self.release(ctx);
            }
            _ => (),
        }
    }
}
