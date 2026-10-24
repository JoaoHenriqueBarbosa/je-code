//! Tradução do resto de `runtime/JSPromise.{h,cpp}`: a capacidade de promessa (`newPromiseCapability`,
//! `createNewPromiseCapability`, `createPromiseCapability`, `createDeferredData`), as funções de
//! resolução e seus corpos nativos (`promiseResolvingFunctionResolve`,
//! `promiseFirstResolvingFunctionResolve`, `promiseCapabilityExecutor` e companhia), `then`,
//! `promiseResolve`, `promiseReject`, `promiseSpeciesConstructor`, `resolvedPromise` e
//! `createPromiseCapabilityObjectStructure`. O estado e as reações da promessa estão em
//! `js_promise.rs`.
//!
//! As funções que o C++ declara como membros de `JSPromise` continuam sendo associadas a `JSPromise`
//! (um segundo bloco `impl` neste módulo); as livres do `.cpp` são livres aqui.
//!
//! DIVERGÊNCIAS:
//! - `JSFunctionWithFields` ainda não existe: a criação da função com campos e o acesso aos campos
//!   são `PromiseHost::create_function_with_fields`, `function_field` e `set_function_field`. Os
//!   corpos `JSC_DEFINE_HOST_FUNCTION` recebem o callee (`callFrame->jsCallee()`) e o argumento
//!   (`callFrame->argument(i)`) como `JSValue` e não devolvem nada (o C++ devolve `jsUndefined()`),
//!   salvo o `promise_capability_executor`, que pode lançar.
//! - `JSObject*` de promessa, de resolve e de reject é `JSValue`; `JSFunction*` do `DeferredData`
//!   também (a conferência `dynamicDowncast<JSFunction>` é `PromiseHost::is_js_function`).
//! - `createDeferredData` recebe o `JSPromiseConstructor*` como o `JSValue` do construtor
//!   (`newPromiseCapability` só o compara e o constrói).
//! - `promise->realm()->promiseConstructor()` de `promiseResolve` é o `promise_constructor()` do host
//!   (há um realm só).

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObject};
use crate::runtime::js_promise::{JSPromise, JSPromiseRef};
use crate::runtime::js_promise_host::{FunctionField, PromiseFunction, PromiseHost, PromiseProperty, Thrown};
use crate::runtime::js_promise_reaction::{JSPromiseReactionRef, JSSlimPromiseReaction, JSSlimPromiseReactionRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::InternalMicrotask;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::PropertyOffset;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `promiseCapabilityResolvePropertyOffset`.
pub const PROMISE_CAPABILITY_RESOLVE_PROPERTY_OFFSET: PropertyOffset = 0;
/// `promiseCapabilityRejectPropertyOffset`.
pub const PROMISE_CAPABILITY_REJECT_PROPERTY_OFFSET: PropertyOffset = 1;
/// `promiseCapabilityPromisePropertyOffset`.
pub const PROMISE_CAPABILITY_PROMISE_PROPERTY_OFFSET: PropertyOffset = 2;

/// O `std::tuple<JSObject*, JSObject*, JSObject*>` de `newPromiseCapability`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PromiseCapability {
    pub promise: JSValue,
    pub resolve: JSValue,
    pub reject: JSValue,
}

/// `JSPromise::DeferredData`.
#[derive(Clone, Debug)]
pub struct DeferredData {
    pub promise: JSPromiseRef,
    pub resolve: JSValue,
    pub reject: JSValue,
}

impl JSPromise {
    /// `createDeferredData(globalObject, promiseConstructor)`.
    pub fn create_deferred_data(host: &dyn PromiseHost, promise_constructor: JSValue) -> Result<DeferredData, Thrown> {
        let capability = JSPromise::new_promise_capability(host, promise_constructor)?;
        let promise = JSPromise::from_value(&capability.promise);
        if let Some(promise) = promise {
            if host.is_js_function(capability.resolve) && host.is_js_function(capability.reject) {
                return Ok(DeferredData { promise, resolve: capability.resolve, reject: capability.reject });
            }
        }
        Err(Thrown::Value(host.create_type_error("constructor is producing a bad value")))
    }

    /// `createNewPromiseCapability(globalObject, constructor)`: o objeto `{ resolve, reject, promise }`.
    pub fn create_new_promise_capability(host: &dyn PromiseHost, constructor: JSValue) -> Result<JSValue, Thrown> {
        let capability = JSPromise::new_promise_capability(host, constructor)?;
        Ok(JSPromise::create_promise_capability(host, capability.promise, capability.resolve, capability.reject))
    }

    /// `createPromiseCapability(vm, globalObject, promise, resolve, reject)`.
    pub fn create_promise_capability(host: &dyn PromiseHost, promise: JSValue, resolve: JSValue, reject: JSValue) -> JSValue {
        let vm = host.vm();
        let capability = JSFinalObject::create(vm, &host.promise_capability_object_structure());
        capability.put_direct_offset(vm, PROMISE_CAPABILITY_RESOLVE_PROPERTY_OFFSET, resolve);
        capability.put_direct_offset(vm, PROMISE_CAPABILITY_REJECT_PROPERTY_OFFSET, reject);
        capability.put_direct_offset(vm, PROMISE_CAPABILITY_PROMISE_PROPERTY_OFFSET, promise);
        capability.as_value()
    }

    /// `newPromiseCapability(globalObject, constructor)`.
    pub fn new_promise_capability(host: &dyn PromiseHost, constructor: JSValue) -> Result<PromiseCapability, Thrown> {
        if constructor == host.promise_constructor() {
            let promise = JSPromise::create(host.vm(), &host.promise_structure());
            let (resolve, reject) = promise.create_first_resolving_functions(host);
            return Ok(PromiseCapability { promise: promise.as_value(), resolve, reject });
        }

        let executor = host.create_function_with_fields(PromiseFunction::CapabilityExecutor);
        host.set_function_field(executor, FunctionField::EXECUTOR_RESOLVE, JSValue::undefined());
        host.set_function_field(executor, FunctionField::EXECUTOR_REJECT, JSValue::undefined());

        // `construct(..., "argument is not a constructor"_s)`.
        let new_object = host.construct(constructor, &[executor])?;

        let resolve = host.function_field(executor, FunctionField::EXECUTOR_RESOLVE);
        let reject = host.function_field(executor, FunctionField::EXECUTOR_REJECT);
        if !host.is_callable(resolve) {
            return Err(Thrown::Value(host.create_type_error("executor did not take a resolve function")));
        }

        if !host.is_callable(reject) {
            return Err(Thrown::Value(host.create_type_error("executor did not take a reject function")));
        }

        Ok(PromiseCapability { promise: new_object, resolve, reject })
    }

    /// `resolvedPromise(globalObject, value)`.
    pub fn resolved_promise(host: &dyn PromiseHost, value: JSValue) -> Result<JSPromiseRef, Thrown> {
        let promise = JSPromise::promise_resolve(host, host.promise_constructor(), value)?;
        Ok(JSPromise::from_value(&promise).expect("promiseResolve com o construtor intrínseco devolve um JSPromise"))
    }

    /// `createResolvingFunctions(vm, globalObject)`: o par (resolve, reject) da especificação, que
    /// compartilham o campo `Other` para o "já resolvido".
    pub fn create_resolving_functions(&self, host: &dyn PromiseHost) -> (JSValue, JSValue) {
        let resolve = host.create_function_with_fields(PromiseFunction::ResolvingFunctionResolve);
        let reject = host.create_function_with_fields(PromiseFunction::ResolvingFunctionReject);

        host.set_function_field(resolve, FunctionField::RESOLVING_PROMISE, self.as_value());
        host.set_function_field(resolve, FunctionField::RESOLVING_OTHER, reject);

        host.set_function_field(reject, FunctionField::RESOLVING_PROMISE, self.as_value());
        host.set_function_field(reject, FunctionField::RESOLVING_OTHER, resolve);

        (resolve, reject)
    }

    /// `createFirstResolveFunction(vm, globalObject)`.
    pub fn create_first_resolve_function(&self, host: &dyn PromiseHost) -> JSValue {
        let resolve = host.create_function_with_fields(PromiseFunction::FirstResolvingFunctionResolve);
        host.set_function_field(resolve, FunctionField::FIRST_RESOLVING_PROMISE, self.as_value());
        resolve
    }

    /// `createFirstRejectFunction(vm, globalObject)`.
    pub fn create_first_reject_function(&self, host: &dyn PromiseHost) -> JSValue {
        let reject = host.create_function_with_fields(PromiseFunction::FirstResolvingFunctionReject);
        host.set_function_field(reject, FunctionField::FIRST_RESOLVING_PROMISE, self.as_value());
        reject
    }

    /// `createFirstResolvingFunctions(vm, globalObject)`.
    pub fn create_first_resolving_functions(&self, host: &dyn PromiseHost) -> (JSValue, JSValue) {
        (self.create_first_resolve_function(host), self.create_first_reject_function(host))
    }

    /// `createResolvingFunctionsWithInternalMicrotask(vm, globalObject, task, context, asyncContext)`; o
    /// `asyncContext` padrão do C++ é `JSValue::empty()`.
    pub fn create_resolving_functions_with_internal_microtask(
        host: &dyn PromiseHost,
        task: InternalMicrotask,
        context: JSValue,
        async_context: JSValue,
    ) -> (JSValue, JSValue) {
        let resolve = host.create_function_with_fields(PromiseFunction::ResolvingFunctionResolveWithInternalMicrotask);
        let reject = host.create_function_with_fields(PromiseFunction::ResolvingFunctionRejectWithInternalMicrotask);

        let context_cell = if !async_context.is_empty() && !async_context.is_undefined() {
            JSSlimPromiseReaction::create_with_async_context(async_context, task, context, None)
        } else {
            JSSlimPromiseReaction::create_internal_microtask(JSValue::undefined(), task, context, None)
        };

        let context_value = context_cell.as_value();
        host.set_function_field(resolve, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_CONTEXT, context_value);
        host.set_function_field(resolve, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_OTHER, reject);

        host.set_function_field(reject, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_CONTEXT, context_value);
        host.set_function_field(reject, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_OTHER, resolve);

        (resolve, reject)
    }

    /// `then(globalObject, onFulfilled, onRejected)`: devolve a promessa de resultado.
    pub fn then(&self, host: &dyn PromiseHost, on_fulfilled: JSValue, on_rejected: JSValue) -> Result<JSValue, Thrown> {
        let result_promise;
        let result_promise_capability;
        if host.promise_species_watchpoint_is_valid(self) {
            let promise = JSPromise::create(host.vm(), &host.promise_structure());
            result_promise = promise.as_value();
            result_promise_capability = result_promise;
        } else {
            let constructor = promise_species_constructor(host, self.as_value())?;
            let capability = JSPromise::new_promise_capability(host, constructor)?;
            result_promise = capability.promise;
            result_promise_capability =
                JSPromise::create_promise_capability(host, capability.promise, capability.resolve, capability.reject);
        }

        self.perform_promise_then(host, on_fulfilled, on_rejected, result_promise_capability);
        Ok(result_promise)
    }

    /// `promiseResolve(globalObject, constructor, argument)`.
    pub fn promise_resolve(host: &dyn PromiseHost, constructor: JSValue, argument: JSValue) -> Result<JSValue, Thrown> {
        if let Some(promise) = JSPromise::from_value(&argument) {
            if host.promise_species_watchpoint_is_valid(&promise) {
                if constructor == host.promise_constructor() {
                    return Ok(argument);
                }
            } else {
                let property = host.get_property(argument, PromiseProperty::Constructor)?;
                if property == constructor {
                    return Ok(argument);
                }
            }
        }

        if constructor == host.promise_constructor() {
            let promise = JSPromise::create(host.vm(), &host.promise_structure());
            promise.resolve(host, argument);
            return Ok(promise.as_value());
        }

        let capability = JSPromise::new_promise_capability(host, constructor)?;
        host.call(capability.resolve, &[argument], "resolve is not a function")?;
        Ok(capability.promise)
    }

    /// `promiseReject(globalObject, constructor, argument)`.
    pub fn promise_reject(host: &dyn PromiseHost, constructor: JSValue, argument: JSValue) -> Result<JSValue, Thrown> {
        if constructor == host.promise_constructor() {
            let promise = JSPromise::create(host.vm(), &host.promise_structure());
            promise.reject(host, argument);
            return Ok(promise.as_value());
        }

        let capability = JSPromise::new_promise_capability(host, constructor)?;
        host.call(capability.reject, &[argument], "reject is not a function")?;
        Ok(capability.promise)
    }
}

/// `promiseSpeciesConstructor(globalObject, thisObject)`.
pub fn promise_species_constructor(host: &dyn PromiseHost, this_object: JSValue) -> Result<JSValue, Thrown> {
    if let Some(promise) = JSPromise::from_value(&this_object) {
        if host.promise_species_watchpoint_is_valid(&promise) {
            return Ok(host.promise_constructor());
        }
    }

    let constructor = host.get_property(this_object, PromiseProperty::Constructor)?;

    if constructor.is_undefined() {
        return Ok(host.promise_constructor());
    }

    if JSObject::from_value(&constructor).is_none() {
        return Err(Thrown::Value(host.create_type_error("|this|.constructor is not an Object or undefined")));
    }

    let constructor = host.get_property(constructor, PromiseProperty::Species)?;

    if constructor.is_undefined_or_null() {
        return Ok(host.promise_constructor());
    }

    if host.is_constructor(constructor) {
        return Ok(constructor);
    }

    Err(Thrown::Value(host.create_type_error("|this|.constructor[Symbol.species] is not a constructor")))
}

/// `createPromiseCapabilityObjectStructure(vm, globalObject)`: a `Structure` de `{ resolve, reject,
/// promise }`, com os offsets que as constantes `promiseCapability*PropertyOffset` fixam.
pub fn create_promise_capability_object_structure(vm: &VM, global_object: &JSGlobalObject) -> StructureRef {
    let mut structure = global_object.structure_cache().empty_object_structure_for_prototype(
        global_object,
        &global_object.object_prototype(),
        JSFinalObject::DEFAULT_INLINE_CAPACITY,
        false,
    );
    let properties = [
        (&vm.property_names.resolve, PROMISE_CAPABILITY_RESOLVE_PROPERTY_OFFSET),
        (&vm.property_names.reject, PROMISE_CAPABILITY_REJECT_PROPERTY_OFFSET),
        // `vm.propertyNames->promise` (minúsculo); `property_names.promise` é o `Promise` do construtor.
        (&vm.property_names.promise_dup, PROMISE_CAPABILITY_PROMISE_PROPERTY_OFFSET),
    ];
    for (identifier, expected_offset) in properties {
        let (next, offset) = Structure::add_property_transition(vm, &structure, &PropertyName::from_identifier(identifier), 0);
        assert_eq!(offset, expected_offset);
        structure = next;
    }
    structure
}

/// `JSFunctionWithFields::ResolvingOther`/`ResolvingWithInternalMicrotaskOther`: os dois lados de um
/// par de funções de resolução compartilham um campo que a primeira chamada zera nos dois.
/// Devolve o outro lado, ou `None` se ele já foi consumido (`dynamicDowncast<JSFunctionWithFields>`
/// falha para o `jsNull()` que fica no lugar).
fn take_other_function(host: &dyn PromiseHost, callee: JSValue, field: FunctionField) -> Option<JSValue> {
    let other = host.function_field(callee, field);
    if !host.is_function_with_fields(other) {
        return None;
    }
    host.set_function_field(callee, field, JSValue::null());
    host.set_function_field(other, field, JSValue::null());
    Some(other)
}

/// `promiseResolvingFunctionResolve(globalObject, callFrame)`.
pub fn promise_resolving_function_resolve(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    if take_other_function(host, callee, FunctionField::RESOLVING_OTHER).is_none() {
        return;
    }
    let promise = JSPromise::from_value(&host.function_field(callee, FunctionField::RESOLVING_PROMISE))
        .expect("função de resolução sem promessa");
    promise.resolve_promise(host, argument);
}

/// `promiseResolvingFunctionReject(globalObject, callFrame)`.
pub fn promise_resolving_function_reject(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    if take_other_function(host, callee, FunctionField::RESOLVING_OTHER).is_none() {
        return;
    }
    let promise = JSPromise::from_value(&host.function_field(callee, FunctionField::RESOLVING_PROMISE))
        .expect("função de rejeição sem promessa");
    promise.reject_promise(host, argument);
}

/// `promiseFirstResolvingFunctionResolve(globalObject, callFrame)`.
pub fn promise_first_resolving_function_resolve(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    let promise = JSPromise::from_value(&host.function_field(callee, FunctionField::FIRST_RESOLVING_PROMISE))
        .expect("função de resolução sem promessa");
    promise.resolve(host, argument);
}

/// `promiseFirstResolvingFunctionReject(globalObject, callFrame)`.
pub fn promise_first_resolving_function_reject(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    let promise = JSPromise::from_value(&host.function_field(callee, FunctionField::FIRST_RESOLVING_PROMISE))
        .expect("função de rejeição sem promessa");
    promise.reject(host, argument);
}

/// A célula `JSSlimPromiseReaction` que guarda a tarefa e o contexto de um par de funções de
/// resolução com microtask interna.
fn internal_microtask_context_cell(host: &dyn PromiseHost, callee: JSValue) -> JSSlimPromiseReactionRef {
    match JSPromiseReactionRef::from_value(&host.function_field(callee, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_CONTEXT)) {
        Some(JSPromiseReactionRef::Slim(slim)) => slim,
        _ => panic!("função de resolução com microtask interna sem célula de contexto"),
    }
}

/// `promiseResolvingFunctionResolveWithInternalMicrotask(globalObject, callFrame)`.
pub fn promise_resolving_function_resolve_with_internal_microtask(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    if take_other_function(host, callee, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_OTHER).is_none() {
        return;
    }
    let context_cell = internal_microtask_context_cell(host, callee);
    let async_context = if context_cell.promise_slot_is_async_context() { context_cell.promise() } else { JSValue::empty() };
    JSPromise::resolve_with_internal_microtask(
        host,
        argument,
        context_cell.internal_microtask(),
        context_cell.handler_or_context(),
        async_context,
    );
}

/// `promiseResolvingFunctionRejectWithInternalMicrotask(globalObject, callFrame)`.
pub fn promise_resolving_function_reject_with_internal_microtask(host: &dyn PromiseHost, callee: JSValue, argument: JSValue) {
    if take_other_function(host, callee, FunctionField::RESOLVING_WITH_INTERNAL_MICROTASK_OTHER).is_none() {
        return;
    }
    let context_cell = internal_microtask_context_cell(host, callee);
    let async_context = if context_cell.promise_slot_is_async_context() { context_cell.promise() } else { JSValue::empty() };
    JSPromise::reject_with_internal_microtask(
        host,
        argument,
        context_cell.internal_microtask(),
        context_cell.handler_or_context(),
        async_context,
    );
}

/// `promiseCapabilityExecutor(globalObject, callFrame)`: `resolve` e `reject` são `argument(0)` e
/// `argument(1)`. Lança `TypeError` se o executor já foi chamado com função de resolução.
pub fn promise_capability_executor(host: &dyn PromiseHost, callee: JSValue, resolve: JSValue, reject: JSValue) -> Result<(), Thrown> {
    if !host.function_field(callee, FunctionField::EXECUTOR_RESOLVE).is_undefined() {
        return Err(Thrown::Value(host.create_type_error("resolve function is already set")));
    }

    if !host.function_field(callee, FunctionField::EXECUTOR_REJECT).is_undefined() {
        return Err(Thrown::Value(host.create_type_error("reject function is already set")));
    }

    host.set_function_field(callee, FunctionField::EXECUTOR_RESOLVE, resolve);
    host.set_function_field(callee, FunctionField::EXECUTOR_REJECT, reject);
    Ok(())
}
