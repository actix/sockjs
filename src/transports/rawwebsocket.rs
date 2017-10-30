use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use rand;

use protocol::{Frame, CloseCode};
use session::{Message, Session};
use manager::{Release, Record, SessionManager, SessionMessage};

use super::{Transport, SendResult};


pub struct RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    s: PhantomData<S>,
    sm: PhantomData<SM>,
    rec: Option<Record>,
}

// Http actor implementation
impl<S, SM> Actor for RawWebsocket<S, SM> where S: Session, SM: SessionManager<S>
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
impl<S, SM> Transport<S, SM> for RawWebsocket<S, SM> where S: Session, SM: SessionManager<S>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult
    {
        match msg {
            Frame::Heartbeat => {
                ws::WsWriter::ping(ctx, "");
            },
            Frame::Message(s) | Frame::MessageVec(s) => {
                ws::WsWriter::text(ctx, &s);
            }
            Frame::MessageBlob(b) => {
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

    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>) {
        ws::WsWriter::ping(ctx, "");
    }

    fn send_close(&mut self, ctx: &mut HttpContext<Self>, _: CloseCode) {
        ws::WsWriter::close(ctx, ws::CloseCode::Normal, "Go away!");
    }

    fn session_record(&mut self) -> &mut Option<Record> {
        &mut self.rec
    }
}

impl<S, SM> Route for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        // session
        let sid = format!("{}", rand::random::<u32>());

        ctx.start(ws::handshake(req)?);
        ctx.add_stream(ws::WsStream::new(payload));

        let mut tr = RawWebsocket{s: PhantomData,
                                  sm: PhantomData,
                                  rec: None};
        // init transport
        tr.init_transport(sid, ctx);

        Reply::async(tr)
    }
}

impl<S, SM> StreamHandler<Frame> for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<Frame> for RawWebsocket<S, SM>
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

impl<S, SM> StreamHandler<ws::Message> for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S> {}

impl<S, SM> Handler<ws::Message> for RawWebsocket<S, SM>
    where S: Session, SM: SessionManager<S>,
{
    fn handle(&mut self, msg: ws::Message, ctx: &mut HttpContext<Self>)
              -> Response<Self, ws::Message>
    {
        // process websocket messages
        match msg {
            ws::Message::Ping(msg) => ws::WsWriter::pong(ctx, &msg),
            ws::Message::Text(text) => {
                if !text.is_empty() {
                    if let Some(ref rec) = self.rec {
                        ctx.state().send(
                            SessionMessage {
                                sid: rec.sid.clone(),
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
                ctx.stop();
            }
            _ => (),
        }
        Self::empty()
    }
}
