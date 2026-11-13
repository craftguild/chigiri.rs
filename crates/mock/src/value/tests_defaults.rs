//! Verify input defaults against declared constraints during IR construction without inferring storage rules.

use chigiri::core::Source;

use crate::ir::MockIr;

fn parse(source: &str) -> Result<MockIr, crate::schema::SchemaError> {
    MockIr::parse(vec![Source::new("defaults.graphql", source)])
}

#[test]
fn rsm_04_defaults_use_scalar_input_and_nested_constraints() {
    let source = r#"
        scalar Text @representation(type:STRING,maxBytes:4)
        input I { text:String = "ok" @constraint(maxLength:4), next:I }
        type Query { x(text:Text = "ok", input:I = {next:{text:"ok"}}):Int }
    "#;
    assert!(parse(source).is_ok());
    for source in [
        source.replace("text:Text = \"ok\"", "text:Text = \"private-value\""),
        source.replace("String = \"ok\"", "String = \"private-value\""),
        source.replace("next:{text:\"ok\"}", "next:{text:\"private-value\"}"),
    ] {
        let error = parse(&source).err().unwrap();
        assert_eq!(error.code, "mock default violates value constraints");
        assert!(error.span.is_some());
        assert!(!error.to_string().contains("private-value"));
    }
}

#[test]
fn sd_13_manual_input_defaults_use_their_own_declared_constraints() {
    let source = r#"
        type Dto { text:String! @constraint(maxLength:4) next:Dto }
        type Item { id:ID! text:String! @constraint(maxLength:4) dto:Dto }
        input Nested { text:String! = "ok" @constraint(maxLength:4), next:Nested }
        input Create { text:String! = "ok" @constraint(maxLength:4), dto:Nested }
        input Patch { text:String = "ok" @constraint(maxLength:4), dto:Nested }
        type Query { x:Int }
        type Mutation {
            create(input:Create! = {text:"ok"}):Item!
            update(id:ID!,input:Patch!):Item!
        }
    "#;
    assert!(parse(source).is_ok());
    for source in [
        source.replace("String! = \"ok\"", "String! = \"private-value\""),
        source.replace("String = \"ok\"", "String = \"private-value\""),
        source.replace(
            "Create! = {text:\"ok\"}",
            "Create! = {text:\"private-value\"}",
        ),
        source.replace(
            "dto:Nested }",
            "dto:Nested = {text:\"ok\",next:{text:\"private-value\"}} }",
        ),
    ] {
        let error = parse(&source).err().unwrap();
        assert_eq!(error.code, "mock default violates value constraints");
        assert!(!error.to_string().contains("private-value"));
    }
}

#[test]
fn rsm_16_default_expansion_is_checked_before_mock_value_copies() {
    let mut source = String::from("input I0 { text:String = \"x\" }\n");
    for index in 1..18 {
        source.push_str(&format!(
            "input I{index} {{ a:I{} = {{}} b:I{} = {{}} }}\n",
            index - 1,
            index - 1
        ));
    }
    source.push_str("type Query { x(input:I17 = {}):Int }");
    let error = parse(&source).err().unwrap();
    assert_eq!(error.code, "mock default violates value constraints");
}

#[test]
fn rsm_04_directive_defaults_share_scalar_and_nested_input_validation() {
    let source = r#"
        scalar Token @representation(type:STRING,maxBytes:4)
        input Input { value:String @constraint(maxLength:4) }
        directive @custom(token:Token = "ok", input:Input = {value:"ok"}) on FIELD
        type Query { x:Int }
    "#;
    assert!(parse(source).is_ok());
    for source in [
        source.replace("Token = \"ok\"", "Token = \"private-value\""),
        source.replace("value:\"ok\"", "value:\"private-value\""),
    ] {
        let error = parse(&source).err().unwrap();
        assert_eq!(error.code, "mock default violates value constraints");
        assert!(error.span.is_some());
        assert!(!error.to_string().contains("private-value"));
    }
}
