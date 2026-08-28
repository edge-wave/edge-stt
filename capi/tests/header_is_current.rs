//! The committed header must match what the source produces. Ignored
//! by default because it needs cbindgen installed.

use std::process::Command;

#[test]
#[ignore = "needs cbindgen"]
fn the_committed_header_matches_the_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the workspace");

    let made = Command::new("cbindgen")
        .current_dir(root)
        .args(["--config", "capi/cbindgen.toml", "--crate", "edge-stt-capi"])
        .output()
        .expect("cbindgen must be installed: cargo install cbindgen");
    assert!(
        made.status.success(),
        "cbindgen failed: {}",
        String::from_utf8_lossy(&made.stderr)
    );

    let fresh = String::from_utf8_lossy(&made.stdout);
    let committed =
        std::fs::read_to_string(root.join("capi/include/edge_stt.h")).expect("a committed header");

    if fresh.trim() != committed.trim() {
        let first = fresh
            .lines()
            .zip(committed.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(n, (a, b))| (n + 1, a.to_string(), b.to_string()));
        panic!(
            "the header no longer matches the source. Regenerate it:\n  \
             cbindgen --config capi/cbindgen.toml --crate edge-stt-capi \\\n    \
             --output capi/include/edge_stt.h\n\nfirst difference: {first:?}"
        );
    }
}
