//! O `error.stack` do V8 para o que o `event.ts` e o `lifecycle.ts` fazem com ele: guardar o ponto de
//! alocação e comparar pilhas por igualdade de texto.
//!
//! A pilha sai no formato do V8 (`Error` numa linha, depois `    at função (arquivo:linha:coluna)` por
//! quadro), montada a partir do `std::backtrace::Backtrace`, para que a análise do original (que corta
//! prefixos e agrupa por caminho) valha igual. Precisa de informação de depuração para ter linhas.

use std::backtrace::Backtrace;
use std::rc::Rc;

/// `Stacktrace` do `event.ts`: a pilha de um listener, já formatada, usada como chave.
#[derive(Clone, Debug)]
pub struct Stacktrace {
    pub value: String,
}

impl Stacktrace {
    /// `Stacktrace.create()`.
    #[inline(never)]
    pub fn create() -> Self {
        Self { value: format_backtrace(&Backtrace::force_capture()) }
    }

    /// `print()`: descarta `Error` e o quadro do próprio `create`, como o original.
    pub fn print(&self) {
        eprintln!("{}", self.value.split('\n').skip(2).collect::<Vec<_>>().join("\n"));
    }
}

/// Captura agora e formata só quando a pilha for pedida (o `new Error()` do rastreador, cujo `stack` o
/// original só lê ao relatar vazamento).
#[inline(never)]
pub fn capture_lazy() -> Rc<dyn Fn() -> String> {
    let backtrace = Backtrace::force_capture();
    Rc::new(move || format_backtrace(&backtrace))
}

/// Converte o texto do `Backtrace` no formato do V8, sem os quadros da própria captura.
fn format_backtrace(backtrace: &Backtrace) -> String {
    let text = backtrace.to_string();
    let mut frames: Vec<(String, Option<String>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(location) = trimmed.strip_prefix("at ") {
            if let Some(frame) = frames.last_mut() {
                frame.1 = Some(location.to_owned());
            }
        } else if let Some((index, symbol)) = trimmed.split_once(": ")
            && !index.is_empty()
            && index.bytes().all(|b| b.is_ascii_digit())
        {
            frames.push((symbol.to_owned(), None));
        }
    }
    let skip = frames.iter().take_while(|(symbol, _)| symbol.contains("Backtrace::") || symbol.contains("stacktrace::capture")).count();
    let mut out = String::from("Error");
    for (symbol, location) in frames.into_iter().skip(skip) {
        out.push_str("\n    at ");
        out.push_str(&symbol);
        if let Some(location) = location {
            out.push_str(" (");
            out.push_str(&location);
            out.push(')');
        }
    }
    out
}
