//! `GET /api/requests/{id}`: a request's case file, read on demand. The
//! drawer refetches it when the stream says the request's card changed.

use super::{CaseFile, Links};
use crate::sessions::Sessions;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;
use tl3_store::Reader;

#[derive(Clone)]
pub(crate) struct CaseFiles {
    pub store: tl3_config::Store,
    pub models: Option<Arc<tl3_config::Models>>,
    pub links: Arc<Links>,
    pub sessions: Arc<Sessions<Reader>>,
}

/// Case files read at once, each on its own read-only session.
pub(crate) const SESSIONS: usize = 4;

pub(crate) fn routes(files: CaseFiles) -> Router {
    Router::new()
        .route("/api/requests/{id}", get(case_file))
        .with_state(files)
}

async fn case_file(State(files): State<CaseFiles>, Path(id): Path<u64>) -> Response {
    let store = files.store.clone();
    let sessions = files.sessions.clone();
    let read = sessions
        .run(
            move || Ok(Reader::open(&store)?),
            move |reader| read(reader, &files, id, crate::now_ms()),
        )
        .await;
    match read {
        Ok(Some(file)) => ([(header::CACHE_CONTROL, "no-store")], Json(file)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "No request has that id.\n").into_response(),
        Err(error) => {
            eprintln!(
                "console case file {id} unavailable: {}",
                crate::describe(&error)
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "The case file could not be read from the store.\n",
            )
                .into_response()
        }
    }
}

/// Case files use their own sessions, so a slow one never holds up the board.
fn read(
    reader: &mut Reader,
    files: &CaseFiles,
    id: u64,
    now_ms: i64,
) -> crate::Result<Option<CaseFile>> {
    let mut tx = reader.transaction()?;
    let file = super::read(&mut tx, id, now_ms, files.models.as_deref(), &files.links)?;
    tx.commit().map_err(tl3_store::Error::from)?;
    Ok(file)
}
