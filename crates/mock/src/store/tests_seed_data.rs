//! Test strict JSONL lines, shared budgets, seed order, numeric precision, and normalized preservation.

use crate::store::test_selection::id as select;

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{CallScope, SeedData};
use crate::ir::{MockIr, ModelId};

fn ir(source: &str) -> Arc<MockIr> {
    Arc::new(MockIr::parse(vec![Source::new("schema/001.graphql", source)]).unwrap())
}

const SDL: &str = "scalar Data @representation(type:JSON) type Item { id:ID! name:String! note:String data:Data label:String! @virtual } type Other { id:ID! } type Query { items:[Item!]! }";

#[test]
fn mp_17_jsonl_empty_crlf_and_terminal_newline_have_one_strict_record_meaning() {
    let ir = ir(SDL);
    let empty = SeedData::jsonl(&ir, &[(ModelId(0), ""), (ModelId(1), "")]).unwrap();
    assert_eq!(empty.canonical().unwrap(), "{\"Item\":[],\"Other\":[]}");
    assert_eq!(empty.store(ir.clone()).unwrap().snapshot().size().0, 0);
    let lines = "{\"id\":\"Item-2\",\"name\":\"first\"}\n{\"id\":\"Item-1\",\"name\":\"second\",\"note\":null}";
    let mut expected = None;
    for source in [
        lines.to_owned(),
        format!("{lines}\n"),
        lines.replace('\n', "\r\n") + "\r\n",
    ] {
        let seed = SeedData::jsonl(&ir, &[(ModelId(0), &source), (ModelId(1), "")]).unwrap();
        seed.store(ir.clone()).unwrap();
        let normalized = seed.canonical().unwrap();
        assert_eq!(
            expected.get_or_insert_with(|| normalized.clone()),
            &normalized
        );
        let reparsed = SeedData::json(&normalized).unwrap();
        assert_eq!(reparsed.canonical().unwrap(), normalized);
    }
    for source in [
        "\n",
        " \t\r\n",
        "// comment",
        "# comment",
        "\u{feff}{}",
        "[]",
        "null",
        "42",
        "{\"id\":\"private\",\"id\":\"duplicate\"}",
        "{\"data\":{\"nested\":1,\"nested\":2}}",
        "{\"data\":9007199254740993}",
        "{\"data\":1e10000}",
        "{\"data\":1e-10000}",
        "{} {}",
        "{\n}",
    ] {
        let error = SeedData::jsonl(&ir, &[(ModelId(0), source), (ModelId(1), "")])
            .err()
            .unwrap();
        assert_eq!(error.model, Some(ModelId(0)));
        assert_eq!(error.line, 1);
        assert!(!format!("{error:?}").contains("private"));
    }
    let error = SeedData::jsonl(&ir, &[(ModelId(0), "{}\n\n"), (ModelId(1), "")])
        .err()
        .unwrap();
    assert_eq!(error.line, 2);
}

#[test]
fn mp_18_seed_storage_validation_reports_model_line_and_known_field_without_values() {
    let ir = ir(SDL);
    let good = "{\"id\":\"first\",\"name\":\"valid\"}\n";
    for (row, field, reason) in [
        (
            "{\"id\":\"private\"}",
            Some("name"),
            "required storage field missing",
        ),
        (
            "{\"id\":\"private\",\"name\":null}",
            Some("name"),
            "null for non-null value",
        ),
        (
            "{\"id\":\"first\",\"name\":\"duplicate\"}",
            Some("id"),
            "duplicate seed ID",
        ),
        (
            "{\"id\":\"private\",\"name\":\"valid\",\"label\":\"computed\"}",
            Some("label"),
            "unknown or computed storage field",
        ),
    ] {
        let input = format!("{good}{row}");
        let seed = SeedData::jsonl(&ir, &[(ModelId(0), &input), (ModelId(1), "")]).unwrap();
        let error = seed.store(ir.clone()).err().unwrap();
        assert_eq!(error.model, Some(ModelId(0)));
        assert_eq!(error.line, 2);
        assert_eq!(error.field.as_deref(), field);
        assert_eq!(error.message, reason);
        assert!(!format!("{error:?}").contains("private"));
    }
}

#[test]
fn mp_19_26_seed_file_order_is_irrelevant_record_order_and_explicit_ids_survive_normalization() {
    let ir = ir(include_str!("../../fixtures/blog.graphql"));
    let users = "{\"id\":\"User-3\",\"name\":\"first\"}\n{\"id\":\"User-1\",\"name\":\"second\"}";
    let posts = "{\"id\":\"Post-1\",\"title\":\"x\",\"body\":\"x\",\"authorId\":\"User-3\"}";
    let forward = SeedData::jsonl(&ir, &[(ModelId(0), users), (ModelId(1), posts)]).unwrap();
    let reverse = SeedData::jsonl(&ir, &[(ModelId(1), posts), (ModelId(0), users)]).unwrap();
    let canonical = forward.canonical().unwrap();
    assert_eq!(reverse.canonical().unwrap(), canonical);
    for seed in [forward, reverse, SeedData::json(&canonical).unwrap()] {
        let mut store = seed.store(ir.clone()).unwrap();
        let rows = store.snapshot().find(ModelId(0), &Object::new()).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.id().unwrap()).collect::<Vec<_>>(),
            ["User-3", "User-1"]
        );
        let mut transaction = store.begin(CallScope::MutationRoot).unwrap();
        transaction.delete(ModelId(0), &select("User-1")).unwrap();
        let input = [("id", Value::from("User-2")), ("name", Value::from("new"))]
            .into_iter()
            .collect();
        assert_eq!(
            transaction
                .create(ModelId(0), &input)
                .unwrap()
                .id()
                .unwrap(),
            "User-2"
        );
        store.commit(transaction.prepare().unwrap()).unwrap();
        assert_eq!(seed.canonical().unwrap(), canonical);
    }
}

#[test]
fn mp_17_31_jsonl_aggregate_raw_records_depth_and_canonical_bytes_are_bounded() {
    let ir = ir(SDL);
    let row = "{\"id\":\"i\",\"name\":\"n\"}";
    let exact = format!("{row}{}", " ".repeat(crate::limits::SEED_BYTES - row.len()));
    SeedData::jsonl(&ir, &[(ModelId(0), &exact), (ModelId(1), "")]).unwrap();
    let error = SeedData::jsonl(&ir, &[(ModelId(0), &exact), (ModelId(1), " ")])
        .err()
        .unwrap();
    assert_eq!(error.message, "aggregate JSONL byte limit");
    let rows = (0..1000)
        .map(|id| format!("{{\"id\":\"{id}\",\"name\":\"n\"}}\n"))
        .collect::<String>();
    SeedData::jsonl(&ir, &[(ModelId(0), &rows), (ModelId(1), "")])
        .unwrap()
        .store(ir.clone())
        .unwrap();
    assert_eq!(
        SeedData::jsonl(&ir, &[(ModelId(0), &(rows + row)), (ModelId(1), "")])
            .err()
            .unwrap()
            .line,
        1001
    );
    for (depth, accepted) in [(31, true), (32, false)] {
        let source = format!(
            "{{\"data\":{}null{}}}",
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        );
        assert_eq!(
            SeedData::jsonl(&ir, &[(ModelId(0), &source), (ModelId(1), "")]).is_ok(),
            accepted
        );
    }
    let list = format!("[{}]", vec!["1e20"; 100].join(","));
    let payload = format!("[{}]", vec![list; 100].join(","));
    let source = (0..2)
        .map(|id| format!("{{\"id\":\"{id}\",\"name\":\"n\",\"data\":{payload}}}\n"))
        .collect::<String>();
    assert!(source.len() < crate::limits::SEED_BYTES);
    let seed = SeedData::jsonl(&ir, &[(ModelId(0), &source), (ModelId(1), "")]).unwrap();
    seed.store(ir).unwrap();
    assert!(seed.canonical().is_err());
}
