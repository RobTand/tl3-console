//! Which stores the console accepts. TL3's migrations only add, so a store
//! that applied everything the pinned TL3 knows, plus newer versions, still
//! has every table and column the pinned board projection reads.

use postgres::{Client, IsolationLevel, NoTls};
use std::{fs, time::Duration};
use tl3_store::{Reader, Transaction};

/// A migration as the store records it: its version and the md5 of its SQL.
pub type Migration = (i32, String);

/// The pinned TL3's migrations, from `migrations.pin`.
pub fn pinned_migrations() -> Vec<Migration> {
    include_str!("../migrations.pin")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let (version, md5) = line.split_once(' ').expect("a version and an md5");
            (version.parse().expect("a version number"), md5.into())
        })
        .collect()
}

/// A store's applied migrations disagree with the pinned TL3's.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Conflict;

/// The versions `applied` has beyond `pinned`, or a conflict when it lacks a
/// pinned version, records one with different SQL, or has an unknown version
/// below the pin's last.
pub(crate) fn judge(pinned: &[Migration], applied: &[Migration]) -> Result<Vec<i32>, Conflict> {
    let last = pinned.last().map_or(0, |&(version, _)| version);
    let mut known = 0;
    let mut extra = Vec::new();
    for (version, md5) in applied {
        match pinned.iter().find(|(pinned, _)| pinned == version) {
            Some((_, pinned_md5)) if pinned_md5 == md5 => known += 1,
            Some(_) => return Err(Conflict),
            None if *version > last => extra.push(*version),
            None => return Err(Conflict),
        }
    }
    if known == pinned.len() {
        Ok(extra)
    } else {
        Err(Conflict)
    }
}

/// What to log when a store is ahead of the pin.
pub fn warning(extra: &[i32]) -> String {
    let versions: Vec<String> = extra.iter().map(i32::to_string).collect();
    format!(
        "store has migrations {} beyond the pinned TL3's; serving anyway. \
         Bump the console's TL3 pin (README, \"The TL3 pin\").",
        versions.join(", ")
    )
}

/// One read-only session on the store. The pinned TL3's reader refuses a store
/// with any newer migration, so a store that is ahead gets a session that
/// checks the migrations itself.
pub(crate) enum Session {
    Pinned(Reader),
    Ahead { client: Client, extra: Vec<i32> },
}

impl Session {
    /// The one way the console opens a store session. A store that is ahead
    /// is logged with `who`, the part of the console that opened it.
    pub(crate) fn open(store: &tl3_config::Store, who: &str) -> tl3_store::Result<Self> {
        match Reader::open(store) {
            Ok(reader) => Ok(Self::Pinned(reader)),
            Err(tl3_store::Error::SchemaVersion { actual, expected }) if actual > expected => {
                let mut client = connect(store)?;
                let extra = Self::check_ahead(&mut client)?;
                eprintln!("console {who}: {}", warning(&extra));
                Ok(Self::Ahead { client, extra })
            }
            Err(error) => Err(error),
        }
    }

    /// The versions beyond the pin, empty when the store matches it.
    pub(crate) fn extra(&self) -> &[i32] {
        match self {
            Self::Pinned(_) => &[],
            Self::Ahead { extra, .. } => extra,
        }
    }

    /// A read transaction whose snapshot the schema check shared, as the
    /// pinned reader's is.
    pub(crate) fn transaction(&mut self) -> tl3_store::Result<Transaction<'_>> {
        match self {
            Self::Pinned(reader) => reader.transaction(),
            Self::Ahead { client, .. } => {
                let mut tx = client
                    .build_transaction()
                    .isolation_level(IsolationLevel::RepeatableRead)
                    .read_only(true)
                    .start()?;
                judge_applied(&mut tx)?;
                Ok(tx)
            }
        }
    }

    fn check_ahead(client: &mut Client) -> tl3_store::Result<Vec<i32>> {
        let mut tx = client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let extra = judge_applied(&mut tx)?;
        tx.commit()?;
        Ok(extra)
    }
}

fn judge_applied(tx: &mut Transaction<'_>) -> tl3_store::Result<Vec<i32>> {
    let applied: Vec<Migration> = tx
        .query(
            "SELECT version, md5(sql) FROM tl3_schema_migrations ORDER BY version",
            &[],
        )?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    judge(&pinned_migrations(), &applied).map_err(|Conflict| tl3_store::Error::MigrationMismatch)
}

/// The change feed follows the same newer-store policy as read sessions.
pub(crate) enum Listener {
    Pinned(tl3_store::Listener),
    Ahead(Client),
}

impl Listener {
    pub(crate) fn open(store: &tl3_config::Store) -> tl3_store::Result<Self> {
        match tl3_store::Listener::open(store) {
            Ok(listener) => Ok(Self::Pinned(listener)),
            Err(tl3_store::Error::SchemaVersion { actual, expected }) if actual > expected => {
                // Match TL3's LISTEN keepalives so dead peers trigger reconnection.
                let mut client = config(store)?
                    .keepalives(true)
                    .keepalives_idle(Duration::from_secs(60))
                    .keepalives_interval(Duration::from_secs(10))
                    .keepalives_retries(3)
                    .connect(NoTls)?;
                let extra = Session::check_ahead(&mut client)?;
                eprintln!("console change feed: {}", warning(&extra));
                client.batch_execute("LISTEN tl3_changes")?;
                Ok(Self::Ahead(client))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn wait(&mut self) -> tl3_store::Result<std::collections::BTreeSet<String>> {
        use postgres::fallible_iterator::FallibleIterator;
        let client = match self {
            Self::Pinned(listener) => return listener.wait(),
            Self::Ahead(client) => client,
        };
        let mut notifications = client.notifications();
        let Some(first) = notifications.blocking_iter().next()? else {
            return Err(tl3_store::Error::Invalid("change feed session closed"));
        };
        let mut tables = std::collections::BTreeSet::from([first.payload().to_string()]);
        let mut pending = notifications.iter();
        while let Some(next) = pending.next()? {
            tables.insert(next.payload().to_string());
        }
        Ok(tables)
    }
}

/// Command filing uses the same checked compatibility path as the readers.
pub(crate) enum ConsoleCommands {
    Pinned(tl3_store::ConsoleCommands),
    Ahead(Client),
}

impl ConsoleCommands {
    pub(crate) fn open(store: &tl3_config::Store) -> tl3_store::Result<Self> {
        match tl3_store::ConsoleCommands::open(store) {
            Ok(commands) => Ok(Self::Pinned(commands)),
            Err(tl3_store::Error::SchemaVersion { actual, expected }) if actual > expected => {
                let mut client = connect(store)?;
                let extra = Session::check_ahead(&mut client)?;
                eprintln!("console command filer: {}", warning(&extra));
                Ok(Self::Ahead(client))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn submit(
        &mut self,
        id: &str,
        node: u64,
        action: &serde_json::Value,
    ) -> tl3_store::Result<bool> {
        let client = match self {
            Self::Pinned(commands) => return commands.submit(id, node, action),
            Self::Ahead(client) => client,
        };
        let node =
            i64::try_from(node).map_err(|_| tl3_store::Error::Invalid("node id out of range"))?;
        let mut tx = client.build_transaction().read_only(false).start()?;
        let filed = tx
            .query_one(
                "SELECT tl3_console_command($1, $2, $3)",
                &[&id, &node, action],
            )?
            .get(0);
        tx.commit()?;
        Ok(filed)
    }
}

/// Connect the way the pinned TL3's reader does.
fn connect(settings: &tl3_config::Store) -> tl3_store::Result<Client> {
    Ok(config(settings)?.connect(NoTls)?)
}

fn config(settings: &tl3_config::Store) -> tl3_store::Result<postgres::Config> {
    settings.validate()?;
    let mut connection = postgres::Config::new();
    connection
        .host(&settings.host)
        .port(settings.port)
        .dbname(&settings.database)
        .user(&settings.user)
        .connect_timeout(Duration::from_secs(settings.connect_timeout_secs.into()))
        .options("-c default_transaction_read_only=on");
    if let Some(path) = &settings.password_file {
        let password = fs::read(path).map_err(tl3_store::Error::Password)?;
        let password = password.strip_suffix(b"\n").unwrap_or(&password);
        let password = password.strip_suffix(b"\r").unwrap_or(password);
        connection.password(password);
    }
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrations(versions: &[(i32, &str)]) -> Vec<Migration> {
        versions
            .iter()
            .map(|&(version, md5)| (version, md5.into()))
            .collect()
    }

    #[test]
    fn an_equal_store_has_nothing_extra() {
        let pinned = migrations(&[(1, "a"), (2, "b")]);
        assert_eq!(judge(&pinned, &pinned), Ok(vec![]));
    }

    #[test]
    fn newer_versions_are_extra() {
        let pinned = migrations(&[(1, "a"), (2, "b")]);
        let applied = migrations(&[(1, "a"), (2, "b"), (3, "c"), (4, "d")]);
        assert_eq!(judge(&pinned, &applied), Ok(vec![3, 4]));
    }

    #[test]
    fn a_missing_pinned_version_conflicts() {
        let pinned = migrations(&[(1, "a"), (2, "b")]);
        assert_eq!(
            judge(&pinned, &migrations(&[(1, "a"), (3, "c")])),
            Err(Conflict)
        );
        assert_eq!(judge(&pinned, &migrations(&[(1, "a")])), Err(Conflict));
    }

    #[test]
    fn a_pinned_version_with_other_sql_conflicts() {
        let pinned = migrations(&[(1, "a"), (2, "b")]);
        let applied = migrations(&[(1, "a"), (2, "changed"), (3, "c")]);
        assert_eq!(judge(&pinned, &applied), Err(Conflict));
    }

    #[test]
    fn the_warning_names_the_versions_and_the_pin_bump() {
        let text = warning(&[11, 12]);
        assert!(text.contains("11") && text.contains("12"), "{text}");
        assert!(text.contains("pin"), "{text}");
    }
}
