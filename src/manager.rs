use std::sync::Arc;
use std::ops::Deref;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, Duration};
use futures::sync::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use actix::*;
use actix::Message as ActixMessage;
use protocol::Frame;
use context::{SockJSContext, SockJSChannel, ChannelItem};
use session::{Message, Session, SessionState, SessionError, CloseReason};

#[doc(hidden)]
pub trait SessionManager<S>: Actor<Context=Context<Self>> +
    Handler<Acquire> + Handler<Release> + Handler<SessionMessage> {}

/// Acquire message
pub struct Acquire {
    sid: Arc<String>,
    addr: Recipient<Syn, Broadcast>,
}
impl Acquire {
    pub fn new(sid: String, addr: Recipient<Syn, Broadcast>) -> Self {
        Acquire{addr, sid: Arc::new(sid)}
    }
}

impl ActixMessage for Acquire {
    type Result = Result<(Record, UnboundedReceiver<ChannelItem>), SessionError>;
}

/// Release message
#[derive(Message)]
pub struct Release {
    pub ses: Record,
}

/// Session message
#[derive(Debug)]
pub struct SessionMessage {
    pub sid: Arc<String>,
    pub msg: Message,
}

impl ActixMessage for SessionMessage {
    type Result = Result<(), ()>;
}

/// Broadcast message to all sessions
#[derive(Debug, Message)]
pub struct Broadcast {
    pub msg: Arc<Frame>,
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

#[derive(Debug)]
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
    fn new(sid: Arc<String>, tx: UnboundedSender<SockJSChannel>) -> Record {
        Record {
            sid, tx,
            state: SessionState::New,
            buffer: VecDeque::new(),
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
    addr: Addr<Syn, S>,
    record: Option<Record>,
    transport: Option<Recipient<Syn, Broadcast>>,
    /// heartbeat
    tick: Instant,
}

/// Session manager
pub struct SockJSManager<S: Session> {
    idle: HashSet<Arc<String>>,
    sessions: HashMap<Arc<String>, Entry<S>>,
    factory: Box<Fn() -> S + Sync + Send>,
}

impl<S: Session> SessionManager<S> for SockJSManager<S> {}

impl<S: Session + Default> Default for SockJSManager<S> {
    fn default() -> SockJSManager<S> {
        SockJSManager {
            idle: HashSet::new(),
            sessions: HashMap::new(),
            factory: Box::new(S::default),
        }
    }
}

impl<S: Session> SockJSManager<S> {

    pub fn new<F>(factory: F) -> Self
        where F: Sync + Send + 'static + Fn() -> S,
    {
        SockJSManager {
            factory: Box::new(factory),
            idle: HashSet::new(),
            sessions: HashMap::new(),
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.hb(ctx);

            let now = Instant::now();
            let mut rem = Vec::new();
            for sid in &act.idle {
                if let Some(entry) = act.sessions.get(sid) {
                    if entry.tick + Duration::new(10, 0) < now {
                        rem.push(Arc::clone(sid));
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
    type Result = Result<(Record, UnboundedReceiver<ChannelItem>), SessionError>;

    fn handle(&mut self, msg: Acquire, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(entry) = self.sessions.get_mut(&msg.sid) {
            if let Some(rec) = entry.record.take() {
                let (tx, rx) = unbounded();
                let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
                self.idle.remove(&msg.sid);
                entry.transport = Some(msg.addr);
                return Ok((rec, rx))
            } else {
                return Err(SessionError::Acquired)
            }
        }
        let (addr, tx) = SockJSContext::start(
            (*self.factory)(), Arc::clone(&msg.sid), ctx.address());
        self.sessions.insert(
            Arc::clone(&msg.sid),
            Entry{addr,
                  record: None,
                  transport: Some(msg.addr),
                  tick: Instant::now(),
            });
        let rec = Record::new(msg.sid, tx);
        let (tx, rx) = unbounded();
        let _ = rec.tx.unbounded_send(SockJSChannel::Opened);
        let _ = rec.tx.unbounded_send(SockJSChannel::Acquired(tx));
        Ok((rec, rx))
    }
}

#[doc(hidden)]
impl<S: Session> Handler<Release> for SockJSManager<S> {
    type Result = ();

    fn handle(&mut self, msg: Release, _: &mut Context<Self>) {
        if let Some(entry) = self.sessions.get_mut(&msg.ses.sid) {
            self.idle.insert(Arc::clone(&msg.ses.sid));
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
    }
}

#[doc(hidden)]
impl<S: Session> Handler<SessionMessage> for SockJSManager<S> {
    type Result = Result<(), ()>;

    fn handle(&mut self, msg: SessionMessage, _: &mut Context<Self>) -> Self::Result {
        if let Some(entry) = self.sessions.get_mut(&msg.sid) {
            entry.addr.do_send(msg.msg);
            Ok(())
        } else {
            Err(())
        }
    }
}

#[doc(hidden)]
impl<S: Session> Handler<Broadcast> for SockJSManager<S> {
    type Result = ();

    fn handle(&mut self, msg: Broadcast, _: &mut Context<Self>) {
        for entry in self.sessions.values_mut() {
            if let Some(ref tr) = entry.transport {
                let _ = tr.send(msg.clone());
                continue
            }
            if let Some(ref mut rec) = entry.record {
                rec.add(Arc::clone(&msg.msg));
            }
        }
    }
}
