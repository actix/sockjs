use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, Duration};
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use actix::*;
use protocol::Frame;
use context::{SockJSContext, SockJSChannel};
use session::{Message, Session, SessionState, SessionError};

pub trait SessionManager<S: Session>: Actor +
    Handler<Acquire> + Handler<Release> + Handler<SessionMessage> {}

/// Acquire message
#[derive(Debug)]
pub struct Acquire {
    sid: String,
}
impl Acquire {
    pub fn new(sid: String) -> Self {
        Acquire{
            sid: sid,
        }
    }
}

impl ResponseType for Acquire {
    type Item = (Record, UnboundedReceiver<Frame>);
    type Error = SessionError;
}

/// Release message
pub struct Release {
    pub ses: Record,
}

impl ResponseType for Release {
    type Item = ();
    type Error = ();
}

/// Session message
#[derive(Debug)]
pub struct SessionMessage {
    pub sid: String,
    pub msg: Message,
}

impl ResponseType for SessionMessage {
    type Item = ();
    type Error = ();
}

/// Session record
#[derive(Debug)]
pub struct Record {
    /// Session id
    pub sid: String,
    /// Session state
    pub state: SessionState,
    /// Peer messages, buffer for peer messages when transport is not connected
    pub buffer: VecDeque<Frame>,
    /// heartbeat
    tick: Instant,
    /// Channel to context
    tx: UnboundedSender<SockJSChannel>,
}

impl Record {
    fn new(id: String, tx: UnboundedSender<SockJSChannel>) -> Record {
        Record {
            sid: id,
            state: SessionState::New,
            buffer: VecDeque::new(),
            tick: Instant::now(),
            tx: tx,
        }
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }
}

struct Entry<S: Session> {
    addr: SyncAddress<S>,
    record: Option<Record>,
}

/*impl<S: Session> Entry<S> {
    fn is_acquired(&self) -> bool {
        self.record.is_none()
    }
}*/

/// Session manager
pub struct SockJSManager<S: Session> {
    idle: HashSet<String>,
    sessions: HashMap<String, Entry<S>>,
}

impl<S: Session> SessionManager<S> for SockJSManager<S> {}

impl<S: Session> SockJSManager<S> {
    pub fn new() -> SockJSManager<S> {
        SockJSManager {
            idle: HashSet::new(),
            sessions: HashMap::new(),
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.hb(ctx);
        });
    }
}

impl<S: Session> Actor for SockJSManager<S> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.hb(ctx)
    }
}

impl<S: Session> Handler<Acquire> for SockJSManager<S> {
    fn handle(&mut self, msg: Acquire, _: &mut Context<Self>) -> Response<Self, Acquire>
    {
        if let Some(val) = self.sessions.get_mut(&msg.sid) {
            if let Some(rec) = val.record.take() {
                let (tx, rx) = unbounded();
                let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
                self.idle.remove(&msg.sid);
                return Self::reply((rec, rx))
            } else {
                return Self::reply_error(SessionError::Acquired)
            }
        }
        let (addr, tx) = SockJSContext::start();
        self.sessions.insert(msg.sid.clone(), Entry{addr: addr, record: None});
        let rec = Record::new(msg.sid, tx);
        let (tx, rx) = unbounded();
        let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
        Self::reply((rec, rx))
    }
}

impl<S: Session> Handler<Release> for SockJSManager<S> {
    fn handle(&mut self, mut msg: Release, _: &mut Context<Self>)
              -> Response<Self, Release>
    {
        if let Some(val) = self.sessions.get_mut(&msg.ses.sid) {
            self.idle.insert(msg.ses.sid.clone());
            msg.ses.tick = Instant::now();
            let _ = msg.ses.tx.unbounded_send(SockJSChannel::Released);
            val.record = Some(msg.ses);
        }
        Self::empty()
    }
}

impl<S: Session> Handler<SessionMessage> for SockJSManager<S> {
    fn handle(&mut self, msg: SessionMessage, _: &mut Context<Self>)
              -> Response<Self, SessionMessage>
    {
        if let Some(entry) = self.sessions.get_mut(&msg.sid) {
            entry.addr.send(msg.msg);
            Self::empty()
        } else {
            Self::reply_error(())
        }
    }
}
