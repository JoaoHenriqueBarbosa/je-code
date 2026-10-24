//! Tradução de `runtime/CallData.h`, `CallData.cpp`, `ConstructData.h` e `ConstructData.cpp`, mais o
//! `getCallDataInline`/`getConstructDataInline` de `JSFunction` e o `getCallData`/`getConstructData` de
//! `InternalFunction`: a descrição de como uma célula é chamada (`CallData`) e os invólucros
//! `call`/`construct` que entram no interpretador.
//!
//! DIVERGÊNCIAS:
//!
//! - `struct CallData` (um `Type` e uma `union`) é o enum [`CallData`]: `None`, `Native` e `JS`. O
//!   `JSCell::getCallData(cell)` virtual vira [`get_call_data`] sobre o `JSValue`, que despacha pela
//!   entrada do `cell_registry` (`JSFunction` e `InternalFunction` são as únicas células chamáveis do
//!   porte; as demais são `JSCell::getCallData`, que devolve `None`). `JSBoundFunction`, `WebAssemblyFunction`
//!   e `ProxyObject` não existem, então `isBoundFunction` e `isWasm` são sempre falsos.
//! - O `JSObject*` que o C++ passa a `executeCall`/`executeConstruct` é o `JSValue` da célula.
//! - O desfecho é um [`LLIntResult`]: `Err(LLIntFailure::Thrown)` é o `JSValue()` vazio (ou o `nullptr`
//!   de `construct`) com a exceção pendente no `VM`, e `Err(LLIntFailure::Unported(..))` é a lacuna do
//!   porte. `construct` devolve o `JSObject*` como o `JSValue` do objeto.
//! - O `vm.interpreter` do `VM` é a alça de `VM::interpreter()`: pilha, `sp` e profundidade são
//!   compartilhados (mutabilidade interior, sem empréstimo atravessando chamada), então um `call`
//!   reentrante, vindo de dentro de um slow path (getter, setter) ou de uma função nativa (callback
//!   de `Array.prototype.map`), empilha o frame abaixo do `sp` do laço em execução, sobre a mesma
//!   pilha, como o C++.
//! - `ProfilingReason`, `profiledCall` e `profiledConstruct` dependem do `ScriptProfilingScope` e do
//!   inspetor, que não existem; ficam de fora.
//! - `call(..., returnedException)` devolve `Ok(Err(exceção))` no lugar do `NakedPtr<Exception>&`.

use std::rc::Rc;

use crate::llint::slow_paths::throw_type_error;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::construct_ability::ConstructAbility;
use crate::runtime::exception::Exception;
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::js_function::{call_host_function_as_constructor, FunctionExecutableRef, JSFunction};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::{to_tagged, TaggedNativeFunction};

/// `struct CallData`.
#[derive(Clone)]
pub enum CallData {
    /// `Type::None`: a célula não é chamável (ou construível).
    None,
    /// `Type::Native`.
    Native {
        function: TaggedNativeFunction,
        is_bound_function: bool,
        is_wasm: bool,
    },
    /// `Type::JS`.
    JS {
        function_executable: FunctionExecutableRef,
        scope: JSScopeRef,
    },
}

impl CallData {
    /// `callData.type == CallData::Type::None`.
    pub fn is_none(&self) -> bool {
        matches!(self, CallData::None)
    }

    /// Os dados de uma função nativa que não é bound nem Wasm.
    fn native(function: TaggedNativeFunction) -> CallData {
        CallData::Native { function, is_bound_function: false, is_wasm: false }
    }
}

/// `NativeExecutable::function()` ou `constructor()` da função de host.
fn host_function_for(function: &JSFunction, kind: CodeSpecializationKind) -> TaggedNativeFunction {
    match function.executable() {
        ExecutableBaseRef::Native(native) => native.borrow().native_function_for(kind),
        _ => unreachable!("função de host sem NativeExecutable"),
    }
}

/// `JSFunction::getCallDataInline`: `callData.native.isBoundFunction = thisObject->inherits<JSBoundFunction>()`.
fn function_call_data(function: &JSFunction) -> CallData {
    if function.is_host_function() {
        return CallData::Native {
            function: host_function_for(function, CodeSpecializationKind::CodeForCall),
            is_bound_function: function.as_bound_function().is_some(),
            is_wasm: false,
        };
    }
    CallData::JS {
        function_executable: function.js_executable(),
        scope: function.scope_unchecked().expect("JSFunction que não é de host sem escopo"),
    }
}

/// `JSFunction::getConstructDataInline`.
fn function_construct_data(function: &JSFunction) -> CallData {
    if function.is_host_function() {
        // `JSFunction::getConstructDataInline`: a função vinculada constrói só se o alvo (ao fim da cadeia
        // de `bind`) constrói; `Math.max.bind()` não constrói, embora o `boundFunctionConstruct` exista.
        if let Some(bound) = function.as_bound_function() {
            if !bound.can_construct() {
                return CallData::None;
            }
            return CallData::native(host_function_for(function, CodeSpecializationKind::CodeForConstruct));
        }
        let constructor = host_function_for(function, CodeSpecializationKind::CodeForConstruct);
        if constructor != to_tagged(call_host_function_as_constructor) {
            return CallData::native(constructor);
        }
        return CallData::None;
    }
    let function_executable = function.js_executable();
    if function_executable.borrow().construct_ability() == ConstructAbility::CannotConstruct {
        return CallData::None;
    }
    CallData::JS {
        function_executable,
        scope: function.scope_unchecked().expect("JSFunction que não é de host sem escopo"),
    }
}

/// `getCallDataInline(JSValue)` (`JSCellInlines.h`): `JSValue` que não é célula e célula que não sobrescreve
/// `getCallData` dão `None`.
pub fn get_call_data(value: JSValue) -> CallData {
    if !value.is_cell() {
        return CallData::None;
    }
    match cell_registry::get(value.as_cell()) {
        Some(CellEntry::Function(function)) => function_call_data(&function),
        // `InternalFunction::getCallData`.
        Some(CellEntry::InternalFunction(function)) => {
            CallData::native(function.native_function_for(CodeSpecializationKind::CodeForCall))
        }
        // `ProxyObject::getCallData` e `ProxyRevoke` (um `InternalFunction` com estado).
        Some(CellEntry::Proxy(proxy)) => proxy.get_call_data(),
        Some(CellEntry::ProxyRevoke(revoke)) => {
            CallData::native(revoke.native_function_for(CodeSpecializationKind::CodeForCall))
        }
        _ => CallData::None,
    }
}

/// `getConstructDataInline(JSValue)`.
pub fn get_construct_data(value: JSValue) -> CallData {
    if !value.is_cell() {
        return CallData::None;
    }
    match cell_registry::get(value.as_cell()) {
        Some(CellEntry::Function(function)) => function_construct_data(&function),
        Some(CellEntry::Proxy(proxy)) => proxy.get_construct_data(),
        // `InternalFunction::getConstructData`: `m_functionForConstruct != callHostFunctionAsConstructor`.
        Some(CellEntry::InternalFunction(function)) => {
            if function.construct_is_default() {
                return CallData::None;
            }
            CallData::native(function.native_function_for(CodeSpecializationKind::CodeForConstruct))
        }
        _ => CallData::None,
    }
}

/// O realm em que o callee executa: `functionScope->realm()` para JS e `function->realm()` para nativo
/// (o `globalObject` que `executeCallImpl` e `executeConstruct` calculam no começo).
pub fn realm_for_call(function: JSValue, call_data: &CallData) -> JSGlobalObjectRef {
    match call_data {
        CallData::JS { scope, .. } => scope.realm(),
        CallData::Native { .. } => {
            if let Some(function) = function.as_js_function() {
                return function.realm();
            }
            match cell_registry::get(function.as_cell()) {
                // `ProxyObject`: o `JSObject::globalObject()` é o realm da `Structure`.
                Some(CellEntry::Proxy(proxy)) => proxy.structure().realm().expect("ProxyObject sem realm na Structure"),
                Some(CellEntry::ProxyRevoke(revoke)) => revoke.global_object(),
                Some(CellEntry::InternalFunction(function)) => function.global_object(),
                _ => unreachable!("CallData::Native de célula que não é função"),
            }
        }
        CallData::None => unreachable!("realm_for_call sem CallData"),
    }
}

/// O `vm.interpreter` do `VM` para uma entrada: a alça compartilha a pilha com o laço que já esteja
/// rodando (ver o cabeçalho), então a reentrada vale em qualquer ponto.
macro_rules! with_vm_interpreter {
    ($global_object:expr, |$interpreter:ident| $body:expr) => {{
        let mut $interpreter = $global_object.vm().interpreter();
        $body
    }};
}

/// `call(globalObject, functionObject, args, errorMessage)`: `this` é a própria função.
pub fn call_as_this_with_error_message(
    global_object: &JSGlobalObject,
    function: JSValue,
    args: &[JSValue],
    error_message: &str,
) -> LLIntResult<JSValue> {
    call_with_error_message(global_object, function, function, args, error_message)
}

/// `call(globalObject, functionObject, thisValue, args, errorMessage)`.
pub fn call_with_error_message(
    global_object: &JSGlobalObject,
    function: JSValue,
    this_value: JSValue,
    args: &[JSValue],
    error_message: &str,
) -> LLIntResult<JSValue> {
    let call_data = get_call_data(function);
    if call_data.is_none() {
        return Err(throw_type_error(global_object, error_message));
    }
    call(global_object, function, &call_data, this_value, args)
}

/// `call(globalObject, functionObject, callData, thisValue, args)`.
pub fn call(
    global_object: &JSGlobalObject,
    function: JSValue,
    call_data: &CallData,
    this_value: JSValue,
    args: &[JSValue],
) -> LLIntResult<JSValue> {
    debug_assert!(!call_data.is_none(), "Expected object to be callable but received CallData::Type::None");
    debug_assert!(!this_value.is_empty(), "Expected thisValue to be non-empty. Use jsUndefined() if you meant to use undefined.");
    debug_assert!(args.iter().all(|arg| !arg.is_empty()), "arguments[i] is JSValue(). Use jsUndefined() if you meant to make it undefined.");
    with_vm_interpreter!(global_object, |interpreter| interpreter.execute_call(function, call_data, this_value, None, args))
}

/// A chamada genérica de `op_iterator_open`, `op_iterator_next` e `op_async_iterator_next` (o `callHelper` do
/// `.asm`) com o frame do chamado em `frame_base`, ver `Interpreter::call_from_llint`.
pub fn call_from_llint(
    global_object: &JSGlobalObject,
    frame_base: usize,
    function: JSValue,
    call_data: &CallData,
    this_value: JSValue,
    args: &[JSValue],
) -> LLIntResult<JSValue> {
    debug_assert!(!call_data.is_none(), "Expected object to be callable but received CallData::Type::None");
    with_vm_interpreter!(global_object, |interpreter| interpreter.call_from_llint(frame_base, function, call_data, this_value, args))
}

/// `call(globalObject, functionObject, callData, thisValue, args, returnedException)`: a exceção JS
/// pendente é limpa e devolvida em `Ok(Err(..))`; o valor de sucesso é `Ok(Ok(..))`.
pub fn call_returning_exception(
    global_object: &JSGlobalObject,
    function: JSValue,
    call_data: &CallData,
    this_value: JSValue,
    args: &[JSValue],
) -> LLIntResult<Result<JSValue, Rc<Exception>>> {
    let vm = global_object.vm();
    match call(global_object, function, call_data, this_value, args) {
        Ok(result) => {
            assert!(!result.is_empty(), "call devolveu JSValue() sem exceção pendente");
            Ok(Ok(result))
        }
        Err(LLIntFailure::Thrown) => {
            let exception = vm.exception().expect("LLIntFailure::Thrown sem exceção pendente no VM");
            vm.clear_exception();
            Ok(Err(exception))
        }
        Err(other) => Err(other),
    }
}

/// `construct(globalObject, constructorObject, args, errorMessage)`: `newTarget` é o próprio construtor.
pub fn construct_with_error_message(
    global_object: &JSGlobalObject,
    constructor: JSValue,
    args: &[JSValue],
    error_message: &str,
) -> LLIntResult<JSValue> {
    construct_with_new_target_and_error_message(global_object, constructor, constructor, args, error_message)
}

/// `construct(globalObject, constructorObject, newTarget, args, errorMessage)`.
pub fn construct_with_new_target_and_error_message(
    global_object: &JSGlobalObject,
    constructor: JSValue,
    new_target: JSValue,
    args: &[JSValue],
    error_message: &str,
) -> LLIntResult<JSValue> {
    let construct_data = get_construct_data(constructor);
    if construct_data.is_none() {
        return Err(throw_type_error(global_object, error_message));
    }
    construct(global_object, constructor, &construct_data, args, new_target)
}

/// `construct(globalObject, constructor, constructData, args, newTarget)`. Para o `newTarget` igual ao
/// construtor (a sobrecarga sem ele) o chamador passa o próprio `constructor`.
pub fn construct(
    global_object: &JSGlobalObject,
    constructor: JSValue,
    construct_data: &CallData,
    args: &[JSValue],
    new_target: JSValue,
) -> LLIntResult<JSValue> {
    debug_assert!(!construct_data.is_none());
    with_vm_interpreter!(global_object, |interpreter| interpreter.execute_construct(constructor, construct_data, args, new_target))
}
