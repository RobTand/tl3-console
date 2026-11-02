//! A request's case file (PRD #327): its timeline, runs, incidents, question,
//! summary and links. Everything comes from TL3's stored rows in one read
//! transaction. MLflow, GitHub and Hindsight are never called; their links are
//! built from stored keys.

mod route;
pub(crate) use route::{CaseFiles, routes};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tl3_controller::{
    Result,
    board::{self, Card, Column, Ending, Request, Rows},
    executor::{Completion, Delivery, Observation, Output, Upstream},
    looks::Cause,
};
use tl3_kernel::{
    Ack, Answer, Command, EarlyEnd, EffectKind, Event, Fact, Notice, Phase, QKind, Reply, Role,
};
use tl3_store::Transaction;
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct CaseFile {
    /// The controller revision this case file shows.
    pub revision: i64,
    /// The request as the board shows it, with its counters and credit.
    pub card: Card,
    /// Stage changes, oldest first.
    pub timeline: Vec<StageChange>,
    pub children: Vec<ChildItem>,
    /// Worker runs, oldest first.
    pub runs: Vec<WorkerRun>,
    /// Open incidents first, then cleared ones.
    pub incidents: Vec<CaseIncident>,
    pub blocked: Vec<BlockedReport>,
    pub question: Option<PendingQuestion>,
    pub summary: Option<ClosingSummary>,
    pub links: Vec<Link>,
}

/// A step that moved the request to a stage, from the audit event log: the
/// commit that started the step, and the kernel event that commit recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct StageChange {
    pub revision: i64,
    /// When the commit landed, in Unix milliseconds.
    pub at_ms: i64,
    pub column: Column,
    pub step: String,
    /// What the controller recorded as the reason, in plain words.
    pub cause: String,
}

/// Work that continues this request: pieces a plan proposed, or a request
/// that replaced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct ChildItem {
    /// The child's request; proposed pieces have none yet.
    pub id: Option<u64>,
    pub title: String,
    pub column: Option<Column>,
    pub stage: String,
    pub files: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct WorkerRun {
    /// The run's effect key.
    pub key: String,
    pub role: String,
    pub generation: u64,
    pub attempt: u8,
    pub round: u8,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    /// None: the store doesn't record token use yet. The trace has it.
    pub tokens: Option<u64>,
    pub status: RunStatus,
    pub outcome: String,
    /// The run's MLflow traces, found by their tags.
    pub trace_url: Option<String>,
}

/// Q51: TL3's own code, prompts, tools or harness, or an outside service
/// such as GitHub, Mergify, PrismaBuild or the model proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
pub enum IncidentCause {
    Tl3,
    Outside,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct CaseIncident {
    pub id: String,
    pub summary: String,
    pub open: bool,
    pub cause: IncidentCause,
    pub needs_rob: bool,
    pub blocking: bool,
    /// The upstream that failed, such as GitHub or Workers.
    pub source: String,
    /// The failure signature's class.
    pub class: String,
    /// The effect whose delivery failed, if one did.
    pub effect: Option<String>,
}

/// Q50: a maker that TL3's environment stopped, and what it cited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct BlockedReport {
    pub generation: Option<u64>,
    /// The worker-checked evidence the reply cited.
    pub evidence: String,
    /// The maker's own report. Model text; it never goes to GitHub.
    pub report: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct QuestionOption {
    pub action: String,
    pub outcome: String,
}

/// The question waiting on Rob, as the writer was asked to put it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct PendingQuestion {
    pub id: u64,
    pub kind: String,
    pub obstacle: String,
    pub options: Vec<QuestionOption>,
    /// An index into `options`.
    pub recommended_option: u32,
    pub recommendation_reason: String,
    pub how_to_answer: String,
    /// False when posting the question failed.
    pub posted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct ClosingSummary {
    pub ending: Ending,
    pub outcome: String,
    pub unmet_goals: Vec<String>,
    pub removed_changes: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
pub enum LinkKind {
    Issue,
    PullRequest,
    Branch,
    MergeQueue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct Link {
    pub kind: LinkKind,
    /// The link's accessible name.
    pub label: String,
    pub url: String,
}

/// What the case file needs to build links: no credentials, no clients.
#[derive(Clone, Debug, Default)]
pub struct Links {
    /// The MLflow UI's traces page for TL3's experiment.
    pub traces: Option<String>,
    /// Each served repository's `owner/name`, lowercased, and branch template.
    pub branches: Vec<(String, String)>,
}

impl Links {
    /// `traces` is the MLflow UI's traces page, such as
    /// `http://192.168.1.107:5033/#/experiments/2/traces`.
    pub fn new(traces: Option<&str>, repos: &[tl3_config::Repository]) -> crate::Result<Links> {
        Ok(Links {
            traces: traces.map(str::to_owned),
            branches: repos
                .iter()
                .map(|repo| {
                    (
                        format!("{}/{}", repo.owner, repo.name).to_ascii_lowercase(),
                        repo.branch_template.clone(),
                    )
                })
                .collect(),
        })
    }
}

/// Build request `id`'s case file inside the caller's read transaction.
/// None when no request has that id.
pub fn read(
    tx: &mut Transaction<'_>,
    id: u64,
    now_ms: i64,
    models: Option<&tl3_config::Models>,
    links: &Links,
) -> crate::Result<Option<CaseFile>> {
    let _ = (tx, id, now_ms, models, links);
    todo!("case file")
}

impl Links {
    fn trace(&self, node: u64, generation: u64, role: &str) -> Option<String> {
        let _ = (node, generation, role);
        todo!("trace links")
    }
}

#[cfg(test)]
mod tests {
    use super::Links;

    #[test]
    fn trace_links_need_an_http_url() {
        let links = Links::new(
            Some("http://192.168.1.107:5033/#/experiments/2/traces"),
            &[],
        )
        .unwrap();
        assert_eq!(
            links.trace(4, 12, "maker").unwrap(),
            "http://192.168.1.107:5033/#/experiments/2/traces?filter=tag%3A%3A%3D%3A%3A4%3A%3Arequest&filter=tag%3A%3A%3D%3A%3A12%3A%3Ageneration&filter=tag%3A%3A%3D%3A%3Amaker%3A%3Arole"
        );
        assert!(
            Links::new(None, &[])
                .unwrap()
                .trace(4, 12, "maker")
                .is_none()
        );
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://host/a b",
        ] {
            assert!(Links::new(Some(refused), &[]).is_err(), "{refused}");
        }
    }
}
