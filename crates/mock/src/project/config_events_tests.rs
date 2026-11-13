//! Check optional event configuration through static contracts and the real VM decoder.

use super::config_tests::CONFIG;
use super::{codegen_project, evaluate_config, read_project_schema};
use crate::events::{EventSettings, SourceLimits};

fn dynamic(events: &str) -> String {
    format!(
        "{}\nconfig.events = {events}\nreturn config\n",
        CONFIG.replacen("return {", "local config: any = {", 1)
    )
}

#[test]
fn ev_05_optional_events_groups_are_owned_and_default_individually() {
    let _case = crate::vm_gate::test_case();
    assert_eq!(
        evaluate_config(CONFIG).unwrap().events,
        EventSettings::default()
    );
    for group in [
        "{}",
        "{limits = {}}",
        "{consumers = {}}",
        "{limits = {}, consumers = {}}",
    ] {
        let source = CONFIG.replacen("return {", &format!("return {{ events = {group},"), 1);
        assert_eq!(
            evaluate_config(&source).unwrap().events,
            EventSettings::default()
        );
    }
    let source = CONFIG.replacen(
        "return {",
        "return { events = { limits = {max_queued_events = 1}, consumers = {audit = {field = 'changed', enabled = false}}},",
        1,
    );
    let settings = evaluate_config(&source).unwrap();
    drop(source);
    let limits = settings.events.limits();
    assert_eq!(limits.max_queued_events(), 1);
    assert_eq!(
        limits.max_queued_bytes(),
        SourceLimits::DEFAULT_QUEUED_BYTES
    );
    assert_eq!(
        limits.max_queue_age_ms(),
        SourceLimits::DEFAULT_QUEUE_AGE_MS
    );
    assert_eq!(settings.events.consumers()["audit"].field, "changed");
    assert!(!settings.events.consumers()["audit"].enabled);
    assert_eq!(
        evaluate_config(CONFIG).unwrap().events,
        EventSettings::default()
    );
}

#[test]
fn ev_09_real_vm_enforces_every_inclusive_limit_and_cross_limit_invariant() {
    let _case = crate::vm_gate::test_case();
    for (key, maximum) in [
        ("max_queued_events", 1_024),
        ("max_queued_bytes", 4_194_304),
        ("max_queue_age_ms", 60_000),
        ("max_retained_bytes", 67_108_864),
        ("max_sequence_bytes", 16_777_216),
    ] {
        for value in [
            "0".into(),
            "-1".into(),
            "1.5".into(),
            "true".into(),
            "'1'".into(),
            (maximum + 1).to_string(),
            "0/0".into(),
            "1/0".into(),
        ] {
            let source = dynamic(&format!("{{limits = {{{key} = {value}}}}}"));
            let error = evaluate_config(&source).err().expect(&source);
            assert_ne!(
                error.phase, "type",
                "must reach host decoder: {key}={value}"
            );
        }
        let source = dynamic(&format!("{{limits = {{{key} = {maximum}}}}}"));
        assert!(evaluate_config(&source).is_ok(), "{key}={maximum}");
    }
    assert!(evaluate_config(&dynamic("{limits = {max_retained_bytes = 1}}")).is_err());
    let minimum = evaluate_config(&dynamic("{limits = {max_queued_events = 1, max_queued_bytes = 1, max_queue_age_ms = 1, max_retained_bytes = 1, max_sequence_bytes = 1}}" )).unwrap();
    assert_eq!(
        minimum.events.limits(),
        SourceLimits::new(1, 1, 1, 1, 1).unwrap()
    );
}

#[test]
fn ev_05_real_vm_rejects_unknown_keys_and_forged_registry_shapes() {
    let _case = crate::vm_gate::test_case();
    for group in [
        "false",
        "{extra = true}",
        "{limits = false}",
        "{limits = {extra = 1}}",
        "{consumers = false}",
        "{consumers = {audit = {field = 'changed'}}}",
        "{consumers = {audit = {enabled = false}}}",
        "{consumers = {audit = {field = 'changed', enabled = 1}}}",
        "{consumers = {audit = {field = 'changed', enabled = false, path = 'other'}}}",
        "{consumers = {audit = function() end}}",
        "{consumers = {['../audit'] = {field = 'changed', enabled = false}}}",
        "{consumers = {['audit-name'] = {field = 'changed', enabled = false}}}",
        "{consumers = {audit = {field = 'changed', enabled = true}, Audit = {field = 'changed', enabled = false}}}",
        "{consumers = {[1] = {field = 'changed', enabled = false}}}",
    ] {
        let error = evaluate_config(&dynamic(group)).err().expect(group);
        assert_ne!(error.phase, "type", "must reach host decoder: {group}");
    }
}

#[test]
fn ev_05_configuration_registry_capacity_counts_disabled_consumers() {
    let _case = crate::vm_gate::test_case();
    for count in [1_024, 1_025] {
        let source = dynamic(&format!(
            "(function() local entries = {{}} for index = 1, {count} do entries['consumer_' .. index] = {{field = 'changed', enabled = false}} end return {{consumers = entries}} end)()"
        ));
        assert_eq!(evaluate_config(&source).is_ok(), count == 1_024);
    }
}

#[test]
fn ev_12_event_settings_change_generation_identity_and_need_local_codegen() {
    let _case = crate::vm_gate::test_case();
    let fixture = super::test_support::Fixture::new(
        "schema { query: Read subscription: Changes } type Read { count: Int! } type Changes { changed(required: ID!): Int! }",
    );
    let initial = fixture.read("generated/update-state.json");
    let config = fixture.read("mock.luau");
    fixture.write(
        "mock.luau",
        &config.replace(
            "return config",
            "config.events = {limits = {max_queued_events = 1}}\nreturn config",
        ),
    );
    let error = read_project_schema(&fixture.project).err().unwrap();
    assert_eq!(
        error.message,
        "configuration contract changed; run chigiri mock codegen"
    );
    codegen_project(&fixture.project).unwrap();
    let after = fixture.read("generated/update-state.json");
    assert_ne!(after, initial);
    assert_eq!(
        read_project_schema(&fixture.project)
            .unwrap()
            .settings()
            .events
            .limits()
            .max_queued_events(),
        1
    );
    codegen_project(&fixture.project).unwrap();
    assert_eq!(fixture.read("generated/update-state.json"), after);

    // Registered field validation precedes generation consistency. Disabled
    // modules cannot retain a removed field or silently target a Query field.
    for field in ["Read.count", "count", "Changed", "missing"] {
        fixture.write("mock.luau", &config.replace("return config", &format!("config.events = {{consumers = {{audit = {{field = '{field}', enabled = false}}}}}}\nreturn config")));
        assert_eq!(
            read_project_schema(&fixture.project).err().unwrap().phase,
            "config"
        );
        let before = fixture.read("generated/update-state.json");
        assert!(codegen_project(&fixture.project).is_err());
        assert_eq!(fixture.read("generated/update-state.json"), before);
    }
}
