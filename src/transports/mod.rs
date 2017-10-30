use actix::*;
use actix_web::*;
use serde_json;
use futures::{Async, Stream};

use protocol::{Frame, CloseCode};
use session::{Session, SessionState};
use manager::{Acquire, Release, Record, SessionManager};

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

/// Result of `Transport::send` method
#[derive(PartialEq)]
pub enum SendResult {
    /// continue transport event loop
    Continue,
    /// stop transport, ask client to reconnect
    Stop,
}

trait Transport<S, SM>: Actor<Context=HttpContext<Self>> +
    StreamHandler<Frame> + Route<State=SyncAddress<SM>>
    where S: Session, SM: SessionManager<S>,
{
    /// Set record
    fn session_record(&mut self) -> &mut Option<Record>;

    /// Stop transport and release session
    fn stop(&mut self, ctx: &mut HttpContext<Self>) {
        if let Some(mut rec) = self.session_record().take() {
            trace!("STOPPING {:?}", ctx.connected());

            // peer dropped connection
            if !ctx.connected() {
                rec.interrupted();
            }
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }

    /// Send sockjs frame
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame, record: &mut Record)
            -> SendResult;

    /// Send close frame
    fn send_close(&mut self, ctx: &mut HttpContext<Self>, code: CloseCode);

    /// Send heartbeat
    fn send_heartbeat(&mut self, ctx: &mut HttpContext<Self>);

    /// Send sockjs frame
    fn send_buffered(&mut self, ctx: &mut HttpContext<Self>, record: &mut Record) -> SendResult {
        while !record.buffer.is_empty() {
            let is_msg = if let Some(front) = record.buffer.front() {
                front.is_msg()} else { false };

            if is_msg {
                let mut msg = Vec::new();
                while let Some(frm) = record.buffer.pop_front() {
                    match frm {
                        Frame::Message(s) => msg.push(s),
                        _ => {
                            record.buffer.push_front(frm);
                            break
                        }
                    }
                }

                record.buffer.push_front(
                    Frame::MessageVec(serde_json::to_string(&msg).unwrap()));
            }

            if let Some(msg) = record.buffer.pop_front() {
                if let SendResult::Stop = self.send(ctx, msg, record) {
                    return SendResult::Stop
                }
            }
        }
        SendResult::Continue
    }

    fn init_transport(&mut self, session: String, ctx: &mut HttpContext<Self>) {
        // acquire session
        ctx.state().call(self, Acquire::new(session))
            .map(|res, act, ctx| {
                match res {
                    Ok(mut rec) => {
                        // copy messages into buffer
                        while let Ok(Async::Ready(Some(msg))) = rec.1.poll() {
                            rec.0.buffer.push_back(msg);
                        };
                        trace!("STATE: {:?}", rec.0.state);

                        match rec.0.state {
                            SessionState::Running => {
                                if let SendResult::Stop = act.send_buffered(ctx, &mut rec.0) {
                                    // release immidietly
                                    ctx.state().send(Release{ses: rec.0});
                                } else {
                                    *act.session_record()  = Some(rec.0);
                                    ctx.add_stream(rec.1);
                                }
                            },

                            SessionState::New => {
                                rec.0.state = SessionState::Running;
                                if let SendResult::Stop = act.send(ctx, Frame::Open, &mut rec.0)
                                {
                                    // release is send stops
                                    ctx.state().send(Release{ses: rec.0});
                                } else if let SendResult::Stop =
                                    act.send_buffered(ctx, &mut rec.0) // send buffered messages
                                {
                                    // release immidietly
                                    ctx.state().send(Release{ses: rec.0});
                                } else {
                                    *act.session_record()  = Some(rec.0);
                                    ctx.add_stream(rec.1);
                                }
                            },

                            SessionState::Interrupted => {
                                act.send(ctx, Frame::Close(CloseCode::Interrupted), &mut rec.0);
                                ctx.state().send(Release{ses: rec.0});
                            },

                            SessionState::Closed => {
                                act.send(ctx, Frame::Close(CloseCode::GoAway), &mut rec.0);
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
