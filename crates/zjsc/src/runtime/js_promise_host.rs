//! O que `JSPromise.cpp` pede do `JSGlobalObject` e do `VM` que ainda não existem no porte, como um
//! trait único: `PromiseHost`. Quem portar o `JSGlobalObject` completo o implementa; os testes o
//! implementam com um host mínimo.
//!
//! No C++ cada item é um acesso direto ao global object (`globalObject->queueMicrotask`,
//! `globalObject->promiseStructure()`, `promiseConstructor()`, `promiseThenWatchpointSet()`,
//! `globalObjectMethodTable()->promiseRejectionTracker`), ao `VM` (`m_synchronousModuleQueue`,
//! `AsyncContextSwapScope::current`) ou a uma operação genérica do motor (`construct`, `call`, `get`,
//! `JSValue::isCallable`, `JSFunctionWithFields`). Cada método diz o que ele é no C++.
//!
//! DIVERGÊNCIAS:
//! - Onde o C++ lê `realm()` do próprio `JSPromise`, o chamador passa o host do realm da promessa
//!   (o `JSGlobalObject` da `Structure`), porque o `JSGlobalObject` do porte ainda não é um host.
//! - Exceções: o C++ usa `ThrowScope`/`TopExceptionScope` e o `VM::exception()` pendente. O porte do
//!   `JSPromise` recebe o resultado como `Result<_, Thrown>`: `Thrown::Value` é a exceção capturável
//!   (o `catchScope.exception()->value()` já limpo, ou o que o chamador lançará), e
//!   `Thrown::Termination` é a de terminação, que `clearExceptionExceptTermination()` não limpa
//!   (o `JSPromise` a deixa pendente e retorna sem mexer na promessa).

use crate::llint::LLIntFailure;
use crate::runtime::error_type::ErrorType;
use crate::runtime::host_call::Thrown as HostThrown;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::microtask::{InternalMicrotask, SynchronousModuleTask};
use crate::runtime::structure::StructureRef;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;

use crate::runtime::js_promise::JSPromise;

/// `enum class JSPromiseRejectionOperation : unsigned` (`GlobalObjectMethodTable.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JSPromiseRejectionOperation {
    /// When a promise is rejected without any handlers.
    Reject,
    /// When a handler is added to a rejected promise for the first time.
    Handle,
}

/// O desfecho excepcional de uma operação do host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Thrown {
    /// Exceção capturável com o valor lançado.
    Value(JSValue),
    /// A exceção de terminação do `VM` (`vm.terminationException()`).
    Termination,
}

/// As propriedades que o `JSPromise.cpp` lê de um objeto com `get(globalObject, ...)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseProperty {
    /// `vm.propertyNames->then`.
    Then,
    /// `vm.propertyNames->constructor`.
    Constructor,
    /// `vm.propertyNames->speciesSymbol`.
    Species,
}

/// Os `NativeExecutable` que `JSFunctionWithFields::create` recebe no `JSPromise.cpp` (os
/// `vm.promise...Executable()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseFunction {
    /// `vm.promiseCapabilityExecutorExecutable()` (corpo: `promise_capability_executor`).
    CapabilityExecutor,
    /// `vm.promiseResolvingFunctionResolveExecutable()`.
    ResolvingFunctionResolve,
    /// `vm.promiseResolvingFunctionRejectExecutable()`.
    ResolvingFunctionReject,
    /// `vm.promiseFirstResolvingFunctionResolveExecutable()`.
    FirstResolvingFunctionResolve,
    /// `vm.promiseFirstResolvingFunctionRejectExecutable()`.
    FirstResolvingFunctionReject,
    /// `vm.promiseResolvingFunctionResolveWithInternalMicrotaskExecutable()`.
    ResolvingFunctionResolveWithInternalMicrotask,
    /// `vm.promiseResolvingFunctionRejectWithInternalMicrotaskExecutable()`.
    ResolvingFunctionRejectWithInternalMicrotask,
}

/// `JSFunctionWithFields::Field`: o índice de um dos dois campos internos. Vários nomes do C++
/// compartilham o mesmo valor (um enum de Rust não admite discriminantes repetidos), então são
/// constantes associadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionField(pub u32);

impl FunctionField {
    pub const EXECUTOR_RESOLVE: FunctionField = FunctionField(0);
    pub const EXECUTOR_REJECT: FunctionField = FunctionField(1);

    pub const RESOLVING_PROMISE: FunctionField = FunctionField(0);
    pub const RESOLVING_OTHER: FunctionField = FunctionField(1);

    pub const FIRST_RESOLVING_PROMISE: FunctionField = FunctionField(0);

    pub const RESOLVING_WITH_INTERNAL_MICROTASK_CONTEXT: FunctionField = FunctionField(0);
    pub const RESOLVING_WITH_INTERNAL_MICROTASK_OTHER: FunctionField = FunctionField(1);

    pub const PROMISE_ALL_CONTEXT: FunctionField = FunctionField(0);
    pub const PROMISE_ALL_RESOLVE: FunctionField = FunctionField(1);

    pub const PROMISE_ALL_SETTLED_CONTEXT: FunctionField = FunctionField(0);
    pub const PROMISE_ALL_SETTLED_OTHER: FunctionField = FunctionField(1);

    pub const PROMISE_ANY_CONTEXT: FunctionField = FunctionField(0);
    pub const PROMISE_ANY_REJECT: FunctionField = FunctionField(1);

    pub const WEB_ASSEMBLY_SUSPENDING_WRAPPED_CALLABLE: FunctionField = FunctionField(0);
    pub const WEB_ASSEMBLY_PROMISING_WRAPPED_FUNCTION: FunctionField = FunctionField(0);
    pub const PROMISE_HANDLER_PINBALL_COMPLETION: FunctionField = FunctionField(0);
}

/// O global object (e o `VM`) vistos pelo `JSPromise`.
pub trait PromiseHost {
    /// `globalObject->vm()`.
    fn vm(&self) -> &VM;

    /// `globalObject->queueMicrotask(vm, task, payload, arg0, ...)` com até
    /// `MAX_MICROTASK_ARGUMENTS` argumentos; os que faltam ficam vazios.
    fn queue_microtask(&self, task: InternalMicrotask, payload: u8, arguments: &[JSValue]);

    /// `vm.m_synchronousModuleQueue != nullptr`.
    fn has_synchronous_module_queue(&self) -> bool;

    /// `vm.m_synchronousModuleQueue->tasks.append(task)`.
    fn append_synchronous_module_task(&self, task: SynchronousModuleTask);

    /// `globalObject->globalObjectMethodTable()->promiseRejectionTracker(globalObject, promise, op)`.
    fn promise_rejection_tracker(&self, promise: &JSPromise, operation: JSPromiseRejectionOperation);

    /// `AsyncContextSwapScope::current(vm, globalObject)`: `jsUndefined()` quando não há contexto.
    fn async_context(&self) -> JSValue;

    /// `JSValue::isCallable()`.
    fn is_callable(&self, value: JSValue) -> bool;

    /// `JSValue::isConstructor()`.
    fn is_constructor(&self, value: JSValue) -> bool;

    /// `dynamicDowncast<JSFunction>(value) != nullptr`.
    fn is_js_function(&self, value: JSValue) -> bool;

    /// `dynamicDowncast<JSFunctionWithFields>(value) != nullptr`.
    fn is_function_with_fields(&self, value: JSValue) -> bool;

    /// `promise->realm() == globalObject`.
    fn owns_promise(&self, promise: &JSPromise) -> bool;

    /// `runInternalMicrotask(globalObject, vm, task, payload, arguments)` (`JSMicrotask.cpp`): roda a
    /// tarefa na hora, sem passar pela fila.
    fn run_internal_microtask(&self, task: InternalMicrotask, payload: u8, arguments: [JSValue; 4]);

    /// `globalObject->promiseStructure()`.
    fn promise_structure(&self) -> StructureRef;

    /// `globalObject->promisePrototype()`.
    fn promise_prototype(&self) -> JSValue;

    /// `globalObject->promiseConstructor()`.
    fn promise_constructor(&self) -> JSValue;

    /// `globalObject->promiseCapabilityObjectStructure()` (a de `createPromiseCapabilityObjectStructure`).
    fn promise_capability_object_structure(&self) -> StructureRef;

    /// `globalObject->promiseThenWatchpointSet().isStillValid()`.
    fn promise_then_watchpoint_is_valid(&self) -> bool;

    /// `promiseSpeciesWatchpointIsValid(vm, promise)`.
    fn promise_species_watchpoint_is_valid(&self, promise: &JSPromise) -> bool;

    /// `globalObject->errorStructure(errorType)`.
    fn error_structure(&self, error_type: ErrorType) -> StructureRef;

    /// `object->get(globalObject, name)`, com o `catchScope` do chamador já resolvido (veja o
    /// cabeçalho do módulo sobre `Thrown`).
    fn get_property(&self, object: JSValue, property: PromiseProperty) -> Result<JSValue, Thrown>;

    /// `construct(globalObject, constructor, args, "argument is not a constructor"_s)`.
    fn construct(&self, constructor: JSValue, arguments: &[JSValue]) -> Result<JSValue, Thrown>;

    /// `call(globalObject, function, jsUndefined(), args, errorMessage)`.
    fn call(&self, function: JSValue, arguments: &[JSValue], error_message: &str) -> Result<JSValue, Thrown>;

    /// `throwTypeError(globalObject, scope, message)`: o `TypeError` que o chamador lançará.
    fn create_type_error(&self, message: &str) -> JSValue;

    /// `JSFunctionWithFields::create(vm, globalObject, executable)`: a função com os dois campos
    /// internos em `JSValue()` (vazio).
    fn create_function_with_fields(&self, function: PromiseFunction) -> JSValue;

    /// `uncheckedDowncast<JSFunctionWithFields>(function)->getField(field)`.
    fn function_field(&self, function: JSValue, field: FunctionField) -> JSValue;

    /// `uncheckedDowncast<JSFunctionWithFields>(function)->setField(vm, field, value)`.
    fn set_function_field(&self, function: JSValue, field: FunctionField, value: JSValue);

    /// `InternalFieldTuple::create(vm, globalObject->internalFieldTupleStructure(), first, second)`.
    fn create_internal_field_tuple(&self, first: JSValue, second: JSValue) -> JSValue;

    /// `value.inherits<InternalFieldTuple>()`.
    fn is_internal_field_tuple(&self, value: JSValue) -> bool;
}

/// O `catchScope.exception()->value()` de uma operação que falhou, seguido de
/// `clearExceptionExceptTermination()`: a terminação fica pendente (`Thrown::Termination`) e qualquer
/// outra exceção é limpa e devolvida como valor.
pub fn caught_exception(global_object: &JSGlobalObject) -> Thrown {
    let vm = global_object.vm();
    let exception = vm.exception().expect("operação falhou sem exceção pendente no VM");
    if vm.is_termination_exception(&exception) {
        return Thrown::Termination;
    }
    vm.clear_exception();
    Thrown::Value(exception.value())
}

/// `LLIntFailure` de `call`/`construct` como o `Thrown` do host: `Thrown` é a exceção pendente (ver
/// `caught_exception`) e as lacunas do porte são `panic!` com a mensagem (o mesmo caminho de
/// `Thrown::Unported`).
pub fn thrown_from_llint_failure(global_object: &JSGlobalObject, failure: LLIntFailure) -> Thrown {
    match failure {
        LLIntFailure::Thrown => caught_exception(global_object),
        LLIntFailure::Unported(what) => panic!("caminho ainda não portado: {what}"),
        LLIntFailure::UnportedOpcode(_) => panic!("opcode sem handler no interpretador"),
    }
}

/// Deixa o `Thrown` pendente no `VM` (o que `throwException(globalObject, scope, error)` faz) e devolve o
/// `Thrown::Pending` de uma função nativa. A terminação já está pendente.
///
/// O `Thrown::Value` quase sempre é a exceção que `caught_exception` tirou do `VM` para atravessar o código do host;
/// no C++ ela nunca saiu de lá. Quando o valor é o da última `Exception`, ela volta a ficar pendente (com a pilha do
/// `throw` que o bun lê no relato de exceção não capturada), em vez de nascer outra sem pilha.
pub fn rethrow(global_object: &JSGlobalObject, thrown: Thrown) -> HostThrown {
    if let Thrown::Value(value) = thrown {
        let vm = global_object.vm();
        match vm.last_exception().filter(|last| last.value() == value) {
            Some(last) => vm.set_exception(last),
            None => {
                let mut scope = ThrowScope::new(vm);
                throw_exception(global_object, &mut scope, value);
            }
        }
    }
    HostThrown::Pending
}

/// O resultado de uma operação de promessa como o de um corpo de função nativa (`HostResult`): o
/// `Thrown` vira exceção pendente (`rethrow`).
pub fn host_result<T>(global_object: &JSGlobalObject, result: Result<T, Thrown>) -> Result<T, HostThrown> {
    result.map_err(|thrown| rethrow(global_object, thrown))
}

/// `value.get(globalObject, name)` seguido do `catchScope.exception()` do `JSPromise.cpp` e do
/// `JSMicrotask.cpp`: o valor da propriedade, ou o `Thrown` do getter (ver `caught_exception`).
pub fn get_property_named(
    global_object: &JSGlobalObject,
    value: JSValue,
    name: &crate::runtime::identifier::Identifier,
) -> Result<JSValue, Thrown> {
    let property_name = crate::runtime::property_name::PropertyName::from_identifier(name);
    match crate::runtime::iterator_operations::get_value_property(global_object, value, &property_name) {
        Ok(property) => Ok(property),
        Err(HostThrown::Pending) => Err(caught_exception(global_object)),
        Err(HostThrown::Unported(what)) => panic!("caminho ainda não portado: {what}"),
        Err(other) => unreachable!("get_value_property só lança Pending ou Unported: {other:?}"),
    }
}
