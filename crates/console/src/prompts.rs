//! Role prompt versions (PRD #327, C5): the console's only writes. Its login
//! may add versions and move the active pointers, and nothing else.

use crate::api::{NewPromptVersion, PromptRole, PromptVersion, RolePrompt};
use axum::{
    Json, Router,
    extract::{Path, State},
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
        .route("/api/prompts/{role}/activate/{version}", post(activate))
        .route("/api/prompts/{role}/keep/{version}", post(keep))
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
                .map(|role| {
                    let active = role.version(role.active).expect("the active version");
                    PromptRole {
                        role: role.role.clone(),
                        active_number: active.number,
                        active_builtin: active.builtin,
                        builtin_update: role.builtin_update.is_some(),
                    }
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

async fn save(
    State(session): State<Session>,
    Path(role): Path<String>,
    headers: HeaderMap,
    Json(edit): Json<NewPromptVersion>,
) -> Response {
    if let Err(failure) = same_origin(&headers) {
        return failure.into_response();
    }
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
) -> Response {
    if let Err(failure) = same_origin(&headers) {
        return failure.into_response();
    }
    let name = role.clone();
    let result = session
        .run(move |prompts| prompts.activate(&name, version, AUTHOR))
        .await;
    edited(&role, result, StatusCode::OK)
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
    /// Run store work off the async threads, on one reused session. A broken
    /// session is replaced once.
    async fn run<T: Send + 'static>(
        &self,
        work: impl Fn(&mut Prompts) -> tl3_store::Result<T> + Send + 'static,
    ) -> Result<T, Failure> {
        let session = self.clone();
        let done = tokio::task::spawn_blocking(move || {
            let mut slot = session.prompts.lock().unwrap_or_else(|e| e.into_inner());
            let mut retried = false;
            loop {
                if slot.is_none() {
                    *slot = Some(Prompts::open(&session.settings)?);
                }
                match work(slot.as_mut().expect("an open session")) {
                    Err(_) if !retried => {
                        retried = true;
                        *slot = None;
                    }
                    result => return result,
                }
            }
        })
        .await;
        match done {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                eprintln!(
                    "console prompt store unavailable: {}",
                    crate::describe(&error)
                );
                Err(Failure::Unavailable)
            }
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
    Unavailable,
    Panicked,
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Failure::Forbidden => text(
                StatusCode::FORBIDDEN,
                "Writes come only from the console's own page.",
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
            "Another version became active while you edited. Reload to see it before saving."
                .into(),
        ),
        Refusal::EmptyText => (StatusCode::BAD_REQUEST, "The prompt can't be empty.".into()),
        Refusal::EmptyNote => (
            StatusCode::BAD_REQUEST,
            "Add a note that says why you changed the prompt.".into(),
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

fn view(prompts: RolePrompts) -> RolePrompt {
    RolePrompt {
        role: prompts.role,
        active: prompts.active,
        activated_by: prompts.activated_by,
        activated_ms: prompts.activated_ms,
        builtin_update: prompts.builtin_update,
        versions: prompts
            .versions
            .into_iter()
            .map(|version| PromptVersion {
                id: version.id,
                number: version.number,
                text: version.body,
                builtin: version.builtin,
                author: version.author,
                note: version.note,
                created_ms: version.created_ms,
            })
            .collect(),
    }
}
