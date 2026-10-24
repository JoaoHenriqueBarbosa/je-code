//! Tradução de `runtime/JSAsyncGenerator.{h,cpp}` e `JSAsyncGeneratorInlines.h`: constantes, enums e a
//! célula `JSAsyncGenerator` (`CellEntry::AsyncGenerator`), um `JSInternalFieldObjectImpl<10>`.
//!
//! Fora desta fatia: `asyncGeneratorNext(globalObject, generator, argument, MicrotaskCallCache*)`, que
//! depende do `MicrotaskCallCache` e do despacho de microtarefas.
//!
//! DIVERGÊNCIA: `dequeue` devolve o alvo como `JSValue` (o `JSObject*` do C++), porque o alvo pode ser um
//! `JSModuleRecord`, que ainda não é objeto portado do registro.

use crate::runtime::js_generator;
use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_promise_reaction::{JSPromiseReactionRef, JSSlimPromiseReaction, JSSlimPromiseReactionRef};
use crate::runtime::js_value::{js_null, js_number_i32, js_undefined, JSValue};

/// `JSInternalFieldObjectImpl<10>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 10;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorState {
    Completed = -1,
    Executing = -2,
    Init = 0,
    DrainingQueue = -3,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorSuspendReason {
    Await = 0,
    Yield = 1,
    /// `yield*`: entrega o valor sem `Await` envolvente.
    YieldNoAwait = 2,
}

pub const REASON_MASK: i32 = 0x3;
pub const REASON_SHIFT: i32 = 2;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsyncGeneratorResumeMode {
    Empty = -1,
    Normal = 0,
    Return = 1,
    Throw = 2,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
    Queue = 4,
    ResumeValue = 5,
    ResumeMode = 6,
    ResumePromise = 7,
    CachedDriverResult = 8,
    CachedDriverResultTarget = 9,
}

/// `JSAsyncGenerator::initialValues()`.
fn initial_values() -> [JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
    [
        js_number_i32(AsyncGeneratorState::Init as i32),
        js_undefined(),
        js_undefined(),
        js_undefined(),
        js_null(),
        js_undefined(),
        js_number_i32(AsyncGeneratorResumeMode::Empty as i32),
        js_undefined(),
        js_undefined(),
        js_undefined(),
    ]
}

define_internal_field_cell!(
    JSAsyncGenerator,
    JSAsyncGeneratorRef,
    AsyncGenerator,
    JSAsyncGeneratorType,
    JS_ASYNC_GENERATOR_S_INFO,
    "AsyncGenerator",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    initial_values()
);

/// O elo da fila circular: a célula do `JSValue` é sempre uma `JSSlimPromiseReaction`.
fn slim_request(value: JSValue) -> JSSlimPromiseReactionRef {
    match JSPromiseReactionRef::from_value(&value) {
        Some(JSPromiseReactionRef::Slim(slim)) => slim,
        _ => unreachable!("a fila do async generator só guarda JSSlimPromiseReaction"),
    }
}

impl JSAsyncGenerator {
    /// `state()`.
    pub fn state(&self) -> i32 {
        self.internal_field_as_int32(Field::State as u32)
    }

    /// `setState(state)`.
    pub fn set_state(&self, state: i32) {
        self.set_internal_field(Field::State as u32, js_number_i32(state));
    }

    /// `next()`.
    pub fn next(&self) -> JSValue {
        self.internal_field(Field::Next as u32)
    }

    /// `thisValue()`.
    pub fn this_value(&self) -> JSValue {
        self.internal_field(Field::This as u32)
    }

    /// `frame()`.
    pub fn frame(&self) -> JSValue {
        self.internal_field(Field::Frame as u32)
    }

    /// `queue()`.
    pub fn queue(&self) -> JSValue {
        self.internal_field(Field::Queue as u32)
    }

    /// `setQueue(vm, value)`.
    pub fn set_queue(&self, value: JSValue) {
        self.set_internal_field(Field::Queue as u32, value);
    }

    /// `resumeValue()`.
    pub fn resume_value(&self) -> JSValue {
        self.internal_field(Field::ResumeValue as u32)
    }

    /// `setResumeValue(vm, value)`.
    pub fn set_resume_value(&self, value: JSValue) {
        self.set_internal_field(Field::ResumeValue as u32, value);
    }

    /// `resumeMode()`.
    pub fn resume_mode(&self) -> i32 {
        self.internal_field_as_int32(Field::ResumeMode as u32)
    }

    /// `setResumeMode(mode)`.
    pub fn set_resume_mode(&self, mode: i32) {
        self.set_internal_field(Field::ResumeMode as u32, js_number_i32(mode));
    }

    /// `resumePromise()`.
    pub fn resume_promise(&self) -> JSValue {
        self.internal_field(Field::ResumePromise as u32)
    }

    /// `setResumePromise(vm, value)`.
    pub fn set_resume_promise(&self, value: JSValue) {
        self.set_internal_field(Field::ResumePromise as u32, value);
    }

    /// `cachedDriverResult()`.
    pub fn cached_driver_result(&self) -> JSValue {
        self.internal_field(Field::CachedDriverResult as u32)
    }

    /// `setCachedDriverResult(vm, value)`.
    pub fn set_cached_driver_result(&self, value: JSValue) {
        self.set_internal_field(Field::CachedDriverResult as u32, value);
    }

    /// `cachedDriverResultTarget()`.
    pub fn cached_driver_result_target(&self) -> JSValue {
        self.internal_field(Field::CachedDriverResultTarget as u32)
    }

    /// `setCachedDriverResultTarget(vm, value)`.
    pub fn set_cached_driver_result_target(&self, value: JSValue) {
        self.set_internal_field(Field::CachedDriverResultTarget as u32, value);
    }

    /// `isQueueEmpty()`.
    pub fn is_queue_empty(&self) -> bool {
        self.resume_mode() == AsyncGeneratorResumeMode::Empty as i32
    }

    /// `enqueue(vm, value, resumeMode, settlementTarget)`: a primeira requisição mora nos campos
    /// `ResumeValue`/`ResumeMode`/`ResumePromise`; as seguintes entram na fila circular (o campo `Queue`
    /// guarda a cauda, e `tail->next()` é a cabeça).
    pub fn enqueue(&self, value: JSValue, resume_mode: i32, settlement_target: JSValue) {
        debug_assert!(settlement_target.is_cell());
        if self.is_queue_empty() {
            self.set_resume_value(value);
            self.set_resume_mode(resume_mode);
            self.set_resume_promise(settlement_target);
            return;
        }

        let last = self.queue();
        if last.is_null() {
            let item = JSSlimPromiseReaction::create_async_generator_request(settlement_target, value, resume_mode as u8, None);
            item.set_next(Some(JSPromiseReactionRef::Slim(item.clone())));
            self.set_queue(item.as_value());
        } else {
            let tail = slim_request(last);
            let head = tail.next();
            let item = JSSlimPromiseReaction::create_async_generator_request(settlement_target, value, resume_mode as u8, head);
            tail.set_next(Some(JSPromiseReactionRef::Slim(item.clone())));
            self.set_queue(item.as_value());
        }
    }

    /// `dequeue(vm)`: devolve o alvo da requisição que sai e promove a próxima da fila, se houver.
    pub fn dequeue(&self) -> JSValue {
        debug_assert!(!self.is_queue_empty());

        let settlement_target = self.resume_promise();

        let last = self.queue();
        if last.is_null() {
            self.set_resume_mode(AsyncGeneratorResumeMode::Empty as i32);
            self.set_resume_value(js_undefined());
            self.set_resume_promise(js_undefined());
        } else {
            let tail = slim_request(last);
            let head = match tail.next() {
                Some(JSPromiseReactionRef::Slim(head)) => head,
                _ => unreachable!("a fila do async generator é circular e só tem JSSlimPromiseReaction"),
            };

            self.set_resume_promise(head.promise());
            self.set_resume_value(head.handler_or_context());
            self.set_resume_mode(i32::from(head.async_generator_resume_mode()));

            if head.cell_id() == tail.cell_id() {
                self.set_queue(js_null());
            } else {
                tail.set_next(head.next());
            }
        }

        settlement_target
    }
}

/// `static bool isSuspendedYieldState(int32_t)`: estado positivo cujos bits de razão são `Yield`.
pub fn is_suspended_yield_state(state: i32) -> bool {
    state > 0 && (state & REASON_MASK) == AsyncGeneratorSuspendReason::Yield as i32
}

/// `static bool isExecutingState(int32_t)`.
pub fn is_executing_state(state: i32) -> bool {
    if state == AsyncGeneratorState::Executing as i32 {
        return true;
    }
    state > 0 && (state & REASON_MASK) == AsyncGeneratorSuspendReason::Await as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_fifo_through_resume_fields_and_circular_list() {
        let vm = crate::runtime::vm::VM::new();
        let structure = JSAsyncGenerator::create_structure(&vm, None, js_null());
        let generator = JSAsyncGenerator::create(&vm, &structure);
        assert!(generator.is_queue_empty());
        let targets: Vec<JSValue> = (0..3)
            .map(|_| JSAsyncGenerator::create(&vm, &structure).as_value())
            .collect();
        for (index, target) in targets.iter().enumerate() {
            generator.enqueue(js_number_i32(index as i32), AsyncGeneratorResumeMode::Normal as i32 + index as i32 % 3, *target);
        }
        assert!(!generator.is_queue_empty());
        for (index, target) in targets.iter().enumerate() {
            assert_eq!(generator.resume_value(), js_number_i32(index as i32));
            assert_eq!(generator.resume_mode(), index as i32 % 3);
            assert_eq!(generator.dequeue(), *target);
        }
        assert!(generator.is_queue_empty());
        assert!(generator.queue().is_null());
    }

    #[test]
    fn matches_generator() {
        assert_eq!(AsyncGeneratorState::Completed as i32, js_generator::State::Completed as i32);
        assert_eq!(AsyncGeneratorState::Executing as i32, js_generator::State::Executing as i32);
        assert_eq!(AsyncGeneratorState::Init as i32, js_generator::State::Init as i32);
        assert_eq!(AsyncGeneratorResumeMode::Normal as i32, js_generator::ResumeMode::NormalMode as i32);
        assert_eq!(AsyncGeneratorResumeMode::Return as i32, js_generator::ResumeMode::ReturnMode as i32);
        assert_eq!(AsyncGeneratorResumeMode::Throw as i32, js_generator::ResumeMode::ThrowMode as i32);
        assert_eq!(Field::CachedDriverResultTarget as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert!(is_executing_state(-2));
        assert!(is_executing_state(4));
        assert!(is_suspended_yield_state(5));
        assert!(!is_suspended_yield_state(4));
    }
}
