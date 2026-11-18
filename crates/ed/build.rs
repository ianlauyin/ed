use std::env;
fn main() {
    assert_eq!(env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("macos"), "This editor only supports macOS.");
    // Sets the minimum macOS of the linked binary; keep in sync with Info.plist and the gpui_apple shader build.
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=27.0");

    // Seems to be required to enable Swift concurrency
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    // Register exported Objective-C selectors, protocols, etc
    println!("cargo:rustc-link-arg=-Wl,-ObjC");
}
