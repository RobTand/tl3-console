//! A request's case file (PRD #327): its timeline, runs, incidents, question,
//! summary and links. Everything comes from TL3's stored rows in one read
//! transaction. MLflow, GitHub and Hindsight are never called; their links are
//! built from stored keys.

mod route;
pub(crate) use route::{CaseFiles, routes};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tl3_controller::{
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
        if let Some(traces) = traces
            && (!(traces.starts_with("http://") || traces.starts_with("https://"))
                || traces.chars().any(|c| c.is_whitespace() || c.is_control()))
        {
            return Err(crate::Error::Invalid("--traces needs an http or https URL"));
        }
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
    let rows = Rows::read(tx)?;
    let Some(request) = rows.requests.iter().find(|request| request.id.0 == id) else {
        return Ok(None);
    };
    let card = card(&rows, request, now_ms, models);
    let key = id.to_string();
    let effects = effects(tx, &key)?;
    let (runs, blocked) = runs(&effects, models, links, &card)?;
    Ok(Some(CaseFile {
        revision: rows.revision,
        timeline: timeline(tx, &key, &effects, &card)?,
        children: children(tx, &key, &rows, now_ms)?,
        runs,
        incidents: incidents(tx, &key)?,
        blocked,
        question: question(request, &effects)?,
        summary: summary(request, &card, &effects)?,
        links: request_links(tx, request, links)?,
        card,
    }))
}

/// The request's card, from the board's own projection. The board drops
/// closed requests after a day; projecting at the close keeps them here.
fn card(rows: &Rows, request: &Request, now_ms: i64, models: Option<&tl3_config::Models>) -> Card {
    let mut shown = request.clone();
    let now_ms = match request.node.phase {
        Phase::Closed(_) => *shown.phase_since_ms.get_or_insert(now_ms),
        _ => now_ms,
    };
    let mut card = board::project(board::Input {
        revision: rows.revision,
        now_ms,
        requests: vec![shown],
        unassigned_incidents: Vec::new(),
        models,
        quota_resets: rows.quota_resets.clone(),
    })
    .cards
    .remove(0);
    card.phase_since_ms = request.phase_since_ms;
    card
}

/// One of the request's effects, with the commits that created and last moved it.
struct Effect {
    key: String,
    delivery: Delivery,
    state: String,
    created_revision: i64,
    created_ms: i64,
    /// The creating commit's event key and result.
    created_by: (String, Value),
    updated_ms: i64,
    /// The completion that last moved the effect, if one did.
    completion: Option<Completion>,
}

fn effects(tx: &mut Transaction<'_>, node: &str) -> crate::Result<Vec<Effect>> {
    let rows = tx
        .query(
            "SELECT effect.effect_key, effect.payload, effect.state::text, effect.created_revision,
                    (extract(epoch FROM created.committed_at) * 1000)::bigint,
                    created.event_key, created.result,
                    (extract(epoch FROM updated.committed_at) * 1000)::bigint,
                    updated.event_key, updated.payload
             FROM tl3_effects effect
             JOIN tl3_events created ON created.revision = effect.created_revision
             JOIN tl3_events updated ON updated.revision = effect.updated_revision
             WHERE COALESCE(effect.payload #>> '{output,Effect,node}',
                            effect.payload #>> '{output,Notice,node}') = $1
             ORDER BY effect.created_revision, effect.effect_key",
            &[&node],
        )
        .map_err(tl3_store::Error::from)?;
    rows.into_iter()
        .map(|row| {
            let key: String = row.get(0);
            let event: String = row.get(8);
            let completion = event
                .starts_with(&format!("completion:{key}:"))
                .then(|| serde_json::from_value(row.get(9)))
                .transpose()?;
            Ok(Effect {
                delivery: serde_json::from_value(row.get(1))?,
                key,
                state: row.get(2),
                created_revision: row.get(3),
                created_ms: row.get(4),
                created_by: (row.get(5), row.get(6)),
                updated_ms: row.get(7),
                completion,
            })
        })
        .collect()
}

/// The stage each step moved the request to, and the step's name. Steps
/// that don't move a request (stopping a run, withdrawing checks, notes) have none.
fn step(output: Output) -> Option<(Column, &'static str)> {
    Some(match output {
        Output::Effect { kind, .. } => match kind {
            EffectKind::StartRun(Role::Ingest) => (Column::Intake, "Reader started"),
            EffectKind::StartRun(Role::Audit) => (Column::Intake, "Auditor started"),
            EffectKind::StartRun(Role::Plan) => (Column::Planning, "Planner started"),
            EffectKind::PrepareWorktree => (Column::Making, "Change files prepared"),
            EffectKind::StartRun(Role::Make) => (Column::Making, "Maker started"),
            EffectKind::SubmitChecks => (Column::Checking, "Checks submitted"),
            EffectKind::StartRun(Role::Review) => (Column::Reviewing, "Reviewer started"),
            EffectKind::Push => (Column::Merging, "Branch pushed"),
            EffectKind::OpenPr => (Column::Merging, "Pull request opened"),
            EffectKind::PostApproval => (Column::Merging, "Approval posted"),
            EffectKind::Enqueue => (Column::Merging, "Sent to the merge queue"),
            EffectKind::StartRebase => (Column::Merging, "Branch update started"),
            EffectKind::ClosePr | EffectKind::RemoveWorktree => (Column::Done, "Cleaning up"),
            EffectKind::StopRun | EffectKind::WithdrawJob => return None,
        },
        Output::Notice {
            notice: Notice::Ask { .. },
            ..
        } => (Column::NeedsRob, "Question posted for Rob"),
        Output::Notice {
            notice: Notice::Summary(_),
            ..
        } => (Column::Done, "Closing summary posted"),
        _ => return None,
    })
}

/// Stage changes from the audit event log: each step the kernel started, at
/// the commit that started it, with that commit's kernel event as the cause.
/// A repeated step, such as a retried run, adds nothing.
fn timeline(
    tx: &mut Transaction<'_>,
    node: &str,
    effects: &[Effect],
    card: &Card,
) -> crate::Result<Vec<StageChange>> {
    let mut timeline: Vec<StageChange> = Vec::new();
    for effect in effects {
        let Some((column, step)) = step(effect.delivery.output) else {
            continue;
        };
        if timeline
            .last()
            .is_some_and(|last| last.column == column && last.step == step)
        {
            continue;
        }
        timeline.push(StageChange {
            revision: effect.created_revision,
            at_ms: effect.created_ms,
            column,
            step: step.into(),
            cause: cause(&effect.created_by.0, &effect.created_by.1, effects),
        });
    }
    // Where the request is now, when no step took it there: Ready, a pause, or closed.
    if timeline
        .last()
        .is_none_or(|last| last.column != card.column || card.ending.is_some())
        && let Some(row) = tx
            .query_opt(
                "SELECT events.revision, (extract(epoch FROM events.committed_at) * 1000)::bigint,
                        events.event_key, events.result
                 FROM tl3_node_phases phases JOIN tl3_events events ON events.committed_at = phases.since
                 WHERE phases.node_id = $1",
                &[&node],
            )
            .map_err(tl3_store::Error::from)?
    {
        let revision: i64 = row.get(0);
        if timeline.last().is_none_or(|last| last.revision <= revision) {
            timeline.push(StageChange {
                revision,
                at_ms: row.get(1),
                column: card.column,
                step: card.phase.clone(),
                cause: cause(row.get(2), &row.get(3), effects),
            });
        }
    }
    Ok(timeline)
}

/// What moved the request, from the commit's recorded kernel event.
fn cause(event_key: &str, result: &Value, effects: &[Effect]) -> String {
    let Ok(event) = serde_json::from_value::<Event>(result["kernel_event"].clone()) else {
        let source = event_key.split(':').next().unwrap_or(event_key);
        return format!("Recorded by the controller ({source})");
    };
    match event {
        Event::NewRequest { .. } => "Rob labelled the issue".into(),
        Event::NewAdoption { .. } => "Rob asked TL3 to adopt a pull request".into(),
        Event::Reply { reply, .. } => reply_cause(reply).into(),
        Event::Ack { node, seq, ack } => {
            let key = format!("effect:{}:{seq}", node.0);
            let step = effects
                .iter()
                .find(|effect| effect.key == key)
                .and_then(|effect| match effect.delivery.output {
                    Output::Effect { kind, .. } => Some(step_name(kind)),
                    _ => None,
                })
                .unwrap_or("A step");
            match ack {
                Ack::Confirmed => format!("{step} finished"),
                Ack::Retryable => format!("{step} will be retried"),
                Ack::Permanent => format!("{step} failed"),
            }
        }
        Event::Fact { fact, .. } => match fact {
            Fact::PrMerged { .. } => "The pull request merged",
            Fact::PrClosedByPerson => "Someone closed the pull request",
            Fact::PrDequeuedByPerson => "Someone took the pull request out of the merge queue",
            Fact::PushByOther { .. } => "Someone else pushed to the branch",
            Fact::CiFailed { infra: true } => "CI's infrastructure failed",
            Fact::CiFailed { infra: false } => "CI failed",
            Fact::Conflict => "The branch conflicts with its base",
            Fact::IncidentCleared { by_rob: true } => "Rob cleared the incident",
            Fact::IncidentCleared { by_rob: false } => "A probe cleared the incident",
            Fact::UpstreamStalled => "An outside service stopped making progress",
        }
        .into(),
        Event::Command { command, .. } => match command {
            Command::Stop => "Rob stopped the request".into(),
            Command::Pause => "Rob paused the request".into(),
            Command::Resume => "Rob resumed the request".into(),
            Command::ChangeRequest => "Rob changed the request".into(),
            Command::Answer { question, answer } => {
                format!("Rob answered question {question}: {}", answer_name(answer))
            }
        },
    }
}

fn reply_cause(reply: Reply) -> &'static str {
    match reply {
        Reply::IngestDone { .. } => "The reader understood the request",
        Reply::IngestEarlyEnd(EarlyEnd::Duplicate) => "The reader found a duplicate",
        Reply::IngestEarlyEnd(EarlyEnd::AlreadyDone) => "The reader found it already done",
        Reply::IngestEarlyEnd(EarlyEnd::NotActionable) => "The reader found it not actionable",
        Reply::AuditAgrees => "The auditor agreed",
        Reply::AuditDisagrees => "The auditor disagreed",
        Reply::PlanLeaf { .. } => "The planner returned a plan",
        Reply::PlanSplit => "The planner proposed a split",
        Reply::PlanCantMeet => "The planner couldn't meet the request",
        Reply::MakerDone { .. } => "The maker made a change",
        Reply::MakerNeedsFiles { .. } => "The maker asked for more files",
        Reply::MakerCantMeet => "The maker couldn't meet the request",
        Reply::MakerOutOfRoom => "The maker ran out of room",
        Reply::ChecksDone { pass: true, .. } => "Checks passed",
        Reply::ChecksDone { .. } => "Checks failed",
        Reply::Approved { .. } => "The reviewer approved",
        Reply::ChangesRequested => "The reviewer requested changes",
        Reply::ReviewerOutOfRoom => "The reviewer ran out of room",
        Reply::RebaseClean { .. } => "The branch update was clean",
        Reply::RebaseNeedsChanges => "The branch update needs changes",
        Reply::RunFailed => "A worker run failed",
    }
}

fn answer_name(answer: Answer) -> String {
    match answer {
        Answer::Continue => "continue".into(),
        Answer::TryAgain => "try again".into(),
        Answer::AddCredit(credit) => format!("add {credit} credit"),
        Answer::ChangeRequest => "change the request".into(),
        Answer::Stop => "stop".into(),
        Answer::Unclear => "unclear".into(),
        Answer::Retry => "retry".into(),
        Answer::MarkHandled => "mark handled".into(),
    }
}

fn step_name(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::StartRun(role) => role_names(role).1,
        EffectKind::SubmitChecks => "Checks submission",
        EffectKind::StartRebase => "Branch update",
        EffectKind::PrepareWorktree => "Worktree preparation",
        EffectKind::Push => "Branch push",
        EffectKind::PostApproval => "Approval post",
        EffectKind::OpenPr => "Pull request opening",
        EffectKind::Enqueue => "Merge queue admission",
        EffectKind::StopRun => "Stopping a run",
        EffectKind::WithdrawJob => "Withdrawing checks",
        EffectKind::ClosePr => "Closing the pull request",
        EffectKind::RemoveWorktree => "Worktree removal",
    }
}

/// A role's config and trace name, and its label on cards.
fn role_names(role: Role) -> (&'static str, &'static str) {
    match role {
        Role::Ingest => ("ingest", "Reader"),
        Role::Audit => ("audit", "Auditor"),
        Role::Plan => ("planner", "Planner"),
        Role::Make => ("maker", "Maker"),
        Role::Review => ("reviewer", "Reviewer"),
    }
}

fn runs(
    effects: &[Effect],
    models: Option<&tl3_config::Models>,
    links: &Links,
    card: &Card,
) -> crate::Result<(Vec<WorkerRun>, Vec<BlockedReport>)> {
    let mut runs = Vec::new();
    let mut blocked = Vec::new();
    for effect in effects {
        let Output::Effect {
            kind: EffectKind::StartRun(role),
            node,
            ..
        } = effect.delivery.output
        else {
            continue;
        };
        let (name, label) = role_names(role);
        let generation = effect.delivery.generation;
        let mut model = None;
        let mut result = None;
        for observation in effect
            .completion
            .iter()
            .flat_map(|completion| &completion.observations)
        {
            match observation {
                Observation::WorkerReply {
                    node: n,
                    reply,
                    model: ran,
                    ..
                } if *n == node => {
                    model = ran.clone();
                    if reply["kind"] == "blocked" {
                        blocked.push(BlockedReport {
                            generation: Some(generation),
                            evidence: cited(&reply["evidence"]),
                            report: reply["report"].as_str().map(plain),
                        });
                    }
                    result = Some(reply_outcome(name, reply));
                }
                Observation::WorkerFailed {
                    node: n,
                    failure_class,
                    failure_message,
                    ..
                } if *n == node => {
                    result = Some((
                        RunStatus::Failed,
                        plain(&format!("Failed: {failure_class}: {failure_message}")),
                    ));
                }
                Observation::Kernel(Event::Reply { node: n, reply, .. }) if *n == node => {
                    result = Some(match reply {
                        Reply::RunFailed => (RunStatus::Failed, "Failed".into()),
                        reply => (RunStatus::Finished, reply_outcome_of(*reply).into()),
                    });
                }
                Observation::Plan { node: n, .. } if *n == node => {
                    result = Some((RunStatus::Finished, "Returned a plan".into()));
                }
                Observation::Made { node: n, .. } if *n == node => {
                    result = Some((RunStatus::Finished, "Made a change".into()));
                }
                Observation::Reviewed { node: n, .. } if *n == node => {
                    result = Some((RunStatus::Finished, "Approved".into()));
                }
                _ => {}
            }
        }
        let ended = !matches!(
            effect.state.as_str(),
            "Intended" | "Sent" | "Uncertain" | "Cancelling"
        );
        let (status, outcome) = match effect.state.as_str() {
            _ if !ended => (RunStatus::Running, "Running".into()),
            "Cancelled" => (RunStatus::Cancelled, "Cancelled".into()),
            "Failed" => result.unwrap_or((RunStatus::Failed, "Failed".into())),
            _ => result.unwrap_or((RunStatus::Finished, "Finished".into())),
        };
        if !ended && card.role.as_deref() == Some(label) {
            model = model.or_else(|| card.model.clone());
        }
        // Makers move one rung down their ladder per attempt.
        let rung =
            (role == Role::Make).then(|| usize::from(effect.delivery.attempt.saturating_sub(1)));
        let effort = model
            .as_deref()
            .and_then(|model| effort(models, name, model, rung));
        runs.push(WorkerRun {
            key: effect.key.clone(),
            role: label.into(),
            generation,
            attempt: effect.delivery.attempt,
            round: effect.delivery.round,
            model,
            effort,
            started_ms: effect.created_ms,
            ended_ms: ended.then_some(effect.updated_ms),
            tokens: None,
            status,
            outcome,
            trace_url: links.trace(node.0, generation, name),
        });
    }
    Ok((runs, blocked))
}

/// Q50: the worker-checked evidence a `blocked` maker reply cites.
fn cited(evidence: &Value) -> String {
    let field = |name: &str| evidence[name].as_str().map(plain);
    match (
        field("action_key"),
        field("tool_error"),
        field("tool"),
        field("class"),
    ) {
        (Some(key), ..) => format!("PrismaBuild action {key} ended in a test infrastructure error"),
        (None, Some(error), Some(tool), Some(class)) => {
            format!("The maker's {tool} tool recorded {error} ({class})")
        }
        _ => "The reply cites no evidence the worker recorded".into(),
    }
}

/// What a worker's validated reply says, in plain words.
fn reply_outcome(role: &str, reply: &Value) -> (RunStatus, String) {
    let field = |name: &str| reply[name].as_str().unwrap_or_default();
    let outcome = match (role, field("kind"), field("verdict")) {
        ("maker", "blocked", _) => {
            return (RunStatus::Failed, "Blocked by the environment".into());
        }
        ("maker", "done", _) => "Made a change",
        ("maker", "needs_files", _) => "Asked for more files",
        ("maker" | "planner", "cant_meet", _) => "Can't meet the request",
        ("maker", "out_of_room", _) | ("reviewer", _, "out_of_room") => "Ran out of room",
        ("planner", "leaf", _) => "Returned a plan",
        ("planner", "split", _) => "Proposed a split",
        ("reviewer", _, "approve") => "Approved",
        ("reviewer", _, "changes") => "Changes requested",
        ("audit", _, "agree") => "Agreed",
        ("audit", _, "disagree") => "Disagreed",
        ("ingest", "brief", _) => "Understood the request",
        ("ingest", _, _) => "Ended the request early",
        _ => "Replied",
    };
    (RunStatus::Finished, outcome.into())
}

fn reply_outcome_of(reply: Reply) -> &'static str {
    match reply {
        Reply::IngestDone { .. } => "Understood the request",
        Reply::IngestEarlyEnd(_) => "Ended the request early",
        Reply::AuditAgrees => "Agreed",
        Reply::AuditDisagrees => "Disagreed",
        Reply::PlanLeaf { .. } => "Returned a plan",
        Reply::PlanSplit => "Proposed a split",
        Reply::PlanCantMeet | Reply::MakerCantMeet => "Can't meet the request",
        Reply::MakerDone { .. } => "Made a change",
        Reply::MakerNeedsFiles { .. } => "Asked for more files",
        Reply::MakerOutOfRoom | Reply::ReviewerOutOfRoom => "Ran out of room",
        Reply::ChecksDone { .. } => "Checks finished",
        Reply::Approved { .. } => "Approved",
        Reply::ChangesRequested => "Changes requested",
        Reply::RebaseClean { .. } | Reply::RebaseNeedsChanges => "Updated the branch",
        Reply::RunFailed => "Failed",
    }
}

/// The configured effort of a model in a role's ladder: the run's own rung
/// when it names that model, otherwise the model's first rung.
fn effort(
    models: Option<&tl3_config::Models>,
    role: &str,
    model: &str,
    rung: Option<usize>,
) -> Option<String> {
    let models = models?;
    let ladder: &[tl3_config::Model] = match role {
        "ingest" => &models.ingest,
        "audit" => &models.audit,
        "planner" => &models.planner,
        "maker" => &models.maker,
        "reviewer" => &models.reviewer,
        _ => return None,
    };
    rung.and_then(|rung| ladder.get(rung))
        .filter(|candidate| candidate.name == model)
        .or_else(|| ladder.iter().find(|candidate| candidate.name == model))?
        .effort
        .clone()
}

impl Links {
    /// The MLflow UI's traces for one run. TL3's workers tag each trace with
    /// its request, generation and role (workers/src/tl3_workers/tracing.py).
    fn trace(&self, node: u64, generation: u64, role: &str) -> Option<String> {
        let base = self.traces.as_deref()?;
        // MLflow 3.17's traces page reads repeated `filter` parameters of the
        // form field::operator::value::key.
        let filter =
            |key: &str, value: &str| format!("filter=tag%3A%3A%3D%3A%3A{value}%3A%3A{key}");
        Some(format!(
            "{base}{}{}&{}&{}",
            if base.contains('?') { '&' } else { '?' },
            filter("request", &node.to_string()),
            filter("generation", &generation.to_string()),
            filter("role", role),
        ))
    }

    fn branch(&self, repo: &str, issue: u64, node: u64) -> Option<String> {
        let repo = repo.to_ascii_lowercase();
        let (_, template) = self.branches.iter().find(|(name, _)| *name == repo)?;
        // tl3d validated the template at startup and named the branch with it.
        Some(
            template
                .replace("{issue}", &issue.to_string())
                .replace("{node}", &node.to_string()),
        )
    }
}

/// A stored incident, as TL3's controller writes it.
#[derive(Deserialize)]
struct StoredIncident {
    id: String,
    open: bool,
    needs_rob: bool,
    blocking: bool,
    summary: String,
    signature: Signature,
    #[serde(default)]
    effect: Option<String>,
    #[serde(default)]
    cause: Option<Cause>,
    #[serde(default)]
    input_failure: Option<Value>,
}

#[derive(Deserialize)]
struct Signature {
    upstream: Upstream,
    class: String,
}

fn incidents(tx: &mut Transaction<'_>, node: &str) -> crate::Result<Vec<CaseIncident>> {
    let rows = tx
        .query(
            "SELECT value FROM tl3_world WHERE table_name = 'incidents' AND value->>'node' = $1
             ORDER BY value->>'open' = 'true' DESC, arrival",
            &[&node],
        )
        .map_err(tl3_store::Error::from)?;
    rows.into_iter()
        .map(|row| {
            let incident: StoredIncident = serde_json::from_value(row.get(0))?;
            // Incidents from before Q51 store no cause. TL3 derives it the same
            // way (looks::Incident::cause): workers and local worktrees are TL3's.
            let tl3 = match incident.cause {
                Some(cause) => cause == Cause::Tl3,
                None => {
                    incident.input_failure.is_none()
                        && match incident.signature.upstream {
                            Upstream::Workers => true,
                            Upstream::Worktrees => !matches!(
                                incident.signature.class.as_str(),
                                "git_remote" | "invalid_token"
                            ),
                            _ => false,
                        }
                }
            };
            Ok(CaseIncident {
                cause: if tl3 {
                    IncidentCause::Tl3
                } else {
                    IncidentCause::Outside
                },
                summary: plain(&incident.summary),
                id: incident.id,
                open: incident.open,
                needs_rob: incident.needs_rob,
                blocking: incident.blocking,
                source: incident.signature.upstream.kind().into(),
                class: plain(&incident.signature.class),
                effect: incident.effect,
            })
        })
        .collect()
}

fn children(
    tx: &mut Transaction<'_>,
    node: &str,
    rows: &Rows,
    now_ms: i64,
) -> crate::Result<Vec<ChildItem>> {
    let mut children = Vec::new();
    if let Some(row) = tx
        .query_opt(
            "SELECT value FROM tl3_world WHERE table_name = 'plans' AND row_id = $1",
            &[&node],
        )
        .map_err(tl3_store::Error::from)?
    {
        let plan: Value = row.get(0);
        if plan["kind"] == "split" {
            for piece in plan["pieces"].as_array().into_iter().flatten() {
                children.push(ChildItem {
                    id: None,
                    title: plain(piece["brief"].as_str().unwrap_or_default()),
                    column: None,
                    stage: "Proposed".into(),
                    files: piece["files"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(plain)
                        .collect(),
                });
            }
        }
    }
    for row in tx
        .query(
            "SELECT row_id FROM tl3_world WHERE table_name = 'request_links' AND value->>'original' = $1
             ORDER BY arrival",
            &[&node],
        )
        .map_err(tl3_store::Error::from)?
    {
        let id: String = row.get(0);
        let Some(child) = rows
            .requests
            .iter()
            .find(|request| request.id.0.to_string() == id)
        else {
            continue;
        };
        let shown = card(rows, child, now_ms, None);
        children.push(ChildItem {
            id: Some(shown.id),
            title: shown.title,
            column: Some(shown.column),
            stage: shown.phase,
            files: Vec::new(),
        });
    }
    Ok(children)
}

/// The writer's facts for a notice: exactly what its posted text was written from.
fn notice_facts(request: &Request, effect: &Effect, notice: Notice) -> crate::Result<Value> {
    let title = plain(&request.title);
    Ok(tl3_controller::adapters::github::writer_input(
        &effect.delivery,
        Some(notice),
        &title,
        &title,
    )?
    .facts)
}

fn question(request: &Request, effects: &[Effect]) -> crate::Result<Option<PendingQuestion>> {
    let Some(question) = request.node.question else {
        return Ok(None);
    };
    let notice = Notice::Ask {
        question: question.id,
    };
    let kind = match question.kind {
        QKind::AuditDisagreement => "Readings disagree",
        QKind::NeedsSplitting => "Needs splitting",
        QKind::CantMeet => "Can't meet the request",
        QKind::OutOfCredit => "Out of credit",
        QKind::EffectFailed => "A step needs help",
        QKind::CleanupFailed => "Cleanup needs help",
    };
    let asked = effects.iter().rev().find(|effect| {
        matches!(effect.delivery.output, Output::Notice { notice: asked, .. } if asked == notice)
    });
    let facts = match asked {
        Some(effect) => notice_facts(request, effect, notice)?,
        None => Value::Null,
    };
    let text = |name: &str| plain(facts[name].as_str().unwrap_or_default());
    Ok(Some(PendingQuestion {
        id: question.id,
        kind: kind.into(),
        obstacle: if asked.is_some() {
            text("obstacle")
        } else {
            "The question's record wasn't found.".into()
        },
        options: facts["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|option| QuestionOption {
                action: plain(option["action"].as_str().unwrap_or_default()),
                outcome: plain(option["outcome"].as_str().unwrap_or_default()),
            })
            .collect(),
        recommended_option: facts["recommended_option"].as_u64().unwrap_or(0) as u32,
        recommendation_reason: text("recommendation_reason"),
        how_to_answer: text("how_to_answer"),
        posted: !request.question_post_failed,
    }))
}

fn summary(
    request: &Request,
    card: &Card,
    effects: &[Effect],
) -> crate::Result<Option<ClosingSummary>> {
    let (Phase::Stopping(outcome) | Phase::Closed(outcome), Some(ending)) =
        (request.node.phase, card.ending)
    else {
        return Ok(None);
    };
    let notice = Notice::Summary(outcome);
    let Some(effect) = effects.iter().rev().find(|effect| {
        matches!(effect.delivery.output, Output::Notice { notice: posted, .. } if posted == notice)
    }) else {
        return Ok(Some(ClosingSummary {
            ending,
            outcome: "No closing summary has been written yet.".into(),
            unmet_goals: Vec::new(),
            removed_changes: None,
            note: None,
        }));
    };
    let facts = notice_facts(request, effect, notice)?;
    Ok(Some(ClosingSummary {
        ending,
        outcome: plain(facts["outcome"].as_str().unwrap_or_default()),
        unmet_goals: facts["unmet_goals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(plain)
            .collect(),
        removed_changes: facts["removed_changes"].as_str().map(str::to_owned),
        note: effect.delivery.note.clone(),
    }))
}

fn request_links(
    tx: &mut Transaction<'_>,
    request: &Request,
    links: &Links,
) -> crate::Result<Vec<Link>> {
    let github = format!("https://github.com/{}", request.repo);
    let mut found = vec![Link {
        kind: LinkKind::Issue,
        label: format!("Issue {}#{} on GitHub", request.repo, request.issue),
        url: format!("{github}/issues/{}", request.issue),
    }];
    let node = request.node;
    let pull = tx
        .query_opt(
            "SELECT (value->>'number')::bigint FROM tl3_world WHERE table_name = 'pull_requests' AND row_id = $1",
            &[&request.id.0.to_string()],
        )
        .map_err(tl3_store::Error::from)?
        .and_then(|row| row.get::<_, Option<i64>>(0));
    if let Some(number) = pull {
        found.push(Link {
            kind: LinkKind::PullRequest,
            label: format!("Pull request #{number} on GitHub"),
            url: format!("{github}/pull/{number}"),
        });
    }
    if (node.worktree || node.had_pr || pull.is_some())
        && let Some(branch) = links.branch(&request.repo, request.issue, request.id.0)
    {
        found.push(Link {
            kind: LinkKind::Branch,
            label: format!("Branch {branch} on GitHub"),
            url: format!("{github}/tree/{branch}"),
        });
    }
    // Mergify reports a queued pull request's place on its checks.
    if let Some(number) = pull
        && (matches!(node.phase, Phase::Queued | Phase::Rebasing) || node.merged.is_some())
    {
        found.push(Link {
            kind: LinkKind::MergeQueue,
            label: format!("Merge queue checks for pull request #{number}"),
            url: format!("{github}/pull/{number}/checks"),
        });
    }
    Ok(found)
}

/// Stored text as one plain line: no control or direction-override
/// characters. The board cleans card text the same way.
fn plain(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            (!c.is_control() || c.is_whitespace())
                && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200b}'..='\u{200f}' | '\u{feff}')
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
