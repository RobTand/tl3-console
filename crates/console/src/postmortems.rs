//! Read-only views of TL3 migration 13. The controller owns selection and writes.

use crate::{hidden::HiddenRepos, schema::Session, sessions::Sessions};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tl3_store::Transaction;
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PostmortemStatus {
    Owed,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PostmortemReason {
    NeededFix,
    Retry,
    Escalation,
    ClosedWithoutMerge,
    Sampled,
    RobAsked,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PostmortemCause {
    Tl3,
    Outside,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Recommendation {
    pub id: i64,
    pub fingerprint: String,
    pub rank: i16,
    pub phase: String,
    pub kind: String,
    pub target: String,
    pub operation: String,
    pub before: String,
    pub after: String,
    pub evidence: String,
    pub effect: String,
    /// Distinct visible postmortems sharing this fingerprint, across all pages.
    pub seen_in: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Postmortem {
    pub id: i64,
    pub node_id: String,
    pub trigger: String,
    pub reason: PostmortemReason,
    pub sample_share: Option<f64>,
    pub repo: String,
    pub issue: i64,
    pub title: String,
    pub status: PostmortemStatus,
    pub owed_ms: i64,
    pub started_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub failed_ms: Option<i64>,
    pub failure: Option<String>,
    pub run_key: Option<String>,
    pub rubric_version: Option<i32>,
    pub model: Option<String>,
    pub cause: Option<PostmortemCause>,
    pub summary: Option<String>,
    #[ts(type = "unknown")]
    pub timeline: Option<Value>,
    #[ts(type = "unknown")]
    pub process_failures: Option<Value>,
    #[ts(type = "unknown")]
    pub grades: Option<Value>,
    pub notes: Option<String>,
    #[serde(default)]
    pub recommendations: Vec<Recommendation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct CodeGrade {
    pub id: i64,
    pub node_id: String,
    pub trigger: String,
    pub rubric_version: i32,
    pub dimension: String,
    pub grade: Option<i16>,
    #[ts(type = "unknown")]
    pub fact: Value,
    pub recorded_ms: i64,
}

/// Keyset pages keep reads bounded even on requests with many triggers.
#[derive(Clone, Debug, Serialize, TS)]
pub struct PostmortemPage {
    pub postmortems: Vec<Postmortem>,
    pub next_before: Option<i64>,
}

#[derive(Clone, Debug, Serialize, TS)]
pub struct RequestPostmortems {
    pub postmortems: Vec<Postmortem>,
    pub grades: Vec<CodeGrade>,
    pub next_before: Option<i64>,
    pub next_grade_before: Option<i64>,
    pub active: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Filters {
    q: Option<String>,
    repo: Option<String>,
    cause: Option<PostmortemCause>,
    reason: Option<PostmortemReason>,
    status: Option<PostmortemStatus>,
    from: Option<String>,
    to: Option<String>,
    before: Option<i64>,
    grade_before: Option<i64>,
    limit: Option<i64>,
}

impl Filters {
    fn valid(&self) -> bool {
        (1..=100).contains(&self.limit.unwrap_or(50))
            && self.before.is_none_or(|id| id > 0)
            && self.grade_before.is_none_or(|id| id > 0)
            && self.q.as_ref().is_none_or(|q| q.len() <= 500)
            && self.repo.as_ref().is_none_or(|repo| repo.len() <= 200)
            && self.from.as_deref().is_none_or(valid_date)
            && self.to.as_deref().is_none_or(valid_date)
            && match (&self.from, &self.to) {
                (Some(from), Some(to)) => from <= to,
                _ => true,
            }
    }
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = date[..4].parse().unwrap_or(0);
    let month: u32 = date[5..7].parse().unwrap_or(0);
    let day: u32 = date[8..].parse().unwrap_or(0);
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

#[derive(Clone)]
struct Views {
    store: tl3_config::Store,
    hidden: HiddenRepos,
    sessions: Arc<Sessions<Session>>,
}

pub(crate) fn routes(store: tl3_config::Store, hidden: HiddenRepos) -> Router {
    Router::new()
        .route("/api/postmortems", get(list))
        .route("/api/requests/{id}/postmortems", get(request))
        .route("/api/postmortems/{id}", get(request))
        .with_state(Views {
            store,
            hidden,
            sessions: Sessions::new(4),
        })
}

async fn list(State(views): State<Views>, Query(filters): Query<Filters>) -> Response {
    respond(views, None, filters).await
}
async fn request(
    State(views): State<Views>,
    Path(id): Path<u64>,
    Query(filters): Query<Filters>,
) -> Response {
    respond(views, Some(id), filters).await
}

async fn respond(views: Views, id: Option<u64>, filters: Filters) -> Response {
    if !filters.valid() {
        return (
            StatusCode::BAD_REQUEST,
            "Use valid filters, ISO dates and a limit from 1 to 100.\n",
        )
            .into_response();
    }
    let store = views.store.clone();
    let sessions = views.sessions.clone();
    let result: crate::Result<Option<Value>> = sessions.run(move || Ok(Session::open(&store, "postmortems")?), move |session| {
        let mut tx = session.transaction()?;
        // Filters without their own upstream index are still bounded in time.
        tx.batch_execute("SET LOCAL statement_timeout = '3s'; SET LOCAL timezone = 'UTC'").map_err(tl3_store::Error::from)?;
        if let Some(id) = id {
            if views.hidden.hides(&mut tx, id)? || tx.query_opt("SELECT 1 FROM tl3_world WHERE table_name = 'requests' AND row_id = $1", &[&id.to_string()]).map_err(tl3_store::Error::from)?.is_none() { return Ok(None); }
        }
        let hidden = views.hidden.names();
        let page = read_page(&mut tx, id, &filters, &hidden)?;
        let result = if let Some(id) = id {
            let mut grades: Vec<CodeGrade> = tx.query("SELECT to_jsonb(g) - 'recorded_at' || jsonb_build_object('recorded_ms', (extract(epoch FROM recorded_at)*1000)::bigint) FROM tl3_code_grades g WHERE node_id = $1 AND ($2::bigint IS NULL OR id < $2) ORDER BY id DESC LIMIT 101", &[&id.to_string(), &filters.grade_before]).map_err(tl3_store::Error::from)?.into_iter().map(|row| serde_json::from_value(row.get(0))).collect::<Result<_, _>>()?;
            let next_grade_before = if grades.len() > 100 { grades.truncate(100); grades.last().map(|g| g.id) } else { None };
            let active: bool = tx.query_one("SELECT EXISTS(SELECT 1 FROM tl3_postmortems WHERE node_id = $1 AND status IN ('owed', 'running'))", &[&id.to_string()]).map_err(tl3_store::Error::from)?.get(0);
            serde_json::to_value(RequestPostmortems { postmortems: page.postmortems, grades, next_before: page.next_before, next_grade_before, active })?
        } else { serde_json::to_value(page)? };
        tx.commit().map_err(tl3_store::Error::from)?;
        Ok(Some(result))
    }).await;
    match result {
        Ok(Some(value)) => ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "No request has that id.\n").into_response(),
        Err(error) => {
            eprintln!(
                "console postmortems unavailable: {}",
                crate::describe(&error)
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Postmortems could not be read from the store.\n",
            )
                .into_response()
        }
    }
}

fn text_filter(value: &Option<impl Serialize>) -> crate::Result<Option<String>> {
    value
        .as_ref()
        .map(|value| {
            serde_json::to_value(value).map(|value| value.as_str().unwrap_or_default().to_owned())
        })
        .transpose()
        .map_err(Into::into)
}

fn read_page(
    tx: &mut Transaction<'_>,
    node: Option<u64>,
    filters: &Filters,
    hidden: &[String],
) -> crate::Result<PostmortemPage> {
    let limit = filters.limit.unwrap_or(50);
    // UNION lets each text predicate use its generated tsvector's GIN index.
    let rows = tx.query("SELECT to_jsonb(p) - ARRAY['search', 'owed_at', 'started_at', 'finished_at', 'failed_at'] || jsonb_build_object(
            'owed_ms', (extract(epoch FROM owed_at)*1000)::bigint,
            'started_ms', (extract(epoch FROM started_at)*1000)::bigint,
            'finished_ms', (extract(epoch FROM finished_at)*1000)::bigint,
            'failed_ms', (extract(epoch FROM failed_at)*1000)::bigint)
        FROM tl3_postmortems p
        WHERE ($1::text IS NULL OR node_id = $1)
          AND NOT lower(repo) = ANY($2)
          AND ($3::bigint IS NULL OR id < $3)
          AND ($4::text IS NULL OR repo = $4)
          AND ($5::text IS NULL OR cause = $5)
          AND ($6::text IS NULL OR reason = $6)
          AND ($7::text IS NULL OR status = $7)
          AND ($8::text IS NULL OR owed_at >= $8::text::date)
          AND ($9::text IS NULL OR owed_at < $9::text::date + INTERVAL '1 day')
          AND ($10::text IS NULL OR id IN (
              SELECT id FROM tl3_postmortems WHERE search @@ websearch_to_tsquery('english', $10)
              UNION SELECT postmortem_id FROM tl3_postmortem_recommendations WHERE search @@ websearch_to_tsquery('english', $10)))
        ORDER BY id DESC LIMIT $11", &[&node.map(|id| id.to_string()), &hidden, &filters.before, &filters.repo, &text_filter(&filters.cause)?, &text_filter(&filters.reason)?, &text_filter(&filters.status)?, &filters.from, &filters.to, &filters.q.as_ref().filter(|q| !q.trim().is_empty()), &(limit + 1)]).map_err(tl3_store::Error::from)?;
    let mut postmortems: Vec<Postmortem> = rows
        .into_iter()
        .map(|row| serde_json::from_value(row.get(0)))
        .collect::<Result<_, _>>()?;
    let next_before = if postmortems.len() > limit as usize {
        postmortems.truncate(limit as usize);
        postmortems.last().map(|p| p.id)
    } else {
        None
    };
    let ids: Vec<i64> = postmortems.iter().map(|p| p.id).collect();
    for row in tx.query("SELECT r.postmortem_id, to_jsonb(r) - 'search' || jsonb_build_object('seen_in', (
            SELECT count(DISTINCT other.postmortem_id) FROM tl3_postmortem_recommendations other
            JOIN tl3_postmortems p ON p.id = other.postmortem_id
            WHERE other.fingerprint = r.fingerprint AND NOT lower(p.repo) = ANY($2)))
        FROM tl3_postmortem_recommendations r WHERE r.postmortem_id = ANY($1) ORDER BY r.postmortem_id, r.rank", &[&ids, &hidden]).map_err(tl3_store::Error::from)? {
        let id: i64 = row.get(0);
        if let Some(postmortem) = postmortems.iter_mut().find(|p| p.id == id) { postmortem.recommendations.push(serde_json::from_value(row.get(1))?); }
    }
    Ok(PostmortemPage {
        postmortems,
        next_before,
    })
}
