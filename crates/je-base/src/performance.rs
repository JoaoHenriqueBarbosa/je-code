//! Porte de `src/vs/base/common/performance.ts`: marcas de desempenho do VS Code (`mark`, `getMarks`,
//! `clearMarks`).
//!
//! O original guarda as marcas num singleton em `globalThis` (`MonacoPerformanceMarks`), e no ambiente
//! Node usa o polyfill baseado em `Date.now()` com `code/timeOrigin` como primeira marca. O singleton é um
//! `thread_local!`, o equivalente do `globalThis` de um realm (cada worker tem o seu). O ramo do navegador
//! (que reusa `performance.mark`) é do host web e entra por [`set_time_origin`] e [`mark`] com `start_time`.
//!
//! Divergência: o `performance.timeOrigin` do Node é o início do processo; aqui é o instante da primeira
//! marca do thread, porque o Rust não expõe o início do processo de forma portável.

use std::cell::RefCell;

use crate::runtime;

/// `PerformanceMark`.
#[derive(Clone, Debug, PartialEq)]
pub struct PerformanceMark {
    pub name: String,
    pub start_time: f64,
}

struct Marks {
    /// O `_data` do original: pares nome e instante.
    data: Vec<(String, f64)>,
}

const TIME_ORIGIN: &str = "code/timeOrigin";

thread_local! {
    static MARKS: RefCell<Option<Marks>> = const { RefCell::new(None) };
}

fn with_marks<R>(f: impl FnOnce(&mut Marks) -> R) -> R {
    MARKS.with(|m| {
        let mut slot = m.borrow_mut();
        let marks = slot.get_or_insert_with(|| Marks { data: vec![(TIME_ORIGIN.to_owned(), runtime::date_now() as f64)] });
        f(marks)
    })
}

/// Define o `code/timeOrigin` (o `timeOrigin` do host: início do processo ou da página). Só vale antes da
/// primeira marca do thread.
pub fn set_time_origin(time_origin: f64) {
    MARKS.with(|m| {
        let mut slot = m.borrow_mut();
        if slot.is_none() {
            *slot = Some(Marks { data: vec![(TIME_ORIGIN.to_owned(), time_origin)] });
        }
    });
}

/// `mark(name, { startTime })`: sem `start_time`, vale `Date.now()`.
pub fn mark(name: &str, start_time: Option<f64>) {
    with_marks(|marks| marks.data.push((name.to_owned(), start_time.unwrap_or_else(|| runtime::date_now() as f64))));
}

/// `getMarks()`.
pub fn get_marks() -> Vec<PerformanceMark> {
    with_marks(|marks| marks.data.iter().map(|(name, start_time)| PerformanceMark { name: name.clone(), start_time: *start_time }).collect())
}

/// `clearMarks(name?)`: sem nome, limpa tudo e preserva o `code/timeOrigin` se ele for a primeira marca; com
/// nome, remove todas as marcas desse nome.
pub fn clear_marks(name: Option<&str>) {
    with_marks(|marks| match name {
        None => {
            let origin = marks.data.first().filter(|(n, _)| n == TIME_ORIGIN).cloned();
            marks.data.clear();
            marks.data.extend(origin);
        }
        Some(name) => marks.data.retain(|(n, _)| n != name),
    });
}
