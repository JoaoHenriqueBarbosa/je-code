//! Tradução de `runtime/JSPromiseReaction.{h,cpp}`: a célula de reação de uma promessa pendente
//! (`JSPromiseReaction`, com as duas finais `JSSlimPromiseReaction` e `JSFullPromiseReaction`) e
//! `tryGetContext`.
//!
//! `#if USE(BUN_JSC_ADDITIONS)` vale (`createWithAsyncContext`, `promiseSlotIsAsyncContext`,
//! `contextIsAsyncContext`).
//!
//! DIVERGÊNCIAS (heap ausente):
//! - As reações são `JSCell` puros no C++. Aqui são valores compartilhados por `Rc`, registrados no
//!   `cell_registry` (`CellEntry::SlimPromiseReaction` e `CellEntry::FullPromiseReaction`, cujo
//!   `js_type` é o `JSSlimPromiseReactionType` e o `JSFullPromiseReactionType`). Não carregam
//!   `Structure` (`vm.slimPromiseReactionStructure` e `vm.fullPromiseReactionStructure` não existem),
//!   então `create` não recebe o `VM&`, e `createStructure` some. `finishCreation` e a barreira de
//!   escrita (`WriteBarrierEarlyInit`, `vm.writeBarrier`) somem.
//! - O `CompactPointerTuple<JSPromiseReaction*, uint8_t> m_next` vira dois campos: o `next` e o byte
//!   `payload` (a `InternalMicrotask`, ou o `resumeMode` do `createAsyncGeneratorRequest`).
//! - O bit por célula (`perCellBit`) do cabeçalho `JSCell` é um `Cell<bool>` da própria reação.
//! - A hierarquia `JSPromiseReaction` -> `Slim`/`Full` é composição com `Deref`; a referência
//!   polimórfica (`JSPromiseReaction*`) é o enum `JSPromiseReactionRef`.
//! - `visitChildren` some (sem GC).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::{promise_reaction_packs_global_context_and_index, InternalMicrotask};

/// `const ClassInfo JSPromiseReaction::s_info`.
pub static JS_PROMISE_REACTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "PromiseReaction", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo JSSlimPromiseReaction::s_info`.
pub static JS_SLIM_PROMISE_REACTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "SlimPromiseReaction", parent_class: Some(&JS_PROMISE_REACTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo JSFullPromiseReaction::s_info`.
pub static JS_FULL_PROMISE_REACTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "FullPromiseReaction", parent_class: Some(&JS_PROMISE_REACTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSPromiseReaction : public JSCell`: o que as duas finais têm em comum.
#[derive(Debug)]
pub struct JSPromiseReaction {
    /// O `cell_id` da célula que contém esta base.
    cell_id: Cell<usize>,
    /// `m_promise`.
    promise: Cell<JSValue>,
    /// `m_next.pointer()`.
    next: RefCell<Option<JSPromiseReactionRef>>,
    /// `m_next.type()`.
    payload: Cell<u8>,
    /// `perCellBit()` do cabeçalho `JSCell`.
    per_cell_bit: Cell<bool>,
}

impl JSPromiseReaction {
    /// `JSPromiseReaction::visitChildren`: a promessa (a próxima reação é `Rc`, já raiz).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.promise.get());
    }
    /// `JSPromiseReaction(vm, structure, promise, next, payload)`.
    fn new(promise: JSValue, next: Option<JSPromiseReactionRef>, payload: u8) -> JSPromiseReaction {
        JSPromiseReaction {
            cell_id: Cell::new(0),
            promise: Cell::new(promise),
            next: RefCell::new(next),
            payload: Cell::new(payload),
            per_cell_bit: Cell::new(false),
        }
    }

    /// `promise()`.
    pub fn promise(&self) -> JSValue {
        self.promise.get()
    }

    /// `next()`.
    pub fn next(&self) -> Option<JSPromiseReactionRef> {
        self.next.borrow().clone()
    }

    /// `internalMicrotask()`.
    pub fn internal_microtask(&self) -> InternalMicrotask {
        InternalMicrotask::from_u8(self.payload.get()).expect("byte da reação fora de InternalMicrotask")
    }

    /// `m_next.type()`.
    pub fn payload(&self) -> u8 {
        self.payload.get()
    }

    /// `setPromise(vm, value)`.
    pub fn set_promise(&self, value: JSValue) {
        self.promise.set(value);
    }

    /// `setNext(vm, value)`.
    pub fn set_next(&self, next: Option<JSPromiseReactionRef>) {
        *self.next.borrow_mut() = next;
    }

    /// `perCellBit()`.
    pub fn per_cell_bit(&self) -> bool {
        self.per_cell_bit.get()
    }

    /// `setPerCellBit(bool)`.
    pub fn set_per_cell_bit(&self, value: bool) {
        self.per_cell_bit.set(value);
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id.get()
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    /// `tryGetContext(JSValue reactionsValue)`: o contexto da reação, ou `JSValue()` (vazio) se o valor
    /// não é uma reação com contexto.
    pub fn try_get_context(reactions_value: JSValue) -> JSValue {
        match JSPromiseReactionRef::from_value(&reactions_value) {
            Some(JSPromiseReactionRef::Slim(slim)) => {
                let task = slim.internal_microtask();
                if task == InternalMicrotask::None {
                    return JSValue::empty();
                }
                if promise_reaction_packs_global_context_and_index(task) {
                    // Na verdade é o `JSPromiseCombinatorsGlobalContext`.
                    return slim.promise();
                }
                slim.handler_or_context()
            }
            Some(JSPromiseReactionRef::Full(full)) => full.context(),
            None => JSValue::empty(),
        }
    }
}

/// `class JSSlimPromiseReaction final : public JSPromiseReaction`.
#[derive(Debug)]
pub struct JSSlimPromiseReaction {
    base: JSPromiseReaction,
    /// `m_handlerOrContext`.
    handler_or_context: Cell<JSValue>,
}

pub type JSSlimPromiseReactionRef = Rc<JSSlimPromiseReaction>;

impl std::ops::Deref for JSSlimPromiseReaction {
    type Target = JSPromiseReaction;

    fn deref(&self) -> &JSPromiseReaction {
        &self.base
    }
}

impl JSSlimPromiseReaction {
    /// `visitChildren`.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        self.base.visit_children(marker);
        marker.value(self.handler_or_context.get());
    }
    /// O construtor privado e o `allocateCell`: cria a célula e a registra.
    fn allocate(
        promise: JSValue,
        handler_or_context: JSValue,
        next: Option<JSPromiseReactionRef>,
        payload: u8,
        per_cell_bit: bool,
    ) -> JSSlimPromiseReactionRef {
        let cell_id = cell_registry::reserve();
        let reaction = Rc::new(JSSlimPromiseReaction {
            base: JSPromiseReaction::new(promise, next, payload),
            handler_or_context: Cell::new(handler_or_context),
        });
        reaction.base.cell_id.set(cell_id);
        reaction.base.set_per_cell_bit(per_cell_bit);
        cell_registry::set(cell_id, CellEntry::SlimPromiseReaction(Rc::clone(&reaction)));
        reaction
    }

    /// `create(vm, promise, handler, isFulfill, next)`.
    pub fn create(promise: JSValue, handler: JSValue, is_fulfill: bool, next: Option<JSPromiseReactionRef>) -> JSSlimPromiseReactionRef {
        JSSlimPromiseReaction::allocate(promise, handler, next, InternalMicrotask::None as u8, is_fulfill)
    }

    /// `create(vm, promise, task, context, next)`.
    pub fn create_internal_microtask(
        promise: JSValue,
        task: InternalMicrotask,
        context: JSValue,
        next: Option<JSPromiseReactionRef>,
    ) -> JSSlimPromiseReactionRef {
        debug_assert!(task != InternalMicrotask::None);
        JSSlimPromiseReaction::allocate(promise, context, next, task as u8, false)
    }

    /// `createWithAsyncContext(vm, asyncContext, task, context, next)`: a tarefa não leva célula, e o
    /// contexto assíncrono capturado fica no campo `promise`, que sobra.
    pub fn create_with_async_context(
        async_context: JSValue,
        task: InternalMicrotask,
        context: JSValue,
        next: Option<JSPromiseReactionRef>,
    ) -> JSSlimPromiseReactionRef {
        debug_assert!(task != InternalMicrotask::None);
        debug_assert!(!async_context.is_empty() && !async_context.is_undefined());
        JSSlimPromiseReaction::allocate(async_context, context, next, task as u8, true)
    }

    /// `createAsyncGeneratorRequest(vm, settlementTarget, value, resumeMode, next)`.
    pub fn create_async_generator_request(
        settlement_target: JSValue,
        value: JSValue,
        resume_mode: u8,
        next: Option<JSPromiseReactionRef>,
    ) -> JSSlimPromiseReactionRef {
        JSSlimPromiseReaction::allocate(settlement_target, value, next, resume_mode, false)
    }

    /// `promiseSlotIsAsyncContext()`.
    pub fn promise_slot_is_async_context(&self) -> bool {
        self.internal_microtask() != InternalMicrotask::None && self.per_cell_bit()
    }

    /// `asyncGeneratorResumeMode()`.
    pub fn async_generator_resume_mode(&self) -> u8 {
        self.payload()
    }

    /// `isFulfillHandler()`.
    pub fn is_fulfill_handler(&self) -> bool {
        debug_assert!(self.internal_microtask() == InternalMicrotask::None);
        self.per_cell_bit()
    }

    /// `handlerOrContext()`.
    pub fn handler_or_context(&self) -> JSValue {
        self.handler_or_context.get()
    }

    /// `setHandlerOrContext(vm, value)`.
    pub fn set_handler_or_context(&self, value: JSValue) {
        self.handler_or_context.set(value);
    }
}

/// `class JSFullPromiseReaction final : public JSPromiseReaction`.
#[derive(Debug)]
pub struct JSFullPromiseReaction {
    base: JSPromiseReaction,
    /// `m_onFulfilled`.
    on_fulfilled: Cell<JSValue>,
    /// `m_onRejected`.
    on_rejected: Cell<JSValue>,
    /// `m_context`.
    context: Cell<JSValue>,
}

pub type JSFullPromiseReactionRef = Rc<JSFullPromiseReaction>;

impl std::ops::Deref for JSFullPromiseReaction {
    type Target = JSPromiseReaction;

    fn deref(&self) -> &JSPromiseReaction {
        &self.base
    }
}

impl JSFullPromiseReaction {
    /// `visitChildren`.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        self.base.visit_children(marker);
        marker.values(&[self.on_fulfilled.get(), self.on_rejected.get(), self.context.get()]);
    }
    /// O construtor privado e o `allocateCell`: cria a célula e a registra.
    fn allocate(
        promise: JSValue,
        on_fulfilled: JSValue,
        on_rejected: JSValue,
        context: JSValue,
        next: Option<JSPromiseReactionRef>,
        per_cell_bit: bool,
    ) -> JSFullPromiseReactionRef {
        let cell_id = cell_registry::reserve();
        let reaction = Rc::new(JSFullPromiseReaction {
            base: JSPromiseReaction::new(promise, next, InternalMicrotask::None as u8),
            on_fulfilled: Cell::new(on_fulfilled),
            on_rejected: Cell::new(on_rejected),
            context: Cell::new(context),
        });
        reaction.base.cell_id.set(cell_id);
        reaction.base.set_per_cell_bit(per_cell_bit);
        cell_registry::set(cell_id, CellEntry::FullPromiseReaction(Rc::clone(&reaction)));
        reaction
    }

    /// `create(vm, promise, onFulfilled, onRejected, context, next)`.
    pub fn create(
        promise: JSValue,
        on_fulfilled: JSValue,
        on_rejected: JSValue,
        context: JSValue,
        next: Option<JSPromiseReactionRef>,
    ) -> JSFullPromiseReactionRef {
        JSFullPromiseReaction::allocate(promise, on_fulfilled, on_rejected, context, next, false)
    }

    /// `createWithAsyncContext(vm, promise, onFulfilled, onRejected, asyncContext, next)`: o `m_context`
    /// é o contexto assíncrono capturado pelo `performPromiseThen`, não um contexto do embedder.
    pub fn create_with_async_context(
        promise: JSValue,
        on_fulfilled: JSValue,
        on_rejected: JSValue,
        async_context: JSValue,
        next: Option<JSPromiseReactionRef>,
    ) -> JSFullPromiseReactionRef {
        debug_assert!(!async_context.is_undefined());
        JSFullPromiseReaction::allocate(promise, on_fulfilled, on_rejected, async_context, next, true)
    }

    /// `contextIsAsyncContext()`.
    pub fn context_is_async_context(&self) -> bool {
        self.per_cell_bit()
    }

    /// `onFulfilled()`.
    pub fn on_fulfilled(&self) -> JSValue {
        self.on_fulfilled.get()
    }

    /// `onRejected()`.
    pub fn on_rejected(&self) -> JSValue {
        self.on_rejected.get()
    }

    /// `context()`.
    pub fn context(&self) -> JSValue {
        self.context.get()
    }

    /// `setOnFulfilled(vm, value)`.
    pub fn set_on_fulfilled(&self, value: JSValue) {
        self.on_fulfilled.set(value);
    }

    /// `setOnRejected(vm, value)`.
    pub fn set_on_rejected(&self, value: JSValue) {
        self.on_rejected.set(value);
    }

    /// `setContext(vm, value)`.
    pub fn set_context(&self, value: JSValue) {
        self.context.set(value);
    }
}

/// `JSPromiseReaction*`: a reação, slim ou full (o `JSType` do cabeçalho vira a variante).
#[derive(Clone, Debug)]
pub enum JSPromiseReactionRef {
    Slim(JSSlimPromiseReactionRef),
    Full(JSFullPromiseReactionRef),
}

impl JSPromiseReactionRef {
    /// A base `JSPromiseReaction`.
    pub fn base(&self) -> &JSPromiseReaction {
        match self {
            JSPromiseReactionRef::Slim(slim) => &***slim,
            JSPromiseReactionRef::Full(full) => &***full,
        }
    }

    /// O "endereço" da célula.
    pub fn cell_id(&self) -> usize {
        self.base().cell_id()
    }

    /// Procura a reação pelo `cell_id` (`uncheckedDowncast<JSPromiseReaction>`); `None` se o id não é de
    /// uma reação.
    pub fn from_cell_id(cell_id: usize) -> Option<JSPromiseReactionRef> {
        match cell_registry::get(cell_id)? {
            CellEntry::SlimPromiseReaction(slim) => Some(JSPromiseReactionRef::Slim(slim)),
            CellEntry::FullPromiseReaction(full) => Some(JSPromiseReactionRef::Full(full)),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSPromiseReaction>(JSValue)`.
    pub fn from_value(value: &JSValue) -> Option<JSPromiseReactionRef> {
        match value {
            JSValue::Cell(cell_id) => JSPromiseReactionRef::from_cell_id(*cell_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slim_handler_reaction_roundtrip() {
        let reaction = JSSlimPromiseReaction::create(JSValue::Int32(1), JSValue::Int32(2), true, None);
        assert_eq!(reaction.internal_microtask(), InternalMicrotask::None);
        assert!(reaction.is_fulfill_handler());
        assert_eq!(reaction.handler_or_context(), JSValue::Int32(2));
        let found = JSPromiseReactionRef::from_cell_id(reaction.cell_id()).expect("registrada");
        assert!(matches!(found, JSPromiseReactionRef::Slim(_)));
    }

    #[test]
    fn async_context_is_marked_by_the_per_cell_bit() {
        let reaction = JSSlimPromiseReaction::create_with_async_context(
            JSValue::Int32(7),
            InternalMicrotask::AsyncFunctionResume,
            JSValue::Int32(3),
            None,
        );
        assert!(reaction.promise_slot_is_async_context());
        let plain = JSSlimPromiseReaction::create_internal_microtask(
            JSValue::undefined(),
            InternalMicrotask::AsyncFunctionResume,
            JSValue::Int32(3),
            None,
        );
        assert!(!plain.promise_slot_is_async_context());
    }

    #[test]
    fn next_chain_and_try_get_context() {
        let tail = JSFullPromiseReaction::create(JSValue::undefined(), JSValue::null(), JSValue::null(), JSValue::Int32(9), None);
        let head = JSSlimPromiseReaction::create(
            JSValue::undefined(),
            JSValue::Int32(1),
            false,
            Some(JSPromiseReactionRef::Full(Rc::clone(&tail))),
        );
        assert_eq!(head.next().map(|next| next.cell_id()), Some(tail.cell_id()));
        assert_eq!(JSPromiseReaction::try_get_context(tail.as_value()), JSValue::Int32(9));
        // Reação slim sem tarefa interna não tem contexto.
        assert!(JSPromiseReaction::try_get_context(head.as_value()).is_empty());
        assert!(JSPromiseReaction::try_get_context(JSValue::Int32(1)).is_empty());
    }
}
