// Gera os goldens do lexer do `marked` rodando o marked.js real do VS Code (v14.0.0, `ref/vscode`) no Node.
// Escreve crates/je-base/tests/golden/marked_lexer.jsonl (a árvore de tokens de cada amostra) e
// crates/je-base/tests/golden/marked_rules.jsonl (o fonte e as flags de cada regex por modo).
// Uso: node scripts/gen_marked_golden.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'je-marked-'));
fs.copyFileSync(path.join(root, 'ref/vscode/src/vs/base/common/marked/marked.js'), path.join(work, 'marked.mjs'));
const { Lexer, lexer, getDefaults } = await import(path.join(work, 'marked.mjs'));

const outDir = path.join(root, 'crates/je-base/tests/golden');
fs.mkdirSync(outDir, { recursive: true });

// ---- Projeção canônica de um token: a ordem das chaves é a que o teste em Rust monta.
function cell(c) {
	return { text: c.text, tokens: c.tokens.map(proj), header: c.header, align: c.align ?? null };
}
function proj(t) {
	const o = { type: t.type, raw: t.raw };
	switch (t.type) {
		case 'space': case 'hr': case 'br': break;
		case 'code':
			if (t.codeBlockStyle) { o.codeBlockStyle = t.codeBlockStyle; }
			if (t.lang !== undefined) { o.lang = t.lang; }
			o.text = t.text;
			break;
		case 'heading': o.depth = t.depth; o.text = t.text; o.tokens = t.tokens.map(proj); break;
		case 'table':
			o.align = t.align.map(a => a ?? null);
			o.header = t.header.map(cell);
			o.rows = t.rows.map(r => r.map(cell));
			break;
		case 'blockquote': o.text = t.text; o.tokens = t.tokens.map(proj); break;
		case 'list': o.ordered = t.ordered; o.start = t.start; o.loose = t.loose; o.items = t.items.map(proj); break;
		case 'list_item':
			o.task = t.task;
			if (t.checked !== undefined) { o.checked = t.checked; }
			o.loose = t.loose; o.text = t.text; o.tokens = t.tokens.map(proj);
			break;
		case 'paragraph': o.text = t.text; o.tokens = t.tokens.map(proj); break;
		case 'html':
			o.block = t.block;
			if (t.pre !== undefined) { o.pre = t.pre; }
			if (t.inLink !== undefined) { o.inLink = t.inLink; }
			if (t.inRawBlock !== undefined) { o.inRawBlock = t.inRawBlock; }
			o.text = t.text;
			break;
		case 'text':
			o.text = t.text;
			if (t.tokens !== undefined) { o.tokens = t.tokens.map(proj); }
			break;
		case 'escape': case 'codespan': o.text = t.text; break;
		case 'image': o.href = t.href; o.title = t.title ?? null; o.text = t.text; break;
		case 'link': o.href = t.href; o.title = t.title ?? null; o.text = t.text; o.tokens = t.tokens.map(proj); break;
		case 'strong': case 'em': case 'del': o.text = t.text; o.tokens = t.tokens.map(proj); break;
		default: throw new Error('tipo não previsto: ' + t.type);
	}
	return o;
}

const MODES = {
	default: {},
	breaks: { breaks: true },
	nogfm: { gfm: false },
	pedantic: { pedantic: true },
};

function lex(src, mode) {
	const options = { ...getDefaults(), ...MODES[mode] };
	try {
		const list = Lexer.lex(src, options);
		const links = Object.keys(list.links).sort().map(k => [k, list.links[k].href, list.links[k].title ?? null]);
		return { tokens: list.map(proj), links };
	} catch (e) {
		return { error: e.constructor.name + ': ' + e.message };
	}
}

// ---- Amostras escritas à mão.
const SAMPLES = [
	'', '\n', '\n\n\n', '   ', 'texto simples', 'duas\nlinhas', 'um\n\ndois', 'um\r\ndois\r\n\r\ntrês\rquatro',
	'# Título\n\ntexto', '## Dois ##\n', '### Três #\ntexto', '#### Quatro # #\n', '##### \n', '###### Seis\n####### sete', '#sem espaço', '# `código` e *ênfase*',
	'Título\n=====\n', 'Título\n-----\n\ntexto', 'a\nb\n---\n', '> a\n> b\n\n> c', '> # h\n> - i\n> - j', '> a\n>\n> b\n> > c\n> > d', '> a\nlazy\n> b', '> ```\n> code\n> ```',
	'> - a\n> - b\n\nsegue', '> a\n- b', '>>> a\nb', '> 1. um\n> 2. dois\ncontinua',
	'- a\n- b\n- c', '* a\n* b', '+ a\n+ b', '1. a\n2. b\n3. c', '1) a\n2) b', '3. a\n4. b', '007. a\n8. b',
	'- a\n\n- b\n- c', '- a\n  - b\n    - c\n  - d\n- e', '1. a\n   - b\n   - c\n2. d', '- a\n\n  b\n- c', '- [ ] tarefa\n- [x] feita\n- [X] outra\n- [x]sem espaço',
	'- a\n- b\n\n\n- c', '-\n- b', '- \n  a', '-   a\n-   b', '-     código\n', '- a\n```\ncode\n```\n- b', '- a\n# h\n- b', '- a\n---\n- b', '- a\n* b\n+ c',
	'* * *\n', '- - -\n', '___\n', '***\n---\n___', '- a\n- - -\n- b', '\t- a\n\t- b', '- a\n\tb', '1.\ta\n2.\tb', '   - a\n   - b', '    - a\n',
	'    código\n    mais\n\ntexto', '\tcódigo com tab\n', 'parágrafo\n    continuação', '    a\n\n    b\n', '```\nsem lang\n```', '```js\nlet a = 1;\n```', '~~~python\nprint(1)\n~~~',
	'```js extra\ncode\n```', '````\n```\ndentro\n```\n````', '```\nnão fecha', '  ```\n  indent\n    mais\n  ```', '```a\\*b\ncode\n```', '``` `x`\ncode\n```', '~~~\n~~~\n~~~',
	'<div>\nbloco html\n</div>\n\ntexto', '<pre>\n  a\n\n  b\n</pre>', '<script>\nalert(1)\n</script>', '<style>a{}</style>\n', '<!-- comentário -->\ntexto', '<!-- a\nb -->', '<?php echo 1; ?>\n', '<!DOCTYPE html>\n', '<![CDATA[ x ]]>\n',
	'<span>inline</span> texto', '<custom-tag>\ntexto\n</custom-tag>', '<div>a</div>', '<a href="x">link</a> e <b>negrito</b>', '<br>', '<img src="a.png">', 'texto <!-- c --> mais', '<details>\n<summary>s</summary>\n\nc\n</details>',
	'[ref]: http://exemplo.com\n\n[ref]', '[ref]: <http://a.b> "Título"\n\n[ref] [outro][ref] [ref][]', '[a]: /u\n[B]: /v \'t\'\n[c]: /w (p)\n\n[A] [b] [C]', '[x]: http://a\n  "título\n  em duas linhas"\n\n[x]', '[x]:\n  http://a\n\n[x]',
	'texto\n[x]: http://a', '[x]: http://a\n[x]: http://b\n\n[x]', '[a b]:  /u\n\n[a   B]', '[ref]: <>\n[ref]', '[foo\\]]: /u\n\n[foo\\]]', '[x]: /u "t\\"q"\n\n[x]', '[ref]: /a\n\n![ref] ![ref][] ![x][ref]', '> [q]: /u\n\n[q]',
	'[link](http://a.b)', '[link](http://a.b "título")', "[link](http://a.b 'título')", '[link](http://a.b (título))', '[link](<http://a b>)', '[link](<a\\>b>)', '[link](<a>b)', '[link]()', '[link]( )', '[ex](a(b)c)', '[ex](a(b c)', '[ex](a\\(b)',
	'[a [b] c](u)', '[a `]` c](u)', '[a *b* c](u) *x*', '[a\nb](u)', '[a](u\n"t")', '![img](a.png)', '![img](a.png "t")', '![a [b]](u)', '[![i](a.png)](http://l)', '[a][b]', '[a][]', '[a]', '[a]()[b]', '\\[a](b)', '[a](b)(c)', '[a](b "c" d)',
	'[a](javascript:alert(1))', '[a](http://x.y/a_b_c)', '[a](http://x.y/*b*)', '[a](  u  )', '[a](u "t" )', '[é](ü)', '[😀](😀)', '[a](u\\)b)',
	'*em*', '_em_', '**strong**', '__strong__', '***both***', '___both___', '*a **b** c*', '**a *b* c**', '*a*b*', '_a_b_', 'snake_case_word', '2*3*4', 'a * b * c', '* a*', '**a**b', '__a__b', '*a**b***', '**a*b***c', '*a* *b*', '*a\nb*', '**a\nb**', '*(a)*', '(*a*)', '"*a*"', '*"a"*',
	'*a `*` b*', '*[a*](u)', '*a [b*](u)', '_a *b_ c*', '**a __b** c__', '*é*', '*😀*', '_😀_', '😀*a*😀', 'é_a_é', '_a_é', 'x_é_', '*a*\\*', '\\*a*', '*a\\*', '**', '*', '_', '__', '____', '* *', '** **', '*\ta*', '***', '*a***', '**a***', '*a*** b',
	'~~del~~', '~del~', '~~a **b** c~~', '~~a', '~~ a ~~', '~~a~~~', '~~~a~~~', 'a~~b~~c', '~~a\nb~~',
	'`code`', '``co`de``', '` a `', '`  `', '`a\nb`', '`` ` ``', '`a', '```a```', '`<b>`', '`&amp; <x>`', '`a` `b`',
	'a  \nb', 'a\\\nb', 'a \nb', 'a  \n', 'a\\\n', '  \nb',
	'a\\*b', '\\# não', '\\\\', '\\a', '\\~~', '\\`', '\\<b>', '&amp; &lt; &#35; &#x1F; &copy &foo; &#12345678;', '&', 'a & b', '<', '>', 'a < b > c', '"quotes" \'single\'', '&quot;',
	'<http://a.b/c>', '<mailto:a@b.co>', '<a@b.co>', '<foo.bar@x.y>', '<not a link>', '<http://a b>', '<x:y>', 'http://exemplo.com', 'https://a.b/c?d=e&f=g#h', 'ftp://a.b/c', 'www.exemplo.com', 'WWW.EXEMPLO.COM', 'veja http://a.b/c. e mais', 'http://a.b/c).', '(http://a.b/c)', 'http://a.b/(c)', 'http://a.b/c_', 'http://a.b/c&amp;', 'http://a.b/c&d;', 'http://a.b/c*', 'http://a.b/c~~', 'http://a.b/c"', 'http://a.b/c,', 'http://a.b/c?', 'http://a.b/c!', 'a@b.co', 'x a@b.co y', 'a.b_c-d@e-f.co.uk', 'a@b', '@a', 'a@b_.co', 'a@b.co_', '[http://a.b](http://c.d)', '<a href="http://a.b">http://a.b</a>', 'http://a.b <b>http://c.d</b>',
	'| a | b |\n| - | - |\n| 1 | 2 |', '| a | b |\n|:-|-:|\n| 1 | 2 |\n| 3 | 4 |', 'a | b\n- | -\n1 | 2', '|a|b|\n|:-:|---|\n|1|\n|2|3|4|', '| a |\n|---|\n', '| a | b |\n| - |\n| 1 | 2 |', '| a \\| b | c |\n|---|---|\n| `x\\|y` | z |', '| a | b |\n|---|---|\n| *e* | [l](u) |\n\ntexto', '| a |\n| - |\n| 1 |\n> q', '| a |\n| - |\n| 1 |\n- l', '| a |\n| - |\n| 1 |\n# h', 'a\n| a |\n| - |', 'Título\n---|---\n', '| a | b |\n|---|---|\n|  | x |\n| y |  |', '|||\n|-|-|-|', ':-- | --: \n--- | ---\n', '| a | b |\n| :---: | ---: |\n| é | 😀 |', '| a | b |\n|---|---|\n| 1 | 2 |\n\n| c |\n|---|\n| 3 |',
	'texto\n\n---\n\ntexto', 'texto\n---', 'texto\n***', 'a\n\n***\n\nb', 'texto\n- lista\n- lista', 'texto\n1. lista', 'texto\n2. não lista', 'texto\n> citação', 'texto\n# título', 'texto\n```\ncode\n```', 'texto\n<div>\nhtml\n</div>', 'texto\n    código', 'texto\n\n    código', 'texto\n| a |\n| - |\n| 1 |',
	'a b', 'a b', ' texto', 'a b', '﻿a', '\ud83d', 'a\ud83db', '\ude00', '😀 😀', 'a😀b', '*\ud83d*', '_\ude00_', '\ud83d_a_', 'é', 'é', 'ÇÃO', 'ǅ', 'İ', 'ß', '日本語 *強調* **太字**', 'العربية *a*', '​*a*', '_a_​',
	'- 😀 item\n- 𝒳 item', '# 😀\n', '[😀]: /u\n[😀]', '[ÁÉ]: /u\n[áé]', '[İ]: /u\n[i̇]', '> 😀\n> é',
	'# Documento\n\nUm parágrafo com *ênfase*, **negrito**, `código` e [um link](http://a.b "t").\n\n- item um\n- item dois com ![img](i.png)\n  - subitem\n\n1. um\n2. dois\n\n> citação com **negrito**\n\n```js\nconst a = 1;\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n[ref]: http://ref.com\n\nVeja [ref] e http://auto.link.\n',
	'Use `[a](./x.txt)` then a real [a](./x.txt) link.', '```\n[a](./x.txt)\n```\n\n[a](./x.txt)', 'See [a](./x.txt) and ![b](./y.png).',
	'- a\n\n  ```\n  code\n  ```\n\n- b', '- a\n\n      código\n- b', '1. a\n\n   b\n\n2. c', '- a\n  b\n  c\n- d', '- a\n b\n- c', '-  a\n   b', '- a\n    b\n- c', '- > q\n  > r\n- s', '- # h\n  t', '- a\n  ---\n- b', '10. dez\n11. onze', '1234567890. longo', '- a\n\n\n  b', '- a\n  \n  b', '*\ta\n*\tb',
	'> a\n> ---\n> b', '> a\n===\n', '> - a\n> b\n> c', '> a\n> > b\n> c', '> > a\n> b', '> a\n\n\n> b', '> - a\n>   - b\n> - c', '>a\n>b', '>    code\n', '> a\n    code', '> a\n> b\n>\n>\n> c',
	'<div>\n\n*md*\n\n</div>', '<div *a*>x</div>', '</div>\n', '<p>a</p>\n<p>b</p>', '<table>\n<tr><td>x</td></tr>\n</table>', '<a href="u" title=\'t\' data=x>l</a>', '<a\nhref="u">l</a>', '<br/>', '<br />', '< br>', '<a href=">">',
	'a<b>c</b>d', '<code>a *b* c</code>', '<kbd>*a*</kbd> *b*', '<pre>*a*</pre> *b*', '<script>*a*</script>', '<a href="x">*a*</a> http://b.c', '<a href="x">http://b.c</a> http://d.e', '</a> http://b.c',
	'a_b_c_d', '_a_ _b_', 'a _b_ c', '(_a_)', '_(a)_', 'a_(b)_c', '__a_ b_', '*a **b***', '***a** b*', '**a *b***', '*a __b__ c*',
	'[a]: /u\n[b]: /v\n\n[a][b] [b][a] [a] [x]', '[a]: /u\n\n[`a`] `[a]`', '[a]: /u\n\n*[a]*', '[a]: /u\n\n[a]: /dup', '[a]: /u\n\n[a](x)', '[a]: /u "t"\n\n[a][]', '[a]: /u\n\n[*a*][a]',
	'foo\n[a]: /u\nbar', '- [a]: /u\n- [a]', '> [a]: /u\n> [a]',
	'a\n\n\n\n\nb', '\n\n a', ' a\n  b\n   c\n    d', '- a\r\n- b\r\n', 'a\tb', '\ta', ' \t a', '\t\t- a', 'a  \n  b', '* a\n\n\n\n* b', '1. a\n\n\n\n2. b',
	'[a](b "c\\"d")', '[a](b \'c\\\'d\')', '[a](b (c\\)d))', '[a]( <b> )', '[a](<b> "c")', '[a](<>)', '[a](<b\\\\>)', '![a](<b c>)', '[a](b\nc)', '[a](b "c\nd")',
	'*a [b *c* d](e) f*', '**[a](b)**', '[**a**](b)', '[a](b)**c**', '~~[a](b)~~', '`[a](b)`', '*`a`*', '`*a*`', '[`a`](b)', '![*a*](b)', '[a *b](c)*',
];

// ---- Amostras sorteadas (semente fixa): fragmentos de sintaxe do markdown colados ao acaso.
let seed = 20261023;
function rnd() { seed |= 0; seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }
const pick = (a) => a[Math.floor(rnd() * a.length)];
const FRAGMENTS = [
	'# ', '## ', '- ', '* ', '+ ', '1. ', '2) ', '> ', '>> ', '    ', '\t', '```', '~~~', '`', '``', '**', '*', '_', '__', '~~', '~', '[', ']', '(', ')', '![', '](', ']: ', '<', '>', '<div>', '</div>',
	'<!--', '-->', '&amp;', '&', 'http://a.b/c', 'www.x.com', 'a@b.co', '\n', '\n', '\n\n', '\\', '\\*', '|', '---', '===', ':--', '| a | b |', 'texto', 'palavra', ' ', ' ', '  \n', 'é', '😀', '\ud83d', '\ude00',
	'[x]', '[ ] ', '[x] ', '<a href="x">', '</a>', '<pre>', '</pre>', "'", '"', '<http://x.y>', '<a@b.c>', '#', 'foo', 'bar baz', '[a]: /u', '[a]', '[a][]', '[a](b)', '`c`', '1.', '-', '+', '***', '___', '|---|---|', '<span>', '</span>',
	'<br>', ' ', ' ', 'Título\n=====', 'x\n---',
];
const FUZZ_COUNT = 450;
for (let i = 0; i < FUZZ_COUNT; i++) {
	const n = 3 + Math.floor(rnd() * 22);
	let s = '';
	for (let j = 0; j < n; j++) { s += pick(FRAGMENTS); }
	SAMPLES.push(s);
}

// ---- Escreve o golden dos tokens: o modo padrão em todas as amostras; os outros três nas amostras à mão
// (e em um oitavo das sorteadas).
const lines = [];
const seen = new Set();
let errors = 0;
let tokenCount = 0;
function countTokens(list) { for (const t of list) { tokenCount++; for (const k of ['tokens', 'items']) { if (t[k]) countTokens(t[k]); } if (t.header) { for (const c of t.header) { countTokens(c.tokens); } for (const r of t.rows) { for (const c of r) { countTokens(c.tokens); } } } } }
SAMPLES.forEach((src, index) => {
	const modes = ['default'];
	if (index < SAMPLES.length - FUZZ_COUNT || index % 8 === 0) { modes.push('breaks', 'nogfm', 'pedantic'); }
	for (const mode of modes) {
		const key = mode + '\u0000' + src;
		if (seen.has(key)) { continue; }
		seen.add(key);
		const result = lex(src, mode);
		if (result.error) { errors++; } else { countTokens(Lexer.lex(src, { ...getDefaults(), ...MODES[mode] })); }
		lines.push(JSON.stringify({ mode, src, ...result }));
	}
});
fs.writeFileSync(path.join(outDir, 'marked_lexer.jsonl'), lines.join('\n') + '\n');
console.log(`marked_lexer.jsonl: ${lines.length} casos, ${errors} com erro, ${tokenCount} tokens (marked.js v14.0.0, Node ${process.version})`);

// ---- Fontes das regexes por modo.
const ruleLines = [];
const norm = (s) => s.replace(/\\\//g, '/').replace(/\n/g, '\\n');
for (const [mode, opts] of Object.entries(MODES)) {
	const o = { ...getDefaults(), ...opts };
	const blockKey = o.pedantic ? 'pedantic' : o.gfm ? 'gfm' : 'normal';
	const inlineKey = o.pedantic ? 'pedantic' : o.gfm ? (o.breaks ? 'breaks' : 'gfm') : 'normal';
	const rules = [];
	for (const [name, re] of Object.entries(Lexer.rules.block[blockKey])) {
		rules.push(['block.' + name, re.source === undefined ? '' : norm(re.source), re.source === undefined ? 'noop' : re.flags]);
	}
	for (const [name, re] of Object.entries(Lexer.rules.inline[inlineKey])) {
		rules.push(['inline.' + name, re.source === undefined ? '' : norm(re.source), re.source === undefined ? 'noop' : re.flags]);
	}
	ruleLines.push(JSON.stringify({ mode, rules }));
}
fs.writeFileSync(path.join(outDir, 'marked_rules.jsonl'), ruleLines.join('\n') + '\n');
console.log(`marked_rules.jsonl: ${ruleLines.length} modos`);
