fn main() {
    assert_eq!(std::env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("macos"), "This editor only supports macOS.");
    println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=15.3");

    // Seems to be required to enable Swift concurrency
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    // Register exported Objective-C selectors, protocols, etc
    println!("cargo:rustc-link-arg=-Wl,-ObjC");
}
