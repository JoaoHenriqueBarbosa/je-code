// H16: custo de parse (compilar) e de avaliar um bundle grande, no motor que roda este arquivo.
// Uso: <motor> bundle-cost.js <bundle.js>
'use strict';
const fs = require('fs');
const path = require('path');
const now = () => Number(process.hrtime.bigint()) / 1e6;
const file = process.argv[2];
let source = fs.readFileSync(file, 'utf8');
const out0 = { rewrittenImports: 0 };
// O extensionHostProcess.js é ESM e `new Function` só aceita corpo de script: os `import` de topo viram
// `require` (as mesmas linhas nos dois motores), e a chamada final de startup é cortada para medir só a
// avaliação do módulo (definições), sem tentar abrir o IPC do VS Code.
if (/(^|[;\n])import[{ ]/.test(source)) {
  source = source.replace(/import\s*(\{[^}]*\}|\*\s*as\s+[\w$]+|[\w$]+)\s*from\s*"([^"]+)";?/g, (_m, spec, mod) => {
    out0.rewrittenImports++;
    let lhs = spec;
    if (spec.startsWith('{')) lhs = spec.replace(/\s+as\s+/g, ':');
    else if (spec.startsWith('*')) lhs = spec.replace(/\*\s*as\s+/, '');
    return 'const ' + lhs + '=__req(' + JSON.stringify(mod) + ');';
  });
  source = source.replace(/import\.meta\.url/g, JSON.stringify('file://' + file)).replace(/import\.meta\.dirname/g, JSON.stringify(path.dirname(file)));
  source = source.replace(/;[\w$]+\(\)\.catch\(r=>console\.log\(r\)\);\s*(\/\/# sourceMappingURL.*)?$/, ';');
}
const out = { bytes: source.length, rewrittenImports: out0.rewrittenImports, compileMs: [] };
for (let i = 0; i < 3; i++) {
  const t = now();
  try { new Function('__req', 'module', 'exports', '__dirname', '__filename', source); } catch (e) { out.compileError = String(e.message).slice(0, 200); break; }
  out.compileMs.push(Math.round((now() - t) * 10) / 10);
}
const missing = [];
// O NLS do VS Code vem de `globalThis._VSCODE_NLS_MESSAGES` (carregado antes pelo bootstrap): um vetor que responde a todo índice.
globalThis._VSCODE_NLS_MESSAGES = new Proxy([], { get: (t, k) => (typeof k === 'string' && /^\d+$/.test(k) ? 'msg' : t[k]) });
globalThis._VSCODE_FILE_ROOT = path.dirname(file);
// Pacotes que o oráculo guarda dentro do node_modules.asar não existem em disco: um módulo vazio no lugar.
const req = (id) => {
  if (id === 'node:module' || id === 'module') {
    // Registrado como lacuna do zjsc se faltar: `createRequire` e `_nodeModulePaths`.
    let real = {};
    try { real = require(id); } catch (e) { missing.push(id + ': ' + String(e.message).split('\n')[0]); }
    if (typeof real.createRequire !== 'function') missing.push(id + ': createRequire ausente (substituído pelo require do sonda)');
    return Object.assign(Object.create(null), real, { createRequire: typeof real.createRequire === 'function' ? real.createRequire : () => req });
  }
  try { return require(id); } catch (e) { missing.push(id + ': ' + String(e.message).split('\n')[0]); return new Proxy(function () {}, { get: (_t, k) => (k === 'default' ? req.empty : req.empty), apply: () => req.empty, construct: () => req.empty }); }
};
req.empty = function () {};
const m = { exports: {} };
if (out.compileError) { console.log(JSON.stringify(out)); process.exit(1); }
const fn = new Function('__req', 'module', 'exports', '__dirname', '__filename', source);
const t = now();
let error = null;
try { fn(req, m, m.exports, path.dirname(file), file); } catch (e) { error = String(e && e.stack || e).split('\n').slice(0, 3).join(' | '); }
out.evalMs = Math.round((now() - t) * 10) / 10;
out.evalError = error;
out.missingModules = missing;
out.exportKeys = Object.keys(m.exports).length;
console.log(JSON.stringify(out));
process.exit(0);
