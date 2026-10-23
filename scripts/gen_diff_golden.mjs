// Carrega o diff.ts do VS Code com os tipos removidos e gera o golden do porte em Rust.
// Gera crates/je-base/tests/golden/diff.txt: carrega o diff.ts do VS Code (1.143.1) com os tipos removidos e
// roda os casos sorteados (semente fixa) no V8 real. Uso: node scripts/gen_diff_golden.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import module from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'je-diff-'));

const base = path.join(root, 'ref/vscode/src/vs/base/common/diff/') ;
let src = fs.readFileSync(base + 'diff.ts', 'utf8');
src = src.replace('constructor(private source: string) { }', 'constructor(source) { this.source = source; }');
src = src.replace("import { stringHash } from '../hash.js';", "import { stringHash } from './hash.mjs';");
src = src.replace("import { Constants } from '../uint.js';", "const Constants = { MAX_SAFE_SMALL_INTEGER: 1 << 30, MIN_SAFE_SMALL_INTEGER: -(1 << 30) };");
src = src.replace("import { DiffChange } from './diffChange.js';", "import { DiffChange } from './diffChange.mjs';");
src = src.replace(/const enum LocalConstants \{\s*MaxDifferencesHistory = 1447\s*\}/, 'const LocalConstants = { MaxDifferencesHistory: 1447 };');
fs.writeFileSync(path.join(work, 'diff.mjs'), module.stripTypeScriptTypes(src));
fs.writeFileSync(path.join(work, 'diffChange.mjs'), module.stripTypeScriptTypes(fs.readFileSync(base + 'diffChange.ts', 'utf8')));
fs.writeFileSync(path.join(work, 'hash.mjs'), `export function numberHash(val, initialHashVal) { return (((initialHashVal << 5) - initialHashVal) + val) | 0; }
export function stringHash(s, hashVal) { hashVal = numberHash(149417, hashVal); for (let i = 0, length = s.length; i < length; i++) { hashVal = numberHash(s.charCodeAt(i), hashVal); } return hashVal; }
`);
const { LcsDiff, StringDiffSequence, computeLevenshteinDistance } = await import(path.join(work, 'diff.mjs'));

// PRNG determinístico (mulberry32).
let seed = 20261023;
function rnd() { seed |= 0; seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }
const pick = (n) => Math.floor(rnd() * n);
const hex = (s) => s.length ? [...s].map((_, i) => s.charCodeAt(i).toString(16)).join(' ') : '-';

function randomString(alphabet, maxLen) {
  const n = pick(maxLen + 1);
  let r = '';
  for (let i = 0; i < n; i++) r += alphabet[pick(alphabet.length)];
  return r;
}
function mutate(s, alphabet) {
  let r = s;
  const edits = pick(6);
  for (let e = 0; e < edits; e++) {
    const p = pick(r.length + 1);
    const kind = pick(3);
    if (kind === 0) r = r.slice(0, p) + alphabet[pick(alphabet.length)] + r.slice(p);
    else if (kind === 1) r = r.slice(0, p) + r.slice(p + 1 + pick(3));
    else r = r.slice(0, p) + alphabet[pick(alphabet.length)] + r.slice(p + 1);
  }
  return r;
}

const out = [];
out.push(`# gerado por scripts/gen_diff_golden.mjs, Node ${process.version}`);

// 1) diff de caracteres (Int32Array), pretty e não pretty.
const alphabets = ['ab', 'abc', 'abcdefgh', 'ab \n'];
for (let i = 0; i < 400; i++) {
  const alphabet = alphabets[pick(alphabets.length)];
  const a = randomString(alphabet, 40);
  const b = rnd() < 0.5 ? mutate(a, alphabet) : randomString(alphabet, 40);
  for (const pretty of [false, true]) {
    const r = new LcsDiff(new StringDiffSequence(a), new StringDiffSequence(b)).ComputeDiff(pretty);
    out.push(`C ${pretty ? 1 : 0} ${hex(a)} | ${hex(b)} | ${r.quitEarly ? 1 : 0} ${r.changes.map(c => `${c.originalStart},${c.originalLength},${c.modifiedStart},${c.modifiedLength}`).join(' ') || '-'}`);
  }
}

// 2) diff de linhas (string[]), pretty, com linhas vazias e só de espaço para exercitar as fronteiras.
class LineSequence { constructor(lines) { this.lines = lines; } getElements() { return this.lines; } }
const words = ['foo', 'bar', 'baz', '', '  ', 'qux', 'a', 'bb', 'ccc', '\t', 'function', '}', '{'];
for (let i = 0; i < 400; i++) {
  const a = Array.from({ length: pick(25) }, () => words[pick(words.length)]);
  const b = a.slice();
  for (let e = pick(6); e > 0; e--) {
    const p = pick(b.length + 1);
    const kind = pick(3);
    if (kind === 0) b.splice(p, 0, words[pick(words.length)]);
    else if (kind === 1) b.splice(p, 1 + pick(2));
    else b[p] = words[pick(words.length)];
  }
  const r = new LcsDiff(new LineSequence(a), new LineSequence(b)).ComputeDiff(true);
  const enc = (l) => l.map(x => x.length ? [...x].map((_, k) => x.charCodeAt(k).toString(16)).join('.') : '-').join(' ') || '=';
  out.push(`L ${enc(a)} | ${enc(b)} | ${r.quitEarly ? 1 : 0} ${r.changes.map(c => `${c.originalStart},${c.originalLength},${c.modifiedStart},${c.modifiedLength}`).join(' ') || '-'}`);
}

// 3) distância de Levenshtein, curta e longa (mais de 32), inclusive blocos de 32 e de 64.
const lev = ['ab', 'abcd', 'abcdefghijklmnopqrstuvwxyz0123456789', 'ab é€'];
const pairs = [];
for (let i = 0; i < 300; i++) {
  const alphabet = lev[pick(lev.length)];
  const len = [5, 20, 31, 32, 33, 40, 64, 65, 100, 130][pick(10)];
  const a = randomString(alphabet, len);
  const b = rnd() < 0.5 ? mutate(a, alphabet) : randomString(alphabet, len);
  pairs.push([a, b]);
}
pairs.push(['', ''], ['abc', ''], ['', 'abc'], ['kitten', 'sitting'], ['a'.repeat(33), 'b'.repeat(70)], ['a'.repeat(32), 'a'.repeat(32)]);
for (const [a, b] of pairs) out.push(`V ${hex(a)} | ${hex(b)} | ${computeLevenshteinDistance(a, b)}`);

fs.writeFileSync(path.join(root, 'crates/je-base/tests/golden/diff.txt'), out.join('\n') + '\n');
console.log(out.length, 'linhas');

fs.rmSync(work, { recursive: true, force: true });
