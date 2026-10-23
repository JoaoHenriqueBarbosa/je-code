// H16: carrega o TypeScript do oráculo e mede uma completion num arquivo pequeno, no motor que roda este arquivo.
// Uso: <motor> ts-completion.js <typescript.js> <full|es5>
'use strict';
const fs = require('fs');
const path = require('path');
const now = () => Number(process.hrtime.bigint()) / 1e6;
const tsPath = process.argv[2];
const libMode = process.argv[3] || 'full';
const out = {};

let t = now();
const source = fs.readFileSync(tsPath, 'utf8');
out.readMs = now() - t;

t = now();
const m = { exports: {} };
new Function('require', 'module', 'exports', '__dirname', '__filename', source)(require, m, m.exports, path.dirname(tsPath), tsPath);
out.loadMs = now() - t; // parse + eval do typescript.js (9 MB)
const ts = m.exports;
out.tsVersion = ts.version;

const libDir = path.dirname(tsPath);
const files = new Map([['/virt/a.ts', 'const words: string[] = ["a", "b"];\nconst n = words.map((w) => w.length);\nwords.']]);
const versions = new Map([['/virt/a.ts', 0]]);
const options = libMode === 'es5' ? { target: ts.ScriptTarget.ES5, lib: ['lib.es5.d.ts'], types: [], noEmit: true } : { target: ts.ScriptTarget.ES2022, types: [], noEmit: true };
const host = {
  getScriptFileNames: () => [...files.keys()],
  getScriptVersion: (f) => String(versions.get(f) || 0),
  getScriptSnapshot: (f) => {
    if (files.has(f)) return ts.ScriptSnapshot.fromString(files.get(f));
    return fs.existsSync(f) ? ts.ScriptSnapshot.fromString(fs.readFileSync(f, 'utf8')) : undefined;
  },
  getCurrentDirectory: () => '/virt',
  getCompilationSettings: () => options,
  getDefaultLibFileName: () => path.join(libDir, 'lib.d.ts'),
  fileExists: (f) => files.has(f) || fs.existsSync(f),
  readFile: (f) => (files.has(f) ? files.get(f) : fs.existsSync(f) ? fs.readFileSync(f, 'utf8') : undefined),
  directoryExists: () => true,
  getDirectories: () => [],
};

if (process.argv[4] === 'parse-only') {
  const dom = fs.readFileSync(path.join(libDir, 'lib.dom.d.ts'), 'utf8');
  t = now();
  const sf = ts.createSourceFile('lib.dom.d.ts', dom, ts.ScriptTarget.ES2022, true);
  out.createSourceFileDomMs = now() - t;
  out.domStatements = sf.statements.length;
  console.log(JSON.stringify(out));
  process.exit(0);
}

t = now();
const service = ts.createLanguageService(host, ts.createDocumentRegistry());
out.createServiceMs = now() - t;

const completions = (label) => {
  const text = files.get('/virt/a.ts');
  const t0 = now();
  const result = service.getCompletionsAtPosition('/virt/a.ts', text.length, { includeCompletion: true });
  const dt = now() - t0;
  out[label] = { ms: dt, entries: result ? result.entries.length : -1 };
  return result;
};
const first = completions('completionCold'); // inclui ler, parsear, ligar e checar lib + o arquivo
const names = new Set(first ? first.entries.map((e) => e.name) : []);
out.hasMapAndFilter = names.has('map') && names.has('filter') && names.has('length');
for (let i = 1; i <= 5; i++) {
  files.set('/virt/a.ts', files.get('/virt/a.ts') + (i === 1 ? '' : '') + '\n');
  versions.set('/virt/a.ts', i);
  files.set('/virt/a.ts', 'const words: string[] = ["a", "b"];\nconst n' + i + ' = words.map((w) => w.length);\nwords.');
  completions('completionWarm' + i); // arquivo editado, lib já em cache
}
t = now();
service.getSemanticDiagnostics('/virt/a.ts');
out.semanticDiagnosticsMs = now() - t;
console.log(JSON.stringify(out));
