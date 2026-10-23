//! Os usos de `RegExp` global do JavaScript (`String#replace` com função, `exec` em laço, `matchAll`)
//! sobre o [`zyarr::RegExp`], que só sabe procurar um casamento a partir de um índice.
//!
//! Um casamento vazio avança o `lastIndex` em uma unidade, como o `RegExpBuiltinExec` com a flag `g`
//! (com `u`/`v` o avanço é de um ponto de código, o que o `exec` do `zyarr` já respeita ao voltar do meio
//! de um par substituto).

use std::cell::Cell;
use std::ops::Range;

use zyarr::{Flags, Match, RegExp, RegExpError};

use crate::js_string::JsString;

/// Um casamento com o texto de origem, para ler os grupos como o `String#replace` os entrega à função.
pub struct Captures<'a> {
    pub input: &'a [u16],
    pub matched: &'a Match,
}

impl<'a> Captures<'a> {
    /// O grupo `n` (`0` é o casamento inteiro); `None` é o `undefined` de um grupo que não participou.
    pub fn group(&self, n: usize) -> Option<&'a [u16]> {
        let range: &Range<usize> = self.matched.captures.get(n)?.as_ref()?;
        Some(&self.input[range.clone()])
    }

    /// O casamento inteiro.
    pub fn whole(&self) -> &'a [u16] {
        self.group(0).unwrap_or_default()
    }

    /// O `offset` que o `replace` entrega.
    pub fn index(&self) -> usize {
        self.matched.start()
    }
}

/// Todos os casamentos, na ordem, como o laço `while (m = re.exec(input))` de uma regex global.
pub fn match_all(regexp: &RegExp, input: &[u16]) -> Vec<Match> {
    let mut matches = Vec::new();
    let mut last_index = 0;
    while let Some(found) = regexp.exec(input, last_index) {
        last_index = if found.end() == found.start() { found.end() + 1 } else { found.end() };
        matches.push(found);
    }
    matches
}

/// `input.replace(globalRegExp, (match, ...groups) => replacement)`.
pub fn replace_all(regexp: &RegExp, input: &[u16], mut replacement: impl FnMut(&Captures<'_>) -> JsString) -> JsString {
    let mut out = JsString::new();
    let mut copied = 0;
    for found in match_all(regexp, input) {
        out.push_units(&input[copied..found.start()]);
        out.push_units(replacement(&Captures { input, matched: &found }).units());
        copied = found.end();
    }
    out.push_units(&input[copied..]);
    out
}

// ---------------------------------------------------------------------------------------------
// `RegExp` com `lastIndex` (as regras de `RegExpBuiltinExec` para as flags `g` e `y`), para varrer um texto
// grande em passos (`searcher.reset(start)` e `regex.exec(text)`).


/// `RegExp` com `lastIndex`.
#[derive(Debug)]
pub struct JsRegExp {
    inner: RegExp,
    /// O padrão como foi dado ao construtor (o `source` do original sem o escape de `/` e de quebra de
    /// linha, que não muda o significado ao recriar o `RegExp`).
    source: Vec<u16>,
    last_index: Cell<usize>,
}

impl JsRegExp {
    /// `new RegExp(pattern, flags)`.
    pub fn new(pattern: &[u16], flags: &str) -> Result<JsRegExp, RegExpError> {
        let flags: Vec<u16> = flags.encode_utf16().collect();
        Ok(JsRegExp {
            inner: RegExp::new(pattern, &flags)?,
            source: pattern.to_vec(),
            last_index: Cell::new(0),
        })
    }

    /// Atalho para texto UTF-8.
    pub fn from_str(pattern: &str, flags: &str) -> Result<JsRegExp, RegExpError> {
        let pattern: Vec<u16> = pattern.encode_utf16().collect();
        JsRegExp::new(&pattern, flags)
    }

    pub fn source(&self) -> &[u16] {
        &self.source
    }

    /// As flags (`global`, `ignoreCase`, `multiline`, `unicode` e as demais).
    pub fn flags(&self) -> Flags {
        self.inner.flags()
    }

    pub fn last_index(&self) -> usize {
        self.last_index.get()
    }

    pub fn set_last_index(&self, last_index: usize) {
        self.last_index.set(last_index);
    }

    /// `regex.exec(text)`: com `g` ou `y` parte de `lastIndex` e o atualiza (zera se não casa); sem as
    /// duas flags parte do zero e não mexe em `lastIndex`.
    pub fn exec(&self, text: &[u16]) -> Option<Match> {
        let flags = self.inner.flags();
        let stateful = flags.global || flags.sticky;
        let start = if stateful { self.last_index.get() } else { 0 };
        if start > text.len() {
            if stateful {
                self.last_index.set(0);
            }
            return None;
        }
        match self.inner.exec(text, start) {
            Some(found) => {
                if stateful {
                    self.last_index.set(found.end());
                }
                Some(found)
            }
            None => {
                if stateful {
                    self.last_index.set(0);
                }
                None
            }
        }
    }
}
