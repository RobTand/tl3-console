//! The web app's end-to-end fixture: an embedded Postgres, a store seeded by
//! the simulated controller, and the console serving built web assets.
//!
//! Prints `{"url": ...}` once serving. Each `advance` line on stdin drives the
//! controller until request #42 changes column and prints `{"column": ...}`.
//! Each `apply` line applies the console commands filed so far, as tl3d does
//! when the console wakes it, and prints `{"applied": true}`. Closing stdin
//! stops Postgres and exits.
//!
//! Scripted workers give two requests a story for the case file: the kernel
//! cache request's maker is blocked twice, Rob clears the incident, and its
//! first review asks for changes; the FP8 request's planner can't meet it.
//! Request #43's first run fails for good, so it waits in Needs Rob with a
//! question TL3 can decide alone.

#[path = "../tests/support/mod.rs"]
mod support;

use serde_json::json;
use std::{collections::BTreeMap, io::BufRead, net::TcpListener, path::PathBuf};
use tl3_controller::{
    Bootstrap, Controller, LabelledIssue, Repository, adapters,
    board::{self, Column},
    executor::{Completion, Delivery, Executor, Observation, Output, Reconciliation, Upstream},
};
use tl3_kernel::{Command, EarlyEnd, EffectKind, Event, Fact, NodeId, Reply, Role};
use tl3_store::{Reader, Store};

const REPOS: [&str; 4] = ["tl3-sandbox", "prismabuild", "prismaquant", "tessera"];

/// Seeded requests and how far each is driven before the next arrives.
const SEED: [(&str, Column); 8] = [
    (
        "Shard the GPU benchmark queue by device class",
        Column::Done,
    ),
    ("Retry Hindsight writes after a 429", Column::Done),
    (
        "Quantize attention layers to FP8 with calibration",
        Column::NeedsRob,
    ),
    ("Fix tile seams at zoom level 14", Column::Reviewing),
    ("Document the merge-queue batching rules", Column::Checking),
    (
        "Cache compiled kernels between campaign rows",
        Column::Making,
    ),
    (
        "Remove the stale celestia worker from the fleet file",
        Column::Planning,
    ),
    (
        "Report per-shard wall time in the CI summary",
        Column::Intake,
    ),
];

/// The seeded requests the scripted workers tell a story for, by node.
const NEEDS_ROB: u64 = 2;
const FIX_ROUND: u64 = 5;
/// The issue whose effect-failed question Rob can leave to TL3.
const DEFERRABLE: u64 = 43;
/// Requests the reader ends early: a duplicate, which is cancelled, and work
/// that already exists, which is done.
const DUPLICATE: u64 = 44;
const ALREADY_DONE: u64 = 45;
/// A request Rob stops while the reader still works on it.
const STOPPED: u64 = 46;

/// The simulated workers, with scripted replies for two seeded requests.
struct Workers {
    fake: adapters::workers::Fake,
    runs: BTreeMap<(u64, &'static str), u32>,
}

impl Executor for Workers {
    fn execute(&mut self, delivery: &Delivery) -> tl3_controller::Result<Completion> {
        if delivery.issue == DEFERRABLE {
            return Ok(Completion::permanent("fixture"));
        }
        let Output::Effect {
            kind: EffectKind::StartRun(role),
            node,
            ..
        } = delivery.output
        else {
            return self.fake.execute(delivery);
        };
        let generation = delivery.generation;
        let early_end = match delivery.issue {
            DUPLICATE => Some(EarlyEnd::Duplicate),
            ALREADY_DONE => Some(EarlyEnd::AlreadyDone),
            _ => None,
        };
        if let (Role::Ingest, Some(early_end)) = (role, early_end) {
            return Ok(Completion::confirmed(vec![Observation::Kernel(
                Event::Reply {
                    node,
                    generation,
                    reply: Reply::IngestEarlyEnd(early_end),
                },
            )]));
        }
        let name = match role {
            Role::Make => "maker",
            Role::Review => "reviewer",
            Role::Plan => "planner",
            _ => return self.fake.execute(delivery),
        };
        let run = self.runs.entry((node.0, name)).or_default();
        *run += 1;
        let observation = match (node.0, role, *run) {
            (FIX_ROUND, Role::Make, 1 | 2) => Observation::WorkerReply {
                node,
                generation,
                role: "maker".into(),
                reply: json!({"kind": "blocked",
                    "evidence": {"tool_error": "tool-error-1", "tool": "write", "class": "EROFS"},
                    "report": "The kernel cache directory is mounted read-only, so the cache index could not be written."}),
                model: Some("default-model".into()),
            },
            (FIX_ROUND, Role::Review, 1) => Observation::Kernel(Event::Reply {
                node,
                generation,
                reply: Reply::ChangesRequested,
            }),
            (NEEDS_ROB, Role::Plan, _) => Observation::Kernel(Event::Reply {
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

fn main() {
    let mut args = std::env::args().skip(1);
    let assets = match (args.next().as_deref(), args.next()) {
        (Some("--assets"), Some(path)) => PathBuf::from(path),
        _ => panic!("usage: console_fixture --assets DIR"),
    };
    let mut database = support::Database::start();
    let settings = database.create("console_fixture");
    let records = tempfile::tempdir().unwrap();
    let mut controller = Controller::open(
        Store::open(&settings).unwrap(),
        {
            let mut registry = adapters::simulation(records.path()).unwrap();
            registry.register(
                Upstream::Workers,
                Workers {
                    fake: adapters::workers::Fake::new(records.path()),
                    runs: BTreeMap::new(),
                },
            );
            registry
        },
        Bootstrap {
            operations: None,
            repos: REPOS
                .iter()
                .map(|name| Repository {
                    owner: "RobTand".into(),
                    name: (*name).into(),
                    installation: 1,
                })
                .collect(),
            rob: 24923402,
            slots: 2,
            pool: 2,
            fail_fast: false,
        },
    )
    .unwrap();
    for (index, (title, target)) in SEED.into_iter().enumerate() {
        let (repo, number) = (REPOS[index % REPOS.len()], 100 + index as u64);
        controller.intake(issue(repo, number, title)).unwrap();
        loop {
            let node = controller.world().unwrap().nodes[index];
            if node.waiting_incident {
                // Rob clears the incident the second blocked maker run raised.
                controller
                    .observe(
                        &format!("fixture-clear-{index}"),
                        Observation::Kernel(Event::Fact {
                            node: NodeId(index as u64),
                            fact: Fact::IncidentCleared { by_rob: true },
                        }),
                    )
                    .unwrap();
            }
            let done = if index as u64 == FIX_ROUND {
                node.round >= 1 && Column::of(node.phase) == Column::Making
            } else {
                column(&settings, repo, number) >= target
            };
            if done || !controller.drive_once().unwrap() {
                break;
            }
        }
    }
    for (node, command) in [(NodeId(5), Command::Pause), (NodeId(4), Command::Stop)] {
        let event = Event::Command {
            node,
            id: 1,
            command,
        };
        controller
            .observe(&format!("fixture-{}", node.0), Observation::Kernel(event))
            .unwrap();
        controller.drive_once().unwrap();
    }
    controller
        .intake(issue(
            "tl3-sandbox",
            DEFERRABLE,
            "Mirror the wheel cache to the Sparks",
        ))
        .unwrap();
    while column(&settings, "tl3-sandbox", DEFERRABLE) != Column::NeedsRob
        && controller.drive_once().unwrap()
    {}
    for (number, title, ends_in) in [
        (
            DUPLICATE,
            "Add the console's board to the status page",
            Column::Cancelled,
        ),
        (
            ALREADY_DONE,
            "Show merge counts on the flow strip",
            Column::Done,
        ),
    ] {
        controller
            .intake(issue("tl3-sandbox", number, title))
            .unwrap();
        while column(&settings, "tl3-sandbox", number) != ends_in
            && controller.drive_once().unwrap()
        {}
    }
    controller
        .intake(issue(
            "tl3-sandbox",
            STOPPED,
            "Rename the kernel cache index",
        ))
        .unwrap();
    let stopped = controller.world().unwrap().nodes.len() as u64 - 1;
    controller
        .observe(
            "fixture-stopped",
            Observation::Kernel(Event::Command {
                node: NodeId(stopped),
                id: 1,
                command: Command::Stop,
            }),
        )
        .unwrap();
    while !controller.world().unwrap().nodes[stopped as usize]
        .phase
        .is_closed()
        && controller.drive_once().unwrap()
    {}
    controller
        .intake(issue(
            "tl3-sandbox",
            42,
            "Add a live board to the operator console",
        ))
        .unwrap();
    let config =
        tl3_config::Config::parse(include_str!("../tests/fixtures/tl3-example.toml")).unwrap();
    let repos: Vec<_> = REPOS
        .iter()
        .map(|name| tl3_config::Repository {
            name: (*name).into(),
            ..config.github.repos[0].clone()
        })
        .collect();
    seed_prompts(&settings);
    seed_postmortems(&settings);
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets,
            links: tl3_console::case_file::Links::new(
                Some("http://192.168.1.107:5033/#/experiments/2/traces"),
                &repos,
            )
            .unwrap(),
            models: Some(config.models),
            public_hosts: vec![],
            hidden: Default::default(),
        },
    )
    .unwrap();
    println!(
        "{}",
        json!({"url": format!("http://{}", console.address())})
    );
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        if line.trim() == "finish-postmortem" {
            finish_postmortem(&settings);
            println!("{}", json!({ "finished": true }));
            continue;
        }
        if line.trim() == "apply" {
            tl3_controller::ingress::drain(&mut controller).unwrap();
            println!("{}", json!({ "applied": true }));
            continue;
        }
        if line.trim() != "advance" {
            continue;
        }
        let before = column(&settings, "tl3-sandbox", 42);
        let mut after = before;
        while after == before && controller.drive_once().unwrap() {
            after = column(&settings, "tl3-sandbox", 42);
        }
        println!("{}", json!({ "column": after }));
    }
    database.stop();
}

/// The built-in prompts, as tl3d seeds them, and a reviewer prompt Rob edited
/// before a deploy shipped a new built-in one.
fn seed_prompts(settings: &tl3_config::Store) {
    let mut store = Store::open(settings).unwrap();
    tl3_controller::prompts::seed(&mut store).unwrap();
    let mut prompts = tl3_store::Prompts::open(settings).unwrap();
    let reviewer = prompts.role("reviewer").unwrap().unwrap();
    let shipped = reviewer.active.body.clone();
    prompts
        .save(
            "reviewer",
            &format!("{shipped}\nBefore anything else, check that each new test fails without the change.\n"),
            "Ask reviewers to check that new tests fail first.",
            "Rob",
            reviewer.active.id,
        )
        .unwrap()
        .unwrap();
    store
        .seed_prompts(&[(
            "reviewer",
            &format!("{shipped}\nName the file and line of every finding.\n"),
        )])
        .unwrap();
}

fn issue(repo: &str, number: u64, title: &str) -> LabelledIssue {
    LabelledIssue {
        delivery: format!("fixture-{repo}-{number}"),
        owner: "RobTand".into(),
        repo: repo.into(),
        installation: 1,
        number,
        title: title.into(),
        actor: 24923402,
        label: "tl3".into(),
        labels: vec![],
    }
}

/// A request's column, read the way the console reads it.
fn column(settings: &tl3_config::Store, repo: &str, issue: u64) -> Column {
    let mut reader = Reader::open(settings).unwrap();
    let mut tx = reader.transaction().unwrap();
    let board = board::read(&mut tx, 0, None).unwrap();
    tx.commit().unwrap();
    let repo = format!("RobTand/{repo}");
    board
        .cards
        .iter()
        .find(|card| card.issue == issue && card.repo == repo)
        .map(|card| card.column)
        .unwrap()
}

fn postmortem_client(settings: &tl3_config::Store) -> postgres::Client {
    postgres::Config::new()
        .host(&settings.host)
        .port(settings.port)
        .user(&settings.user)
        .dbname(&settings.database)
        .password("tl3-disposable-test-password")
        .connect(postgres::NoTls)
        .unwrap()
}

fn seed_postmortems(settings: &tl3_config::Store) {
    postmortem_client(settings).batch_execute(r#"
        INSERT INTO tl3_postmortems(node_id, trigger, reason, repo, issue, title, revision, owed_at, status, cause, summary, timeline, process_failures, notes, started_at, finished_at)
        SELECT '0', 'rob:fixture', 'rob_asked', 'RobTand/tl3-sandbox', 100, 'Shard the GPU benchmark queue by device class', max(revision), clock_timestamp() - interval '15 minutes', 'done', 'tl3',
            'The queue accepted two device classes in the same batch. The second attempt separated them before the change merged.',
            '[{"time":"09:14 UTC","event":"First run submitted a mixed batch"},{"time":"09:18 UTC","event":"Review caught the device mismatch"},{"time":"09:26 UTC","event":"The revised queue merged"}]',
            '[{"phase":"making","failure":"The admission check did not compare device classes","evidence":"Mixed classes in batch 17"}]',
            'Keep the device constraint in the admission check.', clock_timestamp() - interval '14 minutes', clock_timestamp() - interval '12 minutes' FROM tl3_events;
        INSERT INTO tl3_postmortems(node_id, trigger, reason, repo, issue, title, revision, owed_at, status, failed_at, failure)
        SELECT '1', 'rob:fixture', 'rob_asked', 'RobTand/prismabuild', 101, 'Retry Hindsight writes after a 429', max(revision), clock_timestamp() - interval '10 minutes', 'failed', clock_timestamp() - interval '9 minutes', 'The judge model call was interrupted.' FROM tl3_events;
        INSERT INTO tl3_postmortems(node_id, trigger, reason, repo, issue, title, revision, owed_at, status, cause, summary, started_at, finished_at)
        SELECT node_id, 'rob:fixture-previous', reason, repo, issue, title, revision, owed_at - interval '1 minute', status, cause, summary, started_at, finished_at
        FROM tl3_postmortems WHERE node_id = '0' AND trigger = 'rob:fixture';
        INSERT INTO tl3_postmortem_recommendations(postmortem_id, fingerprint, rank, phase, kind, target, operation, before, after, evidence, effect)
        SELECT id, 'rec-0123456789abcdef', 1, 'making', 'prompt', 'Check admission constraints before submitting work', 'add', '',
            'Read the worker capacity and device requirements before submitting a batch.', 'Batch 17 combined two device classes; review caught the mismatch.', 'Catch incompatible work before it reaches the queue.'
        FROM tl3_postmortems WHERE trigger IN ('rob:fixture', 'rob:fixture-previous');
    "#).unwrap();
}

fn finish_postmortem(settings: &tl3_config::Store) {
    postmortem_client(settings).batch_execute("UPDATE tl3_postmortems SET status = 'done', started_at = clock_timestamp(), finished_at = clock_timestamp(), summary = 'Calibration needs representative input samples.', cause = 'tl3' WHERE node_id = '2'").unwrap();
}
