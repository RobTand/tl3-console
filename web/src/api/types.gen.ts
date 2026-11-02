// Generated from the console's Rust types by crates/console/src/api.rs. Do not edit.

export type Board = { 
/**
 * The controller revision this board shows.
 */
revision: number, built_at_ms: number, flow: Flow, 
/**
 * Ordered by id.
 */
cards: Array<Card>, unassigned_incidents: Array<CardIncident>, };

/**
 * What changed between two boards: replace or add `upserts` by id, drop
 * `removals`, and take the rest as given.
 */
export type BoardPatch = { revision: number, built_at_ms: number, flow: Flow, upserts: Array<Card>, removals: Array<number>, unassigned_incidents: Array<CardIncident>, };

export type Card = { 
/**
 * The request's node id.
 */
id: number, 
/**
 * `owner/name`.
 */
repo: string, issue: number, title: string, column: Column, phase: string, ending: Ending | null, waiting_on: string, role: string | null, model: string | null, attempt: number, round: number, retries: Retries, 
/**
 * None until ingest sets up the request's credit.
 */
credit: number | null, 
/**
 * When the request entered its current phase, in Unix milliseconds.
 */
phase_since_ms: number | null, last_progress: string | null, incidents: Array<CardIncident>, badges: Array<Badge>, };

export type CardIncident = { id: string, summary: string, };

/**
 * A workflow stage on the board, in board order.
 */
export type Column = "Backlog" | "Intake" | "Planning" | "Making" | "Checking" | "Reviewing" | "Merging" | "NeedsRob" | "Done";

export type ColumnCount = { column: Column, label: string, count: number, };

export type Badge = "Paused" | "Incident" | "MakerBlocked" | "NeedsRob" | "WaitingForCapacity";

/**
 * How a request ended, or is ending while it cleans up.
 */
export type Ending = "Merged" | "Stopped" | "Replaced" | "Duplicate" | "AlreadyDone" | "NotActionable";

export type Flow = { 
/**
 * One entry per column, in board order.
 */
columns: Array<ColumnCount>, 
/**
 * Requests that haven't reached Done.
 */
in_flight: number, 
/**
 * Merged requests, by when they entered cleanup or closed.
 */
merged_last_hour: number, merged_last_day: number, };

export type Retries = { plan: number, audit: number, run_failures: number, effect_failures: number, };

/**
 * The stream's `database` event: whether the console can read the store.
 */
export type DatabaseStatus = { connected: boolean, };

/**
 * The stream's `patch` event: apply `patch` to the board whose event id is
 * `base`. A client holding another board reconnects for a fresh snapshot.
 */
export type PatchEvent = { base: string, patch: BoardPatch, };

export type CaseFile = { 
/**
 * The controller revision this case file shows.
 */
revision: number, 
/**
 * The request as the board shows it, with its counters and credit.
 */
card: Card, 
/**
 * Stage changes, oldest first.
 */
timeline: Array<StageChange>, children: Array<ChildItem>, 
/**
 * Worker runs, oldest first.
 */
runs: Array<WorkerRun>, 
/**
 * Open incidents first, then cleared ones.
 */
incidents: Array<CaseIncident>, blocked: Array<BlockedReport>, question: PendingQuestion | null, summary: ClosingSummary | null, links: Array<Link>, };

/**
 * A step that moved the request to a stage, from the audit event log: the
 * commit that started the step, and the kernel event that commit recorded.
 */
export type StageChange = { revision: number, 
/**
 * When the commit landed, in Unix milliseconds.
 */
at_ms: number, column: Column, step: string, 
/**
 * What the controller recorded as the reason, in plain words.
 */
cause: string, };

/**
 * Work that continues this request: pieces a plan proposed, or a request
 * that replaced it.
 */
export type ChildItem = { 
/**
 * The child's request; proposed pieces have none yet.
 */
id: number | null, title: string, column: Column | null, stage: string, files: Array<string>, };

export type RunStatus = "Running" | "Finished" | "Failed" | "Cancelled";

export type WorkerRun = { 
/**
 * The run's effect key.
 */
key: string, role: string, generation: number, attempt: number, round: number, model: string | null, effort: string | null, started_ms: number, ended_ms: number | null, 
/**
 * None: the store doesn't record token use yet. The trace has it.
 */
tokens: number | null, status: RunStatus, outcome: string, 
/**
 * The run's MLflow traces, found by their tags.
 */
trace_url: string | null, };

/**
 * Q51: TL3's own code, prompts, tools or harness, or an outside service
 * such as GitHub, Mergify, PrismaBuild or the model proxy.
 */
export type IncidentCause = "Tl3" | "Outside";

export type CaseIncident = { id: string, summary: string, open: boolean, cause: IncidentCause, needs_rob: boolean, blocking: boolean, 
/**
 * The upstream that failed, such as GitHub or Workers.
 */
source: string, 
/**
 * The failure signature's class.
 */
class: string, 
/**
 * The effect whose delivery failed, if one did.
 */
effect: string | null, };

/**
 * Q50: a maker that TL3's environment stopped, and what it cited.
 */
export type BlockedReport = { generation: number | null, 
/**
 * The worker-checked evidence the reply cited.
 */
evidence: string, 
/**
 * The maker's own report. Model text; it never goes to GitHub.
 */
report: string | null, };

export type QuestionOption = { action: string, outcome: string, };

/**
 * The question waiting on Rob, as the writer was asked to put it.
 */
export type PendingQuestion = { id: number, kind: string, obstacle: string, options: Array<QuestionOption>, 
/**
 * An index into `options`.
 */
recommended_option: number, recommendation_reason: string, how_to_answer: string, 
/**
 * False when posting the question failed.
 */
posted: boolean, };

export type ClosingSummary = { ending: Ending, outcome: string, unmet_goals: Array<string>, removed_changes: string | null, note: string | null, };

export type LinkKind = "Issue" | "PullRequest" | "Branch" | "MergeQueue";

export type Link = { kind: LinkKind, 
/**
 * The link's accessible name.
 */
label: string, url: string, };

/**
 * One version of a role's system prompt. `number` counts a role's versions from 1.
 */
export type PromptVersion = { id: number, number: number, text: string, builtin: boolean, author: string, note: string, created_ms: number, };

/**
 * A version in a role's history, without its text. Fetch the text from
 * `GET /api/prompts/{role}/versions/{id}`.
 */
export type PromptSummary = { id: number, number: number, builtin: boolean, author: string, note: string, created_ms: number, };

/**
 * A role's prompt: the active version, its history, newest first, and a
 * built-in version a deploy shipped while another stayed active.
 */
export type RolePrompt = { role: string, active: PromptVersion, activated_by: string, activated_ms: number, builtin_update: PromptVersion | null, versions: Array<PromptSummary>, };

/**
 * A role in the prompt list.
 */
export type PromptRole = { role: string, active_number: number, active_builtin: boolean, builtin_update: boolean, };

/**
 * `POST /api/prompts/{role}/versions`: a version that becomes active at
 * once. `base` is the active version the edit started from.
 */
export type NewPromptVersion = { text: string, note: string, base: number, };

/**
 * `POST /api/prompts/{role}/activate/{version}`: `expected` is the version
 * the page showed as active.
 */
export type ActivateVersion = { expected: number, };
