//! Checked query context retains source columns and explicit author casts.

use chigiri::core::Source;

use super::literal_precision::narrow;
use crate::ir::MockIr;

fn ir() -> MockIr {
    MockIr::parse(vec![Source::new(
        "literal.graphql",
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status! name:String!} type Query{items:[Item!]!}",
    )])
    .unwrap()
}

fn context_count(source: &str, alias: &str) -> usize {
    source
        .matches(&format!(": {alias}): {alias} return "))
        .count()
}

#[test]
fn contextualizes_only_proven_query_tables_and_maps_columns_back() {
    let source = "local x = 'wide'\nreturn ctx.models.Item:find({orderBy = {{field = 'name', direction = 'asc'}}})";
    let (narrowed, map, _) = narrow(source, &ir());
    assert!(narrowed.starts_with("local x = 'wide'\n"));
    assert!(narrowed.contains("function(_chigiri_checked_"));
    assert_eq!(context_count(&narrowed, "ChigiriItemOrderKey"), 1);
    assert!(!narrowed.contains(" :: "));
    let original = source.lines().nth(1).unwrap().len() + 1;
    let generated = narrowed.lines().nth(1).unwrap().len() + 1;
    assert_eq!(map.original(2, generated), original);
    let original_line = source.lines().nth(1).unwrap();
    let generated_line = narrowed.lines().nth(1).unwrap();
    for literal in ["'name'", "'asc'"] {
        let original_start = original_line.find(literal).unwrap() + 1;
        let generated_start = generated_line.find(literal).unwrap() + 1;
        assert_eq!(map.original(2, generated_start), original_start);
        assert_eq!(
            map.original(2, generated_start + literal.len()),
            original_start + literal.len()
        );
    }
}

#[test]
fn preserves_existing_casts() {
    let source = "return m:find({where={field='status',op='eq',value='OPEN' :: 'OPEN'}})";
    let (narrowed, _, _) = narrow(source, &ir());
    assert_eq!(narrowed.matches("'OPEN' :: 'OPEN'").count(), 1);
}

#[test]
fn enum_context_follows_the_model_alias_and_does_not_enter_value_functions() {
    let source = "local model = ctx.models.Item\nreturn model:find({where={field='status',op='eq',value='OPEN'}})";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(narrowed.contains("end)('OPEN')"));
    assert!(sites.is_empty());

    let source = "return ctx.models.Item:find({where={field='status',op='eq',value=(function(): ChigiriStatusValue local unrelated='NOT_A_STATUS'; return 'OPEN' end)()}})";
    let (narrowed, _, _) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 0);
}

#[test]
fn enum_context_includes_long_bracket_literals() {
    for source in [
        "return ctx.models.Item:find({where={field='status',op='eq',value=[[OPEN]]}})",
        "return ctx.models.Item:find({where={field='status',op='in',value={[[OPEN]],[[CLOSED]]}}})",
    ] {
        let (narrowed, _, sites) = narrow(source, &ir());
        assert!(context_count(&narrowed, "ChigiriStatusValue") >= 1);
        assert!(narrowed.contains("end)([[OPEN]])"));
        assert!(sites.is_empty());
    }
}

#[test]
fn aliases_follow_lexical_scope_and_source_order() {
    let source = "local model = ctx.models.Item\nlocal function helper(model) return model end\nlocal rows = model:find({where={field='status',op='eq',value='OPEN'}})\nmodel = helper(model)\nreturn rows";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(sites.is_empty());

    let source = "do local model=ctx.models.Item; local a=model:find({where={field='status',op='eq',value='OPEN'}}) end\ndo local model=ctx.models.Item; local b=model:find({where={field='status',op='eq',value='CLOSED'}}) end";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 2);
    assert!(sites.is_empty());

    let source = "local first=ctx.models.Item\nlocal second=dynamic\nfirst,second=second,first\nreturn second:find({where={field='status',op='eq',value='OPEN'}})";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(sites.is_empty());

    let source = "local model=ctx.models.Item\nlocal function change() model=dynamic end\nreturn model:find({where={field='status',op='eq',value='OPEN'}})";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 0);
    assert!(sites.is_empty());

    let source = "local model=ctx.models.Item\nfor model in model:find({where={field='status',op='eq',value='OPEN'}}) do local ignored=model end";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(sites.is_empty());
}

#[test]
fn direct_models_require_the_generated_or_unshadowed_context() {
    let source = "local resolver=function(ctx: ChigiriQueryItemsContext) return ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}}) end";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(sites.is_empty());

    let source = "local resolver=function(ctx: ChigiriItemsResolverContext) return ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}}) end";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(narrowed, source);
    assert!(sites.is_empty());

    let source = "local ctx = dynamic\nreturn ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}})";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 0);
    assert!(sites.is_empty());

    let source = "local model=ctx.models.Item\nlocal function helper(model) return model:find({where={field='status',op='eq',value='OPEN'}}) end\nreturn helper(dynamic)";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 0);
    assert!(sites.is_empty());

    let source = "local model=ctx.models.Item :: any\nreturn model:find({unknown=true})";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert_eq!(narrowed, source);
    assert!(sites.is_empty());
}

#[test]
fn ab_02_indexed_calls_keep_their_query_context() {
    let source =
        "local first=ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}})[1]";
    let (narrowed, _, sites) = narrow(source, &ir());
    assert!(
        narrowed.contains(": ChigiriItemFindOptions): ChigiriItemFindOptions"),
        "{narrowed}"
    );
    assert_eq!(context_count(&narrowed, "ChigiriStatusValue"), 1);
    assert!(sites.is_empty());
}
