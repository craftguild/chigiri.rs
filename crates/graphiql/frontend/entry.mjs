import React from 'react';
import { createRoot } from 'react-dom/client';
import { GraphiQL } from 'graphiql';
import '@graphiql/react/style.css';
import 'graphiql/graphiql.css';
import '@graphiql/plugin-doc-explorer/style.css';
import '@graphiql/plugin-history/style.css';
import './layout.css';

// Resolve the worker relative to this entry module; do not use a CDN or blob worker.
/** @type {typeof globalThis & {MonacoEnvironment: import('monaco-editor').Environment}} */
(globalThis).MonacoEnvironment = {
  getWorker(_id, label) {
    const name = label === 'graphql' ? 'worker-graphql.js'
      : label === 'json' ? 'worker-json.js' : 'worker-editor.js';
    return new Worker(new URL(name, import.meta.url), { type: 'module' });
  },
};

const root = document.getElementById('root');
// The settings are not created by running the Rust-side HTML attribute escape from the query/Host/environment value of the URL.
const endpoint = new URL(root.dataset.endpoint, window.location.origin);
if (endpoint.origin !== window.location.origin || endpoint.search || endpoint.hash) {
  throw new Error('Invalid GraphQL endpoint');
}

// Keep history, settings, and headers in memory for the lifetime of this page.
const memory = new Map();
const storage = {
  getItem: key => memory.get(key) ?? null,
  setItem: (key, value) => memory.set(key, value),
  removeItem: key => memory.delete(key),
  clear: () => memory.clear(),
  get length() { return memory.size; },
};

// Just a JSON POST connection to transfer the operation that GraphiQL analyzes to an existing Endpoint.
const fetcher = async (parameters, options) => {
  const headers = new Headers(options?.headers);
  headers.set('content-type', 'application/json');
  headers.set('accept', 'application/graphql-response+json');
  headers.set('x-chigiri-preflight', '1');
  const response = await fetch(endpoint, {
    method: 'POST', credentials: 'same-origin', redirect: 'error', headers,
    body: JSON.stringify(parameters),
  });
  return response.json();
};

createRoot(root).render(React.createElement(GraphiQL, {
  fetcher,
  storage,
  shouldPersistHeaders: false,
  showPersistHeadersSettings: false,
}));
