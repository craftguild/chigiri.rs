//! Executable runtime discovery derived from the host capability contract.

use super::topic_types::{CodeExampleTopicDoc, RuntimeTopicDoc};

pub(crate) const STORAGE_PUBLIC_EXAMPLE: &str = r#"type Image { url: String! viewerLabel: String! @virtual }
type Wallet { id: ID! balance: Int! }
type User {
  id: ID!
  name: String!
  avatar: Image!
  walletId: ID!
  wallet: Wallet! @relation(fields: ["walletId"], references: ["id"])
}
type Query { user(id: ID!): User }"#;
pub(crate) const STORAGE_SDL_EXAMPLE: &str = r#"extend type User { verifiedAt: String }
extend type Image { ownerId: ID! }
type MockPreference {
  id: ID!
  userId: ID!
  theme: String!
  user: User! @relation(fields: ["userId"], references: ["id"])
}"#;
pub(crate) const STORAGE_USER_SEED_EXAMPLE: &str = r#"{"id":"u1","name":"Akari","avatar":{"url":"/avatars/u1.png","ownerId":"u1"},"walletId":"w1","verifiedAt":null}"#;
pub(crate) const STORAGE_WALLET_SEED_EXAMPLE: &str = r#"{"id":"w1","balance":125}"#;
pub(crate) const STORAGE_PREFERENCE_SEED_EXAMPLE: &str =
    r#"{"id":"p1","userId":"u1","theme":"dark"}"#;
pub(crate) const STORAGE_RESOLVER_EXAMPLE: &str =
    "return ctx.models.User:find({where={field='id',op='eq',value=args.id}})[1] or ctx.null";
pub(crate) const STORAGE_IMAGE_RESOLVER_EXAMPLE: &str = r#"local request = ctx.request
if type(request) == "table" then
  for _, header in request.headers do
    if header.name == "x-viewer" then return header.value .. ":" .. parent.ownerId end
  end
end
return "anonymous:" .. parent.ownerId"#;
pub(crate) const REQUEST_HEADERS_EXAMPLE: &str = r#"local request = ctx.request
if type(request) ~= "table" then return ctx.null end
local values: {string} = {}
for _, header in request.headers do
  if string.lower(header.name) == "x-tag" then table.insert(values, header.value) end
end
return values"#;
pub(crate) const REQUEST_AUTH_RESOLVER_EXAMPLE: &str = r#"local request = ctx.request
if type(request) ~= "table" then
  ctx.fail({code = "UNAUTHENTICATED", message = "An HTTP request is required."})
end
for _, header in request.headers do
  if string.lower(header.name) == "authorization" and header.value == "Bearer local-demo" then
    return true
  end
end
ctx.fail({code = "UNAUTHENTICATED", message = "The demo token is required."})"#;
pub(crate) const UTC_COMPARISON_EXAMPLE: &str = r#"local nowText = os.date("!%Y-%m-%dT%H:%M:%SZ", assert(os.time()))
if type(nowText) ~= "string" then
  return error("failed to format UTC time")
end
-- args.deadline must use the same UTC format, with no fractional seconds.
return args.deadline > nowText"#;
pub(crate) const REQUEST_AUTH_HOOK_EXAMPLE: &str = r#"local authenticated = false
for _, header in ctx.request.headers do
  if header.name == "authorization" and header.value == "Bearer local-demo" then
    authenticated = true
  end
end
return {statePatch = {authenticated = authenticated}}"#;

pub(super) fn build(doc: &super::ReferenceDocument) -> Vec<RuntimeTopicDoc> {
    vec![
        stdlib(),
        request(),
        timeleap(),
        errors(),
        storage(),
        super::runtime_events::pubsub(doc),
        super::runtime_events::generator(doc),
    ]
}

fn stdlib() -> RuntimeTopicDoc {
    let signatures = crate::api::runtime::GLOBALS
        .iter()
        .chain(
            crate::api::runtime::LIBRARIES
                .iter()
                .filter(|library| library.name != "timeleap")
                .flat_map(|library| library.functions),
        )
        .map(|function| function.signature.to_owned())
        .collect();
    let mut document = item(
        "stdlib",
        "Closed Luau standard-function allowlist, including selected os time functions.",
        &[
            "configuration",
            "module initialization",
            "hooks",
            "resolvers",
        ],
        signatures,
        &[
            "shared work, memory, deadline, cancellation, and reentry limits",
            "strings 4096 bytes; patterns 256 bytes; table sequences 1000 items",
            "initialization allows explicit os conversions and os.difftime only",
            "os.time(), current-time os.date(), and os.clock() require live execution",
            "timeleap changes apparent current wall time only; explicit conversions are unchanged",
            "require and shared Luau modules are unsupported; keep helpers local to each resolver or hook",
            "os.date may return string, a date table, or nil; guard with type and return error(...) before string comparisons",
            "lexical timestamp ordering requires the same canonical UTC format and fractional precision; arbitrary RFC 3339 strings are not interchangeable",
        ],
        "Read apparent time and perform an explicit UTC conversion",
        "local now = os.time()\nlocal target = os.time({year=2026, month=9, day=20, hour=9, min=0, sec=0})\nassert(target ~= nil)\nreturn {now=now, utc=os.date(\"!*t\", target)}",
    );
    document.examples.push(CodeExampleTopicDoc {
        title: "Compare canonical UTC timestamps (Boolean resolver body; args.deadline: String!)"
            .into(),
        code: UTC_COMPARISON_EXAMPLE.into(),
    });
    document
}

fn request() -> RuntimeTopicDoc {
    let mut document = item(
        "request",
        "Readonly transport snapshot shared by every resolver phase in one request.",
        &[
            "query resolvers",
            "mutation resolvers",
            "nested resolvers",
            "optional request.before preparation with declared state",
        ],
        vec!["ctx.request: ChigiriHookRequest | ChigiriNull".into()],
        &[
            "bounded value-copy budget",
            "non-HTTP execution returns ctx.null",
            "use a local request and a type(request) table guard; equality with ctx.null alone does not refine the pinned checker type",
            "demo tokens illustrate mock behavior, not production authentication",
        ],
        "Read duplicate-preserving headers (x-tag values; nullable String-list resolver body)",
        REQUEST_HEADERS_EXAMPLE,
    );
    document.examples.push(CodeExampleTopicDoc {
        title: "Require the demo Bearer token (Boolean resolver body)".into(),
        code: REQUEST_AUTH_RESOLVER_EXAMPLE.into(),
    });
    document.examples.push(CodeExampleTopicDoc {
        title: "Optional shared authentication preparation after declaring authenticated state"
            .into(),
        code: REQUEST_AUTH_HOOK_EXAMPLE.into(),
    });
    document
}

fn timeleap() -> RuntimeTopicDoc {
    let signatures = crate::api::runtime::LIBRARIES
        .iter()
        .find(|library| library.name == "timeleap")
        .expect("timeleap capability")
        .functions
        .iter()
        .map(|function| function.signature.to_owned())
        .collect();
    let mut document = item(
        "timeleap",
        "Direct control of apparent current time with explicit phase and world authority.",
        &[
            "request.before and operation.before: request-local control only",
            "authorized non-HTTP request preparation: request-local control only",
            "dedicated host-owned controller: world = true only",
            "unavailable during initialization, fields, resolvers, and after hooks",
        ],
        signatures,
        &[
            "timestamps are exact integer Unix seconds from 0 through 9,007,199,254,740,991 inclusive and must pass native UTC round-trip",
            "options are closed to world: boolean?; scope and global are rejected",
            "omitted, nil, empty options, and world = false select the current request",
            "request reset resumes the server policy captured for that request",
            "world = true requires the host controller and affects later requests in one server",
            "world reset restores that server's immutable startup policy",
        ],
        "Request preparation",
        "local target = os.time({year=2026, month=9, day=20, hour=9, min=0, sec=0})\nassert(target ~= nil)\ntimeleap.travel_to(target)\nassert(os.time() == target)\ntimeleap.reset()",
    );
    document.examples.push(CodeExampleTopicDoc {
        title: "Dedicated host-owned controller".into(),
        code: "timeleap.travel_to(1789894800, {world=true})\ntimeleap.reset({world=true})".into(),
    });
    document
}

fn errors() -> RuntimeTopicDoc {
    item(
        "errors",
        "Explicit public business failures remain distinct from internal exceptions.",
        &[
            "query resolvers",
            "mutation resolvers",
            "nested resolvers",
            "named consumer setup and execution",
        ],
        vec!["ctx.fail({code: string, message: string?}) -> never".into()],
        &[
            "code [A-Z][A-Z0-9_]{0,63}",
            "MOCK_ prefix reserved",
            "bounded message bytes",
        ],
        "Reject a business operation",
        "ctx.fail({code = \"INSUFFICIENT_BALANCE\", message = \"Balance too low\"})",
    )
}

fn storage() -> RuntimeTopicDoc {
    let mut document = item(
        "storage",
        "Private SDL models and slots share the normal request-atomic model store.",
        &[
            "read in all resolvers",
            "write in mutation-root resolvers and named consumer execution; source setup remains read-only",
        ],
        vec![
            "storage = { sources = { \"storage.graphql\" } }".into(),
            "{\"field\":{\"$ref\":{\"model\":\"User\",\"key\":{\"id\":\"u1\"}}}}".into(),
        ],
        &[
            "same seed, value, record, relation, and request candidate limits",
            "return genuine model records unchanged, including inside payloads or lists; private slots stay internal",
            "stored embedded values may retain private parent slots but remain snapshots, not live model records",
            "arbitrary output DTOs may contain public fields only; a copied ID or a type assertion does not grant record provenance",
            "static check does not prove output provenance; selecting a DTO with private fields fails at runtime with MOCK_OUTPUT_PRIVATE_FIELD",
            "for that failure return an unchanged model record, or construct an allowed public-only DTO; do not mutate a record or strip fields to forge one",
        ],
        "Required public schema",
        STORAGE_PUBLIC_EXAMPLE,
    );
    document.examples.extend([
        CodeExampleTopicDoc {
            title: "storage.graphql".into(),
            code: STORAGE_SDL_EXAMPLE.into(),
        },
        CodeExampleTopicDoc {
            title: "seeds/User.jsonl".into(),
            code: STORAGE_USER_SEED_EXAMPLE.into(),
        },
        CodeExampleTopicDoc {
            title: "seeds/MockPreference.jsonl".into(),
            code: STORAGE_PREFERENCE_SEED_EXAMPLE.into(),
        },
        CodeExampleTopicDoc {
            title: "seeds/Wallet.jsonl".into(),
            code: STORAGE_WALLET_SEED_EXAMPLE.into(),
        },
        CodeExampleTopicDoc {
            title: "Query.user resolver body".into(),
            code: STORAGE_RESOLVER_EXAMPLE.into(),
        },
        CodeExampleTopicDoc {
            title: "Image.viewerLabel resolver body".into(),
            code: STORAGE_IMAGE_RESOLVER_EXAMPLE.into(),
        },
    ]);
    document
}

fn item(
    name: &str,
    purpose: &str,
    availability: &[&str],
    signatures: Vec<String>,
    limits: &[&str],
    title: &str,
    code: &str,
) -> RuntimeTopicDoc {
    RuntimeTopicDoc {
        name: name.into(),
        purpose: purpose.into(),
        availability: availability.iter().map(|value| (*value).into()).collect(),
        signatures,
        limits: limits.iter().map(|value| (*value).into()).collect(),
        examples: vec![CodeExampleTopicDoc {
            title: title.into(),
            code: code.into(),
        }],
    }
}
