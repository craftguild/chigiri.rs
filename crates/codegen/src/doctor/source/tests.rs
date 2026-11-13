//! Verifies retained-handle path diagnostics and identical bounds for supplemental sources.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Sources;
use crate::filesystem::Directory;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    sources: Sources,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "chigiri-doctor-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("generated/src")).unwrap();
        fs::write(root.join("lib.rs"), "source").unwrap();
        fs::write(
            root.join("generated/src/lib.rs"),
            "The source of production",
        )
        .unwrap();
        let root = fs::canonicalize(root).unwrap();
        let capability = Directory::open(&root).unwrap();
        Self {
            sources: Sources::new(
                capability.clone(),
                capability.clone(),
                capability.relative("generated").unwrap(),
                "contract",
            ),
            root,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn diag_03_uses_only_bounded_relative_paths_for_labels_and_source_reads() {
    let fixture = Fixture::new();
    assert_eq!(fixture.sources.label("src/../lib.rs"), "lib.rs");
    assert_eq!(
        fixture.sources.read("src/../lib.rs").as_deref(),
        Some("source")
    );
    assert_eq!(fixture.sources.label("/outside/private.rs"), "<path>");
    assert_eq!(fixture.sources.label("../../private.rs"), "<path>");
    assert!(fixture.sources.read("../../private.rs").is_none());
    assert!(!fixture.sources.generated("src/lib.rs"));
    assert!(fixture.sources.generated("generated/src/lib.rs"));
}

#[test]
fn cross_platform_absolute_paths_and_urls_are_redacted_to_a_common_placeholder() {
    let fixture = Fixture::new();
    for value in [
        "file:///C:/Users/alice/secret.rs",
        "file://server/share/secret.rs",
        r"C:\Users\alice\secret.rs",
        r"\\server\share\secret.rs",
        "https://user:secret@example.org/private",
    ] {
        assert_eq!(fixture.sources.label(value), "<path>");
        assert!(fixture.sources.read(value).is_none());
    }
}

#[test]
fn ignores_symlinks_and_fifos() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("lib.rs", fixture.root.join("link.rs")).unwrap();
    let created = crate::run_bounded_process(
        std::process::Command::new("mkfifo").arg(fixture.root.join("fifo.rs")),
        &[],
        crate::ProcessLimits {
            timeout: std::time::Duration::from_secs(5),
            bytes: 1024,
        },
    )
    .unwrap();
    assert!(created.status.success());
    assert!(fixture.sources.read("link.rs").is_none());
    assert!(fixture.sources.read("fifo.rs").is_none());
}

#[test]
fn sg_proc_01_source_cache_enforces_per_file_and_total_byte_limits() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("oversized.rs"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(fixture.sources.read("oversized.rs").is_none());
    for index in 0..16 {
        let file = format!("source-{index}.rs");
        fs::write(fixture.root.join(&file), vec![b'x'; 1024 * 1024]).unwrap();
        assert_eq!(fixture.sources.read(&file).unwrap().len(), 1024 * 1024);
    }
    assert!(fixture.sources.read("lib.rs").is_none());
    assert_eq!(fixture.sources.cache.borrow().bytes, 16 * 1024 * 1024);
}

#[test]
fn sg_proc_01_fails_closed_for_source_and_non_utf8_content() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("invalid.rs"), [0xff]).unwrap();
    assert!(fixture.sources.read("invalid.rs").is_none());
    for index in 1..1024 {
        assert!(
            fixture
                .sources
                .read(&format!("missing-{index}.rs"))
                .is_none()
        );
    }
    assert!(fixture.sources.read("lib.rs").is_none());
    assert_eq!(fixture.sources.cache.borrow().files.len(), 1024);
}
