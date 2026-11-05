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
        let config = Self {
            store: section("store", file.store)?,
            models: models(file.models).map_err(|error: toml::de::Error| {
                invalid("tl3_config.models", error.to_string())
            })?,
            github: section("github", file.github)?,
        };
        for (index, repo) in config.github.repos.iter().enumerate() {
            for (field, value) in [
                ("owner", &repo.owner),
                ("name", &repo.name),
                ("branch_template", &repo.branch_template),
            ] {
                if value.trim().is_empty() {
                    return Err(invalid(
                        &format!("tl3_config.github.repos[{index}].{field}"),
                        "must not be empty",
                    ));
                }
            }
        }
        Ok(config)
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

// The pinned board projection takes tl3_config::Models. Strip unknown keys at
// this boundary, including each ladder entry, before deserializing that type.
fn models<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<tl3_config::Models, D::Error> {
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
    let mut table = toml::Table::deserialize(deserializer)?;
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
    let mut models: tl3_config::Models = toml::Value::Table(table)
        .try_into()
        .map_err(|error: toml::de::Error| serde::de::Error::custom(error.to_string()))?;
    // Preserve the pinned parser's implicit ladders, used by board projection.
    if models.reviewer.is_empty() {
        models.reviewer = models
            .maker
            .first()
            .into_iter()
            .chain(models.maker.last())
            .cloned()
            .collect();
    }
    if models.writer.is_empty() {
        models.writer = models.ingest.clone();
    }
    if models.postmortem.is_empty() {
        models.postmortem = models
            .reviewer
            .first()
            .into_iter()
            .chain(models.writer.first())
            .cloned()
            .collect();
    }
    for (role, ladder) in [
        ("ingest", &models.ingest),
        ("audit", &models.audit),
        ("planner", &models.planner),
        ("maker", &models.maker),
        ("reviewer", &models.reviewer),
        ("rebaser", &models.rebaser),
        ("comment_classifier", &models.comment_classifier),
        ("writer", &models.writer),
        ("postmortem", &models.postmortem),
    ] {
        if ladder.is_empty() {
            return Err(serde::de::Error::custom(format!(
                "models.{role}: must contain at least one model"
            )));
        }
        for (index, model) in ladder.iter().enumerate() {
            if model.name.trim().is_empty()
                || model.family.trim().is_empty()
                || model
                    .effort
                    .as_ref()
                    .is_some_and(|effort| effort.trim().is_empty())
                || model.budget_tokens == 0
            {
                return Err(serde::de::Error::custom(format!(
                    "models.{role}[{index}]: name, family and effort must not be empty; budget_tokens must be positive"
                )));
            }
        }
    }
    Ok(models)
}
