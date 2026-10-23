//! Os usos de `RegExp` global do JavaScript (`String#replace` com função, `exec` em laço, `matchAll`)
//! sobre o [`zyarr::RegExp`], que só sabe procurar um casamento a partir de um índice.
//!
//! Um casamento vazio avança o `lastIndex` em uma unidade, como o `RegExpBuiltinExec` com a flag `g`
//! (com `u`/`v` o avanço é de um ponto de código, o que o `exec` do `zyarr` já respeita ao voltar do meio
//! de um par substituto).

use std::ops::Range;

use zyarr::{Match, RegExp};

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
