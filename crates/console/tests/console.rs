mod support;

use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use support::Database;
use tl3_console::hidden::HiddenRepos;
use tl3_controller::{Bootstrap, Controller, LabelledIssue, Repository, adapters, board};
use tl3_store::{Reader, Store};

const ROLE: &str = "tl3_console_reader";

fn main() {
    if std::env::args().any(|arg| arg == "--postmortems") {
        postmortem_scenarios();
        return;
    }
    postmortem_scenarios();
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
            public_hosts: vec![],
            hidden: Default::default(),
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
    let hidden_database = hides_repos_from_every_view(assets.path());
    println!("test hides_repos_from_every_view ... ok");
    refuses_hosts_the_console_does_not_serve(&settings, assets.path());
    println!("test refuses_hosts_the_console_does_not_serve ... ok");
    cards_follow_the_quota_fallback(&settings, assets.path());
    println!("test cards_follow_the_quota_fallback ... ok");
    let (filer, mut filer_stream, node) =
        a_command_round_trips_to_tl3d_and_back(&settings, assets.path(), &mut controller);
    println!("test a_command_round_trips_to_tl3d_and_back ... ok");
    commands_stay_small(&settings, &filer, &mut filer_stream, node);
    println!("test commands_stay_small ... ok");
    store_refusals_say_why(&settings, &filer, node);
    println!("test store_refusals_say_why ... ok");
    drop(filer);
    prompt_store_errors_have_actionable_responses(assets.path());
    println!("test prompt_store_errors_have_actionable_responses ... ok");
    let prompt_database = prompts_edit_and_revert_through_the_api(assets.path());
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
    hidden_database.stop();
    prompt_database.stop();
    println!("20 console scenarios passed; no skips");
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
        labels: vec![],
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
    last_commands: Option<Value>,
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
            "GET /api/stream HTTP/1.1\r\nHost: {address}\r\nAccept: text/event-stream\r\n{resume}\r\n"
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
            last_commands: None,
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
            if event.name == "postmortems" {
                continue;
            }
            if event.name == "commands" {
                self.last_commands = Some(event.data);
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
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
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

/// One raw HTTP/1.1 request naming whatever Host the caller likes, or none
/// (HTTP/1.0). Returns the status and the whole body.
fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    host: Option<&str>,
    headers: &str,
) -> (u16, String) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    match host {
        Some(host) => write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{headers}\r\n"
        ),
        None => write!(socket, "{method} {path} HTTP/1.0\r\n{headers}\r\n"),
    }
    .unwrap();
    let mut reader = BufReader::new(socket);
    let (status, headers) = response_head(&mut reader);
    let mut body = Vec::new();
    if headers.contains("transfer-encoding: chunked") {
        body = ChunkedBody::new(reader).rest();
    } else {
        reader.read_to_end(&mut body).unwrap();
    }
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// DNS rebinding: a page on another site points its own name at the console,
/// and the browser sends that name as Host, with an Origin that matches it.
/// Every route is refused for any Host the console doesn't serve, and nothing
/// is served.
fn refuses_hosts_the_console_does_not_serve(settings: &tl3_config::Store, assets: &Path) {
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: settings.clone(),
            assets: assets.into(),
            models: None,
            links: Default::default(),
            public_hosts: vec!["Console.Example.Test".into()],
            hidden: Default::default(),
        },
    )
    .unwrap();
    let address = console.address();
    let port = address.port();
    // The board is built before the served names are compared against it.
    assert_eq!(
        EventStream::open(address, None).board_event().name,
        "snapshot"
    );
    let routes = [
        "/api/board",
        "/api/stream",
        "/api/requests/42",
        "/api/nothing-here",
        "/",
        "/index.html",
        "/missing-asset.js",
    ];
    let own_host = address.to_string();
    for (index, evil) in [
        "evil.example".to_string(),
        format!("evil.example:{port}"),
        // The right port on the wrong name, and the right name on a wrong port.
        format!("192.168.1.107:{port}"),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        // Without the port, the browser's default, it is not the bound address.
        "127.0.0.1".to_string(),
        "console.example.test.evil.example".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        for path in routes {
            for headers in [
                String::new(),
                format!("Origin: http://{evil}\r\nSec-Fetch-Site: same-origin\r\n"),
            ] {
                let (status, body) = request(address, "GET", path, Some(&evil), &headers);
                assert_eq!(status, 421, "GET {path} as {evil}: {body}");
                assert_eq!(body, "This console doesn't answer to that host name.\n");
            }
        }
        // A write is refused before its route, or the lack of one, is known.
        let (status, _) = request(
            address,
            "POST",
            "/api/board",
            Some(&evil),
            "Content-Length: 0\r\n",
        );
        assert_eq!(status, 421, "POST as {evil}");
        // So is a command, with the console's header and an Origin to match
        // the rebound name; nothing reaches the store.
        let id = format!("00000421-0000-4000-8000-{index:012x}");
        let (status, _) = request_with_body(
            address,
            "POST",
            "/api/requests/0/commands",
            &evil,
            &format!(
                "Content-Type: application/json\r\nX-TL3-Console: 1\r\nOrigin: http://{evil}\r\nSec-Fetch-Site: same-origin\r\n"
            ),
            &format!(r#"{{"id":"{id}","action":{{"kind":"pause"}}}}"#),
        );
        assert_eq!(status, 421, "POST a command as {evil}");
        assert!(!filed(settings, &id), "a command for {evil} was filed");
    }
    // HTTP/1.0 without a Host header names no host at all.
    for path in routes {
        let (status, body) = request(address, "GET", path, None, "");
        assert_eq!(status, 421, "GET {path} without Host: {body}");
    }
    // The bound address, `localhost` on loopback and a configured name (in
    // any case) are served.
    for host in [
        own_host.as_str(),
        &format!("localhost:{port}"),
        "console.example.test",
        "CONSOLE.example.test",
    ] {
        let (status, body) = request(address, "GET", "/api/board", Some(host), "");
        assert_eq!(status, 200, "GET /api/board as {host}: {body}");
        assert!(body.contains("\"revision\""), "{body}");
        let (status, body) = request(address, "GET", "/", Some(host), "");
        assert_eq!(status, 200, "GET / as {host}");
        assert!(body.contains("TL3 console"), "{body}");
    }
    // Past the allowlist, a route that takes no writes still says so.
    let (status, _) = request(
        address,
        "POST",
        "/api/board",
        Some(&own_host),
        "Content-Length: 0\r\n",
    );
    assert_eq!(status, 405);
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
            public_hosts: vec![],
            hidden: Default::default(),
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
            public_hosts: vec![],
            hidden: Default::default(),
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
            public_hosts: vec![],
            hidden: Default::default(),
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

fn post(address: SocketAddr, path: &str, headers: &str, body: &str) -> (u16, Vec<u8>) {
    request_with_body(
        address,
        "POST",
        path,
        &address.to_string(),
        &format!("Content-Type: application/json\r\n{headers}"),
        body,
    )
}

/// One raw HTTP/1.1 request, naming whatever Host the caller likes.
fn request_with_body(
    address: SocketAddr,
    method: &str,
    path: &str,
    host: &str,
    headers: &str,
    body: &str,
) -> (u16, Vec<u8>) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    // An oversized body may be refused before it is all read.
    let _ = write!(
        socket,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    );
    let mut reader = BufReader::new(socket);
    let (status, headers) = response_head(&mut reader);
    let body = if headers.contains("transfer-encoding: chunked") {
        ChunkedBody::new(reader).rest()
    } else {
        let mut body = Vec::new();
        let _ = reader.read_to_end(&mut body);
        body
    };
    (status, body)
}

/// The console files a command through its read-only login; tl3d applies it
/// on its next drain, and the stream shows it pending, then applied, with the
/// card's new badge.
fn a_command_round_trips_to_tl3d_and_back(
    settings: &tl3_config::Store,
    assets: &Path,
    controller: &mut Controller,
) -> (tl3_console::Running, EventStream, u64) {
    tl3_store::grant_reader(
        settings,
        "tl3_console_filer",
        settings.password_file.as_deref().unwrap(),
    )
    .unwrap();
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: tl3_config::Store {
                user: "tl3_console_filer".into(),
                ..settings.clone()
            },
            assets: assets.into(),
            models: None,
            links: Default::default(),
            public_hosts: vec![],
            hidden: Default::default(),
        },
    )
    .unwrap();
    let address = console.address();
    let mut stream = EventStream::open(address, None);
    let snapshot = stream.board_event();
    assert_eq!(snapshot.name, "snapshot");
    let card = snapshot.data["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["issue"] == 42)
        .unwrap()
        .clone();
    assert!(
        !card["badges"]
            .as_array()
            .unwrap()
            .contains(&"Paused".into())
    );
    let node = card["id"].as_u64().unwrap();
    let path = format!("/api/requests/{node}/commands");
    let id = "5c6d7e8f-0000-4000-8000-000000000331";
    let body = format!(r#"{{"id":"{id}","action":{{"kind":"pause"}}}}"#);
    let own =
        format!("X-TL3-Console: 1\r\nOrigin: http://{address}\r\nSec-Fetch-Site: same-origin\r\n");
    let own = own.as_str();
    for (headers, status) in [
        ("", 403),
        ("X-TL3-Console: 1\r\nOrigin: http://evil.example\r\n", 403),
        ("X-TL3-Console: 1\r\nSec-Fetch-Site: cross-site\r\n", 403),
    ] {
        assert_eq!(post(address, &path, headers, &body).0, status, "{headers}");
    }
    assert_eq!(
        post(
            address,
            &path,
            own,
            r#"{"id":"x","action":{"kind":"pause"}}"#
        )
        .0,
        422
    );
    assert_eq!(
        post(
            address,
            &path,
            own,
            &format!(r#"{{"id":"{id}","action":{{"kind":"launch"}}}}"#)
        )
        .0,
        422
    );
    let (status, reply) = post(address, &path, own, &body);
    assert_eq!(status, 202, "{}", String::from_utf8_lossy(&reply));
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap(),
        serde_json::json!({"id": id, "filed": true})
    );
    let (status, reply) = post(address, &path, own, &body);
    assert_eq!(status, 202);
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap()["filed"],
        false,
        "resending the same id files nothing more"
    );
    let status_of = |stream: &mut EventStream| -> Option<String> {
        stream.last_commands.as_ref().and_then(|event| {
            event["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|command| command["id"] == id)
                .map(|command| command["status"].as_str().unwrap().to_string())
        })
    };
    while status_of(&mut stream).as_deref() != Some("pending") {
        let event = stream.event();
        if event.name == "commands" {
            stream.last_commands = Some(event.data);
        }
    }
    tl3_controller::ingress::drain(controller).unwrap();
    let mut paused = false;
    while !paused || status_of(&mut stream).as_deref() != Some("applied") {
        let event = stream.event();
        match event.name.as_str() {
            "commands" => stream.last_commands = Some(event.data),
            "patch" => {
                paused |= event.data["patch"]["upserts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|card| {
                        card["id"] == node
                            && card["badges"]
                                .as_array()
                                .unwrap()
                                .contains(&"Paused".into())
                    });
            }
            _ => {}
        }
    }
    assert_eq!(
        stream.last_commands.as_ref().unwrap()["commands"][0]["action"],
        serde_json::json!({"kind": "pause"})
    );
    (console, stream, node)
}

fn filed(settings: &tl3_config::Store, id: &str) -> bool {
    raw(settings)
        .query_opt("SELECT 1 FROM tl3_console_commands WHERE id = $1", &[&id])
        .unwrap()
        .is_some()
}

/// Command bodies are small: a few KiB around TL3's 20,000-character cap on
/// Rob's words. Oversized ones never reach TL3, and the stream doesn't repeat
/// long texts in every commands event.
fn commands_stay_small(
    settings: &tl3_config::Store,
    console: &tl3_console::Running,
    stream: &mut EventStream,
    node: u64,
) {
    let address = console.address();
    let path = format!("/api/requests/{node}/commands");
    let own =
        format!("X-TL3-Console: 1\r\nOrigin: http://{address}\r\nSec-Fetch-Site: same-origin\r\n");
    let comment = |id: &str, chars: usize| {
        format!(
            r#"{{"id":"{id}","action":{{"kind":"comment","text":"{}"}}}}"#,
            "a".repeat(chars)
        )
    };
    let huge = "5c6d7e8f-0000-4000-8000-000000000333";
    assert_eq!(post(address, &path, &own, &comment(huge, 200_000)).0, 413);
    assert!(!filed(settings, huge));
    let over = "5c6d7e8f-0000-4000-8000-000000000334";
    assert_eq!(post(address, &path, &own, &comment(over, 20_001)).0, 422);
    let guidance = "5c6d7e8f-0000-4000-8000-000000000336";
    let long_guidance = format!(
        r#"{{"id":"{guidance}","action":{{"kind":"answer","question":1,"choice":{{"answer":"try_again","guidance":"{}"}}}}}}"#,
        "g".repeat(20_001)
    );
    assert_eq!(post(address, &path, &own, &long_guidance).0, 422);
    assert!(!filed(settings, over) && !filed(settings, guidance));
    let full = "5c6d7e8f-0000-4000-8000-000000000335";
    assert_eq!(post(address, &path, &own, &comment(full, 20_000)).0, 202);
    assert!(filed(settings, full));
    loop {
        let event = stream.event();
        if event.name != "commands" {
            continue;
        }
        let Some(shown) = event.data["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|command| command["id"] == full)
            .cloned()
        else {
            continue;
        };
        let text = shown["action"]["text"].as_str().unwrap();
        assert_eq!(text.chars().count(), 201, "{text}");
        assert!(text.ends_with('…'));
        break;
    }
}

/// The store's own caps on a command come back as HTTP answers that say
/// which: a command over 64 KiB once stored, and a backlog tl3d isn't draining.
fn store_refusals_say_why(settings: &tl3_config::Store, console: &tl3_console::Running, node: u64) {
    let address = console.address();
    let path = format!("/api/requests/{node}/commands");
    let own =
        format!("X-TL3-Console: 1\r\nOrigin: http://{address}\r\nSec-Fetch-Site: same-origin\r\n");
    // 20,000 characters pass the console's cap, but four bytes each is over the store's.
    let wide = "5c6d7e8f-0000-4000-8000-000000000337";
    let body = format!(
        r#"{{"id":"{wide}","action":{{"kind":"comment","text":"{}"}}}}"#,
        "\u{1d11e}".repeat(20_000)
    );
    let (status, reply) = post(address, &path, &own, &body);
    assert_eq!(status, 413, "{}", String::from_utf8_lossy(&reply));
    assert_eq!(
        String::from_utf8(reply).unwrap(),
        "This command is over 64 KiB once stored; shorten your words.\n"
    );
    assert!(!filed(settings, wide));
    let mut owner = raw(settings);
    owner
        .batch_execute(
            "INSERT INTO tl3_console_commands (id, node_id, action)
             SELECT 'backlog-' || n, 0, '{\"kind\":\"pause\"}' FROM generate_series(1, 1000) n",
        )
        .unwrap();
    let late = "5c6d7e8f-0000-4000-8000-000000000338";
    let body = format!(r#"{{"id":"{late}","action":{{"kind":"pause"}}}}"#);
    let (status, reply) = post(address, &path, &own, &body);
    assert_eq!(status, 429, "{}", String::from_utf8_lossy(&reply));
    assert_eq!(
        String::from_utf8(reply).unwrap(),
        "1,000 commands are waiting for TL3 to apply them; try again once it catches up.\n"
    );
    assert!(!filed(settings, late));
    owner
        .batch_execute("DELETE FROM tl3_console_commands WHERE id LIKE 'backlog-%'")
        .unwrap();
}

fn issue_in(repo: &str, number: u64) -> LabelledIssue {
    LabelledIssue {
        repo: repo.into(),
        delivery: format!("hidden-delivery-{repo}-{number}"),
        ..issue(number)
    }
}

/// `hide_repos` (Rob, 2026-11-02): requests of a hidden repository reach the
/// browser through no route. The board, its stream, the counts, the case file
/// and the command route all behave as if the repository did not exist. The
/// store keeps both repositories' requests.
fn hides_repos_from_every_view(assets: &Path) -> Database {
    const HIDDEN: &str = "tl3-sandbox";
    // Its own server: the shared one is near its 20-connection limit, and a
    // console keeps its sessions for the life of the process.
    let mut database = Database::start();
    let settings = database.create("console_hidden");
    let records = tempfile::tempdir().unwrap();
    let mut controller = Controller::open(
        Store::open(&settings).unwrap(),
        adapters::simulation(records.path()).unwrap(),
        Bootstrap {
            repos: vec![
                Repository {
                    owner: "RobTand".into(),
                    name: HIDDEN.into(),
                    installation: 1,
                },
                Repository {
                    owner: "RobTand".into(),
                    name: "tl3-console".into(),
                    installation: 1,
                },
            ],
            ..bootstrap()
        },
    )
    .unwrap();
    // Nodes 0 and 2 are hidden; 1 and 3 are shown.
    let first_hidden = 0;
    let shown = 1;
    controller.intake(issue_in(HIDDEN, 42)).unwrap();
    controller.intake(issue_in("tl3-console", 7)).unwrap();
    let config = tl3_config::Config::parse(include_str!("fixtures/tl3-example.toml")).unwrap();
    let start = |hidden: HiddenRepos| {
        tl3_console::start(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            tl3_console::Options {
                store: settings.clone(),
                assets: assets.into(),
                models: Some(config.models.clone()),
                links: Default::default(),
                public_hosts: vec![],
                hidden,
            },
        )
        .unwrap()
    };
    // Cased unlike the stored `RobTand/tl3-sandbox`: GitHub ignores case, so
    // the store's side of the comparison must too.
    let console = start(
        HiddenRepos::new(
            &[format!("ROBTAND/{}", HIDDEN.to_ascii_uppercase())],
            &config.github.repos,
        )
        .unwrap(),
    );
    let address = console.address();
    let board_of = |address| -> Value {
        let (status, _, body) = get(address, "/api/board");
        assert_eq!(status, 200);
        serde_json::from_slice(&body).unwrap()
    };
    let issues = |board: &Value| -> Vec<u64> {
        board["cards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card["issue"].as_u64().unwrap())
            .collect()
    };
    // The store holds both repositories' requests, so the filter is what hides.
    let stored: i64 = raw(&settings)
        .query_one(
            "SELECT count(*) FROM tl3_world WHERE table_name = 'requests'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(stored, 2);

    // The stream's snapshot, then the board route: no card, no count and no
    // word of the hidden repository.
    let mut stream = EventStream::open_with_guard(address, None, Duration::from_secs(30));
    let snapshot = stream.board_event();
    assert_eq!(snapshot.name, "snapshot");
    assert_eq!(issues(&snapshot.data), [7]);
    assert!(!snapshot.data.to_string().contains(HIDDEN));
    let board = board_of(address);
    assert_eq!(issues(&board), [7]);
    assert_eq!(board["cards"][0]["repo"], "RobTand/tl3-console");
    assert_eq!(board["flow"]["in_flight"], 1);
    let counted: u64 = board["flow"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| column["count"].as_u64().unwrap())
        .sum();
    assert_eq!(counted, 1, "column counts cover shown requests only");
    assert!(!board.to_string().contains(HIDDEN), "{board}");

    // Every patch after more requests arrive.
    controller.intake(issue_in(HIDDEN, 43)).unwrap();
    controller.intake(issue_in("tl3-console", 8)).unwrap();
    loop {
        let event = stream.board_event();
        assert_eq!(event.name, "patch");
        assert!(!event.data.to_string().contains(HIDDEN), "{}", event.data);
        let upserts = event.data["patch"]["upserts"].as_array().unwrap();
        if upserts.iter().any(|card| card["issue"] == 8) {
            assert_eq!(event.data["patch"]["flow"]["in_flight"], 2);
            break;
        }
    }

    // A newer request for the hidden issue replaces request 0, which then
    // leaves the board's rows. tl3d writes the replacement's node in the same
    // commit; the binding alone makes it the newest, so it is all this stores.
    // An input failure on a hidden issue has no request, and a shown one is
    // the control.
    let replacement = 100;
    let incident = |id: &str, repo: &str, issue: u64| {
        serde_json::json!({
            "id": id, "node": null, "open": true, "needs_rob": false, "blocking": false,
            "summary": format!("Reading {repo}#{issue} failed. Other requests can continue."),
            "signature": {"upstream": "GitHub", "class": "transport"},
            "effect": null, "post_key": null, "cause": "upstream",
            "input_failure": {
                "scope": format!("intake:robtand/{repo}/1:{issue}"),
                "target": {
                    "repository": {"owner": "RobTand", "name": repo, "installation": 1},
                    "issue": issue,
                },
                "title": "Failed read", "posted": true, "attempted_epoch": null,
            },
        })
    };
    {
        let mut store = Store::open(&settings).unwrap();
        let snapshot = store.load().unwrap();
        let original = snapshot
            .rows
            .iter()
            .find(|row| row.table == "requests" && row.id == first_hidden.to_string())
            .unwrap()
            .value
            .clone();
        let mut commit =
            tl3_store::Commit::new("console-hidden-rows", serde_json::json!("rows"), &snapshot);
        for (table, id, value) in [
            ("requests", replacement.to_string(), original),
            (
                "incidents",
                "input.hidden".to_string(),
                incident("input.hidden", HIDDEN, 77),
            ),
            (
                "incidents",
                "input.shown".to_string(),
                incident("input.shown", "tl3-console", 78),
            ),
        ] {
            commit.rows.push(tl3_store::RowChange::Put {
                table: table.into(),
                id,
                value,
            });
        }
        store.commit(&commit).unwrap();
    }
    // The shown incident reaches the board; the hidden one never does, and
    // nothing of it reaches a patch on the way.
    loop {
        let event = stream.board_event();
        assert_eq!(event.name, "patch");
        assert!(!event.data.to_string().contains(HIDDEN), "{}", event.data);
        assert!(!event.data.to_string().contains("input.hidden"));
        let shown_incidents = event.data["patch"]["unassigned_incidents"]
            .as_array()
            .unwrap();
        if !shown_incidents.is_empty() {
            assert_eq!(shown_incidents.len(), 1);
            assert_eq!(shown_incidents[0]["id"], "input.shown");
            break;
        }
    }
    // A new connection's snapshot and the board route agree.
    let mut fresh = EventStream::open_with_guard(address, None, Duration::from_secs(30));
    let snapshot = fresh.board_event();
    assert_eq!(snapshot.name, "snapshot");
    for board in [snapshot.data, board_of(address)] {
        assert_eq!(board["unassigned_incidents"].as_array().unwrap().len(), 1);
        assert_eq!(board["unassigned_incidents"][0]["id"], "input.shown");
        assert!(!board.to_string().contains(HIDDEN), "{board}");
        assert_eq!(issues(&board), [7, 8]);
    }

    // The case file and the command route answer 404 for a hidden request,
    // and file nothing.
    let open = |address, node: u64| get(address, &format!("/api/requests/{node}")).0;
    assert_eq!(open(address, first_hidden), 404);
    assert_eq!(
        open(address, 2),
        404,
        "a request that arrived after startup"
    );
    assert_eq!(open(address, shown), 200);
    let own =
        format!("X-TL3-Console: 1\r\nOrigin: http://{address}\r\nSec-Fetch-Site: same-origin\r\n");
    let command = |id: &str| format!(r#"{{"id":"{id}","action":{{"kind":"pause"}}}}"#);
    let refused = "6d6f6f6e-0000-4000-8000-000000000001";
    let (status, _) = post(
        address,
        &format!("/api/requests/{first_hidden}/commands"),
        &own,
        &command(refused),
    );
    assert_eq!(status, 404);
    assert!(
        !filed(&settings, refused),
        "nothing filed for a hidden request"
    );
    // The replacement is hidden too, though no card or case file shows it.
    let refused_replacement = "6d6f6f6e-0000-4000-8000-000000000005";
    let (status, _) = post(
        address,
        &format!("/api/requests/{replacement}/commands"),
        &own,
        &command(refused_replacement),
    );
    assert_eq!(status, 404);
    assert!(!filed(&settings, refused_replacement));
    let accepted = "6d6f6f6e-0000-4000-8000-000000000002";
    let (status, _) = post(
        address,
        &format!("/api/requests/{shown}/commands"),
        &own,
        &command(accepted),
    );
    assert_eq!(status, 202);
    assert!(filed(&settings, accepted));

    // Commands that reached the store for a hidden request (from an older
    // console, or tl3d's own tools) never reach the browser either. Request 0
    // is no longer in the board's rows, yet its command stays hidden.
    let mut direct = tl3_store::ConsoleCommands::open(&settings).unwrap();
    let leaked = "6d6f6f6e-0000-4000-8000-000000000003";
    let pause = serde_json::json!({"kind": "comment", "text": "secret sandbox words"});
    assert!(direct.submit(leaked, first_hidden, &pause).unwrap());
    let seen = "6d6f6f6e-0000-4000-8000-000000000004";
    assert!(
        direct
            .submit(seen, shown, &serde_json::json!({"kind": "pause"}))
            .unwrap()
    );
    loop {
        let event = stream.event();
        if event.name != "commands" {
            continue;
        }
        let commands = event.data["commands"].as_array().unwrap();
        assert!(
            commands
                .iter()
                .all(|command| command["node"] != first_hidden),
            "{commands:?}"
        );
        assert!(!event.data.to_string().contains("secret sandbox words"));
        if commands.iter().any(|command| command["id"] == seen) {
            break;
        }
    }
    database
}

/// An HTTP request from the console's page, or from another site's.
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, Vec<u8>) {
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut head = String::from("Content-Type: application/json\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    request_with_body(address, method, path, &address.to_string(), &head, &body)
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

/// Real SQLSTATEs from the embedded store, followed by a connection outage.
fn prompt_store_errors_have_actionable_responses(assets: &Path) {
    let mut database = Database::start();
    let settings = database.create("prompt_errors");
    let mut store = Store::open(&settings).unwrap();
    tl3_controller::prompts::seed(&mut store).unwrap();
    let login = "console_prompt_errors";
    tl3_store::grant_reader(&settings, login, settings.password_file.as_deref().unwrap()).unwrap();
    let console = tl3_console::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        tl3_console::Options {
            store: tl3_config::Store {
                user: login.into(),
                ..settings.clone()
            },
            assets: assets.into(),
            models: None,
            links: Default::default(),
            public_hosts: vec![],
            hidden: Default::default(),
        },
    )
    .unwrap();
    let address = console.address();
    let role = json_body(send(address, "GET", "/api/prompts/writer", &[], None), 200);
    let base = role["active"]["id"].as_i64().unwrap();
    let page = [("X-TL3-Console", "1")];
    let path = "/api/prompts/writer/versions";
    let edit = serde_json::json!({"text": "An edit.", "note": "Why.", "base": base});
    let mut admin = raw(&settings);
    let sessions = |admin: &mut postgres::Client| -> Vec<i32> {
        admin
            .query(
                "SELECT pid FROM pg_stat_activity WHERE usename = $1 ORDER BY pid",
                &[&login],
            )
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect()
    };
    let original = sessions(&mut admin);
    admin
        .batch_execute("REVOKE tl3_prompt_editor FROM console_prompt_errors")
        .unwrap();
    let denied = send(address, "POST", path, &page, Some(&edit));
    let after_denied = sessions(&mut admin);
    admin
        .batch_execute("GRANT tl3_prompt_editor TO console_prompt_errors")
        .unwrap();
    let nul = serde_json::json!({"text": "Bad\0text", "note": "Why.", "base": base});
    let invalid = send(address, "POST", path, &page, Some(&nul));
    let after_invalid = sessions(&mut admin);
    admin
        .batch_execute(
            "CREATE FUNCTION reject_prompt_text() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'Prompt text cannot be converted' USING ERRCODE = '22001'; END $$;
         CREATE TRIGGER reject_prompt_text BEFORE INSERT ON tl3_prompt_versions
         FOR EACH ROW EXECUTE FUNCTION reject_prompt_text();",
        )
        .unwrap();
    let other_text = send(address, "POST", path, &page, Some(&edit));
    let after_other = sessions(&mut admin);
    admin
        .batch_execute("DROP TRIGGER reject_prompt_text ON tl3_prompt_versions")
        .unwrap();
    // Closing the real socket must return 503, then open a fresh session on recovery.
    drop(admin);
    drop(store);
    database.stop();
    let unavailable = send(address, "POST", path, &page, Some(&edit));
    database.resume();
    let recovered = send(address, "POST", path, &page, Some(&edit));
    database.stop();
    let statuses = [
        denied.0,
        invalid.0,
        other_text.0,
        unavailable.0,
        recovered.0,
    ];
    assert_eq!(
        statuses,
        [500, 422, 422, 503, 201],
        "permission (42501), NUL (22021), text (22001), I/O outage, recovery; bodies: {:?}",
        [&denied.1, &invalid.1, &other_text.1, &unavailable.1]
            .map(|body| String::from_utf8_lossy(body))
    );
    assert!(String::from_utf8_lossy(&denied.1).contains(
        "The console's database login can't edit prompts; TL3's deploy grants it (tl3-deploy console-role)"));
    assert!(String::from_utf8_lossy(&invalid.1).contains("invalid byte sequence"));
    assert!(String::from_utf8_lossy(&other_text.1).contains("Prompt text cannot be converted"));
    assert!(String::from_utf8_lossy(&unavailable.1).contains("Try again shortly"));
    assert_eq!(
        original, after_denied,
        "permission denial dropped the session"
    );
    assert_eq!(original, after_invalid, "bad text dropped the session");
    assert_eq!(original, after_other, "data exception dropped the session");
}

/// Rob saves a prompt version, sees it active, and reverts it, through a
/// console whose login may write prompt versions and nothing else.
fn prompts_edit_and_revert_through_the_api(assets: &Path) -> Database {
    // Test consoles keep their sessions for the process lifetime. Isolate the
    // prompt sessions from the shared cluster's 20-connection limit.
    let mut database = Database::start();
    let settings = database.create("console_prompts");
    let mut store = Store::open(&settings).unwrap();
    tl3_controller::prompts::seed(&mut store).unwrap();
    let login = "tl3_console_prompts";
    tl3_store::grant_reader(&settings, login, settings.password_file.as_deref().unwrap()).unwrap();
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
                public_hosts: vec![],
                hidden: Default::default(),
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
    for path in [
        path.to_string(),
        format!("/api/prompts/writer/activate/{shipped}"),
        format!("/api/prompts/writer/keep/{shipped}"),
    ] {
        let (status, _) = request_with_body(
            address,
            "POST",
            &path,
            "attacker.invalid",
            "Content-Type: application/json\r\nX-TL3-Console: 1\r\nOrigin: http://attacker.invalid\r\nSec-Fetch-Site: same-origin\r\n",
            &edit.to_string(),
        );
        assert_eq!(status, 421, "POST {path} with a foreign Host");
    }
    let unchanged = json_body(send(address, "GET", "/api/prompts/writer", &[], None), 200);
    assert_eq!(unchanged, writer, "foreign-Host writes changed the prompt");
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
    database
}

fn postmortem_scenarios() {
    let mut database = Database::start();
    let settings = database.create("postmortems");
    let records = tempfile::tempdir().unwrap();
    let mut controller = open(&settings, records.path());
    controller.intake(issue(337)).unwrap();
    let mut admin = raw(&settings);
    admin.batch_execute(r#"INSERT INTO tl3_postmortems(node_id, trigger, reason, sample_share, repo, issue, title, revision, owed_at, status, cause, summary)
        SELECT '0', 'close', 'sampled', 0.1, 'RobTand/tl3-sandbox', 337, 'Retry after disconnect', min(revision), '2026-11-01T00:00:00Z', 'done', 'outside', 'Network unavailable' FROM tl3_events;
        INSERT INTO tl3_postmortems(node_id, trigger, reason, repo, issue, title, revision, owed_at)
        SELECT '0', 'question:7', 'escalation', 'RobTand/tl3-sandbox', 337, 'Needs Rob', min(revision), '2026-11-02T00:00:00Z' FROM tl3_events;
        INSERT INTO tl3_postmortems(node_id, trigger, reason, repo, issue, title, revision, owed_at)
        SELECT '9', 'close', 'needed_fix', 'RobTand/private', 99, 'Secret hidden title', min(revision), '2026-11-02T00:00:00Z' FROM tl3_events;
        INSERT INTO tl3_postmortem_recommendations(postmortem_id, fingerprint, rank, phase, kind, target, operation, before, after, evidence, effect)
        SELECT id, 'rec-0123456789abcdef', 1, 'making', 'prompt', 'Retry connection', 'add', '', 'Wait for recovery', 'socket disconnected', 'Recover automatically' FROM tl3_postmortems;
        INSERT INTO tl3_code_grades(node_id, trigger, rubric_version, dimension, grade, fact, revision, recorded_at)
        SELECT '0', 'close', 1, 'reliability', 4, '{"fix_rounds":1}', min(revision), clock_timestamp() FROM tl3_events;"#).unwrap();
    let config = tl3_config::Config::parse(include_str!("fixtures/tl3-example.toml")).unwrap();
    let hidden = HiddenRepos::new(&["RobTand/tl3-sandbox".into()], &config.github.repos).unwrap();
    tl3_store::grant_reader(
        &settings,
        "postmortem_reader",
        settings.password_file.as_deref().unwrap(),
    )
    .unwrap();
    let assets = tempfile::tempdir().unwrap();
    let start = |hidden| {
        tl3_console::start(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            tl3_console::Options {
                store: tl3_config::Store {
                    user: "postmortem_reader".into(),
                    ..settings.clone()
                },
                assets: assets.path().into(),
                models: None,
                links: Default::default(),
                public_hosts: vec![],
                hidden,
            },
        )
        .unwrap()
    };
    let console = start(Default::default());
    let concealed = start(hidden);
    let address = console.address();
    let json = |path: &str| {
        let (status, _, body) = get(address, path);
        assert_eq!(status, 200, "{path}: {}", String::from_utf8_lossy(&body));
        serde_json::from_slice::<Value>(&body).unwrap()
    };
    type Scenario<'a> = (&'static str, Box<dyn FnMut() + 'a>);
    let tests: Vec<Scenario<'_>> = vec![
        (
            "postmortems_request_all_triggers_and_grades",
            Box::new(|| {
                let result = json("/api/requests/0/postmortems");
                assert_eq!(result["postmortems"][0]["trigger"], "question:7");
                assert_eq!(result["postmortems"][1]["sample_share"], 0.1);
                assert_eq!(result["postmortems"][1]["cause"], "outside");
                assert_eq!(result["grades"][0]["grade"], 4);
                assert_eq!(result["grades"][0]["fact"]["fix_rounds"], 1);
                assert_eq!(get(address, "/api/requests/999/postmortems").0, 404);
            }),
        ),
        (
            "postmortems_search_filters_and_keyset_paging",
            Box::new(|| {
                let result = json(
                    "/api/postmortems?q=disconnected&repo=RobTand%2Ftl3-sandbox&cause=outside&reason=sampled&status=done&from=2026-11-01&to=2026-11-02&limit=1",
                );
                assert_eq!(result["postmortems"].as_array().unwrap().len(), 1);
                assert_eq!(result["postmortems"][0]["trigger"], "close");
                assert_eq!(result["postmortems"][0]["recommendations"][0]["seen_in"], 3);
                let first = json("/api/postmortems?limit=1");
                let next = first["next_before"].as_i64().expect("another page");
                assert!(
                    json(&format!("/api/postmortems?before={next}&limit=1"))["postmortems"][0]["id"]
                        .as_i64() < Some(next)
                );
                for query in [
                    "limit=101",
                    "status=bogus",
                    "from=2026-02-30",
                    "from=2026-11-03&to=2026-11-01",
                ] {
                    assert_eq!(get(address, &format!("/api/postmortems?{query}")).0, 400);
                }
            }),
        ),
        (
            "postmortems_hidden_repos_and_counts",
            Box::new(|| {
                let hidden_address = concealed.address();
                assert_eq!(get(hidden_address, "/api/requests/0/postmortems").0, 404);
                let (status, _, body) = get(hidden_address, "/api/postmortems");
                assert_eq!(status, 200);
                let result: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(result["postmortems"].as_array().unwrap().len(), 1);
                assert_eq!(result["postmortems"][0]["recommendations"][0]["seen_in"], 1);
                assert_eq!(get(hidden_address, "/api/postmortems/0").0, 404);
                let response =
                    request(address, "GET", "/api/postmortems", Some("evil.example"), "");
                assert_eq!(response.0, 421);
            }),
        ),
        (
            "postmortems_three_tables_refresh_stream_without_world_change",
            Box::new(|| {
                assert_eq!(get(address, "/api/requests/0/postmortems").0, 200);
                let mut stream = EventStream::open(address, None);
                stream.board_event();
                // Each connection sends an invalidation, so reconnects refresh an open page.
                while stream.event().name != "postmortems" {}
                for sql in [
                    "UPDATE tl3_postmortems SET notes = 'Changed' WHERE node_id = '0'",
                    "UPDATE tl3_postmortem_recommendations SET evidence = 'new evidence'",
                    "INSERT INTO tl3_code_grades(node_id, trigger, rubric_version, dimension, fact, revision, recorded_at) SELECT '0', 'close', 1, 'credit', '{}', min(revision), clock_timestamp() FROM tl3_events",
                ] {
                    admin.batch_execute(sql).unwrap();
                    let event = stream.event();
                    assert_eq!(event.name, "postmortems", "{event:?}");
                }
            }),
        ),
    ];
    let mut failed = 0;
    for (name, mut test) in tests {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut test));
        println!(
            "test {name} ... {}",
            if result.is_ok() { "ok" } else { "FAILED" }
        );
        failed += usize::from(result.is_err());
    }
    database.stop();
    assert_eq!(failed, 0, "postmortem scenarios failed");
}
