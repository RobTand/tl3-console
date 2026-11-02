//! Role prompt versions (PRD #327, C5): the console's only writes. Its login
//! may add versions and move the active pointers, and nothing else.

use crate::api::{
    ActivateVersion, NewPromptVersion, PromptRole, PromptSummary, PromptVersion, RolePrompt,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::{Arc, Mutex};
use tl3_store::{Prompts, Refusal, RolePrompts};

/// The console has one user (PRD #327 leaves others out of scope).
const AUTHOR: &str = "Rob";
/// Browsers send a custom header cross-site only after a CORS preflight, which
/// the console never grants, so another page can't make these writes.
pub const WRITE_HEADER: &str = "x-tl3-console";
/// A write's request body: the longest prompt, escaped as JSON, and a note.
const MAX_BODY_BYTES: usize = tl3_store::TEXT_LIMIT + 64 * 1024;

#[derive(Clone)]
struct Session {
    settings: tl3_config::Store,
    prompts: Arc<Mutex<Option<Prompts>>>,
}

pub(crate) fn routes(settings: tl3_config::Store) -> Router {
    Router::new()
        .route("/api/prompts", get(roles))
        .route("/api/prompts/{role}", get(role))
        .route("/api/prompts/{role}/versions", get(versions).post(save))
        .route("/api/prompts/{role}/versions/{version}", get(version))
        .route("/api/prompts/{role}/activate/{version}", post(activate))
        .route("/api/prompts/{role}/keep/{version}", post(keep))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(Session {
            settings,
            prompts: Arc::default(),
        })
}

async fn roles(State(session): State<Session>) -> Response {
    match session.run(|prompts| prompts.roles()).await {
        Ok(roles) => Json(
            roles
                .iter()
                .map(|role| PromptRole {
                    role: role.role.clone(),
                    active_number: role.active.number,
                    active_builtin: role.active.builtin,
                    builtin_update: role.builtin_update.is_some(),
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(failure) => failure.into_response(),
    }
}

async fn role(State(session): State<Session>, Path(role): Path<String>) -> Response {
    let name = role.clone();
    match session.run(move |prompts| prompts.role(&name)).await {
        Ok(Some(found)) => Json(view(found)).into_response(),
        Ok(None) => refused(&role, Refusal::UnknownRole),
        Err(failure) => failure.into_response(),
    }
}

async fn versions(State(session): State<Session>, Path(role): Path<String>) -> Response {
    let name = role.clone();
    match session.run(move |prompts| prompts.role(&name)).await {
        Ok(Some(found)) => Json(view(found).versions).into_response(),
        Ok(None) => refused(&role, Refusal::UnknownRole),
        Err(failure) => failure.into_response(),
    }
}

async fn version(
    State(session): State<Session>,
    Path((role, id)): Path<(String, i64)>,
) -> Response {
    let name = role.clone();
    match session.run(move |prompts| prompts.version(&name, id)).await {
        Ok(Some(found)) => Json(full(found)).into_response(),
        Ok(None) => refused(&role, Refusal::UnknownVersion),
        Err(failure) => failure.into_response(),
    }
}

async fn save(
    State(session): State<Session>,
    Path(role): Path<String>,
    headers: HeaderMap,
    edit: Result<Json<NewPromptVersion>, JsonRejection>,
) -> Response {
    // The origin check comes first, so a foreign page learns nothing from the body.
    if let Err(failure) = same_origin(&headers) {
        return failure.into_response();
    }
    let Json(edit) = match edit {
        Ok(edit) => edit,
        Err(rejection) => return rejection.into_response(),
    };
    let name = role.clone();
    let result = session
        .run(move |prompts| prompts.save(&name, &edit.text, &edit.note, AUTHOR, edit.base))
        .await;
    edited(&role, result, StatusCode::CREATED)
}

async fn activate(
    State(session): State<Session>,
    Path((role, version)): Path<(String, i64)>,
    headers: HeaderMap,
    seen: Result<Json<ActivateVersion>, JsonRejection>,
) -> Response {
    if let Err(failure) = same_origin(&headers) {
        return failure.into_response();
    }
    let Json(seen) = match seen {
        Ok(seen) => seen,
        Err(rejection) => return rejection.into_response(),
    };
    let name = role.clone();
    let result = session
        .run(move |prompts| prompts.activate(&name, version, AUTHOR, seen.expected))
        .await;
    match result {
        Ok(Err(Refusal::Stale { .. })) => text(
            StatusCode::CONFLICT,
            "Another version became active after this page loaded. Check the history, then choose again.",
        ),
        result => edited(&role, result, StatusCode::OK),
    }
}

async fn keep(
    State(session): State<Session>,
    Path((role, version)): Path<(String, i64)>,
    headers: HeaderMap,
) -> Response {
    if let Err(failure) = same_origin(&headers) {
        return failure.into_response();
    }
    let name = role.clone();
    let result = session
        .run(move |prompts| prompts.keep(&name, version))
        .await;
    edited(&role, result, StatusCode::OK)
}

impl Session {
    /// Run store work off the async threads, on one reused session. A session
    /// Postgres dropped is replaced before the work is sent. The work itself
    /// runs once: a failure at commit may still have committed, so a retry
    /// could save twice or refuse a save that succeeded.
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Prompts) -> tl3_store::Result<T> + Send + 'static,
    ) -> Result<T, Failure> {
        let session = self.clone();
        let done = tokio::task::spawn_blocking(move || {
            let mut slot = session.prompts.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_mut().is_some_and(|prompts| !prompts.is_live()) {
                *slot = None;
            }
            if slot.is_none() {
                // The console's migration check, as every store session has.
                crate::schema::Session::open(&session.settings, "prompts")
                    .map_err(Failure::store)?;
                *slot = Some(Prompts::open(&session.settings).map_err(Failure::store)?);
            }
            let result = work(slot.as_mut().expect("an open session")).map_err(Failure::store);
            if matches!(&result, Err(Failure::Unavailable)) {
                *slot = None;
            }
            result
        })
        .await;
        match done {
            Ok(result) => result,
            Err(_) => Err(Failure::Panicked),
        }
    }
}

/// Writes need the console's header and, when the browser names one, the
/// console's own origin.
fn same_origin(headers: &HeaderMap) -> Result<(), Failure> {
    if headers.get(WRITE_HEADER).is_none_or(|value| value != "1") {
        return Err(Failure::Forbidden);
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .ok_or(Failure::Forbidden)?;
        let origin = origin.to_str().map_err(|_| Failure::Forbidden)?;
        let own = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .is_some_and(|rest| rest == host);
        if !own {
            return Err(Failure::Forbidden);
        }
    }
    Ok(())
}

/// Why a request got no answer from the prompt store.
enum Failure {
    Forbidden,
    Permission,
    InvalidText(String),
    Store,
    Unavailable,
    Panicked,
}

impl Failure {
    fn store(error: tl3_store::Error) -> Self {
        eprintln!("console prompt store: {}", crate::describe(&error));
        match error {
            tl3_store::Error::Database(error) => match error.as_db_error() {
                Some(error) => match error.code().code() {
                    "42501" => Self::Permission,
                    code if code.starts_with("22") => Self::InvalidText(error.message().into()),
                    code if code.starts_with("08")
                        || matches!(code, "57P01" | "57P02" | "57P03") =>
                    {
                        Self::Unavailable
                    }
                    _ => Self::Store,
                },
                None => Self::Unavailable,
            },
            tl3_store::Error::Password(_) => Self::Unavailable,
            _ => Self::Store,
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Failure::Forbidden => text(
                StatusCode::FORBIDDEN,
                "Writes come only from the console's own page.",
            ),
            Failure::Permission => text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The console's database login can't edit prompts; TL3's deploy grants it (tl3-deploy console-role)",
            ),
            Failure::InvalidText(reason) => text(StatusCode::UNPROCESSABLE_ENTITY, &reason),
            Failure::Store => text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The prompt store could not complete this request.",
            ),
            Failure::Unavailable => text(
                StatusCode::SERVICE_UNAVAILABLE,
                "The prompt store is unavailable. Try again shortly.",
            ),
            Failure::Panicked => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

fn edited(role: &str, result: Result<tl3_store::Edit, Failure>, status: StatusCode) -> Response {
    match result {
        Ok(Ok(prompts)) => (status, Json(view(prompts))).into_response(),
        Ok(Err(refusal)) => refused(role, refusal),
        Err(failure) => failure.into_response(),
    }
}

fn refused(role: &str, refusal: Refusal) -> Response {
    let (status, message) = match refusal {
        Refusal::UnknownRole => (
            StatusCode::NOT_FOUND,
            format!("No role named {role} has a prompt."),
        ),
        Refusal::UnknownVersion => (
            StatusCode::NOT_FOUND,
            format!("The {role} prompt has no such version."),
        ),
        Refusal::Stale { .. } => (
            StatusCode::CONFLICT,
            "Another version became active while you edited.".into(),
        ),
        Refusal::EmptyText => (StatusCode::BAD_REQUEST, "The prompt can't be empty.".into()),
        Refusal::EmptyNote => (
            StatusCode::BAD_REQUEST,
            "Add a note that says why you changed the prompt.".into(),
        ),
        Refusal::TextTooLong => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "The prompt is longer than {} KiB.",
                tl3_store::TEXT_LIMIT / 1024
            ),
        ),
        Refusal::NoteTooLong => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Keep the note to {} characters.", tl3_store::NOTE_LIMIT),
        ),
        Refusal::NoUpdate => (
            StatusCode::CONFLICT,
            "No built-in update waits for that version. Reload to see the latest.".into(),
        ),
    };
    text(status, &message)
}

fn text(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        format!("{message}\n"),
    )
        .into_response()
}

fn full(version: tl3_store::PromptVersion) -> PromptVersion {
    PromptVersion {
        id: version.id,
        number: version.number,
        text: version.body,
        builtin: version.builtin,
        author: version.author,
        note: version.note,
        created_ms: version.created_ms,
    }
}

/// The active version and a waiting update carry their text; the history doesn't.
fn view(prompts: RolePrompts) -> RolePrompt {
    RolePrompt {
        role: prompts.role,
        active: full(prompts.active),
        activated_by: prompts.activated_by,
        activated_ms: prompts.activated_ms,
        builtin_update: prompts.builtin_update.map(full),
        versions: prompts
            .versions
            .into_iter()
            .map(|version| PromptSummary {
                id: version.id,
                number: version.number,
                builtin: version.builtin,
                author: version.author,
                note: version.note,
                created_ms: version.created_ms,
            })
            .collect(),
    }
}
