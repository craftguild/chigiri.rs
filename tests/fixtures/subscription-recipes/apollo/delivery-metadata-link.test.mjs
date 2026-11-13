import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  ApolloClient,
  ApolloLink,
  InMemoryCache,
  gql,
} from "@apollo/client/core";
import { Observable } from "rxjs";

import { deliveryMetadataLink } from "./delivery-metadata-link.mjs";

const subscription = gql`
  subscription Presence {
    presence {
      online
    }
  }
`;

function execute(link) {
  const client = new ApolloClient({
    cache: new InMemoryCache(),
    link,
  });
  return ApolloLink.execute(link, { query: subscription }, { client });
}

function collect(observable) {
  return new Promise((resolve, reject) => {
    const values = [];
    observable.subscribe({
      next(value) {
        values.push(value);
      },
      error: reject,
      complete() {
        resolve(values);
      },
    });
  });
}

test("uses the reviewed Apollo Client release", async () => {
  const manifest = JSON.parse(
    await readFile(
      new URL("./node_modules/@apollo/client/package.json", import.meta.url),
      "utf8",
    ),
  );
  assert.equal(manifest.version, "4.3.5");
});

test("observes canonical delivery metadata and forwards raw results", async () => {
  const results = [
    {
      data: { presence: { online: 4 } },
      extensions: {
        chigiri: { delivery: { droppedTotal: null } },
        application: { trace: "first" },
      },
    },
    {
      data: { presence: { online: 5 } },
      errors: [{ message: "ordinary field error" }],
      extensions: {
        chigiri: { delivery: { droppedTotal: 3 } },
        application: { trace: "second" },
      },
    },
    {
      data: { presence: { online: 6 } },
      extensions: { application: { trace: "metadata absent" } },
    },
    {
      data: { presence: { online: 7 } },
      extensions: { chigiri: { delivery: { droppedTotal: "private" } } },
    },
  ];
  const observed = [];
  let completed = false;
  const terminal = new ApolloLink(
    () =>
      new Observable((observer) => {
        for (const result of results) {
          observer.next(result);
        }
        observer.complete();
      }),
  );
  const link = ApolloLink.from([
    deliveryMetadataLink((metadata) => observed.push(metadata)),
    terminal,
  ]);
  const received = [];
  await new Promise((resolve, reject) => {
    execute(link).subscribe({
      next(result) {
        received.push(result);
      },
      error: reject,
      complete() {
        completed = true;
        resolve();
      },
    });
  });

  assert.equal(completed, true);
  assert.equal(received.length, results.length);
  for (let index = 0; index < results.length; index += 1) {
    assert.strictEqual(received[index], results[index]);
  }
  assert.deepEqual(observed, [
    { operationName: "Presence", droppedTotal: null },
    { operationName: "Presence", droppedTotal: 3 },
  ]);
});

test("does not swallow a source error", async () => {
  const sourceError = new Error("source failed");
  const observed = [];
  const terminal = new ApolloLink(
    () =>
      new Observable((observer) => {
        observer.error(sourceError);
      }),
  );
  const link = ApolloLink.from([
    deliveryMetadataLink((metadata) => observed.push(metadata)),
    terminal,
  ]);

  await assert.rejects(collect(execute(link)), (error) => error === sourceError);
  assert.deepEqual(observed, []);
});

test("forwards unsubscription to the source exactly once", () => {
  let teardowns = 0;
  let completed = false;
  const terminal = new ApolloLink(
    () =>
      new Observable(() => () => {
        teardowns += 1;
      }),
  );
  const link = ApolloLink.from([deliveryMetadataLink(() => {}), terminal]);
  const observer = execute(link).subscribe({
    complete() {
      completed = true;
    },
  });

  observer.unsubscribe();
  observer.unsubscribe();
  assert.equal(teardowns, 1);
  assert.equal(completed, false);
});
