# Mock quick start

This walkthrough creates a small Todo API, initializes a mock project,
implements its resolvers directly or with a coding agent, checks it, and starts
GraphiQL.
Run the commands from the repository root.

### 1. Build `chigiri`

Framework libraries, generated SDKs, the unified CLI and its Mock namespace
require Rust 1.95. Source builds use the fixed Rust 1.95.0 toolchain.
Prepare the pinned engine, locked dependencies and native tools with
[Building Chigiri](../building.md#native-builds) before running these commands.

```sh
rustup toolchain install 1.95.0 --profile minimal
make debug
export CHIGIRI="$PWD/target/$(rustc +1.95.0 -vV | sed -n 's/^host: //p')/debug/chigiri"
```

`make` and `make build` create the native release binary without changing
distribution candidates. The four named cross-target builds have separate,
explicit prerequisites; see [Building Chigiri](../building.md).

### 2. Write a schema

```sh
mkdir -p target/todo-demo
cat > target/todo-demo/schema.graphql <<'GRAPHQL'
type Todo {
  id: ID!
  title: String!
  completed: Boolean!
}

type Query {
  todos: [Todo!]!
}

type Mutation {
  addTodo(title: String!): Todo!
}
GRAPHQL
```

### 3. Initialize the mock project

```sh
$CHIGIRI mock init \
  --schema target/todo-demo/schema.graphql \
  --config-dir target/todo-demo/mock
```

`init` validates the schema and creates an editable project. It never
overwrites an existing project directory.

```text
target/todo-demo/mock/
├── AGENTS.md
├── mock.luau
├── storage.graphql
├── resolvers/
│   ├── mutation/addTodo.luau
│   └── query/todos.luau
├── seeds/Todo.jsonl
├── hooks/
└── generated/
```

The new resolver files contain a typed, intentional `NOT_IMPLEMENTED` business
failure. The untouched project passes contract validation and can be checked,
served, or exported; invoking either operation returns that explicit GraphQL
error until its body is replaced.

### 4. Implement the resolvers

The project root contains `AGENTS.md`, which tells coding agents which files
they may edit and which commands define correctness. Open
`target/todo-demo/mock` as the agent workspace and ask it:

```text
Implement the Todo query and mutation resolvers. Follow AGENTS.md, inspect
scoped Chigiri references as needed, run target checks while editing, and finish
with a full chigiri mock check. Do not edit generated files or the source SDL.
```

The agent can request only the contracts it needs:

```sh
$CHIGIRI mock reference types Todo \
  --config-dir target/todo-demo/mock --format json
$CHIGIRI mock reference models Todo \
  --config-dir target/todo-demo/mock --format json
$CHIGIRI mock reference resolvers Query.todos \
  --config-dir target/todo-demo/mock --format json
```

After editing one file, the agent can validate it while another resolver still
uses its generated intentional-error behavior:

```sh
$CHIGIRI mock check \
  --config-dir target/todo-demo/mock \
  --target resolvers/query/todos.luau \
  --format json
```

A successful target check reports `valid: true` and
`readiness_checked: false` because it did not validate the whole project.
Review the agent's ordinary file diff before continuing.

To complete the same step manually, keep each generated function signature,
leave the one-field `{ resolve = ... }` descriptor intact, and replace the
generated `ctx.fail(...)` body with these bodies:

| File | Resolver body |
| --- | --- |
| `resolvers/query/todos.luau` | `return ctx.models.Todo:find({})` |
| `resolvers/mutation/addTodo.luau` | `return ctx.models.Todo:create({ title = args.title, completed = false })` |

The empty `seeds/Todo.jsonl` is valid. Add one JSON object per line if the demo
should start with data.

### 5. Check readiness

```sh
$CHIGIRI mock check --config-dir target/todo-demo/mock
```

A ready project prints:

```text
chigiri mock check: readiness_checked = true
```

`check` does not rewrite project files or invoke resolver/hook entrypoints. It
reports schema problems, invalid seed data, missing or malformed modules, Luau
type errors, initialization failures, identity drift, and resource violations.
It validates executable contracts, not business completeness. Use `--format json` for the closed
`chigiri-mock-check/v3` diagnostic format. Each diagnostic identifies the
affected logical `member` and uses bounded `cause`, `expected`, `actual`, and
`hint` fields plus at most four relevant named reference selections, without
exposing physical paths or source values. `serve` and `export` repeat full
validation against their current captured inputs rather than trusting an
earlier check.

### 6. Serve the API

```sh
$CHIGIRI mock serve \
  --config-dir target/todo-demo/mock \
  --enable-graphiql
```

The server listens on `http://127.0.0.1:4000/graphql`. Open
`http://127.0.0.1:4000/graphiql` or query it from another terminal:

```sh
curl -sS http://127.0.0.1:4000/graphql \
  -H 'content-type: application/json' \
  --data '{"query":"{ todos { id title completed } }"}'

curl -sS http://127.0.0.1:4000/graphql \
  -H 'content-type: application/json' \
  --data '{"query":"mutation { addTodo(title: \"Read the Chigiri docs\") { id title completed } }"}'
```

The first query returns `{"data":{"todos":[]}}`. The mutation creates a Todo
with an automatically generated ID. Mock data returns to its seed state whenever
the process restarts. Press `Ctrl-C` to stop the server.

GraphiQL and schema introspection are disabled unless `--enable-graphiql` is
passed. They are intended for local development.

## Working with a mock project

Use `reference` to inspect the generated model API and the limits enforced by
the current binary:

```sh
$CHIGIRI mock reference all \
  --config-dir target/todo-demo/mock
```

The project uses the public SDL adopted at `init`, so `reference`, `check`,
`serve`, and `export` do not depend on the original source remaining present.
After changing private `storage.graphql` or generation-bound settings, run
`codegen`. To adopt an authorized public SDL change, compare it first if useful,
then run `update`. Both writers protect user-edited resolvers and seeds; review
any `.dist` and recovery material before continuing.

```sh
$CHIGIRI mock codegen --config-dir target/todo-demo/mock
$CHIGIRI mock update --config-dir target/todo-demo/mock --check
# Run this only when choosing to adopt the current public SDL:
$CHIGIRI mock update --config-dir target/todo-demo/mock
$CHIGIRI mock check --config-dir target/todo-demo/mock
```

Export a checked project when it needs to run without its source directory or
original SDL:

```sh
$CHIGIRI mock export \
  --config-dir target/todo-demo/mock \
  --output target/todo-demo/portable.toml

$CHIGIRI mock serve \
  --config target/todo-demo/portable.toml \
  --enable-graphiql
```

See the [mock project workflow](../mock-project-workflow.md),
[ADR-0044](../adr/0044-composable-agent-ready-mock-authoring.md),
[ADR-0045](../adr/0045-stateful-mock-authoring-and-runtime-contract-parity.md),
and the `chigiri mock reference all` output for the full contracts.
