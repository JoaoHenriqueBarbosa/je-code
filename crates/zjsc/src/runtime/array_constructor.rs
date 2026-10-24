//! Porte de `runtime/ArrayConstructor.h` e `ArrayConstructor.cpp`: o construtor `Array` (um
//! `InternalFunction`), `Array.isArray`, `Array.of` e os caminhos de chamada e de construção.
//!
//! LACUNAS, e por quê:
//! - `Array.from` (público) é a entrada `JSBuiltin` de `arrayConstructorTable`, reificada no primeiro
//!   acesso; o `@from` privado e `Array.fromAsync` são builtins de `ArrayConstructor.js` e entram em
//!   `finishCreation` por `put_direct_builtin_function_without_transition`.
//! - `@@species` é o acessor de `put_species_accessor`: no C++ é `arraySpeciesGetterSetter()`, um
//!   `GetterSetter` do `JSGlobalObject` criado no `init`; aqui cada construtor cria o seu (a
//!   identidade do `GetterSetter` só importa para os `Watchpoint`s de espécie, que o porte não tem).
//! - Subclasse (`class A extends Array`, `Reflect.construct(Array, [], F)`): o protótipo vem de
//!   `getFunctionRealm`/`Get(newTarget, "prototype")` (`arrayStructureForIndexingTypeDuringAllocation`
//!   com `newTarget`, em `js_global_object_inlines.rs`). `Array.of` com `this` construtor que não é o
//!   `Array` constrói por `construct(this, [length])` e preenche por `putDirectIndex`.
//! - `constructArrayWithSizeQuirk` segue o C++: tamanho a partir de `MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH`
//!   usa a estrutura de `ArrayWithArrayStorage` (`construct_empty_array`).
//! - `isArray` de `Proxy` precisa do `ProxyObject` (alvo e revogação): `Unported`.
//!
//! DIVERGÊNCIA: `thisValue == globalObject->arrayConstructor()` (o global não guarda o construtor) é
//! "o valor é um `InternalFunction` cuja função de chamada é `callArrayConstructor`", que identifica o
//! `Array` de qualquer realm; o `newTarget` do `Array` também.

use crate::bytecode::op_metadata::ArrayAllocationProfile;
use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::js_global_object_inlines::{
    array_structure_for_profile_during_allocation_with_new_target, construct_array_negative_indexed_with_new_target,
    construct_empty_array_with_new_target,
};
use crate::runtime::array_prototype::{
    array_error_from_llint, create_data_property_at, run_array_function, run_array_function_with_new_target, set_length,
    ArrayCall, ArrayResult,
};
use crate::runtime::call_data::{construct_with_error_message, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::collection_support::put_species_accessor;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef, PropertyAdditionMode, INTERNAL_FUNCTION_S_INFO};
use crate::runtime::js_array::{construct_array, ArrayError, JSArray};
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::js_function::{put_direct_builtin_function_without_transition, put_direct_native_function_without_transition};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::PutError;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::{js_boolean, js_number, EncodedJSValue, JSValue};
use crate::runtime::math_common::to_uint32;
use crate::runtime::native_function::to_tagged;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::property_attribute::{BUILTIN, DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `ArrayInvalidLengthError`.
pub const ARRAY_INVALID_LENGTH_ERROR: &str = "Array length must be a positive integer of safe magnitude.";

/// `const ClassInfo ArrayConstructor::s_info`.
pub static ARRAY_CONSTRUCTOR_S_INFO: ClassInfo = ClassInfo {
    class_name: "Function",
    parent_class: Some(&INTERNAL_FUNCTION_S_INFO),
    static_prop_hash_table: Some(&ARRAY_CONSTRUCTOR_TABLE),
    inherits_js_type_range: None,
};

/// `arrayConstructorTableValues` de `ArrayConstructor.lut.h`: só o `from` público (`DontEnum|Builtin`, `length` 1).
static ARRAY_CONSTRUCTOR_TABLE_VALUES: [HashTableValue; 1] = [HashTableValue {
    key: "from",
    attributes: DONT_ENUM | BUILTIN,
    intrinsic: Intrinsic::NoIntrinsic,
    kind: Kind::BuiltinGenerator { generator: BuiltinCodeIndex::ArrayConstructorFromCode, length: 1 },
}];

/// `arrayConstructorTable`.
static ARRAY_CONSTRUCTOR_TABLE: HashTable = HashTable { class_for_this: None, values: &ARRAY_CONSTRUCTOR_TABLE_VALUES };

/// Os builtins JS de `finishCreation` e da tabela (ver o cabeçalho): `@from`, `fromAsync` e `from`.
pub const ARRAY_CONSTRUCTOR_JS_BUILTINS: [&str; 3] = ["@from", "fromAsync", "from"];

/// `thisValue == globalObject->arrayConstructor()` (ver DIVERGÊNCIA do cabeçalho).
pub fn is_array_constructor(value: &JSValue) -> bool {
    let JSValue::Cell(cell_id) = value else { return false };
    InternalFunction::from_cell_id(*cell_id)
        .is_some_and(|function| function.native_function_for(CodeSpecializationKind::CodeForCall) == to_tagged(array_constructor_call_host))
}

/// `constructArrayWithSizeQuirk(globalObject, profile, length, newTarget)`: o `newTarget` vazio é a chamada
/// sem `new` (e o caminho do LLInt, [`construct_array_with_size_quirk_with_profile`]).
pub fn construct_array_with_size_quirk_with_new_target(
    vm: &VM,
    global_object: &JSGlobalObject,
    mut profile: Option<&mut ArrayAllocationProfile>,
    length: JSValue,
    new_target: JSValue,
) -> ArrayResult {
    if !length.is_number() {
        return Ok(construct_array_negative_indexed_with_new_target(vm, global_object, profile, &[length], new_target)?.as_value());
    }

    let n = to_uint32(length.as_number());
    if f64::from(n) != length.as_number() {
        return Err(ArrayError::RangeError(ARRAY_INVALID_LENGTH_ERROR));
    }
    Ok(construct_empty_array_with_new_target(vm, global_object, profile.as_mut().map(|profile| &mut **profile), n, new_target)?.as_value())
}

/// `constructArrayWithSizeQuirk(globalObject, profile, length)` com `newTarget` vazio.
pub fn construct_array_with_size_quirk_with_profile(
    vm: &VM,
    global_object: &JSGlobalObject,
    profile: Option<&mut ArrayAllocationProfile>,
    length: JSValue,
) -> ArrayResult {
    construct_array_with_size_quirk_with_new_target(vm, global_object, profile, length, JSValue::empty())
}

/// `constructArrayWithSizeQuirk(globalObject, args, newTarget)`.
pub fn construct_array_with_size_quirk(call: &ArrayCall) -> ArrayResult {
    // a single numeric argument denotes the array size (!)
    if call.args.len() == 1 {
        return construct_array_with_size_quirk_with_new_target(call.vm, call.global_object, None, call.args[0], call.new_target);
    }

    // otherwise the array is constructed with the arguments in it
    let structure = array_structure_for_profile_during_allocation_with_new_target(call.global_object, None, call.new_target)?;
    Ok(construct_array(call.vm, &structure, call.args).as_value())
}

/// Quem chamou `isArray`: o C++ olha o `jsCallee` do `topJSCallFrame` e troca o nome na mensagem do `Proxy`
/// revogado quando ele é o `Object.prototype.toString` do realm. O porte não tem essa função (o `LazyProperty`
/// `objectProtoToStringFunction` do `JSGlobalObject` não existe), então o chamador diz quem ele é.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsArrayCaller {
    /// `Array.isArray` (e todo outro chamador que não é `Object.prototype.toString`).
    ArrayIsArray,
    /// `Object.prototype.toString`.
    ObjectPrototypeToString,
}

impl IsArrayCaller {
    /// `makeString(calleeName, " cannot be called on a Proxy that has been revoked")`.
    fn revoked_proxy_message(self) -> &'static str {
        match self {
            IsArrayCaller::ArrayIsArray => "Array.isArray cannot be called on a Proxy that has been revoked",
            IsArrayCaller::ObjectPrototypeToString => "Object.prototype.toString cannot be called on a Proxy that has been revoked",
        }
    }
}

/// `isArray(globalObject, value)`: `JSArray`; o `Proxy` segue o alvo (`isArraySlowInline`) e um `Proxy`
/// revogado lança `TypeError` com o nome do `caller`.
pub fn is_array(value: &JSValue, caller: IsArrayCaller) -> Result<bool, ArrayError> {
    // `type() == ArrayType || type() == DerivedArrayType`: `Array.isArray(Array.prototype)` é `true`.
    if JSArray::from_value_by_class(value).is_some() {
        return Ok(true);
    }
    let Some(mut proxy) = ProxyObject::from_value(value) else {
        return Ok(false);
    };
    loop {
        if proxy.is_revoked() {
            return Err(ArrayError::Put(PutError::TypeError(caller.revoked_proxy_message())));
        }
        let target = proxy.target();
        if JSArray::from_value_by_class(&target).is_some() {
            return Ok(true);
        }
        match ProxyObject::from_value(&target) {
            Some(next) => proxy = next,
            None => return Ok(false),
        }
    }
}

/// `arrayConstructorIsArray`.
pub fn array_constructor_is_array(call: &ArrayCall) -> ArrayResult {
    Ok(js_boolean(is_array(&call.argument(0), IsArrayCaller::ArrayIsArray)?))
}

/// `arrayConstructorPrivateFromFastWithoutMapFn`: o atalho de `Array.from(items)` sem `mapFn`. O C++
/// devolve `undefined` quando o atalho não se aplica e `Array.from` segue o caminho geral; aqui o atalho
/// nunca se aplica, o que é sempre correto (só mais lento).
pub fn array_constructor_private_from_fast_without_map_fn(_call: &ArrayCall) -> ArrayResult {
    Ok(JSValue::undefined())
}

pub(crate) fn array_constructor_private_from_fast_without_map_fn_host(
    global_object: &JSGlobalObject,
    call_frame: &mut NativeCallFrame<'_>,
) -> EncodedJSValue {
    run_array_function(global_object, call_frame, array_constructor_private_from_fast_without_map_fn)
}

/// `arrayConstructorOf`: `Array.of(...items)`.
pub fn array_constructor_of(call: &ArrayCall) -> ArrayResult {
    let is_constructor = !get_construct_data(call.this_value).is_none();
    // `fastArrayOf`: `thisValue == arrayConstructor` ou `!isConstructor`.
    if !is_constructor || is_array_constructor(&call.this_value) {
        let structure = call.global_object.array_structure();
        return Ok(construct_array(call.vm, &structure, call.args).as_value());
    }

    let length = call.args.len();
    let created = construct_with_error_message(
        call.global_object,
        call.this_value,
        &[js_number(length as f64)],
        "Array.of did not get a valid constructor",
    )
    .map_err(array_error_from_llint)?;
    let result = ObjectRef::from_value(&created).expect("construct devolve objeto");
    for (i, value) in call.args.iter().enumerate() {
        create_data_property_at(call, &result, i as u64, *value)?;
    }
    set_length(call, &result, length as u64)?;
    Ok(created)
}

/// `constructWithArrayConstructor`: o `newTarget` (o próprio `Array` ou o do `Reflect.construct`) vem do `this` do quadro.
fn array_constructor_host(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    run_array_function(global_object, call_frame, construct_array_with_size_quirk)
}

/// `callArrayConstructor`: sem `new`, o `newTarget` é `JSValue()`, e o `this` do quadro (um `null` vindo de
/// `Array.bind(null, 3)()`, por exemplo) não é `newTarget`.
fn array_constructor_call_host(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    run_array_function_with_new_target(global_object, call_frame, construct_array_with_size_quirk, JSValue::empty())
}

pub(crate) fn array_constructor_is_array_host(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    run_array_function(global_object, call_frame, array_constructor_is_array)
}

fn array_constructor_of_host(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    run_array_function(global_object, call_frame, array_constructor_of)
}

/// `class ArrayConstructor : public InternalFunction`: sem campos próprios.
pub struct ArrayConstructor;

impl ArrayConstructor {
    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = InternalFunction::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, ArrayConstructor::STRUCTURE_FLAGS),
            &ARRAY_CONSTRUCTOR_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure, arrayPrototype)`: `ArrayConstructor(vm, structure)`
    /// (`InternalFunction(vm, structure, callArrayConstructor, constructWithArrayConstructor)`) e
    /// `finishCreation(vm, globalObject, arrayPrototype)`.
    pub fn create(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        array_prototype: &JSArray,
    ) -> InternalFunctionRef {
        let constructor = InternalFunction::new(
            vm,
            structure,
            array_constructor_call_host,
            Some(array_constructor_host),
        );
        let builtin_names = vm.property_names.builtin_names();
        // O `from` público é entrada de `ARRAY_CONSTRUCTOR_TABLE`: reificado no primeiro acesso, não aqui.
        constructor.finish_creation(vm, 1, &WtfString::from_latin1(b"Array"), PropertyAdditionMode::WithoutStructureTransition);
        constructor.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            array_prototype.as_value(),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        // `@@species` logo após `prototype`, como em `ArrayConstructor::finishCreation`.
        put_species_accessor(vm, global_object, &constructor);
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &constructor,
            &vm.property_names.of,
            0,
            array_constructor_of_host,
            ImplementationVisibility::Public,
            Intrinsic::ArrayConstructorOfIntrinsic,
            DONT_ENUM,
        );
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &constructor,
            &vm.property_names.is_array,
            1,
            array_constructor_is_array_host,
            ImplementationVisibility::Public,
            Intrinsic::ArrayIsArrayIntrinsic,
            DONT_ENUM,
        );
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            &constructor,
            &builtin_names.from_private_name(),
            BuiltinCodeIndex::ArrayConstructorFromCode,
            DONT_ENUM,
        );
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            &constructor,
            builtin_names.from_async_public_name(),
            BuiltinCodeIndex::ArrayConstructorFromAsyncCode,
            DONT_ENUM,
        );
        constructor
    }
}
