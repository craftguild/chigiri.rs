//! Build a bundle from initialized modules for acceptance tests.

use std::fs;
use std::path::Path;

use chigiri::core::Source;
use chigiri_mock::ir::MockIr;
use chigiri_mock::project::Layout;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

pub(super) fn bundle(project: &Path, source: &str) -> String {
    let ir = MockIr::parse(vec![Source::new("schema/001.graphql", source)]).unwrap();
    let layout = Layout::new(&ir).unwrap();
    let mut doc = DocumentMut::new();
    doc["format"] = value(chigiri_mock::info::CONFIG_FORMAT);
    doc["capabilities"] = value(chigiri_mock::info::CAPABILITY_VERSION);
    doc["storage"] = Item::Table(Table::new());
    doc["storage"]["digest"] = value(ir.graph().source().digest().to_hex());
    doc["clock"] = Item::Table(Table::new());
    doc["clock"]["mode"] = value("system");
    // The closed v11 envelope carries explicit ADR-0057 defaults even when
    // this fixture has no event consumers.
    doc["events"] = Item::Table(Table::new());
    doc["events"]["settings_json"] = value(
        serde_json::json!({
            "limits": {
                "max_queued_events": 32,
                "max_queued_bytes": 262_144,
                "max_queue_age_ms": 10_000,
                "max_retained_bytes": 16_777_216,
                "max_sequence_bytes": 1_048_576,
            },
            "consumers": {},
        })
        .to_string(),
    );
    doc["consumers"] = Item::Table(Table::new());
    // The hook-free fixture still carries explicit empty state and registrations.
    doc["state"] = Item::Table(Table::new());
    doc["state"]["contract_json"] = value(r#"{"fields":{},"initial":{}}"#);
    doc["hooks"] = Item::Table(Table::new());
    for phase in ["request", "operation", "field"] {
        doc["hooks"][phase] = Item::Table(Table::new());
        doc["hooks"][phase]["before"] = value(false);
        doc["hooks"][phase]["after"] = value(false);
    }
    doc["hooks"]["modules"] = Item::Table(Table::new());
    doc["server"] = Item::Table(Table::new());
    doc["server"]["listen"] = value("127.0.0.1:4000");
    doc["server"]["allowed_origins"] = value(Array::new());
    doc["schema"] = Item::Table(Table::new());
    doc["schema"]["digest"] = value(ir.graph().source().digest().to_hex());
    doc["schema"]["vocabulary"] = value(chigiri_mock::schema::VOCABULARY_VERSION);
    let mut sources = ArrayOfTables::new();
    for source in ir.graph().source().sources() {
        let mut table = Table::new();
        table["name"] = value(source.name());
        table["text"] = value(source.text());
        sources.push(table);
    }
    doc["schema"]["sources"] = Item::ArrayOfTables(sources);
    let mut seeds = serde_json::Map::new();
    for model in ir.models() {
        seeds.insert(
            ir.graph().types()[model.ty.0].name.to_string(),
            serde_json::json!([]),
        );
    }
    doc["seed"] = Item::Table(Table::new());
    doc["seed"]["records_json"] = value(serde_json::Value::Object(seeds).to_string());
    doc["resolvers"] = Item::Table(Table::new());
    for entry in layout.resolvers {
        doc["resolvers"][&entry.coordinate] = Item::Table(Table::new());
        doc["resolvers"][&entry.coordinate]["luau"] =
            value(fs::read_to_string(project.join(entry.path.as_str())).unwrap());
    }
    doc.to_string()
}
