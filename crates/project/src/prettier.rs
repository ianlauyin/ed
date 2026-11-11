//! Built-in formatting via the Prettier CLI (`prettier --stdin-filepath <path>`).
//!
//! Under `formatter: "auto"`, a project is formatted by whichever of Biome and
//! Prettier it configures. Each format spawns a short-lived process.

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

const NPX_PACKAGE: &str = "prettier";

const CONFIG_FILES: &[&str] = &[
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.yaml",
    ".prettierrc.yml",
    ".prettierrc.json5",
    ".prettierrc.js",
    ".prettierrc.cjs",
    ".prettierrc.mjs",
    ".prettierrc.ts",
    ".prettierrc.cts",
    ".prettierrc.mts",
    ".prettierrc.toml",
    "prettier.config.js",
    "prettier.config.cjs",
    "prettier.config.mjs",
    "prettier.config.ts",
    "prettier.config.cts",
    "prettier.config.mts",
];

const BIOME_CONFIG_FILES: &[&str] = &["biome.json", "biome.jsonc"];

/// The program and leading arguments used to invoke Prettier.
pub(crate) struct PrettierCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

fn has_prettier_config(dir: &Path) -> bool {
    CONFIG_FILES.iter().any(|name| dir.join(name).is_file()) || package_json_has_prettier(dir)
}

fn package_json_has_prettier(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { return false };
    serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|json| json.get("prettier").is_some())
}

/// The CLI formatter `formatter: "auto"` uses for web languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CliFormatter {
    Biome,
    Prettier,
}

/// The formatter configured by the nearest ancestor of `start_dir` holding a Biome or
/// Prettier config; Biome wins a tie. `None` when no ancestor has either, in which
/// case neither CLI formatter is used.
pub(crate) fn detect(start_dir: Option<&Path>) -> Option<CliFormatter> {
    start_dir.into_iter().flat_map(Path::ancestors).find_map(|dir| {
        if BIOME_CONFIG_FILES.iter().any(|name| dir.join(name).is_file()) {
            Some(CliFormatter::Biome)
        } else if has_prettier_config(dir) {
            Some(CliFormatter::Prettier)
        } else {
            None
        }
    })
}

/// Resolves Prettier, in order: the nearest `node_modules/.bin/prettier` above
/// `start_dir`, `prettier` on `PATH`, then `npx --yes prettier`.
pub(crate) fn resolve_command(start_dir: Option<&Path>, path_env: Option<&str>) -> PrettierCommand {
    let local = start_dir.into_iter().flat_map(Path::ancestors).find_map(|dir| {
        let candidate = dir.join("node_modules").join(".bin").join("prettier");
        candidate.is_file().then_some(candidate)
    });
    if let Some(program) = local {
        return PrettierCommand { program, args: Vec::new() };
    }

    let cwd = start_dir.unwrap_or_else(|| Path::new("/"));
    if let Ok(program) = which::which_in("prettier", path_env, cwd) {
        return PrettierCommand { program, args: Vec::new() };
    }

    PrettierCommand {
        program: which::which_in("npx", path_env, cwd).unwrap_or_else(|_| "npx".into()),
        args: vec!["--yes".into(), NPX_PACKAGE.into()],
    }
}

/// The working directory to run Prettier in: the nearest ancestor of `start_dir`
/// holding a Prettier config (so `.prettierignore` next to it applies), else `start_dir`.
pub(crate) fn working_dir(start_dir: &Path) -> PathBuf {
    start_dir.ancestors().find(|dir| has_prettier_config(dir)).unwrap_or(start_dir).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prettier-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_follows_nearest_config() {
        let root = temp_dir("detect");
        let nested = root.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(detect(Some(&nested)), None);
        assert_eq!(detect(None), None);

        std::fs::write(root.join(".prettierrc"), "{}").unwrap();
        assert_eq!(detect(Some(&nested)), Some(CliFormatter::Prettier));

        std::fs::write(nested.join("biome.json"), "{}").unwrap();
        assert_eq!(detect(Some(&nested)), Some(CliFormatter::Biome));

        std::fs::write(nested.join(".prettierrc.json"), "{}").unwrap();
        assert_eq!(detect(Some(&nested)), Some(CliFormatter::Biome), "Biome wins a tie");
    }

    #[test]
    fn package_json_prettier_key_counts_as_config() {
        let root = temp_dir("package");
        std::fs::write(root.join("package.json"), r#"{"name":"x"}"#).unwrap();
        assert_eq!(detect(Some(&root)), None);
        std::fs::write(root.join("package.json"), r#"{"prettier":{"semi":false}}"#).unwrap();
        assert_eq!(detect(Some(&root)), Some(CliFormatter::Prettier));
        assert_eq!(working_dir(&root.join("a")), root);
    }

    #[test]
    fn working_dir_is_nearest_config_ancestor_or_start() {
        let root = temp_dir("workdir");
        let nested = root.join("src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(working_dir(&nested), nested);
        std::fs::write(root.join("prettier.config.mjs"), "").unwrap();
        assert_eq!(working_dir(&nested), root);
    }

    #[test]
    fn prefers_nearest_node_modules_bin() {
        let root = temp_dir("local");
        let bin = root.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prettier"), "").unwrap();
        let nested = root.join("packages/app/src");
        std::fs::create_dir_all(&nested).unwrap();

        let command = resolve_command(Some(&nested), Some(""));
        assert_eq!(command.program, bin.join("prettier"));
        assert!(command.args.is_empty());
    }

    #[test]
    fn falls_back_to_path_then_npx() {
        let root = temp_dir("fallback");
        let path_dir = root.join("path_bin");
        std::fs::create_dir_all(&path_dir).unwrap();
        let path_env = path_dir.to_string_lossy().to_string();

        let command = resolve_command(Some(&root), Some(&path_env));
        assert_eq!(command.program, PathBuf::from("npx"));
        assert_eq!(command.args, vec![OsString::from("--yes"), NPX_PACKAGE.into()]);

        let prettier = path_dir.join("prettier");
        std::fs::write(&prettier, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&prettier, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let command = resolve_command(Some(&root), Some(&path_env));
        assert_eq!(command.program, prettier);
        assert!(command.args.is_empty());
    }
}
