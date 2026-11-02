//! `GET /api/requests/{id}`: a request's case file, read on demand. The
//! drawer refetches it when the stream says the request's card changed.

use super::{CaseFile, Links};
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
}

pub(crate) fn routes(files: CaseFiles) -> Router {
    Router::new()
        .route("/api/requests/{id}", get(case_file))
        .with_state(files)
}

async fn case_file(State(files): State<CaseFiles>, Path(id): Path<u64>) -> Response {
    let read = tokio::task::spawn_blocking(move || read(&files, id, crate::now_ms())).await;
    match read {
        Ok(Ok(Some(file))) => ([(header::CACHE_CONTROL, "no-store")], Json(file)).into_response(),
        Ok(Ok(None)) => (StatusCode::NOT_FOUND, "No request has that id.\n").into_response(),
        Ok(Err(error)) => {
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
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// One read-only session per request keeps a slow case file off the board's
/// session.
fn read(files: &CaseFiles, id: u64, now_ms: i64) -> crate::Result<Option<CaseFile>> {
    let mut reader = Reader::open(&files.store)?;
    let mut tx = reader.transaction()?;
    let file = super::read(&mut tx, id, now_ms, files.models.as_deref(), &files.links)?;
    tx.commit().map_err(tl3_store::Error::from)?;
    Ok(file)
}
