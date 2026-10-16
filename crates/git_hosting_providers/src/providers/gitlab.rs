use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use regex::Regex;
use url::Url;
use urlencoding::encode;

use git::{
    BuildCommitPermalinkParams, BuildPermalinkParams, GitHostingProvider, ParsedGitRemote,
    PullRequest, RemoteUrl,
};

fn merge_request_number_regex() -> &'static Regex {
    static MERGE_REQUEST_NUMBER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        // Matches GitLab MR references:
        // - "(!123)" at the end of line (squash merge pattern)
        // - "See merge request group/project!123" (standard merge commit)
        Regex::new(r"(?:\(!(\d+)\)$|See merge request [^\s]+!(\d+))").unwrap()
    });
    &MERGE_REQUEST_NUMBER_REGEX
}

use crate::get_host_from_git_remote_url;

#[derive(Debug)]
pub struct Gitlab {
    name: String,
    base_url: Url,
}

impl Gitlab {
    pub fn new(name: impl Into<String>, base_url: Url) -> Self {
        Self {
            name: name.into(),
            base_url,
        }
    }

    pub fn public_instance() -> Self {
        Self::new("GitLab", Url::parse("https://gitlab.com").unwrap())
    }

    pub fn from_remote_url(remote_url: &str) -> Result<Self> {
        let host = get_host_from_git_remote_url(remote_url)?;
        if host == "gitlab.com" {
            bail!("the GitLab instance is not self-hosted");
        }

        // TODO: detecting self hosted instances by checking whether "gitlab" is in the url or not
        // is not very reliable. See https://github.com/zed-industries/zed/issues/26393 for more
        // information.
        if !host.contains("gitlab") {
            bail!("not a GitLab URL");
        }

        Ok(Self::new(
            "GitLab Self-Hosted",
            Url::parse(&format!("https://{}", host))?,
        ))
    }
}

impl GitHostingProvider for Gitlab {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    fn format_line_number(&self, line: u32) -> String {
        format!("L{line}")
    }

    fn format_line_numbers(&self, start_line: u32, end_line: u32) -> String {
        format!("L{start_line}-{end_line}")
    }

    fn parse_remote_url(&self, url: &str) -> Option<ParsedGitRemote> {
        let url = RemoteUrl::from_str(url).ok()?;

        let host = url.host_str()?;
        if host != self.base_url.host_str()? {
            return None;
        }

        let mut path_segments = url.path_segments()?.collect::<Vec<_>>();
        let repo = path_segments.pop()?.trim_end_matches(".git");
        let owner = path_segments.join("/");

        Some(ParsedGitRemote {
            owner: owner.into(),
            repo: repo.into(),
        })
    }

    fn build_commit_permalink(
        &self,
        remote: &ParsedGitRemote,
        params: BuildCommitPermalinkParams,
    ) -> Url {
        let BuildCommitPermalinkParams { sha } = params;
        let ParsedGitRemote { owner, repo } = remote;

        self.base_url()
            .join(&format!("{owner}/{repo}/-/commit/{sha}"))
            .unwrap()
    }

    fn build_permalink(&self, remote: ParsedGitRemote, params: BuildPermalinkParams) -> Url {
        let ParsedGitRemote { owner, repo } = remote;
        let BuildPermalinkParams {
            sha,
            path,
            selection,
        } = params;

        let mut permalink = self
            .base_url()
            .join(&format!("{owner}/{repo}/-/blob/{sha}/{path}"))
            .unwrap();
        if path.ends_with(".md") {
            permalink.set_query(Some("plain=1"));
        }
        permalink.set_fragment(
            selection
                .map(|selection| self.line_fragment(&selection))
                .as_deref(),
        );
        permalink
    }

    fn build_create_pull_request_url(
        &self,
        remote: &ParsedGitRemote,
        source_branch: &str,
    ) -> Option<Url> {
        let mut url = self
            .base_url()
            .join(&format!(
                "{}/{}/-/merge_requests/new",
                remote.owner, remote.repo
            ))
            .ok()?;

        let query = format!("merge_request%5Bsource_branch%5D={}", encode(source_branch));

        url.set_query(Some(&query));
        Some(url)
    }

    fn extract_pull_request(&self, remote: &ParsedGitRemote, message: &str) -> Option<PullRequest> {
        // Check commit message for GitLab MR references
        let capture = merge_request_number_regex().captures(message)?;
        // The regex has two capture groups - one for "(!123)" pattern, one for "See merge request" pattern
        let number = capture
            .get(1)
            .or_else(|| capture.get(2))?
            .as_str()
            .parse::<u32>()
            .ok()?;

        let mut url = self.base_url();
        let path = format!(
            "{}/{}/-/merge_requests/{}",
            remote.owner, remote.repo, number
        );
        url.set_path(&path);

        Some(PullRequest { number, url })
    }
}
