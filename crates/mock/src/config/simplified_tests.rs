//! The portable v11 reader rejects every retired bundle format without conversion.

use std::path::Path;
use std::sync::Arc;

use chigiri::core::Source;
use chigiri_mock::ir::MockIr;
use chigiri_mock::luau::{Contract, ScriptSet, render_module, static_candidates};

use super::{Config, Envelope};

#[test]
fn cv_05_bundle_v11_round_trip_is_closed_and_has_no_legacy_conversion() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new("new.graphql", "type Query{value:Int!}")]).unwrap(),
    );
    let mut candidates = static_candidates(&ir).unwrap();
    for candidate in &mut candidates {
        candidate.source = render_module(&ir, candidate.script, "return 7").unwrap();
    }
    let scripts = ScriptSet::new(Contract::new(ir).unwrap(), candidates).unwrap();
    let source = chigiri_mock::bundle::render(
        &chigiri_mock::project::ServerSettings {
            listen: "127.0.0.1:0".parse().unwrap(),
            allowed_origins: vec![],
        },
        &scripts,
        "{}",
        Default::default(),
        &[],
        &Default::default(),
        &chigiri_mock::bundle::EventSources {
            settings: &Default::default(),
            modules: vec![],
        },
    )
    .unwrap();
    assert!(source.contains("format = \"chigiri-mock/v11\""));
    assert!(source.contains("capabilities = \"chigiri-mock-capabilities/v4\""));
    assert!(source.contains("[clock]\nmode = \"system\""));
    assert!(!source.contains("now ="));
    let path = Path::new("absent-original-directory/portable.toml");
    Envelope::parse(path, &source).unwrap();
    Config::parse(path, &source).unwrap();
    for old_brand in [
        source.replace("chigiri-mock/v11", "pythia-mock/v8"),
        source.replace("chigiri-mock-directives/v3", "pythia-mock-directives/v3"),
    ] {
        assert!(Envelope::parse(path, &old_brand).is_err());
        assert!(Config::parse(path, &old_brand).is_err());
    }
    for version in 1..=10 {
        let old = source.replace("chigiri-mock/v11", &format!("chigiri-mock/v{version}"));
        let error = Envelope::parse(path, &old).err().unwrap();
        assert_eq!(error.message, "unsupported format");
        assert!(Config::parse(path, &old).is_err());
    }
    for version in ["v1", "v2"] {
        let old = source.replace(
            "chigiri-mock-directives/v3",
            &format!("chigiri-mock-directives/{version}"),
        );
        assert!(Envelope::parse(path, &old).is_err());
    }
}
