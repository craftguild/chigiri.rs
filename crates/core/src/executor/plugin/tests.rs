//! Test middleware onion order and short-circuit behavior.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use crate::{GraphQlRequest, GraphQlResponse, Value};

use super::{Plugin, RequestNext};
use crate::executor::{BoxFuture, RequestContext};

struct Recorder {
    name: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    short_circuit: bool,
}

impl Plugin<()> for Recorder {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, ()>,
        next: RequestNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            self.events
                .lock()
                .unwrap()
                .push(format!("{} enter", self.name));
            let response = if self.short_circuit {
                GraphQlResponse::executed(Value::String("short".to_owned()))
            } else {
                next.run(context).await
            };
            self.events
                .lock()
                .unwrap()
                .push(format!("{} exit", self.name));
            response
        })
    }
}

#[test]
fn request_plugins_run_in_registration_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let plugins: Vec<Arc<dyn Plugin<()>>> = vec![
        Arc::new(recorder("A", &events, false)),
        Arc::new(recorder("B", &events, false)),
    ];
    let request = GraphQlRequest::new("{ value }");
    let context = RequestContext::new(&(), &request);
    let terminal_events = Arc::clone(&events);
    let next = RequestNext::new(&plugins, move |_| {
        Box::pin(async move {
            terminal_events.lock().unwrap().push("terminal".to_owned());
            GraphQlResponse::executed(Value::Null)
        })
    });

    block_on(next.run(context));

    assert_eq!(
        *events.lock().unwrap(),
        ["A enter", "B enter", "terminal", "B exit", "A exit"]
    );
}

#[test]
fn request_short_circuit_skips_inner_plugins_and_terminal_handler() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let plugins: Vec<Arc<dyn Plugin<()>>> = vec![
        Arc::new(recorder("A", &events, true)),
        Arc::new(recorder("B", &events, false)),
    ];
    let request = GraphQlRequest::new("invalid");
    let context = RequestContext::new(&(), &request);
    let next = RequestNext::new(&plugins, |_| {
        Box::pin(async { GraphQlResponse::executed(Value::Null) })
    });

    let response = block_on(next.run(context));

    assert_eq!(*events.lock().unwrap(), ["A enter", "A exit"]);
    assert_eq!(response.data, Some(Value::String("short".to_owned())));
}

fn recorder(name: &'static str, events: &Arc<Mutex<Vec<String>>>, short_circuit: bool) -> Recorder {
    Recorder {
        name,
        events: Arc::clone(events),
        short_circuit,
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
