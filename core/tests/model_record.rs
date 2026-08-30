//! The models the documentation names and the models there are
//! checksums for. The fetch script trusts both, so they must agree.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repository() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path
}

fn read(relative: &str) -> String {
    let path = repository().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|why| panic!("{}: {why}", path.display()))
}

fn quoted(line: &str) -> Vec<String> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The checksum table, as file name and hash.
fn recorded() -> Vec<(String, String)> {
    read("THIRD-PARTY-LICENSES")
        .lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            let cells = quoted(line);
            match cells.as_slice() {
                [file, hash] if file.starts_with("ggml-") => Some((file.clone(), hash.clone())),
                _ => None,
            }
        })
        .collect()
}

/// Every model named in the ladder table, without its prefix or suffix.
fn documented() -> BTreeSet<String> {
    let models = read("docs/models.md");
    let ladder = models
        .split("## The ladder")
        .nth(1)
        .expect("a section naming the models");
    ladder
        .lines()
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with('|'))
        .flat_map(quoted)
        .collect()
}

#[test]
fn every_recorded_model_has_a_checksum() {
    let recorded = recorded();
    assert!(!recorded.is_empty(), "the checksum table stopped parsing");
    for (file, hash) in recorded {
        assert_eq!(hash.len(), 64, "{file} has no SHA-256");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{file} has a checksum that is not one"
        );
    }
}

#[test]
fn the_documented_models_are_the_recorded_ones() {
    let recorded: BTreeSet<String> = recorded()
        .into_iter()
        .map(|(file, _)| {
            file.trim_start_matches("ggml-")
                .trim_end_matches(".bin")
                .to_string()
        })
        .collect();
    let documented = documented();

    let unfetchable: Vec<_> = documented.difference(&recorded).collect();
    assert!(
        unfetchable.is_empty(),
        "documented with no checksum to fetch them by: {unfetchable:?}"
    );
    let unmentioned: Vec<_> = recorded.difference(&documented).collect();
    assert!(
        unmentioned.is_empty(),
        "there are checksums for models the documentation never names: {unmentioned:?}"
    );
}
