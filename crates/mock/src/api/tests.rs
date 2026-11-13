//! Test registry structure separately from runtime behavior in a real VM.

use super::*;

#[test]
fn mr_02_missing_duplicate_and_empty_rule_are_rejected() {
    validate(METHODS).unwrap();
    assert!(validate(&METHODS[1..]).is_err());
    let mut duplicate = METHODS.to_vec();
    duplicate.push(METHODS[0]);
    assert!(validate(&duplicate).is_err());
    for field in ["id", "name", "rule", "description", "example"] {
        let mut broken = METHODS.to_vec();
        match field {
            "id" => broken[0].id = "",
            "name" => broken[0].name = "",
            "rule" => broken[0].rule.id = "",
            "description" => broken[0].rule.description = "",
            "example" => broken[0].rule.example = "",
            _ => unreachable!(),
        }
        assert!(validate(&broken).is_err(), "{field}");
    }
}
