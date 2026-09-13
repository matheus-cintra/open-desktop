fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .files([
            "native/input.m",
            "native/app.m",
            "native/commands.m",
            "native/permissions.m",
        ])
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=26.0")
        .compile("opendesk_native");
    for framework in ["AppKit", "CoreGraphics", "Carbon", "ServiceManagement"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    println!("cargo:rerun-if-changed=native");
}
