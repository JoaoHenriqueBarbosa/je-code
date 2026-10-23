//! O objeto `RegExp` do JavaScript sobre o motor `zyarr`: guarda o `lastIndex` e aplica as regras de
//! `RegExpBuiltinExec` para as flags `g` e `y`, que o buffer usa para varrer um texto grande em passos
//! (`searcher.reset(start)` e `regex.exec(text)`).
// TODO(integração): mover para je_base (junto do `createRegExp` do strings.ts, que ainda não existe).

use std::cell::Cell;

use zyarr::{Match, RegExp, RegExpError};

/// `RegExp` com `lastIndex`.
#[derive(Debug)]
pub struct JsRegExp {
    inner: RegExp,
    last_index: Cell<usize>,
}

impl JsRegExp {
    /// `new RegExp(pattern, flags)`.
    pub fn new(pattern: &[u16], flags: &str) -> Result<JsRegExp, RegExpError> {
        let flags: Vec<u16> = flags.encode_utf16().collect();
        Ok(JsRegExp {
            inner: RegExp::new(pattern, &flags)?,
            last_index: Cell::new(0),
        })
    }

    /// Atalho para texto UTF-8.
    pub fn from_str(pattern: &str, flags: &str) -> Result<JsRegExp, RegExpError> {
        let pattern: Vec<u16> = pattern.encode_utf16().collect();
        JsRegExp::new(&pattern, flags)
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
