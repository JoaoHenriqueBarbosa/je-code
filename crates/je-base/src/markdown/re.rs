//! Expressões regulares do ECMAScript (motor `zyarr`) compiladas uma vez por thread, para os trechos
//! do porte de `markdownRenderer.ts` e `domSanitize.ts` que o original escreve como literais de regex.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zyarr::{Match, RegExp};

use crate::JsString;

thread_local! {
    static CACHE: RefCell<HashMap<(&'static str, &'static str), Rc<RegExp>>> = RefCell::new(HashMap::new());
}

fn compiled(pattern: &'static str, flags: &'static str) -> Rc<RegExp> {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry((pattern, flags))
            .or_insert_with(|| Rc::new(RegExp::from_str(pattern, flags).expect("regex de markdown válida")))
            .clone()
    })
}

/// `regex.exec(text)` sem a flag `g`: o primeiro casamento.
pub(super) fn exec(pattern: &'static str, flags: &'static str, text: &[u16]) -> Option<Match> {
    compiled(pattern, flags).exec(text, 0)
}

/// `regex.test(text)` sem a flag `g`.
pub(super) fn test(pattern: &'static str, flags: &'static str, text: &[u16]) -> bool {
    exec(pattern, flags, text).is_some()
}

/// Todos os casamentos de uma regex global, na ordem.
pub(super) fn match_all(pattern: &'static str, flags: &'static str, text: &[u16]) -> Vec<Match> {
    crate::js_regexp::match_all(&compiled(pattern, flags), text)
}

/// `text.replace(regex, '')` sem a flag `g`: tira só o primeiro casamento.
pub(super) fn remove_first(pattern: &'static str, flags: &'static str, text: &[u16]) -> JsString {
    match exec(pattern, flags, text) {
        Some(found) => {
            let mut out = JsString::from(&text[..found.start()]);
            out.push_units(&text[found.end()..]);
            out
        }
        None => JsString::from(text),
    }
}

/// O texto de um grupo de captura (`None` quando o grupo não participou).
pub(super) fn group<'a>(found: &Match, text: &'a [u16], n: usize) -> Option<&'a [u16]> {
    found.captures.get(n).cloned().flatten().map(|range| &text[range])
}

pub(super) fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Acrescenta texto Rust a um `JsString`.
pub(super) fn push_str(out: &mut JsString, text: &str) {
    out.push_units(&units(text));
}
