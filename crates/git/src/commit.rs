use crate::{Oid, repository::GitBinary, status::StatusCode};
use anyhow::{Context as _, Result};
use collections::{HashMap, HashSet};
use gpui::SharedString;
use std::str::FromStr;

#[derive(Clone, Debug, Default)]
pub struct ParsedCommitMessage {
    pub message: SharedString,
}

pub(crate) async fn get_messages(git: &GitBinary, shas: &[Oid]) -> Result<HashMap<Oid, String>> {
    if shas.is_empty() {
        return Ok(HashMap::default());
    }

    let output = { get_messages_impl(git, shas).await? };

    Ok(shas
        .iter()
        .cloned()
        .zip(output)
        .collect::<HashMap<Oid, String>>())
}

pub(crate) async fn get_tag_names(
    git: &GitBinary,
    shas: &[Oid],
) -> Result<HashMap<Oid, Vec<String>>> {
    if shas.is_empty() {
        return Ok(HashMap::default());
    }

    let output = git
        .build_command(&[
            "for-each-ref",
            "refs/tags",
            "--sort=-creatordate",
            "--format=%(objectname)%00%(*objectname)%00%(refname:short)",
        ])
        .output()
        .await
        .context("starting git for-each-ref process")?;
    anyhow::ensure!(
        output.status.success(),
        "'git for-each-ref' failed with error {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(parse_tag_names(
        &String::from_utf8_lossy(&output.stdout),
        shas,
    ))
}

fn parse_tag_names(output: &str, shas: &[Oid]) -> HashMap<Oid, Vec<String>> {
    let shas = shas.iter().copied().collect::<HashSet<_>>();
    let mut result = HashMap::<Oid, Vec<String>>::default();

    for line in output.lines() {
        let mut fields = line.split('\0');
        let object_sha = fields.next();
        let peeled_sha = fields.next().filter(|sha| !sha.is_empty());
        let Some(sha) = peeled_sha
            .or(object_sha)
            .and_then(|sha| Oid::from_str(sha).ok())
        else {
            continue;
        };
        let Some(tag_name) = fields.next().filter(|tag_name| !tag_name.is_empty()) else {
            continue;
        };
        result.entry(sha).or_default().push(tag_name.to_string());
    }

    result.retain(|sha, _| shas.contains(sha));
    result
}

async fn get_messages_impl(git: &GitBinary, shas: &[Oid]) -> Result<Vec<String>> {
    const MARKER: &str = "<MARKER>";
    let output = git
        .build_command(&["show"])
        .arg("-s")
        .arg(format!("--format=%B{}", MARKER))
        .args(shas.iter().map(ToString::to_string))
        .output()
        .await
        .context("starting git show process")?;
    anyhow::ensure!(
        output.status.success(),
        "'git show' failed with error {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .split_terminator(MARKER)
        .map(|str| str.trim().replace("<", "&lt;").replace(">", "&gt;"))
        .collect::<Vec<_>>())
}

pub(crate) const GITLINK_MODE: &str = "160000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitDiffObjectKind {
    Blob,
    Gitlink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CommitDiffObject<'a> {
    pub oid: &'a str,
    pub kind: CommitDiffObjectKind,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CommitDiffEntry<'a> {
    pub path: &'a str,
    pub status: StatusCode,
    pub old_object: Option<CommitDiffObject<'a>>,
    pub new_object: Option<CommitDiffObject<'a>>,
}

/// Parses the output of `git diff --raw --no-abbrev -z`.
pub(crate) fn parse_git_diff_raw(
    content: &str,
) -> impl Iterator<Item = Result<CommitDiffEntry<'_>>> {
    let mut parts = content.split('\0');
    std::iter::from_fn(move || {
        let metadata = parts.next()?;
        if metadata.is_empty() {
            return None;
        }

        let path = match parts.next() {
            Some(path) => path,
            None => return Some(Err(anyhow::anyhow!("raw diff is missing the path"))),
        };
        Some(parse_git_diff_raw_entry(metadata, path))
    })
}

fn parse_git_diff_raw_entry<'a>(metadata: &'a str, path: &'a str) -> Result<CommitDiffEntry<'a>> {
    let mut fields = metadata
        .strip_prefix(':')
        .context("raw diff metadata is missing its ':' prefix")?
        .split_ascii_whitespace();
    let old_mode = fields.next().context("raw diff is missing the old mode")?;
    let new_mode = fields.next().context("raw diff is missing the new mode")?;
    let old_oid = fields
        .next()
        .context("raw diff is missing the old object ID")?;
    let new_oid = fields
        .next()
        .context("raw diff is missing the new object ID")?;
    let status = match fields.next() {
        Some("M") => StatusCode::Modified,
        Some("T") => StatusCode::TypeChanged,
        Some("A") => StatusCode::Added,
        Some("D") => StatusCode::Deleted,
        Some(status) => anyhow::bail!("unsupported raw diff status {status}"),
        None => anyhow::bail!("raw diff is missing the status"),
    };

    Ok(CommitDiffEntry {
        path,
        status,
        old_object: (!old_oid.bytes().all(|byte| byte == b'0')).then(|| CommitDiffObject {
            oid: old_oid,
            kind: if old_mode == GITLINK_MODE {
                CommitDiffObjectKind::Gitlink
            } else {
                CommitDiffObjectKind::Blob
            },
        }),
        new_object: (!new_oid.bytes().all(|byte| byte == b'0')).then(|| CommitDiffObject {
            oid: new_oid,
            kind: if new_mode == GITLINK_MODE {
                CommitDiffObjectKind::Gitlink
            } else {
                CommitDiffObjectKind::Blob
            },
        }),
    })
}
