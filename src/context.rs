use std;
use std::sync::Arc;
use std::collections::VecDeque;

use actix::dev::*;
use actix::{ActorState};

use serde_json;
use futures::{Async, Future, Poll, Stream};
use futures::sync::oneshot::Sender;
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use session::{Message, Session, CloseReason};
use protocol::{CloseCode, Frame};
use manager::{SockJSManager, Broadcast};

#[derive(Debug)]
pub enum SockJSChannel {
    Acquired(UnboundedSender<Frame>),
    Released,
    Closed(CloseReason),
}

enum BufItem {
    Message(String),
    Messages(Vec<String>),
    Frame(Frame),
}

impl BufItem {
    fn is_msg(&self) -> bool {
        match *self {
            BufItem::Message(_) | BufItem::Messages(_) => true,
            _ => false,
        }
    }
}


/// Sockjs session context
pub struct SockJSContext<A> where A: Session, A::Context: AsyncContext<A>
{
    act: A,
    sid: Arc<String>,
    state: ActorState,
    modified: bool,
    wait: ActorWaitCell<A>,
    items: ActorItemsCell<A>,
    address: ActorAddressCell<A>,
    rx: UnboundedReceiver<SockJSChannel>,
    tx: Option<UnboundedSender<Frame>>,
    buf: VecDeque<BufItem>,
    sm: SyncAddress<SockJSManager<A>>,
}

impl<A> ActorContext for SockJSContext<A> where A: Session<Context=Self>
{
    /// Stop actor execution
    fn stop(&mut self) {
        self.items.stop();
        self.address.close();
        if self.state == ActorState::Running {
            self.state = ActorState::Stopping;
        }
    }

    /// Terminate actor execution
    fn terminate(&mut self) {
        self.address.close();
        self.items.close();
        self.state = ActorState::Stopped;
    }

    /// Actor execution state
    fn state(&self) -> ActorState {
        self.state
    }
}

impl<A> AsyncContext<A> for SockJSContext<A> where A: Session<Context=Self>
{
    fn spawn<F>(&mut self, fut: F) -> actix::SpawnHandle
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.modified = true;
        self.items.spawn(fut)
    }

    fn wait<F>(&mut self, fut: F)
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.modified = true;
        self.wait.add(fut);
    }

    fn cancel_future(&mut self, handle: actix::SpawnHandle) -> bool {
        self.modified = true;
        self.items.cancel_future(handle)
    }

    fn cancel_future_on_stop(&mut self, handle: actix::SpawnHandle) {
        self.items.cancel_future_on_stop(handle)
    }
}

impl<A> AsyncContextApi<A> for SockJSContext<A> where A: Session<Context=Self> {
    fn address_cell(&mut self) -> &mut ActorAddressCell<A> {
        &mut self.address
    }
}

impl<A> SockJSContext<A> where A: Session<Context=Self>
{
    #[doc(hidden)]
    pub fn subscriber<M>(&mut self) -> Box<actix::Subscriber<M>>
        where A: Handler<M>,
              M: ResponseType + 'static
    {
        Box::new(self.address.unsync_address())
    }

    /// Session id
    pub fn sid(&self) -> &Arc<String> {
        &self.sid
    }

    /// Send message to peer
    pub fn send<M>(&mut self, message: M) where M: Into<Message>
    {
        let msg = Frame::Message(message.into().0);

        let msg = if let Some(ref mut tx) = self.tx {
            match tx.unbounded_send(msg) {
                Ok(()) => return,
                Err(err) => err.into_inner()
            }
        } else {
            self.add_to_buf(msg);
            return
        };
        self.add_to_buf(msg);
        self.tx.take();
    }

    /// Send message to all sessions
    pub fn broadcast<M>(&mut self, message: M) where M: Into<Message>
    {
        self.sm.send(Broadcast::new(Frame::Message(message.into().0)));
    }

    /// Close session
    pub fn close(&mut self) {
        let frm = Frame::Close(CloseCode::GoAway);
        let msg = if let Some(ref mut tx) = self.tx {
            match tx.unbounded_send(frm) {
                Ok(()) => return,
                Err(err) => err.into_inner()
            }
        } else {
            frm
        };
        self.add_to_buf(msg);
    }

    /// Check if transport is connected
    pub fn connected(&mut self) -> bool {
        self.tx.is_some()
    }

    fn add_to_buf(&mut self, msg: Frame) {
        let is_msg = if let Some(front) = self.buf.back() {
            front.is_msg()} else { false };

        if is_msg && msg.is_msg() {
            let item = self.buf.pop_back().unwrap();
            let vec = match item {
                BufItem::Message(m) => {
                    vec![m, msg.into_message()]
                },
                BufItem::Messages(mut vec) => {
                    vec.push(msg.into_message());
                    vec
                },
                _ => unreachable!(),
            };
            self.buf.push_back(BufItem::Messages(vec));
        } else {
            if msg.is_msg() {
                self.buf.push_back(BufItem::Message(msg.into_message()));
            } else {
                self.buf.push_back(BufItem::Frame(msg));
            }
        }
    }
}

impl<A> SockJSContext<A> where A: Session<Context=Self>
{
    pub(crate) fn start(sid: Arc<String>, addr: SyncAddress<SockJSManager<A>>)
                        -> (SyncAddress<A>, UnboundedSender<SockJSChannel>)
    {
        let (tx, rx) = unbounded();

        let mut ctx = SockJSContext {
            act: A::default(),
            sid: sid,
            state: ActorState::Started,
            modified: false,
            items: ActorItemsCell::default(),
            address: ActorAddressCell::default(),
            wait: ActorWaitCell::default(),
            tx: None,
            rx: rx,
            buf: VecDeque::new(),
            sm: addr,
        };
        let addr = ctx.address();
        Arbiter::handle().spawn(ctx);
        (addr, tx)
    }
}

#[doc(hidden)]
impl<A> Future for SockJSContext<A> where A: Session<Context=Self>
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let ctx: &mut SockJSContext<A> = unsafe {
            std::mem::transmute(self as &mut SockJSContext<A>)
        };

        // update state
        match self.state {
            ActorState::Started => {
                Actor::started(&mut self.act, ctx);
                self.state = ActorState::Running;
                self.act.opened(ctx);
            },
            ActorState::Stopping => {
                Actor::stopping(&mut self.act, ctx);
            }
            _ => ()
        }

        let mut prep_stop = false;
        loop {
            self.modified = false;

            // check wait futures
            if self.wait.poll(&mut self.act, ctx) {
                return Ok(Async::NotReady)
            }

            // incoming messages
            self.address.poll(&mut self.act, ctx);

            // spawned futures and streams
            self.items.poll(&mut self.act, ctx);

            // sockjs channel
            loop {
                match self.rx.poll() {
                    Ok(Async::Ready(Some(msg))) => {
                        match msg {
                            SockJSChannel::Acquired(tx) => {
                                while let Some(msg) = self.buf.pop_front() {
                                    match msg {
                                        BufItem::Message(msg) => {
                                            let _ = tx.unbounded_send(Frame::Message(msg));
                                        },
                                        BufItem::Messages(msg) => {
                                            let _ = tx.unbounded_send(
                                                Frame::MessageVec(
                                                    serde_json::to_string(&msg).unwrap()));
                                        },
                                        BufItem::Frame(msg) => {
                                            let _ = tx.unbounded_send(msg);
                                        },
                                    }
                                };
                                self.tx = Some(tx);
                                self.act.acquired(ctx);
                            }
                            SockJSChannel::Released => {
                                self.tx.take();
                                self.act.released(ctx);
                            },
                            SockJSChannel::Closed(reason) => {
                                self.tx.take();
                                self.act.closed(ctx, reason);
                                self.stop()
                            }
                        }
                        continue
                    },
                    Ok(Async::Ready(None)) | Ok(Async::NotReady) =>
                        break,
                    Err(_) => {},
                }
            }

            // are we done
            if self.modified {
                continue
            }

            // check state
            match self.state {
                ActorState::Stopped => {
                    self.state = ActorState::Stopped;
                    Actor::stopped(&mut self.act, ctx);
                    return Ok(Async::Ready(()))
                },
                ActorState::Stopping => {
                    if prep_stop {
                        if self.address.connected() || !self.items.is_empty() {
                            self.state = ActorState::Running;
                            continue
                        } else {
                            self.state = ActorState::Stopped;
                            Actor::stopped(&mut self.act, ctx);
                            return Ok(Async::Ready(()))
                        }
                    } else {
                        Actor::stopping(&mut self.act, ctx);
                        prep_stop = true;
                        continue
                    }
                },
                ActorState::Running => {
                    if !self.address.connected() && self.items.is_empty() {
                        self.state = ActorState::Stopping;
                        Actor::stopping(&mut self.act, ctx);
                        prep_stop = true;
                        continue
                    }
                },
                _ => (),
            }

            return Ok(Async::NotReady)
        }
    }
}

#[doc(hidden)]
impl<A> ToEnvelope<A> for SockJSContext<A>
    where A: Session<Context=SockJSContext<A>>,
{
    fn pack<M>(msg: M,
               tx: Option<Sender<Result<M::Item, M::Error>>>,
               channel_on_drop: bool) -> Envelope<A>
        where A: Handler<M>,
              M: ResponseType + Send + 'static,
              M::Item: Send,
              M::Error: Send
    {
        RemoteEnvelope::new(msg, tx, channel_on_drop).into()
    }
}
