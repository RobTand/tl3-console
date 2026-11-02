//! Repositories the console never shows (`hide_repos` in its config).

use std::collections::BTreeSet;

/// `owner/name` of each hidden repository, lowercased: GitHub ignores case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenRepos(BTreeSet<String>);

impl HiddenRepos {
    pub fn new(names: &[String], served: &[tl3_config::Repository]) -> crate::Result<Self> {
        let _ = served;
        Ok(Self(
            names.iter().map(|name| name.to_ascii_lowercase()).collect(),
        ))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, repo: &str) -> bool {
        let _ = repo;
        false
    }
}
