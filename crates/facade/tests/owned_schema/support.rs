//! Small general-purpose dispatch that drives a real endpoint from owned input.

use std::future::{Future, pending};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use chigiri::core::Source;
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
use chigiri::core::schema::TypeKind;
use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
use chigiri::{
    BoxFuture, DeadlineFactory, Endpoint, FieldResult, GraphQlRequest, GraphQlResponse, Value,
};

pub struct NoDeadline;
impl DeadlineFactory for NoDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(pending())
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    struct Signal(std::thread::Thread);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Signal(std::thread::current())));
    let mut task = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut task) {
            Poll::Ready(result) => return result,
            Poll::Pending => std::thread::park(),
        }
    }
}

pub fn schema(sdl: &str) -> OwnedSchema {
    OwnedSchema::parse(vec![Source::new("test.graphql", sdl)]).unwrap()
}

pub struct Dispatch {
    pub schema: OwnedSchema,
    pub object_schema: OwnedSchema,
    pub coordinates: Vec<(Arc<str>, Arc<str>)>,
    pub output_type: String,
    pub value: Value,
    pub calls: Arc<AtomicUsize>,
    pub lifetime: Arc<()>,
    pub static_object: bool,
    pub ignore_arguments: bool,
    pub rows: Option<usize>,
}

impl Dispatch {
    pub fn new(schema: &OwnedSchema, output_type: &str, value: Value) -> Self {
        let coordinates = schema
            .semantic()
            .types()
            .iter()
            .filter(|ty| ty.kind() == TypeKind::Object)
            .flat_map(|ty| {
                let parent: Arc<str> = Arc::from(ty.name());
                ty.fields()
                    .iter()
                    .map(move |f| (parent.clone(), Arc::from(f.name.value.as_str())))
            })
            .collect();
        Self {
            schema: schema.clone(),
            object_schema: schema.clone(),
            coordinates,
            output_type: output_type.to_owned(),
            value,
            calls: Arc::new(AtomicUsize::new(0)),
            lifetime: Arc::new(()),
            static_object: false,
            ignore_arguments: false,
            rows: None,
        }
    }

    pub fn endpoint(self) -> Endpoint<()> {
        Endpoint::builder(self.schema.clone().bind(self).unwrap())
            .without_context()
            .build()
            .unwrap()
    }
}

impl OwnedResolverDispatch<()> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }
    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}
    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.ignore_arguments {
                return Ok(ResolvedValue::Leaf(self.value.clone()));
            }
            // The field with the argument returns the result of the coercion as it is and checks the behavior of the existing executor.
            if let Some(argument) = arguments.into_values().next() {
                return Ok(ResolvedValue::Leaf(
                    argument.into_value().unwrap_or(Value::Null),
                ));
            }
            if let Some(parent) = parent {
                return Ok(ResolvedValue::Leaf(
                    parent.downcast_ref::<Value>().unwrap().clone(),
                ));
            }
            if self.static_object {
                return Ok(ResolvedValue::object("Item", self.value.clone()));
            }
            if let Some(rows) = self.rows {
                return Ok(ResolvedValue::List(
                    (0..rows)
                        .map(|_| {
                            Ok(ResolvedValue::Object(
                                self.object_schema
                                    .object(&self.output_type, self.value.clone())
                                    .unwrap(),
                            ))
                        })
                        .collect(),
                ));
            }
            Ok(ResolvedValue::Object(
                self.object_schema
                    .object(&self.output_type, self.value.clone())
                    .unwrap(),
            ))
        })
    }
}

pub fn run(endpoint: &Endpoint<()>, query: &str) -> GraphQlResponse {
    block_on(endpoint.execute(GraphQlRequest::new(query), (), &NoDeadline))
}
