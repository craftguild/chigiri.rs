//! Verify invalid predecessor sources cannot bypass aggregate capacity checks or readiness ceilings.

use super::test_support::Fixture;
use super::{ActiveSnapshot, Readiness};

const SDL: &str = "type Item @chigiri_mock_model(key: \"id\") { id: ID! name: String! } type Query { first: Int! @chigiri_mock_resolver }";

#[test]
fn ai_15_23_invalid_first_row_does_not_hide_later_record_array_string_or_depth_limits() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    for rest in [
        "{}\n".repeat(crate::store::MODEL_RECORDS),
        format!(
            "{{\"v\":[{}]}}\n",
            vec!["0"; crate::value::MAX_ITEMS + 1].join(",")
        ),
        format!(
            "{{\"v\":\"{}\"}}\n",
            "x".repeat(crate::value::MAX_STRING_BYTES + 1)
        ),
        format!("{{\"v\":{}0{}}}\n", "[".repeat(32), "]".repeat(32)),
        format!("{{\"v\":{}0{}}}\n", "[".repeat(1000), "]".repeat(1000)),
        format!(
            "{{\"same\":0,\"same\":[{}]}}\n",
            vec!["0"; crate::value::MAX_ITEMS + 1].join(",")
        ),
    ] {
        fixture.write("seeds/Item.jsonl", &("invalid JSON\n".to_owned() + &rest));
        let captured = ActiveSnapshot::capture(&fixture.project);
        let error = captured.err().unwrap();
        assert_eq!(error.phase, "limit");
        assert_eq!(error.file, "seeds/Item.jsonl");
    }
    fixture.write("seeds/Item.jsonl", "invalid JSON\n{}\n");
    let snapshot = ActiveSnapshot::capture(&fixture.project).unwrap();
    assert!(matches!(
        snapshot.validate_selection(&[]).unwrap(),
        Readiness::NotReady(_)
    ));
}

#[test]
fn ai_23_shared_nodes_are_counted_across_rows_even_after_invalid_row() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    let row = format!("{{\"v\":[{}]}}\n", vec!["0"; 100].join(","));
    // Each row has 102 nodes. They share the entire node budget within the individual array limit.
    fixture.write(
        "seeds/Item.jsonl",
        &("invalid\n".to_owned() + &row.repeat(980)),
    );
    ActiveSnapshot::capture(&fixture.project).unwrap();
    fixture.write(
        "seeds/Item.jsonl",
        &("invalid\n".to_owned() + &row.repeat(981)),
    );
    assert_eq!(
        ActiveSnapshot::capture(&fixture.project)
            .err()
            .unwrap()
            .phase,
        "limit"
    );
}

#[test]
fn ai_23_bundle_capacity_precedes_invalid_module_and_seed_semantics() {
    let _case = crate::vm_gate::test_case();
    let fields = (0..10)
        .map(|index| format!("f{index}: Int! @chigiri_mock_resolver"))
        .collect::<Vec<_>>()
        .join(" ");
    let fixture = Fixture::new(&format!("type Query {{ {fields} }}"));
    for index in 0..10 {
        fixture.write(
            &format!("resolvers/query/f{index}.luau"),
            &"x".repeat(crate::luau::MAX_BODY),
        );
    }
    let error = ActiveSnapshot::capture(&fixture.project).err().unwrap();
    assert_eq!(error.phase, "limit");
    assert_eq!(error.message, "canonical bundle exceeds 256 KiB");
}

#[test]
fn ai_23_canonical_seed_expansion_in_later_valid_rows_is_not_hidden_by_invalid_prefix() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(SDL);
    // Short exponent literals normalize to much longer integer values.
    let row = format!("{{\"v\":[{}]}}\n", vec!["1e20"; 100].join(","));
    fixture.write(
        "seeds/Item.jsonl",
        &("invalid\n".to_owned() + &row.repeat(150)),
    );
    let error = ActiveSnapshot::capture(&fixture.project).err().unwrap();
    assert_eq!(error.phase, "limit");
}
