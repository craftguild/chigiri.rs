//! Equivalent string spellings retain query types and diagnostic attribution.

use full_moon::{ast::Expression, visitors::Visitor};

use super::tests::{candidate, contract};

#[test]
fn decoded_literals_and_singletons_match_runtime_values() {
    let _case = crate::vm_gate::test_case();
    let lua = luaur::Lua::new();
    for literal in [
        r#""\97sc""#,
        r#""O\80EN""#,
        r#""\x4fPEN""#,
        r#""\u{4f}PEN""#,
        "\"O\\z \t\nPEN\"",
        r#""\000\001\127\"\\\a\b\f\n\r\t\v""#,
        "\"one\\\ntwo\"",
        "\"one\\\r\ntwo\"",
        "[[\nOPEN]]",
        "[=[\r\nOPEN]=]",
        "[==[one\r\ntwo]==]",
        r#""\195\169""#,
        "'résumé'",
    ] {
        let source = format!("return {literal}");
        let parsed = full_moon::parse_fallible(&source, full_moon::LuaVersion::luau());
        assert!(parsed.errors().is_empty(), "{literal:?}");
        let mut collector = Strings::default();
        collector.visit_ast(parsed.ast());
        let decoded = collector.0.pop().unwrap().unwrap();
        let runtime: String = lua.load(&source).eval().unwrap();
        assert_eq!(decoded, runtime, "{literal:?}");
        let quoted = super::literal_string::singleton(&decoded).unwrap();
        assert!(!quoted.contains(['\n', '\r']));
        let restored: String = lua.load(format!("return {quoted}")).eval().unwrap();
        assert_eq!(restored, runtime, "{literal:?}");
    }
}

#[derive(Default)]
struct Strings(Vec<Option<String>>);

impl Visitor for Strings {
    fn visit_expression(&mut self, expression: &Expression) {
        if matches!(expression, Expression::String(_)) {
            self.0
                .push(super::literal_string::decoded(expression).map(|(_, value)| value));
        }
    }
}

#[test]
fn undecodable_values_do_not_create_diagnostic_sites() {
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    for value in [r#""\255""#, r#""\xZZ""#, r#""\999""#] {
        let body = format!(
            "return ctx.models.Item:find({{where={{field='status',op='eq',value={value}}}}})"
        );
        let script = candidate(&contract, "Query.items", &body);
        let (_, _, sites) = super::literal_precision::narrow(&script.source, contract.ir());
        assert!(sites.is_empty(), "{value:?}");
    }
}

#[test]
fn equivalent_query_strings_pass_and_do_not_mask_a_bad_limit() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! rank:Int! status:Status!} type Query{items:[Item!]!}",
    );
    for options in [
        r#"orderBy={{field="rank",direction="\97sc"}}"#,
        r#"orderBy={{field=[[rank]],direction=[[asc]]}}"#,
        "orderBy={{field=[=[\nrank]=],direction=[==[\r\nasc]==]}}",
        r#"where={field="st\97tus",op="\101q",value="O\80EN"}"#,
        r#"where={field=[[status]],op=[[eq]],value=[[OPEN]]}"#,
        "where={field=[=[\nstatus]=],op=[[\neq]],value=[[\nOPEN]]}",
        r#"where={field="st\x61tus",op="e\u{71}",value="\x4fPEN"}"#,
        "where={field='status',op='eq',value='O\\z \nPEN'}",
        r#"where={field=[[status]],op=[[in]],value={"O\80EN",[[CLOSED]]}}"#,
    ] {
        let body = format!("return ctx.models.Item:find({{{options}}})");
        contract
            .script(candidate(&contract, "Query.items", &body))
            .unwrap_or_else(|error| panic!("{options:?}: {error:?}"));
        for options in [
            format!("{options},limit='bad'"),
            format!("limit='bad',{options}"),
        ] {
            let body = format!("return ctx.models.Item:find({{{options}}})");
            let error = contract
                .script(candidate(&contract, "Query.items", &body))
                .err()
                .unwrap();
            assert_eq!(
                error.member.as_deref(),
                Some("Item.find.limit"),
                "{error:?}"
            );
            assert_eq!(error.actual, "string");
            assert_eq!(error.expected, "nonnegative integer for Item.find.limit");
        }
    }
}

#[test]
fn decoded_model_receivers_support_direct_and_aliased_queries() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    for receiver in [
        r#"ctx.models["It\101m"]"#,
        "ctx.models[ [[\nItem]] ]",
        "ctx.models[ [=[\r\nItem]=] ]",
        r#"ctx["mod\101ls"]["It\x65m"]"#,
        r#"ctx[ [[models]] ]["It\u{65}m"]"#,
    ] {
        for (prefix, model) in [
            (String::new(), receiver),
            (format!("local model={receiver}\n"), "model"),
            (
                format!("local first={receiver}\nlocal model=first\n"),
                "model",
            ),
        ] {
            for (value, valid) in [("OPEN", true), ("SECRET", false)] {
                let body = format!(
                    "{prefix}return {model}:find({{where={{field='status',op='eq',value='{value}'}}}})"
                );
                let result = contract.script(candidate(&contract, "Query.items", &body));
                if valid {
                    result.unwrap_or_else(|error| panic!("{receiver:?}: {error:?}"));
                } else {
                    let error = result.err().unwrap();
                    assert_eq!(error.member.as_deref(), Some("Item.status"));
                    assert_eq!(error.actual, "string outside declared enum");
                    assert!(!format!("{error:?}").contains("SECRET"));
                }
            }
        }
    }
}

#[test]
fn decoded_model_receivers_preserve_shadowing_and_unknown_values() {
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    for prefix in [
        r#"local ctx=dynamic; local model=ctx.models["It\101m"]"#,
        r#"local model=ctx.models["It\101m"]; model=dynamic"#,
        r#"local model=ctx.models["\255"]"#,
    ] {
        let body = format!(
            "{prefix}; return model:find({{where={{field='status',op='eq',value='OPEN'}}}})"
        );
        let script = candidate(&contract, "Query.items", &body);
        let (source, _, sites) = super::literal_precision::narrow(&script.source, contract.ir());
        assert!(!source.contains(": ChigiriStatusValue = 'OPEN'"));
        assert!(sites.is_empty());
    }
}

#[test]
fn invalid_decoded_values_keep_precise_redacted_diagnostics() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! rank:Int! status:Status!} type Query{items:[Item!]!}",
    );
    for (options, member, actual) in [
        (
            r#"where={field="st\97tus",op=[[eq]],value="S\69CRET"}"#,
            "Item.status",
            "string outside declared enum",
        ),
        (
            "where={field=[[status]],op=[[in]],value={[[OPEN]],[[\nSECRET]]}}",
            "Item.status",
            "string outside declared enum",
        ),
        (
            "orderBy={{field=[[rank]],direction=[[\nSECRET]]}}",
            "Item.orderBy.direction",
            "string outside sort direction",
        ),
    ] {
        let body = format!("return ctx.models.Item:find({{{options}}})");
        let script = candidate(&contract, "Query.items", &body);
        let expected_line = script
            .source
            .lines()
            .position(|line| line.contains("value=") || line.contains("direction="))
            .unwrap()
            + 1;
        let error = contract.script(script).err().unwrap();
        assert_eq!(error.member.as_deref(), Some(member), "{error:?}");
        assert_eq!(error.actual, actual);
        assert_eq!(error.body.0, expected_line);
        assert!(!format!("{error:?}").contains("SECRET"));
    }
}
