// Oráculo do Array.prototype.sort do V8: para comparadores determinísticos (inclusive os que não são ordem total),
// registra a quantidade de chamadas, o hash FNV-1a da sequência de pares (a, b) passados ao comparador e o hash do
// vetor final. O porte (je_base::arrays::sort_by_compare) tem de reproduzir os três números.
'use strict';
function mix(x) {
  x = Math.imul(x ^ (x >>> 16), 0x7feb352d) >>> 0;
  x = Math.imul(x ^ (x >>> 15), 0x846ca68b) >>> 0;
  return (x ^ (x >>> 16)) >>> 0;
}
function pairHash(seed, a, b) {
  return mix((Math.imul(seed, 0x9e3779b1) ^ Math.imul(a + 1, 0x85ebca6b) ^ Math.imul(b + 7, 0xc2b2ae35)) >>> 0);
}
function fnv(h, v) {
  for (let i = 0; i < 4; i++) { h = Math.imul(h ^ ((v >>> (8 * i)) & 0xff), 0x01000193) >>> 0; }
  return h;
}
const modes = {
  // resultado arbitrário em {-1, 0, 1}: nada de ordem total
  random: (seed, a, b) => (pairHash(seed, a, b) % 3) - 1,
  // ordem por chave com empates, e 10% dos pares com resultado invertido
  noisy: (seed, a, b) => {
    const ka = mix(seed ^ a) % 50, kb = mix(seed ^ b) % 50;
    const r = ka < kb ? -1 : ka > kb ? 1 : 0;
    return pairHash(seed, a, b) % 10 === 0 ? -r : r;
  },
  // ordem total por chave com empates (estabilidade)
  consistent: (seed, a, b) => {
    const ka = mix(seed ^ a) % 50, kb = mix(seed ^ b) % 50;
    return ka - kb;
  },
};
const sizes = [];
for (let n = 0; n <= 70; n++) sizes.push(n);
sizes.push(100, 128, 129, 200, 256, 500, 1000, 1500, 2000);
const out = [];
for (const mode of Object.keys(modes)) {
  for (const n of sizes) {
    for (let seed = 1; seed <= 5; seed++) {
      const array = Array.from({ length: n }, (_, i) => i);
      let calls = 0, seq = 0x811c9dc5;
      array.sort((a, b) => { calls++; seq = fnv(fnv(seq, a), b); return modes[mode](seed, a, b); });
      let result = 0x811c9dc5;
      for (const v of array) result = fnv(result, v);
      out.push(JSON.stringify({ mode, n, seed, calls, seq, result }));
    }
  }
}
process.stdout.write(out.join('\n') + '\n');
