#![allow(dead_code, unused_variables)]
use std::marker::PhantomData;
use std::time::Duration;

use actix::*;
use actix_web::*;
use actix::dev::ToEnvelope;
use http::header;

use protocol::Frame;
use utils::SockjsHeaders;
use context::SockJSContext;
use session::Session;
use manager::{Acquire, Release, Record, SessionManager};

pub struct Message<M: 'static> {
    pub data: M
}

trait Transport: Actor<Context=HttpContext<Self>> + Route {
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame);
}

pub struct EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire<S>> + Handler<Release>,
{
    sm: PhantomData<SM>,
    size: usize,
    rec: Option<Record<S>>,
    session: S,
}

impl<S, SM> EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire<S>> + Handler<Release>
{
    fn hb(&self, ctx: &mut HttpContext<Self>) {
        ctx.run_later(Duration::new(5, 0), |act, ctx| {
            act.send(ctx, Frame::Heartbeat);
            act.hb(ctx);
        });
    }
}

impl<S, SM> Actor for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire<S>> + Handler<Release>,
          SM::Context: ToEnvelope<SM>
{
    type Context = HttpContext<Self>;
}

impl<S, SM> Transport for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire<S>> + Handler<Release>,
{
    fn send(&mut self, ctx: &mut HttpContext<Self>, msg: Frame) {
        self.size += match msg {
            Frame::Heartbeat => {
                ctx.write("data: h\r\n\r\n");
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
    }
}

impl<S, SM> Route for EventSource<S, SM>
    where S: Session,
          SM: Actor + Handler<Acquire<S>> + Handler<Release>,
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

        ctx.state().call(&act, Acquire::new(session))
            .map(|res, act, ctx| {
                match res {
                    Ok(rec) => {
                        act.rec = Some(rec);
                        act.hb(ctx);
                    },
                    Err(err) => {
                        act.send(ctx, err.into());
                        ctx.write_eof();
                    }
                }
            })
            .map_err(|_, act, ctx| {
                println!("err");
                ctx.stop();
            })
            .wait(ctx);

        Reply::async(act)
    }
}
