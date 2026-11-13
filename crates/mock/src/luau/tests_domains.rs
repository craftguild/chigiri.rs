//! Test domain names and constraint vocabulary across record, DTO, abstract, and input types.

use super::tests::{candidate, contract};
use super::{ScriptSet, static_candidates};

const SDL: &str = r#"
enum Mode { OPEN CLOSED }
scalar Data @representation(type: JSON)
scalar Map @representation(type: OBJECT)
scalar Items @representation(type: ARRAY)
scalar Count @representation(type: INTEGER)
interface Node { id: ID! }
type Member implements Node {
  id: ID!
  name: String!
  mode: Mode!
  peerId: ID
  peer: Member @relation(fields: ["peerId"], references: ["id"])
  data: Data
  label(prefix: String = "x"): String!
}
type Packet implements Node {
  id: ID!
  member: Member
  child: Packet
  data: Data
  label: String! @virtual
}
union Result = Member | Packet
input Pick @oneOf { id: ID, name: String }
input Input { name: String!, mode: Mode! = OPEN, peerId: ID, data: Data }
input Patch { name: String, mode: Mode, peerId: ID, data: Data }
type Query {
  members(mode: Mode): [Member!]!
  result(pick: Pick): Result
  node: Node
  data: Data!
  map: Map!
  items: Items!
  count: Count!
  packet: Packet!
}
type Mutation {
  create(input: Input!): Member!
  update(id: ID!, input: Patch!): Member!
  delete(id: ID!): Boolean!
}
"#;

#[test]
fn rsm_07_second_domain_defaults_oneof_recursive_payload_and_abstract_are_typed() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let set = ScriptSet::new(
        super::Contract::new(contract.ir().clone()).unwrap(),
        static_candidates(contract.ir()).unwrap(),
    )
    .unwrap();
    assert_eq!(set.scripts().len(), contract.ir().scripts().len());
    for (coordinate, body) in [
        (
            "Query.result",
            "return {__typename = 'Packet' :: 'Packet', id = 'p', label = 'p'}",
        ),
        (
            "Query.node",
            "return ctx.models.Member:find({limit = 1})[1]",
        ),
        (
            "Query.result",
            "local pick = args.pick\nif pick == nil then return ctx.null end\nreturn {__typename = 'Packet' :: 'Packet', id = 'p', label = 'p'}",
        ),
        (
            "Query.data",
            "return ctx.jsonObject({a = ctx.null, b = ctx.jsonArray({})})",
        ),
        ("Query.map", "return ctx.jsonObject({})"),
        ("Query.items", "return ctx.jsonArray({ctx.null, 1})"),
        ("Query.count", "return 9007199254740994"),
        (
            "Query.packet",
            "return {id = 'p', label = 'p', data = ctx.jsonObject({})}",
        ),
        ("Member.label", "return parent.name"),
        ("Packet.label", "return parent.id"),
    ] {
        contract
            .script(candidate(&contract, coordinate, body))
            .unwrap_or_else(|error| panic!("{coordinate} `{body}`: {error}"));
    }
    for (coordinate, body) in [
        ("Query.node", "return {id = 'missing concrete'}"),
        ("Query.result", "return {__typename = 'Unknown', id = 'p'}"),
        ("Query.data", "return ctx.null"),
        ("Query.map", "return 'wrong'"),
        ("Query.items", "return 1"),
        ("Query.count", "return true"),
        ("Query.packet", "return {data = 'missing id'}"),
        ("Member.label", "return parent.peer.name"),
        ("Member.label", "return ctx.models.Member:delete(parent.id)"),
    ] {
        assert!(
            contract
                .script(candidate(&contract, coordinate, body))
                .is_err(),
            "{coordinate}: {body}"
        );
    }
}

#[test]
fn rsm_05_06_keyword_and_renamed_domain_use_only_quoted_keys_and_id_aliases() {
    let _test_case = crate::vm_gate::test_case();
    let original = include_str!("../../fixtures/blog.graphql");
    let renamed = original
        .replace("User", "end")
        .replace("Post", "local")
        .replace("getUsers", "while")
        .replace("name", "function");
    let contract = contract(&renamed);
    let candidates = static_candidates(contract.ir()).unwrap();
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.source.contains("NOT_IMPLEMENTED"))
    );
    let set = ScriptSet::new(contract, candidates).unwrap();
    assert!(!set.contract().declarations().contains("type end ="));
    assert!(set.contract().declarations().contains("[\"function\"]:"));
}

#[test]
fn rsm_06_relation_storage_keys_escape_quotes_slashes_control_bytes_and_unicode() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        type Item {
            id: ID!
            peer: Item
        }
        type Query { x: Int }
    "#,
    );
    let set = ScriptSet::new(
        super::Contract::new(contract.ir().clone()).unwrap(),
        static_candidates(contract.ir()).unwrap(),
    )
    .unwrap();
    assert_eq!(set.scripts().len(), contract.ir().scripts().len());
    // Round-trip through the real VM so three-digit escapes are interpreted as characters.
    let key = "peer\"\\\n\u{1}1Name";
    let mut out = super::text::Text::new(256);
    out.add("return ").unwrap();
    out.quoted(key).unwrap();
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let value: String = lua.load(out.finish()).eval().unwrap();
    assert_eq!(value, key);
}

#[test]
fn rsm_02_relation_key_nul_is_rejected_at_ir_with_source_position() {
    let _test_case = crate::vm_gate::test_case();
    let source = r#"type Item { id: ID! peer: Item @chigiri_mock_relation(key: "peer\u0000Id") }
        type Query { x: Int }"#;
    let error = crate::ir::MockIr::parse(vec![chigiri::core::Source::new("nul.graphql", source)])
        .err()
        .unwrap();
    assert!(error.code.starts_with("retired mock relation directive"));
    assert!(error.span.is_some());
}
