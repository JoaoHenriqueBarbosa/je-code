# zyarr

O YARR, o motor de expressões regulares do JavaScriptCore, em Rust sem `unsafe`, extraído do `zjsc`
do pseudo-linus para ser uma crate independente do je-code. A semântica é a do `RegExp` do
ECMAScript (a mesma que o VS Code usa via V8), sobre texto UTF-16.

Sem dependências, sem `unsafe` (`unsafe_code = "forbid"` do workspace), sem C.

## Origem

- Projeto: pseudo-linus, clonado em `ref/pseudo-linus`, commit `9e3d0767` (2026-10-12).
- Caminho: `crates/zjsc/src/yarr` (o YARR) e as partes de `crates/zjsc/src/wtf` de que ele depende.
  O `zjsc` é o JavaScriptCore traduzido para Rust sem `unsafe` e passa 98,6% do test262.
- Por trás: WebKit/JavaScriptCore do Bun 1.4.2 (`oven-sh/bun`, tag `bun-v1.4.2`, que carrega o
  `oven-sh/webkit`). O código do Bun é MIT; o do WebKit, descrito abaixo.
- `src/wtf/rapidhash.rs` vem de `crates/ul-common/src/hash/rapidhash.rs` do mesmo repositório (o hash de
  string do WTF).

Os nomes de módulos, tipos e funções do upstream foram mantidos para facilitar ressincronizar. O que é
do je-code, e não do upstream, está em `src/regexp.rs` (a API segura), nesta README e em `tests/`.

## Licença

Os arquivos do YARR e do WTF herdam as licenças dos fontes do WebKit de que foram traduzidos
(`upstream/JavaScriptCore/yarr/*` e `upstream/WTF/wtf/*` no `zjsc`). Os `.rs` copiados não repetem o
cabeçalho; as condições são as dos cabeçalhos do C++:

- **BSD de duas cláusulas** (Apple Inc.): todo o `yarr/` (`YarrPattern`, `YarrParser`,
  `YarrInterpreter`, `YarrFlags`, `YarrErrorCode`, `YarrCanonicalize`, `YarrUnicodeProperties`,
  `YarrSyntaxChecker`, `Yarr.h`), e do WTF o `ASCIICType`, `OptionSet`, `TriState`, `DataLog`,
  `StringView`, `StringBuilder` e `StringCommon`.
- **GNU Library General Public License, versão 2 ou posterior** (LGPL-2.0-or-later): do WTF o
  `StringImpl`, `AtomStringImpl`, `WTFString`, `AtomString` e `StringHasher`.

O `Cargo.toml` declara `LGPL-2.0-or-later AND BSD-2-Clause`, como o `zjsc`.

Copyrights que os cabeçalhos do upstream trazem para esses arquivos: Apple Inc. (vários anos entre 2004 e
2025), Peter Varga (University of Szeged), Yusuke Suzuki, Sony Interactive Entertainment Inc., Alexey
Shvayka, Tetsuharu Ohzeki, Lars Knoll, Google Inc., Patrick Gansterer e Jarred Sumner.

Condições BSD de duas cláusulas, copiadas do cabeçalho de `YarrPattern.cpp` sem alteração:

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY APPLE INC. ``AS IS'' AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL APPLE INC. OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY
OF LIABILITY, WHETHER IN  IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

Para os arquivos LGPL, o texto da licença é o da GNU Library General Public License 2.0, que os
cabeçalhos de `StringImpl.h` e `WTFString.h` citam.

## Mudanças locais

Tudo o mais é cópia sem alteração do `zjsc`.

YARR (`src/yarr/`):

- Removido `yarr_matching_context_holder.rs` (depende da `VM`; serve ao JIT e nada do interpretador o
  chama). `MatchFrom` continua em `yarr.rs`.
- `yarr.rs`: duas constantes novas no fim, no lugar das `Options` da `VM`:
  `USE_REG_EXP_ALTERNATION_FACTORING = true` (era `Options::use_reg_exp_alternation_factoring()`) e
  `DUMP_COMPILED_REG_EXP_PATTERNS = false` (era `Options::dump_compiled_reg_exp_patterns`). Os dois valores
  são os padrões do `zjsc` (`runtime/options_list.rs`).
- `yarr_pattern_cpp5.rs` e `yarr_pattern_cpp6.rs`: leem essas constantes em vez das `Options`.
- `yarr_pattern_cpp4.rs` e `yarr_pattern_cpp5.rs`: `TriState` vem de `crate::wtf::tri_state` (no `zjsc` era
  reexportado por `crate::parser::source_tainted_origin`).

WTF (`src/wtf/`), só o que o YARR alcança:

- Mantidos: `ascii_ctype`, `option_set`, `tri_state`, `text/{wtf_string, string_impl, string_view,
  string_common, string_hasher, conversion_mode, atom_string, atom_string_impl, string_builder}` e
  `unicode/{utf8_conversion, case_mapping, case_mapping_tables, character_names, properties_tables}`.
- `data_log.rs`: reescrito em 10 linhas. O do `zjsc` escreve num arquivo trocável por thread sobre o
  `host()` do runtime; aqui escreve em `stderr`, que é o padrão dele. Só o `dumpPattern` do YARR chama.
- `rapidhash.rs`: cópia de `ul_common::hash::rapidhash` (o `string_hasher` o importava de `ul_common`).
- `text/string_impl.rs`: o trait `CharType` deixou de herdar de `ul_common::js_number::CharType` e passou a
  trazer `SIZE`, `to_u16` e `from_u16` direto (as mesmas implementações de `u8` e `u16`). Removidos
  `deprecated_is_space_or_newline`, `deprecated_is_not_space_or_newline`, `default_writing_direction`, o
  teste deles e as constantes `UCharDirection` (dependiam da tabela bidirecional do ICU, que o YARR não usa).
- `text/wtf_string.rs`, `text/string_view.rs`, `text/string_builder.rs`, `text/atom_string.rs`: removidas
  as conversões entre texto e `double`/`float` (`to_double`, `to_float`, `characters_to_double` e
  parentes, `number_f64`, `append_number_f64/f32`, `AtomString::number_f64`) e seus testes: dependiam de
  `dtoa`, `dragonbox` e `fast_float`, que o YARR não toca.
- `unicode/mod.rs`: só `char_category` (o resto do módulo era `char_direction`, `is_id_start`,
  `is_id_continue`). `properties_tables.rs` perdeu as tabelas `ID_START` e `ID_CONTINUE`, que só esses dois
  usavam; ficou `GENERAL_CATEGORY_RUNS`.
- `text/` perdeu `uniqued_string_impl`, `symbol_impl`, `string_concatenate`, `text_position` e `base64`.
- Os módulos `wtf` e `yarr` levam `#[allow(dead_code, unused, non_*)]` em `lib.rs`: a árvore copiada tem
  itens que só o resto do JSC usa e nomes do C++, como no `zjsc` (que liga `unused = "allow"` e os três
  `non_*`). Os avisos de clippy que sobram vêm do código copiado.

## API

`src/regexp.rs`, reexportada na raiz:

- `RegExp::new(pattern: &[u16], flags: &[u16])` e `RegExp::from_str(&str, &str)`: compilam padrão e flags
  (`d g i m s u v y`) e devolvem `RegExpError` com a mensagem do JSC (`Invalid regular expression: ...` ou
  `Invalid flags supplied to RegExp constructor.`) e o `ErrorKind` (`SyntaxError` ou `OutOfMemory`).
- `RegExp::exec(input: &[u16], start: usize) -> Option<Match>`: o primeiro casamento a partir de `start`
  (com a flag `y`, só exatamente em `start`), no estilo do `matchInline` do JSC, incluindo o ajuste de
  fronteira de ponto de código em `u`/`v`.
- `Match { captures: Vec<Option<Range<usize>>>, groups: Vec<(String, Option<Range<usize>>)> }` com
  `start()`, `end()` e `group(nome)`. Índices em unidades UTF-16; `None` para grupo que não participou;
  `groups` na ordem da primeira ocorrência de cada nome, com a resolução de nomes duplicados.
- `RegExp::flags()` (`Flags`, com `parse` e `to_flags_string`), `capture_count()`, `capture_names()`,
  `has_named_captures()`.

O `lastIndex` com `g`/`y` (`RegExpBuiltinExec`) é do chamador: ele lê `flags()`, passa o `lastIndex` como
`start` e grava `Match::end()` de volta. `tests/api.rs` tem o laço do `matchAll` escrito assim.

## Testes

`cargo test -p zyarr`: os testes unitários que vieram do `zjsc` (147) e os de `tests/`. Os valores
esperados de `tests/*.rs` (exceto `api.rs`) saem do Node real, conferidos contra o Bun 1.4.2 (JSC); as
mensagens de erro, do Bun. O Node e o Bun deram o mesmo resultado em todos os 229 casos de casamento.

## Limitações herdadas do upstream

- É o interpretador do YARR, sem JIT. O limite de passos (`MATCH_LIMIT`, 100 milhões) devolve "sem
  casamento", como o `interpret` do JSC; o JSC lança no caso do JIT.
- O interpretador não tem verificação de estouro de pilha na execução (FIXME do próprio WebKit,
  bug 195970); o compilador do padrão tem (`StackCheck`, 512 KiB) e recusa com
  `Invalid regular expression: too many nested disjunctions`.
- `RegExp` é `Send` e `Sync` (sem `Rc` nem estado mutável na API): `exec` pode ser chamado de várias threads.
