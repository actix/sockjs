use std::sync::Arc;

use actix::*;
use actix_web::http::Method;
use actix_web::*;
use futures::future::{ok, Either, Future};
use http::header::ACCESS_CONTROL_ALLOW_METHODS;
use serde_json;

use manager::{SessionManager, SessionMessage};
use session::{Message, Session};
use utils::SockjsHeaders;

use super::MAXSIZE;

#[allow(non_snake_case)]
pub fn XhrSend<S, SM>(
    req: HttpRequest<Addr<Syn, SM>>,
) -> Either<HttpResponse, Box<Future<Item = HttpResponse, Error = Error>>>
where
    S: Session,
    SM: SessionManager<S>,
{
    if *req.method() == Method::OPTIONS {
        Either::A(
            HttpResponse::NoContent()
                .content_type("application/jsonscript; charset=UTF-8")
                .header(ACCESS_CONTROL_ALLOW_METHODS, "OPTIONS, POST")
                .sockjs_cache_headers()
                .sockjs_cors_headers(req.headers())
                .sockjs_session_cookie(&req)
                .finish(),
        )
    } else if *req.method() != Method::GET && *req.method() != Method::POST {
        Either::A(
            HttpResponse::Forbidden()
                .reason("Method is not allowed")
                .finish(),
        )
    } else {
        Either::B(read(req))
    }
}

pub fn read<S, SM>(
    req: HttpRequest<Addr<Syn, SM>>,
) -> Box<Future<Item = HttpResponse, Error = Error>>
where
    S: Session,
    SM: SessionManager<S>,
{
    let sid = req.match_info().get("session").unwrap().to_owned();

    Box::new(
        req.clone()
            .body()
            .limit(MAXSIZE)
            .map_err(error::ErrorBadRequest)
            .and_then(move |buf| {
                let sid = Arc::new(sid);

                // empty message
                if buf.is_empty() {
                    return Either::A(ok(
                        HttpResponse::InternalServerError().body("Payload expected.")
                    ));
                } else if buf != b"[]".as_ref() {
                    // deserialize json
                    let mut msgs: Vec<String> = match serde_json::from_slice(&buf) {
                        Ok(msgs) => msgs,
                        Err(_) => {
                            return Either::A(ok(
                                HttpResponse::InternalServerError().body("Broken JSON encoding.")
                            ))
                        }
                    };

                    if !msgs.is_empty() {
                        let last = msgs.pop().unwrap();
                        for msg in msgs {
                            req.state().do_send(SessionMessage {
                                sid: Arc::clone(&sid),
                                msg: Message(msg),
                            });
                        }

                        return Either::B(
                            req.state()
                                .send(SessionMessage {
                                    sid: Arc::clone(&sid),
                                    msg: Message(last),
                                })
                                .from_err()
                                .and_then(move |res| match res {
                                    Ok(_) => Ok(HttpResponse::NoContent()
                                        .content_type("text/plain; charset=UTF-8")
                                        .sockjs_no_cache()
                                        .sockjs_cors_headers(req.headers())
                                        .sockjs_session_cookie(&req)
                                        .finish()),
                                    Err(_) => Err(error::ErrorNotFound("not found")),
                                }),
                        );
                    }
                }

                Either::A(ok(HttpResponse::NoContent()
                    .content_type("text/plain; charset=UTF-8")
                    .sockjs_no_cache()
                    .sockjs_cors_headers(req.headers())
                    .sockjs_session_cookie(&req)
                    .finish()))
            }),
    )
}
