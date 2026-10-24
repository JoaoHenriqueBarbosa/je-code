//! Tradução de `runtime/JSPromise.{h,cpp}`: a célula `JSPromise` (estado, bits de manuseio, reação
//! inline), `performPromiseThen*`, `resolve`/`reject`/`fulfill` (e as versões `*Promise` da
//! especificação), o disparo das reações pendentes (`triggerPromiseReactions`) e as variantes com
//! `InternalMicrotask` (`resolveWithInternalMicrotask*`, `pipeFrom`). A parte que constrói
//! capacidades, funções de resolução e `then`/`promiseResolve`/`promiseReject` está em
//! `js_promise_capability.rs`; a reação em `js_promise_reaction.rs`; o que se pede do global object
//! em `js_promise_host.rs`.
//!
//! `#if USE(BUN_JSC_ADDITIONS)` vale (`derived/cmakeconfig.h`): o contexto assíncrono
//! (`AsyncContextSwapScope`), `performPromiseThenWithContext`, `fulfillWithNonPromise` fica de fora
//! (veja abaixo), `forEachPendingReaction`, a fila síncrona do carregador de módulos.
//!
//! `JSInternalPromise` não existe neste fork (o `JSInternalPromise.{h,cpp}` não está em
//! `upstream/JavaScriptCore/runtime`; o carregador de módulos usa `JSPromise` direto), então não há o
//! que portar dele.
//!
//! Fora desta fatia, e por quê: `fulfillWithNonPromise` (só declarado em `JSPromise.h`, sem corpo em
//! `JSPromise.cpp` deste fork), `performPromiseThenExported` e `createWithInitialValues` (repasse puro
//! de outra função, regra do `CLAUDE.md`: o chamador usa `perform_promise_then`/`create`),
//! as sobrecargas `reject(vm, Exception*)` e `rejectAsHandled(vm, Exception*)` (o chamador passa
//! `exception.value()`), `DECLARE_VISIT_CHILDREN`, `offsetOfPacked`/`offsetOfSlot` (JIT).
//!
//! DIVERGÊNCIAS:
//! - Sem GC: o `m_packed` (`CompactPointerTuple<JSCell*, uint16_t>`) vira `packed_flags` (os 16 bits de
//!   flags) e `payload` (o `cell_id` da célula do payload, `None` é o `nullptr`); o `m_slot` é um
//!   `Cell<JSValue>`. O cast do payload (`uncheckedDowncast<JSPromiseReaction>`) é a consulta ao
//!   `cell_registry` (`JSPromiseReactionRef::from_cell_id`). As barreiras de escrita somem.
//! - O `JSGlobalObject*` e o `VM&` de cada função são o `&dyn PromiseHost` (veja `js_promise_host.rs`);
//!   onde o C++ lê `realm()` o chamador passa o host do realm da promessa. O `PromiseResolveThenableJobFast`
//!   é enfileirado no host passado, não em `promise->realm()` (há um realm só).
//! - `isDefinitelyNonThenable` não grava nem lê o cache `Structure::definitelyNonThenableState` (o
//!   acessor não existe no porte): sempre faz a caminhada pela cadeia de protótipos, que é o que o
//!   C++ faz quando o estado é `NotComputed`. O cache só evita essa caminhada, então o resultado é o
//!   mesmo. `promise->realm() == globalObject` é o `PromiseHost::owns_promise`, e o
//!   `runInternalMicrotask` (`JSMicrotask.cpp`, ainda não portado) é o
//!   `PromiseHost::run_internal_microtask`.
//! - Os `ASSERT(!value.inherits<Exception>())` somem (no porte uma `Exception` não é `JSValue`).

use std::cell::Cell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::error_type::ErrorType;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_promise_host::{JSPromiseRejectionOperation, PromiseHost, PromiseProperty, Thrown};
use crate::runtime::js_promise_reaction::{
    JSFullPromiseReaction, JSPromiseReaction, JSPromiseReactionRef, JSSlimPromiseReaction,
};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::{
    is_module_loader_internal_microtask, promise_reaction_packs_global_context_and_index, InternalMicrotask,
    SynchronousModuleTask, PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG,
};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::INVALID_OFFSET;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSPromise::s_info`.
pub static JS_PROMISE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Promise", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `enum class JSPromise::Status : uint16_t`.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Making this as 0, so that, we can change the status from Pending to others without masking.
    Pending = 0,
    Fulfilled = 1,
    Rejected = 2,
}

impl Status {
    /// `static_cast<Status>(flags & stateMask)`.
    pub fn from_flags(flags: u16) -> Status {
        match flags & STATE_MASK {
            0 => Status::Pending,
            1 => Status::Fulfilled,
            _ => Status::Rejected,
        }
    }

    /// `static_cast<uint8_t>(status)`: o payload das tarefas enfileiradas.
    pub fn payload(self) -> u8 {
        self as u16 as u8
    }
}

/// `enum class JSPromise::InlineReactionKind : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineReactionKind {
    None = 0,
    InternalMicrotask = 1,
    FulfillHandler = 2,
    RejectHandler = 3,
}

pub const STATE_MASK: u16 = 0b0000_0000_0000_0011;
pub const IS_HANDLED_FLAG: u16 = 0b0000_0000_0000_0100;
pub const IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG: u16 = 0b0000_0000_0000_1000;
pub const INLINE_REACTION_KIND_MASK: u16 = 0b0000_0000_0011_0000;
pub const INLINE_REACTION_MICROTASK_MASK: u16 = 0b0011_1111_1100_0000;
pub const INLINE_REACTION_ASYNC_CONTEXT_FLAG: u16 = 0b0100_0000_0000_0000;
pub const INLINE_REACTION_KIND_SHIFT: u32 = 4;
pub const INLINE_REACTION_MICROTASK_SHIFT: u32 = 6;

/// `JSPromise::StructureFlags` (= `JSNonFinalObject::StructureFlags`).
pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

/// `class JSPromise : public JSNonFinalObject`.
///
/// Layout das flags (os 16 bits altos de `m_packed`):
///   bits 0-1: `Status`; bit 2: `isHandled`; bit 3: `isFirstResolvingFunctionCalled`;
///   bits 4-5: `InlineReactionKind`; bits 6-13: `InternalMicrotask` (só com kind == InternalMicrotask);
///   bit 14: o payload é o contexto assíncrono capturado, não o argumento de célula da tarefa;
///   bit 15: reservado.
#[derive(Debug)]
pub struct JSPromise {
    base: JSNonFinalObject,
    /// `m_packed.type()`.
    packed_flags: Cell<u16>,
    /// `m_packed.pointer()`: o `cell_id` do payload (`None` é o `nullptr`).
    payload: Cell<Option<usize>>,
    /// `m_slot`.
    slot: Cell<JSValue>,
}

/// `JSPromise*`.
pub type JSPromiseRef = Rc<JSPromise>;

impl std::ops::Deref for JSPromise {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

/// `cell ? JSValue(cell) : jsUndefined()`.
fn cell_value(cell: Option<usize>) -> JSValue {
    match cell {
        Some(cell_id) => JSValue::from_cell(cell_id),
        None => JSValue::undefined(),
    }
}

/// O ramo repetido de `vm.m_synchronousModuleQueue && isModuleLoaderInternalMicrotask(task)`: a tarefa do
/// carregador de módulos vai para a fila síncrona se ela existe, e as demais para a fila global.
fn queue_internal_microtask(host: &dyn PromiseHost, task: InternalMicrotask, payload: u8, arguments: &[JSValue]) {
    if host.has_synchronous_module_queue() && is_module_loader_internal_microtask(task) {
        host.append_synchronous_module_task(SynchronousModuleTask::new(task, payload, arguments));
        return;
    }
    host.queue_microtask(task, payload, arguments);
}

impl JSPromise {
    /// `JSPromise::visitChildren`: o payload (a cabeça das reações ou o resultado) e o `m_slot`.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        if let Some(payload) = self.payload.get() {
            marker.cell(payload);
        }
        marker.value(self.slot.get());
    }
    /// `create(vm, structure)`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSPromiseRef {
        let cell_id = cell_registry::reserve();
        let promise = Rc::new(JSPromise {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            packed_flags: Cell::new(0),
            payload: Cell::new(None),
            slot: Cell::new(JSValue::empty()),
        });
        promise.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::Promise(Rc::clone(&promise)));
        promise
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(JSType::JSPromiseType, STRUCTURE_FLAGS), &JS_PROMISE_S_INFO)
    }

    /// `uncheckedDowncast<JSPromise>(cell)`: procura a promessa pelo `cell_id`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSPromiseRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Promise(promise)) => Some(promise),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSPromise>(value)` (`value.inherits<JSPromise>()`).
    pub fn from_value(value: &JSValue) -> Option<JSPromiseRef> {
        match value {
            JSValue::Cell(cell_id) => JSPromise::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `realm()`: o `JSGlobalObject` da `Structure`.
    pub fn realm(&self) -> Option<JSGlobalObjectRef> {
        self.structure().realm()
    }

    /// `flags()`.
    pub fn flags(&self) -> u16 {
        self.packed_flags.get()
    }

    /// `status()`.
    pub fn status(&self) -> Status {
        Status::from_flags(self.flags())
    }

    /// `isHandled()`.
    pub fn is_handled(&self) -> bool {
        self.flags() & IS_HANDLED_FLAG != 0
    }

    /// `settlementValue()`: só vale depois da liquidação.
    pub fn settlement_value(&self) -> JSValue {
        debug_assert!(self.status() != Status::Pending);
        self.slot.get()
    }

    /// `result()`.
    pub fn result(&self) -> JSValue {
        if self.status() == Status::Pending {
            return JSValue::undefined();
        }
        self.slot.get()
    }

    /// `markAsHandled()` (https://webidl.spec.whatwg.org/#mark-a-promise-as-handled).
    pub fn mark_as_handled(&self) {
        self.set_flags(self.flags() | IS_HANDLED_FLAG);
    }

    /// `isFirstResolvingFunctionCalled()`.
    pub fn is_first_resolving_function_called(&self) -> bool {
        self.flags() & IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG != 0
    }

    /// `inlineReactionKind()`.
    pub fn inline_reaction_kind(&self) -> InlineReactionKind {
        match (self.flags() & INLINE_REACTION_KIND_MASK) >> INLINE_REACTION_KIND_SHIFT {
            0 => InlineReactionKind::None,
            1 => InlineReactionKind::InternalMicrotask,
            2 => InlineReactionKind::FulfillHandler,
            _ => InlineReactionKind::RejectHandler,
        }
    }

    /// `hasInlineReaction()`.
    pub fn has_inline_reaction(&self) -> bool {
        self.inline_reaction_kind() != InlineReactionKind::None
    }

    /// `hasInlineHandlerReaction()`.
    pub fn has_inline_handler_reaction(&self) -> bool {
        matches!(self.inline_reaction_kind(), InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler)
    }

    /// `inlineReactionContext()`.
    pub fn inline_reaction_context(&self) -> JSValue {
        debug_assert!(self.inline_reaction_kind() == InlineReactionKind::InternalMicrotask);
        self.slot.get()
    }

    /// `inlineReactionCarriesAsyncContext()`.
    pub fn inline_reaction_carries_async_context(&self) -> bool {
        self.flags() & INLINE_REACTION_ASYNC_CONTEXT_FLAG != 0
    }

    /// `inlineHandlerHandler()`.
    pub fn inline_handler_handler(&self) -> JSValue {
        debug_assert!(self.has_inline_handler_reaction());
        self.slot.get()
    }

    /// `payloadCell()`: o `cell_id` do payload.
    pub fn payload_cell(&self) -> Option<usize> {
        self.payload.get()
    }

    /// `inlineReactionMicrotask()`.
    pub fn inline_reaction_microtask(&self) -> InternalMicrotask {
        debug_assert!(self.inline_reaction_kind() == InlineReactionKind::InternalMicrotask);
        microtask_of_flags(self.flags())
    }

    /// `setFlags(newFlags)`.
    fn set_flags(&self, new_flags: u16) {
        self.packed_flags.set(new_flags);
    }

    /// `setPackedCell(vm, newFlags, cell)`.
    fn set_packed_cell(&self, new_flags: u16, cell: Option<usize>) {
        self.packed_flags.set(new_flags);
        self.payload.set(cell);
    }

    /// `setSlot(vm, value)`.
    fn set_slot(&self, value: JSValue) {
        self.slot.set(value);
    }

    /// `clearSlot()`.
    fn clear_slot(&self) {
        self.slot.set(JSValue::empty());
    }

    /// `asyncStackTraceContext()`: `JSValue()` (vazio) se não há.
    pub fn async_stack_trace_context(&self) -> JSValue {
        if self.status() != Status::Pending {
            return JSValue::empty();
        }
        match self.inline_reaction_kind() {
            InlineReactionKind::None => match self.payload_cell() {
                Some(head) => JSPromiseReaction::try_get_context(JSValue::from_cell(head)),
                None => JSValue::empty(),
            },
            InlineReactionKind::InternalMicrotask => {
                if promise_reaction_packs_global_context_and_index(self.inline_reaction_microtask()) {
                    debug_assert!(self.payload_cell().is_some());
                    return cell_value(self.payload_cell());
                }
                self.slot.get()
            }
            InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler => JSValue::empty(),
        }
    }

    /// `forEachPendingReaction(callback)`: a chamada devolve `false` para parar.
    pub fn for_each_pending_reaction(&self, callback: &mut dyn FnMut(InternalMicrotask, JSValue, JSValue) -> bool) {
        debug_assert!(self.status() == Status::Pending);
        match self.inline_reaction_kind() {
            InlineReactionKind::None => {
                let mut current = self.payload_cell().and_then(JSPromiseReactionRef::from_cell_id);
                while let Some(reaction) = current {
                    let mut promise = reaction.base().promise();
                    let context_or_handler = match &reaction {
                        JSPromiseReactionRef::Slim(slim) => {
                            if slim.promise_slot_is_async_context() {
                                promise = JSValue::undefined();
                            }
                            slim.handler_or_context()
                        }
                        JSPromiseReactionRef::Full(full) => full.context(),
                    };
                    if !callback(reaction.base().internal_microtask(), promise, context_or_handler) {
                        return;
                    }
                    current = reaction.base().next();
                }
            }
            InlineReactionKind::InternalMicrotask => {
                let cell = if self.inline_reaction_carries_async_context() { None } else { self.payload_cell() };
                callback(self.inline_reaction_microtask(), cell_value(cell), self.slot.get());
            }
            InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler => {
                callback(InternalMicrotask::None, cell_value(self.payload_cell()), self.slot.get());
            }
        }
    }

    /// `rejectedPromise(globalObject, value)`.
    pub fn rejected_promise(host: &dyn PromiseHost, value: JSValue) -> JSPromiseRef {
        let promise = JSPromise::create(host.vm(), &host.promise_structure());
        promise.reject(host, value);
        promise
    }

    /// `rejectedPromiseWithCaughtException(globalObject, scope)`: `None` se a exceção pendente é a de
    /// terminação (o `TRY_CLEAR_EXCEPTION` do C++ retorna `nullptr`).
    pub fn rejected_promise_with_caught_exception(host: &dyn PromiseHost, scope: &mut ThrowScope<'_>) -> Option<JSPromiseRef> {
        let exception = scope.exception().expect("rejectedPromiseWithCaughtException sem exceção pendente");
        if !scope.try_clear_exception() {
            return None;
        }
        scope.release();
        Some(JSPromise::rejected_promise(host, exception.value()))
    }

    /// `resolve(globalObject, vm, value)`: a função de resolução "first" (só a primeira chamada vale).
    pub fn resolve(&self, host: &dyn PromiseHost, value: JSValue) {
        if !self.is_first_resolving_function_called() {
            self.set_flags(self.flags() | IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG);
            self.resolve_promise(host, value);
        }
    }

    /// `reject(vm, value)`.
    pub fn reject(&self, host: &dyn PromiseHost, value: JSValue) {
        if !self.is_first_resolving_function_called() {
            self.set_flags(self.flags() | IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG);
            self.reject_promise(host, value);
        }
    }

    /// `fulfill(vm, value)`.
    pub fn fulfill(&self, host: &dyn PromiseHost, value: JSValue) {
        if !self.is_first_resolving_function_called() {
            self.set_flags(self.flags() | IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG);
            self.fulfill_promise(host, value);
        }
    }

    /// `pipeFrom(vm, from)`: liga a liquidação de `from` a esta promessa por uma microtask interna, sem
    /// efeito observável pelo usuário.
    pub fn pipe_from(&self, host: &dyn PromiseHost, from: &JSPromise) {
        if self.is_first_resolving_function_called() {
            return;
        }
        self.set_flags(self.flags() | IS_FIRST_RESOLVING_FUNCTION_CALLED_FLAG);
        from.perform_promise_then_with_internal_microtask(
            host,
            InternalMicrotask::PromiseFulfillWithoutHandlerJob,
            Some(self.cell_id()),
            JSValue::undefined(),
            JSValue::empty(),
        );
    }

    /// `rejectAsHandled(vm, value)`: marca antes de rejeitar, o que evita a ida e volta com o
    /// `PromiseRejectionTracker` e não é observável pelo usuário.
    pub fn reject_as_handled(&self, host: &dyn PromiseHost, value: JSValue) {
        if !self.is_first_resolving_function_called() {
            self.mark_as_handled();
            self.reject(host, value);
        }
    }

    /// `rejectWithCaughtException(vm, scope)`: `None` se a pendente é a exceção de terminação.
    pub fn reject_with_caught_exception(&self, host: &dyn PromiseHost, scope: &mut ThrowScope<'_>) -> Option<&JSPromise> {
        let exception = scope.exception().expect("rejectWithCaughtException sem exceção pendente");
        if !scope.try_clear_exception() {
            return None;
        }
        scope.release();
        self.reject(host, exception.value());
        Some(self)
    }

    /// `setInlineMicrotaskReaction(vm, task, cell, context, extraFlags)`.
    fn set_inline_microtask_reaction(&self, task: InternalMicrotask, cell: Option<usize>, context: JSValue, extra_flags: u16) {
        debug_assert!(self.status() == Status::Pending);
        debug_assert!(self.inline_reaction_kind() == InlineReactionKind::None);
        debug_assert!(self.payload_cell().is_none());
        debug_assert!(task != InternalMicrotask::None);
        // The inline reaction always implies markAsHandled; fold both into one flag update.
        let new_flags = self.flags()
            | IS_HANDLED_FLAG
            | extra_flags
            | ((InlineReactionKind::InternalMicrotask as u16) << INLINE_REACTION_KIND_SHIFT)
            | ((task as u16) << INLINE_REACTION_MICROTASK_SHIFT);
        self.set_slot(context);
        self.set_packed_cell(new_flags, cell);
    }

    /// `setInlineHandlerReaction(vm, kind, resultPromise, handler)`: `result_promise` é o `cell_id`.
    fn set_inline_handler_reaction(&self, kind: InlineReactionKind, result_promise: usize, handler: JSValue) {
        debug_assert!(self.status() == Status::Pending);
        debug_assert!(self.inline_reaction_kind() == InlineReactionKind::None);
        debug_assert!(self.payload_cell().is_none());
        debug_assert!(matches!(kind, InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler));
        let new_flags = self.flags() | IS_HANDLED_FLAG | ((kind as u16) << INLINE_REACTION_KIND_SHIFT);
        self.set_slot(handler);
        self.set_packed_cell(new_flags, Some(result_promise));
    }

    /// `spillInlineReaction(vm)`: troca a reação inline por uma célula de reação na lista da promessa.
    fn spill_inline_reaction(&self) -> JSPromiseReactionRef {
        let kind = self.inline_reaction_kind();
        let reaction = match kind {
            InlineReactionKind::InternalMicrotask => {
                let task = self.inline_reaction_microtask();
                let context = self.slot.get();
                let cell = self.payload_cell();
                if self.inline_reaction_carries_async_context() {
                    JSSlimPromiseReaction::create_with_async_context(cell_value(cell), task, context, None)
                } else {
                    JSSlimPromiseReaction::create_internal_microtask(cell_value(cell), task, context, None)
                }
            }
            InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler => {
                let result_promise = cell_value(self.payload_cell());
                let handler = self.slot.get();
                JSSlimPromiseReaction::create(result_promise, handler, kind == InlineReactionKind::FulfillHandler, None)
            }
            InlineReactionKind::None => unreachable!("spillInlineReaction sem reação inline"),
        };
        self.clear_slot();
        let new_flags =
            self.flags() & !(INLINE_REACTION_KIND_MASK | INLINE_REACTION_MICROTASK_MASK | INLINE_REACTION_ASYNC_CONTEXT_FLAG);
        self.set_packed_cell(new_flags, Some(reaction.cell_id()));
        JSPromiseReactionRef::Slim(reaction)
    }

    /// `reactionHead(vm)`: a cabeça da lista de reações (a inline é derramada antes).
    fn reaction_head(&self) -> Option<JSPromiseReactionRef> {
        debug_assert!(self.status() == Status::Pending);
        if self.inline_reaction_kind() != InlineReactionKind::None {
            return Some(self.spill_inline_reaction());
        }
        self.payload_cell().and_then(JSPromiseReactionRef::from_cell_id)
    }

    /// `performPromiseThen(vm, globalObject, onFulfilled, onRejected, promiseOrCapability)`.
    pub fn perform_promise_then(
        &self,
        host: &dyn PromiseHost,
        on_fulfilled: JSValue,
        on_rejected: JSValue,
        promise_or_capability: JSValue,
    ) {
        let fulfilled_callable = host.is_callable(on_fulfilled);
        let rejected_callable = host.is_callable(on_rejected);

        // The handler must run under the async context active now (AsyncContextSwapScope).
        let async_context = host.async_context();
        let has_async_context = !async_context.is_undefined();
        let payload_flags: u8 = if has_async_context { PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG } else { 0 };

        match self.status() {
            Status::Pending => {
                let only_fulfill = fulfilled_callable && !rejected_callable;
                let only_reject = !fulfilled_callable && rejected_callable;
                // Inline reactions have no slot for an async context, so fall through to
                // the heap-allocated JSFullPromiseReaction path when one is captured.
                if !has_async_context && self.inline_reaction_kind() == InlineReactionKind::None && self.payload_cell().is_none() {
                    if let (true, Some(result_promise)) = (only_fulfill || only_reject, JSPromise::from_value(&promise_or_capability)) {
                        let (kind, handler) = if only_fulfill {
                            (InlineReactionKind::FulfillHandler, on_fulfilled)
                        } else {
                            (InlineReactionKind::RejectHandler, on_rejected)
                        };
                        self.set_inline_handler_reaction(kind, result_promise.cell_id(), handler);
                        return;
                    }
                }
                let existing = self.reaction_head();
                let reaction = if has_async_context {
                    // Normalize non-callable sides to jsUndefined() so dispatch can use a
                    // tag check instead of isCallable().
                    JSPromiseReactionRef::Full(JSFullPromiseReaction::create_with_async_context(
                        promise_or_capability,
                        if fulfilled_callable { on_fulfilled } else { JSValue::undefined() },
                        if rejected_callable { on_rejected } else { JSValue::undefined() },
                        async_context,
                        existing,
                    ))
                } else if only_fulfill {
                    JSPromiseReactionRef::Slim(JSSlimPromiseReaction::create(promise_or_capability, on_fulfilled, true, existing))
                } else if only_reject {
                    JSPromiseReactionRef::Slim(JSSlimPromiseReaction::create(promise_or_capability, on_rejected, false, existing))
                } else if fulfilled_callable {
                    debug_assert!(rejected_callable);
                    JSPromiseReactionRef::Full(JSFullPromiseReaction::create(
                        promise_or_capability,
                        on_fulfilled,
                        on_rejected,
                        JSValue::undefined(),
                        existing,
                    ))
                } else {
                    JSPromiseReactionRef::Slim(JSSlimPromiseReaction::create_internal_microtask(
                        promise_or_capability,
                        InternalMicrotask::PromiseResolveWithoutHandlerJob,
                        JSValue::undefined(),
                        existing,
                    ))
                };
                self.set_packed_cell(self.flags() | IS_HANDLED_FLAG, Some(reaction.cell_id()));
            }
            Status::Rejected => {
                let settled = self.settlement_value();
                if !self.is_handled() {
                    host.promise_rejection_tracker(self, JSPromiseRejectionOperation::Handle);
                }
                if rejected_callable {
                    host.queue_microtask(
                        InternalMicrotask::PromiseReactionJob,
                        Status::Rejected.payload() | payload_flags,
                        &[promise_or_capability, on_rejected, settled, async_context],
                    );
                } else {
                    host.queue_microtask(
                        InternalMicrotask::PromiseResolveWithoutHandlerJob,
                        Status::Rejected.payload(),
                        &[promise_or_capability, settled, JSValue::undefined()],
                    );
                }
                self.mark_as_handled();
            }
            Status::Fulfilled => {
                let settled = self.settlement_value();
                if fulfilled_callable {
                    host.queue_microtask(
                        InternalMicrotask::PromiseReactionJob,
                        Status::Fulfilled.payload() | payload_flags,
                        &[promise_or_capability, on_fulfilled, settled, async_context],
                    );
                } else {
                    host.queue_microtask(
                        InternalMicrotask::PromiseResolveWithoutHandlerJob,
                        Status::Fulfilled.payload(),
                        &[promise_or_capability, settled, JSValue::undefined()],
                    );
                }
            }
        }
    }

    /// `performPromiseThenWithContext(vm, globalObject, onFulfilled, onRejected, promiseOrCapability, userContext)`.
    pub fn perform_promise_then_with_context(
        &self,
        host: &dyn PromiseHost,
        on_fulfilled: JSValue,
        on_rejected: JSValue,
        promise_or_capability: JSValue,
        user_context: JSValue,
    ) {
        let fulfilled_callable = host.is_callable(on_fulfilled);
        let rejected_callable = host.is_callable(on_rejected);

        // PromiseReactionJob unwraps an InternalFieldTuple context as [userContext,
        // asyncContext], so pair the two when an async context is captured - and also
        // when userContext is itself an InternalFieldTuple (e.g. the ReadableStream
        // async iterator's), which would otherwise be mistaken for that pair.
        let async_context = host.async_context();
        let context = if !async_context.is_undefined() || host.is_internal_field_tuple(user_context) {
            host.create_internal_field_tuple(user_context, async_context)
        } else {
            user_context
        };

        match self.status() {
            Status::Pending => {
                let existing = self.reaction_head();
                let reaction = JSFullPromiseReaction::create(
                    promise_or_capability,
                    if fulfilled_callable { on_fulfilled } else { JSValue::undefined() },
                    if rejected_callable { on_rejected } else { JSValue::undefined() },
                    context,
                    existing,
                );
                self.set_packed_cell(self.flags() | IS_HANDLED_FLAG, Some(reaction.cell_id()));
            }
            Status::Rejected => {
                let settled = self.settlement_value();
                if !self.is_handled() {
                    host.promise_rejection_tracker(self, JSPromiseRejectionOperation::Handle);
                }
                if rejected_callable {
                    host.queue_microtask(
                        InternalMicrotask::PromiseReactionJob,
                        Status::Rejected.payload(),
                        &[promise_or_capability, on_rejected, settled, context],
                    );
                } else {
                    host.queue_microtask(
                        InternalMicrotask::PromiseResolveWithoutHandlerJob,
                        Status::Rejected.payload(),
                        &[promise_or_capability, settled, JSValue::undefined()],
                    );
                }
                self.mark_as_handled();
            }
            Status::Fulfilled => {
                let settled = self.settlement_value();
                if fulfilled_callable {
                    host.queue_microtask(
                        InternalMicrotask::PromiseReactionJob,
                        Status::Fulfilled.payload(),
                        &[promise_or_capability, on_fulfilled, settled, context],
                    );
                } else {
                    host.queue_microtask(
                        InternalMicrotask::PromiseResolveWithoutHandlerJob,
                        Status::Fulfilled.payload(),
                        &[promise_or_capability, settled, JSValue::undefined()],
                    );
                }
            }
        }
    }

    /// `performPromiseThenWithInternalMicrotask(vm, task, cell, context, asyncContext)`. O
    /// `asyncContext` padrão do C++ (`JSValue()`) é `JSValue::empty()`; só as tarefas sem argumento de
    /// célula (a família do `await`) o capturam, e ele viaja nos campos que a célula usaria.
    pub fn perform_promise_then_with_internal_microtask(
        &self,
        host: &dyn PromiseHost,
        task: InternalMicrotask,
        cell: Option<usize>,
        context: JSValue,
        async_context: JSValue,
    ) {
        let has_async_context = !async_context.is_empty() && !async_context.is_undefined();
        assert!(!(has_async_context && cell.is_some()));
        let cell_argument = cell_value(cell);
        match self.status() {
            Status::Pending => {
                if self.inline_reaction_kind() == InlineReactionKind::None && self.payload_cell().is_none() {
                    if has_async_context {
                        if async_context.is_cell() {
                            self.set_inline_microtask_reaction(
                                task,
                                Some(async_context.as_cell()),
                                context,
                                INLINE_REACTION_ASYNC_CONTEXT_FLAG,
                            );
                            return;
                        }
                        // A non-cell context does not fit the payload cell; take a slim reaction.
                    } else {
                        self.set_inline_microtask_reaction(task, cell, context, 0);
                        return;
                    }
                }
                let existing = self.reaction_head();
                let reaction = if has_async_context {
                    JSSlimPromiseReaction::create_with_async_context(async_context, task, context, existing)
                } else {
                    JSSlimPromiseReaction::create_internal_microtask(cell_argument, task, context, existing)
                };
                self.set_packed_cell(self.flags() | IS_HANDLED_FLAG, Some(reaction.cell_id()));
            }
            Status::Rejected => {
                let settled = self.settlement_value();
                if !self.is_handled() {
                    host.promise_rejection_tracker(self, JSPromiseRejectionOperation::Handle);
                }
                queue_internal_microtask(host, task, Status::Rejected.payload(), &[cell_argument, settled, context, async_context]);
                self.mark_as_handled();
            }
            Status::Fulfilled => {
                let settled = self.settlement_value();
                queue_internal_microtask(host, task, Status::Fulfilled.payload(), &[cell_argument, settled, context, async_context]);
            }
        }
    }

    /// `isThenFastAndNonObservable()`.
    pub fn is_then_fast_and_non_observable(&self, host: &dyn PromiseHost) -> bool {
        if !host.promise_then_watchpoint_is_valid() {
            return false;
        }
        if Rc::ptr_eq(&self.structure(), &host.promise_structure()) {
            return true;
        }
        if self.get_prototype_direct() != host.promise_prototype() {
            return false;
        }
        let vm = host.vm();
        if self.get_direct_offset(vm, &PropertyName::from_identifier(&vm.property_names.then)) != INVALID_OFFSET {
            return false;
        }
        true
    }

    /// `settleInlineInternalMicrotask(vm, globalObject, newStatus, argument, flagsSnapshot)`.
    fn settle_inline_internal_microtask(&self, host: &dyn PromiseHost, new_status: Status, argument: JSValue, flags_snapshot: u16) {
        debug_assert!(
            flags_snapshot & INLINE_REACTION_KIND_MASK
                == (InlineReactionKind::InternalMicrotask as u16) << INLINE_REACTION_KIND_SHIFT
        );
        debug_assert!(flags_snapshot & IS_HANDLED_FLAG != 0);
        let task = microtask_of_flags(flags_snapshot);
        let context = self.slot.get();
        let mut cell_argument = cell_value(self.payload_cell());
        let mut async_context = JSValue::empty();
        if flags_snapshot & INLINE_REACTION_ASYNC_CONTEXT_FLAG != 0 {
            async_context = cell_argument;
            cell_argument = JSValue::undefined();
        }
        let settled_flags = (flags_snapshot
            & !(INLINE_REACTION_KIND_MASK | INLINE_REACTION_MICROTASK_MASK | INLINE_REACTION_ASYNC_CONTEXT_FLAG))
            | new_status as u16;
        self.set_slot(argument);
        self.set_packed_cell(settled_flags, None);
        queue_internal_microtask(host, task, new_status.payload(), &[cell_argument, argument, context, async_context]);
    }

    /// `settleInlineHandler(vm, globalObject, newStatus, argument, flagsSnapshot)`.
    fn settle_inline_handler(&self, host: &dyn PromiseHost, new_status: Status, argument: JSValue, flags_snapshot: u16) {
        debug_assert!(flags_snapshot & IS_HANDLED_FLAG != 0);
        let kind = (flags_snapshot & INLINE_REACTION_KIND_MASK) >> INLINE_REACTION_KIND_SHIFT;
        debug_assert!(kind == InlineReactionKind::FulfillHandler as u16 || kind == InlineReactionKind::RejectHandler as u16);
        let settled_is_fulfilled = new_status == Status::Fulfilled;
        let handler_is_fulfill = kind == InlineReactionKind::FulfillHandler as u16;
        let result_promise = cell_value(self.payload_cell());
        let handler = self.slot.get();
        let settled_flags =
            (flags_snapshot & !(INLINE_REACTION_KIND_MASK | INLINE_REACTION_MICROTASK_MASK)) | new_status as u16;
        self.set_slot(argument);
        self.set_packed_cell(settled_flags, None);
        if settled_is_fulfilled == handler_is_fulfill {
            host.queue_microtask(InternalMicrotask::PromiseReactionJob, new_status.payload(), &[result_promise, handler, argument]);
        } else {
            host.queue_microtask(
                InternalMicrotask::PromiseResolveWithoutHandlerJob,
                new_status.payload(),
                &[result_promise, argument, JSValue::undefined()],
            );
        }
    }

    /// `rejectPromise(vm, argument)`.
    pub fn reject_promise(&self, host: &dyn PromiseHost, argument: JSValue) {
        self.settle_promise(host, Status::Rejected, argument);
    }

    /// `fulfillPromise(vm, argument)`.
    pub fn fulfill_promise(&self, host: &dyn PromiseHost, argument: JSValue) {
        self.settle_promise(host, Status::Fulfilled, argument);
    }

    /// O corpo comum de `rejectPromise` e `fulfillPromise`: só a rejeição avisa o rastreador de
    /// rejeições (`PromiseRejectionTracker`) e só ela passa pelo `Reject` quando não há tratador.
    fn settle_promise(&self, host: &dyn PromiseHost, new_status: Status, argument: JSValue) {
        debug_assert!(self.status() == Status::Pending);
        let current_flags = self.flags();
        match self.inline_reaction_kind() {
            InlineReactionKind::InternalMicrotask => {
                self.settle_inline_internal_microtask(host, new_status, argument, current_flags)
            }
            InlineReactionKind::FulfillHandler | InlineReactionKind::RejectHandler => {
                self.settle_inline_handler(host, new_status, argument, current_flags)
            }
            InlineReactionKind::None => {
                let reactions = self.payload_cell().and_then(JSPromiseReactionRef::from_cell_id);
                let settled_flags = current_flags | new_status as u16;
                self.set_slot(argument);
                self.set_packed_cell(settled_flags, None);

                if new_status == Status::Rejected && !self.is_handled() {
                    host.promise_rejection_tracker(self, JSPromiseRejectionOperation::Reject);
                }

                if let Some(head) = reactions {
                    JSPromise::trigger_promise_reactions(host, new_status, head, argument);
                }
            }
        }
    }

    /// `resolvePromise(globalObject, vm, resolution)`.
    pub fn resolve_promise(&self, host: &dyn PromiseHost, resolution: JSValue) {
        let this_value = self.as_value();
        if resolution == this_value {
            let vm = host.vm();
            let message = WtfString::from_latin1(b"Cannot resolve a promise with itself");
            let error = ErrorInstance::create(vm, host.error_structure(ErrorType::TypeError), message.clone(), ErrorType::TypeError);
            crate::runtime::error_natives::put_message_property(vm, &error, &message);
            return self.reject_promise(host, error.as_value());
        }

        let Some(resolution_object) = JSObject::from_value(&resolution) else {
            return self.fulfill_promise(host, resolution);
        };

        if let Some(promise) = JSPromise::from_value(&resolution) {
            if promise.is_then_fast_and_non_observable(host) {
                return host.queue_microtask(
                    InternalMicrotask::PromiseResolveThenableJobFast,
                    0,
                    &[resolution, this_value, host.async_context()],
                );
            }
        }

        if is_definitely_non_thenable(&resolution_object, host) {
            return self.fulfill_promise(host, resolution);
        }

        let then = match host.get_property(resolution, PromiseProperty::Then) {
            Ok(then) => then,
            Err(Thrown::Value(error)) => return self.reject_promise(host, error),
            Err(Thrown::Termination) => return,
        };

        if !host.is_callable(then) {
            return self.fulfill_promise(host, resolution);
        }

        host.queue_microtask(
            InternalMicrotask::PromiseResolveThenableJob,
            0,
            &[resolution, then, this_value, host.async_context()],
        );
    }

    /// `triggerPromiseReactions(vm, globalObject, status, head, argument)`: as reações entram na fila na
    /// ordem em que foram registradas (a lista guarda a mais nova primeiro, então ela é invertida).
    fn trigger_promise_reactions(host: &dyn PromiseHost, status: Status, head: JSPromiseReactionRef, argument: JSValue) {
        if head.base().next().is_none() {
            queue_reaction(host, status, &head, argument);
            return;
        }

        // Reverse the order of singly-linked-list.
        let mut previous: Option<JSPromiseReactionRef> = None;
        let mut current = Some(head);
        while let Some(reaction) = current {
            let next = reaction.base().next();
            reaction.base().set_next(previous.take());
            previous = Some(reaction);
            current = next;
        }

        let mut current = previous;
        while let Some(reaction) = current {
            let next = reaction.base().next();
            queue_reaction(host, status, &reaction, argument);
            current = next;
        }
    }

    /// `resolveWithInternalMicrotaskForAsyncAwait(globalObject, vm, resolution, task, context)`.
    pub fn resolve_with_internal_microtask_for_async_await(
        host: &dyn PromiseHost,
        resolution: JSValue,
        task: InternalMicrotask,
        context: JSValue,
    ) {
        // The continuation resumes under the async context active at the await.
        let async_context = host.async_context();

        if let Some(promise) = JSPromise::from_value(&resolution) {
            if host.owns_promise(&promise) && host.promise_species_watchpoint_is_valid(&promise) {
                return promise.perform_promise_then_with_internal_microtask(host, task, None, context, async_context);
            }

            match host.get_property(resolution, PromiseProperty::Constructor) {
                Ok(constructor) => {
                    if constructor == host.promise_constructor() {
                        return promise.perform_promise_then_with_internal_microtask(host, task, None, context, async_context);
                    }
                }
                Err(Thrown::Value(error)) => {
                    let arguments = [JSValue::undefined(), error, context, async_context];
                    host.run_internal_microtask(task, Status::Rejected.payload(), arguments);
                    return;
                }
                Err(Thrown::Termination) => return,
            }
        }

        JSPromise::resolve_with_internal_microtask(host, resolution, task, context, async_context);
    }

    /// `resolveWithInternalMicrotask(globalObject, vm, resolution, task, context, asyncContext)`; o
    /// `asyncContext` padrão do C++ é `JSValue::empty()`.
    pub fn resolve_with_internal_microtask(
        host: &dyn PromiseHost,
        resolution: JSValue,
        task: InternalMicrotask,
        context: JSValue,
        async_context: JSValue,
    ) {
        let Some(resolution_object) = JSObject::from_value(&resolution) else {
            return JSPromise::fulfill_with_internal_microtask(host, resolution, task, context, async_context);
        };

        if let Some(promise) = JSPromise::from_value(&resolution) {
            if host.owns_promise(&promise) && promise.is_then_fast_and_non_observable(host) {
                return host.queue_microtask(
                    InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotaskFast,
                    task as u8,
                    &[resolution, context, async_context],
                );
            }
        }

        if is_definitely_non_thenable(&resolution_object, host) {
            return JSPromise::fulfill_with_internal_microtask(host, resolution, task, context, async_context);
        }

        let then = match host.get_property(resolution, PromiseProperty::Then) {
            Ok(then) => then,
            Err(Thrown::Value(error)) => {
                return JSPromise::reject_with_internal_microtask(host, error, task, context, async_context);
            }
            Err(Thrown::Termination) => return,
        };

        if !host.is_callable(then) {
            return JSPromise::fulfill_with_internal_microtask(host, resolution, task, context, async_context);
        }

        host.queue_microtask(
            InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotask,
            task as u8,
            &[resolution, then, context, async_context],
        );
    }

    /// `rejectWithInternalMicrotask(vm, globalObject, argument, task, context, asyncContext)`.
    pub fn reject_with_internal_microtask(
        host: &dyn PromiseHost,
        argument: JSValue,
        task: InternalMicrotask,
        context: JSValue,
        async_context: JSValue,
    ) {
        host.queue_microtask(task, Status::Rejected.payload(), &[JSValue::undefined(), argument, context, async_context]);
    }

    /// `fulfillWithInternalMicrotask(vm, globalObject, argument, task, context, asyncContext)`.
    pub fn fulfill_with_internal_microtask(
        host: &dyn PromiseHost,
        argument: JSValue,
        task: InternalMicrotask,
        context: JSValue,
        async_context: JSValue,
    ) {
        host.queue_microtask(task, Status::Fulfilled.payload(), &[JSValue::undefined(), argument, context, async_context]);
    }
}

/// `static_cast<InternalMicrotask>((flags & inlineReactionMicrotaskMask) >> inlineReactionMicrotaskShift)`.
fn microtask_of_flags(flags: u16) -> InternalMicrotask {
    let value = ((flags & INLINE_REACTION_MICROTASK_MASK) >> INLINE_REACTION_MICROTASK_SHIFT) as u8;
    InternalMicrotask::from_u8(value).expect("flags da promessa com InternalMicrotask inválida")
}

/// O lambda `queue` de `triggerPromiseReactions`: enfileira a tarefa de uma reação.
fn queue_reaction(host: &dyn PromiseHost, status: Status, reaction: &JSPromiseReactionRef, argument: JSValue) {
    let is_resolved = status == Status::Fulfilled;
    let promise = reaction.base().promise();
    let mut task = InternalMicrotask::PromiseReactionJob;
    let mut handler;
    let mut arg = argument;

    match reaction {
        JSPromiseReactionRef::Slim(slim) => {
            let internal_task = slim.internal_microtask();
            if internal_task != InternalMicrotask::None {
                handler = argument;
                arg = slim.handler_or_context();
                if slim.promise_slot_is_async_context() {
                    // `promise` is the captured async context; the task takes no cell.
                    queue_internal_microtask(host, internal_task, status.payload(), &[JSValue::undefined(), handler, arg, promise]);
                    return;
                }
                queue_internal_microtask(host, internal_task, status.payload(), &[promise, handler, arg]);
                return;
            } else if slim.is_fulfill_handler() == is_resolved {
                handler = slim.handler_or_context();
            } else {
                task = InternalMicrotask::PromiseResolveWithoutHandlerJob;
                handler = argument;
                arg = JSValue::undefined();
            }
        }
        JSPromiseReactionRef::Full(full) => {
            handler = if is_resolved { full.on_fulfilled() } else { full.on_rejected() };
            // performPromiseThen normalizes non-callable sides to jsUndefined() when storing
            // an async context in a full reaction; cheap tag check instead of isCallable().
            if handler.is_undefined() {
                task = InternalMicrotask::PromiseResolveWithoutHandlerJob;
                handler = argument;
                arg = JSValue::undefined();
            } else {
                let context = full.context();
                if full.context_is_async_context() {
                    host.queue_microtask(
                        task,
                        status.payload() | PROMISE_REACTION_JOB_ASYNC_CONTEXT_FLAG,
                        &[promise, handler, arg, context],
                    );
                    return;
                }
                if !context.is_undefined_or_null() {
                    host.queue_microtask(task, status.payload(), &[promise, handler, arg, context]);
                    return;
                }
            }
        }
    }

    host.queue_microtask(task, status.payload(), &[promise, handler, arg]);
}

/// `isDefinitelyNonThenable(object, globalObject)`: `true` se `object` não tem como ter `then` (sem
/// propriedades especiais na cadeia de protótipos, e com o `then` de `Object.prototype` ainda
/// intocado).
pub fn is_definitely_non_thenable(object: &JSObject, host: &dyn PromiseHost) -> bool {
    if !host.promise_then_watchpoint_is_valid() {
        return false;
    }

    let mut current = Some(object.structure());
    while let Some(structure) = current {
        if structure.has_special_properties()
            || structure.type_info().get_own_property_slot_is_impure_for_property_absence()
            || structure.type_info().overrides_get_prototype()
            || !structure.has_mono_proto()
        {
            return false;
        }
        // `storedPrototypeStructure()`: a `Structure` do protótipo, nula se o protótipo é `null`.
        current = JSObject::from_value(&structure.stored_prototype()).map(|prototype| prototype.structure());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_promise_host::{FunctionField, PromiseFunction};
    use std::cell::RefCell;

    /// Um host mínimo: as microtasks ficam gravadas, e são "chamáveis" os `Int32` a partir de 100.
    struct TestHost {
        vm: VM,
        structure: StructureRef,
        queued: RefCell<Vec<(InternalMicrotask, u8, Vec<JSValue>)>>,
        trackers: RefCell<Vec<JSPromiseRejectionOperation>>,
    }

    impl TestHost {
        fn new() -> TestHost {
            let vm = VM::new();
            let structure = JSPromise::create_structure(&vm, None, JSValue::null());
            TestHost { vm, structure, queued: RefCell::new(Vec::new()), trackers: RefCell::new(Vec::new()) }
        }

        fn promise(&self) -> JSPromiseRef {
            JSPromise::create(&self.vm, &self.structure)
        }
    }

    impl PromiseHost for TestHost {
        fn vm(&self) -> &VM {
            &self.vm
        }
        fn queue_microtask(&self, task: InternalMicrotask, payload: u8, arguments: &[JSValue]) {
            self.queued.borrow_mut().push((task, payload, arguments.to_vec()));
        }
        fn has_synchronous_module_queue(&self) -> bool {
            false
        }
        fn append_synchronous_module_task(&self, _task: SynchronousModuleTask) {
            unimplemented!()
        }
        fn promise_rejection_tracker(&self, _promise: &JSPromise, operation: JSPromiseRejectionOperation) {
            self.trackers.borrow_mut().push(operation);
        }
        fn async_context(&self) -> JSValue {
            JSValue::undefined()
        }
        fn is_callable(&self, value: JSValue) -> bool {
            matches!(value, JSValue::Int32(number) if number >= 100)
        }
        fn is_constructor(&self, _value: JSValue) -> bool {
            false
        }
        fn is_js_function(&self, _value: JSValue) -> bool {
            false
        }
        fn is_function_with_fields(&self, _value: JSValue) -> bool {
            false
        }
        fn owns_promise(&self, _promise: &JSPromise) -> bool {
            true
        }
        fn promise_structure(&self) -> StructureRef {
            Rc::clone(&self.structure)
        }
        fn promise_prototype(&self) -> JSValue {
            JSValue::null()
        }
        fn promise_constructor(&self) -> JSValue {
            JSValue::undefined()
        }
        fn promise_capability_object_structure(&self) -> StructureRef {
            unimplemented!()
        }
        fn promise_then_watchpoint_is_valid(&self) -> bool {
            true
        }
        fn promise_species_watchpoint_is_valid(&self, _promise: &JSPromise) -> bool {
            true
        }
        fn error_structure(&self, _error_type: ErrorType) -> StructureRef {
            unimplemented!()
        }
        fn get_property(&self, _object: JSValue, _property: PromiseProperty) -> Result<JSValue, Thrown> {
            unimplemented!()
        }
        fn construct(&self, _constructor: JSValue, _arguments: &[JSValue]) -> Result<JSValue, Thrown> {
            unimplemented!()
        }
        fn call(&self, _function: JSValue, _arguments: &[JSValue], _error_message: &str) -> Result<JSValue, Thrown> {
            unimplemented!()
        }
        fn create_type_error(&self, _message: &str) -> JSValue {
            unimplemented!()
        }
        fn create_function_with_fields(&self, _function: PromiseFunction) -> JSValue {
            unimplemented!()
        }
        fn function_field(&self, _function: JSValue, _field: FunctionField) -> JSValue {
            unimplemented!()
        }
        fn set_function_field(&self, _function: JSValue, _field: FunctionField, _value: JSValue) {
            unimplemented!()
        }
        fn create_internal_field_tuple(&self, _first: JSValue, _second: JSValue) -> JSValue {
            unimplemented!()
        }
        fn is_internal_field_tuple(&self, _value: JSValue) -> bool {
            false
        }
        fn run_internal_microtask(&self, _task: InternalMicrotask, _payload: u8, _arguments: [JSValue; 4]) {
            unimplemented!()
        }
    }

    #[test]
    fn settles_once_and_exposes_the_result() {
        let host = TestHost::new();
        let promise = host.promise();
        assert_eq!(promise.status(), Status::Pending);
        assert!(promise.result().is_undefined());
        promise.resolve(&host, JSValue::Int32(5));
        assert_eq!(promise.status(), Status::Fulfilled);
        assert_eq!(promise.result(), JSValue::Int32(5));
        // Só a primeira chamada vale.
        promise.reject(&host, JSValue::Int32(6));
        assert_eq!(promise.status(), Status::Fulfilled);
        assert!(host.trackers.borrow().is_empty());
    }

    #[test]
    fn inline_fulfill_handler_is_queued_on_fulfill() {
        let host = TestHost::new();
        let promise = host.promise();
        let result = host.promise();
        promise.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(1), result.as_value());
        assert_eq!(promise.inline_reaction_kind(), InlineReactionKind::FulfillHandler);
        assert!(promise.is_handled());
        promise.resolve(&host, JSValue::Int32(7));
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[0].1, Status::Fulfilled.payload());
        assert_eq!(queued[0].2, vec![result.as_value(), JSValue::Int32(100), JSValue::Int32(7)]);
    }

    #[test]
    fn rejection_without_handler_notifies_the_tracker_and_handle_later() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.reject(&host, JSValue::Int32(3));
        assert_eq!(promise.status(), Status::Rejected);
        assert_eq!(*host.trackers.borrow(), vec![JSPromiseRejectionOperation::Reject]);
        promise.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(101), JSValue::undefined());
        assert_eq!(*host.trackers.borrow(), vec![JSPromiseRejectionOperation::Reject, JSPromiseRejectionOperation::Handle]);
        let queued = host.queued.borrow();
        assert_eq!(queued[0].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[0].1, Status::Rejected.payload());
        assert!(promise.is_handled());
    }

    #[test]
    fn second_reaction_spills_the_inline_one_and_runs_in_registration_order() {
        let host = TestHost::new();
        let promise = host.promise();
        let first = host.promise();
        let second = host.promise();
        promise.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(1), first.as_value());
        promise.perform_promise_then(&host, JSValue::Int32(101), JSValue::Int32(1), second.as_value());
        assert_eq!(promise.inline_reaction_kind(), InlineReactionKind::None);
        promise.resolve(&host, JSValue::Int32(9));
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].2[0], first.as_value());
        assert_eq!(queued[1].2[0], second.as_value());
    }

    #[test]
    fn inline_internal_microtask_is_queued_on_settle() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.perform_promise_then_with_internal_microtask(
            &host,
            InternalMicrotask::AsyncFunctionResume,
            None,
            JSValue::Int32(11),
            JSValue::empty(),
        );
        assert_eq!(promise.inline_reaction_kind(), InlineReactionKind::InternalMicrotask);
        assert_eq!(promise.inline_reaction_microtask(), InternalMicrotask::AsyncFunctionResume);
        promise.fulfill(&host, JSValue::Int32(2));
        let queued = host.queued.borrow();
        assert_eq!(queued[0].0, InternalMicrotask::AsyncFunctionResume);
        assert_eq!(queued[0].2[1..3], [JSValue::Int32(2), JSValue::Int32(11)]);
    }

    #[test]
    fn pending_reactions_are_visible_to_for_each_pending_reaction() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.perform_promise_then_with_internal_microtask(
            &host,
            InternalMicrotask::AsyncFunctionResume,
            None,
            JSValue::Int32(11),
            JSValue::empty(),
        );
        let mut seen = Vec::new();
        promise.for_each_pending_reaction(&mut |task, _promise, context| {
            seen.push((task, context));
            true
        });
        assert_eq!(seen, vec![(InternalMicrotask::AsyncFunctionResume, JSValue::Int32(11))]);
        assert_eq!(promise.async_stack_trace_context(), JSValue::Int32(11));
    }

    #[test]
    fn rejecting_after_resolving_is_ignored() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.reject(&host, JSValue::Int32(3));
        promise.resolve(&host, JSValue::Int32(4));
        promise.fulfill(&host, JSValue::Int32(5));
        assert_eq!(promise.status(), Status::Rejected);
        assert_eq!(promise.result(), JSValue::Int32(3));
        assert!(promise.is_first_resolving_function_called());
    }

    #[test]
    fn reject_as_handled_never_reaches_the_tracker() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.reject_as_handled(&host, JSValue::Int32(8));
        assert_eq!(promise.status(), Status::Rejected);
        assert!(promise.is_handled());
        assert!(host.trackers.borrow().is_empty());
        // Segunda chamada não faz nada.
        promise.reject_as_handled(&host, JSValue::Int32(9));
        assert_eq!(promise.result(), JSValue::Int32(8));
    }

    #[test]
    fn rejection_of_a_promise_that_already_has_a_reaction_does_not_notify_the_tracker() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(101), host.promise().as_value());
        promise.reject(&host, JSValue::Int32(5));
        assert!(host.trackers.borrow().is_empty());
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[0].1, Status::Rejected.payload());
        assert_eq!(queued[0].2[1..], [JSValue::Int32(101), JSValue::Int32(5)]);
    }

    #[test]
    fn resolving_with_a_primitive_fulfills_without_queueing() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.resolve(&host, JSValue::Int32(1));
        assert_eq!(promise.status(), Status::Fulfilled);
        assert!(host.queued.borrow().is_empty());
    }

    #[test]
    fn resolving_with_a_fast_promise_queues_the_fast_thenable_job() {
        let host = TestHost::new();
        let target = host.promise();
        let thenable = host.promise();
        target.resolve(&host, thenable.as_value());
        // A promessa continua pendente: a tarefa é que a liga à `thenable`.
        assert_eq!(target.status(), Status::Pending);
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseResolveThenableJobFast);
        assert_eq!(queued[0].1, 0);
        assert_eq!(queued[0].2, vec![thenable.as_value(), target.as_value(), JSValue::undefined()]);
    }

    #[test]
    fn settled_promise_without_the_matching_handler_queues_resolve_without_handler() {
        let host = TestHost::new();
        let rejected = host.promise();
        rejected.reject(&host, JSValue::Int32(3));
        let result = host.promise();
        // Só o tratador de cumprimento: a rejeição passa adiante.
        rejected.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(1), result.as_value());
        let fulfilled = host.promise();
        fulfilled.resolve(&host, JSValue::Int32(4));
        fulfilled.perform_promise_then(&host, JSValue::Int32(1), JSValue::Int32(101), result.as_value());
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseResolveWithoutHandlerJob);
        assert_eq!(queued[0].1, Status::Rejected.payload());
        assert_eq!(queued[0].2, vec![result.as_value(), JSValue::Int32(3), JSValue::undefined()]);
        assert_eq!(queued[1].0, InternalMicrotask::PromiseResolveWithoutHandlerJob);
        assert_eq!(queued[1].1, Status::Fulfilled.payload());
        assert_eq!(queued[1].2, vec![result.as_value(), JSValue::Int32(4), JSValue::undefined()]);
    }

    #[test]
    fn inline_reject_handler_ignores_fulfillment_and_runs_on_rejection() {
        let host = TestHost::new();
        let on_fulfill = host.promise();
        let result = host.promise();
        on_fulfill.perform_promise_then(&host, JSValue::Int32(1), JSValue::Int32(100), result.as_value());
        assert_eq!(on_fulfill.inline_reaction_kind(), InlineReactionKind::RejectHandler);
        on_fulfill.resolve(&host, JSValue::Int32(7));
        let on_reject = host.promise();
        on_reject.perform_promise_then(&host, JSValue::Int32(1), JSValue::Int32(100), result.as_value());
        on_reject.reject(&host, JSValue::Int32(8));
        let queued = host.queued.borrow();
        assert_eq!(queued[0].0, InternalMicrotask::PromiseResolveWithoutHandlerJob);
        assert_eq!(queued[0].2, vec![result.as_value(), JSValue::Int32(7), JSValue::undefined()]);
        assert_eq!(queued[1].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[1].1, Status::Rejected.payload());
        assert_eq!(queued[1].2, vec![result.as_value(), JSValue::Int32(100), JSValue::Int32(8)]);
    }

    #[test]
    fn reaction_without_callable_handlers_passes_the_settlement_through() {
        let host = TestHost::new();
        let promise = host.promise();
        let result = host.promise();
        promise.perform_promise_then(&host, JSValue::Int32(1), JSValue::Int32(2), result.as_value());
        assert!(promise.is_handled());
        promise.resolve(&host, JSValue::Int32(4));
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseResolveWithoutHandlerJob);
        assert_eq!(queued[0].1, Status::Fulfilled.payload());
        assert_eq!(queued[0].2, vec![result.as_value(), JSValue::Int32(4), JSValue::undefined()]);
    }

    #[test]
    fn mixed_reaction_kinds_are_queued_in_registration_order() {
        let host = TestHost::new();
        let promise = host.promise();
        let first = host.promise();
        let third = host.promise();
        promise.perform_promise_then(&host, JSValue::Int32(100), JSValue::Int32(1), first.as_value());
        promise.perform_promise_then_with_internal_microtask(
            &host,
            InternalMicrotask::AsyncFunctionResume,
            None,
            JSValue::Int32(11),
            JSValue::empty(),
        );
        promise.perform_promise_then(&host, JSValue::Int32(102), JSValue::Int32(103), third.as_value());
        promise.resolve(&host, JSValue::Int32(9));
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 3);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[0].2, vec![first.as_value(), JSValue::Int32(100), JSValue::Int32(9)]);
        assert_eq!(queued[1].0, InternalMicrotask::AsyncFunctionResume);
        assert_eq!(queued[1].2[1..3], [JSValue::Int32(9), JSValue::Int32(11)]);
        assert_eq!(queued[2].0, InternalMicrotask::PromiseReactionJob);
        assert_eq!(queued[2].2, vec![third.as_value(), JSValue::Int32(102), JSValue::Int32(9)]);
    }

    #[test]
    fn pipe_from_forwards_the_settlement_through_an_internal_microtask() {
        let host = TestHost::new();
        let source = host.promise();
        let target = host.promise();
        target.pipe_from(&host, &source);
        assert!(target.is_first_resolving_function_called());
        assert_eq!(source.inline_reaction_kind(), InlineReactionKind::InternalMicrotask);
        assert_eq!(source.inline_reaction_microtask(), InternalMicrotask::PromiseFulfillWithoutHandlerJob);
        // Já foi chamada uma vez: a segunda não liga nada.
        let other = host.promise();
        target.pipe_from(&host, &other);
        assert_eq!(other.inline_reaction_kind(), InlineReactionKind::None);

        source.reject(&host, JSValue::Int32(2));
        let queued = host.queued.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, InternalMicrotask::PromiseFulfillWithoutHandlerJob);
        assert_eq!(queued[0].1, Status::Rejected.payload());
        assert_eq!(queued[0].2[0], target.as_value());
        assert_eq!(queued[0].2[1], JSValue::Int32(2));
        // A reação inline conta como tratador: nada de rastreador.
        assert!(host.trackers.borrow().is_empty());
    }

    #[test]
    fn internal_microtask_on_an_already_settled_promise_is_queued_at_once() {
        let host = TestHost::new();
        let promise = host.promise();
        promise.reject(&host, JSValue::Int32(6));
        promise.perform_promise_then_with_internal_microtask(
            &host,
            InternalMicrotask::AsyncFunctionResume,
            None,
            JSValue::Int32(12),
            JSValue::empty(),
        );
        assert_eq!(*host.trackers.borrow(), vec![JSPromiseRejectionOperation::Reject, JSPromiseRejectionOperation::Handle]);
        assert!(promise.is_handled());
        let queued = host.queued.borrow();
        assert_eq!(queued[0].0, InternalMicrotask::AsyncFunctionResume);
        assert_eq!(queued[0].1, Status::Rejected.payload());
        assert_eq!(queued[0].2[1..3], [JSValue::Int32(6), JSValue::Int32(12)]);
    }
}
