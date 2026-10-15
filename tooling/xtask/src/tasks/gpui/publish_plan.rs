use std::fmt;

use anyhow::{Context as _, Result, bail, ensure};
use guppy::graph::{DependencyDirection, ExternalSource, PackageGraph, PackageMetadata};
use url::Url;

use crate::tasks::gpui::{GitSha, GpuiArgs};

pub enum Repository {
    Workspace,
    Git(GitRepository),
}

pub struct GitRepository {
    pub owner: String,
    pub repo: String,
    pub revision: GitSha,
}

impl GitRepository {
    fn new(repository: &str, revision: &str) -> Result<Self> {
        let url = Url::parse(repository).context("invalid Git repository URL")?;
        ensure!(
            url.host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case("github.com")),
            "expected a GitHub repository, got {repository}"
        );
        let (owner, repo) = url
            .path()
            .trim_matches('/')
            .split_once('/')
            .context("repository URL must contain an owner and repository name")?;
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        ensure!(
            !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
            "expected an owner/repo path, got {repository}"
        );
        let revision = revision
            .parse::<GitSha>()
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("invalid resolved revision for {repository}"))?;

        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            revision,
        })
    }
}

impl Repository {
    pub fn fork(repository: &str, revision: &str) -> Result<Self> {
        Ok(Self::Git(GitRepository::new(repository, revision)?))
    }

    pub fn publish_info<'a>(&'a self, name: &'a str) -> CratePublishInfo<'a> {
        CratePublishInfo {
            repository: self,
            original_name: name,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Workspace => "zed-industries/zed".to_owned(),
            Self::Git(repository) => format!(
                "{owner}/{repo}",
                owner = repository.owner,
                repo = repository.repo
            ),
        }
    }

    pub fn target_name(&self, name: &str) -> String {
        match self {
            Self::Workspace => {
                if name == "gpui_macros" {
                    "gpui-macros".to_owned()
                } else if name == "gpui" || name.starts_with("gpui_") {
                    name.to_owned()
                } else {
                    format!("gpui_{name}")
                }
            }
            Self::Git(_) => {
                if name.starts_with("zed-") {
                    name.to_owned()
                } else {
                    format!("zed-{name}")
                }
            }
        }
    }

    pub fn display<'a>(&'a self, args: &'a GpuiArgs) -> impl fmt::Display + 'a {
        let revision = match self {
            Self::Workspace => &args.sha,
            Self::Git(repository) => &repository.revision,
        };
        fmt::from_fn(move |formatter| {
            write!(
                formatter,
                "{name}@{revision}",
                name = self.name(),
                revision = revision.as_ref()
            )
        })
    }
}

pub struct CratePublishInfo<'a> {
    pub repository: &'a Repository,
    pub original_name: &'a str,
}

pub struct PublishPlanEntry<'graph> {
    pub package: PackageMetadata<'graph>,
    pub repository: Repository,
}

pub fn build_publish_plan(graph: &PackageGraph) -> Result<Vec<PublishPlanEntry<'_>>> {
    let packages = super::crate_graph::gpui_crates(graph)?;
    packages
        .packages(DependencyDirection::Reverse)
        .map(|package| {
            let repository = if package.in_workspace() {
                Repository::Workspace
            } else if let Some(ExternalSource::Git {
                repository,
                resolved,
                ..
            }) = package.source().parse_external()
            {
                Repository::fork(repository, resolved)?
            } else {
                bail!(
                    "unexpected source for GPUI crate {}: {}",
                    package.name(),
                    package.source()
                );
            };
            Ok(PublishPlanEntry {
                package,
                repository,
            })
        })
        .collect()
}
