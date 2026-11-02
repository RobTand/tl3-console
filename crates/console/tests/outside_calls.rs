//! The console never calls GitHub, MLflow, Hindsight or any other outside
//! service. It has no HTTP client of its own, and its code touches none of
//! TL3's clients for those services. The one TL3 adapter function it calls,
//! `adapters::github::writer_input`, builds text from a stored delivery and
//! sends nothing.

use std::{fs, path::Path, process::Command};

/// HTTP clients, and TL3 crates whose job is talking to an outside service.
const CLIENT_CRATES: &[&str] = &[
    "reqwest",
    "ureq",
    "hyper",
    "hyper-util",
    "isahc",
    "surf",
    "attohttpc",
    "curl",
    "octocrab",
    "tl3-github",
    "tl3-mergify",
    "tl3-prismabuild",
    "tl3-worktrees",
];

/// Code that reaches an outside service: clients, sockets, and TL3's adapters,
/// memory (Hindsight) and controller, which owns the adapters.
const OUTSIDE_CODE: &[&str] = &[
    "reqwest",
    "ureq",
    "hyper::",
    "tcpstream",
    "udpsocket",
    "tl3_github",
    "octocrab",
    "adapters::github",
    "adapters::mergify",
    "adapters::prismabuild",
    "adapters::workers",
    "adapters::worktrees",
    "adapters::simulation",
    "controller::open",
    "memory::",
    "hindsight",
    "mlflow",
];

const ALLOWED: &[&str] = &["adapters::github::writer_input"];

/// The console crate's direct dependencies, as Cargo resolves them.
fn direct_dependencies(manifest: &Path) -> Vec<String> {
    if let Some(cargo) = option_env!("CARGO") {
        let output = Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .current_dir(manifest)
            .output()
            .expect("cargo metadata runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let console = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "tl3-console")
            .expect("tl3-console in cargo metadata");
        return console["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|dependency| dependency["kind"].is_null())
            .map(|dependency| dependency["name"].as_str().unwrap().to_string())
            .collect();
    }
    // Without Cargo (a Bazel test), read the lockfile's resolved list.
    let lock = fs::read_to_string(manifest.join("../../Cargo.lock")).unwrap();
    let entry = lock
        .split("[[package]]")
        .find(|entry| entry.contains("\nname = \"tl3-console\"\n"))
        .expect("tl3-console in Cargo.lock");
    entry
        .split("dependencies = [")
        .nth(1)
        .unwrap_or_default()
        .split(']')
        .next()
        .unwrap()
        .split(',')
        .filter_map(|item| item.trim().trim_matches('"').split(' ').next())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Lines of code, without comments, that reach an outside service.
fn outside_references(source: &str) -> Vec<String> {
    source
        .lines()
        .map(|line| {
            line.split("//")
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        })
        .filter(|code| {
            let mut code = code.clone();
            for allowed in ALLOWED {
                code = code.replace(allowed, "");
            }
            OUTSIDE_CODE.iter().any(|token| code.contains(token))
        })
        .collect()
}

fn sources(directory: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn the_console_has_no_outside_client_dependency() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dependencies = direct_dependencies(manifest);
    assert!(
        dependencies.iter().any(|name| name == "tl3-controller"),
        "{dependencies:?}"
    );
    let clients: Vec<_> = dependencies
        .iter()
        .filter(|name| CLIENT_CRATES.contains(&name.as_str()))
        .collect();
    assert!(
        clients.is_empty(),
        "the console depends on outside clients: {clients:?}"
    );
}

#[test]
fn console_code_reaches_no_outside_service() {
    let mut found = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(found.len() > 5, "the console's sources were read");
    let reaching: Vec<_> = found
        .iter()
        .flat_map(|(path, source)| {
            outside_references(source)
                .into_iter()
                .map(move |line| format!("{path}: {}", line.trim()))
        })
        .collect();
    assert!(reaching.is_empty(), "{reaching:#?}");
}

#[test]
fn the_checks_catch_outside_references() {
    for planted in [
        "let client = reqwest::blocking::Client::new();",
        "use tl3_controller::adapters::github::Adapter;",
        "let memory = tl3_controller::memory::Hindsight::new();",
        "std::net::TcpStream::connect(mlflow)?;",
        "Controller::open(store, registry, bootstrap)?;",
    ] {
        assert_eq!(outside_references(planted).len(), 1, "{planted}");
    }
    for allowed in [
        "tl3_controller::adapters::github::writer_input(&delivery, notice, title, title)",
        "// The console never calls MLflow or Hindsight.",
    ] {
        assert!(outside_references(allowed).is_empty(), "{allowed}");
    }
}
