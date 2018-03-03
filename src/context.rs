use std;
use std::sync::Arc;
use std::collections::VecDeque;

use actix::dev::*;
use actix::{ActorState, Message as ActixMessage};

use serde_json;
use futures::{Async, Future, Poll, Stream};
use futures::sync::oneshot::Sender;
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use session::{Message, Session, CloseReason};
use protocol::{CloseCode, Frame};
use manager::{SockJSManager, Broadcast};

#[derive(Debug)]
pub enum SockJSChannel {
    Opened,
    Acquired(UnboundedSender<ChannelItem>),
    Released,
    Closed(CloseReason),
}

#[derive(Message, Debug)]
pub enum ChannelItem {
    Frame(Frame),
    Ready,
}

#[derive(Debug)]
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
    inner: ContextImpl<A>,
    sid: Arc<String>,
    rx: UnboundedReceiver<SockJSChannel>,
    tx: Option<UnboundedSender<ChannelItem>>,
    buf: VecDeque<BufItem>,
    sm: Addr<Syn, SockJSManager<A>>,
}

impl<A> ActorContext for SockJSContext<A> where A: Session<Context=Self>
{
    /// Stop actor execution
    fn stop(&mut self) {
        self.inner.stop()
    }

    /// Terminate actor execution
    fn terminate(&mut self) {
        self.inner.terminate()
    }

    /// Actor execution state
    fn state(&self) -> ActorState {
        self.inner.state()
    }
}

impl<A> AsyncContext<A> for SockJSContext<A> where A: Session<Context=Self>
{
    fn spawn<F>(&mut self, fut: F) -> actix::SpawnHandle
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.inner.spawn(fut)
    }

    fn wait<F>(&mut self, fut: F)
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.inner.wait(fut);
    }

    #[doc(hidden)]
        #[inline]
    fn waiting(&self) -> bool {
        self.inner.waiting() || self.inner.state() == ActorState::Stopping ||
            self.inner.state() == ActorState::Stopped
    }

    fn cancel_future(&mut self, handle: SpawnHandle) -> bool {
        self.inner.cancel_future(handle)
    }

    #[inline]
    fn unsync_address(&mut self) -> Addr<Unsync, A> {
        self.inner.unsync_address()
    }

    #[inline]
    fn sync_address(&mut self) -> Addr<Syn, A> {
        self.inner.sync_address()
    }
}

impl<A> SockJSContext<A> where A: Session<Context=Self>
{
    #[doc(hidden)]
    pub fn recipient<M>(&mut self) -> Recipient<Unsync, M>
        where A: Handler<M>, M: ActixMessage + 'static
    {
        self.inner.unsync_address().recipient()
    }

    /// Session id
    pub fn sid(&self) -> &Arc<String> {
        &self.sid
    }

    /// Send message to peer
    pub fn send<M>(&mut self, message: M) where M: Into<Message> {
        self.send_frame(Frame::Message(message.into().0));
    }

    /// Send message to all sessions
    pub fn broadcast<M>(&mut self, message: M) where M: Into<Message> {
        self.sm.do_send(Broadcast::new(Frame::Message(message.into().0)));
    }

    /// Close session
    pub fn close(&mut self) {
        self.send_frame(Frame::Close(CloseCode::GoAway));
    }

    fn send_frame(&mut self, frm: Frame) {
        let msg = if let Some(ref mut tx) = self.tx {
            match tx.unbounded_send(ChannelItem::Frame(frm)) {
                Ok(()) => return,
                Err(err) => {
                    match err.into_inner() {
                        ChannelItem::Frame(frm) => frm,
                        _ => unreachable!()
                    }
                }
            }
        } else {
            frm
        };
        self.add_to_buf(msg);
        self.tx.take();
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
        } else if msg.is_msg() {
            self.buf.push_back(BufItem::Message(msg.into_message()));
        } else {
            self.buf.push_back(BufItem::Frame(msg));
        }
    }
}

impl<A> SockJSContext<A> where A: Session<Context=Self>
{
    pub(crate) fn start(session: A, sid: Arc<String>, addr: Addr<Syn, SockJSManager<A>>)
                        -> (Addr<Syn, A>, UnboundedSender<SockJSChannel>)
    {
        let (tx, rx) = unbounded();

        let mut ctx = SockJSContext {
            sid, rx,
            inner: ContextImpl::new(Some(session)),
            tx: None,
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

        match self.inner.poll(ctx) {
            Ok(Async::NotReady) => {
                // sockjs channel
                loop {
                    match self.rx.poll() {
                        Ok(Async::Ready(Some(msg))) => {
                            match msg {
                                SockJSChannel::Opened => {
                                    self.inner.actor().opened(ctx);
                                },
                                SockJSChannel::Acquired(tx) => {
                                    while let Some(msg) = self.buf.pop_front() {
                                        match msg {
                                            BufItem::Message(msg) => {
                                                let _ = tx.unbounded_send(
                                                    ChannelItem::Frame(Frame::Message(msg)));
                                            },
                                            BufItem::Messages(msg) => {
                                                let _ = tx.unbounded_send(
                                                    ChannelItem::Frame(
                                                        Frame::MessageVec(
                                                            serde_json::to_string(&msg).unwrap())));
                                            },
                                            BufItem::Frame(msg) => {
                                                let _ = tx.unbounded_send(ChannelItem::Frame(msg));
                                            },
                                        }
                                    };
                                    let _ = tx.unbounded_send(ChannelItem::Ready);
                                    self.tx = Some(tx);
                                    self.inner.actor().acquired(ctx);
                                }
                                SockJSChannel::Released => {
                                    self.tx.take();
                                    self.inner.actor().released(ctx);
                                },
                                SockJSChannel::Closed(reason) => {
                                    self.tx.take();
                                    self.inner.actor().closed(ctx, reason);
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
                Ok(Async::NotReady)
            },
            Ok(Async::Ready(())) => Ok(Async::Ready(())),
            Err(()) => Err(())
        }
    }
}

impl<A, M> ToEnvelope<Syn, A, M> for SockJSContext<A>
    where A: Session<Context=SockJSContext<A>> + Handler<M>,
          M: ActixMessage + Send + 'static, M::Result: Send,
{
    fn pack(msg: M, tx: Option<Sender<M::Result>>) -> SyncEnvelope<A> {
        SyncEnvelope::new(msg, tx)
    }
}
