//! Porte de `runtime/JSTypedArrayViewConstructor.{h,cpp}`: o `%TypedArray%` (o construtor que `Int8Array` e
//! as irmãs herdam), com `%TypedArray%.of`, `%TypedArray%.from` (o builtin JS `TypedArrayConstructor.js`) e
//! `@@species`. Chamá-lo ou construí-lo lança.
//!
//! DIVERGÊNCIAS:
//!
//! - `@@species` é o acessor que `put_species_accessor` cria (o `JSFunction` `"get
//!   [Symbol.species]"` sobre `globalFuncSpeciesGetter`); no C++ ele é o
//!   `globalObject->typedArraySpeciesGetterSetter()`, criado em `JSGlobalObject::init` para os watchpoints o
//!   compararem. Sem watchpoints, não há o que comparar.
//! - `installTypedArrayConstructorSpeciesWatchpoint` é chamado por `TypedArrayRealm::init` (logo depois de criar
//!   o `%TypedArray%`), não do `create` daqui, porque o set e o watchpoint vivem no `TypedArrayRealm`.
//! - `typedArrayOfFast` (o atalho de `of` quando `this` é um dos 12 construtores originais) não existe: o
//!   caminho geral constrói pelo próprio construtor e dá o mesmo resultado, sem efeito observável a mais.

use crate::host_function;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::call_data::{construct_with_error_message, get_construct_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{create_native_collection_constructor, native_constructor_structure, put_native_function, put_species_accessor};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_function::{JSFunction, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::thrown_from_llint;
use crate::runtime::js_function::put_direct_builtin_function_without_transition;
use crate::runtime::js_generic_typed_array_view::validate_typed_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `const ClassInfo JSTypedArrayViewConstructor::s_info` (`"Function"`).
pub static TYPED_ARRAY_VIEW_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `constructTypedArrayView`: serve de chamada e de construção.
fn construct_typed_array_view_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(Thrown::type_error("%TypedArray% should not be called directly"))
}

// https://tc39.es/ecma262/#sec-%typedarray%.of
fn typed_array_constructor_of_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let length = call.argument_count();
    let this_value = call.this_value();

    if get_construct_data(this_value).is_none() {
        return Err(Thrown::type_error("TypedArray.of requires |this| to be a constructor"));
    }

    let constructed = construct_with_error_message(
        global_object,
        this_value,
        &[js_number(length as f64)],
        "TypedArray.of requires |this| to be a constructor",
    )
    .map_err(thrown_from_llint)?;

    let view = validate_typed_array(constructed)?;
    if view.length() < length {
        return Err(Thrown::type_error("TypedArray.of constructed typed array of insufficient length"));
    }

    for (index, argument) in call.arguments().iter().enumerate() {
        view.set_index(global_object, index as u64, *argument)?;
    }
    Ok(view.as_value())
}

host_function!(construct_typed_array_view, construct_typed_array_view_body);
host_function!(typed_array_constructor_of, typed_array_constructor_of_body);

/// `class JSTypedArrayViewConstructor final : public InternalFunction`: sem campos próprios. No bun é um
/// `JSFunction` sobre `NativeExecutable` (`ownKeys`: `length,name,prototype,of,from,@@species`).
pub struct TypedArrayViewConstructor;

impl TypedArrayViewConstructor {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, &TYPED_ARRAY_VIEW_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, globalObject, structure, prototype)` e `finishCreation`: `length` 0, o nome `TypedArray`,
    /// `prototype`, `of`, `from` e, por último, `@@species` (a ordem medida no bun).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef, prototype: &JSObject) -> JSFunctionRef {
        let constructor = create_native_collection_constructor(
            vm,
            global_object,
            structure,
            prototype,
            "TypedArray",
            0,
            construct_typed_array_view,
            construct_typed_array_view,
            false,
        );
        let names = &vm.property_names;
        put_native_function(vm, global_object, &constructor, &names.of, 0, typed_array_constructor_of, Intrinsic::NoIntrinsic);
        put_direct_builtin_function_without_transition(
            vm,
            global_object,
            &constructor,
            &names.from,
            BuiltinCodeIndex::TypedArrayConstructorFromCode,
            DONT_ENUM,
        );
        put_species_accessor(vm, global_object, &constructor);
        constructor
    }
}
