use std::str::FromStr;
use std::sync::{Arc, LazyLock};

use git::{
    BuildCommitPermalinkParams, BuildPermalinkParams, GitHostingProvider, ParsedGitRemote,
    PullRequest, RemoteUrl,
};
use regex::Regex;
use url::Url;

const CHROMIUM_REVIEW_URL: &str = "https://chromium-review.googlesource.com";

/// Parses Gerrit URLs like
/// https://chromium-review.googlesource.com/c/chromium/src/+/3310961.
fn pull_request_regex() -> &'static Regex {
    static PULL_REQUEST_NUMBER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r#"Reviewed-on: ({CHROMIUM_REVIEW_URL}/c/(.*)/\+/(\d+))"#
        ))
        .unwrap()
    });
    &PULL_REQUEST_NUMBER_REGEX
}

pub struct Chromium;

impl GitHostingProvider for Chromium {
    fn name(&self) -> String {
        "Chromium".to_string()
    }

    fn base_url(&self) -> Url {
        Url::parse("https://chromium.googlesource.com").unwrap()
    }

    fn format_line_number(&self, line: u32) -> String {
        format!("{line}")
    }

    fn format_line_numbers(&self, start_line: u32, _end_line: u32) -> String {
        format!("{start_line}")
    }

    fn parse_remote_url(&self, url: &str) -> Option<ParsedGitRemote> {
        let url = RemoteUrl::from_str(url).ok()?;

        let host = url.host_str()?;
        if host != self.base_url().host_str()? {
            return None;
        }

        let path_segments = url.path_segments()?.collect::<Vec<_>>();
        let joined_path = path_segments.join("/");
        let repo = joined_path.trim_end_matches(".git");

        Some(ParsedGitRemote {
            owner: Arc::from(""),
            repo: repo.into(),
        })
    }

    fn build_commit_permalink(
        &self,
        remote: &ParsedGitRemote,
        params: BuildCommitPermalinkParams,
    ) -> Url {
        let BuildCommitPermalinkParams { sha } = params;
        let ParsedGitRemote { owner: _, repo } = remote;

        self.base_url().join(&format!("{repo}/+/{sha}")).unwrap()
    }

    fn build_permalink(&self, remote: ParsedGitRemote, params: BuildPermalinkParams) -> Url {
        let ParsedGitRemote { owner: _, repo } = remote;
        let BuildPermalinkParams {
            sha,
            path,
            selection,
        } = params;

        let mut permalink = self
            .base_url()
            .join(&format!("{repo}/+/{sha}/{path}"))
            .unwrap();
        permalink.set_fragment(
            selection
                .map(|selection| self.line_fragment(&selection))
                .as_deref(),
        );
        permalink
    }

    fn extract_pull_request(&self, remote: &ParsedGitRemote, message: &str) -> Option<PullRequest> {
        let capture = pull_request_regex().captures(message)?;
        let url = Url::parse(capture.get(1)?.as_str()).unwrap();
        let repo = capture.get(2)?.as_str();
        if repo != remote.repo.as_ref() {
            return None;
        }

        let number = capture.get(3)?.as_str().parse::<u32>().ok()?;

        Some(PullRequest { number, url })
    }
}
