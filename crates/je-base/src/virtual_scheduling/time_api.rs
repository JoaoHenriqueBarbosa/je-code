//! Porte de `src/vs/base/test/common/virtualScheduling/timeApi.ts`: o subconjunto das APIs de tempo do
//! host de que o processador e os embeddings precisam, e a API real.
//!
//! Notas de tradução:
//! - Os ids dos timers (`TimeoutId`, `IntervalId`, `ImmediateId`, `AnimationFrameId`) são todos o
//!   [`TimerId`] do loop. O original os distingue só por marca de tipo, e dividir a numeração deixa a
//!   API real chamar o agendador do [`runtime`] sem converter.
//! - `Date: DateConstructor` vira [`TimeApi::date_now`] (o que o código de teste lê do `Date` é o
//!   `Date.now()`; um `new Date()` sem argumentos é o mesmo relógio). `performanceNow` e
//!   `performanceTimeOrigin` viram [`TimeApi::performance_now`] e [`TimeApi::performance_time_origin`].
//! - Os membros opcionais (`setImmediate`, `requestAnimationFrame` e seus cancelamentos) viram métodos
//!   com implementação padrão que devolve o `handler` em `Err`: "a API não tem isso", e quem chamou
//!   usa outro caminho com o mesmo `handler` (o `if (api.setImmediate)` do original).
//! - Os handlers de `setTimeout`/`setImmediate` são `FnOnce` (disparam uma vez) e o de `setInterval` é
//!   `Rc<dyn Fn()>` (dispara várias).
//! - `captureGlobalTimeApi` captura a API instalada no thread ([`runtime::time_api`]); o
//!   `globalThis.setTimeout.bind(globalThis)` do original não existe em Rust. `realTimeApi` (o
//!   instantâneo do host no carregamento do módulo) é a [`RealTimeApi`], que chama o agendador nativo
//!   do loop e por isso nunca fica "presa" numa API virtual instalada.
//! - A API real não tem `requestAnimationFrame` (o loop não tem quadro de pintura). O `setImmediate`
//!   dela é um timer nativo de atraso zero, que dispara depois de todas as microtarefas pendentes, na
//!   ordem de criação, como o `setImmediate` do Node entre as macrotarefas.
//! - O relógio de parede da API real é o do loop (`Date.now()` relativo ao início do thread, ver
//!   [`runtime::date_now`]); a origem do relógio monotônico dela é `0`, de modo que
//!   `date_now == performance_time_origin + performance_now`.

use std::rc::Rc;

use crate::runtime::{self, TimerId};

pub type TimeoutId = TimerId;
pub type IntervalId = TimerId;
pub type ImmediateId = TimerId;
pub type AnimationFrameId = TimerId;

/// Uma tarefa que roda uma vez.
pub type Handler = Box<dyn FnOnce()>;

/// O callback de `requestAnimationFrame`: recebe o `performance.now()` do quadro.
pub type FrameCallback = Box<dyn FnOnce(f64)>;

/// Gera, para uma API que embrulha outra (o caminho até ela, como `underlying` ou `recorder.real`), os
/// métodos que só repassam: o que o embrulho não intercepta (cancelamentos e relógios). Definido uma
/// vez para a API de log e a de gravação.
macro_rules! delegate_time_api_passthrough {
    ($($path:ident).+) => {
        fn clear_timeout(&self, id: $crate::virtual_scheduling::time_api::TimeoutId) {
            self.$($path).+.clear_timeout(id);
        }

        fn clear_interval(&self, id: $crate::virtual_scheduling::time_api::IntervalId) {
            self.$($path).+.clear_interval(id);
        }

        fn clear_immediate(&self, id: $crate::virtual_scheduling::time_api::ImmediateId) {
            self.$($path).+.clear_immediate(id);
        }

        fn cancel_animation_frame(&self, id: $crate::virtual_scheduling::time_api::AnimationFrameId) {
            self.$($path).+.cancel_animation_frame(id);
        }

        fn date_now(&self) -> u64 {
            self.$($path).+.date_now()
        }

        fn performance_now(&self) -> f64 {
            self.$($path).+.performance_now()
        }

        fn performance_time_origin(&self) -> f64 {
            self.$($path).+.performance_time_origin()
        }
    };
}
pub(crate) use delegate_time_api_passthrough;

/// O subconjunto das APIs de tempo do host de que o processador e os embeddings precisam.
///
/// Serve tanto para a API real do host ([`real_time_api`]) quanto para a virtual, que passa por um
/// `VirtualClock`. Como é uma interface simples, o processador nunca olha o estado global do thread: a
/// fronteira entre "tempo real" e "tempo virtual" é exatamente qual instância de `TimeApi` está em uso.
pub trait TimeApi {
    fn set_timeout(&self, handler: Handler, timeout: u64) -> TimeoutId;

    fn clear_timeout(&self, id: TimeoutId);

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> IntervalId;

    fn clear_interval(&self, id: IntervalId);

    /// `setImmediate?`: `Err(handler)` se a API não tem.
    fn set_immediate(&self, handler: Handler) -> Result<ImmediateId, Handler> {
        Err(handler)
    }

    /// `clearImmediate?`.
    fn clear_immediate(&self, _id: ImmediateId) {}

    /// `requestAnimationFrame?`: `Err(callback)` se a API não tem.
    fn request_animation_frame(&self, callback: FrameCallback) -> Result<AnimationFrameId, FrameCallback> {
        Err(callback)
    }

    /// `cancelAnimationFrame?`.
    fn cancel_animation_frame(&self, _id: AnimationFrameId) {}

    /// `Date.now()`.
    fn date_now(&self) -> u64;

    /// `performanceNow()`.
    fn performance_now(&self) -> f64;

    /// `performanceTimeOrigin`.
    fn performance_time_origin(&self) -> f64;
}

/// A API de tempo do próprio loop: o agendador nativo do [`runtime`], sem passar pela API instalada.
pub struct RealTimeApi;

impl TimeApi for RealTimeApi {
    fn set_timeout(&self, handler: Handler, timeout: u64) -> TimeoutId {
        runtime::native_set_timeout(handler, timeout)
    }

    fn clear_timeout(&self, id: TimeoutId) {
        runtime::native_clear_timeout(id);
    }

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> IntervalId {
        runtime::native_set_interval(handler, interval)
    }

    fn clear_interval(&self, id: IntervalId) {
        runtime::native_clear_interval(id);
    }

    fn set_immediate(&self, handler: Handler) -> Result<ImmediateId, Handler> {
        Ok(runtime::native_set_timeout(handler, 0))
    }

    fn clear_immediate(&self, id: ImmediateId) {
        runtime::native_clear_timeout(id);
    }

    fn date_now(&self) -> u64 {
        runtime::native_date_now()
    }

    #[allow(clippy::cast_precision_loss)]
    fn performance_now(&self) -> f64 {
        runtime::now() as f64
    }

    fn performance_time_origin(&self) -> f64 {
        0.0
    }
}

/// `captureGlobalTimeApi()`: a API instalada agora no thread.
pub fn capture_global_time_api() -> Rc<dyn TimeApi> {
    runtime::time_api()
}

/// `realTimeApi`: a API real do loop, que não muda com a que estiver instalada.
pub fn real_time_api() -> Rc<dyn TimeApi> {
    Rc::new(RealTimeApi)
}
