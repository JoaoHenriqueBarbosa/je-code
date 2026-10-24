//! Porte de `runtime/JSPromiseConstructor.{h,cpp}` e do que `JSPromise.cpp`, `JSMicrotask.cpp` e
//! `JSGlobalObject.cpp` pedem do global: o construtor `Promise` (um `JSFunction` sobre o builtin
//! `PromiseConstructor.js`, com `resolve`, `reject`, `race`, `all`, `allSettled`, `any`,
//! `withResolvers`, `try` e os privados `@resolve`/`@reject` do Bun), as funções de
//! elemento de `all`/`allSettled`/`any`, as estruturas e criadores de resultado do `allSettled`, o
//! `AggregateError` do `any`, os corpos nativos das funções de resolução de `js_promise_capability.rs`,
//! o `impl PromiseHost for JSGlobalObject` (inclusive o `runInternalMicrotask`, com os corpos em
//! `js_microtask.rs`) e `JSGlobalObject::init_promise`, que monta tudo no global.
//!
//! `Promise.prototype` está em `promise_prototype.rs`, as funções globais de `@resolvePromise` e
//! companhia em `promise_global_functions.rs` e a fila de microtasks em `microtask_queue.rs`.
//!
//! LACUNAS, e por quê:
//! - As tarefas de async generator e carregador de módulos (`runInternalMicrotask`) e o
//!   `InternalFieldTuple` (só nasce com contexto assíncrono, que não existe) respondem `panic!` com o nome
//!   do que falta (ver `js_microtask.rs`).
//! - `JSValue::get` de primitivo é `Unported` (`get_value_property`).
//!
//! DIVERGÊNCIAS:
//! - Os conjuntos de watchpoint (`promiseThenWatchpointSet`, `promiseResolveWatchpointSet`,
//!   `promiseSpeciesWatchpointSet`) não existem: `promise_then_watchpoint_is_valid` e
//!   `promise_species_watchpoint_is_valid` conferem direto as propriedades que o C++ vigia
//!   (`Promise.prototype.then` e a ausência de `then` em `Object.prototype`; `Promise.prototype.constructor`
//!   e `Promise[@@species]`), como `array_prototype.rs` faz com o `Array`. O que o C++ vigia e a checagem
//!   direta enxergam são os mesmos valores.
//! - `promiseRejectionTracker` (`GlobalObjectMethodTable`) é o `JSGlobalObject::promiseRejectionTracker`
//!   padrão do C++ (`Reject` chama `vm.promiseRejected`, `Handle` não faz nada), sem tabela de métodos
//!   substituível; a fila `m_aboutToBeNotifiedRejectedPromises` e o `didExhaustMicrotaskQueue` estão em
//!   `microtask_queue.rs`. `m_unhandledRejectionCallback` é o campo `unhandled_rejection_callback` de
//!   `PromiseGlobalData`.
//! - `Promise[@@species]` (`promiseSpeciesGetterSetter()`) é criado pelo `JSPromiseConstructor::create`
//!   (o C++ o cria no `JSGlobalObject::init` e o passa) e guardado em `PromiseGlobalData`, que é o que
//!   `promise_species_watchpoint_is_valid` compara.
//! - `m_synchronousModuleQueue` (VM) é o campo `synchronous_module_queue` do global (há um global só).
//! - `arrayStructureForIndexingTypeDuringAllocation(ArrayWithContiguous)` é a `arrayStructure()` do global
//!   (a única que o porte tem, `ArrayWithUndecided`): o array transiciona sozinho no primeiro elemento.
//! - O `NativeExecutable` de cada `JSFunctionWithFields` (`vm.promise...Executable()`) é o
//!   `get_host_function` do `VM`, que já o compartilha por função nativa.
//! - O `JSPromiseConstructor` usa a `Structure` de `JSFunction` (o `ClassInfo` próprio não é observável).

use std::rc::Rc;

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::host_function;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::call_data::{call_with_error_message, construct_with_error_message, get_construct_data};
use crate::runtime::collection_support::put_species_accessor;
use crate::runtime::error::create_type_error;
use crate::runtime::aggregate_error;
use crate::runtime::error_type::ErrorType;
use crate::runtime::exception_helpers::throw_out_of_memory_error;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown as HostThrown};
use crate::runtime::proxy_object::to_this_strict;
use crate::runtime::stack_frame::StackFrame;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::{call_checked, for_each_in_iterable, get_value_property};
use crate::runtime::js_array::JSArray;
use crate::runtime::js_function::{
    put_direct_builtin_function_without_transition,
    put_direct_native_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::js_function_with_fields::JSFunctionWithFields;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask;
use crate::runtime::js_object::{JSFinalObject, JSObject, MAX_STORAGE_VECTOR_LENGTH};
use crate::runtime::js_promise::{is_definitely_non_thenable, JSPromise, JSPromiseRef, Status};
use crate::runtime::js_promise_capability::{
    create_promise_capability_object_structure, promise_capability_executor, promise_first_resolving_function_reject,
    promise_first_resolving_function_resolve, promise_resolving_function_reject,
    promise_resolving_function_reject_with_internal_microtask, promise_resolving_function_resolve,
    promise_resolving_function_resolve_with_internal_microtask, promise_species_constructor, PromiseCapability,
};
use crate::runtime::js_promise_combinators_context::{
    JSPromiseCombinatorsContext, JSPromiseCombinatorsContextRef, JSPromiseCombinatorsGlobalContext,
    JSPromiseCombinatorsGlobalContextRef,
};
use crate::runtime::js_promise_host::{
    caught_exception, get_property_named, host_result, thrown_from_llint_failure, FunctionField, JSPromiseRejectionOperation,
    PromiseFunction, PromiseHost, PromiseProperty, Thrown as PromiseThrown,
};
use crate::runtime::js_scope::JSScope;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::microtask::{InternalMicrotask, SynchronousModuleTask};
use crate::runtime::microtask_queue::QueuedTask;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::options_list::Options;
use crate::runtime::promise_prototype::{create_default_promise_then, JSPromisePrototype};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_function::JS_FUNCTION_S_INFO;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry_with_intrinsic};
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, FUNCTION, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::{PropertyOffset, INVALID_OFFSET};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::throw_scope::ThrowScope;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `promiseAllSettledStatusPropertyOffset`.
const PROMISE_ALL_SETTLED_STATUS_PROPERTY_OFFSET: PropertyOffset = 0;
/// `promiseAllSettledValuePropertyOffset` e `promiseAllSettledReasonPropertyOffset`.
const PROMISE_ALL_SETTLED_VALUE_PROPERTY_OFFSET: PropertyOffset = 1;

/// O estado de `Promise` que o `JSGlobalObject` guarda (`m_promiseStructure`, `m_promisePrototype`,
/// `m_promiseConstructor`, `m_promiseCapabilityObjectStructure`, os dois de `allSettled`,
/// `m_promiseProtoThenFunction` e o `m_synchronousModuleQueue` do `VM`).
/// Preenchido por `JSGlobalObject::init_promise`.
#[derive(Debug, Default)]
pub struct PromiseGlobalData {
    pub(crate) structure: Option<StructureRef>,
    pub(crate) prototype: Option<JSValue>,
    pub(crate) constructor: Option<JSValue>,
    pub(crate) capability_object_structure: Option<StructureRef>,
    pub(crate) all_settled_fulfilled_result_structure: Option<StructureRef>,
    pub(crate) all_settled_rejected_result_structure: Option<StructureRef>,
    /// `promiseProtoThenFunction()`: a função `then` original de `Promise.prototype`.
    pub(crate) then_function: Option<JSValue>,
    /// A função `Promise.resolve` original, o que o `promiseResolveWatchpointSet` vigia.
    pub(crate) resolve_function: Option<JSValue>,
    /// `promiseSpeciesGetterSetter()`: o `GetterSetter` de `Promise[@@species]`.
    pub(crate) species_getter_setter: Option<JSValue>,
    /// `m_unhandledRejectionCallback`: `None` é o `nullptr`.
    pub(crate) unhandled_rejection_callback: Option<JSValue>,
    /// `vm.m_synchronousModuleQueue`: `None` é o `nullptr`.
    pub(crate) synchronous_module_queue: Option<Vec<SynchronousModuleTask>>,
}

/// `uncheckedDowncast<JSPromise>(value)`.
pub(crate) fn promise_of(value: &JSValue) -> JSPromiseRef {
    JSPromise::from_value(value).expect("uncheckedDowncast<JSPromise> em valor que não é promessa")
}

impl JSGlobalObject {
    /// `promisePrototype()` etc. como valor guardado em `promise_data`; invariante do `init_promise`.
    fn promise_datum(&self, pick: impl FnOnce(&PromiseGlobalData) -> Option<JSValue>, what: &str) -> JSValue {
        pick(&self.promise_data.borrow()).unwrap_or_else(|| panic!("JSGlobalObject sem {what}"))
    }

    /// `promiseProtoThenFunction()`.
    pub fn promise_proto_then_function(&self) -> JSValue {
        self.promise_datum(|data| data.then_function, "promiseProtoThenFunction")
    }

    /// `unhandledRejectionCallback()`: `None` é o `nullptr`.
    pub fn unhandled_rejection_callback(&self) -> Option<JSValue> {
        self.promise_data.borrow().unhandled_rejection_callback
    }

    /// `m_unhandledRejectionCallback.set(vm, this, callback)`.
    pub fn set_unhandled_rejection_callback(&self, callback: Option<JSValue>) {
        self.promise_data.borrow_mut().unhandled_rejection_callback = callback;
    }

    /// `promiseAllSettledFulfilledResultStructure()`.
    pub fn promise_all_settled_fulfilled_result_structure(&self) -> StructureRef {
        self.promise_data
            .borrow()
            .all_settled_fulfilled_result_structure
            .clone()
            .expect("JSGlobalObject sem promiseAllSettledFulfilledResultStructure")
    }

    /// `promiseAllSettledRejectedResultStructure()`.
    pub fn promise_all_settled_rejected_result_structure(&self) -> StructureRef {
        self.promise_data
            .borrow()
            .all_settled_rejected_result_structure
            .clone()
            .expect("JSGlobalObject sem promiseAllSettledRejectedResultStructure")
    }

    /// `promiseResolveWatchpointSet().isStillValid()`: `Promise.resolve` ainda é a função original.
    fn promise_resolve_watchpoint_is_valid(&self) -> bool {
        let (constructor, resolve_function) = {
            let data = self.promise_data.borrow();
            (data.constructor, data.resolve_function)
        };
        let (Some(constructor), Some(resolve_function)) = (constructor, resolve_function) else { return false };
        let Some(constructor) = constructor.as_js_function() else { return false };
        constructor.get_direct_by_name(self.vm(), &PropertyName::from_identifier(&self.vm().property_names.resolve))
            == resolve_function
    }
}

// Os métodos deste impl são itens do trait `PromiseHost`: quem chama de fora importa o trait
// (`use crate::runtime::js_promise_host::PromiseHost;`); `pub(crate)` aqui não compila.
impl PromiseHost for JSGlobalObject {
    fn vm(&self) -> &VM {
        JSGlobalObject::vm(self)
    }

    fn queue_microtask(&self, task: InternalMicrotask, payload: u8, arguments: &[JSValue]) {
        self.vm().default_microtask_queue.enqueue(QueuedTask::new(self.cell_id(), task, payload, arguments));
    }

    fn has_synchronous_module_queue(&self) -> bool {
        self.promise_data.borrow().synchronous_module_queue.is_some()
    }

    fn append_synchronous_module_task(&self, task: SynchronousModuleTask) {
        self.promise_data
            .borrow_mut()
            .synchronous_module_queue
            .as_mut()
            .expect("vm.m_synchronousModuleQueue->tasks.append sem a fila")
            .push(task);
    }

    /// `JSGlobalObject::promiseRejectionTracker` (ver o cabeçalho).
    fn promise_rejection_tracker(&self, promise: &JSPromise, operation: JSPromiseRejectionOperation) {
        match operation {
            // Handler novo numa rejeição que já foi entregue a `unhandledRejection`: emite `rejectionHandled` (ou
            // entrega ao embedder que ouve todo tratador novo, o navegador).
            JSPromiseRejectionOperation::Handle => match self.vm().rejection_handled_reporter() {
                Some(reporter) => reporter(self, JSValue::Cell(promise.cell_id())),
                None => crate::runtime::process_exit::promise_handled(self, promise.cell_id()),
            },
            JSPromiseRejectionOperation::Reject => {
                let promise = JSPromise::from_cell_id(promise.cell_id()).expect("a promessa do rastreador está no registro de células");
                self.vm().promise_rejected(promise);
            }
        }
    }

    /// `AsyncContextSwapScope::current(vm, globalObject)`.
    fn async_context(&self) -> JSValue {
        self.vm().async_context()
    }

    fn is_callable(&self, value: JSValue) -> bool {
        value.is_callable()
    }

    fn is_constructor(&self, value: JSValue) -> bool {
        !get_construct_data(value).is_none()
    }

    fn is_js_function(&self, value: JSValue) -> bool {
        value.as_js_function().is_some()
    }

    fn is_function_with_fields(&self, value: JSValue) -> bool {
        JSFunctionWithFields::from_value(&value).is_some()
    }

    fn owns_promise(&self, promise: &JSPromise) -> bool {
        promise.realm().is_some_and(|realm| std::ptr::eq(&*realm, self))
    }

    /// `runInternalMicrotask(globalObject, vm, task, payload, arguments)` (`JSMicrotask.cpp`).
    fn run_internal_microtask(&self, task: InternalMicrotask, payload: u8, arguments: [JSValue; 4]) {
        let result = match task {
            InternalMicrotask::None => unreachable!("RELEASE_ASSERT_NOT_REACHED: InternalMicrotask::None"),
            InternalMicrotask::PromiseResolveThenableJobFast => js_microtask::promise_resolve_thenable_job_fast(self, arguments),
            InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotaskFast => {
                js_microtask::promise_resolve_thenable_job_with_internal_microtask_fast(self, payload, arguments)
            }
            InternalMicrotask::PromiseResolveThenableJob => js_microtask::promise_resolve_thenable_job_task(self, arguments),
            InternalMicrotask::PromiseResolveThenableJobWithInternalMicrotask => {
                js_microtask::promise_resolve_thenable_job_with_internal_microtask(self, payload, arguments)
            }
            InternalMicrotask::PromiseResolveWithoutHandlerJob => {
                js_microtask::promise_resolve_without_handler_job(self, payload, arguments)
            }
            InternalMicrotask::PromiseFulfillWithoutHandlerJob => {
                js_microtask::promise_fulfill_without_handler_job(self, payload, arguments);
                Ok(())
            }
            InternalMicrotask::PromiseRaceResolveJob => {
                js_microtask::promise_race_resolve_job(self, payload, arguments);
                Ok(())
            }
            InternalMicrotask::PromiseAllResolveJob => js_microtask::promise_all_resolve_job(self, payload, arguments),
            InternalMicrotask::PromiseAllSettledResolveJob => js_microtask::promise_all_settled_resolve_job(self, payload, arguments),
            InternalMicrotask::PromiseAnyResolveJob => js_microtask::promise_any_resolve_job(self, payload, arguments),
            InternalMicrotask::PromiseReactionJob => js_microtask::promise_reaction_job(self, payload, arguments),
            InternalMicrotask::InvokeFunctionJob => js_microtask::invoke_function_job(self, arguments),
            InternalMicrotask::PromiseFinallyReactionJob => js_microtask::promise_finally_reaction_job(self, payload, arguments),
            InternalMicrotask::PromiseFinallyAwaitJob => {
                js_microtask::promise_finally_await_job(self, payload, arguments);
                Ok(())
            }
            InternalMicrotask::BunPerformMicrotaskJob => js_microtask::bun_perform_microtask_job(self, arguments),
            InternalMicrotask::BunInvokeJobWithArguments => js_microtask::bun_invoke_job_with_arguments(self, arguments),
            InternalMicrotask::AsyncFunctionResume => js_microtask::async_function_resume(payload, arguments),
            InternalMicrotask::AsyncGeneratorDriverResume => js_microtask::async_generator_driver_resume(payload, arguments),
            InternalMicrotask::AsyncFromSyncIteratorContinue
            | InternalMicrotask::AsyncFromSyncIteratorDone
            | InternalMicrotask::AsyncGeneratorYieldAwaited
            | InternalMicrotask::AsyncGeneratorBodyCallNormal
            | InternalMicrotask::AsyncGeneratorBodyCallReturn
            | InternalMicrotask::AsyncGeneratorAwaitReturn => {
                crate::runtime::js_microtask_async::run_async_internal_microtask(self, task, payload, arguments);
                Ok(())
            }
            InternalMicrotask::AsyncModuleExecutionResume => js_microtask::async_module_execution_resume(payload, arguments),
            InternalMicrotask::AsyncModuleExecutionDone => js_microtask::async_module_execution_done(payload, arguments),
            InternalMicrotask::DynamicImportEvaluateSettled => js_microtask::dynamic_import_evaluate_settled(self, payload, arguments),
            InternalMicrotask::DynamicImportDeferDependencySettled => {
                js_microtask::dynamic_import_defer_dependency_settled(self, payload, arguments)
            }
            InternalMicrotask::ImportModuleNamespace => js_microtask::import_module_namespace(self, payload, arguments),
            InternalMicrotask::DynamicImportLoadSettled => {
                crate::runtime::js_module_loader::dynamic_import_load_settled(self, arguments);
                Ok(())
            }
            InternalMicrotask::ModuleRegistryFetchSettled
            | InternalMicrotask::ModuleRegistryModuleSettled
            | InternalMicrotask::ModuleGraphLoadingError
            | InternalMicrotask::ModuleLoadStep
            | InternalMicrotask::ModuleLoadTopSettled
            | InternalMicrotask::ModuleLoadTopRejected
            | InternalMicrotask::ModuleLoadSpecifierTransform
            | InternalMicrotask::ModuleLoadCombinedLoadSettled
            | InternalMicrotask::ModuleLoadCombinedStateSettled
            | InternalMicrotask::ModuleLoadLinkEvaluateSettled
            | InternalMicrotask::ModuleLoadReturnRecord
            | InternalMicrotask::ModuleLoadReturnModuleKey
            | InternalMicrotask::ModuleLoadStoreError
            | InternalMicrotask::DynamicImportDeferLoadSettled => {
                panic!("{task:?} ainda não portado: o carregador de módulos assíncrono (o do porte é síncrono)")
            }
            InternalMicrotask::WebAssemblyCompileStreaming | InternalMicrotask::WebAssemblyInstantiateStreaming => {
                panic!("{task:?} ainda não portado: JSWebAssemblyStreamingContext")
            }
            InternalMicrotask::Opaque => panic!("InternalMicrotask::Opaque ainda não portado: o JSMicrotaskDispatcher"),
        };
        if let Err(thrown) = result {
            crate::runtime::js_promise_host::rethrow(self, thrown);
        }
    }

    fn promise_structure(&self) -> StructureRef {
        self.promise_data.borrow().structure.clone().expect("JSGlobalObject sem promiseStructure")
    }

    fn promise_prototype(&self) -> JSValue {
        self.promise_datum(|data| data.prototype, "promisePrototype")
    }

    fn promise_constructor(&self) -> JSValue {
        self.promise_datum(|data| data.constructor, "promiseConstructor")
    }

    fn promise_capability_object_structure(&self) -> StructureRef {
        self.promise_data
            .borrow()
            .capability_object_structure
            .clone()
            .expect("JSGlobalObject sem promiseCapabilityObjectStructure")
    }

    /// `promiseThenWatchpointSet().isStillValid()`: `Promise.prototype.then` é o `then` original e
    /// `Object.prototype` não tem `then` próprio (ver o cabeçalho do módulo).
    fn promise_then_watchpoint_is_valid(&self) -> bool {
        let vm = self.vm();
        let (prototype, then_function) = {
            let data = self.promise_data.borrow();
            (data.prototype, data.then_function)
        };
        let (Some(prototype), Some(then_function)) = (prototype, then_function) else { return false };
        let Some(prototype) = JSObject::from_value(&prototype) else { return false };
        let then = PropertyName::from_identifier(&vm.property_names.then);
        prototype.get_direct_by_name(vm, &then) == then_function && self.object_prototype().get_direct_offset(vm, &then) == INVALID_OFFSET
    }

    /// `promiseSpeciesWatchpointIsValid(vm, promise)` (`JSPromisePrototype.cpp`): o conjunto vigia
    /// `Promise.prototype.constructor` e `Promise[@@species]` (ver o cabeçalho do módulo).
    fn promise_species_watchpoint_is_valid(&self, promise: &JSPromise) -> bool {
        let vm = self.vm();
        let (structure, prototype, constructor, species_getter_setter) = {
            let data = self.promise_data.borrow();
            (data.structure.clone(), data.prototype, data.constructor, data.species_getter_setter)
        };
        let (Some(structure), Some(prototype), Some(constructor), Some(species_getter_setter)) =
            (structure, prototype, constructor, species_getter_setter)
        else {
            return false;
        };
        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);

        let Some(prototype_object) = JSObject::from_value(&prototype) else { return false };
        if prototype_object.get_direct_by_name(vm, &constructor_name) != constructor {
            return false;
        }
        let Some(constructor_function) = constructor.as_js_function() else { return false };
        if constructor_function.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.species_symbol))
            != species_getter_setter
        {
            return false;
        }

        if Rc::ptr_eq(&promise.structure(), &structure) {
            return true;
        }

        if promise.get_prototype_direct() != prototype {
            return false;
        }

        promise.get_direct_offset(vm, &constructor_name) == INVALID_OFFSET
    }

    fn error_structure(&self, error_type: ErrorType) -> StructureRef {
        self.error_structure_for(error_type)
    }

    fn get_property(&self, object: JSValue, property: PromiseProperty) -> Result<JSValue, PromiseThrown> {
        let names = &self.vm().property_names;
        let identifier = match property {
            PromiseProperty::Then => &names.then,
            PromiseProperty::Constructor => &names.constructor,
            PromiseProperty::Species => &names.species_symbol,
        };
        get_property_named(self, object, identifier)
    }

    fn construct(&self, constructor: JSValue, arguments: &[JSValue]) -> Result<JSValue, PromiseThrown> {
        construct_with_error_message(self, constructor, arguments, "argument is not a constructor")
            .map_err(|failure| thrown_from_llint_failure(self, failure))
    }

    fn call(&self, function: JSValue, arguments: &[JSValue], error_message: &str) -> Result<JSValue, PromiseThrown> {
        call_with_error_message(self, function, JSValue::undefined(), arguments, error_message)
            .map_err(|failure| thrown_from_llint_failure(self, failure))
    }

    fn create_type_error(&self, message: &str) -> JSValue {
        create_type_error(self, &WtfString::from_utf8(message.as_bytes())).as_value()
    }

    fn create_function_with_fields(&self, function: PromiseFunction) -> JSValue {
        let (length, native_function) = promise_function_native(function);
        JSFunctionWithFields::create(self.vm(), self, length, native_function).as_value()
    }

    fn function_field(&self, function: JSValue, field: FunctionField) -> JSValue {
        JSFunctionWithFields::get_field(&JSFunctionWithFields::from_value(&function).expect("uncheckedDowncast<JSFunctionWithFields>"), field)
    }

    fn set_function_field(&self, function: JSValue, field: FunctionField, value: JSValue) {
        JSFunctionWithFields::set_field(
            &JSFunctionWithFields::from_value(&function).expect("uncheckedDowncast<JSFunctionWithFields>"),
            field,
            value,
        );
    }

    fn create_internal_field_tuple(&self, _first: JSValue, _second: JSValue) -> JSValue {
        panic!("InternalFieldTuple ainda não portado: só nasce com contexto assíncrono, que o porte não tem")
    }

    fn is_internal_field_tuple(&self, _value: JSValue) -> bool {
        false
    }
}

/// O `getHostFunction` de cada `vm.promise...Executable()` (`VM.cpp`): comprimento e corpo nativo.
fn promise_function_native(function: PromiseFunction) -> (u32, NativeFunction) {
    match function {
        PromiseFunction::CapabilityExecutor => (2, promise_capability_executor_host),
        PromiseFunction::ResolvingFunctionResolve => (1, promise_resolving_function_resolve_host),
        PromiseFunction::ResolvingFunctionReject => (1, promise_resolving_function_reject_host),
        PromiseFunction::FirstResolvingFunctionResolve => (1, promise_first_resolving_function_resolve_host),
        PromiseFunction::FirstResolvingFunctionReject => (1, promise_first_resolving_function_reject_host),
        PromiseFunction::ResolvingFunctionResolveWithInternalMicrotask => {
            (1, promise_resolving_function_resolve_with_internal_microtask_host)
        }
        PromiseFunction::ResolvingFunctionRejectWithInternalMicrotask => {
            (1, promise_resolving_function_reject_with_internal_microtask_host)
        }
    }
}

/// O `JSC_DEFINE_HOST_FUNCTION` de uma função de resolução: o corpo recebe o callee e o `argument(0)` e
/// o C++ devolve `jsUndefined()`.
macro_rules! resolving_function_native {
    ($name:ident, $body:path) => {
        fn $name(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
            $body(global_object, JSValue::from_cell(call.callee()), call.argument(0));
            Ok(JSValue::undefined())
        }
    };
}

resolving_function_native!(promise_resolving_function_resolve_native, promise_resolving_function_resolve);
resolving_function_native!(promise_resolving_function_reject_native, promise_resolving_function_reject);
resolving_function_native!(promise_first_resolving_function_resolve_native, promise_first_resolving_function_resolve);
resolving_function_native!(promise_first_resolving_function_reject_native, promise_first_resolving_function_reject);
resolving_function_native!(
    promise_resolving_function_resolve_with_internal_microtask_native,
    promise_resolving_function_resolve_with_internal_microtask
);
resolving_function_native!(
    promise_resolving_function_reject_with_internal_microtask_native,
    promise_resolving_function_reject_with_internal_microtask
);

/// `promiseCapabilityExecutor`.
fn promise_capability_executor_native(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    host_result(
        global_object,
        promise_capability_executor(global_object, JSValue::from_cell(call.callee()), call.argument(0), call.argument(1)),
    )?;
    Ok(JSValue::undefined())
}

host_function!(promise_capability_executor_host, promise_capability_executor_native);
host_function!(promise_resolving_function_resolve_host, promise_resolving_function_resolve_native);
host_function!(promise_resolving_function_reject_host, promise_resolving_function_reject_native);
host_function!(promise_first_resolving_function_resolve_host, promise_first_resolving_function_resolve_native);
host_function!(promise_first_resolving_function_reject_host, promise_first_resolving_function_reject_native);
host_function!(
    promise_resolving_function_resolve_with_internal_microtask_host,
    promise_resolving_function_resolve_with_internal_microtask_native
);
host_function!(
    promise_resolving_function_reject_with_internal_microtask_host,
    promise_resolving_function_reject_with_internal_microtask_native
);

/// `promiseConstructorFuncResolve` e `promiseConstructorFuncReject` (e os `@resolve`/`@reject`): `this`
/// tem de ser um objeto (o construtor), e `argument(0)` o valor.
fn promise_constructor_func_resolve_or_reject(global_object: &JSGlobalObject, call: &HostCall, rejecting: bool) -> HostResult {
    let this_value = to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(HostThrown::type_error("|this| is not an object"));
    }
    let argument = call.argument(0);
    host_result(
        global_object,
        if rejecting {
            JSPromise::promise_reject(global_object, this_value, argument)
        } else {
            JSPromise::promise_resolve(global_object, this_value, argument)
        },
    )
}

fn promise_constructor_func_resolve(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_constructor_func_resolve_or_reject(global_object, call, false)
}

fn promise_constructor_func_reject(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_constructor_func_resolve_or_reject(global_object, call, true)
}

/// `promiseConstructorFuncWithResolvers`.
fn promise_constructor_func_with_resolvers(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    host_result(global_object, JSPromise::create_new_promise_capability(global_object, to_this_strict(call.this_value())))
}

/// `promiseConstructorFuncIsPromise`.
fn promise_constructor_func_is_promise(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(crate::runtime::js_value::js_boolean(JSPromise::from_value(&call.argument(0)).is_some()))
}

/// Qual combinador (`race`, `all`, `allSettled`, `any`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CombinatorKind {
    Race,
    All,
    AllSettled,
    Any,
}

impl CombinatorKind {
    /// A `InternalMicrotask` de `...ResolveJob` do combinador.
    fn resolve_job(self) -> InternalMicrotask {
        match self {
            CombinatorKind::Race => InternalMicrotask::PromiseRaceResolveJob,
            CombinatorKind::All => InternalMicrotask::PromiseAllResolveJob,
            CombinatorKind::AllSettled => InternalMicrotask::PromiseAllSettledResolveJob,
            CombinatorKind::Any => InternalMicrotask::PromiseAnyResolveJob,
        }
    }

    /// `race` não conta elementos (não tem `values`).
    fn counts_elements(self) -> bool {
        self != CombinatorKind::Race
    }
}

/// `isFastPromiseConstructor(globalObject, value)`.
fn is_fast_promise_constructor(global_object: &JSGlobalObject, value: JSValue) -> bool {
    value == global_object.promise_constructor() && global_object.promise_resolve_watchpoint_is_valid()
}

/// `canSkipIntermediatePromise(globalObject, value)`.
fn can_skip_intermediate_promise(global_object: &JSGlobalObject, value: JSValue) -> bool {
    if !global_object.promise_then_watchpoint_is_valid() {
        return false;
    }
    if !value.is_cell() {
        return true;
    }
    if JSPromise::from_value(&value).is_some() {
        return false;
    }
    match JSObject::from_value(&value) {
        None => !value.is_object(),
        Some(object) => is_definitely_non_thenable(&object, global_object),
    }
}

/// O `{ resolve, reject, promiseResolve }` do caminho lento (`this` não é o `Promise` intrínseco).
#[derive(Clone, Copy)]
struct SlowTarget {
    resolve: JSValue,
    reject: JSValue,
    /// `thisValue.get(resolve)`, já conferida como chamável.
    promise_resolve: JSValue,
}

/// O estado de uma chamada de `Promise.race`/`all`/`allSettled`/`any`, nos dois caminhos do C++ (o
/// rápido, com `this` igual ao `Promise` intrínseco, e o lento, por `newPromiseCapability`).
struct Combinator<'a> {
    global_object: &'a JSGlobalObject,
    kind: CombinatorKind,
    this_value: JSValue,
    /// A promessa de resultado (a nova, no caminho rápido; a da capacidade, no lento).
    promise: JSValue,
    /// `None` no caminho rápido.
    slow: Option<SlowTarget>,
    /// `globalContext` (`None` em `race`).
    context: Option<JSPromiseCombinatorsGlobalContextRef>,
    /// `index`.
    index: u64,
    /// `resolve` e `onRejected`, criados na primeira vez que o caminho rápido os precisa.
    first_resolve: Option<JSValue>,
    first_reject: Option<JSValue>,
    /// Tudo acima vive só neste local Rust enquanto o iterável roda JS (a pilha C++ do JSC).
    marked: crate::runtime::gc::RootScope,
}

impl Combinator<'_> {
    /// `callReject(exception)` e `callRejectWithScopeException()`: rejeita o resultado com a exceção
    /// capturada e devolve a promessa. Terminação segue pendente.
    fn reject_with(&self, thrown: PromiseThrown) -> HostResult {
        let PromiseThrown::Value(error) = thrown else { return Err(HostThrown::Pending) };
        match &self.slow {
            None => promise_of(&self.promise).reject(self.global_object, error),
            Some(slow) => {
                host_result(self.global_object, self.global_object.call(slow.reject, &[error], "reject is not a function"))?;
            }
        }
        Ok(self.promise)
    }

    fn first_resolve(&mut self) -> JSValue {
        let promise = promise_of(&self.promise);
        let global_object = self.global_object;
        let marked = &self.marked;
        *self.first_resolve.get_or_insert_with(|| {
            let function = promise.create_first_resolve_function(global_object);
            marked.append(function);
            function
        })
    }

    fn first_reject(&mut self) -> JSValue {
        let promise = promise_of(&self.promise);
        let global_object = self.global_object;
        let marked = &self.marked;
        *self.first_reject.get_or_insert_with(|| {
            let function = promise.create_first_reject_function(global_object);
            marked.append(function);
            function
        })
    }

    /// Um elemento do iterável (o corpo da lambda de `forEachInIterable`).
    fn element(&mut self, value: JSValue) -> Result<(), HostThrown> {
        let global_object = self.global_object;
        if let Some(context) = &self.context {
            host_result(global_object, context.put_direct_index(global_object, self.index, JSValue::undefined()))?;
        }
        match self.slow {
            None => self.fast_element(value),
            Some(slow) => self.slow_element(value, slow),
        }
    }

    /// O caminho rápido de um elemento.
    fn fast_element(&mut self, value: JSValue) -> Result<(), HostThrown> {
        let global_object = self.global_object;
        let kind = self.kind;

        if can_skip_intermediate_promise(global_object, value) {
            match &self.context {
                None => global_object.queue_microtask(
                    InternalMicrotask::PromiseRaceResolveJob,
                    Status::Fulfilled.payload(),
                    &[self.promise, value, self.promise],
                ),
                Some(context) => {
                    context.set_remaining_elements_count(context.remaining_elements_count() + 1);
                    global_object.queue_microtask(
                        kind.resolve_job(),
                        Status::Fulfilled.payload(),
                        &[context.as_value(), value, js_number(self.index as f64)],
                    );
                    self.index += 1;
                }
            }
            return Ok(());
        }

        let next_promise = host_result(global_object, JSPromise::resolved_promise(global_object, value))?;

        if let Some(context) = &self.context {
            context.set_remaining_elements_count(context.remaining_elements_count() + 1);
        }

        if next_promise.is_then_fast_and_non_observable(global_object) {
            let constructor = host_result(global_object, promise_species_constructor(global_object, next_promise.as_value()))?;
            if constructor == global_object.promise_constructor() {
                match &self.context {
                    None => next_promise.perform_promise_then_with_internal_microtask(
                        global_object,
                        InternalMicrotask::PromiseRaceResolveJob,
                        Some(promise_of(&self.promise).cell_id()),
                        self.promise,
                        JSValue::empty(),
                    ),
                    Some(context) => {
                        next_promise.perform_promise_then_with_internal_microtask(
                            global_object,
                            kind.resolve_job(),
                            Some(context.cell_id()),
                            js_number(self.index as f64),
                            JSValue::empty(),
                        );
                        self.index += 1;
                    }
                }
                return Ok(());
            }
        }

        let (on_fulfilled, on_rejected) = match (kind, self.context.clone()) {
            (CombinatorKind::Race, _) => (self.first_resolve(), self.first_reject()),
            (CombinatorKind::All, Some(context)) => {
                let on_rejected = self.first_reject();
                let on_fulfilled = JSFunctionWithFields::create(self.global_object.vm(), global_object, 1, promise_all_fulfill_function_host);
                let element_context = JSPromiseCombinatorsContext::create(&context, self.index);
                JSFunctionWithFields::set_field(&on_fulfilled, FunctionField::PROMISE_ALL_CONTEXT, element_context.as_value());
                (on_fulfilled.as_value(), on_rejected)
            }
            (CombinatorKind::AllSettled, Some(context)) => {
                let element_context = JSPromiseCombinatorsContext::create(&context, self.index);
                all_settled_function_pair(
                    global_object,
                    &element_context,
                    promise_all_settled_fulfill_function_host,
                    promise_all_settled_reject_function_host,
                )
            }
            (CombinatorKind::Any, Some(context)) => {
                // For Promise.any, onFulfilled just resolves the main promise directly.
                let on_fulfilled = self.first_resolve();
                let element_context = JSPromiseCombinatorsContext::create(&context, self.index);
                let on_rejected = JSFunctionWithFields::create(self.global_object.vm(), global_object, 1, promise_any_reject_function_host);
                JSFunctionWithFields::set_field(&on_rejected, FunctionField::PROMISE_ANY_CONTEXT, element_context.as_value());
                (on_fulfilled, on_rejected.as_value())
            }
            (_, None) => unreachable!("combinador de elementos sem globalContext"),
        };

        let vm = global_object.vm();
        let then = get_value_property(global_object, next_promise.as_value(), &PropertyName::from_identifier(&vm.property_names.then))?;
        call_checked(global_object, then, next_promise.as_value(), &[on_fulfilled, on_rejected], "then is not a function")?;
        if kind.counts_elements() {
            self.index += 1;
        }
        Ok(())
    }

    /// O caminho lento de um elemento: `promiseResolve` do `this` e as funções de elemento `...Slow...`.
    fn slow_element(&mut self, value: JSValue, slow: SlowTarget) -> Result<(), HostThrown> {
        let global_object = self.global_object;
        let vm = global_object.vm();

        let next_promise =
            call_checked(global_object, slow.promise_resolve, self.this_value, &[value], "Promise resolve is not a function")?;

        let element_context = self.context.clone().map(|context| {
            context.set_remaining_elements_count(context.remaining_elements_count() + 1);
            let element_context = JSPromiseCombinatorsContext::create(&context, self.index);
            self.index += 1;
            element_context
        });

        let (on_fulfilled, on_rejected) = match (self.kind, element_context) {
            (CombinatorKind::Race, _) => (slow.resolve, slow.reject),
            (CombinatorKind::All, Some(context)) => {
                let on_fulfilled = JSFunctionWithFields::create(vm, global_object, 1, promise_all_slow_fulfill_function_host);
                JSFunctionWithFields::set_field(&on_fulfilled, FunctionField::PROMISE_ALL_CONTEXT, context.as_value());
                JSFunctionWithFields::set_field(&on_fulfilled, FunctionField::PROMISE_ALL_RESOLVE, slow.resolve);
                (on_fulfilled.as_value(), slow.reject)
            }
            (CombinatorKind::AllSettled, Some(context)) => all_settled_function_pair(
                global_object,
                &context,
                promise_all_settled_slow_fulfill_function_host,
                promise_all_settled_slow_reject_function_host,
            ),
            (CombinatorKind::Any, Some(context)) => {
                let on_rejected = JSFunctionWithFields::create(vm, global_object, 1, promise_any_slow_reject_function_host);
                JSFunctionWithFields::set_field(&on_rejected, FunctionField::PROMISE_ANY_CONTEXT, context.as_value());
                JSFunctionWithFields::set_field(&on_rejected, FunctionField::PROMISE_ANY_REJECT, slow.reject);
                (slow.resolve, on_rejected.as_value())
            }
            (_, None) => unreachable!("combinador de elementos sem globalContext"),
        };

        let then = get_value_property(global_object, next_promise, &PropertyName::from_identifier(&vm.property_names.then))?;
        call_checked(global_object, then, next_promise, &[on_fulfilled, on_rejected], "then is not a function")?;
        Ok(())
    }

    /// O que vem depois do laço: o último decremento (o `- 1` do `remainingElementsCount` inicial) e, no
    /// zero, a liquidação do resultado.
    fn finish(&self, call: &HostCall) -> HostResult {
        let global_object = self.global_object;
        let Some(context) = &self.context else { return Ok(self.promise) };

        let count = context.remaining_elements_count() - 1;
        context.set_remaining_elements_count(count);
        if count != 0 {
            return Ok(self.promise);
        }

        let values = context.values();
        let result = match (self.kind, &self.slow) {
            (CombinatorKind::All | CombinatorKind::AllSettled, None) => {
                promise_of(&self.promise).resolve(global_object, values);
                Ok(JSValue::undefined())
            }
            (CombinatorKind::Any, None) => {
                let error = create_aggregate_error(global_object, values, call.capture_stack_frames_with_native(global_object));
                promise_of(&self.promise).reject(global_object, error);
                Ok(JSValue::undefined())
            }
            (CombinatorKind::All | CombinatorKind::AllSettled, Some(slow)) => {
                global_object.call(slow.resolve, &[values], "resolve is not a function")
            }
            (CombinatorKind::Any, Some(slow)) => {
                let error = create_aggregate_error(global_object, values, call.capture_stack_frames_with_native(global_object));
                global_object.call(slow.reject, &[error], "reject is not a function")
            }
            (CombinatorKind::Race, _) => unreachable!("race não tem globalContext"),
        };
        match result {
            Ok(_) => Ok(self.promise),
            Err(thrown) => self.reject_with(thrown),
        }
    }
}

/// O par `onFulfilled`/`onRejected` de um elemento de `allSettled`: as duas funções dividem o contexto e
/// apontam uma para a outra (`...Other`), para que só a primeira chamada valha.
fn all_settled_function_pair(
    global_object: &JSGlobalObject,
    context: &JSPromiseCombinatorsContextRef,
    fulfill: NativeFunction,
    reject: NativeFunction,
) -> (JSValue, JSValue) {
    let vm = global_object.vm();
    let on_fulfilled = JSFunctionWithFields::create(vm, global_object, 1, fulfill);
    JSFunctionWithFields::set_field(&on_fulfilled, FunctionField::PROMISE_ALL_SETTLED_CONTEXT, context.as_value());
    let on_rejected = JSFunctionWithFields::create(vm, global_object, 1, reject);
    JSFunctionWithFields::set_field(&on_rejected, FunctionField::PROMISE_ALL_SETTLED_CONTEXT, context.as_value());

    JSFunctionWithFields::set_field(&on_fulfilled, FunctionField::PROMISE_ALL_SETTLED_OTHER, on_rejected.as_value());
    JSFunctionWithFields::set_field(&on_rejected, FunctionField::PROMISE_ALL_SETTLED_OTHER, on_fulfilled.as_value());
    (on_fulfilled.as_value(), on_rejected.as_value())
}

/// `vectorLengthHintForCombinator(iterable)`.
fn vector_length_hint_for_combinator(iterable: JSValue) -> u32 {
    JSArray::from_value(&iterable).map_or(0, |array| array.length().min(MAX_STORAGE_VECTOR_LENGTH))
}

/// `promiseRaceSlow`/`promiseConstructorFuncRace`, `promiseAllSlow`/`promiseConstructorFuncAll`,
/// `promiseAllSettledSlow`/`promiseConstructorFuncAllSettled` e `promiseAnySlow`/
/// `promiseConstructorFuncAny`, os oito corpos do C++ num só (a diferença é `CombinatorKind` e o caminho
/// rápido ou lento de `Combinator`).
fn promise_combinator(global_object: &JSGlobalObject, call: &HostCall, kind: CombinatorKind) -> HostResult {
    let vm = global_object.vm();
    let this_value = to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(HostThrown::type_error("|this| is not an object"));
    }
    let iterable = call.argument(0);

    let mut combinator = Combinator {
        global_object,
        kind,
        this_value,
        promise: JSValue::empty(),
        slow: None,
        context: None,
        index: 0,
        first_resolve: None,
        first_reject: None,
        marked: crate::runtime::gc::RootScope::new(&[]),
    };

    // O `promise` (e, no caminho lento, `resolve`, `reject` e `this.resolve`).
    if is_fast_promise_constructor(global_object, this_value) {
        combinator.promise = JSPromise::create(vm, &global_object.promise_structure()).as_value();
        combinator.marked.append(combinator.promise);
    } else {
        let PromiseCapability { promise, resolve, reject } =
            host_result(global_object, JSPromise::new_promise_capability(global_object, this_value))?;
        combinator.marked.append(promise);
        combinator.marked.append(resolve);
        combinator.marked.append(reject);
        combinator.promise = promise;
        combinator.slow = Some(SlowTarget { resolve, reject, promise_resolve: JSValue::empty() });

        let promise_resolve = match get_property_named(global_object, this_value, &vm.property_names.resolve) {
            Ok(promise_resolve) => promise_resolve,
            Err(thrown) => return combinator.reject_with(thrown),
        };
        combinator.marked.append(promise_resolve);
        if !promise_resolve.is_callable() {
            return combinator.reject_with(PromiseThrown::Value(global_object.create_type_error("Promise resolve is not a function")));
        }
        combinator.slow = Some(SlowTarget { resolve, reject, promise_resolve });
    }

    if kind.counts_elements() {
        let Some(values) = JSArray::try_create_with_hint(vm, &global_object.array_structure(), 0, vector_length_hint_for_combinator(iterable))
        else {
            let mut scope = ThrowScope::new(vm);
            throw_out_of_memory_error(global_object, &mut scope);
            return combinator.reject_with(caught_exception(global_object));
        };
        // `allSettled` lento guarda `resolve` no lugar da promessa (`promiseAllSettledSlowFulfillFunction`).
        let promise_slot = match (kind, &combinator.slow) {
            (CombinatorKind::AllSettled, Some(slow)) => slow.resolve,
            _ => combinator.promise,
        };
        combinator.context = Some(JSPromiseCombinatorsGlobalContext::create(promise_slot, values.as_value(), 1));
        combinator.marked.append(combinator.context.as_ref().expect("acabou de ser criado").as_value());
    }

    if let Err(thrown) = for_each_in_iterable(global_object, iterable, |value| combinator.element(value)) {
        // `forEachInIterable` deixa a exceção pendente (as do protocolo, como o `TypeError` de não
        // iterável, ainda não foram lançadas): `callRejectWithScopeException`.
        throw_thrown(global_object, thrown);
        return combinator.reject_with(caught_exception(global_object));
    }

    combinator.finish(call)
}

fn promise_constructor_func_race(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_combinator(global_object, call, CombinatorKind::Race)
}

fn promise_constructor_func_all(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_combinator(global_object, call, CombinatorKind::All)
}

fn promise_constructor_func_all_settled(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_combinator(global_object, call, CombinatorKind::AllSettled)
}

fn promise_constructor_func_any(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_combinator(global_object, call, CombinatorKind::Any)
}

/// O que acontece quando o último elemento chega (a cauda de todas as funções de elemento).
enum ElementCompletion {
    /// `promise->resolve(globalObject, vm, values)`.
    ResolvePromise,
    /// `promise->reject(vm, createAggregateError(errors))`.
    RejectPromiseWithAggregateError,
    /// `call(globalObject, resolve, undefined, [values])`.
    CallResolve(JSValue),
    /// `call(globalObject, reject, undefined, [createAggregateError(errors)])`.
    CallRejectWithAggregateError(JSValue),
}

/// A cauda comum de `promiseAllFulfillFunction`, `promiseAllSettled*Function` e `promiseAny*Function`:
/// grava o valor no índice do elemento, decrementa a contagem e, no zero, liquida.
fn complete_combinator_element(
    global_object: &JSGlobalObject,
    context: &JSPromiseCombinatorsContext,
    value: JSValue,
    completion: ElementCompletion,
) -> HostResult {
    let global_context = context.global_context();
    host_result(global_object, global_context.put_direct_index(global_object, context.index(), value))?;

    let count = global_context.remaining_elements_count() - 1;
    global_context.set_remaining_elements_count(count);
    if count == 0 {
        let values = global_context.values();
        match completion {
            ElementCompletion::ResolvePromise => promise_of(&global_context.promise()).resolve(global_object, values),
            ElementCompletion::RejectPromiseWithAggregateError => {
                promise_of(&global_context.promise()).reject(global_object, create_aggregate_error(global_object, values, None))
            }
            ElementCompletion::CallResolve(resolve) => {
                host_result(global_object, global_object.call(resolve, &[values], "resolve is not a function"))?;
            }
            ElementCompletion::CallRejectWithAggregateError(reject) => {
                let error = create_aggregate_error(global_object, values, None);
                host_result(global_object, global_object.call(reject, &[error], "reject is not a function"))?;
            }
        }
    }
    Ok(JSValue::undefined())
}

/// `dynamicDowncast<JSPromiseCombinatorsContext>(callee->getField(field))` e o `setField(field, jsNull())`
/// que o consome (a segunda chamada não faz nada).
fn take_context(callee: &JSFunction, field: FunctionField) -> Option<JSPromiseCombinatorsContextRef> {
    let context = JSPromiseCombinatorsContext::from_value(&JSFunctionWithFields::get_field(callee, field))?;
    JSFunctionWithFields::set_field(callee, field, JSValue::null());
    Some(context)
}

/// O começo das funções de elemento de `allSettled`: o contexto e a função irmã, e os dois campos de
/// cada uma zerados (só a primeira das duas, `fulfill` ou `reject`, vale).
fn take_all_settled_context(callee: &JSFunction) -> Option<JSPromiseCombinatorsContextRef> {
    let context = JSPromiseCombinatorsContext::from_value(&JSFunctionWithFields::get_field(
        callee,
        FunctionField::PROMISE_ALL_SETTLED_CONTEXT,
    ))?;
    let other = JSFunctionWithFields::from_value(&JSFunctionWithFields::get_field(callee, FunctionField::PROMISE_ALL_SETTLED_OTHER))?;

    for function in [callee, &*other] {
        JSFunctionWithFields::set_field(function, FunctionField::PROMISE_ALL_SETTLED_CONTEXT, JSValue::null());
        JSFunctionWithFields::set_field(function, FunctionField::PROMISE_ALL_SETTLED_OTHER, JSValue::null());
    }
    Some(context)
}

/// `promiseAllFulfillFunction`.
fn promise_all_fulfill_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callee = JSFunctionWithFields::callee(call);
    let Some(context) = take_context(&callee, FunctionField::PROMISE_ALL_CONTEXT) else { return Ok(JSValue::undefined()) };
    complete_combinator_element(global_object, &context, call.argument(0), ElementCompletion::ResolvePromise)
}

/// `promiseAllSlowFulfillFunction`.
fn promise_all_slow_fulfill_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callee = JSFunctionWithFields::callee(call);
    let resolve = JSFunctionWithFields::get_field(&callee, FunctionField::PROMISE_ALL_RESOLVE);
    let Some(context) = take_context(&callee, FunctionField::PROMISE_ALL_CONTEXT) else { return Ok(JSValue::undefined()) };
    JSFunctionWithFields::set_field(&callee, FunctionField::PROMISE_ALL_RESOLVE, JSValue::null());
    complete_combinator_element(global_object, &context, call.argument(0), ElementCompletion::CallResolve(resolve))
}

/// `promiseAllSettledFulfillFunction`, `promiseAllSettledRejectFunction` e as duas `...Slow...`: o
/// resultado `{ status, value | reason }` entra no índice, e a liquidação é a promessa (rápido) ou a
/// função `resolve` que a global guarda no lugar da promessa (lento).
fn promise_all_settled_function(global_object: &JSGlobalObject, call: &HostCall, rejected: bool, slow: bool) -> HostResult {
    let callee = JSFunctionWithFields::callee(call);
    let Some(context) = take_all_settled_context(&callee) else { return Ok(JSValue::undefined()) };

    let argument = call.argument(0);
    let result_object = if rejected {
        create_promise_all_settled_rejected_result(global_object, argument)
    } else {
        create_promise_all_settled_fulfilled_result(global_object, argument)
    };
    let completion = if slow {
        ElementCompletion::CallResolve(context.global_context().promise())
    } else {
        ElementCompletion::ResolvePromise
    };
    complete_combinator_element(global_object, &context, result_object, completion)
}

fn promise_all_settled_fulfill_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_all_settled_function(global_object, call, false, false)
}

fn promise_all_settled_reject_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_all_settled_function(global_object, call, true, false)
}

fn promise_all_settled_slow_fulfill_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_all_settled_function(global_object, call, false, true)
}

fn promise_all_settled_slow_reject_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    promise_all_settled_function(global_object, call, true, true)
}

/// `promiseAnyRejectFunction`.
fn promise_any_reject_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callee = JSFunctionWithFields::callee(call);
    let Some(context) = take_context(&callee, FunctionField::PROMISE_ANY_CONTEXT) else { return Ok(JSValue::undefined()) };
    complete_combinator_element(global_object, &context, call.argument(0), ElementCompletion::RejectPromiseWithAggregateError)
}

/// `promiseAnySlowRejectFunction`.
fn promise_any_slow_reject_function(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callee = JSFunctionWithFields::callee(call);
    let reject = JSFunctionWithFields::get_field(&callee, FunctionField::PROMISE_ANY_REJECT);
    let Some(context) = take_context(&callee, FunctionField::PROMISE_ANY_CONTEXT) else { return Ok(JSValue::undefined()) };
    JSFunctionWithFields::set_field(&callee, FunctionField::PROMISE_ANY_REJECT, JSValue::null());
    complete_combinator_element(global_object, &context, call.argument(0), ElementCompletion::CallRejectWithAggregateError(reject))
}

host_function!(promise_all_fulfill_function_host, promise_all_fulfill_function);
host_function!(promise_all_slow_fulfill_function_host, promise_all_slow_fulfill_function);
host_function!(promise_all_settled_fulfill_function_host, promise_all_settled_fulfill_function);
host_function!(promise_all_settled_reject_function_host, promise_all_settled_reject_function);
host_function!(promise_all_settled_slow_fulfill_function_host, promise_all_settled_slow_fulfill_function);
host_function!(promise_all_settled_slow_reject_function_host, promise_all_settled_slow_reject_function);
host_function!(promise_any_reject_function_host, promise_any_reject_function);
host_function!(promise_any_slow_reject_function_host, promise_any_slow_reject_function);

host_function!(promise_constructor_func_resolve_host, promise_constructor_func_resolve);
host_function!(promise_constructor_func_reject_host, promise_constructor_func_reject);
host_function!(promise_constructor_func_with_resolvers_host, promise_constructor_func_with_resolvers);
host_function!(promise_constructor_func_is_promise_host, promise_constructor_func_is_promise);
host_function!(promise_constructor_func_race_host, promise_constructor_func_race);
host_function!(promise_constructor_func_all_host, promise_constructor_func_all);
host_function!(promise_constructor_func_all_settled_host, promise_constructor_func_all_settled);
host_function!(promise_constructor_func_any_host, promise_constructor_func_any);

/// `createPromiseAllSettledFulfilledResultStructure` e `...Rejected...`: `{ status, value | reason }`
/// com os offsets que as constantes `promiseAllSettled*PropertyOffset` fixam.
fn create_promise_all_settled_result_structure(vm: &VM, global_object: &JSGlobalObject, second_property: &Identifier) -> StructureRef {
    const INLINE_CAPACITY: u32 = 2;
    let mut structure = global_object.structure_cache().empty_object_structure_for_prototype(
        global_object,
        &global_object.object_prototype(),
        INLINE_CAPACITY,
        false,
    );
    let properties = [
        (&vm.property_names.status, PROMISE_ALL_SETTLED_STATUS_PROPERTY_OFFSET),
        (second_property, PROMISE_ALL_SETTLED_VALUE_PROPERTY_OFFSET),
    ];
    for (identifier, expected_offset) in properties {
        let (next, offset) = Structure::add_property_transition(vm, &structure, &PropertyName::from_identifier(identifier), 0);
        assert_eq!(offset, expected_offset);
        structure = next;
    }
    structure
}

/// `createPromiseAllSettledFulfilledResult(globalObject, value)` e a rejeitada: `{ status, value | reason }`.
fn create_promise_all_settled_result(global_object: &JSGlobalObject, structure: &StructureRef, status: &[u8], second: JSValue) -> JSValue {
    let vm = global_object.vm();
    let result_object = JSFinalObject::create(vm, structure);
    result_object.put_direct_offset(
        vm,
        PROMISE_ALL_SETTLED_STATUS_PROPERTY_OFFSET,
        JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(status))),
    );
    result_object.put_direct_offset(vm, PROMISE_ALL_SETTLED_VALUE_PROPERTY_OFFSET, second);
    result_object.as_value()
}

/// `createPromiseAllSettledFulfilledResult(globalObject, value)`.
pub fn create_promise_all_settled_fulfilled_result(global_object: &JSGlobalObject, value: JSValue) -> JSValue {
    create_promise_all_settled_result(global_object, &global_object.promise_all_settled_fulfilled_result_structure(), b"fulfilled", value)
}

/// `createPromiseAllSettledRejectedResult(globalObject, reason)`.
pub fn create_promise_all_settled_rejected_result(global_object: &JSGlobalObject, reason: JSValue) -> JSValue {
    create_promise_all_settled_result(global_object, &global_object.promise_all_settled_rejected_result_structure(), b"rejected", reason)
}

/// `createAggregateError(vm, globalObject->errorStructure(ErrorType::AggregateError), errors, String(),
/// jsUndefined())` (`AggregateError.cpp`): o `ErrorInstance` com `errors` (`DontEnum`). O `cause` é o
/// `jsUndefined()` explícito (não o `JSValue()` vazio), e `ErrorInstance::finishCreation` só omite a
/// propriedade quando ele é vazio: o erro de `Promise.any` tem `cause` próprio, valendo `undefined`.
///
/// `frames` é a pilha capturada de dentro do `Promise.any` (o erro de `Promise.any([])` mostra `at any (unknown)`
/// no bun); os rejeitadores de elemento rodam numa microtask, sem frames, e o erro fica sem `stack`.
pub fn create_aggregate_error(global_object: &JSGlobalObject, errors: JSValue, frames: Option<Vec<StackFrame>>) -> JSValue {
    let vm = global_object.vm();
    let structure = global_object.error_structure_for(ErrorType::AggregateError);
    let error = aggregate_error::create_aggregate_error(vm, structure, errors, None, Some(JSValue::undefined()));
    if let Some(frames) = frames {
        error.set_pending_stack(frames);
    }
    error.as_value()
}

/// `const ClassInfo JSPromiseConstructor::s_info` (`"Function"`, base `JSFunction`, `&promiseConstructorTable`).
pub static JS_PROMISE_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&JS_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&PROMISE_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `promiseConstructorTableValues` de `JSPromiseConstructor.lut.h`, na ordem do `@begin`.
static PROMISE_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 7] = [
    native_entry_with_intrinsic("resolve", promise_constructor_func_resolve_host, 1, Intrinsic::PromiseConstructorResolveIntrinsic),
    native_entry_with_intrinsic("reject", promise_constructor_func_reject_host, 1, Intrinsic::PromiseConstructorRejectIntrinsic),
    native_entry_with_intrinsic("race", promise_constructor_func_race_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("all", promise_constructor_func_all_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("allSettled", promise_constructor_func_all_settled_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("any", promise_constructor_func_any_host, 1, Intrinsic::NoIntrinsic),
    native_entry_with_intrinsic("withResolvers", promise_constructor_func_with_resolvers_host, 0, Intrinsic::NoIntrinsic),
];

/// `promiseConstructorTable`.
static PROMISE_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &PROMISE_CONSTRUCTOR_TABLE_VALUES };

/// `class JSPromiseConstructor : public JSFunction`: espaço de nomes de `create`.
pub struct JSPromiseConstructor;

impl JSPromiseConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::JSFunctionType, JSPromiseConstructor::STRUCTURE_FLAGS),
            &JS_PROMISE_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, structure, promisePrototype)` com `finishCreation` e `addOwnInternalSlots`: o
    /// `JSFunction` sobre `promiseConstructorPromiseConstructorCodeGenerator(vm)`, com o `@@species`
    /// (`promiseSpeciesGetterSetter()`, `JSGlobalObject.cpp:1222`).
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        promise_prototype: &JSObject,
    ) -> JSFunctionRef {
        let executable = vm.builtin_executables().code_generator(vm, BuiltinCodeIndex::PromiseConstructorPromiseConstructorCode);
        let scope = JSScope::from_cell_id(global_object.cell_id()).expect("o JSGlobalObject é um JSScope registrado");
        let constructor = JSFunction::create_with_structure(vm, global_object, &executable, scope, structure);

        // `finishCreation(vm, promisePrototype)`.
        // As entradas de `PROMISE_CONSTRUCTOR_TABLE` (lut) não nascem aqui: reificam no primeiro acesso.
        let define = |name: &Identifier, length: u32, function: NativeFunction, visibility: ImplementationVisibility, intrinsic: Intrinsic| {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                &constructor,
                name,
                length,
                function,
                visibility,
                intrinsic,
                DONT_ENUM,
            );
        };
        let literal = |text: &[u8]| Identifier::from_span(vm, text);
        constructor.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            promise_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        // `try` (builtin JS) vem antes do `@@species` no golden.
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            &constructor,
            &vm.property_names.try_keyword,
            BuiltinCodeIndex::PromiseConstructorTryCode,
            DONT_ENUM,
        );
        // `putDirectNonIndexAccessorWithoutTransition(vm, speciesSymbol, promiseSpeciesGetterSetter(), Accessor | ReadOnly | DontEnum)`.
        put_species_accessor(vm, global_object, &constructor);
        if Options::use_promise_is_promise() {
            define(&literal(b"isPromise"), 1, promise_constructor_func_is_promise_host, ImplementationVisibility::Public, Intrinsic::NoIntrinsic);
        }

        // `addOwnInternalSlots(vm, globalObject)` (`USE(BUN_JSC_ADDITIONS)`): `@resolve` e `@reject`.
        let builtin_names = vm.property_names.builtin_names();
        define(
            &builtin_names.resolve_private_name(),
            1,
            promise_constructor_func_resolve_host,
            ImplementationVisibility::Private,
            Intrinsic::PromiseConstructorResolveIntrinsic,
        );
        define(
            &builtin_names.reject_private_name(),
            1,
            promise_constructor_func_reject_host,
            ImplementationVisibility::Private,
            Intrinsic::PromiseConstructorRejectIntrinsic,
        );
        constructor
    }
}

impl JSGlobalObject {
    /// As linhas de `JSGlobalObject::init` (`JSGlobalObject.cpp`) que criam o `Promise`: o
    /// `defaultPromiseThen` (`LinkTimeConstant::DefaultPromiseThen`), `Promise.prototype`,
    /// `m_promiseStructure`, o `Promise` (propriedade global e
    /// `LinkTimeConstant::Promise`), as três `Structure` de objeto, as `LinkTimeConstant` de função
    /// (`promise_global_functions.rs`) e a posse de `m_promiseResolveWatchpointSet`. Depende do
    /// `Object.prototype`, do `Function.prototype` e da `arrayStructure()` já criados.
    pub fn init_promise(&self) {
        let vm = self.vm();
        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);

        let then_function = create_default_promise_then(vm, self);
        self.set_link_time_constant(LinkTimeConstant::DefaultPromiseThen, then_function.as_value());

        let prototype_structure = JSPromisePrototype::create_structure(vm, self, self.object_prototype().as_value());
        let prototype = JSPromisePrototype::create(vm, self, &prototype_structure, Rc::clone(&then_function));
        prototype.did_become_prototype(vm);

        let structure = JSPromise::create_structure(vm, Some(self), prototype.as_value());
        let constructor_structure = JSPromiseConstructor::create_structure(vm, self, self.function_prototype().as_value());
        let constructor = JSPromiseConstructor::create(vm, self, constructor_structure, &prototype);
        prototype.put_direct_without_transition(vm, &constructor_name, constructor.as_value(), DONT_ENUM);
        self.put_direct(vm, &PropertyName::from_identifier(vm.property_names.builtin_names().promise_public_name()), constructor.as_value(), DONT_ENUM);
        self.set_link_time_constant(LinkTimeConstant::Promise, constructor.as_value());

        // O `resolve` original é lido já aqui (o `promiseResolveWatchpointSet` o vigia): o `resolve` da tabela é
        // reificado agora, depois das propriedades eager, como o bun mostra em `Reflect.ownKeys(Promise)`.
        crate::runtime::lookup::reify_static_property(vm, &constructor, &PROMISE_CONSTRUCTOR_TABLE_VALUES[0]);
        let resolve_function =
            constructor.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.resolve));
        let species_getter_setter =
            constructor.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.species_symbol));

        {
            let mut data = self.promise_data.borrow_mut();
            data.structure = Some(structure);
            data.prototype = Some(prototype.as_value());
            data.constructor = Some(constructor.as_value());
            data.then_function = Some(then_function.as_value());
            data.resolve_function = Some(resolve_function);
            data.species_getter_setter = Some(species_getter_setter);
        }

        // `m_promiseCapabilityObjectStructure` e as duas de `allSettled` (`LazyProperty` no C++: aqui nascem
        // junto do global, que já tem o `Object.prototype` e o `StructureCache`).
        let capability_object_structure = create_promise_capability_object_structure(vm, self);
        let fulfilled_structure = create_promise_all_settled_result_structure(vm, self, &vm.property_names.value);
        let rejected_structure = create_promise_all_settled_result_structure(vm, self, &vm.property_names.reason);
        {
            let mut data = self.promise_data.borrow_mut();
            data.capability_object_structure = Some(capability_object_structure);
            data.all_settled_fulfilled_result_structure = Some(fulfilled_structure);
            data.all_settled_rejected_result_structure = Some(rejected_structure);
        }

        crate::runtime::promise_global_functions::install_promise_link_time_constants(self);
    }
}
