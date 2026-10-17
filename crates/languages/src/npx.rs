use language::LspAdapterDelegate;
use lsp::LanguageServerBinary;
use std::sync::Arc;

/// Resolves a Node-based language server: `bin` on the shell `PATH`, otherwise
/// the latest `packages` run through `npx`, which prefers a version installed
/// in the project and otherwise downloads into npm's cache on first use.
pub(crate) async fn node_server_binary(
    delegate: &Arc<dyn LspAdapterDelegate>,
    packages: &[&str],
    bin: &str,
    arguments: &[&str],
) -> Option<LanguageServerBinary> {
    let env = delegate.shell_env().await;
    if let Some(path) = delegate.which(bin.as_ref()).await {
        return Some(LanguageServerBinary {
            path,
            env: Some(env),
            arguments: arguments.iter().map(Into::into).collect(),
        });
    }

    let npx = delegate.which("npx".as_ref()).await?;
    Some(LanguageServerBinary {
        path: npx,
        env: Some(env),
        arguments: npx_arguments(packages, bin, arguments),
    })
}

fn npx_arguments(packages: &[&str], bin: &str, arguments: &[&str]) -> Vec<std::ffi::OsString> {
    let mut args = vec!["--yes".into()];
    for package in packages {
        args.push("--package".into());
        args.push(package.into());
    }
    args.push(bin.into());
    args.extend(arguments.iter().map(Into::into));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_bin_from_requested_packages() {
        assert_eq!(
            npx_arguments(
                &["typescript-language-server", "typescript"],
                "typescript-language-server",
                &["--stdio"],
            ),
            [
                "--yes",
                "--package",
                "typescript-language-server",
                "--package",
                "typescript",
                "typescript-language-server",
                "--stdio",
            ]
        );
    }
}
