#![allow(dead_code, unused_variables)]
use std::fmt::Debug;

use session::SessionError;

pub enum Message {
    Open,
    Msg,
    Close,
    Closed,
}

pub enum Frame {
    Open,
    Close(CloseCode),
    Message,
    MessageBlob,
    Heartbeat,
}

pub enum CloseCode {
    Interrupted,
    GoAway,
    Acquired,
}

impl CloseCode {
    pub fn num(&self) -> usize {
        match *self {
            CloseCode::Interrupted => 1002,
            CloseCode::GoAway => 3000,
            CloseCode::Acquired => 2010,
        }
    }

    pub fn reason(&self) -> &'static str {
        match *self {
            CloseCode::Interrupted => "Connection interrupted",
            CloseCode::GoAway => "Go away!",
            CloseCode::Acquired => "Another connection still open",
        }
    }
}

impl From<SessionError> for Frame {
    fn from(err: SessionError) -> Frame {
        match err {
            SessionError::Acquired => Frame::Close(CloseCode::Acquired),
            SessionError::Interrupted => Frame::Close(CloseCode::Interrupted),
            SessionError::Closing => Frame::Close(CloseCode::GoAway),
        }
    }
}

pub const IFRAME_HTML: &'static str = r#"<!DOCTYPE html>
<html>
<head>
<meta http-equiv="X-UA-Compatible" content="IE=edge" />
<meta http-equiv="Content-Type" content="text/html; charset=UTF-8" />
  <script src="https://cdn.jsdelivr.net/npm/sockjs-client@1/dist/sockjs.min.js"></script>
  <script>
    document.domain = document.domain;
    SockJS.bootstrap_iframe();
  </script>
</head>
<body>
  <h2>Don't panic!</h2>
  <p>This is a SockJS hidden iframe. It's used for cross domain magic.</p>
</body>
</html>"#;
