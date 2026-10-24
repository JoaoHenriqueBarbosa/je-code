//! Threads do mesmo pseudo-processo. Um interpretador recursivo (o `python3`, o `bun`) precisa de uma pilha bem
//! maior que a da thread do processo, e uma thread nova do SO não herda o pseudo-processo: sem ele `open`, stdin e
//! stderr não funcionam. [`run_with_stack`] cria a thread com a pilha pedida, instala nela o processo de quem chamou
//! e espera o trabalho.

use sysabi::sys;

/// Roda `job` numa thread de `stack_bytes` de pilha com o pseudo-processo de quem chamou instalado, e devolve o
/// resultado. Os desvios de controle do kernel (`execve`, `_exit`, morte por sinal) desempilham a thread e seguem até o
/// pseudo-processo, que é quem troca o programa ou termina; um panic de bug (mensagem `&str` ou `String`) vira `None`.
pub fn run_with_stack<T: Send + 'static>(stack_bytes: usize, job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let current = sys::try_current();
    let spawned = std::thread::Builder::new().stack_size(stack_bytes).spawn(move || {
        if let Some(c) = current {
            sys::install(c);
        }
        job()
    });
    match spawned.map(|h| h.join()) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(payload)) if !payload.is::<&'static str>() && !payload.is::<String>() => std::panic::resume_unwind(payload),
        _ => None,
    }
}

/// Como `run_with_stack`, mas tenta as pilhas de `stacks` da maior à menor: o SO que recusa a reserva (limite de
/// memória) cai para a seguinte, como o CPython sobe com pouca memória. Nunca devolve falha: um panic de bug, ou a
/// recusa de todas as pilhas, mata o processo por SIGSEGV (o desvio de controle que o kernel captura na entrada do
/// pseudo-processo), o crash que o agente veria no programa real, sem mensagem que o programa real não tem.
pub fn run_with_stack_ladder<T: Send + 'static>(stacks: &[usize], job: impl FnOnce() -> T + Send + 'static) -> T {
    use std::sync::{Arc, Mutex};
    // O `spawn` que falha descarta o fecho; o trabalho fica fora dele para a tentativa seguinte.
    let job = Arc::new(Mutex::new(Some(job)));
    for &size in stacks {
        let slot = job.clone();
        let taken = job.clone();
        let result = run_with_stack(size, move || {
            let job = slot.lock().unwrap_or_else(|e| e.into_inner()).take().expect("job taken once");
            job()
        });
        if let Some(value) = result {
            return value;
        }
        // Com o trabalho já retirado, a thread subiu e o job entrou em pânico: é bug, e bug é crash.
        if taken.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            break;
        }
    }
    std::panic::resume_unwind(Box::new(sysabi::KillUnwind(sysabi::Signal::SIGSEGV)))
}
