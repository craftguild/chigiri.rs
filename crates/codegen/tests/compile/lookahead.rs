//! Genuine generated traits receive the effective executor selection.
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use chigiri::{FieldContext, FieldResult};

struct Types;
struct Pool { prepared: bool }
struct Code { prepared: bool }
struct Text;
impl generated::SchemaTypes for Types {
    type Pool = Pool;
    type Code = Code;
    type Text = Text;
}
struct Fields;
static PREPARATIONS: AtomicUsize = AtomicUsize::new(0);

impl generated::QueryResolver<(), Types, chigiri::FieldError> for Fields {
    async fn entry<'a>(&'a self, context: FieldContext<'a, ()>) -> FieldResult<Pool> {
        assert_eq!(context.selection().unwrap().response_key, "article");
        let fields = context.selected_fields("Pool").unwrap();
        let nodes = fields.iter().find(|field| field.name() == "nodes").unwrap();
        let code = nodes.selected_fields("Code").unwrap();
        let prepared = code.iter().any(|field| field.name() == "tokens");
        if prepared { PREPARATIONS.fetch_add(1, Ordering::SeqCst); }
        assert!(nodes.selected_fields("Pool").is_err());
        Ok(Pool { prepared })
    }
}
impl generated::PoolResolver<(), Types, chigiri::FieldError> for Fields {
    async fn nodes<'a>(&'a self, _: FieldContext<'a, ()>, pool: &'a Pool) -> FieldResult<Vec<generated::Node<Types>>> {
        Ok(vec![generated::Node::Code(Code { prepared: pool.prepared }), generated::Node::Text(Text)])
    }
}
impl generated::CodeResolver<(), Types, chigiri::FieldError> for Fields {
    async fn source<'a>(&'a self, _: FieldContext<'a, ()>, _: &'a Code) -> FieldResult<String> { Ok("source".into()) }
    async fn tokens<'a>(&'a self, _: FieldContext<'a, ()>, code: &'a Code) -> FieldResult<Vec<String>> {
        assert!(code.prepared);
        Ok(vec!["source".into()])
    }
}
impl generated::TextResolver<(), Types, chigiri::FieldError> for Fields {
    async fn text<'a>(&'a self, _: FieldContext<'a, ()>, _: &'a Text) -> FieldResult<String> { Ok("text".into()) }
}

struct NeverDeadline;
impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> { Box::pin(std::future::pending()) }
}
fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) { return value; }
        std::thread::yield_now();
    }
}

#[test]
fn nested_concrete_union_selections_drive_one_preparation_before_field_execution() {
    let resolvers = generated::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Fields).with_pool(Fields).with_text(Fields).with_code(Fields).build();
    let endpoint = chigiri::Endpoint::builder(resolvers).without_context().build().unwrap();
    for show in [false, true] {
        PREPARATIONS.store(0, Ordering::SeqCst);
        let mut variables = chigiri::Object::new();
        variables.insert("show", show);
        let request = chigiri::GraphQlRequest::new(r#"
            query Display($show: Boolean!) {
              article: entry { nodes { __typename ...CodeFields ... on Text { text } } }
              article: entry { nodes { ... on Code { source } } }
            }
            fragment CodeFields on Code { prepared: tokens @include(if: $show) }
        "#).with_variables(variables);
        let response = block_on(endpoint.execute(request, (), &NeverDeadline));
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        assert_eq!(PREPARATIONS.load(Ordering::SeqCst), usize::from(show));
        let chigiri::Value::Object(data) = response.data.unwrap() else { panic!("data") };
        let chigiri::Value::Object(pool) = data.get("article").unwrap() else { panic!("pool") };
        let chigiri::Value::List(nodes) = pool.get("nodes").unwrap() else { panic!("nodes") };
        let chigiri::Value::Object(code) = &nodes[0] else { panic!("code") };
        assert_eq!(code.get("__typename"), Some(&chigiri::Value::from("Code")));
        assert_eq!(code.get("prepared").is_some(), show);
    }
}
