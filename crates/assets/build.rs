fn main() {
    // Release builds embed `assets/` via `util::fs_embed!`, whose generated
    // `include_bytes!` only tracks files that already existed. Watch the whole
    // directory so added or removed assets (e.g. a new icon) trigger a rebuild.
    println!("cargo::rerun-if-changed=../../assets");
}
