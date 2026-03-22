use std::fs;
use std::process::Command;

fn main() {
    // Re-run when git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    // Git short hash
    let hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Dirty check
    let dirty = Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);

    // Build date (YYYY-MM-DD)
    let date = Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Auto-incrementing build number (persisted in .build-number)
    let build_number_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build-number");
    let current: u64 = fs::read_to_string(&build_number_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let next = current + 1;
    let _ = fs::write(&build_number_path, next.to_string());

    println!("cargo:rustc-env=ATERM_GIT_HASH={}", hash);
    println!("cargo:rustc-env=ATERM_BUILD_DATE={}", date);
    println!("cargo:rustc-env=ATERM_DIRTY={}", dirty);
    println!("cargo:rustc-env=ATERM_BUILD_NUMBER={}", next);
}
