//! The console's own config file, apart from TL3's `tl3.toml`.
//!
//! ```toml
//! listen = "192.168.1.107:8034"
//! assets = "/home/rob/.local/share/tl3-console/assets"
//! user = "tl3_console"
//! password_file = "/home/rob/.config/tl3/secrets/console-postgres-password"
//! tl3_config = "/home/rob/.config/tl3/tl3.toml"
//! traces = "http://192.168.1.107:5033/#/experiments/2/traces"
//! public_hosts = ["console.example.test"]
//! hide_repos = ["RobTand/tl3-sandbox"]
//! ```
//!
//! The console reads only its view of TL3 config, tolerates additional keys,
//! and connects as its own login.

mod tl3;
pub use tl3::{Repository, Tl3};

use serde::Deserialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub listen: SocketAddr,
    /// The built web app. Deploy builds and installs it.
    pub assets: PathBuf,
    /// The read-only Postgres login TL3's `tl3-deploy console-role` grants.
    pub user: String,
    pub password_file: PathBuf,
    /// TL3's config, read for `[store]`, `[models]` and the served repos only.
    pub tl3_config: PathBuf,
    /// The MLflow UI's traces page for TL3's experiment. The case file links
    /// each worker run to its traces there; without it, runs have no link.
    #[serde(default)]
    pub traces: Option<String>,
    /// Host names the console answers to besides `listen`, such as a tunnel's
    /// public name. A request for any other Host is refused.
    #[serde(default)]
    pub public_hosts: Vec<String>,
    /// Served repositories (`owner/name`) whose requests the console never shows.
    #[serde(default)]
    pub hide_repos: Vec<String>,
}

/// What the console runs with.
#[derive(Clone, Debug)]
pub struct Settings {
    pub listen: SocketAddr,
    pub assets: PathBuf,
    pub store: tl3_config::Store,
    pub models: tl3_config::Models,
    /// Where the case file's links point.
    pub links: crate::case_file::Links,
    /// Host names the console answers to besides `listen`.
    pub public_hosts: Vec<String>,
    /// Repositories whose requests the console never shows.
    pub hidden: crate::hidden::HiddenRepos,
}

impl Settings {
    /// Read both files and check the password file's metadata, never its contents.
    pub fn load(path: impl AsRef<Path>) -> crate::Result<Self> {
        let console = read("config", path.as_ref())?;
        let file = File::parse(&console)?;
        let settings = Self::parse(&console, &read("tl3_config", &file.tl3_config)?)?;
        secret_file(&file.password_file)?;
        Ok(settings)
    }

    /// The same, from text, without touching the password file.
    pub fn parse(console: &str, tl3: &str) -> crate::Result<Self> {
        let file = File::parse(console)?;
        let tl3 = Tl3::parse(tl3)?;
        Ok(Self {
            listen: file.listen,
            assets: file.assets,
            store: tl3.store(file.user, file.password_file)?,
            links: crate::case_file::Links::new(file.traces.as_deref(), &tl3.github.repos)?,
            models: tl3.models,
            public_hosts: file.public_hosts,
            hidden: crate::hidden::HiddenRepos::new(&file.hide_repos, &tl3.github.repos)?,
        })
    }
}

impl File {
    fn parse(text: &str) -> crate::Result<Self> {
        // TOML errors can quote source lines; keep only the message.
        let file: Self = toml::from_str(text)
            .map_err(|error: toml::de::Error| invalid("console config", error.message()))?;
        let ip = file.listen.ip();
        if file.listen.port() == 0 || ip.is_multicast() || ip.is_unspecified() {
            return Err(invalid(
                "listen",
                "use a unicast listen address with a fixed port from 1 to 65535",
            ));
        }
        for (field, path) in [
            ("assets", &file.assets),
            ("password_file", &file.password_file),
            ("tl3_config", &file.tl3_config),
        ] {
            if !path.is_absolute() {
                return Err(invalid(field, "use an absolute path"));
            }
        }
        if !file
            .public_hosts
            .iter()
            .all(|name| crate::hosts::valid_name(name))
        {
            return Err(invalid("public_hosts", crate::hosts::NAME_RULE));
        }
        // TL3's `tl3-deploy console-role` names this role in GRANT statements.
        if !file
            .user
            .starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            || !file
                .user
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(invalid(
                "user",
                "use a lowercase Postgres role name: letters, digits and underscores",
            ));
        }
        Ok(file)
    }
}

pub(crate) fn invalid(field: &str, message: impl ToString) -> crate::Error {
    crate::Error::Config(tl3_config::Error::Invalid {
        field: field.into(),
        message: message.to_string(),
    })
}

fn read(field: &str, path: &Path) -> crate::Result<String> {
    std::fs::read_to_string(path).map_err(|source| {
        crate::Error::Config(tl3_config::Error::File {
            field: field.into(),
            path: path.into(),
            source,
        })
    })
}

fn secret_file(path: &Path) -> crate::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|source| {
        crate::Error::Config(tl3_config::Error::File {
            field: "password_file".into(),
            path: path.into(),
            source,
        })
    })?;
    let mode = metadata.permissions().mode();
    if !metadata.is_file() || mode & 0o077 != 0 || mode & 0o400 == 0 {
        return Err(invalid(
            "password_file",
            "must be a regular owner-only file; run chmod 600 on it",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Settings;
    use std::{fs, os::unix::fs::PermissionsExt};

    const TL3: &str = include_str!("../tests/fixtures/tl3-example.toml");

    fn console(fields: &[(&str, &str)]) -> String {
        let mut values = vec![
            ("listen", "\"192.168.1.107:8034\""),
            ("assets", "\"/srv/tl3-console/assets\""),
            ("user", "\"tl3_console\""),
            ("password_file", "\"/run/tl3-console/password\""),
            ("tl3_config", "\"/etc/tl3/tl3.toml\""),
        ];
        for (key, value) in fields {
            values.retain(|(name, _)| name != key);
            if !value.is_empty() {
                values.push((key, value));
            }
        }
        values
            .iter()
            .map(|(key, value)| format!("{key} = {value}\n"))
            .collect()
    }

    fn rejects(fields: &[(&str, &str)], named: &str) {
        let error = Settings::parse(&console(fields), TL3)
            .unwrap_err()
            .to_string();
        assert!(error.contains(named), "{fields:?}: {error}");
    }

    #[test]
    fn public_hosts_are_host_names() {
        let settings = Settings::parse(&console(&[]), TL3).unwrap();
        assert!(settings.public_hosts.is_empty());
        let settings = Settings::parse(
            &console(&[(
                "public_hosts",
                "[\"console.example.test\", \"10.0.0.7:8034\"]",
            )]),
            TL3,
        )
        .unwrap();
        assert_eq!(
            settings.public_hosts,
            ["console.example.test", "10.0.0.7:8034"]
        );
        rejects(
            &[("public_hosts", "[\"http://console.example.test\"]")],
            "public_hosts",
        );
        rejects(&[("public_hosts", "[\"\"]")], "public_hosts");
    }

    #[test]
    fn hide_repos_names_served_repositories() {
        let settings = Settings::parse(&console(&[]), TL3).unwrap();
        assert!(settings.hidden.is_empty());
        let settings = Settings::parse(
            &console(&[("hide_repos", "[\"RobTand/tl3-sandbox\"]")]),
            TL3,
        )
        .unwrap();
        assert!(settings.hidden.contains("RobTand/tl3-sandbox"));
        // GitHub ignores case, and so does TL3's own repository identity.
        assert!(settings.hidden.contains("robtand/TL3-SANDBOX"));
        assert!(!settings.hidden.contains("RobTand/tl3-console"));
        assert!(!settings.hidden.contains("RobTand/tl3-sandbox-two"));
    }

    #[test]
    fn hide_repos_refuses_a_name_tl3_does_not_serve() {
        // A typo must not quietly hide nothing; `tl3-console check` fails on it.
        for entry in [
            "RobTand/tl3-sandbx",
            "tl3-sandbox",
            "OtherOwner/tl3-sandbox",
            "",
        ] {
            let error = Settings::parse(&console(&[("hide_repos", &format!("[{entry:?}]"))]), TL3)
                .unwrap_err()
                .to_string();
            assert!(error.contains("hide_repos"), "{entry}: {error}");
            assert!(error.contains(&format!("{entry:?}")), "{entry}: {error}");
            assert!(error.contains("github.repos"), "{entry}: {error}");
        }
        // One good name does not excuse a bad one.
        rejects(
            &[("hide_repos", "[\"RobTand/tl3-sandbox\", \"RobTand/oops\"]")],
            "RobTand/oops",
        );
    }

    #[test]
    fn connects_to_tl3s_store_as_its_own_login() {
        let settings = Settings::parse(&console(&[]), TL3).unwrap();
        let tl3 = tl3_config::Config::parse(TL3).unwrap();
        assert_eq!(settings.listen, "192.168.1.107:8034".parse().unwrap());
        assert_eq!(
            settings.assets,
            std::path::Path::new("/srv/tl3-console/assets")
        );
        assert_eq!(settings.store.user, "tl3_console");
        assert_eq!(
            settings.store.password_file.as_deref(),
            Some(std::path::Path::new("/run/tl3-console/password"))
        );
        assert_eq!(
            (
                settings.store.host,
                settings.store.port,
                settings.store.database
            ),
            (tl3.store.host, tl3.store.port, tl3.store.database)
        );
        assert_eq!(
            format!("{:?}", settings.models),
            format!("{:?}", tl3.models)
        );
        assert_eq!(
            settings.links.branches,
            tl3.github
                .repos
                .iter()
                .map(|repo| (
                    format!("{}/{}", repo.owner, repo.name).to_ascii_lowercase(),
                    repo.branch_template.clone(),
                ))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn refuses_unknown_fields_and_unsafe_values() {
        rejects(&[("port", "8034")], "port");
        rejects(&[("listen", "\"0.0.0.0:8034\"")], "listen");
        rejects(&[("listen", "\"127.0.0.1:0\"")], "listen");
        rejects(&[("assets", "\"assets\"")], "assets");
        rejects(&[("password_file", "\"password\"")], "password_file");
        rejects(&[("tl3_config", "\"tl3.toml\"")], "tl3_config");
        rejects(&[("user", "\"tl3 console\"")], "user");
        rejects(&[("user", "")], "user");
        rejects(&[("traces", "\"file:///etc/passwd\"")], "traces");
    }

    #[test]
    fn links_run_to_the_traces_page_and_tl3s_branches() {
        let settings = Settings::parse(&console(&[]), TL3).unwrap();
        assert_eq!(settings.links.traces, None);
        let traces = "http://192.168.1.107:5033/#/experiments/2/traces";
        let settings =
            Settings::parse(&console(&[("traces", &format!("{traces:?}"))]), TL3).unwrap();
        assert_eq!(settings.links.traces.as_deref(), Some(traces));
        let tl3 = tl3_config::Config::parse(TL3).unwrap();
        assert_eq!(settings.links.branches.len(), tl3.github.repos.len());
    }

    #[test]
    fn tolerates_future_tl3_fields_without_changing_used_settings() {
        let future = include_str!("../tests/fixtures/tl3-future.toml");
        let expected = Settings::parse(&console(&[]), TL3).unwrap();
        let actual = Settings::parse(&console(&[]), future).unwrap();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    #[test]
    fn tolerates_unknown_keys_inside_used_tl3_sections() {
        let original: toml::Value = toml::from_str(TL3).unwrap();
        let expected = Settings::parse(&console(&[]), TL3).unwrap();
        for section in ["store", "models", "github", "repository", "model"] {
            let mut value = original.clone();
            let table = match section {
                "repository" => value["github"]["repos"][0].as_table_mut().unwrap(),
                "model" => value["models"]["maker"][0].as_table_mut().unwrap(),
                name => value[name].as_table_mut().unwrap(),
            };
            table.insert("future_option".into(), toml::Value::Boolean(true));
            let actual = Settings::parse(&console(&[]), &toml::to_string(&value).unwrap())
                .unwrap_or_else(|error| panic!("{section}: {error}"));
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{section}");
        }
    }

    #[test]
    fn needs_only_the_tl3_sections_the_console_uses() {
        let mut value: toml::Value = toml::from_str(TL3).unwrap();
        value
            .as_table_mut()
            .unwrap()
            .retain(|key, _| matches!(key, "store" | "models" | "github"));
        Settings::parse(&console(&[]), &toml::to_string(&value).unwrap()).unwrap();
    }

    #[test]
    fn refuses_missing_or_malformed_tl3_store() {
        let mut value: toml::Value = toml::from_str(TL3).unwrap();
        for store in [None, Some(toml::Value::String("wrong".into()))] {
            value.as_table_mut().unwrap().remove("store");
            if let Some(store) = store {
                value.as_table_mut().unwrap().insert("store".into(), store);
            }
            let error = Settings::parse(&console(&[]), &toml::to_string(&value).unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains("tl3_config"), "{error}");
            assert!(error.contains("store"), "{error}");
        }
        let malformed = TL3.replace("port = 5432", "port = \"wrong\"");
        let error = Settings::parse(&console(&[]), &malformed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("tl3_config"), "{error}");
        assert!(error.contains("port"), "{error}");
    }

    #[test]
    fn refuses_malformed_used_tl3_sections_and_values() {
        let original: toml::Value = toml::from_str(TL3).unwrap();
        for section in ["store", "models", "github"] {
            for replacement in [None, Some(toml::Value::Integer(7))] {
                let mut value = original.clone();
                value.as_table_mut().unwrap().remove(section);
                if let Some(replacement) = replacement {
                    value
                        .as_table_mut()
                        .unwrap()
                        .insert(section.into(), replacement);
                }
                let error = Settings::parse(&console(&[]), &toml::to_string(&value).unwrap())
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(section), "{section}: {error}");
            }
        }
        for (key, replacement) in [
            ("host", toml::Value::String("remote.example.test".into())),
            ("port", toml::Value::Integer(0)),
            ("database", toml::Value::String(String::new())),
            ("connect_timeout_secs", toml::Value::Integer(0)),
        ] {
            let mut value = original.clone();
            value["store"][key] = replacement;
            let error = Settings::parse(&console(&[]), &toml::to_string(&value).unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains("store"), "{key}: {error}");
        }
        for (section, key) in [
            ("store", "database"),
            ("models", "maker"),
            ("github", "repos"),
        ] {
            let mut value = original.clone();
            value[section].as_table_mut().unwrap().remove(key);
            let error = Settings::parse(&console(&[]), &toml::to_string(&value).unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains(section), "{section}.{key}: {error}");
            assert!(error.contains(key), "{section}.{key}: {error}");
        }
    }

    #[test]
    fn unrelated_tl3_validation_does_not_gate_console() {
        let mut value: toml::Value = toml::from_str(TL3).unwrap();
        value["limits"] = "future representation".into();
        value["console"] = false.into();
        value["store"]["user"] = 7.into();
        value["github"]["app_credentials_file"] = 7.into();
        value["github"]["rob"] = false.into();
        value["github"]["repos"][0]["installation_id"] = 0.into();
        value["github"]["repos"][0]
            .as_table_mut()
            .unwrap()
            .insert("protected_paths".into(), 7.into());
        Settings::parse(&console(&[]), &toml::to_string(&value).unwrap()).unwrap();
    }

    #[test]
    fn accepts_valid_repository_names_and_custom_templates() {
        let mut value: toml::Value = toml::from_str(TL3).unwrap();
        value["github"]["repos"][0]["owner"] = "Rob-Tand".into();
        value["github"]["repos"][0]["name"] = "tl3.console_example".into();
        let template = "feature/node-{node}-issue-{issue}";
        value["github"]["repos"][0]
            .as_table_mut()
            .unwrap()
            .insert("branch_template".into(), template.into());
        let text = toml::to_string(&value).unwrap();
        tl3_config::Config::parse(&text).unwrap();
        let settings = Settings::parse(&console(&[]), &text).unwrap();
        assert_eq!(
            settings.links.branches,
            vec![("rob-tand/tl3.console_example".into(), template.into())]
        );
    }

    fn rejects_like_pinned(change: impl FnOnce(&mut toml::Value), field: &str) {
        let mut value: toml::Value = toml::from_str(TL3).unwrap();
        change(&mut value);
        let text = toml::to_string(&value).unwrap();
        let expected = tl3_config::Config::parse(&text).unwrap_err().to_string();
        assert!(expected.contains(field), "{expected}");
        let actual = Settings::parse(&console(&[]), &text)
            .expect_err("console accepted a value rejected by the pinned parser")
            .to_string();
        assert_eq!(actual, format!("tl3_config: {expected}"));
    }

    #[test]
    fn rejects_empty_served_repositories() {
        rejects_like_pinned(
            |value| value["github"]["repos"] = toml::Value::Array(vec![]),
            "github.repos",
        );
    }

    #[test]
    fn rejects_invalid_repository_owners() {
        for owner in [
            "bad/owner",
            "bad_owner",
            "bad.owner",
            " owner",
            "owner\n",
            "",
            ".",
            "..",
        ] {
            rejects_like_pinned(
                |value| value["github"]["repos"][0]["owner"] = owner.into(),
                "github.repos[0].owner",
            );
        }
    }

    #[test]
    fn rejects_invalid_repository_names() {
        for name in ["bad/name", "bad name", "name\t", "", ".", ".."] {
            rejects_like_pinned(
                |value| value["github"]["repos"][0]["name"] = name.into(),
                "github.repos[0].name",
            );
        }
    }

    #[test]
    fn rejects_duplicate_repositories() {
        for mixed_case in [false, true] {
            rejects_like_pinned(
                |value| {
                    let mut duplicate = value["github"]["repos"][0].clone();
                    if mixed_case {
                        duplicate["owner"] = "robtand".into();
                        duplicate["name"] = "TL3-SANDBOX".into();
                    }
                    value["github"]["repos"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                },
                "github.repos[1]",
            );
        }
    }

    #[test]
    fn rejects_invalid_branch_templates() {
        for template in [
            "broken",
            "tl3/{issue}",
            "tl3/{node}",
            "tl3/{issue}{node}",
            "tl3/{issue}-{node}-{other}",
            "../{issue}-{node}",
            "tl3/{issue}-{node}.lock",
        ] {
            rejects_like_pinned(
                |value| {
                    value["github"]["repos"][0]
                        .as_table_mut()
                        .unwrap()
                        .insert("branch_template".into(), template.into());
                },
                "github.repos[0].branch_template",
            );
        }
    }

    fn rejects_model_strings(field: &str, values: &[&str]) {
        for role in [
            "ingest",
            "audit",
            "planner",
            "maker",
            "reviewer",
            "rebaser",
            "comment_classifier",
            "writer",
            "postmortem",
        ] {
            for text in values {
                rejects_like_pinned(
                    |value| {
                        let fallback = value["models"]["maker"].clone();
                        let models = value["models"].as_table_mut().unwrap();
                        let ladder = models.entry(role).or_insert(fallback);
                        ladder[0]
                            .as_table_mut()
                            .unwrap()
                            .insert(field.into(), (*text).into());
                    },
                    &format!("models.{role}[0].{field}"),
                );
            }
        }
    }

    #[test]
    fn rejects_model_name_whitespace() {
        rejects_model_strings("name", &[" model", "model ", "\tmodel", "model\n"]);
    }

    #[test]
    fn rejects_model_name_control_characters() {
        rejects_model_strings("name", &["mo\tdel", "mo\ndel", "mo\0del", "mo\u{7f}del"]);
    }

    #[test]
    fn rejects_model_family_whitespace() {
        rejects_model_strings("family", &[" family", "family ", "\tfamily", "family\n"]);
    }

    #[test]
    fn rejects_model_family_control_characters() {
        rejects_model_strings(
            "family",
            &["fa\tmily", "fa\nmily", "fa\0mily", "fa\u{7f}mily"],
        );
    }

    #[test]
    fn rejects_model_effort_whitespace() {
        rejects_model_strings("effort", &[" high", "high ", "\thigh", "high\n"]);
    }

    #[test]
    fn rejects_model_effort_control_characters() {
        rejects_model_strings("effort", &["hi\tgh", "hi\ngh", "hi\0gh", "hi\u{7f}gh"]);
    }

    #[test]
    fn rejects_a_planner_without_a_fallback() {
        rejects_like_pinned(
            |value| {
                value["models"]["planner"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
            },
            "models.planner",
        );
    }

    #[test]
    fn rejects_a_maker_without_a_second_attempt() {
        rejects_like_pinned(
            |value| {
                value["models"]["maker"].as_array_mut().unwrap().truncate(1);
            },
            "models.maker",
        );
    }

    #[test]
    fn rejects_a_maker_without_family_escalation() {
        for family in ["default", "DEFAULT"] {
            rejects_like_pinned(
                |value| {
                    value["models"]["maker"][1]["family"] = family.into();
                },
                "models.maker",
            );
        }
    }

    #[test]
    fn rejects_reviewers_without_a_different_family_for_every_maker() {
        for index in [0, 1] {
            rejects_like_pinned(
                |value| {
                    let mut reviewer = value["models"]["maker"][index].clone();
                    reviewer["family"] = reviewer["family"]
                        .as_str()
                        .unwrap()
                        .to_ascii_uppercase()
                        .into();
                    value["models"]
                        .as_table_mut()
                        .unwrap()
                        .insert("reviewer".into(), toml::Value::Array(vec![reviewer]));
                },
                "models.reviewer",
            );
        }
    }

    #[test]
    fn refuses_an_invalid_tl3_store() {
        let error = Settings::parse(&console(&[]), "[store]\nport = \"x\"\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("tl3_config"), "{error}");
    }

    #[test]
    fn load_checks_the_password_file_without_reading_it() {
        let directory = tempfile::tempdir().unwrap();
        let tl3 = directory.path().join("tl3.toml");
        fs::write(&tl3, TL3).unwrap();
        let password = directory.path().join("password");
        fs::write(&password, "secret\n").unwrap();
        let path = directory.path().join("console.toml");
        let text = console(&[
            (
                "password_file",
                &format!("{:?}", password.display().to_string()),
            ),
            ("tl3_config", &format!("{:?}", tl3.display().to_string())),
        ]);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&password, fs::Permissions::from_mode(0o640)).unwrap();
        let error = Settings::load(&path).unwrap_err().to_string();
        assert!(error.contains("password_file"), "{error}");
        fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            Settings::load(&path).unwrap().store.password_file,
            Some(password)
        );
    }
}
