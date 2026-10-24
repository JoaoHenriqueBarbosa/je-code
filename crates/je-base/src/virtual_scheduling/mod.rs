//! Porte de `src/vs/base/test/common/virtualScheduling/` (`index.ts`): as primitivas de agendamento
//! virtual de testes. O VS Code as tem como a casa nova do tempo virtual: elas substituem o
//! `timeTravelScheduler.ts` antigo.
//!
//! # Ligação com o `runtime`
//!
//! O projeto já tem um loop de eventos com ordem de JavaScript e um relógio falso próprio
//! (`runtime::with_faked_timers`, o porte do `timeTravelScheduler.ts` antigo, usado pelos testes de
//! `common::run_with_faked_timers`). Esta pasta NÃO cria um segundo agendador: o que o
//! `globalTimeApi.ts` do original faz trocando `globalThis.setTimeout`, `setInterval`, `Date` e
//! `performance.now` aqui é instalar uma [`TimeApi`] no thread (`runtime::set_time_api`), para a qual
//! `runtime::set_timeout`, `runtime::clear_timeout`, `runtime::date_now` e `runtime::time_api()`
//! delegam. Sem nada instalado vale a [`RealTimeApi`], que chama o agendador nativo do loop (as funções
//! `native_*` do `runtime`).
//!
//! - [`create_virtual_time_api`] devolve a API que põe cada timer no [`VirtualClock`].
//! - [`VirtualTimeProcessor`] dispara os eventos do relógio, e cede a vez ao host entre eles pela API
//!   real (os timers nativos do mesmo loop, que rodam depois do fecho de microtarefas).
//! - [`run_with_faked_timers`] instala a API virtual, roda a função e esvazia a fila.
//!
//! Notas de tradução comuns: o estado global de teste (`TraceContext.instance`, a raiz de trace) é um por
//! thread; ids de timer são o `TimerId` do loop; `Date` e `performance` viram os métodos de [`TimeApi`]
//! (`date_now`, `performance_now`, `performance_time_origin`). Cada arquivo lista o resto.

pub mod time_api;

pub mod embedding;
pub mod global_time_api;
pub mod logging_time_api;
pub mod processor;
pub mod recording_time_api;
pub mod run_with_faked_timers;
pub mod trace;
pub mod trace_logger;
pub mod virtual_clock;
pub mod virtual_time_api;

pub use embedding::{Embedding, EmbeddingChoice, drain_microtasks_embedding, next_macrotask, sync_embedding};
pub use global_time_api::push_global_time_api;
pub use logging_time_api::{OnCall, create_logging_time_api};
pub use processor::{
    RunOptions, TerminationPolicy, VirtualTimeProcessor, VirtualTimeProcessorOptions, until_idle, until_time, until_token,
};
pub use recording_time_api::{RecordedHistory, RecordedTimerEvent, create_recording_real_time_api};
pub use run_with_faked_timers::{OnHistory, RunWithFakedTimersOptions, run_with_faked_timers};
pub use time_api::{
    AnimationFrameId, FrameCallback, Handler, ImmediateId, IntervalId, RealTimeApi, TimeApi, TimeoutId, capture_global_time_api,
    real_time_api,
};
pub use trace::{AfterMicrotaskClosure, RunAsHandlerOptions, Trace, TraceContext, create_trace_root, root_trace};
pub use trace_logger::{TraceLogBuffer, TraceLogEntry, TraceLogLevel, TraceLogger, create_trace_logger};
pub use virtual_clock::{EventSource, VirtualClock, VirtualEvent, VirtualTime};
pub use virtual_time_api::{CreateVirtualTimeApiOptions, create_virtual_time_api};
