//! The console's HTTP API types. The web app's TypeScript types are generated
//! from these and the board's, so the two can't drift.

use serde::{Deserialize, Serialize};
use tl3_controller::{
    board::{
        Badge, Board, BoardPatch, Card, CardIncident, Column, ColumnCount, Ending, Flow, Retries,
    },
    console::{
        Action, AnswerKind, CardQuestion, Choice, CommandRecord, CommandStatus, Detail,
        QuestionOption,
    },
};
use ts_rs::TS;

/// The stream's `database` event: whether the console can read the store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct DatabaseStatus {
    pub connected: bool,
}

/// The stream's `patch` event: apply `patch` to the board whose event id is
/// `base`. A client holding another board reconnects for a fresh snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct PatchEvent {
    pub base: String,
    pub patch: BoardPatch,
}

/// `POST /api/requests/{id}/commands`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    /// A lowercase UUID the browser chooses; resending it files nothing more.
    pub id: String,
    pub action: Action,
}

/// The answer to a command: filed for tl3d, or already filed under this id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct Filed {
    pub id: String,
    pub filed: bool,
}

/// The stream's `commands` event: the newest filed commands and how each settled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
pub struct CommandsEvent {
    pub commands: Vec<CommandRecord>,
}

/// The generated `web/src/api/types.gen.ts`.
pub fn typescript() -> String {
    let config = ts_rs::Config::new().with_large_int("number");
    let mut output = String::from(
        "// Generated from the console's Rust types by crates/console/src/api.rs. Do not edit.\n",
    );
    for (docs, decl) in [
        (Board::docs(), Board::decl(&config)),
        (BoardPatch::docs(), BoardPatch::decl(&config)),
        (Card::docs(), Card::decl(&config)),
        (CardIncident::docs(), CardIncident::decl(&config)),
        (Column::docs(), Column::decl(&config)),
        (ColumnCount::docs(), ColumnCount::decl(&config)),
        (Badge::docs(), Badge::decl(&config)),
        (Ending::docs(), Ending::decl(&config)),
        (Flow::docs(), Flow::decl(&config)),
        (Retries::docs(), Retries::decl(&config)),
        (DatabaseStatus::docs(), DatabaseStatus::decl(&config)),
        (PatchEvent::docs(), PatchEvent::decl(&config)),
        (CardQuestion::docs(), CardQuestion::decl(&config)),
        (QuestionOption::docs(), QuestionOption::decl(&config)),
        (AnswerKind::docs(), AnswerKind::decl(&config)),
        (Detail::docs(), Detail::decl(&config)),
        (Action::docs(), Action::decl(&config)),
        (Choice::docs(), Choice::decl(&config)),
        (CommandRequest::docs(), CommandRequest::decl(&config)),
        (Filed::docs(), Filed::decl(&config)),
        (CommandStatus::docs(), CommandStatus::decl(&config)),
        (CommandRecord::docs(), CommandRecord::decl(&config)),
        (CommandsEvent::docs(), CommandsEvent::decl(&config)),
    ] {
        output.push('\n');
        output.push_str(&docs.unwrap_or_default());
        output.push_str("export ");
        output.push_str(&decl);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    #[test]
    fn web_api_types_are_current() {
        // Bazel names the file as a runfile; Cargo finds it beside the crate.
        let path = env::var_os("TL3_API_TYPES").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/api/types.gen.ts"),
            PathBuf::from,
        );
        let generated = super::typescript();
        if env::var_os("TL3_WRITE_API_TYPES").is_some() {
            fs::write(&path, &generated).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap_or_default(),
            generated,
            "web/src/api/types.gen.ts is stale; regenerate it with TL3_WRITE_API_TYPES=1 cargo test -p tl3-console --lib"
        );
    }
}
