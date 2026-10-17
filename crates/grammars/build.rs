fn main() {
    // Release builds embed `src/` via `util::fs_embed!`, whose generated
    // `include_bytes!` only tracks files that already existed. Watch the whole
    // directory so added or removed query files trigger a rebuild.
    println!("cargo::rerun-if-changed=src");
}
