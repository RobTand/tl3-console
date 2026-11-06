//! TL3 config as read by the console. Controller-only sections are ignored.

use super::invalid;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Tl3 {
    store: Store,
    pub models: tl3_config::Models,
    pub github: GitHub,
}

#[derive(Deserialize)]
struct File {
    store: toml::Value,
    models: toml::Value,
    github: toml::Value,
}

#[derive(Debug, Deserialize)]
struct Store {
    host: String,
    port: u16,
    database: String,
    connect_timeout_secs: u32,
}

#[derive(Debug, Deserialize)]
pub struct GitHub {
    pub repos: Vec<Repository>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub name: String,
    #[serde(default = "branch_template")]
    pub branch_template: String,
}

fn branch_template() -> String {
    "tl3/issue-{issue}-node-{node}".into()
}

impl Tl3 {
    pub fn parse(text: &str) -> crate::Result<Self> {
        let file: File = toml::from_str(text)
            .map_err(|error: toml::de::Error| invalid("tl3_config", error.message()))?;
        let store = section("store", file.store)?;
        let github: GitHub = section("github", file.github)?;
        let models = models(file.models, &github.repos)?;
        Ok(Self {
            store,
            models,
            github,
        })
    }

    pub(super) fn store(
        &self,
        user: String,
        password_file: PathBuf,
    ) -> crate::Result<tl3_config::Store> {
        let store = tl3_config::Store {
            host: self.store.host.clone(),
            port: self.store.port,
            database: self.store.database.clone(),
            connect_timeout_secs: self.store.connect_timeout_secs,
            user,
            password_file: Some(password_file),
        };
        store
            .validate()
            .map_err(|error| invalid("tl3_config", error))?;
        Ok(store)
    }
}

fn section<T: serde::de::DeserializeOwned>(name: &str, value: toml::Value) -> crate::Result<T> {
    // Value deserialization preserves the field path without retaining source lines.
    value
        .try_into()
        .map_err(|error: toml::de::Error| invalid(&format!("tl3_config.{name}"), error.to_string()))
}

// The pinned validators for GitHub names, templates and model ladders are private
// to Config. Give that parser only the console's fields plus fixed values for
// everything else. Config::parse also supplies its implicit model ladders.
fn models(value: toml::Value, repos: &[Repository]) -> crate::Result<tl3_config::Models> {
    const ROLES: &[&str] = &[
        "ingest",
        "audit",
        "planner",
        "maker",
        "reviewer",
        "rebaser",
        "comment_classifier",
        "writer",
        "postmortem",
    ];
    let mut table: toml::Table = section("models", value)?;
    table.retain(|key, _| ROLES.contains(&key) || matches!(key, "base_url" | "key_file"));
    for role in ROLES {
        if let Some(toml::Value::Array(ladder)) = table.get_mut(*role) {
            for model in ladder {
                if let Some(model) = model.as_table_mut() {
                    model.retain(|key, _| {
                        matches!(key, "name" | "family" | "effort" | "budget_tokens")
                    });
                }
            }
        }
    }
    let mut config: toml::Table = toml::from_str(VALIDATION_DEFAULTS)
        .expect("the console's fixed validation context is valid TOML");
    config.insert("models".into(), toml::Value::Table(table));
    let repositories = repos
        .iter()
        .map(|repo| {
            toml::Value::Table(toml::Table::from_iter([
                ("owner".into(), repo.owner.clone().into()),
                ("name".into(), repo.name.clone().into()),
                (
                    "branch_template".into(),
                    repo.branch_template.clone().into(),
                ),
                ("installation_id".into(), toml::Value::Integer(1)),
            ]))
        })
        .collect();
    config
        .get_mut("github")
        .unwrap()
        .as_table_mut()
        .unwrap()
        .insert("repos".into(), toml::Value::Array(repositories));
    let text = toml::to_string(&config).map_err(|error| invalid("tl3_config", error))?;
    tl3_config::Config::parse(&text)
        .map(|config| config.models)
        .map_err(|error| invalid("tl3_config", error))
}

// These fields never come from TL3's file and never start services or inspect
// files. The maximum context avoids imposing the controller's unused token
// limit on the console; model budgets still have to be positive u32 values.
const VALIDATION_DEFAULTS: &str = r#"
worktrees_root = "/console-config-validation"

[limits]
fit_tokens = 100000
context_tokens = 4294967295
credit_reserve = 3
free_rechecks = 3
w = 1

[github]
app_credentials_file = "/console-config-validation/github.json"

[github.rob]
login = "console"
id = 1

[prismabuild]
tools_dir = "/console-config-validation"
container_image = "sha256:0000000000000000000000000000000000000000000000000000000000000000"

[hindsight]
endpoint = "http://127.0.0.1:1"
bank = "console"

[status]
listen = "127.0.0.1:1"

[allowances]
"#;
