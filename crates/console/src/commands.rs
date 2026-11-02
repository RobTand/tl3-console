//! `POST /api/requests/{id}/commands` (PRD #327, C3). The console files the
//! command for tl3d and answers 202 with its id; the outcome arrives on the
//! stream. Writes need the console's own header, which a cross-site form
//! can't send, and a same-origin browser.

use crate::api::{CommandRequest, Filed};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::sync::{Arc, Mutex};
use tl3_controller::console::{Action, Choice, CommandRecord};
use tl3_store::ConsoleCommands;

/// Browsers never send this header cross-site without a CORS preflight,
/// which the console never grants.
pub const HEADER: &str = "x-tl3-console";

/// TL3 refuses Rob's words beyond 20,000 characters, so the console does too,
/// before filing them.
const MAX_TEXT_CHARS: usize = 20_000;
/// Room for that text with every character JSON-escaped, and the rest.
const MAX_BODY_BYTES: usize = 4 * 1024 + 6 * MAX_TEXT_CHARS;

#[derive(Clone)]
struct Filer {
    settings: tl3_config::Store,
    session: Arc<Mutex<Option<ConsoleCommands>>>,
}

impl Filer {
    /// File on the open session; a broken session is replaced once.
    fn submit(&self, id: &str, node: u64, action: &serde_json::Value) -> tl3_store::Result<bool> {
        let mut session = self.session.lock().expect("command session");
        let mut retried = false;
        loop {
            if session.is_none() {
                *session = Some(ConsoleCommands::open(&self.settings)?);
            }
            match session
                .as_mut()
                .expect("an open session")
                .submit(id, node, action)
            {
                Err(error) if !retried && is_connection(&error) => {
                    retried = true;
                    *session = None;
                }
                result => return result,
            }
        }
    }
}

/// The SQLSTATE of a store error that Postgres raised.
fn sqlstate(error: &tl3_store::Error) -> Option<&str> {
    match error {
        tl3_store::Error::Database(error) => error.code().map(|code| code.code()),
        _ => None,
    }
}

fn is_connection(error: &tl3_store::Error) -> bool {
    matches!(error, tl3_store::Error::Database(error) if error.is_closed() || error.as_db_error().is_none())
}

pub(crate) fn router(settings: tl3_config::Store) -> Router {
    Router::new()
        .route("/api/requests/{id}/commands", post(file))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(Filer {
            settings,
            session: Arc::default(),
        })
}

fn refused(status: StatusCode, reason: &'static str) -> Response {
    (status, format!("{reason}\n")).into_response()
}

/// The write came from this console's own page.
fn same_origin(headers: &HeaderMap) -> bool {
    if headers.get(HEADER).is_none_or(|value| value != "1") {
        return false;
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site != "same-origin")
    {
        return false;
    }
    match headers.get(header::ORIGIN) {
        None => true,
        Some(origin) => {
            let Some(host) = headers
                .get(header::HOST)
                .and_then(|host| host.to_str().ok())
            else {
                return false;
            };
            origin
                .to_str()
                .ok()
                .and_then(|origin| origin.split_once("://"))
                .is_some_and(|(scheme, rest)| matches!(scheme, "http" | "https") && rest == host)
        }
    }
}

fn is_uuid(id: &str) -> bool {
    let groups: Vec<_> = id.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len)
        && id
            .chars()
            .all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c))
}

async fn file(
    State(filer): State<Filer>,
    Path(node): Path<u64>,
    headers: HeaderMap,
    body: std::result::Result<Json<CommandRequest>, JsonRejection>,
) -> Response {
    if !same_origin(&headers) {
        return refused(
            StatusCode::FORBIDDEN,
            "Commands come only from the console's own page.",
        );
    }
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return refused(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Commands are at most a few KiB plus 20,000 characters of your words.",
            );
        }
        Err(_) => {
            return refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                "The console couldn't read this command.",
            );
        }
    };
    if words(&request.action).is_some_and(|text| text.chars().count() > MAX_TEXT_CHARS) {
        return refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Answers can be at most 20,000 characters.",
        );
    }
    if !is_uuid(&request.id) {
        return refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Command ids are lowercase UUIDs.",
        );
    }
    let action = serde_json::to_value(&request.action).expect("actions serialize");
    let id = request.id.clone();
    let filed = tokio::task::spawn_blocking(move || filer.submit(&request.id, node, &action)).await;
    match filed {
        Ok(Ok(filed)) => (StatusCode::ACCEPTED, Json(Filed { id, filed })).into_response(),
        Ok(Err(error)) => {
            let _ = sqlstate(&error);
            eprintln!(
                "console command {id} not filed: {}",
                crate::describe(&error)
            );
            refused(
                StatusCode::SERVICE_UNAVAILABLE,
                "The console couldn't reach the store; try again.",
            )
        }
        Err(_) => refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "The console couldn't file this command; try again.",
        ),
    }
}

/// Long texts a `commands` event repeats are cut to this many characters.
pub(crate) const SHOWN_CHARS: usize = 200;

/// Rob's own words in an action, if it carries any.
fn words(action: &Action) -> Option<&String> {
    match action {
        Action::Comment { text }
        | Action::Answer {
            choice: Choice::ChangeRequest { text } | Choice::TryAgain { guidance: text },
            ..
        } => Some(text),
        _ => None,
    }
}

/// The newest commands as the stream shows them: Rob's long words abbreviated.
pub(crate) fn brief(mut records: Vec<CommandRecord>) -> Vec<CommandRecord> {
    for record in &mut records {
        let text = match &mut record.action {
            Some(Action::Comment { text })
            | Some(Action::Answer {
                choice: Choice::ChangeRequest { text } | Choice::TryAgain { guidance: text },
                ..
            }) => text,
            _ => continue,
        };
        if let Some((cut, _)) = text.char_indices().nth(SHOWN_CHARS) {
            text.truncate(cut);
            text.push('…');
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use tl3_controller::console::{Action, Choice, CommandStatus};

    fn record(action: Action) -> CommandRecord {
        CommandRecord {
            id: "x".into(),
            node: 1,
            action: Some(action),
            status: CommandStatus::Pending,
            reason: None,
            received_ms: 1,
            settled_ms: None,
        }
    }

    #[test]
    fn the_stream_abbreviates_long_words() {
        let long = "é".repeat(20_000);
        let shown = brief(vec![
            record(Action::Comment { text: long.clone() }),
            record(Action::Answer {
                question: 3,
                choice: Choice::ChangeRequest { text: long.clone() },
            }),
            record(Action::Answer {
                question: 3,
                choice: Choice::TryAgain { guidance: long },
            }),
            record(Action::Comment {
                text: "short".into(),
            }),
        ]);
        let expected = format!("{}…", "é".repeat(SHOWN_CHARS));
        assert_eq!(
            shown[0].action,
            Some(Action::Comment {
                text: expected.clone()
            })
        );
        assert_eq!(
            shown[1].action,
            Some(Action::Answer {
                question: 3,
                choice: Choice::ChangeRequest {
                    text: expected.clone()
                }
            })
        );
        assert_eq!(
            shown[2].action,
            Some(Action::Answer {
                question: 3,
                choice: Choice::TryAgain { guidance: expected }
            })
        );
        assert_eq!(
            shown[3].action,
            Some(Action::Comment {
                text: "short".into()
            })
        );
    }

    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn writes_need_the_console_header_and_the_same_origin() {
        let own = [("host", "192.168.1.107:8034"), (HEADER, "1")];
        assert!(same_origin(&headers(&own)));
        assert!(same_origin(&headers(&[
            own[0],
            own[1],
            ("origin", "http://192.168.1.107:8034"),
            ("sec-fetch-site", "same-origin"),
        ])));
        for case in [
            vec![own[0]],
            vec![own[0], (HEADER, "0")],
            vec![own[0], own[1], ("origin", "http://evil.example")],
            vec![
                own[0],
                own[1],
                ("origin", "http://192.168.1.107:8034.evil.example"),
            ],
            vec![own[0], own[1], ("origin", "null")],
            vec![own[0], own[1], ("sec-fetch-site", "cross-site")],
            vec![own[0], own[1], ("sec-fetch-site", "same-site")],
            vec![own[1], ("origin", "http://192.168.1.107:8034")],
        ] {
            assert!(!same_origin(&headers(&case)), "{case:?}");
        }
    }

    #[test]
    fn ids_are_lowercase_uuids() {
        assert!(is_uuid("0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a11"));
        for id in [
            "",
            "0B9F3C3E-5F43-4A8E-9D1C-2F0E7C1D9A11",
            "0b9f3c3e5f434a8e9d1c2f0e7c1d9a11",
            "0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a1",
            "0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a1g",
        ] {
            assert!(!is_uuid(id), "{id}");
        }
    }
}
