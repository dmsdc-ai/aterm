fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_language(cbindgen::Language::C)
        .with_include_guard("ATERM_CORE_H")
        .with_no_includes()
        .with_sys_include("stdint.h")
        .with_sys_include("stdbool.h")
        .with_sys_include("stddef.h")
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("aterm_core.h");

    // Core Text font fallback (macOS only)
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=src/coretext_fallback.c");
        cc::Build::new()
            .file(format!("{}/src/coretext_fallback.c", crate_dir))
            .compile("coretext_fallback");
        println!("cargo:rustc-link-lib=framework=CoreText");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
