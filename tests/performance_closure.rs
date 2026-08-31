#[path = "../benches/generator.rs"]
mod generator;
#[path = "../benches/rss.rs"]
mod rss;

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use generator::{SEED, Scale, generate};
use graphia::daemon::debounce::SemanticAction;
use graphia::incremental::IncrementalWorkspace;
use graphia::storage::build_or_update;

#[test]
fn multi_scale_generator_is_deterministic() {
    let first = generate(Scale::Small);
    let second = generate(Scale::Small);
    assert_eq!(first.metadata, second.metadata);
    assert_eq!(first.metadata.files, 100);
    assert_eq!(first.metadata.languages.values().sum::<usize>(), 100);
    assert_eq!(first.metadata.seed, SEED);
    assert_eq!(Scale::Medium.file_count(), 1_000);
    assert_eq!(Scale::Large.file_count(), 5_000);
}

#[test]
fn rss_measurement_has_explicit_availability() {
    let measurement = rss::measure();
    assert!(measurement.peak_rss_bytes.is_some() || measurement.unavailable_reason.is_some());
    assert_ne!(measurement.peak_rss_bytes, Some(0));
}

#[test]
fn incremental_update_beats_clean_rebuild() {
    let dataset = generate(Scale::Small);
    let root = dataset.root.path();
    build_or_update(root, true).expect("initial index");
    let target = root.join("src/rust/module_0000.rs");
    let original = fs::read_to_string(&target).expect("source");
    build_or_update(root, true).expect("clean rebuild");
    let clean_start = Instant::now();
    for _ in 0..3 {
        build_or_update(root, true).expect("clean rebuild sample");
    }
    let clean_time = clean_start.elapsed();
    let mut workspace =
        IncrementalWorkspace::new(root.to_path_buf()).expect("incremental workspace");
    fs::write(
        &target,
        format!("{original}\npub fn closure_edit() -> usize {{ 1 }}\n"),
    )
    .expect("edit source");
    let start = Instant::now();
    let summary = workspace
        .apply_changes(&[SemanticAction::Modified(PathBuf::from(&target))])
        .expect("incremental update");
    let first_incremental = start.elapsed();
    let incremental_start = Instant::now();
    for iteration in 0..2 {
        fs::write(
            &target,
            format!("{original}\npub fn closure_edit() -> usize {{ {iteration} }}\n"),
        )
        .expect("repeat edit");
        workspace
            .apply_changes(&[SemanticAction::Modified(PathBuf::from(&target))])
            .expect("incremental update sample");
    }
    let incremental_time = first_incremental + incremental_start.elapsed();
    assert!(summary);
    assert!(
        incremental_time < clean_time,
        "incremental={incremental_time:?}, clean={clean_time:?}"
    );
}
