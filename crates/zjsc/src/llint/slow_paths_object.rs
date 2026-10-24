//! Slow paths de acesso a propriedade e criação de objeto de `llint/LLIntSlowPaths.cpp` e
//! `runtime/CommonSlowPaths.cpp`: `slow_path_get_by_id`, `put_by_id`, `get_by_val`, `put_by_val`,
//! `in_by_id`, `del_by_id`, `del_by_val`,
//! `create_this`, `new_object` e os quatro de `instanceof` (`instanceof`, `instanceof_from_instanceof`,
//! `get_hasInstance_from_instanceof`, `get_prototype_from_instanceof`). Os de escopo (`resolve_scope`,
//! `get_from_scope`, `put_to_scope`) e o `op_get_scope` vivem em `slow_paths.rs` e em `dispatch.rs`.
//!
//! Modo sem inline cache: só o caminho lento, o que o C++ executa quando o cache erra. Cada função
//! recebe o [`SlowPathFrame`] de `slow_paths_arith` e o `Op*` já decodificado, e termina em
//! `Ok(())` com o `dst` escrito, ou em `Err(LLIntFailure)`: `Thrown` com a exceção pendente no `VM`
//! (o `LLINT_CHECK_EXCEPTION`/`LLINT_THROW`), ou `Unported` (ver abaixo).
//!
//! DIVERGÊNCIAS:
//!
//! - Tudo que alimenta o cache some: `metadata.m_modeMetadata`, `m_oldStructureID`, `m_structureChain`,
//!   `hitCountForLLIntCaching`, `setupGetByIdPrototypeCache`, `shouldConvertToPolyProto`,
//!   `tryCacheGetFromScopeGlobal`, `tryCachePutToScopeGlobal`, `ValueProfile` e `ArrayProfile`
//!   (`LLINT_RETURN_PROFILED`, `observeStructure`,
//!   `setOutOfBounds`): só o JIT os lê. Os atalhos `fastGetOwnProperty` e `trySetIndexQuickly` também
//!   somem: são otimizações que dão o mesmo resultado que o caminho geral.
//! - `Unported(motivo)` é a lacuna do runtime, no lugar de inventar o comportamento. Os motivos são
//!   os mesmos em toda função: (1) base primitiva (resolvido só na leitura: `get_primitive_property`, que
//!   busca no protótipo do invólucro sem criá-lo; escrita, `in`, `delete` e os handlers de `handlers_accessor`
//!   e `handlers_object` ainda passam por `object_for_access` e seguem `Unported`); (2) base que é escopo (resolvido: `CellEntry::as_js_object` entrega o escopo como `JSObject`, e `get_property`, `put_by_id`, `put_by_val`, `in_by_id`, `in_by_val` e `del_by_*` passam por `scope_base` (`SymbolTable` primeiro: `get_property_slot`, `put_to_scope_base`, `has_property`, `delete_property`) para ambiente léxico, de módulo e global léxico; a
//!   `JSFunction` tem caminho próprio em `get_property`, `put_by_id`, `put_by_val`, `in_by_id` e `del_by_*`,
//!   com o `getOwnPropertySlot` preguiçoso de `prototype`/`name`/`length`, e `object_for_access` a devolve
//!   como `ObjectRef::Function`; `object_handle_for_access` é para quem já a tratou antes); (3) `hasInstance`
//!   (chamada JS), que o `js_object.rs` ainda não tem. Já resolvidos: a base `undefined`/`null` lança
//!   `createNotAnObjectError` com o texto-fonte da instrução (`throw_not_an_object`, o `ErrorInstance`
//!   com `sourceAppender`; o `in` lança `createInvalidInParameterError`, `object_for_in`), e o
//!   `toPropertyKey` de objeto e de `Symbol` (`to_property_key`).
//! - As funções locais `to_property_key`
//!   é, no C++, `JSValue::toPropertyKey`; vive aqui até a classe ganhar o método, e sobe então.
//! - O `put` do `JSGlobalObject` (`symbolTablePut` e depois `Base::put`) é `JSGlobalObject::put`,
//!   chamado por `put_to_global_object` quando a base é o global (o registro o guarda como escopo). O
//!   `put` direto (`putDirectWithReify`) sobre o global não passa por ali, como no C++.
//! - `putDirectWithReify` fora do caminho rápido (não extensível, `DontDelete` ou classe que sobrescreve
//!   `defineOwnProperty`) é o `[[DefineOwnProperty]]` do `object_constructor`.

use crate::bytecode::bytecode_ops::{
    OpCreateThis, OpDelById, OpDelByVal, OpGetById, OpGetByVal, OpInById, OpInstanceof, OpNewObject, OpPutById, OpPutByVal,
};
use crate::bytecode::code_block::CodeBlock;
use crate::bytecode::code_type::CodeType;
use crate::bytecode::object_allocation_profile::inline_capacity_for;
use crate::bytecode::virtual_register::VirtualRegister;
use crate::llint::slow_paths::{put_error_failure, throw_error_object, throw_type_error, thrown_failure};
use crate::runtime::exception_helpers::{create_invalid_in_parameter_error, create_not_an_object_error_at, SourceSite};
use crate::llint::slow_paths_arith::SlowPathFrame;
use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::cell_registry;
use crate::runtime::delete_property_slot::DeletePropertySlot;
use crate::runtime::error_messages::{READONLY_PROPERTY_WRITE_ERROR, UNABLE_TO_DELETE_PROPERTY_ERROR};
use crate::runtime::identifier::{is_index, Identifier};
use crate::runtime::internal_function::get_function_realm;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::host_function_support::{default_has_instance, ObjectRef};
use crate::runtime::js_bound_function::object_has_instance_with_value;
use crate::runtime::js_object::{JSFinalObject, JSObject, JSObjectHandle, JSObjectRef, PutError};
use crate::runtime::js_scope::{put_on_object, JSScope, JSScopeRef};
use crate::runtime::js_symbol_table_object::SymbolTablePut;
use crate::runtime::js_type::{is_object_type, is_typed_array_type, JSType};
use crate::runtime::object_constructor::define_own_property_of;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::property_attribute::DONT_DELETE;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::{object_delete_property, ProxyObject};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// O que os macros `LLINT_BEGIN` dão a cada slow path: `vm`, `globalObject` e `codeBlock`.
pub(super) struct Ctx<'a> {
    pub(super) vm: &'a VM,
    pub(super) global_object: &'a JSGlobalObject,
    pub(super) code_block: &'a CodeBlock,
}

impl<'a> Ctx<'a> {
    pub(super) fn new(f: &SlowPathFrame<'a>) -> Ctx<'a> {
        Ctx { vm: f.vm, global_object: &**f.code_block.global_object(), code_block: f.code_block }
    }

    /// `codeBlock->identifier(index)`.
    pub(super) fn identifier(&self, index: u32) -> Identifier {
        self.code_block.identifier(index as usize)
    }
}

/// `valor.isObject()`: célula cujo `JSType` é de objeto (inclui funções e escopos, que ainda não
/// passam por `JSObject::from_value`).
fn is_object_value(value: JSValue) -> bool {
    match value {
        JSValue::Cell(cell_id) => cell_registry::cell_type(cell_id).is_some_and(is_object_type),
        _ => false,
    }
}

/// Lança o erro que `create` monta sobre `value`, com o texto-fonte da instrução em execução (o `ErrorInstance`
/// do C++ o acrescenta a partir do `topCallFrame`).
fn throw_at_site(
    f: &SlowPathFrame,
    create: fn(&JSGlobalObject, JSValue, Option<&dyn SourceSite>) -> JSObjectHandle,
    value: JSValue,
) -> LLIntFailure {
    let global_object = Ctx::new(f).global_object;
    let site = f.error_site();
    throw_error_object(global_object, create(global_object, value, Some(&site)))
}

/// `LLINT_THROW(createInvalidInParameterError(globalObject, value))`: o lado direito de um `in` que não é
/// objeto, com o texto-fonte da instrução em execução.
pub(super) fn throw_invalid_in_parameter(f: &SlowPathFrame, value: JSValue) -> LLIntFailure {
    throw_at_site(f, create_invalid_in_parameter_error, value)
}

/// `LLINT_THROW(createInvalidPrivateNameError(globalObject))`, com o texto-fonte da instrução em execução.
pub(super) fn throw_invalid_private_name(f: &SlowPathFrame) -> LLIntFailure {
    let global_object = Ctx::new(f).global_object;
    let site = f.error_site();
    throw_error_object(global_object, crate::runtime::exception_helpers::create_invalid_private_name_error(global_object, Some(&site)))
}

/// `LLINT_THROW(createTypeError(globalObject, message, defaultSourceAppender, TypeNothing))`: o erro de mensagem
/// fixa (`createRedefinedPrivateNameError`, `createPrivateMethodAccessError`, `createReinstallPrivateMethodError`)
/// com o texto-fonte da instrução em execução.
pub(super) fn throw_default_appended_type_error(f: &SlowPathFrame, message: &str) -> LLIntFailure {
    let global_object = Ctx::new(f).global_object;
    let site = f.error_site();
    throw_error_object(
        global_object,
        crate::runtime::exception_helpers::create_type_error_with_default_appender(global_object, message, Some(&site)),
    )
}

/// `throwException(globalObject, scope, createNotAnObjectError(globalObject, value))` de `synthesizePrototype`,
/// `toObjectSlowCase`, `getOwnPropertySlot` e `requireObjectCoercible` sobre `undefined`/`null`: a mensagem
/// `undefined is not an object (evaluating 'a.b')` leva o texto-fonte da instrução.
pub(super) fn throw_not_an_object(f: &SlowPathFrame, value: JSValue) -> LLIntFailure {
    throw_at_site(f, create_not_an_object_error_at, value)
}

/// A base de `in_by_id`/`in_by_val`: `!baseValue.isObject()` lança `createInvalidInParameterError`; o objeto
/// que o registro não entrega como `JSObject` segue `Unported` (`object_for_access`).
pub(super) fn object_for_in(f: &SlowPathFrame, value: JSValue) -> LLIntResult<JSObjectHandle> {
    if !value.is_object() {
        return Err(throw_invalid_in_parameter(f, value));
    }
    object_handle_for_access(f, value)
}

/// `asObject(value)` para os objetos que o registro entrega como `JSObject` ou `JSFunction`; `undefined` e
/// `null` lançam o `TypeError` de `createNotAnObjectError` com o texto-fonte, e o resto é `Unported` (ver o
/// motivo 1 e 2 do cabeçalho).
pub(super) fn object_for_access(f: &SlowPathFrame, value: JSValue) -> LLIntResult<ObjectRef> {
    if let Some(object) = ObjectRef::from_value(&value) {
        return Ok(object);
    }
    if value.is_undefined_or_null() {
        return Err(throw_not_an_object(f, value));
    }
    // Primitivo (String, Symbol, BigInt, Number, Boolean): quem precisa do protótipo usa
    // `synthesize_prototype_for_access`; os `asObject` do C++ que chegam aqui são invariantes.
    Err(LLIntFailure::Unported(if is_primitive_base(value) {
        "asObject sobre primitivo (invariante do C++)"
    } else {
        "célula que não é objeto no registro"
    }))
}

/// `JSValue::synthesizePrototype(globalObject)` para quem busca no protótipo com um `this` próprio
/// (`super.x`): objeto e função passam, `undefined`/`null` lançam `createNotAnObjectError`, e o primitivo
/// (String, Number, Boolean, Symbol, BigInt) vira o protótipo do invólucro, sem criar o invólucro.
pub(super) fn synthesize_prototype_for_access(f: &SlowPathFrame, value: JSValue) -> LLIntResult<ObjectRef> {
    if is_primitive_base(value) {
        let prototype = value.primitive_wrapper_prototype(Ctx::new(f).global_object).ok_or(LLIntFailure::Thrown)?;
        return ObjectRef::from_value(&prototype).ok_or(LLIntFailure::Unported("protótipo de primitivo que é escopo"));
    }
    object_for_access(f, value)
}

/// `object_for_access` para quem já tratou a `JSFunction` antes (o `JSFunction` tem caminho próprio em
/// `get_property`, `put_by_id`, `put_by_val`, `in_by_id`, `in_by_val` e `del_by_*`): o `JSObjectHandle`. No
/// C++ a `JSFunction` é um `JSObject` e o acesso é o mesmo, então a função que chegar aqui vira o handle da
/// própria célula (`JSObject::from_value` a aceita), sem a materialização preguiçosa de `name`/`length`/
/// `prototype`, que só os caminhos próprios da função fazem.
pub(super) fn object_handle_for_access(f: &SlowPathFrame, value: JSValue) -> LLIntResult<JSObjectHandle> {
    match object_for_access(f, value)? {
        ObjectRef::Handle(object) => Ok(object),
        ObjectRef::Function(_) => JSObject::from_value(&value)
            .ok_or(LLIntFailure::Unported("JSFunction sem JSObjectHandle no registro de células")),
    }
}

/// `baseValue.toObject(globalObject)` (o `for-in` e o `op_spread` sobre primitivo): objeto e função passam, e
/// `String`, `Number`, `Boolean`, `Symbol` e `BigInt` viram o invólucro. `undefined` e `null` lançam o
/// `TypeError` do `toObject` (`createNotAnObjectError` com o texto-fonte).
pub(super) fn to_object_for_access(f: &SlowPathFrame, value: JSValue) -> LLIntResult<ObjectRef> {
    if let Some(object) = ObjectRef::from_value(&value) {
        return Ok(object);
    }
    if value.is_undefined_or_null() {
        return Err(throw_not_an_object(f, value));
    }
    value.to_object(Ctx::new(f).global_object).ok_or(LLIntFailure::Thrown)
}

/// `object->methodTable()->put(...)` sobre um `ObjectRef`: `JSFunction::put` para função, o do objeto (e do
/// `Proxy`) para o resto.
pub(super) fn put_to_object_ref(
    ctx: &Ctx,
    object: &ObjectRef,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    match object {
        ObjectRef::Handle(object) => put_to_object(ctx, object, name, value, slot),
        ObjectRef::Function(function) => put_to_function(ctx, function, name, value, slot),
    }
}

/// `putDirectWithReify` sobre um `ObjectRef`: o ramo `isJSFunction` ou o geral.
pub(super) fn put_direct_ref_with_reify(
    ctx: &Ctx,
    object: &ObjectRef,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    match object {
        ObjectRef::Handle(object) => put_direct_with_reify(ctx, object, name, value, slot),
        ObjectRef::Function(function) => put_direct_function_with_reify(ctx, function, name, value, slot),
    }
}

/// `JSValue::tryGetAsUint32Index()`.
pub(super) fn try_get_as_uint32_index(value: JSValue) -> Option<u32> {
    if value.is_uint32() {
        let index = value.as_uint32();
        debug_assert!(is_index(index));
        return Some(index);
    }
    if value.is_number() {
        let number = value.as_number();
        // `truncateDoubleToUint64`: fora de [0, 2^64) o C++ dá 0 (`NaN` e negativos).
        let truncated = if (0.0..18_446_744_073_709_551_616.0).contains(&number) { number as u64 } else { 0 };
        let as_uint = truncated as u32;
        if f64::from(as_uint) == number && is_index(as_uint) {
            return Some(as_uint);
        }
    }
    None
}

/// `JSValue::toPropertyKey(globalObject)`.
pub(super) fn to_property_key(ctx: &Ctx, value: JSValue) -> LLIntResult<Identifier> {
    if value.is_string() {
        return Ok(Identifier::from_string(ctx.vm, &value.as_js_string().value()));
    }
    if value.is_int32() {
        return Ok(Identifier::from_i32(ctx.vm, value.as_int32()));
    }
    if value.is_double() {
        return Ok(Identifier::from_double(ctx.vm, value.as_double()));
    }
    if matches!(value, JSValue::Undefined | JSValue::Null | JSValue::Bool(_)) {
        return Ok(Identifier::from_string(ctx.vm, &value.to_wtf_string()));
    }
    // Objeto (`toPrimitive` com dica de string, que chama código do usuário) e `Symbol` (`privateName().uid()`).
    value.to_property_key(ctx.global_object).ok_or(LLIntFailure::Thrown)
}

/// `baseValue.get(globalObject, name)`.
fn get_property(f: &SlowPathFrame, base: JSValue, name: &PropertyName) -> LLIntResult<JSValue> {
    let ctx = Ctx::new(f);
    if let Some(function) = base.as_js_function() {
        // `JSFunction::getOwnPropertySlot` reifica `prototype`, `name` e `length` sob demanda.
        let value = ObjectRef::Function(function).get(ctx.global_object, name);
        return if ctx.vm.exception().is_some() { Err(LLIntFailure::Thrown) } else { Ok(value) };
    }
    if !base.is_object() && !base.is_undefined_or_null() {
        return get_primitive_property(&ctx, base, name);
    }
    if let Some(scope) = scope_base(base) {
        // `JSScope::getOwnPropertySlot` (a `SymbolTable` primeiro) e depois o `getPropertySlot` da base.
        let ident = Identifier::from_uid(ctx.vm, name.uid());
        let found = scope.get_property_slot(ctx.global_object, &ident);
        return if ctx.vm.exception().is_some() { Err(LLIntFailure::Thrown) } else { Ok(found.unwrap_or(JSValue::undefined())) };
    }
    let value = object_handle_for_access(f, base)?.get(ctx.vm, name);
    // `RETURN_IF_EXCEPTION`: o `Proxy` (revogado ou com trap que lança) deixa a exceção pendente no `VM`.
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(value)
}

/// `JSValue::get(globalObject, name)` sobre primitivo (`getPropertySlot` com o ramo de `synthesizePrototype`):
/// `length` e os índices da string respondem direto (o `JSString::getStringPropertySlot` do C++), o resto é
/// busca no protótipo do invólucro (`String.prototype`, `Number.prototype`, ...) com o primitivo como
/// `this` do slot, sem criar o invólucro. A exceção de um getter fica pendente (`Thrown`).
pub(super) fn get_primitive_property(ctx: &Ctx, base: JSValue, name: &PropertyName) -> LLIntResult<JSValue> {
    if base.is_string() {
        let string = base.as_js_string();
        if *name == ctx.vm.property_names.length {
            return Ok(JSValue::Int32(string.length() as i32));
        }
        if let Some(index) = name.parse_index() {
            if index < string.length() {
                return Ok(JSValue::from_js_string(crate::runtime::js_string::js_substring(ctx.vm, &string, index, 1)));
            }
        }
    }
    let prototype = base.primitive_wrapper_prototype(ctx.global_object).ok_or(LLIntFailure::Thrown)?;
    let mut slot = PropertySlot::new(base, InternalMethodType::Get);
    let found = match ObjectRef::from_value(&prototype) {
        Some(object) => object.get_property_slot(ctx.global_object, name, &mut slot),
        None => false,
    };
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    if !found {
        return Ok(JSValue::undefined());
    }
    let value = slot.get_value_for(name);
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(value)
}

/// `Structure::get` mais os atributos do `canPutDirectFast` (`CommonSlowPaths.h`). Os dois testes que
/// `isJSFunction` dispensa no C++ o porte não tem, então vale para função e para objeto.
fn can_put_direct_fast(vm: &VM, structure: &StructureRef, name: &PropertyName) -> bool {
    if !structure.is_structure_extensible() {
        return false;
    }
    let (_, current_attributes) = structure.get_with_attributes(vm, name);
    if current_attributes & DONT_DELETE != 0 {
        return false;
    }
    // `hasNonReifiedStaticProperties()` e `defineOwnProperty` sobrescrito: nenhum objeto do porte tem
    // tabela estática, e o único tipo que sobrescreve (`JSArray`) ainda não existe.
    true
}

/// O ramo `isJSFunction` de `putDirectWithReify`: reifica a propriedade preguiçosa antes do `putDirect`, e
/// `defineOwnProperty` de `JSFunction` quando o `putDirect` rápido não vale.
pub(super) fn put_direct_function_with_reify(
    ctx: &Ctx,
    function: &JSFunctionRef,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    if *name == ctx.vm.property_names.prototype {
        slot.disable_caching();
        if let Some(rare_data) = function.rare_data() {
            rare_data.clear(ctx.vm, "Store to prototype property of a function");
        }
    }
    function.reify_lazy_property_if_needed(ctx.global_object, name, false);
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    let structure = function.structure();
    if can_put_direct_fast(ctx.vm, &structure, name) {
        let success = function.put_direct_with_slot(ctx.vm, name, value, 0, slot);
        debug_assert!(success);
        return Ok(());
    }
    slot.disable_caching();
    let descriptor = PropertyDescriptor::new(value, 0);
    function
        .define_own_property(ctx.global_object, name, &descriptor, slot.is_strict_mode())
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    Ok(())
}

/// `function->methodTable()->put(...)`: `JSFunction::put`.
pub(super) fn put_to_function(
    ctx: &Ctx,
    function: &JSFunctionRef,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    function.put(ctx.global_object, name, value, slot).map_err(|error| put_error_failure(ctx.global_object, error))?;
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(())
}

/// `CommonSlowPaths::putDirectWithReify(vm, globalObject, baseObject, name, value, slot)`.
pub(super) fn put_direct_with_reify(
    ctx: &Ctx,
    object: &JSObjectHandle,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    // O ramo `inherits<JSFunction>()` (reificar `prototype` preguiçoso) é `put_direct_function_with_reify`.
    let structure = object.structure();
    if can_put_direct_fast(ctx.vm, &structure, name) && !overrides_define_own_property(object.type_()) {
        let success = object.put_direct_with_slot(ctx.vm, name, value, 0, slot);
        debug_assert!(success);
        return Ok(());
    }
    slot.disable_caching();
    // `baseObject->methodTable()->defineOwnProperty(..., slot.isStrictMode())`: o resultado é descartado, o
    // `throwException` já lança em modo estrito.
    let descriptor = PropertyDescriptor::new(value, 0);
    define_own_property_of(ctx.global_object, object, name, &descriptor, slot.is_strict_mode())
        .map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
    Ok(())
}

/// `classInfoForCells()->methodTable.defineOwnProperty != &JSObject::defineOwnProperty` de `canPutDirectFast`:
/// os tipos cuja classe sobrescreve `defineOwnProperty` (`JSArray`, `ProxyObject`, `JSGlobalProxy`,
/// `JSGenericTypedArrayView`, `ErrorInstance`, `RegExpObject`, `StringObject`, `JSModuleNamespaceObject`,
/// `GenericArguments`, `ClonedArguments` e `JSGlobalObject`).
fn overrides_define_own_property(type_: JSType) -> bool {
    is_typed_array_type(type_)
        || matches!(
            type_,
            JSType::ArrayType
                | JSType::DerivedArrayType
                | JSType::ProxyObjectType
                | JSType::GlobalProxyType
                | JSType::ErrorInstanceType
                | JSType::RegExpObjectType
                | JSType::StringObjectType
                | JSType::DerivedStringObjectType
                | JSType::ModuleNamespaceObjectType
                | JSType::DirectArgumentsType
                | JSType::ScopedArgumentsType
                | JSType::ClonedArgumentsType
                | JSType::GlobalObjectType
        )
}

/// `codeBlock->putByIdContext()`.
pub(super) fn put_by_id_context(code_block: &CodeBlock) -> PutContext {
    if code_block.code_type() == CodeType::EvalCode {
        PutContext::PutByIdEval
    } else {
        PutContext::PutById
    }
}

// ---------------------------------------------------------------------------------------------
// get_by_id, put_by_id, get_by_val, put_by_val
// ---------------------------------------------------------------------------------------------

/// `slow_path_get_by_id`: `baseValue.get<true>(globalObject, ident, slot)` (`performLLIntGetByID`).
pub fn slow_path_get_by_id(f: &mut SlowPathFrame, op: &OpGetById) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let ident = ctx.identifier(op.property);
    let value = get_property(f, f.get(op.base), &PropertyName::from_identifier(&ident))?;
    f.set(op.dst, value);
    Ok(())
}

/// `object->methodTable()->put(object, globalObject, name, value, slot)`: o `Proxy` responde pelo trap `set`.
fn put_to_object(
    ctx: &Ctx,
    object: &JSObjectHandle,
    name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> LLIntResult<()> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        proxy.put_with_slot(ctx.global_object, name, value, slot).map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
        return Ok(());
    }
    // `methodTable()->put`: `JSArray::put` para um `Array` (`length` por `setLength`), `JSObject::put` no resto.
    crate::runtime::js_array::put_through_method_table(ctx.vm, object, name, value, slot)
        .map_err(|error| crate::llint::handlers_object::array_failure(ctx, error))?;
    Ok(())
}

/// `globalObject->methodTable()->put(...)`: `JSGlobalObject::put` (`symbolTablePut` e depois `Base::put`)
/// quando a base é o `JSGlobalObject`, que o registro entrega como escopo e `object_for_access` não alcança.
/// `false` se a base é outra coisa.
fn put_to_global_object(ctx: &Ctx, base: JSValue, name: &PropertyName, value: JSValue, slot: &mut PutPropertySlot) -> LLIntResult<bool> {
    let JSValue::Cell(cell_id) = base else { return Ok(false) };
    let Some(JSScopeRef::GlobalObject(global_object)) = JSScope::from_cell_id(cell_id) else { return Ok(false) };
    global_object.put(ctx.vm, name, value, slot).map_err(|error| put_error_failure(ctx.global_object, error))?;
    Ok(true)
}

/// `JSValue::putToPrimitive(globalObject, propertyName, value, slot)` e `putToPrimitiveByIndex` (JSCJSValue.cpp:
/// `JSValue::put` sobre primitivo `String`, `Number`, `Boolean`, `Symbol` ou `BigInt`). Um índice só é
/// interceptado por setter ou somente leitura da cadeia do protótipo do invólucro
/// (`attemptToInterceptPutByIndexOnHoleForPrototype`, o setter recebe o primitivo como `this`); sem interceptação
/// é `typeError(shouldThrow, ReadonlyPropertyWriteError)` (`TypeError` em modo estrito, silêncio no sloppy), mesmo
/// dentro do comprimento da string. `length` de string é somente leitura do mesmo jeito. O resto é o `put` do
/// protótipo do invólucro com o `this` do slot (setter da cadeia recebe o primitivo, propriedade de dados vira
/// falha porque o receptor não é objeto: `definePropertyOnReceiver`). Serve ao LLInt e ao ramo JSONP.
pub fn put_to_primitive(global_object: &JSGlobalObject, base: JSValue, name: &PropertyName, value: JSValue, slot: &PutPropertySlot) -> LLIntResult<()> {
    let vm = global_object.vm();
    let readonly_write = || if slot.is_strict_mode() { Err(throw_type_error(global_object, READONLY_PROPERTY_WRITE_ERROR)) } else { Ok(()) };
    let is_index = name.parse_index();
    if is_index.is_none() && base.is_string() && *name == vm.property_names.length {
        return readonly_write();
    }
    let prototype = base.primitive_wrapper_prototype(global_object).ok_or(LLIntFailure::Thrown)?;
    let Some(prototype) = JSObject::from_value(&prototype) else { return Ok(()) };
    let result = match is_index {
        Some(index) => match prototype.attempt_to_intercept_put_by_index_on_hole_for_prototype(vm, base, index, value, slot.is_strict_mode()) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => return readonly_write(),
            Err(error) => Err(error),
        },
        None => {
            let ident = Identifier::from_uid(vm, name.uid());
            put_on_object(global_object, &prototype, slot.this_value(), &ident, value, slot.is_strict_mode(), slot.context(), false)
        }
    };
    match result {
        Ok(_) if vm.exception().is_some() => Err(LLIntFailure::Thrown),
        Ok(_) => Ok(()),
        Err(error) => Err(put_error_failure(global_object, error)),
    }
}

/// A base que é `JSScope` com `getOwnPropertySlot`/`put` próprios sobre a `SymbolTable`
/// (`JSLexicalEnvironment`, `JSModuleEnvironment`, `JSGlobalLexicalEnvironment`). O `JSGlobalObject` tem
/// caminho próprio (`put_to_global_object`) e o `JSWithScope` é, em `get_by_id`/`put_by_id`, o objeto vazio do
/// C++ (o objeto embrulhado só é consultado pelo `get_from_scope`/`put_to_scope`), então ambos seguem o
/// `JSObjectHandle` que `CellEntry::as_js_object` entrega.
pub(super) fn scope_base(base: JSValue) -> Option<JSScopeRef> {
    let JSValue::Cell(cell_id) = base else { return None };
    match JSScope::from_cell_id(cell_id)? {
        JSScopeRef::GlobalObject(_) | JSScopeRef::WithScope(_) => None,
        scope => Some(scope),
    }
}

/// `JSLexicalEnvironment::put` (e o de `JSModuleEnvironment` e `JSGlobalLexicalEnvironment`): `symbolTablePut`
/// primeiro (binding `const`/somente leitura lança `TypeError` em modo estrito, silêncio no sloppy) e, para o
/// que a `SymbolTable` não tem, o `JSObject::put` da base com o contexto do `slot` do chamador.
pub(super) fn put_to_scope_base(ctx: &Ctx, scope: &JSScopeRef, name: &PropertyName, value: JSValue, slot: &PutPropertySlot) -> LLIntResult<()> {
    let ident = Identifier::from_uid(ctx.vm, name.uid());
    if let Some(key) = ident.impl_() {
        match scope.symbol_table_put(&key, value, slot.is_strict_mode(), false) {
            SymbolTablePut::Stored => return Ok(()),
            SymbolTablePut::ReadOnly { should_throw } => {
                return if should_throw { Err(throw_type_error(ctx.global_object, READONLY_PROPERTY_WRITE_ERROR)) } else { Ok(()) };
            }
            SymbolTablePut::NotFound => {}
        }
    }
    scope
        .put(ctx.global_object, &ident, value, slot.is_strict_mode(), slot.context(), false)
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(())
}

/// `scope->hasProperty(globalObject, ident)` de `in_by_id`/`in_by_val` sobre a base que é escopo
/// (`getOwnPropertySlot` por `symbolTableGet` primeiro, depois o `JSObject::hasProperty`).
pub(super) fn scope_has_property(ctx: &Ctx, scope: &JSScopeRef, name: &PropertyName) -> LLIntResult<bool> {
    let ident = Identifier::from_uid(ctx.vm, name.uid());
    let found = scope.has_property(ctx.global_object, &ident);
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(found)
}

/// `true` para `String`, `Number`, `Boolean`, `Symbol` e `BigInt` (o que não é objeto nem `undefined`/`null`).
pub(super) fn is_primitive_base(value: JSValue) -> bool {
    !value.is_object() && !value.is_undefined_or_null()
}

/// `getOperand(base).toObject(globalObject)` do `delete`: o objeto, a função, ou o invólucro do primitivo.
fn object_for_delete(f: &SlowPathFrame, value: JSValue) -> LLIntResult<JSObjectHandle> {
    if is_primitive_base(value) {
        return match to_object_for_access(f, value)? {
            ObjectRef::Handle(object) => Ok(object),
            // JSValue::toObjectSlowCase (JSCJSValue.cpp): o invólucro de primitivo é sempre um objeto
            // do próprio tipo (StringObject, NumberObject...), nunca uma JSFunction.
            ObjectRef::Function(_) => unreachable!("ASSERT_NOT_REACHED: toObject de primitivo devolveu JSFunction"),
        };
    }
    object_handle_for_access(f, value)
}

/// `slow_path_put_by_id`: `putDirectWithReify` se `isDirect`, senão `baseValue.putInline`.
pub fn slow_path_put_by_id(f: &mut SlowPathFrame, op: &OpPutById) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let ident = ctx.identifier(op.property);
    let name = PropertyName::from_identifier(&ident);
    let base = f.get(op.base);
    let value = f.get(op.value);
    let mut slot = PutPropertySlot::new(base, op.flags.ecma_mode().is_strict(), put_by_id_context(ctx.code_block), false);

    if let Some(function) = base.as_js_function() {
        if op.flags.is_direct() {
            put_direct_function_with_reify(&ctx, &function, &name, value, &mut slot)?;
        } else {
            put_to_function(&ctx, &function, &name, value, &mut slot)?;
        }
        return Ok(());
    }
    if !op.flags.is_direct() && put_to_global_object(&ctx, base, &name, value, &mut slot)? {
        return Ok(());
    }
    if is_primitive_base(base) {
        return put_to_primitive(ctx.global_object, base, &name, value, &slot);
    }
    if !op.flags.is_direct() {
        if let Some(scope) = scope_base(base) {
            return put_to_scope_base(&ctx, &scope, &name, value, &slot);
        }
    }
    let object = object_handle_for_access(f, base)?;
    if op.flags.is_direct() {
        put_direct_with_reify(&ctx, &object, &name, value, &mut slot)?;
    } else {
        put_to_object(&ctx, &object, &name, value, &mut slot)?;
    }
    Ok(())
}

/// `getByVal` (`LLIntSlowPaths.cpp`) sem os atalhos de cache (ver o cabeçalho).
fn get_by_val(f: &SlowPathFrame, base: JSValue, subscript: JSValue) -> LLIntResult<JSValue> {
    let ctx = Ctx::new(f);
    if let Some(index) = try_get_as_uint32_index(subscript) {
        // `isJSString(baseValue)`: `JSString::getIndex` cai em `object_for_access` como célula não objeto.
        if let Some(function) = base.as_js_function() {
            // Índice numa função é um nome como outro qualquer (`JSFunction` não tem indexados próprios).
            return get_property(f, function.as_value(), &PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)));
        }
        if !base.is_object() && !base.is_undefined_or_null() {
            return get_primitive_property(&ctx, base, &PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)));
        }
        return Ok(object_handle_for_access(f, base)?.get_by_index(ctx.vm, index));
    }

    // `baseValue.requireObjectCoercible(globalObject)`: `undefined` e `null` lançam `createNotAnObjectError`.
    if base.is_undefined_or_null() {
        return Err(throw_not_an_object(f, base));
    }
    let property = to_property_key(&ctx, subscript)?;
    get_property(f, base, &PropertyName::from_identifier(&property))
}

/// `slow_path_get_by_val`.
pub fn slow_path_get_by_val(f: &mut SlowPathFrame, op: &OpGetByVal) -> LLIntResult<()> {
    let value = get_by_val(f, f.get(op.base), f.get(op.property))?;
    f.set(op.dst, value);
    Ok(())
}

/// `slow_path_put_by_val`: por índice (`putByIndex`) ou `baseValue.put(globalObject, property, ...)`.
pub fn slow_path_put_by_val(f: &mut SlowPathFrame, op: &OpPutByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let base = f.get(op.base);
    let subscript = f.get(op.property);
    let value = f.get(op.value);
    let is_strict_mode = op.ecma_mode.is_strict();

    if let Some(index) = try_get_as_uint32_index(subscript) {
        if let Some(function) = base.as_js_function() {
            // `JSFunction` não sobrescreve `putByIndex`: é o de `JSObject`.
            function.put_by_index(ctx.vm, index, value, is_strict_mode).map_err(|error| put_error_failure(ctx.global_object, error))?;
            return Ok(());
        }
        // `object->methodTable()->putByIndex` e `baseValue.putByIndex` (primitivo: `putToPrimitive`).
        if is_primitive_base(base) {
            let name = PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index));
            let slot = PutPropertySlot::new(base, is_strict_mode, PutContext::UnknownContext, false);
            return put_to_primitive(ctx.global_object, base, &name, value, &slot);
        }
        if let Some(scope) = scope_base(base) {
            // O índice é um nome como outro qualquer para o `put` do escopo (`SymbolTable` primeiro).
            let name = PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index));
            let slot = PutPropertySlot::new(base, is_strict_mode, PutContext::UnknownContext, false);
            return put_to_scope_base(&ctx, &scope, &name, value, &slot);
        }
        let object = object_handle_for_access(f, base)?;
        if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
            proxy
                .put_by_index_common(ctx.global_object, base, index, value, is_strict_mode)
                .map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
            return Ok(());
        }
        object.put_by_index(ctx.vm, index, value, is_strict_mode).map_err(|error| put_error_failure(ctx.global_object, error))?;
        return Ok(());
    }

    let property = to_property_key(&ctx, subscript)?;
    let name = PropertyName::from_identifier(&property);
    let mut slot = PutPropertySlot::new(base, is_strict_mode, PutContext::UnknownContext, false);
    if let Some(function) = base.as_js_function() {
        return put_to_function(&ctx, &function, &name, value, &mut slot);
    }
    if put_to_global_object(&ctx, base, &name, value, &mut slot)? {
        return Ok(());
    }
    if is_primitive_base(base) {
        return put_to_primitive(ctx.global_object, base, &name, value, &slot);
    }
    if let Some(scope) = scope_base(base) {
        return put_to_scope_base(&ctx, &scope, &name, value, &slot);
    }
    let object = object_handle_for_access(f, base)?;
    put_to_object(&ctx, &object, &name, value, &mut slot)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// in_by_id, del_by_id, del_by_val
// ---------------------------------------------------------------------------------------------

/// `slow_path_in_by_id`: `asObject(base)->hasProperty(globalObject, ident)`.
pub fn slow_path_in_by_id(f: &mut SlowPathFrame, op: &OpInById) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let ident = ctx.identifier(op.property);
    // `!baseValue.isObject()`: `createInvalidInParameterError`.
    if let Some(function) = f.get(op.base).as_js_function() {
        let found = function_has_property(&ctx, &function, &PropertyName::from_identifier(&ident))?;
        f.set(op.dst, js_boolean(found));
        return Ok(());
    }
    let name = PropertyName::from_identifier(&ident);
    let found = match scope_base(f.get(op.base)) {
        Some(scope) => scope_has_property(&ctx, &scope, &name)?,
        None => object_for_in(f, f.get(op.base))?.has_property(ctx.vm, &name),
    };
    f.set(op.dst, js_boolean(found));
    Ok(())
}

/// `function->hasProperty(globalObject, name)`: `getPropertySlot` de `JSFunction` com o tipo `HasProperty`.
pub(super) fn function_has_property(ctx: &Ctx, function: &JSFunctionRef, name: &PropertyName) -> LLIntResult<bool> {
    let mut slot = PropertySlot::new(function.as_value(), InternalMethodType::HasProperty);
    let found = function.get_property_slot(ctx.global_object, name, &mut slot);
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(found)
}

/// `JSFunction::deleteProperty` (o índice é um nome como outro qualquer).
fn delete_function_property(ctx: &Ctx, function: &JSFunctionRef, key: DeleteKey) -> LLIntResult<bool> {
    let name = match key {
        DeleteKey::Index(index) => PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)),
        DeleteKey::Name(name) => name,
    };
    let deleted = function
        .delete_property(ctx.global_object, &name, &mut DeletePropertySlot::default())
        .map_err(|error| put_error_failure(ctx.global_object, error))?;
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    Ok(deleted)
}

/// `JSCell::deleteProperty(object, globalObject, name)` e `deletePropertyByIndex` (o índice é um nome
/// como outro qualquer; o `Proxy` responde pelo trap `deleteProperty`).
fn delete_property(ctx: &Ctx, object: &JSObjectHandle, key: DeleteKey) -> LLIntResult<bool> {
    let name = match key {
        DeleteKey::Index(index) => PropertyName::from_identifier(&Identifier::from_u32(ctx.vm, index)),
        DeleteKey::Name(name) => name,
    };
    object_delete_property(ctx.global_object, object, &name).map_err(|thrown| thrown_failure(ctx.global_object, thrown))
}

/// A chave de um `delete`: o índice (`deletePropertyByIndex`) ou o nome.
enum DeleteKey {
    Index(u32),
    Name(PropertyName),
}

/// O fim comum de `del_by_id` e `del_by_val`: lança em modo estrito se não pôde apagar, senão
/// escreve o booleano.
fn finish_delete(f: &mut SlowPathFrame, ctx: &Ctx, dst: VirtualRegister, is_strict: bool, could_delete: bool) -> LLIntResult<()> {
    if !could_delete && is_strict {
        return Err(throw_type_error(ctx.global_object, UNABLE_TO_DELETE_PROPERTY_ERROR));
    }
    f.set(dst, js_boolean(could_delete));
    Ok(())
}

/// `slow_path_del_by_id`.
pub fn slow_path_del_by_id(f: &mut SlowPathFrame, op: &OpDelById) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let ident = ctx.identifier(op.property);
    // O `delete x` de identificador: a base é o escopo que `resolve_scope` devolveu, e o
    // `deleteProperty` dele consulta a `SymbolTable` (binding declarado devolve falso).
    if let JSValue::Cell(cell_id) = f.get(op.base) {
        if let Some(scope) = JSScope::from_cell_id(cell_id) {
            let could_delete = scope.delete_property(ctx.global_object, &ident).map_err(|error| put_error_failure(ctx.global_object, error))?;
            if ctx.vm.exception().is_some() {
                return Err(LLIntFailure::Thrown);
            }
            return finish_delete(f, &ctx, op.dst, op.ecma_mode.is_strict(), could_delete);
        }
    }
    // `getOperand(base).toObject(globalObject)`.
    if let Some(function) = f.get(op.base).as_js_function() {
        let could_delete = delete_function_property(&ctx, &function, DeleteKey::Name(PropertyName::from_identifier(&ident)))?;
        return finish_delete(f, &ctx, op.dst, op.ecma_mode.is_strict(), could_delete);
    }
    let object = object_for_delete(f, f.get(op.base))?;
    let could_delete = delete_property(&ctx, &object, DeleteKey::Name(PropertyName::from_identifier(&ident)))?;
    finish_delete(f, &ctx, op.dst, op.ecma_mode.is_strict(), could_delete)
}

/// `slow_path_del_by_val`.
pub fn slow_path_del_by_val(f: &mut SlowPathFrame, op: &OpDelByVal) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    // Base que é escopo (como em `del_by_id`): o `deleteProperty` dele consulta a `SymbolTable`; o índice é
    // um nome como outro qualquer.
    if let JSValue::Cell(cell_id) = f.get(op.base) {
        if let Some(scope) = JSScope::from_cell_id(cell_id) {
            let subscript = f.get(op.property);
            let ident = match try_get_as_uint32_index(subscript) {
                Some(index) => Identifier::from_u32(ctx.vm, index),
                None => to_property_key(&ctx, subscript)?,
            };
            let could_delete = scope.delete_property(ctx.global_object, &ident).map_err(|error| put_error_failure(ctx.global_object, error))?;
            if ctx.vm.exception().is_some() {
                return Err(LLIntFailure::Thrown);
            }
            return finish_delete(f, &ctx, op.dst, op.ecma_mode.is_strict(), could_delete);
        }
    }
    let function = f.get(op.base).as_js_function();
    let object = match function {
        Some(_) => None,
        None => Some(object_for_delete(f, f.get(op.base))?),
    };
    let subscript = f.get(op.property);

    // `subscript.getUInt32(i)`.
    let key = match try_get_as_uint32_index(subscript) {
        Some(index) => DeleteKey::Index(index),
        None => DeleteKey::Name(PropertyName::from_identifier(&to_property_key(&ctx, subscript)?)),
    };
    let could_delete = match (&function, &object) {
        (Some(function), _) => delete_function_property(&ctx, function, key)?,
        (None, Some(object)) => delete_property(&ctx, object, key)?,
        // Invariante: o `match` anterior só deixa `object` vazio quando há `function`.
        (None, None) => unreachable!("sem função, o objeto foi obtido acima"),
    };
    finish_delete(f, &ctx, op.dst, op.ecma_mode.is_strict(), could_delete)
}

// ---------------------------------------------------------------------------------------------
// Criação de objeto
// ---------------------------------------------------------------------------------------------

/// `constructEmptyObject(globalObject, prototype)`.
fn construct_empty_object(global_object: &JSGlobalObject, prototype: &JSObject) -> JSObjectRef {
    let structure = global_object.structure_cache().empty_object_structure_for_prototype(
        global_object,
        prototype,
        JSFinalObject::DEFAULT_INLINE_CAPACITY,
        false,
    );
    JSFinalObject::create(global_object.vm(), &structure)
}

/// `slow_path_new_object`: `constructEmptyObject(vm, metadata.m_objectAllocationProfile.structure())`.
/// O perfil nasce na linkagem com `initializeProfile(objectPrototype, inlineCapacity)`, e a
/// `Structure` dele é a do `StructureCache` para a mesma capacidade, que é a que sai daqui.
pub fn slow_path_new_object(f: &mut SlowPathFrame, op: &OpNewObject) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let prototype = ctx.global_object.object_prototype();
    let (inline_capacity, _allocator) = inline_capacity_for(ctx.vm, ctx.global_object, &prototype, op.inline_capacity, false);
    let structure =
        ctx.global_object.structure_cache().empty_object_structure_for_prototype(ctx.global_object, &prototype, inline_capacity, false);
    f.set(op.dst, JSFinalObject::create(ctx.vm, &structure).as_value());
    Ok(())
}

/// `slow_path_create_this`, o ramo geral (https://tc39.es/ecma262/#sec-getprototypefromconstructor):
/// o ramo `JSFunction::canUseAllocationProfiles` é uma otimização do mesmo resultado: o `prototype` sai do
/// `getOwnPropertySlot` preguiçoso de `JSFunction` (`get_property`). Só um `prototype` que é escopo segue
/// `Unported` (`ObjectRef` não o alcança).
/// `getFunctionRealm` do ramo sem objeto em `prototype` vem de `get_function_realm`.
pub fn slow_path_create_this(f: &mut SlowPathFrame, op: &OpCreateThis) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let constructor = f.get(op.callee);
    let proto = get_property(f, constructor, &PropertyName::from_identifier(&ctx.vm.property_names.prototype))?;

    let result = if is_object_value(proto) {
        let prototype = ObjectRef::from_value(&proto)
            // `proto.isObject()` (JSObject* prototype = asObject(proto) no C++): escopo (JSScope) não é
            // alcançável como valor de `prototype`, então toda célula-objeto aqui tem ObjectRef.
            .expect("ASSERT(proto.isObject()): prototype objeto sem ObjectRef");
        construct_empty_object(ctx.global_object, &prototype)
    } else {
        let function_global_object =
            get_function_realm(constructor).map_err(|thrown| thrown_failure(ctx.global_object, thrown))?;
        construct_empty_object(&function_global_object, &function_global_object.object_prototype())
    };
    f.set(op.dst, result.as_value());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// instanceof
// ---------------------------------------------------------------------------------------------

/// `slow_path_get_hasInstance_from_instanceof`: `constructor[Symbol.hasInstance]` para o registrador
/// temporário.
pub fn slow_path_get_has_instance_from_instanceof(f: &mut SlowPathFrame, op: &OpInstanceof) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let name = PropertyName::from_identifier(&ctx.vm.property_names.has_instance_symbol);
    let result = get_property(f, f.get(op.constructor), &name)?;
    f.set(op.has_instance_or_prototype, result);
    Ok(())
}

/// `slow_path_get_prototype_from_instanceof`: `constructor.prototype` para o registrador temporário.
pub fn slow_path_get_prototype_from_instanceof(f: &mut SlowPathFrame, op: &OpInstanceof) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let name = PropertyName::from_identifier(&ctx.vm.property_names.prototype);
    let result = get_property(f, f.get(op.constructor), &name)?;
    f.set(op.has_instance_or_prototype, result);
    Ok(())
}

/// `slow_path_instanceof_from_instanceof`: `JSObject::defaultHasInstance(value, proto)`.
pub fn slow_path_instanceof_from_instanceof(f: &mut SlowPathFrame, op: &OpInstanceof) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let result = default_has_instance(ctx.global_object, f.get(op.value), f.get(op.has_instance_or_prototype))
        .ok_or(LLIntFailure::Thrown)?;
    f.set(op.dst, js_boolean(result));
    Ok(())
}

/// `slow_path_instanceof`.
pub fn slow_path_instanceof(f: &mut SlowPathFrame, op: &OpInstanceof) -> LLIntResult<()> {
    let ctx = Ctx::new(f);
    let constructor = f.get(op.constructor);

    if !constructor.is_object() {
        return Err(throw_type_error(ctx.global_object, "Right hand side of instanceof is not an object"));
    }

    let value = f.get(op.value);
    let object = constructor.as_object();
    let has_instance_name = PropertyName::from_identifier(&ctx.vm.property_names.has_instance_symbol);
    let has_instance = object.get(ctx.global_object, &has_instance_name);
    if ctx.vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }

    // `functionProtoHasInstanceSymbolFunction()` é o valor que `Function.prototype` guarda.
    let function_proto_has_instance = ctx.global_object.function_prototype().get_direct_by_name(ctx.vm, &has_instance_name);
    let result = if has_instance != function_proto_has_instance || !object.structure().type_info().implements_default_has_instance() {
        let site = f.error_site();
        object_has_instance_with_value(ctx.global_object, constructor, value, has_instance, Some(&site))
    } else if !value.is_object() {
        Some(false)
    } else {
        let prototype = object.get(ctx.global_object, &PropertyName::from_identifier(&ctx.vm.property_names.prototype));
        if ctx.vm.exception().is_some() {
            return Err(LLIntFailure::Thrown);
        }
        default_has_instance(ctx.global_object, value, prototype)
    };
    f.set(op.dst, js_boolean(result.ok_or(LLIntFailure::Thrown)?));
    Ok(())
}

#[cfg(test)]
mod define_own_property_tests {
    use super::*;

    #[test]
    fn classes_that_override_define_own_property_skip_the_fast_put() {
        for type_ in [JSType::ArrayType, JSType::ProxyObjectType, JSType::Int32ArrayType, JSType::ErrorInstanceType] {
            assert!(overrides_define_own_property(type_), "{type_:?}");
        }
        for type_ in [JSType::FinalObjectType, JSType::DataViewType] {
            assert!(!overrides_define_own_property(type_), "{type_:?}");
        }
    }
}
