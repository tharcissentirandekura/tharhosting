/*!
 * The parts of GitHub's webhook payloads that tharhosting reads.
 */

use serde::Deserialize;

/**
 * The relevant fields of a GitHub `push` event: which ref was pushed, to which
 * repository, and whether the push deleted the branch.
 */
#[derive(Debug, Deserialize)]
pub struct GithubPush {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub repository: GithubRepo,
    #[serde(default)]
    pub deleted: bool,
}

/**
 * The same GitHub repository, as the three URL forms its payload can carry.
 */
#[derive(Debug, Deserialize)]
pub struct GithubRepo {
    pub clone_url: Option<String>,
    pub html_url: Option<String>,
    pub ssh_url: Option<String>,
}

impl GithubRepo {
    /**
     * Iterates over whichever of the repository's URLs are present.
     */
    pub fn urls(&self) -> impl Iterator<Item = &str> {
        let available_formats = [&self.clone_url, &self.html_url, &self.ssh_url];
        available_formats.into_iter().flatten().map(String::as_str)
    }
}
