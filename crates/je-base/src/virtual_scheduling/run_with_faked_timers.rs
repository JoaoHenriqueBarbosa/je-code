//! Porte de `src/vs/base/test/common/virtualScheduling/runWithFakedTimers.ts`: roda uma função com um
//! relógio virtual instalado como a API de tempo global.
//!
//! Depois que `f` resolve, a fila virtual é esvaziada (então qualquer timer que `f` agendou e esperou,
//! transitivamente, termina de forma determinística). Se `f` falha, a fila *não* é esvaziada: o erro
//! original sai direto.
//!
//! É construído SOBRE o [`runtime`](crate::runtime) existente, sem segundo agendador: instala a API
//! virtual como a `TimeApi` do thread (`push_global_time_api`), então `runtime::set_timeout`,
//! `runtime::date_now` e `runtime::time_api()` caem no `VirtualClock`; o `VirtualTimeProcessor` dispara
//! os eventos e usa a API real, isto é, os timers nativos do mesmo loop, só para ceder a vez entre
//! eventos. Não confundir com `common::run_with_faked_timers` dos testes (que roda sobre
//! `runtime::with_faked_timers`, o porte do `timeTravelScheduler.ts` antigo, que troca o relógio do
//! próprio loop).
//!
//! Notas de tradução:
//! - `async function runWithFakedTimers<T>(options, fn: () => Promise<T>): Promise<T>` é uma função que
//!   devolve `Promise<T>` e leva `f: impl FnOnce() -> F` onde `F` é um `Future` que dá `Result<T, Error>`
//!   (a função `async` do porte). O primeiro trecho roda de forma síncrona, como a função `async` do
//!   original: a API virtual já está instalada quando `f` é chamada.
//! - `RunWithFakedTimersOptions` é uma struct com `Default`; `onHistory` recebe os eventos como
//!   [`RecordedTimerEvent`] (no modo virtual, o histórico do processador convertido).
//! - O `try/finally` que restaura os globais é um guarda no `Drop`, o que cobre também o pânico e o
//!   futuro descartado no meio, para a API virtual nunca vazar para o próximo teste do thread.
//! - Se `f` falha, a rejeição do `run` (por exemplo, o processador descartado) é engolida
//!   (`runPromise.catch(() => {})`) e o erro de `f` é o devolvido; se `f` termina bem e o `run` rejeita,
//!   a rejeição do `run` é a devolvida.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use super::embedding::drain_microtasks_embedding;
use super::global_time_api::push_global_time_api;
use super::processor::{RunOptions, VirtualTimeProcessor, VirtualTimeProcessorOptions, until_token};
use super::recording_time_api::{RecordedTimerEvent, create_recording_real_time_api};
use super::time_api::real_time_api;
use super::virtual_clock::{VirtualClock, VirtualTime};
use super::virtual_time_api::{CreateVirtualTimeApiOptions, create_virtual_time_api};
use crate::async_::async_fn;
use crate::cancellation::CancellationTokenSource;
use crate::errors::Error;
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::promise::Promise;

/// `onHistory`.
pub type OnHistory = Rc<dyn Fn(&[RecordedTimerEvent])>;

#[derive(Clone, Default)]
pub struct RunWithFakedTimersOptions {
    pub start_time: Option<VirtualTime>,
    /// Padrão `true`. `Some(false)` ignora o tempo virtual por inteiro (para os casos em que o mesmo
    /// teste é parametrizado sobre tempo real e virtual).
    pub use_fake_timers: Option<bool>,
    /// Sem efeito no novo processador; aceito por compatibilidade com o legado. O embedding que esvazia
    /// microtarefas escolhe sozinho a primitiva de macrotarefa mais rápida.
    pub use_set_immediate: Option<bool>,
    /// Máximo de eventos virtuais que o run pode executar antes de ser rejeitado. Padrão 100.
    pub max_task_count: Option<usize>,
    /// Se definido, chamado quando `f` resolve com os eventos de timer gravados. No modo virtual os
    /// eventos vêm do histórico do próprio [`VirtualTimeProcessor`]; no modo real uma API de gravação
    /// embrulhando a API de tempo do host fica instalada enquanto `f` roda. Útil para diagnóstico de
    /// raias.
    pub on_history: Option<OnHistory>,
}

/// Descarta o handle ao sair do escopo, por qualquer caminho (inclusive pânico).
struct DisposeOnDrop(DisposableRef);

impl Drop for DisposeOnDrop {
    fn drop(&mut self) {
        self.0.dispose();
    }
}

/// `runWithFakedTimers(options, fn)`.
pub fn run_with_faked_timers<T, F>(options: RunWithFakedTimersOptions, f: impl FnOnce() -> F + 'static) -> Promise<T>
where
    T: Clone + 'static,
    F: Future<Output = Result<T, Error>> + 'static,
{
    async_fn(async move {
        if options.use_fake_timers == Some(false) {
            return run_real(&options, f).await;
        }
        run_virtual(&options, f).await
    })
}

/// O ramo `useFakeTimers: false`: sem tempo virtual, com gravação só se pediram o histórico.
async fn run_real<T, F>(options: &RunWithFakedTimersOptions, f: impl FnOnce() -> F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    let Some(on_history) = options.on_history.clone() else { return f().await };
    let history = Rc::new(RefCell::new(Vec::new()));
    let restore = DisposeOnDrop(push_global_time_api(create_recording_real_time_api(history.clone())));
    let result = f().await;
    drop(restore);
    on_history(&history.borrow());
    result
}

async fn run_virtual<T, F>(options: &RunWithFakedTimersOptions, f: impl FnOnce() -> F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    let clock = Rc::new(VirtualClock::new(options.start_time.unwrap_or(0)));
    let virtual_api = create_virtual_time_api(clock.clone(), CreateVirtualTimeApiOptions::default());
    let restore_globals = DisposeOnDrop(push_global_time_api(virtual_api));

    let real_api = real_time_api();
    let processor = VirtualTimeProcessor::new(
        clock,
        drain_microtasks_embedding(real_api.clone()),
        real_api,
        VirtualTimeProcessorOptions { default_max_events: Some(options.max_task_count.unwrap_or(100)) },
    );

    let cts = CancellationTokenSource::new(None);
    let run_promise = processor.run(RunOptions::new(until_token(cts.token())));

    let result = f().await;

    // Para de interceptar o agendamento real antes de esvaziar: as tarefas agendadas durante o próprio
    // esvaziamento não podem voltar para a fila virtual.
    drop(restore_globals);
    cts.cancel();

    let drained = if result.is_ok() {
        run_promise.await
    } else {
        // Evita uma rejeição sem tratador caso o descarte rejeite o run.
        let _ = run_promise.catch(|_| ());
        Ok(())
    };

    cts.dispose();
    if let Some(on_history) = &options.on_history {
        let history: Vec<RecordedTimerEvent> = processor.history().iter().map(RecordedTimerEvent::from).collect();
        on_history(&history);
    }
    processor.dispose();

    result.and_then(|value| drained.map(|()| value))
}
