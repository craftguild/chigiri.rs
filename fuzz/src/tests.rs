//! Replay only the explicit fixed corpus, independently of local discoveries.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::TARGETS;

fn inventory() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut inventory: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut unique = BTreeSet::new();
    for line in include_str!("../fixed-corpus.txt").lines() {
        let (target, name) = line.split_once('/').expect("fixed target/name");
        assert!(TARGETS.iter().any(|(known, _)| *known == target));
        assert!(!name.is_empty() && name.len() <= 128);
        assert!(name.as_bytes()[0].is_ascii_alphanumeric());
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        );
        assert!(unique.insert(line), "duplicate fixed input");
        inventory.entry(target).or_default().push(name);
    }
    assert_eq!(inventory.len(), TARGETS.len());
    inventory
}

fn replay(root: &Path) {
    let inventory = inventory();
    for (target, run) in TARGETS {
        let inputs = &inventory[target];
        assert!(!inputs.is_empty(), "The corpus is empty.");
        for name in inputs {
            let input = fs::read(root.join(target).join(name)).unwrap();
            assert!(
                input.len() <= 256 * 1024,
                "The fixed input is above the limit."
            );
            run(&input);
        }
    }
}

#[test]
fn config_corpus_includes_a_valid_manifest_and_a_closed_shape_rejection() {
    let valid = include_str!("../corpus/config_parser/valid");
    let invalid = include_str!("../corpus/config_parser/closed-shape");
    assert!(chigiri_codegen::Manifest::parse("fuzz.toml", valid).is_ok());
    let error = chigiri_codegen::Manifest::parse("fuzz.toml", invalid).unwrap_err();
    assert_eq!(error.report().diagnostics()[0].code(), "SF4011");
}

#[test]
fn all_fixed_corpus_inputs_respect_the_production_size_limit() {
    replay(&Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus"));
}

struct TemporaryCorpus(PathBuf);

impl Drop for TemporaryCorpus {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn ignored_discovery_is_not_replayed_but_oversized_fixed_input_still_fails() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let parent = root.join("target");
    fs::create_dir_all(&parent).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = parent.join(format!("fixed-corpus-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let fixture = TemporaryCorpus(path);
    for (target, names) in inventory() {
        fs::create_dir(fixture.0.join(target)).unwrap();
        for name in names {
            fs::copy(
                root.join("corpus").join(target).join(name),
                fixture.0.join(target).join(name),
            )
            .unwrap();
        }
    }
    let oversized = vec![0; 256 * 1024 + 1];
    fs::write(fixture.0.join("parser/discovered"), &oversized).unwrap();
    replay(&fixture.0);
    fs::write(fixture.0.join("parser/query"), oversized).unwrap();
    assert!(std::panic::catch_unwind(|| replay(&fixture.0)).is_err());
}
