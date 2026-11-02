mod support;

use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::Database;
use tl3_controller::{Bootstrap, Controller, LabelledIssue, Repository, adapters, board};
use tl3_store::{Reader, Store, grant_reader};

const ROLE: &str = "tl3_console_reader";

fn main() {
    let mut database = Database::start();
    let settings = database.create("console");
    let records = tempfile::tempdir().unwrap();
    let mut controller = open(&settings, records.path());
    controller.intake(issue(42)).unwrap();
    let reader = read_only_role(&settings);
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>TL3 console</title>",
    )
    .unwrap();
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: reader.clone(),
            assets: assets.path().into(),
            models: None,
            links: Default::default(),
        },
    )
    .unwrap();
    let address = console.address();

    commit_reaches_open_stream(address, &settings, &mut controller);
    println!("test commit_reaches_open_stream ... ok");
    println!("test worker_progress_reaches_the_stream ... ok");
    last_event_id_gets_a_fresh_consistent_snapshot(address, &settings);
    println!("test last_event_id_gets_a_fresh_consistent_snapshot ... ok");
    serves_board_and_web_app(address, &settings);
    println!("test serves_board_and_web_app ... ok");
    console_reads_through_a_role_that_cannot_write(&settings);
    println!("test console_reads_through_a_role_that_cannot_write ... ok");
    cards_follow_the_quota_fallback(&settings, assets.path());
    println!("test cards_follow_the_quota_fallback ... ok");
    prompts_edit_and_revert_through_the_api(&mut database, assets.path());
    println!("test prompts_edit_and_revert_through_the_api ... ok");
    drop(controller);
    builds_once_the_store_is_migrated(&mut database, assets.path());
    println!("test builds_once_the_store_is_migrated ... ok");
    accepts_an_equal_schema(&mut database);
    println!("test accepts_an_equal_schema ... ok");
    pinned_migrations_match_the_pinned_tl3(&mut database);
    println!("test pinned_migrations_match_the_pinned_tl3 ... ok");
    accepts_a_newer_schema_with_a_warning(&mut database, assets.path());
    println!("test accepts_a_newer_schema_with_a_warning ... ok");
    refuses_an_older_schema(&mut database, assets.path());
    println!("test refuses_an_older_schema ... ok");
    refuses_a_conflicting_schema(&mut database, assets.path());
    println!("test refuses_a_conflicting_schema ... ok");
    streams_again_after_postgres_restarts(address, &mut database, &settings, records.path());
    println!("test streams_again_after_postgres_restarts ... ok");
    database.stop();
    println!("14 console scenarios passed; no skips");
}

fn bootstrap() -> Bootstrap {
    Bootstrap {
        operations: None,
        repos: vec![Repository {
            owner: "RobTand".into(),
            name: "tl3-sandbox".into(),
            installation: 1,
        }],
        rob: 24923402,
        slots: 1,
        pool: 1,
    }
}

fn issue(number: u64) -> LabelledIssue {
    LabelledIssue {
        delivery: format!("console-delivery-{number}"),
        owner: "RobTand".into(),
        repo: "tl3-sandbox".into(),
        installation: 1,
        number,
        title: format!("Console fixture {number}"),
        actor: 24923402,
        label: "tl3".into(),
    }
}

fn open(settings: &tl3_config::Store, records: &Path) -> Controller {
    Controller::open(
        Store::open(settings).unwrap(),
        adapters::simulation(records).unwrap(),
        bootstrap(),
    )
    .unwrap()
}

fn raw(settings: &tl3_config::Store) -> postgres::Client {
    postgres::Config::new()
        .host(&settings.host)
        .port(settings.port)
        .user(&settings.user)
        .dbname(&settings.database)
        .password("tl3-disposable-test-password")
        .connect(postgres::NoTls)
        .unwrap()
}

/// The console runs as a role that may only read, so any write would fail.
fn read_only_role(settings: &tl3_config::Store) -> tl3_config::Store {
    let mut client = raw(settings);
    client
        .batch_execute(&format!(
            "CREATE ROLE {ROLE} LOGIN PASSWORD 'tl3-disposable-test-password';
             GRANT SELECT ON ALL TABLES IN SCHEMA public TO {ROLE};"
        ))
        .unwrap();
    tl3_config::Store {
        user: ROLE.into(),
        ..settings.clone()
    }
}

fn revision(settings: &tl3_config::Store) -> i64 {
    Reader::open(settings)
        .unwrap()
        .load()
        .unwrap()
        .revision
        .get()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn card_column(cards: &Value, issue: u64) -> Option<String> {
    cards
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["issue"] == issue)
        .map(|card| card["column"].as_str().unwrap().to_string())
}

fn commit_reaches_open_stream(
    address: SocketAddr,
    settings: &tl3_config::Store,
    controller: &mut Controller,
) {
    let mut stream = EventStream::open(address, None);
    let first = stream.board_event();
    assert_eq!(first.name, "snapshot");
    assert_eq!(first.data["revision"], revision(settings));
    assert_eq!(
        card_column(&first.data["cards"], 42).as_deref(),
        Some("Intake")
    );
    let mut column = "Intake".to_string();
    let mut seen = first.data["revision"].as_i64().unwrap();
    let mut shown = first.id.clone();
    for _ in 0..100 {
        if column == "Making" {
            break;
        }
        assert!(controller.drive_once().unwrap());
        let committed = revision(settings);
        while seen < committed {
            let event = stream.board_event();
            assert_eq!(event.name, "patch", "{event:?}");
            // Each patch names the board it applies to, so a gap is visible.
            assert_eq!(event.data["base"], shown.as_str(), "{event:?}");
            let patch = &event.data["patch"];
            assert!(event.id.starts_with(&format!("{}.", patch["revision"])));
            seen = patch["revision"].as_i64().unwrap();
            shown = event.id.clone();
            if let Some(moved) = card_column(&patch["upserts"], 42) {
                column = moved;
            }
        }
    }
    assert_eq!(column, "Making", "the card moved live without a reload");
    worker_progress_reaches_the_stream(&mut stream, settings, &shown);
}

/// A worker's progress, with no controller commit, updates the card.
fn worker_progress_reaches_the_stream(
    stream: &mut EventStream,
    settings: &tl3_config::Store,
    shown: &str,
) {
    let mut store = Store::open(settings).unwrap();
    let snapshot = store.load().unwrap();
    store
        .claim_worker("console-progress", "0", snapshot.epoch.get(), 1)
        .unwrap();
    store
        .record_worker_progress("console-progress", 1, 7, "tool_result", None)
        .unwrap();
    let mut base = shown.to_string();
    loop {
        let event = stream.board_event();
        assert_eq!(event.data["base"], base.as_str());
        base = event.id.clone();
        let patch = &event.data["patch"];
        assert_eq!(patch["revision"], snapshot.revision.get());
        let card = patch["upserts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|card| card["issue"] == 42);
        if let Some(card) = card
            && card["last_progress"]
                .as_str()
                .is_some_and(|text| text.starts_with("Tool result received at "))
        {
            break;
        }
    }
}

fn last_event_id_gets_a_fresh_consistent_snapshot(
    address: SocketAddr,
    settings: &tl3_config::Store,
) {
    let mut stream = EventStream::open(address, Some("1.1"));
    let event = stream.board_event();
    assert_eq!(event.name, "snapshot");
    let mut reader = Reader::open(settings).unwrap();
    let mut tx = reader.transaction().unwrap();
    let expected = board::read(&mut tx, now_ms(), None).unwrap();
    tx.commit().unwrap();
    assert_eq!(event.data["revision"], expected.revision);
    assert_eq!(
        event.data["cards"],
        serde_json::to_value(&expected.cards).unwrap()
    );
    assert_eq!(
        event.data["flow"]["columns"],
        serde_json::to_value(&expected.flow.columns).unwrap()
    );
}

fn serves_board_and_web_app(address: SocketAddr, settings: &tl3_config::Store) {
    let (status, headers, body) = get(address, "/api/board");
    assert_eq!(status, 200);
    assert!(headers.contains("content-type: application/json"));
    let board: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(board["revision"], revision(settings));
    assert_eq!(card_column(&board["cards"], 42).as_deref(), Some("Making"));
    for path in ["/", "/board/RobTand/tl3-sandbox"] {
        let (status, headers, body) = get(address, path);
        assert_eq!(status, 200, "{path}");
        assert!(headers.contains("content-type: text/html"), "{path}");
        assert!(headers.contains("content-security-policy: default-src 'self'"));
        assert_eq!(body, b"<!doctype html><title>TL3 console</title>", "{path}");
    }
    assert_eq!(get(address, "/api/unknown").0, 404);
}

fn console_reads_through_a_role_that_cannot_write(settings: &tl3_config::Store) {
    let mut client = raw(settings);
    for table in ["tl3_world", "tl3_events", "tl3_meta", "tl3_node_phases"] {
        for privilege in ["INSERT", "UPDATE", "DELETE", "TRUNCATE"] {
            assert!(
                !client
                    .query_one(
                        "SELECT has_table_privilege($1, $2, $3)",
                        &[&ROLE, &table, &privilege],
                    )
                    .unwrap()
                    .get::<_, bool>(0),
                "{ROLE} must not {privilege} {table}"
            );
        }
    }
    let sessions: i64 = client
        .query_one(
            "SELECT count(*) FROM pg_stat_activity WHERE usename = $1",
            &[&ROLE],
        )
        .unwrap()
        .get(0);
    assert!(
        sessions >= 1,
        "the console's sessions use the read-only role"
    );
}

fn streams_again_after_postgres_restarts(
    address: SocketAddr,
    database: &mut Database,
    settings: &tl3_config::Store,
    records: &Path,
) {
    let mut stream = EventStream::open(address, None);
    assert_eq!(stream.board_event().name, "snapshot");
    database.restart();
    let mut controller = open(settings, records);
    controller.intake(issue(43)).unwrap();
    let committed = revision(settings);
    loop {
        let event = stream.board_event();
        let (board, cards) = if event.name == "snapshot" {
            (&event.data, &event.data["cards"])
        } else {
            (&event.data["patch"], &event.data["patch"]["upserts"])
        };
        if card_column(cards, 43).is_some() {
            assert_eq!(board["revision"], committed);
            break;
        }
    }
    let status = stream
        .last_database
        .expect("the stream reports database state");
    assert_eq!(status["connected"], true);
}

#[derive(Debug)]
struct Event {
    name: String,
    id: String,
    data: Value,
}

/// A browser's view of `/api/stream`: Server-Sent Events over chunked HTTP/1.1.
struct EventStream {
    body: ChunkedBody,
    last_database: Option<Value>,
}

impl EventStream {
    fn open(address: SocketAddr, last_event_id: Option<&str>) -> Self {
        // A hang guard only; events arrive as soon as Postgres commits.
        Self::open_with_guard(address, last_event_id, Duration::from_secs(60))
    }

    fn open_with_guard(address: SocketAddr, last_event_id: Option<&str>, guard: Duration) -> Self {
        let mut socket = TcpStream::connect(address).unwrap();
        socket.set_read_timeout(Some(guard)).unwrap();
        let resume = last_event_id
            .map(|id| format!("Last-Event-ID: {id}\r\n"))
            .unwrap_or_default();
        write!(
            socket,
            "GET /api/stream HTTP/1.1\r\nHost: console\r\nAccept: text/event-stream\r\n{resume}\r\n"
        )
        .unwrap();
        let mut reader = BufReader::new(socket);
        let (status, headers) = response_head(&mut reader);
        assert_eq!(status, 200);
        assert!(headers.contains("content-type: text/event-stream"));
        assert!(headers.contains("transfer-encoding: chunked"));
        Self {
            body: ChunkedBody::new(reader),
            last_database: None,
        }
    }

    fn event(&mut self) -> Event {
        let (mut name, mut id, mut data) = (String::new(), String::new(), String::new());
        loop {
            let line = self.body.line();
            if line.is_empty() {
                if !data.is_empty() {
                    return Event {
                        name,
                        id,
                        data: serde_json::from_str(&data).unwrap(),
                    };
                }
                continue;
            }
            if let Some(value) = line.strip_prefix("event:") {
                name = value.trim().into();
            } else if let Some(value) = line.strip_prefix("id:") {
                id = value.trim().into();
            } else if let Some(value) = line.strip_prefix("data:") {
                data.push_str(value.trim_start());
            }
        }
    }

    /// The next board snapshot or patch, noting database events on the way.
    fn board_event(&mut self) -> Event {
        loop {
            let event = self.event();
            if event.name == "database" {
                self.last_database = Some(event.data);
                continue;
            }
            return event;
        }
    }
}

fn response_head(reader: &mut BufReader<TcpStream>) -> (u16, String) {
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    let code = status.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            return (code, headers.to_ascii_lowercase());
        }
        headers.push_str(&line);
    }
}

struct ChunkedBody {
    reader: BufReader<TcpStream>,
    pending: Vec<u8>,
}

impl ChunkedBody {
    fn new(reader: BufReader<TcpStream>) -> Self {
        Self {
            reader,
            pending: Vec::new(),
        }
    }

    /// Next chunk's bytes, or None at the end of the body.
    fn chunk(&mut self) -> Option<Vec<u8>> {
        let mut size = String::new();
        self.reader.read_line(&mut size).unwrap();
        let size = usize::from_str_radix(size.trim(), 16).unwrap();
        let mut data = vec![0; size + 2];
        self.reader.read_exact(&mut data).unwrap();
        assert_eq!(&data[size..], b"\r\n");
        data.truncate(size);
        (size > 0).then_some(data)
    }

    fn line(&mut self) -> String {
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return String::from_utf8(line)
                    .unwrap()
                    .trim_end_matches(['\r', '\n'])
                    .into();
            }
            let chunk = self.chunk().expect("the event stream stays open");
            self.pending.extend(chunk);
        }
    }

    fn rest(&mut self) -> Vec<u8> {
        while let Some(chunk) = self.chunk() {
            self.pending.extend(chunk);
        }
        std::mem::take(&mut self.pending)
    }
}

fn get(address: SocketAddr, path: &str) -> (u16, String, Vec<u8>) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: console\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reader = BufReader::new(socket);
    let (status, headers) = response_head(&mut reader);
    let body = if headers.contains("transfer-encoding: chunked") {
        ChunkedBody::new(reader).rest()
    } else {
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        body
    };
    (status, headers, body)
}

/// A console started before tl3d migrates the store builds the board once the
/// schema matches, without waiting for a controller commit.
fn builds_once_the_store_is_migrated(database: &mut Database, assets: &Path) {
    let settings = database.create("console_unmigrated");
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.into(),
            models: None,
            links: Default::default(),
        },
    )
    .unwrap();
    let mut stream = EventStream::open_with_guard(console.address(), None, Duration::from_secs(20));
    drop(Store::open(&settings).unwrap());
    let event = stream.board_event();
    assert_eq!(event.name, "snapshot");
    assert_eq!(event.data["revision"], 0);
    assert_eq!(event.data["cards"], serde_json::json!([]));
}

/// The highest migration the pinned TL3 applies to a store it opens.
fn pinned_version(settings: &tl3_config::Store) -> i32 {
    raw(settings)
        .query_one("SELECT MAX(version) FROM tl3_schema_migrations", &[])
        .unwrap()
        .get(0)
}

fn add_migration(settings: &tl3_config::Store, version: i32) {
    raw(settings)
        .execute(
            "INSERT INTO tl3_schema_migrations VALUES ($1, 'SELECT 1')",
            &[&version],
        )
        .unwrap();
}

fn start_console(settings: &tl3_config::Store, assets: &Path) -> tl3_console::Running {
    tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.into(),
            models: None,
            links: Default::default(),
        },
    )
    .unwrap()
}

/// A store the pinned TL3 migrated passes the deploy's check with nothing to
/// warn about.
fn accepts_an_equal_schema(database: &mut Database) {
    let settings = database.create("console_equal_schema");
    drop(Store::open(&settings).unwrap());
    assert_eq!(tl3_console::check(&settings).unwrap(), Vec::<i32>::new());
}

/// `migrations.pin` is what lets the console tell a newer store from a
/// conflicting one, so it must be what the pinned TL3 really applies.
fn pinned_migrations_match_the_pinned_tl3(database: &mut Database) {
    let settings = database.create("console_pinned_migrations");
    drop(Store::open(&settings).unwrap());
    let applied: Vec<(i32, String)> = raw(&settings)
        .query(
            "SELECT version, md5(sql) FROM tl3_schema_migrations ORDER BY version",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let lines: String = applied
        .iter()
        .map(|(version, md5)| format!("{version} {md5}\n"))
        .collect();
    assert_eq!(
        tl3_console::schema::pinned_migrations(),
        applied,
        "update crates/console/migrations.pin to:\n{lines}"
    );
}

/// A TL3 deploy that migrates past the pinned commit: the deploy's check and
/// the board keep working, and the log says which versions are extra.
fn accepts_a_newer_schema_with_a_warning(database: &mut Database, assets: &Path) {
    let settings = database.create("console_newer_schema");
    drop(Store::open(&settings).unwrap());
    let next = pinned_version(&settings) + 1;
    add_migration(&settings, next);
    add_migration(&settings, next + 1);
    let extra = tl3_console::check(&settings).unwrap();
    assert_eq!(extra, [next, next + 1]);
    let warning = tl3_console::schema::warning(&extra);
    assert!(
        warning.contains(&next.to_string()) && warning.contains(&(next + 1).to_string()),
        "{warning}"
    );
    assert!(warning.contains("pin"), "{warning}");
    let console = start_console(&settings, assets);
    let mut stream = EventStream::open_with_guard(console.address(), None, Duration::from_secs(20));
    assert_eq!(stream.board_event().name, "snapshot");
    assert_eq!(get(console.address(), "/api/board").0, 200);
}

/// A store behind the pinned TL3 lacks tables the board reads.
fn refuses_an_older_schema(database: &mut Database, assets: &Path) {
    let settings = database.create("console_older_schema");
    drop(Store::open(&settings).unwrap());
    let pinned = pinned_version(&settings);
    let sql: String = raw(&settings)
        .query_one(
            "DELETE FROM tl3_schema_migrations WHERE version = $1 RETURNING sql",
            &[&pinned],
        )
        .unwrap()
        .get(0);
    let error = tl3_console::check(&settings).unwrap_err().to_string();
    assert!(
        error.contains(&format!("schema version {}", pinned - 1)),
        "{error}"
    );
    let console = start_console(&settings, assets);
    assert_eq!(get(console.address(), "/api/board").0, 503);
    let mut stream = EventStream::open_with_guard(console.address(), None, Duration::from_secs(20));
    raw(&settings)
        .execute(
            "INSERT INTO tl3_schema_migrations VALUES ($1, $2)",
            &[&pinned, &sql],
        )
        .unwrap();
    assert_eq!(stream.board_event().name, "snapshot");
}

/// A newer store that disagrees with the pin about a version both know is not
/// the pinned TL3's schema plus additions.
fn refuses_a_conflicting_schema(database: &mut Database, assets: &Path) {
    let settings = database.create("console_conflicting_schema");
    drop(Store::open(&settings).unwrap());
    add_migration(&settings, pinned_version(&settings) + 1);
    raw(&settings)
        .execute(
            "UPDATE tl3_schema_migrations SET sql = 'changed' WHERE version = 1",
            &[],
        )
        .unwrap();
    let error = tl3_console::check(&settings).unwrap_err().to_string();
    assert!(error.contains("migration"), "{error}");
    let console = start_console(&settings, assets);
    assert_eq!(get(console.address(), "/api/board").0, 503);
    drop(console);
    // A pinned version missing from a newer store is a conflict too.
    let settings = database.create("console_missing_migration");
    drop(Store::open(&settings).unwrap());
    add_migration(&settings, pinned_version(&settings) + 1);
    raw(&settings)
        .execute("DELETE FROM tl3_schema_migrations WHERE version = 3", &[])
        .unwrap();
    assert!(tl3_console::check(&settings).is_err());
}

/// Q53: when the maker's model reports quota, the card names the fallback the
/// worker will run, as soon as the quota row commits.
fn cards_follow_the_quota_fallback(settings: &tl3_config::Store, assets: &Path) {
    let config = tl3_config::Config::parse(include_str!("fixtures/tl3-example.toml")).unwrap();
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.into(),
            models: Some(config.models),
            links: Default::default(),
        },
    )
    .unwrap();
    let mut stream = EventStream::open_with_guard(console.address(), None, Duration::from_secs(20));
    let model = |cards: &Value| {
        cards
            .as_array()
            .unwrap()
            .iter()
            .find(|card| card["issue"] == 42)
            .map(|card| card["model"].clone())
    };
    let first = stream.board_event();
    assert_eq!(
        model(&first.data["cards"]),
        Some(Value::from("default-model"))
    );
    let mut store = Store::open(settings).unwrap();
    let snapshot = store.load().unwrap();
    let mut quota = tl3_store::Commit::new("console-quota", serde_json::json!("quota"), &snapshot);
    quota.rows.push(tl3_store::RowChange::Put {
        table: "model_quotas".into(),
        id: "default-model".into(),
        value: serde_json::json!({"reset_ms": now_ms() + 3_600_000}),
    });
    store.commit(&quota).unwrap();
    loop {
        let event = stream.board_event();
        if let Some(model) = model(&event.data["patch"]["upserts"]) {
            assert_eq!(model, "alternate-model");
            break;
        }
    }
}

/// An HTTP request from the console's page, or from another site's.
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, Vec<u8>) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    write!(socket, "{head}\r\n{body}").unwrap();
    let mut reader = BufReader::new(socket);
    let (status, headers) = response_head(&mut reader);
    let body = if headers.contains("transfer-encoding: chunked") {
        ChunkedBody::new(reader).rest()
    } else {
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        body
    };
    (status, body)
}

fn json_body(response: (u16, Vec<u8>), status: u16) -> Value {
    assert_eq!(
        response.0,
        status,
        "{}",
        String::from_utf8_lossy(&response.1)
    );
    serde_json::from_slice(&response.1).unwrap()
}

/// Rob saves a prompt version, sees it active, and reverts it, through a
/// console whose login may write prompt versions and nothing else.
fn prompts_edit_and_revert_through_the_api(database: &mut Database, assets: &Path) {
    let settings = database.create("console_prompts");
    let mut store = Store::open(&settings).unwrap();
    tl3_controller::prompts::seed(&mut store).unwrap();
    let login = "tl3_console_prompts";
    grant_reader(&settings, login, settings.password_file.as_deref().unwrap()).unwrap();
    let console_store = tl3_config::Store {
        user: login.into(),
        ..settings.clone()
    };
    let start = |store: &tl3_config::Store| {
        tl3_console::start(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            tl3_console::Options {
                store: store.clone(),
                assets: assets.into(),
                models: None,
                links: Default::default(),
            },
        )
        .unwrap()
    };
    let console = start(&console_store);
    let address = console.address();
    let page = [("X-TL3-Console", "1")];
    let roles = json_body(send(address, "GET", "/api/prompts", &[], None), 200);
    let names: Vec<_> = tl3_controller::prompts::BUILTIN
        .iter()
        .map(|(role, _)| serde_json::json!({"role": role, "active_number": 1, "active_builtin": true, "builtin_update": false}))
        .collect();
    assert_eq!(roles, Value::Array(names));
    let writer = json_body(send(address, "GET", "/api/prompts/writer", &[], None), 200);
    let shipped = writer["active"]["id"].as_i64().unwrap();
    let builtin = tl3_controller::prompts::BUILTIN
        .iter()
        .find(|(role, _)| *role == "writer")
        .unwrap()
        .1;
    assert_eq!(writer["active"]["text"], builtin);
    let edit = serde_json::json!({"text": "Write for Rob, briefly.", "note": "Shorter notes.", "base": shipped});
    let path = "/api/prompts/writer/versions";
    assert_eq!(send(address, "POST", path, &[], Some(&edit)).0, 403);
    let other_site = [
        ("X-TL3-Console", "1"),
        ("Origin", "http://attacker.invalid"),
    ];
    assert_eq!(send(address, "POST", path, &other_site, Some(&edit)).0, 403);
    let no_note = serde_json::json!({"text": "Write for Rob.", "note": " ", "base": shipped});
    assert_eq!(send(address, "POST", path, &page, Some(&no_note)).0, 400);
    let own_site = format!("http://{address}");
    let saved = json_body(
        send(
            address,
            "POST",
            path,
            &[("X-TL3-Console", "1"), ("Origin", &own_site)],
            Some(&edit),
        ),
        201,
    );
    let version = &saved["active"];
    assert_eq!(saved["versions"][0]["id"], version["id"]);
    assert_eq!(
        (
            &version["number"],
            &version["text"],
            &version["note"],
            &version["author"],
            &version["builtin"]
        ),
        (
            &Value::from(2),
            &Value::from("Write for Rob, briefly."),
            &Value::from("Shorter notes."),
            &Value::from("Rob"),
            &Value::from(false)
        )
    );
    let mine = version["id"].as_i64().unwrap();
    // The history names each version without its text; one comes on request.
    let history = json_body(send(address, "GET", path, &[], None), 200);
    assert_eq!(history, saved["versions"]);
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("text").is_none())
    );
    let first = json_body(
        send(address, "GET", &format!("{path}/{shipped}"), &[], None),
        200,
    );
    assert_eq!(
        (&first["text"], &first["number"]),
        (&Value::from(builtin), &Value::from(1))
    );
    assert_eq!(
        send(address, "GET", &format!("{path}/999999"), &[], None).0,
        404
    );
    // An editor opened before the save can't overwrite it.
    assert_eq!(send(address, "POST", path, &page, Some(&edit)).0, 409);
    let activate = |version: i64, expected: i64, headers: &[(&str, &str)]| {
        send(
            address,
            "POST",
            &format!("/api/prompts/writer/activate/{version}"),
            headers,
            Some(&serde_json::json!({"expected": expected})),
        )
    };
    assert_eq!(activate(shipped, mine, &[]).0, 403);
    // A page that still shows v1 active can't revert over v2.
    let (status, body) = activate(shipped, shipped, &page);
    assert_eq!(status, 409);
    assert!(String::from_utf8_lossy(&body).contains("Another version became active"));
    let reverted = json_body(activate(shipped, mine, &page), 200);
    assert_eq!(reverted["active"]["id"], shipped);
    assert_eq!(
        store
            .run_prompt("console-run", "writer")
            .unwrap()
            .unwrap()
            .id,
        shipped,
        "the next run uses the reverted version"
    );
    assert_eq!(activate(999_999, shipped, &page).0, 404);
    assert_eq!(
        send(
            address,
            "POST",
            "/api/prompts/nobody/versions",
            &page,
            Some(&edit)
        )
        .0,
        404
    );
    // Two saves from the same base: one lands, the other is refused.
    let base = shipped;
    let racers: Vec<_> = (0..2)
        .map(|n| {
            std::thread::spawn(move || {
                let edit = serde_json::json!({"text": format!("Racer {n}."), "note": "Race.", "base": base});
                send(
                    address,
                    "POST",
                    "/api/prompts/writer/versions",
                    &[("X-TL3-Console", "1")],
                    Some(&edit),
                )
                .0
            })
        })
        .collect();
    let mut statuses: Vec<u16> = racers.into_iter().map(|r| r.join().unwrap()).collect();
    statuses.sort();
    assert_eq!(statuses, [201, 409]);
    let raced = json_body(send(address, "GET", "/api/prompts/writer", &[], None), 200);
    assert_eq!(raced["versions"].as_array().unwrap().len(), 3);
    let current = raced["active"]["id"].as_i64().unwrap();
    // Limits: an oversized body is refused before it is read; a long note or
    // prompt within it is refused by the store.
    let huge =
        serde_json::json!({"text": "t".repeat(400 * 1024), "note": "Huge.", "base": current});
    assert_eq!(send(address, "POST", path, &page, Some(&huge)).0, 413);
    let long_text = serde_json::json!({"text": "t".repeat(tl3_store::TEXT_LIMIT + 1), "note": "Long.", "base": current});
    assert_eq!(send(address, "POST", path, &page, Some(&long_text)).0, 422);
    let long_note = serde_json::json!({"text": "Text.", "note": "n".repeat(tl3_store::NOTE_LIMIT + 1), "base": current});
    assert_eq!(send(address, "POST", path, &page, Some(&long_note)).0, 422);
    let after = json_body(send(address, "GET", path, &[], None), 200);
    assert_eq!(after.as_array().unwrap().len(), 3, "nothing was saved");
    // A deploy ships a changed built-in while Rob's version is active.
    json_body(activate(mine, current, &page), 200);
    store
        .seed_prompts(&[("writer", "Write, second release.")])
        .unwrap();
    let flagged = json_body(send(address, "GET", "/api/prompts/writer", &[], None), 200);
    assert_eq!(flagged["active"]["id"], mine);
    assert_eq!(flagged["builtin_update"]["text"], "Write, second release.");
    let update = flagged["builtin_update"]["id"].as_i64().unwrap();
    let roles = json_body(send(address, "GET", "/api/prompts", &[], None), 200);
    let writer = roles
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == "writer")
        .unwrap();
    assert_eq!(writer["builtin_update"], true);
    let keep = format!("/api/prompts/writer/keep/{update}");
    assert_eq!(send(address, "POST", &keep, &[], None).0, 403);
    let kept = json_body(send(address, "POST", &keep, &page, None), 200);
    assert_eq!(
        (&kept["active"]["id"], &kept["builtin_update"]),
        (&Value::from(mine), &Value::Null)
    );
    // Postgres drops the console's session; the next save opens a new one
    // first and lands once.
    raw(&settings)
        .execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE usename = $1 AND query NOT ILIKE 'LISTEN%'",
            &[&login],
        )
        .unwrap();
    let again = serde_json::json!({"text": "After a restart.", "note": "Reconnect.", "base": mine});
    let reconnected = json_body(send(address, "POST", path, &page, Some(&again)), 201);
    assert_eq!(reconnected["active"]["text"], "After a restart.");
    assert_eq!(reconnected["versions"].as_array().unwrap().len(), 5);
    // A store ahead of the pin still serves prompts, as it serves the board.
    raw(&settings)
        .execute(
            "INSERT INTO tl3_schema_migrations VALUES (999, 'SELECT 1')",
            &[],
        )
        .unwrap();
    let ahead = start(&console_store);
    json_body(
        send(ahead.address(), "GET", "/api/prompts/writer", &[], None),
        200,
    );
    raw(&settings)
        .execute("DELETE FROM tl3_schema_migrations WHERE version = 999", &[])
        .unwrap();
}
