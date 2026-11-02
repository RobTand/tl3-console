//! Repositories the console never shows (`hide_repos` in its config).
//!
//! The board's builder drops a hidden repository's requests, and the
//! input-failure incidents on its issues, before it projects the board, so the
//! snapshot, every patch and every count already leave them out. The case file
//! and command routes answer 404 for its requests. Nothing is filtered in the
//! browser.

use std::collections::BTreeSet;
use tl3_store::Transaction;

/// `owner/name` of each hidden repository, lowercased: GitHub ignores case,
/// and so does TL3's own repository identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenRepos(BTreeSet<String>);

impl HiddenRepos {
    /// Every name must be one of the repositories TL3 serves, so that a typo
    /// fails `tl3-console check` instead of hiding nothing.
    pub fn new(names: &[String], served: &[tl3_config::Repository]) -> crate::Result<Self> {
        let served: BTreeSet<String> = served
            .iter()
            .map(|repo| format!("{}/{}", repo.owner, repo.name).to_ascii_lowercase())
            .collect();
        let mut hidden = BTreeSet::new();
        for name in names {
            let key = name.to_ascii_lowercase();
            if !served.contains(&key) {
                return Err(crate::config::invalid(
                    "hide_repos",
                    format!(
                        "{name:?} is not a repository TL3 serves (github.repos in tl3.toml); \
                         name one as owner/name"
                    ),
                ));
            }
            hidden.insert(key);
        }
        Ok(Self(hidden))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `repo`, as `owner/name`, is hidden.
    pub fn contains(&self, repo: &str) -> bool {
        self.0.contains(&repo.to_ascii_lowercase())
    }

    /// Whether request `id` belongs to a hidden repository, read by key
    /// inside the caller's transaction.
    pub(crate) fn hides(&self, tx: &mut Transaction<'_>, id: u64) -> tl3_store::Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        let binding = tx.query_opt(
            "SELECT value #>> '{repo,owner}', value #>> '{repo,name}' FROM tl3_world
                 WHERE table_name = 'requests' AND row_id = $1",
            &[&id.to_string()],
        )?;
        Ok(binding.is_some_and(|row| {
            let field = |index| row.get::<_, Option<String>>(index).unwrap_or_default();
            self.contains(&format!("{}/{}", field(0), field(1)))
        }))
    }

    /// Every request of a hidden repository, including ones a newer request
    /// for the same issue replaced: their commands stay hidden too.
    pub(crate) fn requests(&self, tx: &mut Transaction<'_>) -> tl3_store::Result<BTreeSet<u64>> {
        Ok(self
            .rows(tx, "requests", ["repo", "owner"], ["repo", "name"])?
            .iter()
            .filter_map(|id| id.parse().ok())
            .collect())
    }

    /// Input-failure incidents on a hidden repository's issues. They belong to
    /// no request, so the board lists them apart, but they name the repository.
    pub(crate) fn incidents(
        &self,
        tx: &mut Transaction<'_>,
    ) -> tl3_store::Result<BTreeSet<String>> {
        self.rows(
            tx,
            "incidents",
            ["input_failure", "target", "repository", "owner"],
            ["input_failure", "target", "repository", "name"],
        )
        .map(|ids| ids.into_iter().collect())
    }

    /// The ids of `table`'s rows whose repository, read at the JSON paths,
    /// is hidden. GitHub ignores case, so the store's side is lowercased.
    fn rows<const N: usize>(
        &self,
        tx: &mut Transaction<'_>,
        table: &str,
        owner: [&str; N],
        name: [&str; N],
    ) -> tl3_store::Result<Vec<String>> {
        if self.is_empty() {
            return Ok(Vec::new());
        }
        let names: Vec<&str> = self.0.iter().map(String::as_str).collect();
        Ok(tx
            .query(
                "SELECT row_id FROM tl3_world WHERE table_name = $1
                 AND lower((value #>> $2::text[]) || '/' || (value #>> $3::text[])) = ANY($4)",
                &[
                    &table,
                    &owner.as_slice(),
                    &name.as_slice(),
                    &names.as_slice(),
                ],
            )?
            .iter()
            .map(|row| row.get(0))
            .collect())
    }
}
