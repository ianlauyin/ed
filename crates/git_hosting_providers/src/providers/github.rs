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

use crate::get_host_from_git_remote_url;

fn pull_request_number_regex() -> &'static Regex {
    static PULL_REQUEST_NUMBER_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\(#(\d+)\)$").unwrap());
    &PULL_REQUEST_NUMBER_REGEX
}

#[derive(Debug)]
pub struct Github {
    name: String,
    base_url: Url,
}

impl Github {
    pub fn new(name: impl Into<String>, base_url: Url) -> Self {
        Self {
            name: name.into(),
            base_url,
        }
    }

    pub fn public_instance() -> Self {
        Self::new("GitHub", Url::parse("https://github.com").unwrap())
    }

    pub fn from_remote_url(remote_url: &str) -> Result<Self> {
        let host = get_host_from_git_remote_url(remote_url)?;
        if host == "github.com" {
            bail!("the GitHub instance is not self-hosted");
        }

        // TODO: detecting self hosted instances by checking whether "github" is in the url or not
        // is not very reliable. See https://github.com/zed-industries/zed/issues/26393 for more
        // information.
        if !host.contains("github") {
            bail!("not a GitHub URL");
        }

        Ok(Self::new(
            "GitHub Self-Hosted",
            Url::parse(&format!("https://{}", host))?,
        ))
    }
}

impl GitHostingProvider for Github {
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
        format!("L{start_line}-L{end_line}")
    }

    fn parse_remote_url(&self, url: &str) -> Option<ParsedGitRemote> {
        let url = RemoteUrl::from_str(url).ok()?;

        let host = url.host_str()?;
        if host != self.base_url.host_str()? {
            return None;
        }

        let mut path_segments = url.path_segments()?;
        let mut owner = path_segments.next()?;
        if owner.is_empty() {
            owner = path_segments.next()?;
        }

        let repo = path_segments.next()?.trim_end_matches(".git");

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
            .join(&format!("{owner}/{repo}/commit/{sha}"))
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
            .join(&format!("{owner}/{repo}/blob/{sha}/{path}"))
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
        let ParsedGitRemote { owner, repo } = remote;
        let encoded_source = encode(source_branch);

        self.base_url()
            .join(&format!("{owner}/{repo}/pull/new/{encoded_source}"))
            .ok()
    }

    fn extract_pull_request(&self, remote: &ParsedGitRemote, message: &str) -> Option<PullRequest> {
        let line = message.lines().next()?;
        let capture = pull_request_number_regex().captures(line)?;
        let number = capture.get(1)?.as_str().parse::<u32>().ok()?;

        let mut url = self.base_url();
        let path = format!("/{}/{}/pull/{}", remote.owner, remote.repo, number);
        url.set_path(&path);

        Some(PullRequest { number, url })
    }
}
