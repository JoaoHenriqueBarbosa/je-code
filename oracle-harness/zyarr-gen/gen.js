const fs = require('fs');
const { C } = require('./cases.js');
const node = require('./node.json');
const errsNode = require('./errs_node.json');
const errsBun = require('./errs_bun.json');
const outDir = require('path').resolve(__dirname, '../../crates/zyarr/tests');

const rs = (s) => {
  let o = '"';
  for (const ch of s) {
    const c = ch.codePointAt(0);
    if (ch === '"') o += '\\"';
    else if (ch === '\\') o += '\\\\';
    else if (c >= 0x20 && c < 0x7f) o += ch;
    else o += `\\u{${c.toString(16)}}`;
  }
  return o + '"';
};
const opt = (r) => (r ? `Some((${r[0]}, ${r[1]}))` : 'None');
const header = (topic, desc, needUnits) => `//! ${desc}\n//!\n//! Os valores esperados saem do Node real (V8 ${process.version}, \`RegExp.prototype.exec\` com a flag\n//! \`d\` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.\n//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.\n\nmod common;\n\nuse common::{check, ${needUnits ? 'check_units, ' : ''}Expected};\n\n`;

const titles = {
  syntax: 'Sintaxe básica: literais, quantificadores, classes, âncoras, lookahead, escapes.',
  flags: 'As flags `i m s u y` (e `d g v`): comportamento de cada uma sobre os mesmos padrões.',
  lookbehind: 'Lookbehind positivo e negativo: casamento para trás, capturas e backreferences dentro dele.',
  named_groups: 'Grupos nomeados: `groups`, `\\k<nome>`, nomes duplicados em alternativas e nomes Unicode.',
  backreference: 'Backreferences numéricas: grupo que não participou, referência adiante, octal legado.',
  unicode_properties: 'Classes Unicode `\\p{...}` e `\\P{...}` em modo `u` e `v`.',
  surrogates: 'Pares substitutos: modo `u` (ponto de código) contra modo não `u` (unidade de código).',
};
const byTopic = {};
node.forEach((c) => (byTopic[c.topic] ||= []).push(c));
for (const [topic, cases] of Object.entries(byTopic)) {
  let src = header(topic, titles[topic], cases.some((c) => !c.pattern.isWellFormed()));
  for (const c of cases) {
    const shown = c.units.map((u) => (u >= 0x20 && u < 0x7f && u !== 0x22 && u !== 0x5c ? String.fromCharCode(u) : `\\u${u.toString(16).padStart(4, '0')}`)).join('');
    src += `#[test]\nfn ${c.name}() {\n    // entrada (JS): '${shown}'\n`;
    let exp;
    if (c.res && c.res.error) exp = 'ERROR ' + c.res.error;
    else if (c.res === null) exp = 'None';
    else {
      const caps = c.res.caps.map(opt).join(', ');
      const groups = c.res.groups.map(([k, r]) => `(${rs(k)}, ${opt(r)})`).join(', ');
      exp = `Some(Expected { captures: &[${caps}], groups: &[${groups}] })`;
    }
    if (exp.startsWith('ERROR')) throw new Error(`caso ${c.topic}/${c.name} deu erro no node: ${exp}`);
    const units = `&[${c.units.join(', ')}]`;
    const lone = !c.pattern.isWellFormed();
    const pat = lone ? `&[${Array.from({ length: c.pattern.length }, (_, i) => c.pattern.charCodeAt(i)).join(', ')}]` : rs(c.pattern);
    src += `    ${lone ? 'check_units' : 'check'}(${pat}, ${rs(c.flags)}, ${units}, ${c.start}, ${exp});\n}\n\n`;
  }
  fs.writeFileSync(`${outDir}/${topic}.rs`, src.trimEnd() + '\n');
}

// erros
let src = `//! Erros de sintaxe: o padrão (ou as flags) que o Node rejeita, com a mensagem do JavaScriptCore.\n//!\n//! A rejeição confere com o Node real (V8 ${process.version}); a mensagem é a do Bun 1.4.2 (JavaScriptCore), que o\n//! zyarr reproduz (o V8 escreve outro texto). Arquivo gerado; não editar à mão.\n\nmod common;\n\nuse common::check_error;\n\n`;
let i = 0;
errsNode.forEach((e, idx) => {
  const b = errsBun[idx];
  if (!e.error) { return; }
  if (!b.error) throw new Error('bun aceita e node rejeita: ' + e.p);
  src += `#[test]\nfn error_${String(i++).padStart(2, '0')}() {\n    check_error(${rs(e.p)}, ${rs(e.f)}, ${rs(b.error)});\n}\n\n`;
});
fs.writeFileSync(`${outDir}/errors.rs`, src.trimEnd() + '\n');
console.log('ok', i, 'erros');
