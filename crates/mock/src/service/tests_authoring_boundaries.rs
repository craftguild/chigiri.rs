//! ADR-0047 HTTP acceptance for stored embedded values with private metadata.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chigiri::core::Source;
use chigiri::{BoxFuture, DeadlineFactory, Endpoint, FieldError, GraphQlRequest};
use serde_json::{Value, json};

use super::{Client, HttpScope, Worker, endpoint};
use crate::ir::MockIr;
use crate::luau::{Contract, ScriptCandidate, static_candidates};
use crate::store::Store;

struct Clock;

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

fn candidates(ir: &MockIr, edits: &[(&str, &str)]) -> Vec<ScriptCandidate> {
    let mut candidates = static_candidates(ir).unwrap();
    for (coordinate, body) in edits {
        let (parent, field) = coordinate.split_once('.').unwrap();
        let parent = ir.graph().type_id(parent).unwrap();
        let field = ir.graph().types()[parent.0].fields[field];
        let index = ir
            .scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap();
        candidates[index].source =
            crate::luau::render_module(ir, candidates[index].script, body).unwrap();
    }
    candidates
}

async fn query(endpoint: &Endpoint<Client>, client: &Client, source: &str) -> Value {
    let response = tokio::time::timeout(
        Duration::from_secs(8),
        endpoint.execute(GraphQlRequest::new(source), client.clone(), &Clock),
    )
    .await
    .unwrap();
    assert!(
        response.errors.is_empty(),
        "query: {source}\nerrors: {:?}",
        response
            .errors
            .iter()
            .map(|error| (&error.message, &error.path))
            .collect::<Vec<_>>()
    );
    chigiri_http::value_to_json(&response.data.unwrap()).unwrap()
}

#[tokio::test]
async fn ab_05_06_http_selects_public_children_and_private_virtual_parent_data() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        r#"
        interface Asset { url:String! }
        type Meta { label:String! }
        type Image implements Asset { url:String! nested:Meta! owner:String! @virtual }
        type Post { id:ID! image:Image! images:[Image!]! maybe:Image asset:Asset! }
        type Query { posts:[Post!]! }
        type Mutation { create:Post! replace:Post! }
        "#,
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type Image{ownerId:ID!} extend type Meta{secret:String!}",
    )];
    let ir = Arc::new(MockIr::parse_with_storage(public, storage).unwrap());
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = candidates(
        &ir,
        &[
            ("Query.posts", "return ctx.models.Post:find({})"),
            (
                "Image.owner",
                "return parent.ownerId .. ':' .. parent.nested.secret",
            ),
            (
                "Mutation.create",
                r#"local image={url='created',ownerId='u4',nested={label='m4',secret='s4'}}
                return ctx.models.Post:create({id='created',image=image,images={image},maybe=image,asset={__typename='Image' :: 'Image',url='asset-created',ownerId='u5',nested={label='m5',secret='s5'}}})"#,
            ),
            (
                "Mutation.replace",
                r#"local image={url='updated',ownerId='u6',nested={label='m6',secret='s6'}}
                return ctx.models.Post:update({where={field='id',op='eq',value='p'}},{image=image,images={image},maybe=image,asset={__typename='Image' :: 'Image',url='asset-updated',ownerId='u7',nested={label='m7',secret='s7'}}})[1]"#,
            ),
        ],
    );
    let seed = r#"{"Post":[{"id":"p","image":{"url":"one","ownerId":"u1","nested":{"label":"m1","secret":"s1"}},"images":[{"url":"two","ownerId":"u2","nested":{"label":"m2","secret":"s2"}}],"maybe":null,"asset":{"__typename":"Image","url":"three","ownerId":"u3","nested":{"label":"m3","secret":"s3"}}}]}"#;
    let mut worker = Worker::start(
        contract,
        candidates,
        Store::from_seed_json(ir, seed).unwrap(),
    )
    .unwrap();
    let client = &worker.client;
    let endpoint = endpoint(client.clone(), true, |_, _| {
        FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
    })
    .unwrap();
    assert_eq!(
        query(
            &endpoint,
            client,
            "{posts{id image{url owner nested{label}} images{url owner} maybe{url} asset{__typename ... on Image{url owner}}}}",
        )
        .await,
        json!({"posts":[{"id":"p","image":{"url":"one","owner":"u1:s1","nested":{"label":"m1"}},"images":[{"url":"two","owner":"u2:s2"}],"maybe":null,"asset":{"__typename":"Image","url":"three","owner":"u3:s3"}}]})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            "mutation{create{id image{url owner} images{url owner} maybe{url owner} asset{... on Image{url owner}}}}",
        )
        .await,
        json!({"create":{"id":"created","image":{"url":"created","owner":"u4:s4"},"images":[{"url":"created","owner":"u4:s4"}],"maybe":{"url":"created","owner":"u4:s4"},"asset":{"url":"asset-created","owner":"u5:s5"}}})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            "mutation{replace{id image{url owner} images{url owner} maybe{url owner} asset{... on Image{url owner}}}}",
        )
        .await,
        json!({"replace":{"id":"p","image":{"url":"updated","owner":"u6:s6"},"images":[{"url":"updated","owner":"u6:s6"}],"maybe":{"url":"updated","owner":"u6:s6"},"asset":{"url":"asset-updated","owner":"u7:s7"}}})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            "{image:__type(name:\"Image\"){fields{name}} meta:__type(name:\"Meta\"){fields{name}}}",
        )
        .await,
        json!({"image":{"fields":[{"name":"url"},{"name":"nested"},{"name":"owner"}]},"meta":{"fields":[{"name":"label"}]}})
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn ab_05_06_private_model_values_remain_internal_stored_slots() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        r#"
        type Image { url:String! }
        type Post { id:ID! image:Image! }
        type Query { posts:[Post!]! }
        type Mutation { create:Post! }
        "#,
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "type SecretMeta{secret:String!} extend type Image{meta:SecretMeta!}",
    )];
    let ir = Arc::new(MockIr::parse_with_storage(public, storage).unwrap());
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = candidates(
        &ir,
        &[
            ("Query.posts", "return ctx.models.Post:find({})"),
            (
                "Mutation.create",
                r#"local meta=ctx.models.SecretMeta:create({secret="private"})
                return ctx.models.Post:create({id="created",image={url="created",meta=meta}})"#,
            ),
        ],
    );
    let mut worker = Worker::start(
        contract,
        candidates,
        Store::from_seed_json(ir, "{}").unwrap(),
    )
    .unwrap();
    let client = &worker.client;
    let endpoint = endpoint(client.clone(), true, |_, _| {
        FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
    })
    .unwrap();
    assert_eq!(
        query(&endpoint, client, "mutation{create{id image{url}}}").await,
        json!({"create":{"id":"created","image":{"url":"created"}}})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            "{private:__type(name:\"SecretMeta\"){name}}"
        )
        .await,
        json!({"private":null})
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn ab_05_07_seed_reference_is_a_fixed_snapshot_while_relation_stays_live() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        r#"
        type Image { id:ID! url:String! owner:String! @virtual }
        type Post {
          id:ID!
          snapshot:Image!
          live:Image! @relation(fields:["liveId"],references:["id"])
        }
        type Query { posts:[Post!]! }
        type Mutation { rename:Image! }
        "#,
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type Image{ownerId:ID!}",
    )];
    let ir = Arc::new(MockIr::parse_with_storage(public, storage).unwrap());
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = candidates(
        &ir,
        &[
            ("Query.posts", "return ctx.models.Post:find({})"),
            ("Image.owner", "return parent.ownerId"),
            (
                "Mutation.rename",
                "return ctx.models.Image:update({where={field='id',op='eq',value='i'}},{url='new',ownerId='changed'})[1]",
            ),
        ],
    );
    let seed = r#"{"Image":[{"id":"i","url":"old","ownerId":"original"}],"Post":[{"id":"p","snapshot":{"$ref":{"model":"Image","key":{"id":"i"}}},"liveId":"i"}]}"#;
    let mut worker = Worker::start(
        contract,
        candidates,
        Store::from_seed_json(ir, seed).unwrap(),
    )
    .unwrap();
    let client = &worker.client;
    let endpoint = endpoint(client.clone(), true, |_, _| {
        FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
    })
    .unwrap();
    let selection = "{posts{id snapshot{id url owner} live{id url owner}}}";
    assert_eq!(
        query(&endpoint, client, selection).await,
        json!({"posts":[{"id":"p","snapshot":{"id":"i","url":"old","owner":"original"},"live":{"id":"i","url":"old","owner":"original"}}]})
    );
    assert_eq!(
        query(&endpoint, client, "mutation{rename{id url owner}}").await,
        json!({"rename":{"id":"i","url":"new","owner":"changed"}})
    );
    assert_eq!(
        query(&endpoint, client, selection).await,
        json!({"posts":[{"id":"p","snapshot":{"id":"i","url":"old","owner":"original"},"live":{"id":"i","url":"new","owner":"changed"}}]})
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn ab_06_10_private_dto_failure_rolls_back_writes_and_next_request_recovers() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        r#"
        type Image { url:String! }
        type Item { id:ID! }
        type Query { items:[Item!]! }
        type Mutation { bad:Image! good:Item! }
        "#,
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type Image{ownerId:ID!}",
    )];
    let ir = Arc::new(MockIr::parse_with_storage(public, storage).unwrap());
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = candidates(
        &ir,
        &[
            ("Query.items", "return ctx.models.Item:find({})"),
            (
                "Mutation.bad",
                "ctx.models.Item:create({id='rolled-back'}); return {url='x',ownerId='private-value'} :: any",
            ),
            (
                "Mutation.good",
                "return ctx.models.Item:create({id='kept'})",
            ),
        ],
    );
    let mut worker = Worker::start(contract, candidates, Store::empty(ir)).unwrap();
    let client = &worker.client;
    let endpoint = endpoint(client.clone(), true, |_, _| {
        FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
    })
    .unwrap();
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let captured = diagnostics.clone();
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap()).with_diagnostics(move |event| {
        captured.lock().unwrap().push(event);
    });
    let failed = tokio::time::timeout(
        Duration::from_secs(8),
        scope.run(endpoint.execute(
            GraphQlRequest::new("mutation{bad{url}}"),
            client.clone(),
            &Clock,
        )),
    )
    .await
    .unwrap();
    assert_eq!(failed.errors.len(), 1);
    assert!(!failed.errors[0].message.contains("private-value"));
    {
        let diagnostics = diagnostics.lock().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].cause, "MOCK_OUTPUT_PRIVATE_FIELD");
        assert_eq!(
            diagnostics[0].module.as_deref(),
            Some("resolvers/mutation/bad.luau")
        );
        assert_eq!(diagnostics[0].member.as_deref(), Some("Image.ownerId"));
        assert_eq!(
            diagnostics[0].actual.as_deref(),
            Some("private field supplied by resolver DTO")
        );
        assert!(
            diagnostics[0]
                .expected
                .as_deref()
                .unwrap()
                .contains("Image")
        );
        assert!(!format!("{:?}", diagnostics[0].hint).contains("private-value"));
        assert_eq!(diagnostics[0].line, None);
        assert_eq!(diagnostics[0].column, None);
    }
    assert_eq!(
        query(&endpoint, client, "{items{id}}").await,
        json!({"items":[]})
    );
    assert_eq!(
        query(&endpoint, client, "mutation{good{id}}").await,
        json!({"good":{"id":"kept"}})
    );
    assert_eq!(
        query(&endpoint, client, "{items{id}}").await,
        json!({"items":[{"id":"kept"}]})
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}
