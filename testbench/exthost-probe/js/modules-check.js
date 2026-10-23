// Quais módulos `node:*` o extension host do VS Code importa e o motor entrega, e se `createRequire` existe.
'use strict';
const names = ['assert', 'child_process', 'crypto', 'events', 'fs', 'fs/promises', 'http', 'https', 'module', 'net', 'os', 'path', 'stream', 'string_decoder', 'tls', 'url', 'util', 'vm', 'v8', 'zlib', 'worker_threads', 'readline', 'perf_hooks', 'timers', 'tty', 'dns', 'buffer', 'querystring', 'async_hooks', 'inspector', 'http2', 'diagnostics_channel', 'process'];
const ok = [];
const missing = [];
for (const n of names) {
  try { require('node:' + n); ok.push(n); } catch (e) { missing.push(n); }
}
const m = (() => { try { return require('node:module'); } catch { return {}; } })();
console.log(JSON.stringify({ ok, missing, createRequire: typeof m.createRequire, nodeModulePaths: typeof m._nodeModulePaths, processVersions: typeof process.versions, processBinding: typeof process.binding }));
