//! Which stores the console accepts. TL3's migrations only add, so a store
//! that applied everything the pinned TL3 knows, plus newer versions, still
//! has every table and column the pinned board projection reads.

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
/// pinned version or records one with different SQL.
pub(crate) fn judge(pinned: &[Migration], applied: &[Migration]) -> Result<Vec<i32>, Conflict> {
    let _ = (pinned, applied);
    todo!("accept stores ahead of the pin")
}

/// What to log when a store is ahead of the pin.
pub fn warning(extra: &[i32]) -> String {
    let _ = extra;
    todo!("name the extra versions and suggest a pin bump")
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
