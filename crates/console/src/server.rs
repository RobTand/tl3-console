//! HTTP: the board, its event stream and the built web app.

use crate::{
    Published,
    api::{CommandsEvent, DatabaseStatus, PatchEvent},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::get,
};
use futures_util::stream::{self, Stream};
use std::{collections::VecDeque, convert::Infallible, path::Path, sync::Arc, time::Duration};
use tl3_controller::{board::Board, console::CommandRecord};
use tokio::sync::watch;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
/// EventSource waits this long before reconnecting after a dropped stream.
const RETRY: Duration = Duration::from_secs(1);

pub(crate) fn router(
    published: watch::Receiver<Published>,
    assets: &Path,
    case_files: Router,
    hosts: crate::hosts::Hosts,
) -> Router {
    let files = ServeDir::new(assets).fallback(ServeFile::new(assets.join("index.html")));
    let app = Router::new()
        .route("/api/board", get(board))
        .route("/api/stream", get(events))
        .route("/api/{*rest}", get(not_found))
        .fallback_service(files)
        .with_state(published)
        .merge(case_files);
    harden(app, hosts)
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
}

/// The body limit for routes that set none of their own. The commands route
/// sets a larger one, sized for Rob's words (`commands::MAX_BODY_BYTES`).
pub(crate) const BODY_LIMIT: usize = 64 * 1024;

/// What every route sits behind, added last so it covers the routes merged in
/// before it: the Host allowlist, then the body limit on any route that reads
/// a body. Axum's own default limit is 2 MiB.
pub(crate) fn harden(app: Router, hosts: crate::hosts::Hosts) -> Router {
    app.layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::middleware::from_fn_with_state(
            hosts,
            crate::hosts::guard,
        ))
}

async fn board(State(published): State<watch::Receiver<Published>>) -> Response {
    let board = published.borrow().board.clone();
    match board {
        Some(board) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(board.as_ref().clone()),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The board has not been read from the store yet.\n",
        )
            .into_response(),
    }
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// Every connection, including a reconnect that sends `Last-Event-ID`,
/// starts with a full snapshot, then gets a patch for each newer board.
async fn events(
    State(published): State<watch::Receiver<Published>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let client = Client {
        published,
        shown: None,
        shown_id: String::new(),
        shown_commands: None,
        connected: None,
        queue: VecDeque::new(),
        retry_sent: false,
    };
    Sse::new(stream::unfold(client, |mut client| async move {
        let event = client.next().await?;
        Some((Ok(event), client))
    }))
}

struct Client {
    published: watch::Receiver<Published>,
    shown: Option<Arc<Board>>,
    /// The event id of `shown`, which the next patch names as its base.
    shown_id: String,
    shown_commands: Option<Arc<Vec<CommandRecord>>>,
    connected: Option<bool>,
    queue: VecDeque<Event>,
    retry_sent: bool,
}

impl Client {
    async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Some(self.with_retry(event));
            }
            let current = self.published.borrow_and_update().clone();
            if self.connected != Some(current.connected) {
                self.connected = Some(current.connected);
                self.queue.push_back(json(
                    Event::default().event("database"),
                    &DatabaseStatus {
                        connected: current.connected,
                    },
                ));
            }
            if let Some(board) = current.board
                && !self
                    .shown
                    .as_ref()
                    .is_some_and(|shown| Arc::ptr_eq(shown, &board))
            {
                let id = format!("{}.{}", board.revision, current.build);
                self.queue.push_back(match &self.shown {
                    None => json(Event::default().event("snapshot").id(&id), board.as_ref()),
                    Some(shown) => json(
                        Event::default().event("patch").id(&id),
                        &PatchEvent {
                            base: std::mem::take(&mut self.shown_id),
                            patch: board.patch_since(shown),
                        },
                    ),
                });
                self.shown = Some(board);
                self.shown_id = id;
            }
            if let Some(commands) = current.commands
                && !self
                    .shown_commands
                    .as_ref()
                    .is_some_and(|shown| Arc::ptr_eq(shown, &commands))
            {
                self.queue.push_back(json(
                    Event::default().event("commands"),
                    &CommandsEvent {
                        commands: commands.as_ref().clone(),
                    },
                ));
                self.shown_commands = Some(commands);
            }
            if self.queue.is_empty() && self.published.changed().await.is_err() {
                return None;
            }
        }
    }

    fn with_retry(&mut self, event: Event) -> Event {
        if std::mem::replace(&mut self.retry_sent, true) {
            event
        } else {
            event.retry(RETRY)
        }
    }
}

fn json(event: Event, data: &impl serde::Serialize) -> Event {
    event
        .json_data(data)
        .expect("board types serialize to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::Hosts;
    use axum::body::Bytes;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        path::PathBuf,
    };

    /// The Host the probe app answers to; it listens on a Unix socket.
    const HOST: &str = "127.0.0.1:8034";

    /// Serve a probe app behind `harden`: GET /probe answers, POST /probe
    /// reads its body and reports its length.
    fn serve() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("probe.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let hosts = Hosts::new(HOST.parse().unwrap(), &[]).unwrap();
        let probe = Router::new().route(
            "/probe",
            get(|| async { "read" }).post(|body: Bytes| async move { body.len().to_string() }),
        );
        let app = harden(probe, hosts);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        std::thread::spawn(move || {
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::from_std(listener).unwrap();
                axum::serve(listener, app).await
            })
        });
        (directory, path)
    }

    fn send(socket: &Path, method: &str, host: &str, body_len: usize) -> (u16, String) {
        let mut socket = UnixStream::connect(socket).unwrap();
        let head = format!(
            "{method} /probe HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {body_len}\r\n\r\n"
        );
        // The server may answer and close before it reads a refused body.
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(&vec![b'a'; body_len]);
        let mut response = String::new();
        let _ = socket.read_to_string(&mut response);
        let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = response.rsplit("\r\n\r\n").next().unwrap().to_string();
        (status, body)
    }

    #[test]
    fn bodies_over_the_limit_are_refused_on_every_write_route() {
        let (_directory, socket) = serve();
        assert_eq!(send(&socket, "POST", HOST, 0), (200, "0".into()));
        assert_eq!(
            send(&socket, "POST", HOST, BODY_LIMIT),
            (200, BODY_LIMIT.to_string())
        );
        assert_eq!(send(&socket, "POST", HOST, BODY_LIMIT + 1).0, 413);
    }

    #[test]
    fn a_route_added_before_harden_is_behind_the_host_allowlist() {
        let (_directory, socket) = serve();
        assert_eq!(send(&socket, "GET", HOST, 0).0, 200);
        assert_eq!(send(&socket, "GET", "evil.example", 0).0, 421);
        assert_eq!(send(&socket, "POST", "evil.example", 0).0, 421);
    }
}
