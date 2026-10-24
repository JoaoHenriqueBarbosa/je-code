//! Porte de `src/vs/base/test/common/virtualScheduling/loggingTimeApi.ts`: embrulha `underlying` para que
//! toda chamada a `setTimeout`, `setInterval`, `setImmediate` ou `requestAnimationFrame` invoque `on_call`
//! antes.
//!
//! Útil para diagnóstico, por exemplo para registrar timers criados fora do tempo virtual e achar
//! vazamentos de agendamento real numa fixture.
//!
//! Notas de tradução:
//! - `onCall(name, stack, handler?)` perde o terceiro argumento: o handler Rust é consumido pela chamada
//!   que agenda e não tem texto de origem para um diagnóstico exibir. `stack` é o `new Error().stack`
//!   formatado na hora (texto, como no original).
//! - Em `set_immediate` e `request_animation_frame`, a API de baixo pode não ter o recurso (o
//!   `underlying.setImmediate ? ... : undefined` do original): `on_call` só dispara se a chamada foi
//!   aceita, e a pilha é capturada antes. Nos timers (sempre presentes) `on_call` dispara antes de
//!   agendar, como no original.

use std::rc::Rc;

use super::time_api::{
    AnimationFrameId, FrameCallback, Handler, ImmediateId, IntervalId, TimeApi, TimeoutId, delegate_time_api_passthrough,
};
use crate::lifecycle::StackSource;

/// O `onCall` do original: `(name, stack)`.
pub type OnCall = Rc<dyn Fn(&str, Option<String>)>;

struct LoggingTimeApi {
    underlying: Rc<dyn TimeApi>,
    on_call: OnCall,
}

impl LoggingTimeApi {
    /// `new Error().stack`, formatado agora.
    fn stack() -> Option<String> {
        Some(StackSource::capture().stack())
    }
}

impl TimeApi for LoggingTimeApi {
    fn set_timeout(&self, handler: Handler, timeout: u64) -> TimeoutId {
        (self.on_call)("setTimeout", Self::stack());
        self.underlying.set_timeout(handler, timeout)
    }

    fn set_interval(&self, handler: Rc<dyn Fn()>, interval: u64) -> IntervalId {
        (self.on_call)("setInterval", Self::stack());
        self.underlying.set_interval(handler, interval)
    }

    fn set_immediate(&self, handler: Handler) -> Result<ImmediateId, Handler> {
        let stack = Self::stack();
        let scheduled = self.underlying.set_immediate(handler)?;
        (self.on_call)("setImmediate", stack);
        Ok(scheduled)
    }

    fn request_animation_frame(&self, callback: FrameCallback) -> Result<AnimationFrameId, FrameCallback> {
        let stack = Self::stack();
        let scheduled = self.underlying.request_animation_frame(callback)?;
        (self.on_call)("requestAnimationFrame", stack);
        Ok(scheduled)
    }

    delegate_time_api_passthrough!(underlying);
}

/// Embrulha `underlying` para que cada chamada de agendamento invoque `on_call` primeiro.
pub fn create_logging_time_api(underlying: Rc<dyn TimeApi>, on_call: impl Fn(&str, Option<String>) + 'static) -> Rc<dyn TimeApi> {
    Rc::new(LoggingTimeApi { underlying, on_call: Rc::new(on_call) })
}
