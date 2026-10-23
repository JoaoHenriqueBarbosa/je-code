//! Expressões regulares do `marked`: um invólucro fino sobre o `zyarr` que devolve os grupos como
//! fatias da entrada, sem copiar texto.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zyarr::{Match, RegExp};

use crate::JsString;
use crate::js_regexp::replace_all;

/// Uma regex compilada, com o fonte e as flags de JavaScript guardados para a conferência do golden.
pub(crate) struct Re {
    regexp: RegExp,
    pub(crate) source: String,
    pub(crate) flags: String,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

impl Re {
    /// Compila `source` com as flags de JavaScript.
    pub(crate) fn new(source: &str, flags: &str) -> Re {
        Re::compile(source, flags, "")
    }

    /// Como [`Re::new`], mas casando só na posição inicial (flag `y`). Vale para as regras do `marked` que
    /// começam por `^`: o resultado é o mesmo, e o motor não varre o resto do texto procurando um `^`.
    pub(crate) fn anchored(source: &str, flags: &str) -> Re {
        Re::compile(source, flags, "y")
    }

    fn compile(source: &str, flags: &str, extra: &str) -> Re {
        let all = format!("{flags}{extra}");
        let regexp = RegExp::new(&units(source), &units(&all))
            .unwrap_or_else(|error| panic!("regex fixa inválida /{source}/{flags}: {}", error.message()));
        Re {
            regexp,
            source: source.to_owned(),
            flags: flags.to_owned(),
        }
    }

    /// `re.exec(input)` sem `lastIndex` (procura a partir de `start`).
    pub(crate) fn exec_at<'a>(&self, input: &'a [u16], start: usize) -> Option<Cap<'a>> {
        self.regexp.exec(input, start).map(|m| Cap { input, m })
    }

    pub(crate) fn exec<'a>(&self, input: &'a [u16]) -> Option<Cap<'a>> {
        self.exec_at(input, 0)
    }

    pub(crate) fn test(&self, input: &[u16]) -> bool {
        self.regexp.exec(input, 0).is_some()
    }

    /// `input.replace(re, (m) => replacement(m))` com a regex global.
    pub(crate) fn replace_all(&self, input: &[u16], mut replacement: impl FnMut(&Cap<'_>) -> JsString) -> JsString {
        replace_all(&self.regexp, input, |c| {
            replacement(&Cap {
                input: c.input,
                m: c.matched.clone(),
            })
        })
    }

    /// `input.replace(re, '')` com a regex global.
    pub(crate) fn remove_all(&self, input: &[u16]) -> JsString {
        self.replace_all(input, |_| JsString::new())
    }
}

/// Um casamento e a entrada, para ler os grupos como `cap[n]`.
pub(crate) struct Cap<'a> {
    input: &'a [u16],
    m: Match,
}

impl<'a> Cap<'a> {
    /// `cap[n]`: `None` é o `undefined` de um grupo que não participou.
    pub(crate) fn g(&self, n: usize) -> Option<&'a [u16]> {
        let range = self.m.captures.get(n)?.as_ref()?;
        Some(&self.input[range.clone()])
    }

    /// `cap[0]`.
    pub(crate) fn whole(&self) -> &'a [u16] {
        self.g(0).unwrap_or_default()
    }

    /// `cap.index`.
    pub(crate) fn start(&self) -> usize {
        self.m.start()
    }

    /// O `lastIndex` de uma regex global depois do casamento.
    pub(crate) fn end(&self) -> usize {
        self.m.end()
    }
}

thread_local! {
    static DYNAMIC: RefCell<HashMap<(String, &'static str), Rc<Re>>> = RefCell::new(HashMap::new());
}

/// Regex montada em tempo de execução (as da lista, que dependem do marcador e do recuo), compilada
/// uma vez por fonte.
pub(crate) fn cached(source: String, flags: &'static str) -> Rc<Re> {
    DYNAMIC.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (source, flags);
        if let Some(found) = cache.get(&key) {
            return Rc::clone(found);
        }
        let compiled = Rc::new(Re::anchored(&key.0, flags));
        cache.insert(key, Rc::clone(&compiled));
        compiled
    })
}
