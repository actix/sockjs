#![allow(dead_code, unused_variables)]
use std::collections::HashMap;
use std::time::{Instant, Duration};
use std::marker::PhantomData;

use actix::*;
use session::{State, Session, SessionError};
use transports::Message;


/// Acquire message
#[derive(Debug)]
pub struct Acquire<S: Session> {
    sid: String,
    ses: PhantomData<S>,
}
impl<S: Session> Acquire<S> {
    pub fn new(sid: String) -> Self {
        Acquire{
            sid: sid,
            ses: PhantomData,
        }
    }
}

unsafe impl<S: Session> Send for Acquire<S> {}

impl<S: Session> ResponseType for Acquire<S> {
    type Item = Record<S>;
    type Error = SessionError;
}

/// Release message
pub struct Release {
    pub sid: String,
}

impl ResponseType for Release {
    type Item = ();
    type Error = ();
}

/// Session record
pub struct Record<S: Session> {
    st: State,
    state: S::State,
    messages: Vec<S::Message>,
    tick: Instant,
}

impl<S: Session> Default for Record<S> {
    fn default() -> Record<S> {
        Record {
            st: State::New,
            state: S::State::default(),
            messages: Vec::new(),
            tick: Instant::now(),
        }
    }
}


/// Session manager
pub struct SessionManager<S: Session> {
    acquired: HashMap<String, Box<Subscriber<Message<S::Message>> + Send>>,
    sessions: HashMap<String, Option<Record<S>>>,
}

impl<S: Session> SessionManager<S> {
    pub fn new() -> SessionManager<S> {
        SessionManager {
            acquired: HashMap::new(),
            sessions: HashMap::new(),
        }
    }

    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(Duration::new(10, 0), |act, ctx| {
            act.hb(ctx);
        });
    }
}

impl<S: Session> Actor for SessionManager<S> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.hb(ctx)
    }
}

impl<S: Session> Handler<Acquire<S>> for SessionManager<S> {
    fn handle(&mut self, msg: Acquire<S>, ctx: &mut Context<Self>)
              -> Response<Self, Acquire<S>>
    {
        if let Some(ref val) = self.sessions.get_mut(&msg.sid) {
            if val.is_none() {
                println!("acquired");
            } else {
                println!("existing");
            }
            return Self::reply_error(SessionError::Acquired)
        }
        println!("create");
        self.sessions.insert(msg.sid, None);
        let rec = Record::default();
        Self::reply(rec)
    }
}

impl<S: Session + 'static> Handler<Release> for SessionManager<S> {
    fn handle(&mut self, msg: Release, ctx: &mut Context<Self>) -> Response<Self, Release> {
        unimplemented!()
    }
}
