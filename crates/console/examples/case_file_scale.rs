//! Measures one request's case file in a store of about 2,000 requests and
//! 200,000 events, against a budget for dl380g10 in a release build: a case
//! file within 25 ms. The drawer reads one on open and again on every change
//! to its card.
//!
//! It drives one request to a merge with the simulated controller, copies its
//! node, binding and effects to fill the store, and adds filler events and
//! incidents. Run it through PrismaBuild:
//! `cargo run --release -p tl3-console --example case_file_scale`.
//! It exits 1 when the median exceeds its budget.

#[path = "../tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};
use tl3_console::case_file::{self, Links};
use tl3_controller::{Bootstrap, Controller, LabelledIssue, Repository, adapters};
use tl3_store::{Commit, Reader, RowChange, Store};

const REQUESTS: usize = 2_000;
const EVENTS: i64 = 200_000;
const BUDGET: Duration = Duration::from_millis(25);
const ROUNDS: usize = 9;

fn main() {
    let mut database = support::Database::start();
    let settings = database.create("case_file_scale");
    let records = tempfile::tempdir().unwrap();
    let repos = ["tl3-sandbox", "prismabuild", "prismaquant", "tessera"];
    let mut controller = Controller::open(
        Store::open(&settings).unwrap(),
        adapters::simulation(records.path()).unwrap(),
        Bootstrap {
            operations: None,
            repos: repos
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
        },
    )
    .unwrap();
    controller
        .intake(LabelledIssue {
            delivery: "scale-0".into(),
            owner: "RobTand".into(),
            repo: repos[0].into(),
            installation: 1,
            number: 1,
            title: "Seed request".into(),
            actor: 24923402,
            label: "tl3".into(),
        })
        .unwrap();
    while controller.drive_once().unwrap() {}
    drop(controller);
    let mut store = Store::open(&settings).unwrap();
    let snapshot = store.load().unwrap();
    let row = |table: &str| {
        snapshot
            .rows
            .iter()
            .find(|row| row.table == table && row.id == "0")
            .unwrap()
            .value
            .clone()
    };
    let (node, request) = (row("nodes"), row("requests"));
    let mut seed = Commit::new("scale-seed", serde_json::json!(REQUESTS), &snapshot);
    for index in 1..REQUESTS {
        let mut binding = request.clone();
        binding["issue"] = serde_json::json!(index + 1);
        binding["repo"]["name"] = serde_json::json!(repos[index % repos.len()]);
        binding["title"] = serde_json::json!(format!("Scaled request {index}"));
        let incident = serde_json::json!({
            "id": format!("incident-{index}"), "node": index, "open": false,
            "needs_rob": false, "blocking": false, "summary": "A filler incident.",
            "signature": {"upstream": "GitHub", "class": "http_502"},
        });
        for (table, value) in [
            ("nodes", node.clone()),
            ("requests", binding),
            ("incidents", incident),
        ] {
            seed.rows.push(RowChange::Put {
                table: table.into(),
                id: match table {
                    "incidents" => format!("incident-{index}"),
                    _ => index.to_string(),
                },
                value,
            });
        }
    }
    store.commit(&seed).unwrap();
    drop(store);
    let mut client = postgres::Config::new()
        .host(&settings.host)
        .port(settings.port)
        .user(&settings.user)
        .dbname(&settings.database)
        .password("tl3-disposable-test-password")
        .connect(postgres::NoTls)
        .unwrap();
    // Each copy gets the seed request's effects under its own keys.
    let effects = client
        .execute(
            "INSERT INTO tl3_effects (effect_key, epoch, kind, payload, state, created_revision, updated_revision)
             SELECT 'effect:' || n || ':' || split_part(e.effect_key, ':', 3), e.epoch, e.kind,
                    jsonb_set(e.payload, '{output,Effect,node}', to_jsonb(n)), e.state,
                    e.created_revision, e.updated_revision
             FROM tl3_effects e, generate_series(1, $1::int - 1) n
             WHERE e.effect_key LIKE 'effect:0:%'",
            &[&(REQUESTS as i32)],
        )
        .unwrap();
    client
        .batch_execute(&format!(
            "INSERT INTO tl3_events (event_key, revision, epoch, payload, result)
             SELECT 'scale-filler-' || r, r, (SELECT epoch FROM tl3_meta), jsonb_build_object('filler', r), '{{}}'::jsonb
             FROM generate_series((SELECT revision FROM tl3_meta) + 1, (SELECT revision FROM tl3_meta) + {EVENTS}) r;
             UPDATE tl3_meta SET revision = revision + {EVENTS};
             ANALYZE;"
        ))
        .unwrap();
    let counts = client
        .query_one(
            "SELECT (SELECT count(*) FROM tl3_events), (SELECT count(*) FROM tl3_effects),
                    (SELECT count(*) FROM tl3_world WHERE table_name = 'nodes')",
            &[],
        )
        .unwrap();
    let (events, all_effects, nodes): (i64, i64, i64) =
        (counts.get(0), counts.get(1), counts.get(2));
    println!("store: {nodes} requests, {events} events, {all_effects} effects ({effects} copied)");
    let links = Links::new(
        Some("http://192.168.1.107:5033/#/experiments/2/traces"),
        &[],
    )
    .unwrap();
    let mut reader = Reader::open(&settings).unwrap();
    let mut timeline = 0;
    let median = median(|| {
        let mut tx = reader.transaction().unwrap();
        let file = case_file::read(&mut tx, 0, 0, None, &links)
            .unwrap()
            .unwrap();
        tx.commit().unwrap();
        timeline = file.timeline.len();
        assert!(!file.runs.is_empty());
    });
    println!(
        "case file at {REQUESTS} requests and {events} events: median {median:?} (budget {BUDGET:?}), {timeline} timeline entries"
    );
    database.stop();
    if median > BUDGET {
        eprintln!("case_file_scale: over budget");
        std::process::exit(1);
    }
}

fn median(mut run: impl FnMut()) -> Duration {
    let mut times: Vec<_> = (0..ROUNDS)
        .map(|_| {
            let started = Instant::now();
            run();
            started.elapsed()
        })
        .collect();
    times.sort();
    times[ROUNDS / 2]
}
