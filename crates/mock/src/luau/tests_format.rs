//! Verifies fixed formatting bytes, Luau semantics, control comments, size
//! limits, and private diagnostics.

use super::{MAX_BODY, format};

#[test]
fn fmt_02_fixed_style_idempotence_and_final_newline() {
    let input = "--!strict\r\nlocal function f( x:number )\r\n\r\nreturn { value=x,text='hello' }\r\n\r\nend\r\nreturn f( 2 )\r\n\r\n";
    let expected = "--!strict\nlocal function f(x: number)\n    return { value = x, text = \"hello\" }\nend\nreturn f(2)\n";
    let output = format::source(input, MAX_BODY).unwrap();
    assert_eq!(output, expected);
    assert_eq!(format::source(&output, MAX_BODY).unwrap(), output);
    assert!(format::check(input, MAX_BODY).is_err());
    format::check(&output, MAX_BODY).unwrap();
    for input in [
        "return 1",
        "return 1\n\n",
        "return 1 -- trailing comment\n\n",
        "-- trailing comment",
    ] {
        let output = format::source(input, MAX_BODY).unwrap();
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
        format::check(&output, MAX_BODY).unwrap();
    }
}

#[test]
fn fmt_02_hook_cast_arguments_are_stable_before_save_and_adoption_check() {
    let input = r#"--!strict
local hook: ChigiriFieldAfterHook = {
    run = function(ctx: ChigiriFieldAfterContext): ChigiriFieldAfterResult
        ctx.log('info', 'field.after', {role=(ctx.state.role :: string),checked=(ctx.state.operationChecked :: boolean),errors=#ctx.result.errors})
        return nil
    end,
}
return hook
"#;
    let output = format::source(input, MAX_BODY).unwrap();
    assert!(output.contains("ctx.log(\"info\", \"field.after\", {"));
    assert_eq!(format::source(&output, MAX_BODY).unwrap(), output);
    format::check(&output, MAX_BODY).unwrap();
    assert!(output.contains("checked = ctx.state.operationChecked :: boolean"));
}

#[test]
fn fmt_03_comments_strings_types_associativity_and_statement_order() {
    let _test_case = crate::vm_gate::test_case();
    let input = r#"--!strict
-- Preserve regular comments and execution order.
type Pair = {first:number,second:string}
local seen = ""
local function step(s:string):number seen ..= s return #seen end
local a=step('a')
local b=step('b')
--[=[ Regular multiline comment ]=]
local long=[==[Start
  middle whitespace__TRAILING_SPACES__

End]==]
local text='stylua: ignore start'
local pair:Pair={first=2^(3^2)-10-(3-1),second=long..text}
return seen..tostring(a)..tostring(b)..tostring(pair.first)..pair.second
"#
    .replace("__TRAILING_SPACES__", "  ");
    let output = format::source(&input, MAX_BODY).unwrap();
    assert!(output.starts_with("--!strict\n"));
    assert!(output.contains("-- Preserve regular comments and execution order."));
    assert!(output.contains("--[=[ Regular multiline comment ]=]"));
    assert!(output.contains("type Pair ="));
    let _lease = crate::vm_gate::test_guard();
    let lua = luaur::Lua::new();
    let before: String = lua.load(&input).eval().unwrap();
    let after: String = lua.load(&output).eval().unwrap();
    assert_eq!(before, after);
    assert!(after.starts_with("ab12500"));
    assert!(after.contains("  middle whitespace  \n\n"));
}

#[test]
fn fmt_03_control_comments_rejected_but_literal_words_allowed() {
    for control in [
        "stylua: ignore",
        "stylua: ignore start",
        "stylua: ignore end",
    ] {
        for comment in [
            format!("-- {control}\n"),
            format!("--[=[\n{control}\n]=]\n"),
        ] {
            let error = format::source(&format!("\n{comment}return 1"), MAX_BODY).unwrap_err();
            assert_eq!(error.phase, "format");
            assert!(error.message.contains("control comment"));
            assert!(error.body.0 >= 2);
        }
        for input in [
            format!("return '{control}'"),
            format!("return [=[{control}]=]"),
            format!("return `{control}`"),
        ] {
            let output = format::source(&input, MAX_BODY).unwrap();
            assert!(output.contains(control));
        }
    }
}

#[test]
fn fmt_16_20_limits_before_parse_after_expansion_and_private_errors() {
    let source = "return{a=1,b=2,c=3}";
    let expanded = format::source(source, MAX_BODY).unwrap();
    assert!(expanded.len() > source.len());
    assert!(format::source(source, expanded.len()).is_ok());
    let error = format::source(source, expanded.len() - 1).unwrap_err();
    assert_eq!(error.message, "Luau format output byte limit");
    let error = format::source("invalid secret_token", 3).unwrap_err();
    assert_eq!(error.message, "Luau format input byte limit");
    let error = format::source("local x = 1\nlocal secret_token = )", MAX_BODY).unwrap_err();
    assert_eq!(error.phase, "format");
    assert_eq!(error.body.0, 2);
    assert!(!error.to_string().contains("secret_token"));
}
