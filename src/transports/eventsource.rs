use std::time::Duration;
use std::marker::PhantomData;

use actix::*;
use actix_web::*;
use http::header;

use protocol::{Frame, CloseCode};
use utils::SockjsHeaders;
use context::SockJSContext;
use session::{Message, Session, SessionState};
use manager::{Acquire, Release, Record, SessionManager};

use super::{MAXSIZE, Transport, SendResult};


pub struct EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>,
{
    sm: PhantomData<SM>,
    size: usize,
    rec: Option<Record>,
    session: S,
}

impl<S, SM> EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>
{
    fn hb(&self, ctx: &mut HttpContext<Self>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            act.send(ctx, Frame::Heartbeat);
            act.hb(ctx);
        });
    }
}

// Http actor implementation
impl<S, SM> Actor for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>,
          SM::Context: ToEnvelope<SM>
{
    type Context = HttpContext<Self>;

    fn stopping(&mut self, ctx: &mut HttpContext<Self>) {
        if let Some(rec) = self.rec.take() {
            ctx.state().send(Release{ses: rec});
        }
        ctx.terminate()
    }
}

// Transport implementation
impl<S, SM> Transport for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame) -> SendResult {
        self.size += match msg {
            Frame::Heartbeat => {
                ctx.write("data: h\r\n\r\n");
                11
            },
            Frame::Message(s) => {
                let blob = format!("data: a[{:?}]\r\n\r\n", s);
                let size = blob.len();
                ctx.write(blob);
                size
            }
            Frame::Open => {
                ctx.write("data: o\r\n\r\n");
                11
            },
            Frame::Close(code) => {
                let blob = format!("data: c[{}, {:?}]\r\n\r\n", code.num(), code.reason());
                let size = blob.len();
                ctx.write(blob);
                size
            }
            _ => 0,
        };

        if self.size > MAXSIZE { SendResult::Stop } else { SendResult::Continue }
    }
}

impl<S, SM> Route for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>,
{
    type State = SyncAddress<SM>;

    fn request(req: &mut HttpRequest, payload: Payload, ctx: &mut HttpContext<Self>)
               -> RouteResult<Self>
    {
        ctx.start(httpcodes::HTTPOk
                  .builder()
                  .header(header::CONTENT_TYPE, "text/event-stream")
                  .force_close()
                  .sockjs_cache_control()
                  .sockjs_session_cookie(req)
                  .body(Body::Streaming));
        ctx.write("\r\n");

        let act = EventSource{sm: PhantomData,
                              size: 0,
                              rec: None,
                              session: S::default()};
        let session = req.match_info().get("session").unwrap().to_owned();

        // acquire session
        ctx.state().call(&act, Acquire::new(session))
            .map(|res, act, ctx| {
                match res {
                    Ok(mut rec) => {
                        if rec.0.state == SessionState::New {
                            act.send(ctx, Frame::Open);
                        }
                        rec.0.state = SessionState::Running;

                        while let Some(msg) = rec.0.messages.pop_front() {
                            match msg {
                                Message::Str(s) => act.send(ctx, Frame::Message(s)),
                                Message::Bin(b) => act.send(ctx, Frame::MessageBlob(b)),
                            };
                        }
                        act.rec = Some(rec.0);
                        ctx.add_stream(rec.1);
                        act.hb(ctx);
                    },
                    Err(err) => {
                        act.send(ctx, err.into());
                        ctx.write_eof();
                    }
                }
            })
        // session manager is dead?
            .map_err(|_, act, ctx| {
                act.send(ctx, Frame::Close(CloseCode::InternalError));
                ctx.stop();
            })
            .wait(ctx);

        Reply::async(act)
    }
}

impl<S, SM> StreamHandler<Message> for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release> {}

impl<S, SM> Handler<Message> for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire> + Handler<Release>,
{
    fn handle(&mut self, msg: Message, ctx: &mut HttpContext<Self>) -> Response<Self, Message> {
        if ctx.connected() {
            match msg {
                Message::Str(s) => self.send(ctx, Frame::Message(s)),
                Message::Bin(b) => self.send(ctx, Frame::MessageBlob(b)),
            };
        } else {
            if let Some(ref mut rec) = self.rec {
                rec.messages.push_back(msg);
            };
        }
        Self::empty()
    }
}
