use super::{GENERATOR_INFO_SCHEMA, GeneratorBuild, GeneratorInfo, shape};

fn unknown() -> GeneratorInfo {
    GeneratorInfo {
        schema_version: GENERATOR_INFO_SCHEMA.into(),
        version: "0.1.0".into(),
        revision: "unknown".into(),
        git_commit: None,
        dirty: None,
        build: GeneratorBuild {
            rustc: "unknown".into(),
            host: "aarch64-apple-darwin".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            profile: "debug".into(),
        },
    }
}

#[test]
fn sg_info_01_clean_dirty_unknown_only() {
    let mut info = unknown();
    info.validate().unwrap();
    let hash = "bf3a360c185382e1030082ab1a9dbd656aaba9aa";
    for dirty in [false, true] {
        info.git_commit = Some(hash.into());
        info.dirty = Some(dirty);
        info.revision = if dirty {
            format!("{hash}-dirty").into()
        } else {
            hash.into()
        };
        info.validate().unwrap();
        info.dirty = Some(!dirty);
        assert!(info.validate().is_err());
        info.dirty = None;
        assert!(info.validate().is_err());
    }
    info = unknown();
    info.dirty = Some(false);
    assert!(info.validate().is_err());
    info = unknown();
    info.revision = "1234567".into();
    assert!(info.validate().is_err());
}

#[test]
fn sg_info_01_bounds_all_fields_and_does_not_leak_values_in_errors() {
    for field in [
        "schema", "version", "revision", "commit", "rustc", "host", "target", "profile",
    ] {
        let mut info = unknown();
        let secret = "/private/secret".repeat(100);
        match field {
            "schema" => info.schema_version = secret.into(),
            "version" => info.version = secret.into(),
            "revision" => info.revision = secret.into(),
            "commit" => info.git_commit = Some(secret.into()),
            "rustc" => info.build.rustc = secret.into(),
            "host" => info.build.host = secret.into(),
            "target" => info.build.target = secret.into(),
            "profile" => info.build.profile = secret.into(),
            _ => unreachable!(),
        }
        let error = info.validate().unwrap_err();
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
    for hash in [
        "0".repeat(40),
        "A".repeat(40),
        "a".repeat(39),
        "a".repeat(41),
    ] {
        assert!(!shape::commit(&hash));
    }
}

#[test]
fn sg_info_01_rejects_invalid_semver_rustc_banners_paths_and_free_text() {
    for valid in ["0.1.0", "1.2.3-rc.1+build.02", "1.85.0-nightly"] {
        assert!(shape::semver(valid), "{valid}");
    }
    for invalid in [
        "1",
        "1.2",
        "01.2.3",
        "1.2.3-01",
        "1.2.3+",
        "1.2.3-a..b",
        "1.2.3+/secret",
        "1.2.3 a",
    ] {
        assert!(!shape::semver(invalid), "{invalid}");
    }
    assert!(shape::rustc("rustc 1.85.0 (4d91de4e4 2025-02-17)"));
    assert!(shape::rustc("rustc 1.95.0-nightly (abcdef012 2026-09-01)"));
    for invalid in [
        "rustc /secret (abcdef0 2025-02-17)",
        "rustc 1.85.0 (user secret)",
        "rustc 1.85.0 (abcdef0 2025-02-17) /home/user",
        "unknown\n",
    ] {
        assert!(!shape::rustc(invalid), "{invalid}");
    }
}
