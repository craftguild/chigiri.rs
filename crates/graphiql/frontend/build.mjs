// We generate the fixed entrypoint and local worker, usually with esbuild, and then we transfer all of output to inventory.
import { build } from 'esbuild';
import { writeFile, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));
// This script is copied and run in a dedicated target work directory.
await rm(new URL('assets', import.meta.url), { recursive: true, force: true });
const result = await build({
  absWorkingDir: root,
  entryPoints: {
    app: 'entry.mjs',
    'worker-graphql': 'node_modules/monaco-graphql/esm/graphql.worker.js',
    'worker-json': 'node_modules/monaco-editor/esm/vs/language/json/json.worker.js',
    'worker-editor': 'node_modules/monaco-editor/esm/vs/editor/editor.worker.js',
  },
  outdir: 'assets', bundle: true, minify: true, splitting: true,
  format: 'esm', target: 'es2022', metafile: true, legalComments: 'external',
  loader: { '.ttf': 'file' }, define: { 'process.env.NODE_ENV': '"production"' },
  chunkNames: 'chunks/[name]-[hash]', assetNames: 'files/[name]-[hash]',
});
await writeFile(new URL('meta.json', import.meta.url), JSON.stringify(result.metafile));
