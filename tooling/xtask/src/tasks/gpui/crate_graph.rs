use anyhow::{Context as _, Result, ensure};
use guppy::{
    MetadataCommand,
    graph::{
        DependencyDirection, ExternalSource, PackageGraph, PackageMetadata, PackageSet,
        PackageSource,
    },
};
use url::Url;

pub fn load_workspace_graph() -> Result<PackageGraph> {
    MetadataCommand::new()
        .build_graph()
        .context("failed to load the workspace crate graph")
}

pub fn is_gpui_root(package: PackageMetadata<'_>) -> bool {
    package.in_workspace() && package.name().starts_with("gpui")
}

/// Returns all GPUI crates in the current workspace
pub fn gpui_crates(graph: &PackageGraph) -> Result<PackageSet<'_>> {
    let roots = graph
        .workspace()
        .iter_by_name()
        .filter(|(_, package)| is_gpui_root(*package))
        .map(|(_, package)| package.id())
        .collect::<Vec<_>>();
    ensure!(!roots.is_empty(), "no GPUI crates found in the workspace");

    let dependencies = graph
        .query_forward(roots)?
        // we do not need dev dependencies for releases
        .resolve_with_fn(|_, link| !link.dev_only());

    // Keep all workspace crates and Zed forks
    Ok(
        dependencies.filter(DependencyDirection::Reverse, |package| {
            package.in_workspace() || is_zed_fork(package.source())
        }),
    )
}

pub fn explain_crate<'graph>(
    graph: &'graph PackageGraph,
    name: &str,
) -> Result<PackageSet<'graph>> {
    let crates = gpui_crates(graph)?;
    let selected = crates.filter(DependencyDirection::Forward, |package| {
        package.name() == name
    });
    ensure!(
        !selected.is_empty(),
        "crate `{name}` is not in the GPUI publish list"
    );

    let dependents = selected
        .to_package_query(DependencyDirection::Reverse)
        .resolve_with_fn(|_, link| !link.dev_only());

    let dependencies = crates
        .to_package_query(DependencyDirection::Forward)
        .resolve_with_fn(|_, link| !link.dev_only());
    Ok(dependents.intersection(&dependencies))
}

fn is_zed_fork(source: PackageSource<'_>) -> bool {
    let Some(ExternalSource::Git { repository, .. }) = source.parse_external() else {
        return false;
    };

    Url::parse(repository).is_ok_and(|repository| {
        repository
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("github.com"))
            && repository.path_segments().is_some_and(|mut segments| {
                segments
                    .next()
                    .is_some_and(|owner| owner.eq_ignore_ascii_case("zed-industries"))
                    && segments.next().is_some_and(|name| !name.is_empty())
            })
    })
}
