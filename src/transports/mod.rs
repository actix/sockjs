use actix::*;
use actix_web::*;

use context::ChannelItem;
use protocol::{Frame, CloseCode};
use session::{Session, SessionState};
use manager::{Acquire, Release, Broadcast, Record, SessionManager};

mod xhr;
mod xhrsend;
mod xhrstreaming;
mod eventsource;
mod jsonp;
mod htmlfile;
mod websocket;
mod rawwebsocket;

pub use self::xhr::Xhr;
pub use self::xhrsend::XhrSend;
pub use self::xhrstreaming::XhrStreaming;
pub use self::eventsource::EventSource;
pub use self::htmlfile::HTMLFile;
pub use self::websocket::Websocket;
pub use self::rawwebsocket::RawWebsocket;
pub use self::jsonp::{JSONPolling, JSONPollingSend};

pub const MAXSIZE: usize = 131_072;  // 128K bytes

bitflags! {
    pub struct Flags: u8 {
        const READY = 0b0000_0001;
        const RELEASE = 0b0000_0010;
    }
}

type TransportContext<T, SM> = HttpContext<T, SyncAddress<SM>>;

/// Result of `Transport::send` method
#[derive(PartialEq)]
pub enum SendResult {
    /// continue transport event loop
    Continue,
    /// stop transport, ask client to reconnect
    Stop,
}

trait Transport<S, SM>: Actor<Context=TransportContext<Self, SM>> +
    Handler<ChannelItem> + Handler<Broadcast>
    where S: Session, SM: SessionManager<S>,
{
    /// Session flags
    fn flags(&mut self) -> &mut Flags;

    /// Set record
    fn session_record(&mut self) -> &mut Option<Record>;

    /// Stop transport and release session
    fn release(&mut self, ctx: &mut TransportContext<Self, SM>) {
        if let Some(mut rec) = self.session_record().take() {
            if !ctx.connected() {
                rec.interrupted();
            }
            ctx.state().send(Release{ses: rec});
        }
        ctx.stop();
    }

    fn handle_broadcast(&mut self, msg: Broadcast, ctx: &mut Self::Context) {
        if let Some(mut rec) = self.session_record().take() {
            if self.flags().contains(Flags::READY) {
                rec.add(msg.msg);
                *self.session_record() = Some(rec);
            } else if SendResult::Stop == self.send(ctx, &msg.msg, &mut rec) {
                *self.session_record() = Some(rec);
                self.release(ctx);
            } else {
                *self.session_record() = Some(rec);
            }
        }
    }

    fn handle_message(&mut self, msg: ChannelItem, ctx: &mut Self::Context) {
        match msg {
            ChannelItem::Frame(msg) => {
                if let Some(mut rec) = self.session_record().take() {
                    if self.flags().contains(Flags::READY) {
                        if SendResult::Stop == self.send(ctx, &msg, &mut rec) {
                            *self.session_record() = Some(rec);
                            self.release(ctx);
                        } else {
                            *self.session_record() = Some(rec);
                        }
                    } else {
                        rec.add(msg);
                        *self.session_record() = Some(rec);
                    }
                }
            }
            ChannelItem::Ready => {
                if let Some(mut rec) = self.session_record().take() {
                    if SendResult::Stop == self.send_buffered(ctx, &mut rec) {
                        *self.session_record() = Some(rec);
                        self.release(ctx);
                    } else {
                        *self.session_record() = Some(rec);
                    }
                }
                if self.flags().contains(Flags::RELEASE) {
                    self.release(ctx)
                } else {
                    self.flags().insert(Flags::READY);
                }
            }
        }
    }

    /// Send sockjs frame
    fn send(&mut self, ctx: &mut TransportContext<Self, SM>, msg: &Frame, record: &mut Record)
            -> SendResult;

    /// Send close frame
    fn send_close(&mut self, ctx: &mut TransportContext<Self, SM>, code: CloseCode);

    /// Send heartbeat
    fn send_heartbeat(&mut self, ctx: &mut TransportContext<Self, SM>);

    /// Send sockjs frame
    fn send_buffered(&mut self, ctx: &mut TransportContext<Self, SM>, record: &mut Record)
                     -> SendResult {
        while !record.buffer.is_empty() {
            if let Some(msg) = record.buffer.pop_front() {
                if let SendResult::Stop = self.send(ctx, msg.as_ref(), record) {
                    return SendResult::Stop
                }
            }
        }
        SendResult::Continue
    }

    fn init_transport(&mut self, session: String, ctx: &mut TransportContext<Self, SM>) {
        // acquire session
        let addr = ctx.sync_subscriber();
        ctx.state().call(self, Acquire::new(session, addr))
            .map(|res, act, ctx| {
                match res {
                    Ok(mut rec) => {
                        // copy messages into buffer
                        trace!("STATE: {:?}", rec.0.state);

                        match rec.0.state {
                            SessionState::Running => {
                                if let SendResult::Stop = act.send_buffered(ctx, &mut rec.0) {
                                    // release immidietly
                                    act.flags().insert(Flags::RELEASE);
                                }
                                *act.session_record() = Some(rec.0);
                                ctx.add_message_stream(rec.1);
                            },
                            SessionState::New => {
                                rec.0.state = SessionState::Running;
                                if let SendResult::Stop = act.send(ctx, &Frame::Open, &mut rec.0)
                                {
                                    // release is send stops
                                    act.flags().insert(Flags::RELEASE);
                                } else if let SendResult::Stop =
                                    act.send_buffered(ctx, &mut rec.0) // send buffered messages
                                {
                                    // release immidietly
                                    act.flags().insert(Flags::RELEASE);
                                }
                                *act.session_record() = Some(rec.0);
                                ctx.add_message_stream(rec.1);
                            },

                            SessionState::Interrupted => {
                                act.send(ctx, &Frame::Close(CloseCode::Interrupted), &mut rec.0);
                                ctx.state().send(Release{ses: rec.0});
                            },

                            SessionState::Closed => {
                                act.send(ctx, &Frame::Close(CloseCode::GoAway), &mut rec.0);
                                ctx.state().send(Release{ses: rec.0});
                            }
                        }
                    },
                    Err(err) => {
                        act.send_close(ctx, err.into());
                        ctx.write_eof();
                    }
                }
            })
            // session manager is dead?
            .map_err(|_, act, ctx| {
                act.send_close(ctx, CloseCode::InternalError);
                ctx.write_eof();
            })
            .wait(ctx);
    }
}
