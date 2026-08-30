//! Compile the C surface test against the built library and run it.
//! If C cannot use this header, nothing else about it matters.

use std::path::PathBuf;
use std::process::Command;

fn target_directory() -> PathBuf {
    let mut here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.pop();
    here.join("target").join(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    })
}

/// The shared library is not built by `cargo test`, only the rlib the
/// harness needs, so ask for it rather than hoping someone built it.
fn build_the_library() {
    let mut cargo = Command::new(env!("CARGO"));
    cargo.args(["build", "-p", "edge-stt-capi"]);
    if !cfg!(debug_assertions) {
        cargo.arg("--release");
    }
    let built = cargo.output().expect("cargo to run");
    assert!(
        built.status.success(),
        "the library did not build: {}",
        String::from_utf8_lossy(&built.stderr)
    );
}

#[test]
fn a_c_program_can_use_the_header() {
    build_the_library();

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let built = target_directory();
    let program = std::env::temp_dir().join("edge_stt_surface");

    let compiled = Command::new("cc")
        .arg(root.join("tests/surface.c"))
        .arg("-I")
        .arg(root.join("include"))
        .arg("-L")
        .arg(&built)
        .args(["-ledge_stt_capi", "-o"])
        .arg(&program)
        .output()
        .expect("a C compiler");
    assert!(
        compiled.status.success(),
        "the header did not compile: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let ran = Command::new(&program)
        .env("DYLD_LIBRARY_PATH", &built)
        .env("LD_LIBRARY_PATH", &built)
        .output()
        .expect("the compiled program to run");
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
}
