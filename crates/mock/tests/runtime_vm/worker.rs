//! Verify queued jobs stop waiting, return, and recover before the VM deadline.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source, Value};
use chigiri_mock::ir::{FieldId, MockIr};
use chigiri_mock::luau::{Contract, static_candidates};
use chigiri_mock::output::{ObjectData, OutputValue};
use chigiri_mock::service::Worker;
use chigiri_mock::store::{Related, Store};

pub fn cancel_running_future() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "blog.graphql",
            include_str!("../../fixtures/blog.graphql"),
        )])
        .unwrap(),
    );
    let field = |coordinate: &str| field(&ir, coordinate);
    let mut candidates = static_candidates(&ir).unwrap();
    for candidate in &mut candidates {
        let field = &ir.graph().fields()[ir.scripts()[candidate.script.0].field.0];
        let coordinate = format!("{}.{}", ir.graph().types()[field.parent.0].name, field.name);
        let body = match coordinate.as_str() {
            "Query.getUsers" => "return ctx.models.User:find({limit = args.limit :: number})",
            "Query.findPost" => "return ctx.models.Post:find({limit = args.limit :: number})",
            "Mutation.createUser" => {
                "return ctx.models.User:create({id = 'User-3', name = args.input.name})"
            }
            "Mutation.createPost" => {
                "return ctx.models.Post:create({id = 'Post-2', title = args.input.title, body = args.input.body, authorId = args.input.authorId})"
            }
            "Mutation.updatePost" => "return ctx.models.Post:find({limit = 1})[1]",
            "Mutation.deletePost" => "return true",
            _ => continue,
        };
        candidate.source = chigiri_mock::luau::render_module(&ir, candidate.script, body).unwrap();
    }
    for candidate in &mut candidates {
        if ir.scripts()[candidate.script.0].field == field("Mutation.createPost") {
            candidate.source = chigiri_mock::luau::render_module(
                &ir,
                candidate.script,
                r#"
if args.input.title == 'cancel' then
    ctx.models.User:create({id = 'discarded', name = 'discarded'})
    ctx.models.Post:create({id = 'Post-2', title = args.input.title, body = args.input.body, authorId = args.input.authorId})
    while true do end
end
return ctx.models.Post:create({id = 'Post-2', title = args.input.title, body = args.input.body, authorId = args.input.authorId})
"#,
            )
            .unwrap();
        }
    }
    let store = Store::from_seed_json(
        ir.clone(),
        r#"{
        "User":[{"id":"User-1","name":"Ada"},{"id":"User-2","name":"Grace"}],
        "Post":[{"id":"Post-1","title":"old","body":"body","authorId":"User-1"}]
    }"#,
    )
    .unwrap();
    let mut worker = Worker::start(Contract::new(ir.clone()).unwrap(), candidates, store).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let client = &worker.client;
        let query_args: Object = [("limit", Value::Int(20))].into_iter().collect();
        client.execute(field("Query.getUsers"), query_args.clone(), None).await.unwrap();
        let mut context = Context::from_waker(Waker::noop());
        let start = Instant::now();
        let deadline = start + Duration::from_millis(400);
        let mut target = Box::pin(client.execute(field("Mutation.createPost"), post("cancel"), None));
        assert!(target.as_mut().poll(&mut context).is_pending());
        let mut next = Box::pin(client.execute(field("Mutation.createPost"), post("next"), None));
        assert!(next.as_mut().poll(&mut context).is_pending());
        let mut queued = Vec::new();
        while queued.len() < 15 {
            assert!(Instant::now() < deadline, "worker did not dequeue target");
            let mut probe = Box::pin(client.execute(field("Query.getUsers"), query_args.clone(), None));
            match probe.as_mut().poll(&mut context) {
                Poll::Pending => queued.push(probe),
                Poll::Ready(Err(error)) => {
                    assert_eq!(error.message, "worker queue unavailable");
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Poll::Ready(Ok(_)) => panic!("target did not keep worker busy"),
            }
        }
        // The target and 16 following jobs remain pending, so dropping the target
        // removes it from a queue whose capacity is 16.
        // The hook exposes when Lua reaches the write, keeping the runtime bounded.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(target.as_mut().poll(&mut context).is_pending());
        drop(queued);
        drop(target);
        let output = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), next)
            .await
            .expect("request cancellation before the 500 ms VM deadline")
            .unwrap();
        let OutputValue::Object(created) = output else { panic!("model result required") };
        let ObjectData::Model(record) = created.data() else { panic!("model record required") };
        assert_eq!(record.id().unwrap(), "Post-2");
        let snapshot = created.snapshot();
        assert_eq!(snapshot.size().0, 4);
        let user = ir.model_for(ir.graph().type_id("User").unwrap()).unwrap();
        let post_model = ir.model_for(ir.graph().type_id("Post").unwrap()).unwrap();
        for (model, expected) in [(user, vec!["User-1", "User-2"]), (post_model, vec!["Post-1", "Post-2"])] {
            let rows = snapshot.reader().find(model, &query_args).unwrap();
            assert_eq!(rows.iter().map(|row| row.id().unwrap()).collect::<Vec<_>>(), expected);
        }
        for (id, expected) in [("User-1", "Post-1"), ("User-2", "Post-2")] {
            let Related::Many(rows) = snapshot.related(field("User.posts"), snapshot.record(user, id).unwrap()).unwrap() else { panic!("inverse list required") };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].as_ref().unwrap().id().unwrap(), expected);
        }
        let input: Object = [("name", Value::from("after"))].into_iter().collect();
        let args = [("input", Value::Object(input))].into_iter().collect();
        let output = client.execute(field("Mutation.createUser"), args, None).await.unwrap();
        let OutputValue::Object(created) = output else { panic!("model result required") };
        let ObjectData::Model(record) = created.data() else { panic!("model record required") };
        assert_eq!(record.id().unwrap(), "User-3");
        worker.stop(Instant::now() + Duration::from_secs(2)).await.unwrap();
        assert!(worker.client.execute(field("Query.getUsers"), query_args, None).await.is_err());
        println!("dequeued future drop: both models/order/counters/index restored, next job and stop/join succeeded");
    });
}

fn field(ir: &MockIr, coordinate: &str) -> FieldId {
    let (parent, name) = coordinate.split_once('.').unwrap();
    ir.graph().types()[ir.graph().type_id(parent).unwrap().0].fields[name]
}

fn post(title: &str) -> Object {
    let input = [
        ("title", Value::from(title)),
        ("body", Value::from("body")),
        ("authorId", Value::from("User-2")),
    ]
    .into_iter()
    .collect();
    [("input", Value::Object(input))].into_iter().collect()
}
