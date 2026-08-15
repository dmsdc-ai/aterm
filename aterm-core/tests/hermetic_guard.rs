//! Integration tests link the lib built *without* `cfg(test)`, so their only
//! hermetic signal is `ATERM_HERMETIC` from `.cargo/config.toml`. Drop that
//! entry and every unit-test guard still passes while this binary quietly
//! regains the production daemon — so the entry gets its own guard here.

#[test]
fn hermetic_env_reaches_integration_tests() {
    assert!(
        std::env::var_os("ATERM_HERMETIC").is_some_and(|v| !v.is_empty()),
        "ATERM_HERMETIC not set — check the [env] block in .cargo/config.toml"
    );
}
