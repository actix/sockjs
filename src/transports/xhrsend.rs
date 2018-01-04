use std::sync::Arc;

use actix::*;
use actix_web::*;
use serde_json;
use futures::future::{ok, Future, Either};
use http::header::ACCESS_CONTROL_ALLOW_METHODS;

use utils::SockjsHeaders;
use session::{Message, Session};
use manager::{SessionManager, SessionMessage};

use super::MAXSIZE;


#[allow(non_snake_case)]
pub fn XhrSend<S, SM>(req: HttpRequest<SyncAddress<SM>>)
                      -> Either<HttpResponse, Box<Future<Item=HttpResponse, Error=Error>>>
    where S: Session, SM: SessionManager<S>
{
    if *req.method() == Method::OPTIONS {
        Either::A(httpcodes::HTTPNoContent
                  .build()
                  .content_type("application/jsonscript; charset=UTF-8")
                  .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, POST")
                  .sockjs_cache_headers()
                  .sockjs_cors_headers(req.headers())
                  .sockjs_session_cookie(&req)
                  .finish()
                  .unwrap())
    }
    else if *req.method() != Method::GET && *req.method() != Method::POST {
        Either::A(httpcodes::HTTPForbidden.with_reason("Method is not allowed"))
    } else {
        Either::B(read(req))
    }
}

pub fn read<S, SM>(req: HttpRequest<SyncAddress<SM>>)
                   -> Box<Future<Item=HttpResponse, Error=Error>>
    where S: Session, SM: SessionManager<S>
{
    let sid = req.match_info().get("session").unwrap().to_owned();

    Box::new(
        req.body().limit(MAXSIZE)
            .map_err(|e| Error::from(error::ErrorBadRequest(e)))
            .and_then(move |buf| {
                let sid = Arc::new(sid);

                // empty message
                if buf.is_empty() {
                    return Either::A(
                        ok(httpcodes::HTTPInternalServerError.with_body("Payload expected.")))
                } else if buf != b"[]".as_ref() {
                    // deserialize json
                    let mut msgs: Vec<String> = match serde_json::from_slice(&buf) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            return Either::A(
                                ok(httpcodes::HTTPInternalServerError
                                   .with_body("Broken JSON encoding.")))
                        }
                    };

                    if !msgs.is_empty() {
                        let last = msgs.pop().unwrap();
                        for msg in msgs {
                            req.state().send(
                                SessionMessage {
                                    sid: sid.clone(),
                                    msg: Message(msg)});
                        }

                        return Either::B(
                            req.state().call_fut(SessionMessage { sid: sid.clone(),
                                                                  msg: Message(last)})
                                .from_err()
                                .and_then(move |res| {
                                    match res {
                                        Ok(_) => {
                                            Ok(httpcodes::HTTPNoContent
                                               .build()
                                               .content_type("text/plain; charset=UTF-8")
                                               .sockjs_no_cache()
                                               .sockjs_cors_headers(req.headers())
                                               .sockjs_session_cookie(&req)
                                               .finish()
                                               .unwrap())
                                        },
                                        Err(e) => Err(Error::from(error::ErrorNotFound(e))),
                                    }
                                }))
                    }
                }

                Either::A(ok(httpcodes::HTTPNoContent
                             .build()
                             .content_type("text/plain; charset=UTF-8")
                             .sockjs_no_cache()
                             .sockjs_cors_headers(req.headers())
                             .sockjs_session_cookie(&req)
                             .finish()
                             .unwrap()))
            }))
}
