//! Os slow paths de `llint/LLIntSlowPaths.cpp` e de `runtime/CommonSlowPaths.cpp` que o laço de
//! despacho (`dispatch.rs`) chama: `slow_path_resolve_scope`, `slow_path_get_from_scope`,
//! `slow_path_put_to_scope`, `slow_path_new_func` e os `throw_*` de `LLIntExceptions`/`ExceptionHelpers`.
//! Os aritméticos, bit a bit e de comparação vivem em `slow_paths_arith` e são reexportados daqui.
//!
//! DIVERGÊNCIAS:
//!
//! - Cada slow path recebe o [`SlowPathFrame`] e o `Op*` decodificado, e devolve `LLIntResult<()>`:
//!   `Ok(())` com o `dst` escrito, ou o [`LLIntFailure`] (a exceção já está pendente no `VM` quando é
//!   `Thrown`). O `LLINT_RETURN`/`LLINT_THROW` do C++ vira `f.set(dst, ...)`/`Err(...)`.
//! - Sem o cache de `Metadata` (`m_resolveType`, `m_structure`, `m_watchpointSet`, `m_globalLexicalBindingEpoch`):
//!   ele só alimenta o caminho rápido do `.asm`, e este porte despacha sempre pelo caminho lento, que é o
//!   que o C++ faz quando o cache está frio (a semântica é a mesma).
//! - `slow_path_get_from_scope`/`slow_path_put_to_scope` consultam a `SymbolTable` do escopo pelo nome
//!   (`JSScopeRef::symbol_table_get`/`symbol_table_put`) em vez do `offset` do operando: o `offset` é só
//!   atalho de cache do `getFromScope`/`putToScope`, e o nome dá o mesmo slot.
//! - Variáveis em escopo `with` ainda não existem (`JSWithScope` foi portado, mas o `resolve_scope` do
//!   `with` não foi conferido). O ramo `ModuleVar` é o `JSModuleEnvironment::symbol_table_get_with_imports`.

use std::rc::Rc;

pub use crate::llint::slow_paths_arith::*;

use crate::bytecode::bytecode_ops::{OpGetFromScope, OpNewFunc, OpPutToScope, OpResolveScope};
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::error::{
    create_out_of_memory_error, create_range_error, create_reference_error, create_stack_overflow_error, create_type_error,
};
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;
use crate::runtime::get_put_info::{is_initialization, ResolveMode, ResolveType};
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, JSObjectHandle, PutError};
use crate::runtime::property_name::PropertyName;
use crate::runtime::js_scope::{get_property_slot_on_object, put_on_object, JSScope, JSScopeRef};
use crate::runtime::js_symbol_table_object::SymbolTablePut;
use crate::runtime::put_property_slot::PutContext;
use crate::runtime::js_value::JSValue;
use crate::runtime::throw_scope::ThrowScope;
use crate::wtf::text::wtf_string::String as WtfString;

/// `throwException(globalObject, scope, error)` seguido do `return` de `LLINT_THROW`: deixa a exceção
/// pendente no `VM` e devolve o desfecho que o laço propaga.
pub fn throw_error_object(global_object: &JSGlobalObject, error: JSObjectHandle) -> LLIntFailure {
    let mut scope = ThrowScope::new(global_object.vm());
    scope.throw_exception(global_object, error);
    LLIntFailure::Thrown
}

/// `throwStackOverflowError(globalObject, scope)`.
pub fn throw_stack_overflow_error(global_object: &JSGlobalObject) -> LLIntFailure {
    throw_error_object(global_object, create_stack_overflow_error(global_object))
}

/// `throwOutOfMemoryError(globalObject, scope)`.
pub fn throw_out_of_memory_error(global_object: &JSGlobalObject) -> LLIntFailure {
    throw_error_object(global_object, create_out_of_memory_error(global_object))
}

/// `throwException(globalObject, scope, createReferenceError(globalObject, message))`.
pub fn throw_reference_error(global_object: &JSGlobalObject, message: &str) -> LLIntFailure {
    throw_error_object(global_object, create_reference_error(global_object, &WtfString::from_utf8(message.as_bytes())))
}

/// `throwTypeError(globalObject, scope, message)`.
pub fn throw_type_error(global_object: &JSGlobalObject, message: &str) -> LLIntFailure {
    throw_error_object(global_object, create_type_error(global_object, &WtfString::from_utf8(message.as_bytes())))
}

/// O `Thrown` de uma função do `runtime` (as que não são `JSObject::put`) como o desfecho do laço: o erro
/// é lançado no `VM` (`Pending` já está), e a lacuna do porte continua lacuna.
pub fn thrown_failure(global_object: &JSGlobalObject, thrown: Thrown) -> LLIntFailure {
    match thrown {
        Thrown::TypeError(message) => throw_type_error(global_object, &message),
        Thrown::RangeError(message) => {
            throw_error_object(global_object, create_range_error(global_object, &WtfString::from_utf8(message.as_bytes())))
        }
        Thrown::StackOverflow => throw_stack_overflow_error(global_object),
        Thrown::OutOfMemory => throw_out_of_memory_error(global_object),
        Thrown::WebAssembly(kind, message) => throw_error_object(
            global_object,
            crate::runtime::wasm_errors::create_wasm_error(global_object, kind, &WtfString::from_utf8(message.as_bytes())).as_object(),
        ),
        Thrown::Unported(what) => LLIntFailure::Unported(what),
        Thrown::Pending => LLIntFailure::Thrown,
    }
}

/// O `PutError` do `JSObject::put` como o desfecho do laço (o que o `RETURN_IF_EXCEPTION` deixa pendente).
pub fn put_error_failure(global_object: &JSGlobalObject, error: PutError) -> LLIntFailure {
    match error {
        PutError::TypeError(message) => throw_type_error(global_object, message),
        PutError::StackOverflow => throw_stack_overflow_error(global_object),
        PutError::OutOfMemory => throw_out_of_memory_error(global_object),
        PutError::RangeError(message) => {
            throw_error_object(global_object, create_range_error(global_object, &WtfString::from_utf8(message.as_bytes())))
        }
        PutError::Pending => LLIntFailure::Thrown,
        PutError::Unported(what) => LLIntFailure::Unported(what),
    }
}

/// `createUndefinedVariableError` (ramo `USE(BUN_JSC_ADDITIONS)`): o bun mede `x is not defined`.
fn variable_not_found(global_object: &JSGlobalObject, ident: &Identifier) -> LLIntFailure {
    let error = crate::runtime::exception_helpers::create_undefined_variable_error(global_object, ident);
    throw_error_object(global_object, error)
}

/// `createTDZError(globalObject, ident.string())` (LLIntSlowPaths.cpp): a sobrecarga de `StringView` não
/// troca por "uninitialized variable" quando o nome é vazio, e a mensagem termina com ponto.
fn tdz_error(global_object: &JSGlobalObject, ident: &Identifier) -> LLIntFailure {
    let name = String::from_utf8_lossy(&ident.utf8()).into_owned();
    throw_reference_error(global_object, &format!("Cannot access '{name}' before initialization."))
}

/// `callFrame->uncheckedR(reg).Register::scope()`: o `JSScope` que o registrador guarda.
pub(super) fn scope_of(value: JSValue) -> LLIntResult<JSScopeRef> {
    // `Register::scope()` é um `bitwise_cast`: o bytecode só lê como escopo um registrador que guarda um.
    debug_assert!(value.is_cell(), "registrador de escopo sem célula");
    Ok(JSScope::from_cell_id(value.as_cell()).expect("registrador de escopo sem JSScope"))
}

/// `slow_path_resolve_scope`: o escopo em que a variável vive. `ClosureVar` anda `localScopeDepth`
/// escopos (`getScope` do `.asm`); os demais tipos fazem `JSScope::resolve`, e a ausência em toda a
/// cadeia resolve para o objeto global (o `GlobalProperty` sem a propriedade, que o `get_from_scope`
/// trata).
pub fn slow_path_resolve_scope(f: &mut SlowPathFrame, op: &OpResolveScope) -> LLIntResult<()> {
    let global_object = Rc::clone(f.code_block.global_object());
    let mut scope = scope_of(f.get(op.scope))?;
    let resolved = match op.resolve_type {
        ResolveType::ClosureVar | ResolveType::ClosureVarWithVarInjectionChecks | ResolveType::ResolvedClosureVar => {
            for _ in 0..op.local_scope_depth {
                scope = scope.next().expect("localScopeDepth além da cadeia de escopos");
            }
            scope
        }
        // `Dynamic` (imports de módulo e `with`) e `ModuleVar` seguem o `JSScope::resolve` geral: o
        // `JSModuleEnvironment` resolve o import por nome (`symbol_table_get_with_imports`), o equivalente
        // do `ResolveOp` de `ModuleVar` que o C++ guarda no metadata.
        _ => {
            let ident = f.code_block.identifier(op.var as usize);
            JSScope::resolve(&global_object, &scope, &ident).unwrap_or(JSScopeRef::GlobalObject(global_object))
        }
    };
    // `JSScope::resolve` devolve o `it.get()`, o `objectAtScope` do nó: para o `with` é o objeto embrulhado (não
    // o `JSWithScope`). É ele que `f()` recebe como `this` (o `FunctionCallResolveNode` move o resultado do
    // `resolve_scope` para o `this` do `call`) e que o `get_from_scope`/`put_to_scope` consultam. O
    // `StrictEvalActivation` não embrulha objeto e segue como escopo.
    match &resolved {
        JSScopeRef::WithScope(with_scope) if !with_scope.is_strict_eval_activation() => f.set(op.dst, with_scope.object_value()),
        _ => f.set(op.dst, resolved.into_js_value()),
    }
    Ok(())
}

/// O operando de escopo que é o objeto do `with` (`JSScope::objectAtScope`): no C++ é só um `JSObject*`, aqui
/// qualquer célula de objeto que não é um `JSScope`.
fn with_object_operand(value: JSValue) -> Option<JSObjectHandle> {
    if !value.is_cell() || JSScope::from_cell_id(value.as_cell()).is_some() {
        return None;
    }
    JSObject::from_value(&value)
}

/// `slow_path_get_from_scope` (`JSScope::getFromScopeCommon`): a variável em `scope`, com a checagem de TDZ
/// e o `ReferenceError` de variável ausente quando o modo é `ThrowIfNotFound`.
pub fn slow_path_get_from_scope(f: &mut SlowPathFrame, op: &OpGetFromScope) -> LLIntResult<()> {
    let global_object = Rc::clone(f.code_block.global_object());
    let ident = f.code_block.identifier(op.var as usize);
    if let Some(object) = with_object_operand(f.get(op.scope)) {
        return match get_property_slot_on_object(&global_object, &object, object.as_value(), &ident) {
            Some(value) => {
                f.set(op.dst, value);
                Ok(())
            }
            None if op.get_put_info.resolve_mode() == ResolveMode::ThrowIfNotFound => Err(variable_not_found(&global_object, &ident)),
            None => {
                f.set(op.dst, JSValue::undefined());
                Ok(())
            }
        };
    }
    let scope = scope_of(f.get(op.scope))?;

    match scope.get_property_slot(&global_object, &ident) {
        // O slot vazio é o `jsTDZValue()` de `let`/`const`/`class` ainda não inicializado, mas só o ambiente
        // léxico global checa aqui (getFromScopeCommon). Nos demais escopos o gerador emite `op_check_tdz`, e
        // o `this` privado do contexto de arrow function nasce vazio e é lido de volta sem erro.
        Some(value) if value.is_empty() && scope.is_global_lexical_environment() => Err(tdz_error(&global_object, &ident)),
        Some(value) => {
            f.set(op.dst, value);
            Ok(())
        }
        None if op.get_put_info.resolve_mode() == ResolveMode::ThrowIfNotFound => {
            Err(variable_not_found(&global_object, &ident))
        }
        None => {
            f.set(op.dst, JSValue::undefined());
            Ok(())
        }
    }
}

/// `slow_path_put_to_scope` (`JSScope::putToScopeCommon` mais o `LLINT_PUT_TO_SCOPE` de variável global):
/// escreve na `SymbolTable` do escopo, ou na propriedade do objeto global. Variável `const` lança o
/// `TypeError` de leitura apenas; `let`/`const`/`class` ainda em TDZ lança o `ReferenceError`.
pub fn slow_path_put_to_scope(f: &mut SlowPathFrame, op: &OpPutToScope) -> LLIntResult<()> {
    let global_object = Rc::clone(f.code_block.global_object());
    let vm = global_object.vm();
    let ident = f.code_block.identifier(op.var as usize);
    let value = f.get(op.value);
    let info = op.get_put_info;
    let is_strict = info.ecma_mode().is_strict();
    let initialization = is_initialization(info.initialization_mode());
    if let Some(object) = with_object_operand(f.get(op.scope)) {
        // `hasProperty` roda sempre (um `Proxy` de `with` vê o segundo `has` antes do `set`).
        let has_property = object.has_property(vm, &PropertyName::from_identifier(&ident));
        if vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        if is_strict && info.resolve_mode() == ResolveMode::ThrowIfNotFound && !has_property {
            return Err(variable_not_found(&global_object, &ident));
        }
        return match put_on_object(&global_object, &object, object.as_value(), &ident, value, is_strict, PutContext::UnknownContext, initialization) {
            Ok(_) if vm.exception().is_some() => Err(LLIntFailure::Thrown),
            Ok(_) => Ok(()),
            Err(error) => Err(put_error_failure(&global_object, error)),
        };
    }
    let scope = scope_of(f.get(op.scope))?;

    // `ClosureVar` do LLInt (`.pClosureVar`) grava direto no `offset` do ambiente, sem consultar o
    // `ReadOnly` da entrada: é assim que `emitPushFunctionNameScope` grava o callee no escopo do nome da
    // função (variável só de leitura em modo sloppy) quando ele é capturado por `eval`, arrow, `with`...
    // Só `ResolvedClosureVar` traz o `offset` pronto do gerador; o gerador nunca emite `ClosureVar`
    // (o offset desse só existiria depois do cache do slow path), então ele não entra aqui.
    if info.resolve_type() == ResolveType::ResolvedClosureVar {
        if let JSScopeRef::LexicalEnvironment(environment) = &scope {
            crate::runtime::js_symbol_table_object::SymbolTableObjectVariables::set_variable_at(&**environment,crate::runtime::scope_offset::ScopeOffset::new(op.offset), value);
            return Ok(());
        }
    }

    if let Some(key) = ident.impl_() {
        // Só o ambiente léxico global checa TDZ aqui (LLIntSlowPaths.cpp `slow_path_put_to_scope`); nos
        // demais escopos o gerador emite `op_check_tdz`, e o `this` privado do contexto de arrow function
        // nasce vazio e é gravado sem inicialização por `emitPutThisToArrowFunctionContextScope`.
        if !initialization && scope.is_global_lexical_environment() {
            if let Some((current, _attributes)) = scope.symbol_table_get(&key) {
                if current.is_empty() {
                    return Err(tdz_error(&global_object, &ident));
                }
            }
        }
        // No objeto global o `put` é o `JSGlobalObject::put` (`symbolTablePut` com `ReadOnly` sempre valendo): a
        // inicialização só relaxa o `ReadOnly` das variáveis `const` de ambientes léxicos. Sem isso,
        // `var Infinity = 1` gravaria na entrada só de leitura (`NaN`, `Infinity`, `undefined`).
        let relaxes_read_only = initialization && !matches!(scope, JSScopeRef::GlobalObject(_));
        match scope.symbol_table_put(&key, value, is_strict, relaxes_read_only) {
            SymbolTablePut::Stored => return Ok(()),
            SymbolTablePut::ReadOnly { should_throw } => {
                return if should_throw { Err(throw_type_error(&global_object, READONLY_PROPERTY_WRITE_ERROR)) } else { Ok(()) };
            }
            SymbolTablePut::NotFound => {}
        }
    }

    // `bool hasProperty = scope->hasProperty(globalObject, ident)` roda sempre no C++ (um `Proxy` de `with`
    // vê o segundo `has` antes do `set`, o `SetMutableBinding` da especificação).
    let has_property = scope.has_property(&global_object, &ident);
    if vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    if is_strict && info.resolve_mode() == ResolveMode::ThrowIfNotFound && !has_property {
        return Err(variable_not_found(&global_object, &ident));
    }
    // `JSScopeRef::put` lança na VM o que o C++ lança (`Ok(false)` com exceção pendente) e só devolve
    // `Err` para o que ainda não foi portado.
    match scope.put(&global_object, &ident, value, is_strict, PutContext::UnknownContext, initialization) {
        Ok(_) if vm.exception().is_some() => Err(LLIntFailure::Thrown),
        Ok(_) => Ok(()),
        Err(error) => Err(put_error_failure(&global_object, error)),
    }
}

/// `slow_path_new_func`: `JSFunction::create(vm, globalObject, codeBlock->functionDecl(i), scope)`.
pub fn slow_path_new_func(f: &mut SlowPathFrame, op: &OpNewFunc) -> LLIntResult<()> {
    let global_object = Rc::clone(f.code_block.global_object());
    let scope = scope_of(f.get(op.scope))?;
    let executable = Rc::clone(f.code_block.function_decl(op.function_decl as usize));
    let function = JSFunction::create(global_object.vm(), &global_object, &executable, scope);
    f.set(op.dst, JSValue::from_cell(function.cell_id()));
    Ok(())
}
