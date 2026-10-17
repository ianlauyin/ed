const ZED_MANIFEST: &str = include_str!("../zed/Cargo.toml");

fn main() {
    let zed_cargo_toml =
        toml_edit::ImDocument::parse(ZED_MANIFEST).expect("failed to parse zed Cargo.toml");
    let version = zed_cargo_toml
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml_edit::Item::as_str)
        .expect("zed Cargo.toml must declare a package version string");
    println!("cargo:rustc-env=ZED_PKG_VERSION={version}");
}
