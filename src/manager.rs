use std::sync::Arc;
use std::ops::Deref;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, Duration};
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use actix::*;
use protocol::Frame;
use context::{SockJSContext, SockJSChannel};
use session::{Message, Session, SessionState, SessionError, CloseReason};

#[doc(hidden)]
pub trait SessionManager<S>: Actor +
    Handler<Acquire> + Handler<Release> + Handler<SessionMessage> {}

/// Acquire message
pub struct Acquire {
    sid: Arc<String>,
    addr: Box<Subscriber<Broadcast> + Send>,
}
impl Acquire {
    pub fn new(sid: String, addr: Box<Subscriber<Broadcast> + Send>) -> Self {
        Acquire{
            sid: Arc::new(sid),
            addr: addr,
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
    pub sid: Arc<String>,
    pub msg: Message,
}

impl ResponseType for SessionMessage {
    type Item = ();
    type Error = ();
}

/// Broadcast message to all sessions
#[derive(Debug)]
pub struct Broadcast {
    pub msg: Arc<Frame>,
}

impl ResponseType for Broadcast {
    type Item = ();
    type Error = ();
}

impl Broadcast {
    pub fn new(frm: Frame) -> Broadcast {
        Broadcast {msg: Arc::new(frm)}
    }
}

impl Clone for Broadcast {
    fn clone(&self) -> Broadcast {
        Broadcast {msg: Arc::clone(&self.msg)}
    }
}

pub enum RecordEntry {
    Frame(Frame),
    Arc(Arc<Frame>),
}

impl AsRef<Frame> for RecordEntry {
    fn as_ref(&self) -> &Frame {
        match *self {
            RecordEntry::Frame(ref frame) => frame,
            RecordEntry::Arc(ref frame) => frame.as_ref(),
        }
    }
}

impl Deref for RecordEntry {
    type Target = Frame;

    fn deref(&self) -> &Frame {
        match *self {
            RecordEntry::Frame(ref frame) => frame,
            RecordEntry::Arc(ref frame) => frame.as_ref(),
        }
    }
}

impl From<Frame> for RecordEntry {
    fn from(f: Frame) -> RecordEntry {
        RecordEntry::Frame(f)
    }
}

impl From<Arc<Frame>> for RecordEntry {
    fn from(f: Arc<Frame>) -> RecordEntry {
        RecordEntry::Arc(f)
    }
}

impl From<Broadcast> for RecordEntry {
    fn from(f: Broadcast) -> RecordEntry {
        RecordEntry::Arc(f.msg)
    }
}

/// Session record
pub struct Record {
    /// Session id
    pub sid: Arc<String>,
    /// Session state
    pub state: SessionState,
    /// Peer messages, buffer for peer messages when transport is not connected
    pub buffer: VecDeque<RecordEntry>,
    /// Channel to context
    tx: UnboundedSender<SockJSChannel>,
}

impl Record {
    fn new(id: Arc<String>, tx: UnboundedSender<SockJSChannel>) -> Record {
        Record {
            sid: id,
            state: SessionState::New,
            buffer: VecDeque::new(),
            tx: tx,
        }
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }

    pub fn interrupted(&mut self) {
        if self.state == SessionState::Running {
            self.state = SessionState::Interrupted;
        }
    }

    pub fn add<F: Into<RecordEntry>>(&mut self, frm: F) {
        self.buffer.push_back(frm.into())
    }
}

struct Entry<S: Session> {
    addr: SyncAddress<S>,
    record: Option<Record>,
    transport: Option<Box<Subscriber<Broadcast> + Send>>,
    /// heartbeat
    tick: Instant,
}

/// Session manager
pub struct SockJSManager<S: Session> {
    idle: HashSet<Arc<String>>,
    sessions: HashMap<Arc<String>, Entry<S>>,
}

impl<S: Session> SessionManager<S> for SockJSManager<S> {}

impl<S: Session> Default for SockJSManager<S> {
    fn default() -> SockJSManager<S> {
        SockJSManager {
            idle: HashSet::new(),
            sessions: HashMap::new(),
        }
    }
}

impl<S: Session> SockJSManager<S> {

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.hb(ctx);

            let now = Instant::now();
            let mut rem = Vec::new();
            for sid in &act.idle {
                if let Some(entry) = act.sessions.get(sid) {
                    if entry.tick + Duration::new(10, 0) < now {
                        rem.push(sid.clone());
                    }
                }
            }

            for sid in rem {
                act.idle.remove(&sid);
                if let Some(entry) = act.sessions.remove(&sid) {
                    if let Some(rec) = entry.record {
                        let _ = rec.tx.unbounded_send(
                            SockJSChannel::Closed(CloseReason::Expired));
                    }
                }
            }
        });
    }
}

impl<S: Session> Actor for SockJSManager<S> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.hb(ctx)
    }
}

#[doc(hidden)]
impl<S: Session> Handler<Acquire> for SockJSManager<S> {
    fn handle(&mut self, msg: Acquire, ctx: &mut Context<Self>) -> Response<Self, Acquire>
    {
        if let Some(entry) = self.sessions.get_mut(&msg.sid) {
            if let Some(rec) = entry.record.take() {
                let (tx, rx) = unbounded();
                let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
                self.idle.remove(&msg.sid);
                entry.transport = Some(msg.addr);
                return Self::reply((rec, rx))
            } else {
                return Self::reply_error(SessionError::Acquired)
            }
        }
        let (addr, tx) = SockJSContext::start(Arc::clone(&msg.sid), ctx.address());
        self.sessions.insert(
            msg.sid.clone(),
            Entry{addr: addr,
                  record: None,
                  transport: Some(msg.addr),
                  tick: Instant::now(),
            });
        let rec = Record::new(msg.sid, tx);
        let (tx, rx) = unbounded();
        let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
        Self::reply((rec, rx))
    }
}

#[doc(hidden)]
impl<S: Session> Handler<Release> for SockJSManager<S> {
    fn handle(&mut self, msg: Release, _: &mut Context<Self>)
              -> Response<Self, Release>
    {
        if let Some(entry) = self.sessions.get_mut(&msg.ses.sid) {
            self.idle.insert(msg.ses.sid.clone());
            let _ = match msg.ses.state {
                SessionState::Closed =>
                    msg.ses.tx.unbounded_send(
                        SockJSChannel::Closed(CloseReason::Normal)),
                SessionState::Interrupted =>
                    msg.ses.tx.unbounded_send(
                        SockJSChannel::Closed(CloseReason::Interrupted)),
                _ => msg.ses.tx.unbounded_send(SockJSChannel::Released)
            };
            entry.tick = Instant::now();
            entry.record = Some(msg.ses);
            entry.transport.take();
        }
        Self::empty()
    }
}

#[doc(hidden)]
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

#[doc(hidden)]
impl<S: Session> Handler<Broadcast> for SockJSManager<S> {
    fn handle(&mut self, msg: Broadcast, _: &mut Context<Self>)
              -> Response<Self, Broadcast>
    {
        for entry in self.sessions.values_mut() {
            if let Some(ref tr) = entry.transport {
                let _ = tr.send(msg.clone());
                continue
            }
            if let Some(ref mut rec) = entry.record {
                rec.add(Arc::clone(&msg.msg));
            }
        }
        Self::empty()
    }
}
