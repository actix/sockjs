#![allow(dead_code, unused_variables)]

use std;

use bytes::Bytes;
use futures::{Async, Future, Poll};
use actix::dev::*;

/// SockJS Context
pub struct SockJSContext<A> where A: Actor<Context=SockJSContext<A>>,
{
    act: A,
    state: ActorState,
    items: ActorItemsCell<A>,
    address: ActorAddressCell<A>,
    wait: Option<Box<ActorFuture<Item=(), Error=(), Actor=A>>>,
}

impl<A> ActorContext<A> for SockJSContext<A> where A: Actor<Context=Self>
{
    /// Stop actor execution
    fn stop(&mut self) {
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

impl<A> AsyncContext<A> for SockJSContext<A> where A: Actor<Context=Self>
{
    fn spawn<F>(&mut self, fut: F) -> SpawnHandle
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.items.spawn(fut)
    }

    fn wait<F>(&mut self, fut: F)
        where F: ActorFuture<Item=(), Error=(), Actor=A> + 'static
    {
        self.wait = Some(Box::new(fut));
    }

    fn cancel_future(&mut self, handle: SpawnHandle) -> bool {
        self.items.cancel_future(handle)
    }
}

impl<A> AsyncContextApi<A> for SockJSContext<A> where A: Actor<Context=Self> {
    fn address_cell(&mut self) -> &mut ActorAddressCell<A> {
        &mut self.address
    }
}

impl<A> SockJSContext<A> where A: Actor<Context=Self>
{
    #[doc(hidden)]
    pub fn subscriber<M: 'static>(&mut self) -> Box<Subscriber<M>>
        where A: Handler<M>
    {
        Box::new(self.address.unsync_address())
    }

    /// Manually expire a session
    pub fn expire(&mut self) {
        unimplemented!()
    }

    /// Send message to peer
    pub fn send(&mut self) {
        unimplemented!()
    }

    /// Close session
    pub fn close(&mut self, code: usize, reason: String) {
        unimplemented!()
    }
}

impl<A> SockJSContext<A> where A: Actor<Context=Self>
{
    pub(crate) fn new(act: A) -> SockJSContext<A>
    {
        SockJSContext {
            act: act,
            state: ActorState::Started,
            items: ActorItemsCell::default(),
            address: ActorAddressCell::default(),
            wait: None,
        }
    }

    pub(crate) fn alive(&mut self) -> bool {
        if self.state == ActorState::Stopped {
            false
        } else {
            self.address.connected() || !self.items.is_empty()
        }
    }

    pub(crate) fn replace_actor(&mut self, srv: A) -> A {
        std::mem::replace(&mut self.act, srv)
    }
}

#[doc(hidden)]
impl<A> Future for SockJSContext<A> where A: Actor<Context=Self>
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
            },
            ActorState::Stopping => {
                Actor::stopping(&mut self.act, ctx);
            }
            _ => ()
        }

        // check wait future
        if self.wait.is_some() {
            if let Some(ref mut fut) = self.wait {
                if let Ok(Async::NotReady) = fut.poll(&mut self.act, ctx) {
                    return Ok(Async::NotReady)
                }
            }
            self.wait = None;
        }

        let mut prep_stop = false;
        loop {
            let mut not_ready = true;

            if let Ok(Async::Ready(_)) = self.address.poll(&mut self.act, ctx) {
                not_ready = false
            }

            self.items.poll(&mut self.act, ctx);

            // are we done
            if !not_ready {
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

impl<A> std::fmt::Debug for SockJSContext<A> where A: Actor<Context=Self> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SockJSContext({:?}: actor:{:?}) {{ state: {:?}, connected: {}, items: {} }}",
               self as *const _,
               &self.act as *const _,
               self.state, "-", self.items.is_empty())
    }
}
