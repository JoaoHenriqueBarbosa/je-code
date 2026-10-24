//! Porte de `runtime/ProxyObject.{h,cpp}` e `ProxyObjectInlines.h`: a célula `ProxyObject` (um
//! `JSInternalFieldObjectImpl<2>` com `Target` e `Handler`, `CellEntry::Proxy`) e todos os métodos
//! internos com as invariantes e as mensagens do C++: `getOwnPropertySlot` (`performGet`,
//! `performInternalMethodGetOwnProperty`, `performHasProperty`), `put`/`putByIndex`, `deleteProperty`,
//! `defineOwnProperty`, `getOwnPropertyNames`, `preventExtensions`, `isExtensible`, `getPrototype`,
//! `setPrototype`, e o `[[Call]]`/`[[Construct]]` (`performProxyCall`, `performProxyConstruct`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//! - O `JSObject` do porte não tem a tabela de métodos virtual (`methodTable()->get/put/...`), então a
//!   segunda metade deste módulo é o despacho: `object_get`, `object_has_property`, `object_set`,
//!   `object_delete_property`, `object_get_own_property_descriptor`, `object_define_own_property`,
//!   `object_is_extensible`, `object_prevent_extensions`,
//!   `object_set_prototype`, `object_get_own_property_names` e [`get_property_slot`] (o laço de
//!   `JSObject::getPropertySlot` que consulta o `Proxy` e o `getPrototype` sobrescrito). Cada uma
//!   despacha para o `Proxy` quando o objeto é um e, senão, para o `JSObject` comum (com `JSArray` e
//!   `JSFunction` à mão). Quem precisa do comportamento de `Proxy` (`Reflect`, `Object.*`, o `JSObject`
//!   quando ganhar o despacho) chama estas, nunca o `JSObject` direto.
//! - Os métodos por índice (`getOwnPropertySlotByIndex`, `deletePropertyByIndex`) não existem: o nome de
//!   índice é um `PropertyName` como qualquer outro e os dois caminhos do C++ convertem o índice em
//!   `Identifier` antes de chamar o mesmo código. `put_by_index_common` guarda a conversão do `put`.
//! - O cache de offsets dos traps (`m_handlerTrapsOffsetsCache`, `isHandlerTrapsCacheValid`,
//!   `m_handlerStructureID`) e o atalho `forwardsGetOwnPropertyNamesToTarget` são só otimização;
//!   `get_handler_trap` faz a consulta completa (`getPropertySlot` e o `getValue`), com a mesma ordem de
//!   efeitos observáveis.
//! - `proxyObjectStructure()`, `callableProxyObjectStructure()` e `proxyRevokeStructure()` são campos do
//!   `JSGlobalObject` (preenchidos por `init`); o `LazyProperty` do C++ nasce junto do global.
//! - `JSValue::toThis(strict)` é a identidade, exceto para objeto que herda de `JSScope` (o global, o
//!   ambiente léxico, o `with`), que vira `undefined` (`to_this_strict`).
//! - `CallFrame::globalObjectOfClosestCodeBlock` (o realm do chamador no erro de `construct` revogado) é
//!   o realm do callee: o porte tem um realm só.
//! - `vm.isSafeToRecurseSoft()` é `VM::is_safe_to_recurse` (o limite brando do `VM` nunca é gravado).

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::host_function;
use crate::runtime::call_data::{call, construct, get_call_data, get_construct_data, CallData};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::delete_property_slot::DeletePropertySlot;
use crate::runtime::error_messages::READONLY_PROPERTY_WRITE_ERROR;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::interpreter::interpreter::Interpreter;
use crate::runtime::host_call::{throw_thrown, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::indexing_type::{MAY_HAVE_INDEXED_ACCESSORS, NON_ARRAY};
use crate::runtime::iterator_operations::{call_checked, thrown_from_llint};
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_global_proxy;
use crate::runtime::js_internal_field_object_impl::JSInternalFieldObjectImpl;
use crate::runtime::js_object::{type_error, validate_and_apply_property_descriptor, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO, JSNonFinalObject};
use crate::runtime::js_type::{is_typed_array_type, JSType};
use crate::runtime::js_type_info::{
    TypeInfo, IMPLEMENTS_DEFAULT_HAS_INSTANCE, IMPLEMENTS_HAS_INSTANCE,
    INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO, OVERRIDES_GET_CALL_DATA,
    OVERRIDES_GET_OWN_PROPERTY_NAMES, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_PROTOTYPE, OVERRIDES_IS_EXTENSIBLE,
    OVERRIDES_PUT, PROHIBITS_PROPERTY_CACHING,
};
use crate::runtime::js_value::{js_null, js_undefined, JSValue};
use crate::runtime::native_function::to_tagged;
use crate::runtime::object_constructor::{
    construct_object_from_property_descriptor, identifier_to_js_value, own_descriptor, prevent_extensions,
    set_prototype_with_cycle_check, to_property_descriptor,
};
use crate::runtime::operations::same_value;
use crate::runtime::own_property_names::{get_own_property_names, get_own_property_names_with_special};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::put_property_slot::{PutContext, PutPropertySlot};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol::Symbol;
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Target = 0,
    Handler = 1,
}

/// `s_proxyAlreadyRevokedErrorMessage`.
pub const PROXY_ALREADY_REVOKED_ERROR_MESSAGE: &str =
    "Proxy has already been revoked. No more operations are allowed to be performed on it";

/// `const ClassInfo ProxyObject::s_info`.
pub static PROXY_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "ProxyObject", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

// ---------------------------------------------------------------------------------------------
// Auxiliares.
// ---------------------------------------------------------------------------------------------

/// `isNonExtensibleOrHasNonConfigurableProperties()`.
fn is_non_extensible_or_has_non_configurable_properties(target: &JSObject) -> bool {
    !target.is_structure_extensible() || target.structure().has_non_configurable_properties()
}

/// O nome da propriedade como o texto das mensagens de erro (`StringView(propertyName.uid())`).
fn property_name_text(vm: &VM, property_name: &PropertyName) -> String {
    let identifier = Identifier::from_uid(vm, property_name.uid());
    String::from_utf8_lossy(&identifier.utf8()).into_owned()
}

/// `identifierToSafePublicJSValue(vm, Identifier::fromUid(vm, propertyName.uid()))`.
fn key_value(vm: &VM, property_name: &PropertyName) -> JSValue {
    identifier_to_js_value(vm, &Identifier::from_uid(vm, property_name.uid()))
}

/// `receiver.toThis(globalObject, ECMAMode::strict())`: a identidade, salvo para um objeto que herda de
/// `JSScope` (veja o cabeçalho do módulo).
pub fn to_this_strict(value: JSValue) -> JSValue {
    if let JSValue::Cell(cell_id) = value {
        if matches!(cell_registry::get(cell_id), Some(CellEntry::Scope(_))) {
            return js_undefined();
        }
    }
    value
}

/// `Identifier::from(vm, index)` para um índice de `uint64_t` (o `getIndex` de `forEachInArrayLike`).
fn index_identifier(vm: &VM, index: u64) -> Identifier {
    match u32::try_from(index) {
        Ok(index) => Identifier::from_u32(vm, index),
        Err(_) => Identifier::from_span(vm, index.to_string().as_bytes()),
    }
}

/// `forEachInArrayLike(globalObject, arrayLikeObject, functor)` (https://tc39.es/ecma262/#sec-createlistfromarraylike):
/// o `functor` devolve `false` para parar. `max_length` é o teto de `length` aceito: acima dele a leitura nem
/// começa e o resultado é `Thrown::StackOverflow` (o `Interpreter::maxArguments` do `sizeFrameForVarargs`, que o
/// `Reflect.apply` do C++ alcança pelo `@apply`), em vez de percorrer até 2^32 índices.
fn for_each_in_array_like(
    global_object: &JSGlobalObject,
    array_like_object: &JSObject,
    max_length: u64,
    mut functor: impl FnMut(JSValue) -> Result<bool, Thrown>,
) -> Result<(), Thrown> {
    let vm = global_object.vm();
    let receiver = array_like_object.as_value();
    let length = object_get(global_object, array_like_object, &PropertyName::from_identifier(&vm.property_names.length), receiver)?;
    let length = length.to_length_checked()?;
    if length > max_length {
        return Err(Thrown::StackOverflow);
    }
    for index in 0..length {
        let name = PropertyName::from_identifier(&index_identifier(vm, index));
        let value = object_get(global_object, array_like_object, &name, receiver)?;
        if !functor(value)? {
            return Ok(());
        }
    }
    Ok(())
}

/// `CreateListFromArrayLike(object)` para `Reflect.apply`/`Reflect.construct`.
pub fn list_from_array_like(global_object: &JSGlobalObject, object: &JSObject) -> Result<Vec<JSValue>, Thrown> {
    let mut values = Vec::new();
    for_each_in_array_like(global_object, object, Interpreter::MAX_ARGUMENTS as u64, |value| {
        values.push(value);
        Ok(true)
    })?;
    Ok(values)
}

/// `JSObject::getMethod(globalObject, callData, ident, errorMessage)` para um trap do manipulador:
/// `None` se o valor é `undefined` ou `null`, `TypeError` se é qualquer outro valor não chamável.
fn get_method(global_object: &JSGlobalObject, handler: &JSObject, name: &str) -> Result<Option<JSValue>, Thrown> {
    let identifier = Identifier::from_span(global_object.vm(), name.as_bytes());
    let method = object_get(global_object, handler, &PropertyName::from_identifier(&identifier), handler.as_value())?;
    // `if (!method.isCell()) { if (method.isUndefinedOrNull()) return jsUndefined(); throwVMTypeError(...) }`:
    // um primitivo que não é `undefined`/`null` (número, booleano, string) também é um trap inválido.
    if method.is_undefined_or_null() {
        return Ok(None);
    }
    if get_call_data(method).is_none() {
        return Err(Thrown::type_error(&format!("'{name}' property of a Proxy's handler should be callable")));
    }
    Ok(Some(method))
}

/// `slot.getValue(globalObject, propertyName)`: o valor, ou o resultado do getter se o slot é de accessor.
pub(crate) fn slot_value(global_object: &JSGlobalObject, slot: &PropertySlot, property_name: &PropertyName) -> Result<JSValue, Thrown> {
    if slot.is_accessor() {
        let value = slot.getter_setter().call_getter(slot.this_value())?;
        if global_object.vm().exception().is_some() {
            return Err(Thrown::Pending);
        }
        return Ok(value);
    }
    let value = slot.get_value_for(property_name);
    if slot.is_custom() && global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(value)
}

/// `https://tc39.es/ecma262/#sec-completepropertydescriptor`.
fn complete_property_descriptor(desc: &mut PropertyDescriptor) {
    if desc.is_accessor_descriptor() {
        if !desc.getter_present() {
            desc.set_getter(js_undefined());
        }
        if !desc.setter_present() {
            desc.set_setter(js_undefined());
        }
    } else {
        if desc.value().is_empty() {
            desc.set_value(js_undefined());
        }
        if !desc.writable_present() {
            desc.set_writable(false);
        }
    }
    if !desc.enumerable_present() {
        desc.set_enumerable(false);
    }
    if !desc.configurable_present() {
        desc.set_configurable(false);
    }
}

// ---------------------------------------------------------------------------------------------
// A pilha das operações que recursam no alvo.
// ---------------------------------------------------------------------------------------------

/// O custo em bytes lógicos (ver `VM::enter_logical_frame`) de uma operação do `Proxy` que recursa no alvo: o do
/// primeiro `Proxy` da cadeia (que inclui o que a operação gasta antes dele) e o de cada `Proxy` seguinte. Medidos no
/// bun 1.4.2 com cadeias de `Proxy` sem trap: com `n` o maior número de `Proxy` que passa, `level` é
/// `5177344 / (n + 1)` e `first` fecha a conta em `5177344`, então `n` passa e `n + 1` dá `RangeError`.
#[derive(Clone, Copy)]
struct FrameCost {
    first: usize,
    level: usize,
}

/// `'x' in p`, `p.x` e `getOwnPropertyDescriptor`/`hasOwnProperty`: 12278.
const GET_COST: FrameCost = FrameCost { first: 8727, level: 421 };
/// `p.x = 1`: 8185. No fim da cadeia o `set` comum lê o descritor no receptor (o `Proxy` de fora), o que percorre a
/// cadeia de novo com `GET_COST`: por nível são 211 + 421 = 632.
const PUT_COST: FrameCost = FrameCost { first: 4635, level: 211 };
/// `Object.keys`, `Reflect.ownKeys`, `for in`: 11401.
const OWN_KEYS_COST: FrameCost = FrameCost { first: 1744, level: 454 };
/// `Object.getPrototypeOf` e `Object.isExtensible`: 39907.
const GET_PROTOTYPE_COST: FrameCost = FrameCost { first: 29470, level: 129 };
/// `Object.setPrototypeOf`: 31925.
const SET_PROTOTYPE_COST: FrameCost = FrameCost { first: 5656, level: 162 };
/// `Object.defineProperty`: 16802.
const DEFINE_COST: FrameCost = FrameCost { first: 2636, level: 308 };
/// `delete p.x`: 18779.
const DELETE_COST: FrameCost = FrameCost { first: 13394, level: 275 };
/// `Object.preventExtensions`: 39906.
const PREVENT_EXTENSIONS_COST: FrameCost = FrameCost { first: 29599, level: 129 };
/// `p()`: 7786. A função alvo, chamada no fim, gasta sua parte do primeiro custo.
const CALL_COST: FrameCost = FrameCost { first: 7440, level: 664 };
/// `new p()`: 5600. No fim da cadeia o `prototype` do `new.target` (o `Proxy` de fora) é lido, o que percorre a
/// cadeia de novo com `GET_COST`: por nível são 503 + 421 = 924.
const CONSTRUCT_COST: FrameCost = FrameCost { first: 3447, level: 503 };

thread_local! {
    /// Quantos frames de `Proxy` estão abertos: o primeiro paga `FrameCost::first`.
    static PROXY_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Um frame aberto por [`ProxyObject::enter_frame`].
struct ProxyFrame {
    _stack: crate::runtime::vm::LogicalStackFrame,
}

impl Drop for ProxyFrame {
    fn drop(&mut self) {
        PROXY_DEPTH.with(|cell| cell.set(cell.get() - 1));
    }
}

// ---------------------------------------------------------------------------------------------
// [[Call]] e [[Construct]].
// ---------------------------------------------------------------------------------------------

/// `performProxyCall`.
fn perform_proxy_call_body(global_object: &JSGlobalObject, call_frame: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let _frame = ProxyObject::enter_frame(vm, CALL_COST)?;
    let proxy = ProxyObject::from_cell_id(call_frame.callee()).expect("jsCallee de performProxyCall não é um ProxyObject");
    let handler = proxy.handler_object()?;
    let apply_method = get_method(global_object, &handler, "apply")?;
    let target = proxy.target();
    let Some(apply_method) = apply_method else {
        let call_data = get_call_data(target);
        assert!(!call_data.is_none(), "ProxyObject chamável com alvo que não é chamável");
        return call(global_object, target, &call_data, call_frame.this_value(), call_frame.arguments()).map_err(thrown_from_llint);
    };

    let arg_array = construct_array(vm, &global_object.array_structure(), call_frame.arguments());
    let arguments = [target, to_this_strict(call_frame.this_value()), arg_array.as_value()];
    ProxyObject::call_trap(global_object, apply_method, &handler, "apply", &arguments)
}

/// `performProxyConstruct`. O realm do erro de revogado e do resultado inválido é o do callee (veja o
/// cabeçalho do módulo).
fn perform_proxy_construct_body(global_object: &JSGlobalObject, call_frame: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let _frame = ProxyObject::enter_frame(vm, CONSTRUCT_COST)?;
    let proxy = ProxyObject::from_cell_id(call_frame.callee()).expect("jsCallee de performProxyConstruct não é um ProxyObject");
    let handler = proxy.handler_object()?;
    let construct_method = get_method(global_object, &handler, "construct")?;
    let target = proxy.target();
    let Some(construct_method) = construct_method else {
        let construct_data = get_construct_data(target);
        assert!(!construct_data.is_none(), "ProxyObject construtível com alvo que não é construtor");
        return construct(global_object, target, &construct_data, call_frame.arguments(), call_frame.new_target())
            .map_err(thrown_from_llint);
    };

    let arg_array = construct_array(vm, &global_object.array_structure(), call_frame.arguments());
    let arguments = [target, arg_array.as_value(), call_frame.new_target()];
    let result = ProxyObject::call_trap(global_object, construct_method, &handler, "construct", &arguments)?;
    if !result.is_object() {
        return Err(Thrown::type_error("Result from Proxy handler's 'construct' method should be an object"));
    }
    Ok(result)
}

host_function!(perform_proxy_call, perform_proxy_call_body);
host_function!(perform_proxy_construct, perform_proxy_construct_body);

// ---------------------------------------------------------------------------------------------
// A célula.
// ---------------------------------------------------------------------------------------------

/// `class ProxyObject final : public JSInternalFieldObjectImpl<2>`.
pub struct ProxyObject {
    base: JSInternalFieldObjectImpl<{ NUMBER_OF_INTERNAL_FIELDS as usize }>,
    /// `m_isCallable`.
    is_callable: Cell<bool>,
    /// `m_isConstructible`.
    is_constructible: Cell<bool>,
}

/// O `ProxyObject*`.
pub type ProxyObjectRef = Rc<ProxyObject>;

impl std::ops::Deref for ProxyObject {
    type Target = JSInternalFieldObjectImpl<{ NUMBER_OF_INTERNAL_FIELDS as usize }>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl ProxyObject {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnPropertyNames |
    /// OverridesGetPrototype | OverridesGetCallData | OverridesPut | OverridesIsExtensible |
    /// InterceptsGetOwnPropertySlotByIndexEvenWhenLengthIsNotZero | ProhibitsPropertyCaching`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_PROPERTY_NAMES
        | OVERRIDES_GET_PROTOTYPE
        | OVERRIDES_GET_CALL_DATA
        | OVERRIDES_PUT
        | OVERRIDES_IS_EXTENSIBLE
        | INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO
        | PROHIBITS_PROPERTY_CACHING;

    /// `createStructure(vm, globalObject, prototype, isCallable)` (`ProxyObjectInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue, is_callable: bool) -> StructureRef {
        let mut flags = ProxyObject::STRUCTURE_FLAGS;
        if is_callable {
            flags |= IMPLEMENTS_HAS_INSTANCE | IMPLEMENTS_DEFAULT_HAS_INSTANCE;
        }
        Structure::create_with_indexing_type(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ProxyObjectType, flags),
            &PROXY_OBJECT_S_INFO,
            NON_ARRAY | MAY_HAVE_INDEXED_ACCESSORS,
            0,
        )
    }

    /// `structureForTarget(globalObject, target)`.
    fn structure_for_target(global_object: &JSGlobalObject, target: JSValue) -> StructureRef {
        if target.is_callable() {
            global_object.callable_proxy_object_structure()
        } else {
            global_object.proxy_object_structure()
        }
    }

    /// `ProxyObject::create(globalObject, target, handler)`: o `TypeError` que o `finishCreation` lança se
    /// o alvo ou o manipulador não é objeto.
    pub fn create(global_object: &JSGlobalObject, target: JSValue, handler: JSValue) -> Result<ProxyObjectRef, Thrown> {
        let vm = global_object.vm();
        let structure = ProxyObject::structure_for_target(global_object, target);
        if !target.is_object() {
            return Err(Thrown::type_error("A Proxy's 'target' should be an Object"));
        }
        if !handler.is_object() {
            return Err(Thrown::type_error("A Proxy's 'handler' should be an Object"));
        }

        let cell_id = cell_registry::reserve();
        let proxy = Rc::new(ProxyObject {
            // `initialValues()`: `jsNull()` e `jsUndefined()`.
            base: JSInternalFieldObjectImpl::new(vm, &structure, [js_null(), js_undefined()]),
            is_callable: Cell::new(false),
            is_constructible: Cell::new(false),
        });
        proxy.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::Proxy(Rc::clone(&proxy)));
        proxy.finish_creation(vm);
        debug_assert!(proxy.type_() == JSType::ProxyObjectType);

        proxy.is_callable.set(target.is_callable());
        if proxy.is_callable.get() {
            let info = proxy.structure().type_info();
            assert!(info.implements_has_instance() && info.implements_default_has_instance());
        }
        proxy.is_constructible.set(!get_construct_data(target).is_none());

        proxy.set_internal_field(Field::Target as u32, target);
        proxy.set_internal_field(Field::Handler as u32, handler);
        Ok(proxy)
    }

    /// O `ProxyObject*` do `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<ProxyObjectRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Proxy(proxy)) => Some(proxy),
            _ => None,
        }
    }

    /// `jsDynamicCast<ProxyObject*>(value)`.
    pub fn from_value(value: &JSValue) -> Option<ProxyObjectRef> {
        match value {
            JSValue::Cell(cell_id) => ProxyObject::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `target()`: sempre um objeto.
    pub fn target(&self) -> JSValue {
        self.internal_field(Field::Target as u32)
    }

    /// `handler()`: `null` se revogado.
    pub fn handler(&self) -> JSValue {
        self.internal_field(Field::Handler as u32)
    }

    /// `m_isCallable`.
    pub fn is_callable(&self) -> bool {
        self.is_callable.get()
    }

    /// `m_isConstructible`.
    pub fn is_constructible(&self) -> bool {
        self.is_constructible.get()
    }

    /// `revoke(vm)`: o manipulador vira `null`, uma vez só.
    pub fn revoke(&self) {
        assert!(!self.handler().is_null() && self.handler().is_object());
        self.set_internal_field(Field::Handler as u32, js_null());
    }

    /// `isRevoked()`.
    pub fn is_revoked(&self) -> bool {
        self.handler().is_null()
    }

    /// `getCallData(cell)`.
    pub fn get_call_data(&self) -> CallData {
        if !self.is_callable() {
            return CallData::None;
        }
        CallData::Native { function: to_tagged(perform_proxy_call), is_bound_function: false, is_wasm: false }
    }

    /// `getConstructData(cell)`.
    pub fn get_construct_data(&self) -> CallData {
        if !self.is_constructible() {
            return CallData::None;
        }
        CallData::Native { function: to_tagged(perform_proxy_construct), is_bound_function: false, is_wasm: false }
    }

    /// O alvo como objeto.
    fn target_object(&self) -> ObjectRef {
        self.target().as_object()
    }

    /// O manipulador como objeto, ou o `TypeError` de revogado.
    fn handler_object(&self) -> Result<ObjectRef, Thrown> {
        let handler = self.handler();
        if handler.is_null() {
            return Err(Thrown::type_error(PROXY_ALREADY_REVOKED_ERROR_MESSAGE));
        }
        Ok(handler.as_object())
    }

    /// `getHandlerTrap(globalObject, handler, callData, ident, trap)` sem o cache: `None` quando o trap
    /// não existe (`undefined`/`null`), `TypeError` quando existe e não é chamável.
    fn get_handler_trap(&self, global_object: &JSGlobalObject, handler: &ObjectRef, name: &Identifier) -> Result<Option<JSValue>, Thrown> {
        let mut slot = PropertySlot::new(handler.as_value(), InternalMethodType::Get);
        let property_name = PropertyName::from_identifier(name);
        let has_property = get_property_slot(global_object, handler.as_value(), &property_name, &mut slot)?;
        if !has_property {
            return Ok(None);
        }
        let trap = slot_value(global_object, &slot, &property_name)?;
        if trap.is_undefined_or_null() {
            return Ok(None);
        }
        if get_call_data(trap).is_none() {
            let text = String::from_utf8_lossy(&name.utf8()).into_owned();
            return Err(Thrown::type_error(&format!("'{text}' property of a Proxy's handler should be callable")));
        }
        Ok(Some(trap))
    }

    /// O `Identifier` de um trap (`vm.propertyNames->get` e irmãos, ou `makeIdentifier`).
    fn trap_name(vm: &VM, name: &str) -> Identifier {
        Identifier::from_span(vm, name.as_bytes())
    }

    /// Chama o trap com o manipulador como `this`.
    fn call_trap(global_object: &JSGlobalObject, trap: JSValue, handler: &ObjectRef, name: &str, arguments: &[JSValue]) -> Result<JSValue, Thrown> {
        call_checked(
            global_object,
            trap,
            handler.as_value(),
            arguments,
            &format!("'{name}' property of a Proxy's handler should be callable"),
        )
    }

    /// `if (!vm.isSafeToRecurseSoft()) throwStackOverflowError`.
    fn check_recursion(vm: &VM) -> Result<(), Thrown> {
        if vm.is_safe_to_recurse() {
            Ok(())
        } else {
            Err(Thrown::StackOverflow)
        }
    }

    /// O `isSafeToRecurseSoft()` de uma operação que recursa no alvo: entra num frame lógico com o custo dela (ver
    /// [`FrameCost`]), que sai da conta quando o [`ProxyFrame`] morre.
    fn enter_frame(vm: &VM, cost: FrameCost) -> Result<ProxyFrame, Thrown> {
        let depth = PROXY_DEPTH.with(Cell::get);
        let bytes = if depth == 0 { cost.first } else { cost.level };
        let stack = vm.enter_logical_frame(bytes).ok_or(Thrown::StackOverflow)?;
        PROXY_DEPTH.with(|cell| cell.set(depth + 1));
        Ok(ProxyFrame { _stack: stack })
    }

    /// `getOwnPropertySlotCommon(globalObject, propertyName, slot)`: `getOwnPropertySlot` e
    /// `getOwnPropertySlotByIndex` do C++.
    pub fn get_own_property_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> Result<bool, Thrown> {
        slot.disable_caching();
        slot.set_is_tainted_by_opaque_object();

        if slot.is_vm_inquiry() {
            slot.set_value(self, 0, js_undefined());
            return Ok(false);
        }

        ProxyObject::check_recursion(global_object.vm())?;
        match slot.internal_method_type() {
            InternalMethodType::Get => self.perform_get_slot(global_object, property_name, slot),
            InternalMethodType::GetOwnProperty => self.perform_internal_method_get_own_property(global_object, property_name, slot),
            InternalMethodType::HasProperty => {
                // Nobody should rely on our value, but be safe and protect against any bad actors reading our value.
                slot.set_value(self, 0, js_undefined());
                self.perform_has_property(global_object, property_name)
            }
            InternalMethodType::VMInquiry => Ok(false),
        }
    }

    /// `performGet(globalObject, propertyName, slot)`.
    fn perform_get_slot(&self, global_object: &JSGlobalObject, property_name: &PropertyName, slot: &mut PropertySlot) -> Result<bool, Thrown> {
        let result = self.perform_get(global_object, property_name, slot.this_value())?;
        slot.set_value(self, 0, result);
        Ok(true)
    }

    /// `performProxyGet(globalObject, proxyObject, receiver, propertyName)`.
    pub fn perform_get(&self, global_object: &JSGlobalObject, property_name: &PropertyName, receiver: JSValue) -> Result<JSValue, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, GET_COST)?;
        let target = self.target_object();

        // O nome privado mora no armazenamento do próprio `Proxy` (como os campos `#privados`), sem trap.
        if property_name.is_private_name() {
            let value = self.get_direct_by_name(vm, property_name);
            return Ok(if value.is_empty() { js_undefined() } else { value });
        }

        let handler = self.handler_object()?;
        let Some(get_handler) = self.get_handler_trap(global_object, &handler, &vm.property_names.get)? else {
            // performDefaultGet.
            return object_get(global_object, &target, property_name, receiver);
        };

        let arguments = [self.target(), key_value(vm, property_name), to_this_strict(receiver)];
        let trap_result = ProxyObject::call_trap(global_object, get_handler, &handler, "get", &arguments)?;

        if target.structure().has_non_configurable_read_only_or_getter_setter_properties() {
            ProxyObject::validate_get_trap_result(global_object, trap_result, &target, property_name)?;
        }
        Ok(trap_result)
    }

    /// `validateGetTrapResult(globalObject, trapResult, target, propertyName)`.
    pub fn validate_get_trap_result(global_object: &JSGlobalObject, trap_result: JSValue, target: &JSObject, property_name: &PropertyName) -> Result<(), Thrown> {
        let Some(descriptor) = object_get_own_property_descriptor(global_object, target, property_name)? else {
            return Ok(());
        };
        if descriptor.configurable() {
            return Ok(());
        }
        if descriptor.is_data_descriptor() && !descriptor.writable() {
            if !same_value(descriptor.value(), trap_result) {
                return Err(Thrown::type_error(
                    "Proxy handler's 'get' result of a non-configurable and non-writable property should be the same value as the target's property",
                ));
            }
        } else if descriptor.is_accessor_descriptor() && descriptor.getter().is_undefined() && !trap_result.is_undefined() {
            return Err(Thrown::type_error(
                "Proxy handler's 'get' result of a non-configurable accessor property without a getter should be undefined",
            ));
        }
        Ok(())
    }

    /// `performInternalMethodGetOwnProperty(globalObject, propertyName, slot)`.
    fn perform_internal_method_get_own_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        slot: &mut PropertySlot,
    ) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, GET_COST)?;
        let target = self.target_object();

        if property_name.is_private_name() {
            return Ok(false);
        }

        let handler = self.handler_object()?;
        let Some(get_own_property_descriptor_method) =
            self.get_handler_trap(global_object, &handler, &vm.property_names.get_own_property_descriptor)?
        else {
            // performDefaultGetOwnProperty.
            return own_property_slot(global_object, self.target(), property_name, slot);
        };

        let arguments = [self.target(), key_value(vm, property_name)];
        let trap_result =
            ProxyObject::call_trap(global_object, get_own_property_descriptor_method, &handler, "getOwnPropertyDescriptor", &arguments)?;

        if trap_result.is_undefined() && !is_non_extensible_or_has_non_configurable_properties(&target) {
            return Ok(false);
        }

        if !trap_result.is_undefined() && !trap_result.is_object() {
            return Err(Thrown::type_error("result of 'getOwnPropertyDescriptor' call should either be an Object or undefined"));
        }

        let target_property_descriptor = object_get_own_property_descriptor(global_object, &target, property_name)?;
        let is_target_property_descriptor_defined = target_property_descriptor.is_some();
        let target_property_descriptor = target_property_descriptor.unwrap_or_default();

        if trap_result.is_undefined() {
            if !is_target_property_descriptor_defined {
                return Ok(false);
            }
            if !target_property_descriptor.configurable() {
                return Err(Thrown::type_error("When the result of 'getOwnPropertyDescriptor' is undefined the target must be configurable"));
            }
            if !object_is_extensible(global_object, &target)? {
                return Err(Thrown::type_error(
                    "When 'getOwnPropertyDescriptor' returns undefined, the 'target' of a Proxy should be extensible",
                ));
            }
            return Ok(false);
        }

        let is_extensible = object_is_extensible(global_object, &target)?;
        let mut trap_result_as_descriptor = to_property_descriptor(global_object, trap_result)?;
        complete_property_descriptor(&mut trap_result_as_descriptor);
        let valid = validate_and_apply_property_descriptor(
            vm,
            None,
            property_name,
            is_extensible,
            &trap_result_as_descriptor,
            is_target_property_descriptor_defined,
            &target_property_descriptor,
            false,
        )?;
        if !valid {
            return Err(Thrown::type_error("Result from 'getOwnPropertyDescriptor' fails the IsCompatiblePropertyDescriptor test"));
        }

        if !trap_result_as_descriptor.configurable() {
            if !is_target_property_descriptor_defined || target_property_descriptor.configurable() {
                return Err(Thrown::type_error(
                    "Result from 'getOwnPropertyDescriptor' can't be non-configurable when the 'target' doesn't have it as an own property or if it is a configurable own property on 'target'",
                ));
            }
            if trap_result_as_descriptor.writable_present() && !trap_result_as_descriptor.writable() && target_property_descriptor.writable() {
                return Err(Thrown::type_error(
                    "Result from 'getOwnPropertyDescriptor' can't be non-configurable and non-writable when the target's property is writable",
                ));
            }
        }

        if trap_result_as_descriptor.is_accessor_descriptor() {
            let getter_setter = trap_result_as_descriptor.slow_getter_setter(vm);
            slot.set_getter_slot(self, trap_result_as_descriptor.attributes(), getter_setter);
        } else if trap_result_as_descriptor.is_data_descriptor() && !trap_result_as_descriptor.value().is_empty() {
            slot.set_value(self, trap_result_as_descriptor.attributes(), trap_result_as_descriptor.value());
        } else {
            // We use undefined because it's the default value in object properties.
            slot.set_value(self, trap_result_as_descriptor.attributes(), js_undefined());
        }
        Ok(true)
    }

    /// `performHasProperty(globalObject, propertyName, slot)`, sem o `slot` (que só recebe um valor
    /// qualquer; ninguém lê o que o alvo escreveria nele).
    pub fn perform_has_property(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, GET_COST)?;
        let target = self.target_object();

        if property_name.is_private_name() {
            return Ok(!self.get_direct_by_name(vm, property_name).is_empty());
        }

        let handler = self.handler_object()?;
        let Some(has_method) = self.get_handler_trap(global_object, &handler, &vm.property_names.has)? else {
            // performDefaultHasProperty.
            return object_has_property(global_object, &target, property_name);
        };

        let arguments = [self.target(), key_value(vm, property_name)];
        let trap_result = ProxyObject::call_trap(global_object, has_method, &handler, "has", &arguments)?;
        if trap_result.to_boolean() {
            return Ok(true);
        }

        if is_non_extensible_or_has_non_configurable_properties(&target) {
            ProxyObject::validate_negative_has_trap_result(global_object, &target, property_name)?;
        }
        Ok(false)
    }

    /// `validateNegativeHasTrapResult(globalObject, target, propertyName)`.
    pub fn validate_negative_has_trap_result(global_object: &JSGlobalObject, target: &JSObject, property_name: &PropertyName) -> Result<(), Thrown> {
        let Some(descriptor) = object_get_own_property_descriptor(global_object, target, property_name)? else {
            return Ok(());
        };
        if !descriptor.configurable() {
            return Err(Thrown::type_error("Proxy 'has' must return 'true' for non-configurable properties"));
        }
        if !object_is_extensible(global_object, target)? {
            return Err(Thrown::type_error(
                "Proxy 'has' must return 'true' for a non-extensible 'target' object with a configurable property",
            ));
        }
        Ok(())
    }

    /// `ProxyObject::put(cell, globalObject, propertyName, value, slot)`.
    pub fn put_with_slot(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        value: JSValue,
        slot: &mut PutPropertySlot,
    ) -> Result<bool, Thrown> {
        slot.disable_caching();
        slot.set_is_tainted_by_opaque_object();
        self.perform_put(global_object, property_name, value, slot.this_value(), slot.is_strict_mode())
    }

    /// `putByIndexCommon(globalObject, thisValue, propertyName, putValue, shouldThrow)` (e `putByIndex`,
    /// com `thisValue` igual ao próprio `Proxy`).
    pub fn put_by_index_common(
        &self,
        global_object: &JSGlobalObject,
        this_value: JSValue,
        index: u32,
        put_value: JSValue,
        should_throw: bool,
    ) -> Result<bool, Thrown> {
        let name = PropertyName::from_identifier(&Identifier::from_u32(global_object.vm(), index));
        self.perform_put(global_object, &name, put_value, this_value, should_throw)
    }

    /// `performPut(globalObject, putValue, thisValue, propertyName, performDefaultPut, shouldThrow)`.
    pub fn perform_put(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        put_value: JSValue,
        this_value: JSValue,
        should_throw: bool,
    ) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, PUT_COST)?;

        if property_name.is_private_name() {
            self.put_direct(vm, property_name, put_value, 0);
            return Ok(true);
        }

        let handler = self.handler_object()?;
        let set_method = self.get_handler_trap(global_object, &handler, &vm.property_names.set)?;
        let target = self.target_object();
        let Some(set_method) = set_method else {
            // performDefaultPut: `PutPropertySlot(slot.thisValue(), isStrictMode)`.
            return object_set(global_object, &target, property_name, put_value, this_value, should_throw);
        };

        let arguments = [self.target(), key_value(vm, property_name), put_value, to_this_strict(this_value)];
        let trap_result = ProxyObject::call_trap(global_object, set_method, &handler, "set", &arguments)?;
        if !trap_result.to_boolean() {
            if should_throw {
                return Err(Thrown::type_error(&format!(
                    "Proxy object's 'set' trap returned falsy value for property '{}'",
                    property_name_text(vm, property_name)
                )));
            }
            return Ok(false);
        }

        if target.structure().has_non_configurable_read_only_or_getter_setter_properties() {
            ProxyObject::validate_positive_set_trap_result(global_object, &target, property_name, put_value)?;
        }
        Ok(true)
    }

    /// `validatePositiveSetTrapResult(globalObject, target, propertyName, putValue)`.
    pub fn validate_positive_set_trap_result(
        global_object: &JSGlobalObject,
        target: &JSObject,
        property_name: &PropertyName,
        put_value: JSValue,
    ) -> Result<(), Thrown> {
        let Some(descriptor) = object_get_own_property_descriptor(global_object, target, property_name)? else {
            return Ok(());
        };
        if descriptor.configurable() {
            return Ok(());
        }
        if descriptor.is_data_descriptor() && !descriptor.writable() {
            if !same_value(descriptor.value(), put_value) {
                return Err(Thrown::type_error(
                    "Proxy handler's 'set' on a non-configurable and non-writable property on 'target' should either return false or be the same value already on the 'target'",
                ));
            }
        } else if descriptor.is_accessor_descriptor() && descriptor.setter().is_undefined() {
            return Err(Thrown::type_error(
                "Proxy handler's 'set' method on a non-configurable accessor property without a setter should return false",
            ));
        }
        Ok(())
    }

    /// `performDelete(globalObject, propertyName, performDefaultDelete)`: `deleteProperty` e
    /// `deletePropertyByIndex` do C++.
    pub fn perform_delete(&self, global_object: &JSGlobalObject, property_name: &PropertyName) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, DELETE_COST)?;

        // O nome privado sai do armazenamento do próprio `Proxy` (o `JSObject::delete_property` não despacha ao trap
        // para ele).
        if property_name.is_private_name() {
            let object: &JSObject = self;
            return object
                .delete_property(vm, property_name, &mut crate::runtime::delete_property_slot::DeletePropertySlot::default())
                .map_err(|_| Thrown::type_error(crate::runtime::error_messages::UNABLE_TO_DELETE_PROPERTY_ERROR));
        }

        let handler = self.handler_object()?;
        let delete_property_method = get_method(global_object, &handler, "deleteProperty")?;
        let target = self.target_object();
        let Some(delete_property_method) = delete_property_method else {
            // performDefaultDelete.
            return object_delete_property(global_object, &target, property_name);
        };

        let arguments = [self.target(), key_value(vm, property_name)];
        let trap_result = ProxyObject::call_trap(global_object, delete_property_method, &handler, "deleteProperty", &arguments)?;
        if !trap_result.to_boolean() {
            return Ok(false);
        }

        if !is_non_extensible_or_has_non_configurable_properties(&target) {
            return Ok(true);
        }

        if let Some(descriptor) = object_get_own_property_descriptor(global_object, &target, property_name)? {
            if !descriptor.configurable() {
                return Err(Thrown::type_error(
                    "Proxy handler's 'deleteProperty' method should return false when the target's property is not configurable",
                ));
            }
            if !object_is_extensible(global_object, &target)? {
                return Err(Thrown::type_error(
                    "Proxy handler's 'deleteProperty' method should return false when the target has property and is not extensible",
                ));
            }
        }
        Ok(true)
    }

    /// `performPreventExtensions(globalObject)`.
    pub fn perform_prevent_extensions(&self, global_object: &JSGlobalObject) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, PREVENT_EXTENSIONS_COST)?;

        let handler = self.handler_object()?;
        let prevent_extensions_method = get_method(global_object, &handler, "preventExtensions")?;
        let target = self.target_object();
        let Some(prevent_extensions_method) = prevent_extensions_method else {
            return object_prevent_extensions(global_object, &target);
        };

        let trap_result =
            ProxyObject::call_trap(global_object, prevent_extensions_method, &handler, "preventExtensions", &[self.target()])?;
        let trap_result_as_bool = trap_result.to_boolean();

        if trap_result_as_bool && object_is_extensible(global_object, &target)? {
            return Err(Thrown::type_error(
                "Proxy's 'preventExtensions' trap returned true even though its target is extensible. It should have returned false",
            ));
        }
        Ok(trap_result_as_bool)
    }

    /// `performIsExtensible(globalObject)`.
    pub fn perform_is_extensible(&self, global_object: &JSGlobalObject) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, GET_PROTOTYPE_COST)?;

        let handler = self.handler_object()?;
        let is_extensible_method = get_method(global_object, &handler, "isExtensible")?;

        let target = self.target_object();
        let Some(is_extensible_method) = is_extensible_method else {
            return object_is_extensible(global_object, &target);
        };

        let trap_result = ProxyObject::call_trap(global_object, is_extensible_method, &handler, "isExtensible", &[self.target()])?;
        let trap_result_as_bool = trap_result.to_boolean();

        let is_target_extensible = object_is_extensible(global_object, &target)?;
        if trap_result_as_bool != is_target_extensible {
            if is_target_extensible {
                return Err(Thrown::type_error(
                    "Proxy object's 'isExtensible' trap returned false when the target is extensible. It should have returned true",
                ));
            }
            return Err(Thrown::type_error(
                "Proxy object's 'isExtensible' trap returned true when the target is non-extensible. It should have returned false",
            ));
        }
        Ok(trap_result_as_bool)
    }

    /// `performDefineOwnProperty(globalObject, propertyName, descriptor, shouldThrow)`.
    pub fn perform_define_own_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        should_throw: bool,
    ) -> Result<bool, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, DEFINE_COST)?;

        let target = self.target_object();

        if property_name.is_private_name() {
            return Ok(false);
        }

        let handler = self.handler_object()?;
        let Some(define_property_method) = get_method(global_object, &handler, "defineProperty")? else {
            // performDefaultDefineOwnProperty.
            return object_define_own_property(global_object, &target, property_name, descriptor, should_throw);
        };

        let descriptor_object = construct_object_from_property_descriptor(global_object, descriptor);
        let arguments = [self.target(), key_value(vm, property_name), descriptor_object.as_value()];
        let trap_result = ProxyObject::call_trap(global_object, define_property_method, &handler, "defineProperty", &arguments)?;

        if !trap_result.to_boolean() {
            if should_throw {
                return Err(Thrown::type_error(&format!(
                    "Proxy's 'defineProperty' trap returned falsy value for property '{}'",
                    property_name_text(vm, property_name)
                )));
            }
            return Ok(false);
        }

        let setting_configurable_to_false = descriptor.configurable_present() && !descriptor.configurable();
        if !setting_configurable_to_false && !is_non_extensible_or_has_non_configurable_properties(&target) {
            return Ok(true);
        }

        let target_descriptor = object_get_own_property_descriptor(global_object, &target, property_name)?;
        let target_is_extensible = object_is_extensible(global_object, &target)?;

        let Some(target_descriptor) = target_descriptor else {
            if !target_is_extensible {
                return Err(Thrown::type_error(
                    "Proxy's 'defineProperty' trap returned true even though getOwnPropertyDescriptor of the Proxy's target returned undefined and the target is non-extensible",
                ));
            }
            if setting_configurable_to_false {
                return Err(Thrown::type_error(
                    "Proxy's 'defineProperty' trap returned true for a non-configurable field even though getOwnPropertyDescriptor of the Proxy's target returned undefined",
                ));
            }
            return Ok(true);
        };

        let is_compatible_descriptor =
            validate_and_apply_property_descriptor(vm, None, property_name, target_is_extensible, descriptor, true, &target_descriptor, false)?;
        if !is_compatible_descriptor {
            return Err(Thrown::type_error(
                "Proxy's 'defineProperty' trap did not define a property on its target that is compatible with the trap's input descriptor",
            ));
        }
        if setting_configurable_to_false && target_descriptor.configurable() {
            return Err(Thrown::type_error(
                "Proxy's 'defineProperty' trap did not define a non-configurable property on its target even though the input descriptor to the trap said it must do so",
            ));
        }
        if target_descriptor.is_data_descriptor()
            && !target_descriptor.configurable()
            && target_descriptor.writable()
            && descriptor.writable_present()
            && !descriptor.writable()
        {
            return Err(Thrown::type_error(
                "Proxy's 'defineProperty' trap returned true for a non-writable input descriptor when the target's property is non-configurable and writable",
            ));
        }
        Ok(true)
    }

    /// `ProxyObject::getOwnPropertyNames(object, globalObject, propertyNames, mode)`.
    pub fn get_own_property_names(
        &self,
        global_object: &JSGlobalObject,
        property_names: &mut PropertyNameArrayBuilder<'_>,
        mode: DontEnumPropertiesMode,
    ) -> Result<(), Thrown> {
        match mode {
            DontEnumPropertiesMode::Include => self.perform_get_own_property_names(global_object, property_names),
            DontEnumPropertiesMode::Exclude => self.perform_get_own_enumerable_property_names(global_object, property_names),
        }
    }

    /// `performGetOwnPropertyNames(globalObject, propertyNames)`: o `[[OwnPropertyKeys]]` do `Proxy`.
    pub fn perform_get_own_property_names(&self, global_object: &JSGlobalObject, property_names: &mut PropertyNameArrayBuilder<'_>) -> Result<(), Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, OWN_KEYS_COST)?;

        let handler = self.handler_object()?;
        let own_keys_method = self.get_handler_trap(global_object, &handler, &ProxyObject::trap_name(vm, "ownKeys"))?;
        let target = self.target_object();
        let Some(own_keys_method) = own_keys_method else {
            return object_get_own_property_names(global_object, &target, property_names, DontEnumPropertiesMode::Include);
        };

        let trap_result = ProxyObject::call_trap(global_object, own_keys_method, &handler, "ownKeys", &[self.target()])?;
        if !trap_result.is_object() {
            return Err(Thrown::type_error("Proxy handler's 'ownKeys' method must return an object"));
        }

        let mut unchecked_result_keys: HashSet<UniquedKey> = HashSet::new();
        let trap_result_object = trap_result.as_object();
        for_each_in_array_like(global_object, &trap_result_object, u64::MAX, |value| {
            if !value.is_string() && !(value.is_cell() && Symbol::from_cell_id(value.as_cell()).is_some()) {
                return Err(Thrown::type_error(
                    "Proxy handler's 'ownKeys' method must return an array-like object containing only Strings and Symbols",
                ));
            }

            let identifier = value.to_property_key(global_object).ok_or(Thrown::Pending)?;
            // `toPropertyKey` de String ou Symbol sempre dá um `Identifier` com `StringImpl` (a vazia é
            // `emptyIdentifier`); o C++ usa `ident.impl()` direto como chave do `HashSet`.
            let key = identifier.impl_().expect("chave de ownKeys sem StringImpl");
            if !unchecked_result_keys.insert(key) {
                return Err(Thrown::type_error("Proxy handler's 'ownKeys' trap result must not contain any duplicate names"));
            }

            property_names.add(&identifier);
            Ok(true)
        })?;

        if !is_non_extensible_or_has_non_configurable_properties(&target) {
            return Ok(());
        }

        let target_is_extensible = object_is_extensible(global_object, &target)?;

        let mut target_keys = PropertyNameArrayBuilder::new(vm, PropertyNameMode::StringsAndSymbols, PrivateSymbolMode::Exclude);
        object_get_own_property_names(global_object, &target, &mut target_keys, DontEnumPropertiesMode::Include)?;
        let mut target_non_configurable_keys: Vec<Identifier> = Vec::new();
        let mut target_configurable_keys: Vec<Identifier> = Vec::new();
        for identifier in target_keys.iter() {
            let descriptor = object_get_own_property_descriptor(global_object, &target, &PropertyName::from_identifier(identifier))?;
            if descriptor.is_some_and(|descriptor| !descriptor.configurable()) {
                target_non_configurable_keys.push(identifier.clone());
            } else if !target_is_extensible {
                target_configurable_keys.push(identifier.clone());
            }
        }

        let take = |keys: &mut HashSet<UniquedKey>, identifier: &Identifier| identifier.impl_().is_some_and(|key| keys.remove(&key));

        for identifier in &target_non_configurable_keys {
            if !take(&mut unchecked_result_keys, identifier) {
                return Err(Thrown::type_error(&format!(
                    "Proxy object's 'target' has the non-configurable property '{}' that was not in the result from the 'ownKeys' trap",
                    String::from_utf8_lossy(&identifier.utf8())
                )));
            }
        }

        if !target_is_extensible {
            for identifier in &target_configurable_keys {
                if !take(&mut unchecked_result_keys, identifier) {
                    return Err(Thrown::type_error(&format!(
                        "Proxy object's non-extensible 'target' has configurable property '{}' that was not in the result from the 'ownKeys' trap",
                        String::from_utf8_lossy(&identifier.utf8())
                    )));
                }
            }

            if !unchecked_result_keys.is_empty() {
                return Err(Thrown::type_error(
                    "Proxy handler's 'ownKeys' method returned a key that was not present in its non-extensible target",
                ));
            }
        }
        Ok(())
    }

    /// `performGetOwnEnumerablePropertyNames(globalObject, propertyNames)`.
    fn perform_get_own_enumerable_property_names(
        &self,
        global_object: &JSGlobalObject,
        property_names: &mut PropertyNameArrayBuilder<'_>,
    ) -> Result<(), Thrown> {
        let vm = global_object.vm();
        let mut unfiltered_names = PropertyNameArrayBuilder::new(vm, property_names.property_name_mode(), property_names.private_symbol_mode());
        self.perform_get_own_property_names(global_object, &mut unfiltered_names)?;
        // Filtering DontEnum properties is observable in proxies and must occur after the invariant checks pass.
        for identifier in unfiltered_names.iter() {
            let mut slot = PropertySlot::new(self.as_value(), InternalMethodType::GetOwnProperty);
            let is_property_defined = self.get_own_property_slot(global_object, &PropertyName::from_identifier(identifier), &mut slot)?;
            if !is_property_defined || slot.attributes() & DONT_ENUM != 0 {
                continue;
            }
            property_names.add(identifier);
        }
        Ok(())
    }

    /// `performSetPrototype(globalObject, prototype, shouldThrowIfCantSet)`.
    pub fn perform_set_prototype(&self, global_object: &JSGlobalObject, prototype: JSValue, should_throw_if_cant_set: bool) -> Result<bool, Thrown> {
        debug_assert!(prototype.is_object() || prototype.is_null());

        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, SET_PROTOTYPE_COST)?;

        let handler = self.handler_object()?;
        let set_prototype_of_method = get_method(global_object, &handler, "setPrototypeOf")?;

        let target = self.target_object();
        let Some(set_prototype_of_method) = set_prototype_of_method else {
            return object_set_prototype(global_object, &target, prototype, should_throw_if_cant_set);
        };

        let arguments = [self.target(), prototype];
        let trap_result = ProxyObject::call_trap(global_object, set_prototype_of_method, &handler, "setPrototypeOf", &arguments)?;

        if !trap_result.to_boolean() {
            if should_throw_if_cant_set {
                return Err(Thrown::type_error(
                    "Proxy 'setPrototypeOf' returned false indicating it could not set the prototype value. The operation was expected to succeed",
                ));
            }
            return Ok(false);
        }

        if object_is_extensible(global_object, &target)? {
            return Ok(true);
        }

        let target_prototype = target.get_prototype(global_object)?;
        if !same_value(prototype, target_prototype) {
            return Err(Thrown::type_error(
                "Proxy 'setPrototypeOf' trap returned true when its target is non-extensible and the new prototype value is not the same as the current prototype value. It should have returned false",
            ));
        }
        Ok(true)
    }

    /// `performGetPrototype(globalObject)`.
    pub fn perform_get_prototype(&self, global_object: &JSGlobalObject) -> Result<JSValue, Thrown> {
        let vm = global_object.vm();
        let _frame = ProxyObject::enter_frame(vm, GET_PROTOTYPE_COST)?;

        let handler = self.handler_object()?;
        let get_prototype_of_method = get_method(global_object, &handler, "getPrototypeOf")?;

        let target = self.target_object();
        let Some(get_prototype_of_method) = get_prototype_of_method else {
            return target.get_prototype(global_object);
        };

        let trap_result = ProxyObject::call_trap(global_object, get_prototype_of_method, &handler, "getPrototypeOf", &[self.target()])?;

        if !trap_result.is_object() && !trap_result.is_null() {
            return Err(Thrown::type_error("Proxy handler's 'getPrototypeOf' trap should either return an object or null"));
        }

        if object_is_extensible(global_object, &target)? {
            return Ok(trap_result);
        }

        let target_prototype = target.get_prototype(global_object)?;
        if !same_value(target_prototype, trap_result) {
            return Err(Thrown::type_error(
                "Proxy's 'getPrototypeOf' trap for a non-extensible target should return the same value as the target's prototype",
            ));
        }
        Ok(trap_result)
    }
}

// ---------------------------------------------------------------------------------------------
// O despacho: o que `methodTable()` faria. Cada operação vai para o `Proxy` se o objeto é um e, senão,
// para o `JSObject` comum, com `JSArray` e `JSFunction` à mão (como `own_descriptor` já fazia).
// ---------------------------------------------------------------------------------------------

/// `object->methodTable()->getOwnPropertySlot(object, globalObject, propertyName, slot)`: `Err(Pending)`
/// se o `Proxy` ou a materialização preguiçosa de uma função lançou.
pub fn own_property_slot(global_object: &JSGlobalObject, object: JSValue, property_name: &PropertyName, slot: &mut PropertySlot) -> Result<bool, Thrown> {
    let vm = global_object.vm();
    if let Some(proxy) = ProxyObject::from_value(&object) {
        return proxy.get_own_property_slot(global_object, property_name, slot);
    }
    if let Some(array) = JSArray::from_value_by_class(&object) {
        return Ok(array.get_own_property_slot(vm, property_name, slot));
    }
    let Some(object) = ObjectRef::from_value(&object) else {
        return Ok(false);
    };
    let found = object.get_own_property_slot(global_object, property_name, slot);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(found)
}

/// `getPropertySlot(globalObject, propertyName, slot)` (o laço de `JSObject.h`): consulta o `Proxy` pelo
/// `getOwnPropertySlot` dele, e o `getPrototype` sobrescrito (o do `Proxy`) pelo despacho.
pub fn get_property_slot(global_object: &JSGlobalObject, object: JSValue, property_name: &PropertyName, slot: &mut PropertySlot) -> Result<bool, Thrown> {
    let mut current = object;
    loop {
        let Some(object) = ObjectRef::from_value(&current) else {
            return Ok(false);
        };
        if own_property_slot(global_object, current, property_name, slot)? {
            return Ok(true);
        }
        if slot.is_vm_inquiry() && slot.is_tainted_by_opaque_object() {
            return Ok(false);
        }
        if object.type_() == JSType::ProxyObjectType && slot.internal_method_type() == InternalMethodType::HasProperty {
            return Ok(false);
        }

        // FIXME do C++: This doesn't look like it's following the specification:
        // https://bugs.webkit.org/show_bug.cgi?id=172572
        let prototype = if object.structure().type_info().overrides_get_prototype() && !slot.is_vm_inquiry() {
            object.get_prototype(global_object)?
        } else {
            object.get_prototype_direct()
        };
        if !prototype.is_object() {
            return Ok(false);
        }
        current = prototype;
    }
}

/// O `Thrown` de um trap como o `PutError` dos métodos do `JSObject`: a exceção é lançada no `VM` (o
/// `ThrowScope` do trap) e o `PutError::Pending` é o `RETURN_IF_EXCEPTION` que o chamador propaga.
pub fn put_error_from_thrown(global_object: &JSGlobalObject, thrown: Thrown) -> PutError {
    match thrown {
        Thrown::Unported(what) => PutError::Unported(what),
        thrown => {
            throw_thrown(global_object, thrown);
            PutError::Pending
        }
    }
}

/// `ProxyObject::put` do `Proxy` que o laço de `putInlineSlow` encontrou na cadeia (`proxy` é o `Proxy`),
/// no realm da `Structure` dele (veja `get_property_slot_from_proxy`).
pub fn put_from_proxy(
    proxy: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    slot: &mut PutPropertySlot,
) -> Result<bool, PutError> {
    let global_object = proxy.structure().realm().expect("Proxy sem realm na Structure");
    let proxy = ProxyObject::from_cell_id(proxy.cell_id()).expect("célula do tipo ProxyObjectType que não é ProxyObject");
    proxy.put_with_slot(&global_object, property_name, value, slot).map_err(|thrown| put_error_from_thrown(&global_object, thrown))
}

/// `putByIndexCommon` do `Proxy` que um `attemptToInterceptPutByIndexOnHoleForPrototype` encontrou na
/// cadeia (`proxy` é o `Proxy`), no realm da `Structure` dele (veja `get_property_slot_from_proxy`).
pub fn put_by_index_from_proxy(
    proxy: &JSObject,
    this_value: JSValue,
    index: u32,
    value: JSValue,
    should_throw: bool,
) -> Result<bool, PutError> {
    let global_object = proxy.structure().realm().expect("Proxy sem realm na Structure");
    let proxy = ProxyObject::from_cell_id(proxy.cell_id()).expect("célula do tipo ProxyObjectType que não é ProxyObject");
    proxy
        .put_by_index_common(&global_object, this_value, index, value, should_throw)
        .map_err(|thrown| put_error_from_thrown(&global_object, thrown))
}

/// `ProxyObject::deleteProperty` (e `deletePropertyByIndex`, com o índice como nome) quando o receptor
/// do `JSObject::delete_property` é o próprio `Proxy`, no realm da `Structure` dele.
pub fn delete_from_proxy(proxy: &JSObject, property_name: &PropertyName) -> Result<bool, PutError> {
    let global_object = proxy.structure().realm().expect("Proxy sem realm na Structure");
    let proxy = ProxyObject::from_cell_id(proxy.cell_id()).expect("célula do tipo ProxyObjectType que não é ProxyObject");
    proxy.perform_delete(&global_object, property_name).map_err(|thrown| put_error_from_thrown(&global_object, thrown))
}

/// `ProxyObject::defineOwnProperty` quando o receptor do `JSObject::define_own_property` é o próprio
/// `Proxy`, no realm da `Structure` dele.
pub fn define_own_property_from_proxy(
    proxy: &JSObject,
    property_name: &PropertyName,
    descriptor: &PropertyDescriptor,
    should_throw: bool,
) -> Result<bool, PutError> {
    let global_object = proxy.structure().realm().expect("Proxy sem realm na Structure");
    let proxy = ProxyObject::from_cell_id(proxy.cell_id()).expect("célula do tipo ProxyObjectType que não é ProxyObject");
    proxy
        .perform_define_own_property(&global_object, property_name, descriptor, should_throw)
        .map_err(|thrown| put_error_from_thrown(&global_object, thrown))
}

/// `getPropertySlot` de um `JSObject` cuja cadeia chegou num `Proxy` (`proxy` é o `Proxy`): o laço de
/// `JSObject::getPropertySlot` do C++ consulta `getOwnPropertySlot` virtual do `Proxy` com o
/// `globalObject` do chamador, que o `JSObject` do porte não recebe; o realm é o da `Structure` do
/// `Proxy`. A exceção do trap fica pendente no `VM` e o resultado é `false`, como no C++.
pub fn get_property_slot_from_proxy(proxy: &JSObject, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
    let global_object = proxy.structure().realm().expect("Proxy sem realm na Structure");
    match get_property_slot(&global_object, proxy.as_value(), property_name, slot) {
        Ok(found) => found,
        Err(Thrown::Pending) => false,
        Err(thrown) => {
            throw_thrown(&global_object, thrown);
            false
        }
    }
}

/// `object.[[Get]](propertyName, receiver)`: `Err(Pending)` se um getter ou um trap lançou.
pub fn object_get(global_object: &JSGlobalObject, object: &JSObject, property_name: &PropertyName, receiver: JSValue) -> Result<JSValue, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_get(global_object, property_name, receiver);
    }
    let mut slot = PropertySlot::new(receiver, InternalMethodType::Get);
    if get_property_slot(global_object, object.as_value(), property_name, &mut slot)? {
        return slot_value(global_object, &slot, property_name);
    }
    Ok(js_undefined())
}

/// `object.[[HasProperty]](propertyName)`.
pub fn object_has_property(global_object: &JSGlobalObject, object: &JSObject, property_name: &PropertyName) -> Result<bool, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_has_property(global_object, property_name);
    }
    let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::HasProperty);
    get_property_slot(global_object, object.as_value(), property_name, &mut slot)
}

/// `object.[[Set]](propertyName, value, receiver)`; `is_strict` é o `shouldThrow` do slot.
pub fn object_set(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    receiver: JSValue,
    is_strict: bool,
) -> Result<bool, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_put(global_object, property_name, value, receiver, is_strict);
    }
    let vm = global_object.vm();
    // `JSObject::put` com `thisValue` alterado (`Reflect.set(target, key, value, receiver)`, ou o alvo de um
    // `Proxy` sem trap `set` com o próprio `Proxy` de receptor): `ordinarySetSlow`, que define no receptor.
    if js_global_proxy::is_this_value_altered(receiver, object) {
        // `[[Set]]` do objeto exótico de índice inteiro com receptor diferente (o alvo de um `Proxy` de typed array): o
        // índice fora do alcance devolve `true` sem definir nada no receptor (`Reflect.set(proxy, 8, 1)` do bun).
        if let Some(result) = crate::runtime::typed_array_dispatch::put(object, property_name, value, receiver, is_strict) {
            return result.map_err(Thrown::from);
        }
        return ordinary_set_slow(global_object, object, property_name, value, receiver, is_strict);
    }
    let mut slot = PutPropertySlot::new(receiver, is_strict, PutContext::UnknownContext, false);
    crate::runtime::js_array::put_through_method_table(vm, object, property_name, value, &mut slot).map_err(Thrown::from)
}

/// `ordinarySetSlow(globalObject, object, propertyName, value, receiver, shouldThrow)`: o `[[Set]]` com um
/// receptor que não é o próprio objeto (`Reflect.set`, `Proxy` na cadeia). O `Proxy` não tem descritor
/// próprio a consultar: o laço de `ordinary_set_with_own_descriptor` o entrega ao trap `set`.
pub fn ordinary_set_slow(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    receiver: JSValue,
    should_throw: bool,
) -> Result<bool, Thrown> {
    let mut own_descriptor = PropertyDescriptor::default();
    if object.type_() != JSType::ProxyObjectType {
        if let Some(found) = object_get_own_property_descriptor(global_object, object, property_name)? {
            own_descriptor = found;
        }
    }
    ordinary_set_with_own_descriptor(global_object, object, property_name, value, receiver, own_descriptor, should_throw)
}

/// `ordinarySetWithOwnDescriptor(...)` (https://tc39.es/ecma262/#sec-ordinarysetwithowndescriptor).
pub fn ordinary_set_with_own_descriptor(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    receiver: JSValue,
    mut own_descriptor: PropertyDescriptor,
    should_throw: bool,
) -> Result<bool, Thrown> {
    // `ObjectRef` e não `JSObjectHandle`: o protótipo pode ser uma `JSFunction`, que o C++ trata como
    // qualquer `JSObject*` (o `[[GetOwnProperty]]` dela materializa `name`/`length`/`prototype`).
    let mut holder: Option<ObjectRef> = None;
    loop {
        let current: &JSObject = match &holder {
            Some(handle) => handle,
            None => object,
        };
        if let Some(proxy) = ProxyObject::from_cell_id(current.cell_id()) {
            return proxy.perform_put(global_object, property_name, value, receiver, should_throw);
        }
        if holder.is_some() && is_typed_array_type(current.type_()) {
            // 10.4.5.5 TypedArray [[Set]](P, V, Receiver): nome numérico canônico com receptor diferente do
            // próprio TypedArray devolve `true` se o índice é inválido, senão cai no `OrdinarySet`;
            // `JSGenericTypedArrayView::put` responde pelos dois, e `None` (nome não numérico) segue o
            // laço comum (`Base::put`).
            if let Some(answer) = crate::runtime::typed_array_dispatch::put(current, property_name, value, receiver, should_throw) {
                return answer.map_err(Thrown::from);
            }
        }

        // 9.1.9.1-2 Let ownDesc be ? O.[[GetOwnProperty]](P).
        let own_descriptor_found = if holder.is_none() {
            !own_descriptor.is_empty()
        } else if let Some(found) = object_get_own_property_descriptor(global_object, current, property_name)? {
            own_descriptor = found;
            true
        } else {
            false
        };

        if !own_descriptor_found {
            // 9.1.9.1-3-a Let parent be ? O.[[GetPrototypeOf]]().
            let prototype = current.get_prototype(global_object)?;
            // 9.1.9.1-3-b If parent is not null, then return ? parent.[[Set]](P, V, Receiver).
            if !prototype.is_null() {
                // `getPrototype` só devolve objeto (`JSValue::isObject()`), e toda célula-objeto está no registro.
                holder = Some(prototype.as_object());
                continue;
            }
            // 9.1.9.1-3-c-i Let ownDesc be the PropertyDescriptor{[[Value]]: undefined, [[Writable]]: true,
            // [[Enumerable]]: true, [[Configurable]]: true}.
            own_descriptor = PropertyDescriptor::new(JSValue::undefined(), 0);
        }
        break;
    }

    // 9.1.9.1-4 If IsDataDescriptor(ownDesc) is true, then
    if own_descriptor.is_data_descriptor() {
        // 9.1.9.1-4-a If ownDesc.[[Writable]] is false, return false.
        if !own_descriptor.writable() {
            return Ok(type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR)?);
        }
        // 9.1.9.1-4-b If Type(Receiver) is not Object, return false.
        if !receiver.is_object() {
            return Ok(type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR)?);
        }
        // `JSFunction` incluída: `object_get_own_property_descriptor`/`object_define_own_property` a materializam.
        let receiver_object = receiver.as_object();
        // 9.1.9.1-4-c Let existingDescriptor be ? Receiver.[[GetOwnProperty]](P).
        // 9.1.9.1-4-d If existingDescriptor is not undefined, then
        if let Some(existing) = object_get_own_property_descriptor(global_object, &receiver_object, property_name)? {
            // 9.1.9.1-4-d-i-ii If IsAccessorDescriptor(existingDescriptor) is true or [[Writable]] is false, return false.
            if existing.is_accessor_descriptor() || !existing.writable() {
                return Ok(type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR)?);
            }
            // 9.1.9.1-4-d-iii Let valueDesc be the PropertyDescriptor{[[Value]]: V}.
            let mut value_descriptor = PropertyDescriptor::default();
            value_descriptor.set_value(value);
            // 9.1.9.1-4-d-iv Return ? Receiver.[[DefineOwnProperty]](P, valueDesc).
            return object_define_own_property(global_object, &receiver_object, property_name, &value_descriptor, should_throw);
        }
        // 9.1.9.1-4-e-i Return ? CreateDataProperty(Receiver, P, V).
        return object_define_own_property(global_object, &receiver_object, property_name, &PropertyDescriptor::new(value, 0), should_throw);
    }

    // 9.1.9.1-5 Assert: IsAccessorDescriptor(ownDesc) is true.
    debug_assert!(own_descriptor.is_accessor_descriptor());

    // 9.1.9.1-6-7 Let setter be ownDesc.[[Set]]. If setter is undefined, return false.
    let setter = own_descriptor.setter();
    if !setter.is_object() {
        return Ok(type_error(should_throw, READONLY_PROPERTY_WRITE_ERROR)?);
    }

    // 9.1.9.1-8 Perform ? Call(setter, Receiver, << V >>).
    call_checked(global_object, setter, receiver, &[value], "setter is not callable")?;

    // 9.1.9.1-9 Return true.
    Ok(true)
}

/// `object.[[Delete]](propertyName)`: o `length` do array não é configurável.
pub fn object_delete_property(global_object: &JSGlobalObject, object: &JSObject, property_name: &PropertyName) -> Result<bool, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_delete(global_object, property_name);
    }
    let vm = global_object.vm();
    if JSArray::from_value_by_class(&object.as_value()).is_some() && *property_name == vm.property_names.length {
        return Ok(false);
    }
    // `JSFunction::deleteProperty` materializa `length`/`name`/`prototype` antes de apagar: sem isso o `delete` pelo
    // `Proxy` de uma função deixaria a propriedade preguiçosa viva.
    if let Some(ObjectRef::Function(function)) = ObjectRef::from_value(&object.as_value()) {
        return Ok(function.delete_property(global_object, property_name, &mut DeletePropertySlot::default())?);
    }
    Ok(object.delete_property(vm, property_name, &mut DeletePropertySlot::default())?)
}

/// `object.[[GetOwnProperty]](propertyName)` (`getOwnPropertyDescriptor`): `None` se não existe.
pub fn object_get_own_property_descriptor(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
) -> Result<Option<PropertyDescriptor>, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::GetOwnProperty);
        if !proxy.get_own_property_slot(global_object, property_name, &mut slot)? {
            return Ok(None);
        }
        let mut descriptor = PropertyDescriptor::default();
        descriptor.set_property_slot(&slot, property_name);
        return Ok(Some(descriptor));
    }
    // `object` já é um `JSObject`: `asObject` é invariante, como no C++.
    let object_ref = object.as_value().as_object();
    let Some(lookup) = object_ref.for_property_lookup(global_object, property_name) else {
        return Err(Thrown::Pending);
    };
    own_descriptor(global_object, lookup, property_name)
}

/// `object.[[DefineOwnProperty]](propertyName, descriptor)`.
pub fn object_define_own_property(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_name: &PropertyName,
    descriptor: &PropertyDescriptor,
    should_throw: bool,
) -> Result<bool, Thrown> {
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_define_own_property(global_object, property_name, descriptor, should_throw);
    }
    // `JSArray::defineOwnProperty`: `length` (RangeError, encolhimento bloqueado por índice não configurável)
    // e a recusa de índice novo com `length` não gravável.
    if let Some(array) = crate::runtime::js_array::JSArray::from_cell_id_by_class(object.cell_id()) {
        return Ok(array.define_own_property(global_object.vm(), property_name, descriptor, should_throw)?);
    }
    let object_ref = object.as_value().as_object();
    Ok(object_ref.define_own_property(global_object, property_name, descriptor, should_throw)?)
}

/// `object.[[IsExtensible]]()`.
pub fn object_is_extensible(global_object: &JSGlobalObject, object: &JSObject) -> Result<bool, Thrown> {
    if let Some(target) = js_global_proxy::target_of(object) {
        return object_is_extensible(global_object, &target);
    }
    match ProxyObject::from_cell_id(object.cell_id()) {
        Some(proxy) => proxy.perform_is_extensible(global_object),
        None => Ok(object.is_structure_extensible()),
    }
}

/// `object.[[PreventExtensions]]()`.
pub fn object_prevent_extensions(global_object: &JSGlobalObject, object: &JSObject) -> Result<bool, Thrown> {
    if let Some(target) = js_global_proxy::target_of(object) {
        return object_prevent_extensions(global_object, &target);
    }
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_prevent_extensions(global_object);
    }
    if crate::runtime::typed_array_dispatch::refuses_prevent_extensions(object) {
        return Ok(false);
    }
    prevent_extensions(global_object.vm(), object)?;
    Ok(true)
}

/// `object.[[SetPrototypeOf]](prototype)` (`setPrototype(vm, globalObject, prototype, shouldThrowIfCantSet)`).
/// Os `TypeError` de `set_prototype_with_cycle_check` (imutável, não extensível, ciclo) são exatamente
/// os casos em que o C++ devolve `false` quando não deve lançar.
pub fn object_set_prototype(
    global_object: &JSGlobalObject,
    object: &JSObject,
    prototype: JSValue,
    should_throw_if_cant_set: bool,
) -> Result<bool, Thrown> {
    if let Some(target) = js_global_proxy::target_of(object) {
        return object_set_prototype(global_object, &target, prototype, should_throw_if_cant_set);
    }
    if let Some(proxy) = ProxyObject::from_cell_id(object.cell_id()) {
        return proxy.perform_set_prototype(global_object, prototype, should_throw_if_cant_set);
    }
    match set_prototype_with_cycle_check(global_object.vm(), object, prototype) {
        Ok(()) => Ok(true),
        Err(Thrown::TypeError(_)) if !should_throw_if_cant_set => Ok(false),
        Err(error) => Err(error),
    }
}

/// `object->methodTable()->getOwnPropertyNames(object, globalObject, propertyNames, mode)`.
pub fn object_get_own_property_names(
    global_object: &JSGlobalObject,
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
) -> Result<(), Thrown> {
    if let Some(target) = js_global_proxy::target_of(object) {
        return object_get_own_property_names(global_object, &target, property_names, mode);
    }
    match ProxyObject::from_cell_id(object.cell_id()) {
        Some(proxy) => proxy.get_own_property_names(global_object, property_names, mode),
        // `JSFunction::getOwnSpecialPropertyNames` (`length`, `name`, `prototype`): o alvo de um `Proxy` sem trap `ownKeys`.
        None => match object.as_value().as_js_function() {
            Some(function) => get_own_property_names_with_special(global_object.vm(), object, property_names, mode, |names| {
                function.get_own_special_property_names(global_object, names, mode)
            }),
            None => get_own_property_names(global_object.vm(), object, property_names, mode),
        },
    }
}
