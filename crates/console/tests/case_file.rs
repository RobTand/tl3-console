//! The case file (#329) against an embedded Postgres, from requests the
//! simulated controller drove: one merged, one with a fix round and an
//! incident, one split, and one awaiting Rob.

mod support;

use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    rc::Rc,
    time::Duration,
};
use tl3_console::case_file::Links;
use tl3_controller::{
    Bootstrap, Controller, LabelledIssue, Repository, adapters,
    executor::{Completion, Delivery, Executor, Observation, Output, Reconciliation, Upstream},
};
use tl3_kernel::{Command, EffectKind, Event, Fact, NodeId, Reply, Role};
use tl3_store::Store;

const MERGED: u64 = 0;
const FIX_ROUND: u64 = 1;
const SPLIT: u64 = 2;
const AWAITING_ROB: u64 = 3;

fn main() {
    let mut database = support::Database::start();
    let settings = database.create("case_file");
    let records = tempfile::tempdir().unwrap();
    let script = Rc::new(RefCell::new(Script::default()));
    let mut controller = open(&settings, records.path(), &script);
    // Stands in for MLflow's UI: building case files must never connect to it.
    let mlflow = TcpListener::bind("127.0.0.1:0").unwrap();
    mlflow.set_nonblocking(true).unwrap();
    let traces = format!(
        "http://{}/#/experiments/2/traces",
        mlflow.local_addr().unwrap()
    );
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "<!doctype html>").unwrap();
    let config = tl3_config::Config::parse(include_str!("fixtures/tl3-example.toml")).unwrap();
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.path().into(),
            models: Some(config.models),
            links: Links::new(Some(&traces), &[repository()]).unwrap(),
            public_hosts: vec![],
            hidden: Default::default(),
        },
    )
    .unwrap();
    let address = console.address();

    let files = Files { address };
    drive(&mut controller, MERGED, |n| n.phase.is_closed());
    merged_request(&files, &traces);
    println!("test merged_request ... ok");

    drive(&mut controller, FIX_ROUND, |n| n.waiting_incident);
    // Rob clears the incident the second blocked run raised.
    controller
        .observe(
            "case-file-clear",
            Observation::Kernel(Event::Fact {
                node: NodeId(FIX_ROUND),
                fact: Fact::IncidentCleared { by_rob: true },
            }),
        )
        .unwrap();
    drive(&mut controller, FIX_ROUND, |n| n.phase.is_closed());
    fix_round_and_incident(&files);
    println!("test fix_round_and_incident ... ok");

    drive(&mut controller, SPLIT, |n| n.question.is_some());
    split_request(&files);
    println!("test split_request ... ok");

    drive(&mut controller, AWAITING_ROB, |n| n.question.is_some());
    awaiting_rob(&files);
    println!("test awaiting_rob ... ok");

    // Rob stops the request that waits on him. The issue stays open, so it
    // waits in the backlog, not in Done.
    controller
        .observe(
            "case-file-stop",
            Observation::Kernel(Event::Command {
                node: NodeId(AWAITING_ROB),
                id: 1,
                command: Command::Stop,
            }),
        )
        .unwrap();
    drive(&mut controller, AWAITING_ROB, |n| n.phase.is_closed());
    stopped_request(&files);
    println!("test stopped_request ... ok");

    assert_eq!(
        mlflow.accept().map(|_| ()).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "building case files never connects to MLflow"
    );
    assert_eq!(get(address, "/api/requests/99").0, 404);
    assert_eq!(get(address, "/api/requests/not-a-number").0, 400);
    println!("test case_files_never_reach_mlflow ... ok");
    case_file_on_a_store_ahead_of_the_pin(&settings, assets.path());
    println!("test case_file_on_a_store_ahead_of_the_pin ... ok");
    drop(controller);
    database.stop();
    println!("7 case file scenarios passed; no skips");
}

fn repository() -> tl3_config::Repository {
    let config = tl3_config::Config::parse(include_str!("fixtures/tl3-example.toml")).unwrap();
    config.github.repos[0].clone()
}

fn open(settings: &tl3_config::Store, records: &Path, script: &Rc<RefCell<Script>>) -> Controller {
    let mut registry = adapters::simulation(records).unwrap();
    registry.register(
        Upstream::Workers,
        Workers {
            fake: adapters::workers::Fake::new(records),
            script: script.clone(),
        },
    );
    let mut controller = Controller::open(
        Store::open(settings).unwrap(),
        registry,
        Bootstrap {
            operations: None,
            repos: vec![Repository {
                owner: "RobTand".into(),
                name: "tl3-sandbox".into(),
                installation: 1,
            }],
            rob: 24923402,
            slots: 2,
            pool: 2,
        },
    )
    .unwrap();
    for (number, title) in [
        (101, "Retry Hindsight writes after a 429"),
        (102, "Cache compiled kernels between campaign rows"),
        (103, "Split the fleet file by host class"),
        (104, "Quantize attention layers to FP8"),
    ] {
        controller
            .intake(LabelledIssue {
                delivery: format!("case-file-{number}"),
                owner: "RobTand".into(),
                repo: "tl3-sandbox".into(),
                installation: 1,
                number,
                title: title.into(),
                actor: 24923402,
                label: "tl3".into(),
                labels: vec![],
            })
            .unwrap();
    }
    controller
}

/// Drive the controller until request `node` meets `done`.
fn drive(controller: &mut Controller, node: u64, done: impl Fn(&tl3_kernel::Node) -> bool) {
    for _ in 0..500 {
        if done(&controller.world().unwrap().nodes[node as usize]) {
            return;
        }
        if !controller.drive_once().unwrap() {
            let world = controller.world().unwrap();
            let nodes: Vec<_> = world
                .nodes
                .iter()
                .map(|n| format!("{:?} waiting on {:?}", n.phase, n.waiting_on()))
                .collect();
            panic!("request {node} stalled: {nodes:#?}");
        }
    }
    panic!("request {node} never got there");
}

/// How many times each request's maker and reviewer ran, to script replies.
#[derive(Default)]
struct Script {
    runs: BTreeMap<(u64, &'static str), u32>,
}

/// The simulated workers, with scripted replies for the scenarios.
struct Workers {
    fake: adapters::workers::Fake,
    script: Rc<RefCell<Script>>,
}

impl Executor for Workers {
    fn execute(&mut self, delivery: &Delivery) -> tl3_controller::Result<Completion> {
        let Output::Effect {
            kind: EffectKind::StartRun(role),
            node,
            ..
        } = delivery.output
        else {
            return self.fake.execute(delivery);
        };
        let generation = delivery.generation;
        let run = {
            let mut script = self.script.borrow_mut();
            let count = script.runs.entry((node.0, role_name(role))).or_default();
            *count += 1;
            *count
        };
        let reply = |role: &str, reply: Value| Observation::WorkerReply {
            node,
            generation,
            role: role.into(),
            reply,
            model: Some("default-model".into()),
        };
        let observation = match (node.0, role, run) {
            (FIX_ROUND, Role::Make, 1 | 2) => reply(
                "maker",
                json!({"kind": "blocked",
                    "evidence": {"tool_error": "tool-error-1", "tool": "write", "class": "EROFS"},
                    "report": "The worktree is mounted read-only, so no file could be written."}),
            ),
            (FIX_ROUND, Role::Review, 1) => Observation::Kernel(Event::Reply {
                node,
                generation,
                reply: Reply::ChangesRequested,
            }),
            (SPLIT, Role::Plan, _) => reply(
                "planner",
                json!({"kind": "split", "reason": "does_not_fit", "violations": [],
                    "pieces": [
                        {"brief": "Move GPU hosts into their own fleet file", "goal_ids": [0], "files": ["fleet/gpu.toml"]},
                        {"brief": "Move CPU hosts into their own fleet file", "goal_ids": [0], "files": ["fleet/cpu.toml"]},
                    ],
                    "forecast": {"tokenizer": "o200k_base", "brief_tokens": 100,
                        "file_tokens": {"fleet/gpu.toml": 60000, "fleet/cpu.toml": 60000},
                        "overhead_tokens": 0, "tokens": 120100, "fit_tokens": 90000,
                        "context_tokens": 200000, "fits": false}}),
            ),
            (AWAITING_ROB, Role::Plan, _) => Observation::Kernel(Event::Reply {
                node,
                generation,
                reply: Reply::PlanCantMeet,
            }),
            _ => return self.fake.execute(delivery),
        };
        Ok(Completion::confirmed(vec![observation]))
    }

    fn reconcile(&mut self, delivery: &Delivery) -> tl3_controller::Result<Reconciliation> {
        self.fake.reconcile(delivery)
    }
}

struct Files {
    address: SocketAddr,
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Ingest => "ingest",
        Role::Audit => "audit",
        Role::Plan => "planner",
        Role::Make => "maker",
        Role::Review => "reviewer",
    }
}

/// A TL3 deploy that migrates past the pin must not take the drawer down with
/// it: the case-file sessions accept the store like the board's, and say why.
fn case_file_on_a_store_ahead_of_the_pin(settings: &tl3_config::Store, assets: &Path) {
    let mut admin = postgres::Config::new()
        .host(&settings.host)
        .port(settings.port)
        .user(&settings.user)
        .dbname(&settings.database)
        .password("tl3-disposable-test-password")
        .connect(postgres::NoTls)
        .unwrap();
    let ahead: i32 = admin
        .query_one(
            "INSERT INTO tl3_schema_migrations \
             SELECT MAX(version) + 1, 'SELECT 1' FROM tl3_schema_migrations RETURNING version",
            &[],
        )
        .unwrap()
        .get(0);
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.into(),
            models: None,
            links: Default::default(),
            public_hosts: vec![],
            hidden: Default::default(),
        },
    )
    .unwrap();
    let log = capture_stderr(|| get(console.address(), &format!("/api/requests/{MERGED}")));
    let (status, body) = log.0;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let warning = log
        .1
        .lines()
        .find(|line| line.contains("case file") && line.contains("beyond"))
        .unwrap_or_else(|| panic!("no case-file warning in:\n{}", log.1));
    assert!(warning.contains(&ahead.to_string()), "{warning}");
    drop(console);
    admin
        .execute(
            "DELETE FROM tl3_schema_migrations WHERE version = $1",
            &[&ahead],
        )
        .unwrap();
}

/// What `run` returns, and what the process wrote to stderr meanwhile.
fn capture_stderr<T>(run: impl FnOnce() -> T) -> (T, String) {
    let mut file = tempfile::tempfile().unwrap();
    let saved = rustix::io::dup(rustix::stdio::stderr()).unwrap();
    rustix::stdio::dup2_stderr(&file).unwrap();
    let result = run();
    rustix::stdio::dup2_stderr(&saved).unwrap();
    let mut text = String::new();
    std::io::Seek::rewind(&mut file).unwrap();
    file.read_to_string(&mut text).unwrap();
    (result, text)
}

fn case_file(files: &Files, id: u64) -> Value {
    let (status, body) = get(files.address, &format!("/api/requests/{id}"));
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

fn strings(values: &Value, field: &str) -> Vec<String> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value[field].as_str().unwrap().to_string())
        .collect()
}

fn link(file: &Value, kind: &str) -> Option<String> {
    file["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["kind"] == kind)
        .map(|link| {
            assert!(
                !link["label"].as_str().unwrap().is_empty(),
                "every link has an accessible name"
            );
            link["url"].as_str().unwrap().to_string()
        })
}

fn merged_request(files: &Files, traces: &str) {
    let file = case_file(files, MERGED);
    assert_eq!(file["card"]["issue"], 101);
    assert_eq!(file["card"]["ending"], "Merged");
    let timeline = &file["timeline"];
    let columns = strings(timeline, "column");
    assert_eq!(columns.first().map(String::as_str), Some("Intake"));
    assert_eq!(columns.last().map(String::as_str), Some("Done"));
    for stage in ["Planning", "Making", "Checking", "Reviewing", "Merging"] {
        assert!(columns.iter().any(|column| column == stage), "{columns:?}");
    }
    assert_eq!(
        timeline[0]["cause"], "Rob labelled the issue",
        "{timeline:#}"
    );
    let times: Vec<i64> = timeline
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["at_ms"].as_i64().unwrap())
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] <= pair[1]), "{times:?}");
    assert!(
        timeline
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| !entry["cause"].as_str().unwrap().is_empty())
    );
    let runs = &file["runs"];
    assert_eq!(
        strings(runs, "role"),
        ["Reader", "Auditor", "Planner", "Maker", "Reviewer"]
    );
    for run in runs.as_array().unwrap() {
        assert_eq!(run["status"], "Finished", "{run:#}");
        assert!(run["ended_ms"].as_i64() >= run["started_ms"].as_i64());
        assert_eq!(run["tokens"], Value::Null);
    }
    let maker = &runs[3];
    assert_eq!(maker["outcome"], "Made a change");
    assert_eq!(
        maker["model"],
        Value::Null,
        "the simulated maker reports no model"
    );
    assert_eq!(
        maker["trace_url"],
        format!(
            "{traces}?filter=tag%3A%3A%3D%3A%3A0%3A%3Arequest&filter=tag%3A%3A%3D%3A%3A{}%3A%3Ageneration&filter=tag%3A%3A%3D%3A%3Amaker%3A%3Arole",
            maker["generation"]
        )
    );
    assert_eq!(
        link(&file, "Issue").as_deref(),
        Some("https://github.com/RobTand/tl3-sandbox/issues/101")
    );
    let pull = link(&file, "PullRequest").expect("a pull request link");
    assert!(
        pull.starts_with("https://github.com/RobTand/tl3-sandbox/pull/"),
        "{pull}"
    );
    assert_eq!(
        link(&file, "MergeQueue"),
        Some(format!("{pull}/checks")),
        "Mergify reports the queue on the pull request's checks"
    );
    assert_eq!(
        link(&file, "Branch").as_deref(),
        Some("https://github.com/RobTand/tl3-sandbox/tree/tl3/issue-101-node-0")
    );
    let summary = &file["summary"];
    assert_eq!(summary["ending"], "Merged");
    assert_eq!(summary["outcome"], "Your change was merged.");
    assert_eq!(summary["unmet_goals"], json!([]));
    assert_eq!(file["question"], Value::Null);
    assert_eq!(file["incidents"], json!([]));
    assert_eq!(file["children"], json!([]));
}

fn fix_round_and_incident(files: &Files) {
    let file = case_file(files, FIX_ROUND);
    let card = &file["card"];
    assert_eq!(card["ending"], "Merged");
    assert_eq!(card["round"], 1, "one fix round after the first review");
    let columns = strings(&file["timeline"], "column");
    let reviewed = columns.iter().position(|c| c == "Reviewing").unwrap();
    assert!(
        columns[reviewed..].iter().any(|c| c == "Making"),
        "the fix round goes back to Making: {columns:?}"
    );
    let runs = file["runs"].as_array().unwrap();
    let makers: Vec<_> = runs.iter().filter(|run| run["role"] == "Maker").collect();
    assert_eq!(makers.len(), 4, "two blocked runs, then one per round");
    for blocked in &makers[..2] {
        assert_eq!(blocked["status"], "Failed");
        assert_eq!(blocked["outcome"], "Blocked by the environment");
        assert_eq!(
            blocked["model"], "default-model",
            "the run's reply names its model"
        );
        assert_eq!(
            blocked["effort"], "high",
            "attempt 1 runs the maker ladder's first rung"
        );
    }
    let reviews: Vec<_> = runs
        .iter()
        .filter(|run| run["role"] == "Reviewer")
        .collect();
    assert_eq!(reviews[0]["outcome"], "Changes requested");
    assert_eq!(reviews.last().unwrap()["outcome"], "Approved");
    let incidents = file["incidents"].as_array().unwrap();
    assert_eq!(incidents.len(), 1, "{incidents:#?}");
    let incident = &incidents[0];
    assert_eq!(incident["open"], false, "Rob cleared it");
    assert_eq!(incident["cause"], "Tl3");
    assert_eq!(incident["source"], "Workers");
    assert_eq!(incident["class"], "environment_blocked");
    assert!(
        incident["summary"]
            .as_str()
            .unwrap()
            .contains("tool-error-1"),
        "{incident:#}"
    );
    let blocked = file["blocked"].as_array().unwrap();
    assert_eq!(blocked.len(), 2);
    assert_eq!(
        blocked[0]["evidence"],
        "The maker's write tool recorded tool-error-1 (EROFS)"
    );
    assert_eq!(
        blocked[0]["report"],
        "The worktree is mounted read-only, so no file could be written."
    );
    assert_eq!(blocked[0]["generation"], makers[0]["generation"]);
}

fn split_request(files: &Files) {
    let file = case_file(files, SPLIT);
    assert_eq!(file["card"]["column"], "NeedsRob");
    let children = file["children"].as_array().unwrap();
    assert_eq!(
        strings(&file["children"], "title"),
        [
            "Move GPU hosts into their own fleet file",
            "Move CPU hosts into their own fleet file"
        ]
    );
    assert_eq!(children[0]["stage"], "Proposed");
    assert_eq!(children[0]["id"], Value::Null);
    assert_eq!(children[0]["files"], json!(["fleet/gpu.toml"]));
    let question = &file["question"];
    assert_eq!(question["kind"], "Needs splitting");
    assert_eq!(
        strings(&question["options"], "action"),
        ["Change the request", "Stop"]
    );
    assert_eq!(file["runs"][2]["outcome"], "Proposed a split");
}

fn awaiting_rob(files: &Files) {
    let file = case_file(files, AWAITING_ROB);
    assert_eq!(file["card"]["column"], "NeedsRob");
    assert_eq!(
        strings(&file["timeline"], "column")
            .last()
            .map(String::as_str),
        Some("NeedsRob")
    );
    let question = &file["question"];
    assert_eq!(question["kind"], "Can't meet the request");
    assert_eq!(
        question["obstacle"],
        "I could not finish your request with the approach I tried."
    );
    assert_eq!(
        strings(&question["options"], "action"),
        ["Try again with guidance", "Change the request", "Stop"]
    );
    assert_eq!(question["recommended_option"], 0);
    assert!(
        !question["recommendation_reason"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(question["posted"], true);
    let planners: Vec<_> = file["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|run| run["role"] == "Planner")
        .map(|run| run["outcome"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        planners,
        ["Can't meet the request", "Can't meet the request"]
    );
    assert_eq!(file["summary"], Value::Null);
}

fn stopped_request(files: &Files) {
    let file = case_file(files, AWAITING_ROB);
    let card = &file["card"];
    assert_eq!(card["ending"], "Stopped");
    assert_eq!(card["column"], "Backlog");
    assert_eq!(card["badges"], json!(["Stopped"]));
    let columns = strings(&file["timeline"], "column");
    assert_eq!(
        columns.last().map(String::as_str),
        Some("Backlog"),
        "the closing steps land where the card is: {columns:?}"
    );
    assert!(
        !columns.iter().any(|column| column == "Done"),
        "a stopped request never passes through Done: {columns:?}"
    );
}

fn get(address: SocketAddr, path: &str) -> (u16, Vec<u8>) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reader = BufReader::new(socket);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    let code = status.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        chunked |= line
            .to_ascii_lowercase()
            .starts_with("transfer-encoding: chunked");
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            let mut data = vec![0; size + 2];
            reader.read_exact(&mut data).unwrap();
            if size == 0 {
                break;
            }
            body.extend_from_slice(&data[..size]);
        }
    } else {
        reader.read_to_end(&mut body).unwrap();
    }
    (code, body)
}
