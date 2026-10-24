//! Porte de `runtime/JSIteratorPrototype.{h,cpp}` e `JSIteratorPrototypeInlines.h`: o
//! `%IteratorPrototype%` (`Iterator.prototype`), um `JSNonFinalObject` comum.
//!
//! O QUE ESTÁ AQUI: `@@iterator` (`iteratorProtoFuncIterator`), as nativas `toArray`, `forEach`,
//! `includes` e `join` e os builtins JS `some`, `every`, `find`, `reduce`, `map`, `filter`, `take`,
//! `drop`, `flatMap`, `chunks`, `windows` e `@@dispose` (`builtins/JSIteratorPrototype.js`), na ordem de
//! `finishCreation` e atrás das mesmas `Options` (`useIteratorChunking`, `useIteratorIncludes`,
//! `useIteratorJoin`, `useExplicitResourceManagement`). `constructor` e `@@toStringTag` são
//! `CustomGetterSetter` com `setterThatIgnoresPrototypeProperties`; o getter de `constructor` devolve
//! `globalObject->iteratorConstructor()` (`iterator_constructor.rs`).
//!
//! LACUNAS, e por quê:
//! - `CachedCall` (caminho rápido de `forEach`, `includes` e `join` quando `next`/o callback são JS): só
//!   o caminho genérico existe, com a mesma ordem de chamadas observável.

use crate::host_function;
use crate::{custom_getter, custom_setter};
use crate::runtime::builtins_source::{public_name, BuiltinCodeIndex};
use crate::runtime::call_data::{call, get_call_data};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::proxy_object::{object_set, to_this_strict};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::iterator_operations::{
    for_each_in_iterator_protocol, iterator_close, iterator_direct, iterator_step, iterator_value, thrown_from_llint,
};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_function::{
    call_host_function_as_constructor, put_direct_builtin_function_without_transition,
    put_direct_native_function_without_transition, JSFunction, JSFunctionRef,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::math_common::{is_integer, max_safe_integer_as_uint64};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::operations::same_value_zero;
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::{CUSTOM_ACCESSOR, DONT_ENUM};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::math_extras::truncate_double_to_uint64;
use crate::wtf::text::wtf_string::{join_runs_with_separator, String as WtfString};

/// `const ClassInfo JSIteratorPrototype::s_info`.
pub static JS_ITERATOR_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const INCLUDES_SKIP_ERROR: &str =
    "Iterator.prototype.includes requires that the second argument is a non-negative safe integral Number or Infinity.";

/// `iteratorProtoFuncIterator`: `callFrame->thisValue().toThis(globalObject, ECMAMode::strict())`.
fn iterator_proto_iterator(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(to_this_strict(call.this_value()))
}

/// `setterThatIgnoresPrototypeProperties(globalObject, thisValue, homeObject, propertyName, value,
/// shouldThrow)` (`JSObject.cpp`; só o `JSIteratorPrototype` o usa, por isso mora aqui): o setter dos
/// acessores `constructor` e `@@toStringTag` do `Iterator.prototype`.
fn setter_that_ignores_prototype_properties(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    home_object: &JSObject,
    property_name: &PropertyName,
    value: JSValue,
    should_throw: bool,
) -> Result<bool, Thrown> {
    let Some(this_object) = ObjectRef::from_value(&this_value).filter(|_| this_value.is_object()) else {
        return Err(Thrown::type_error("SetterThatIgnoresPrototypeProperties expected |this| to be an object."));
    };
    if this_object.cell_id() == home_object.cell_id() {
        return Err(Thrown::type_error("SetterThatIgnoresPrototypeProperties was called on a home object."));
    }

    let has_property = this_object.has_own_property(global_object, property_name);
    pending_or(global_object, ())?;
    if has_property {
        return object_set(global_object, &this_object, property_name, value, this_value, should_throw);
    }

    // `createDataProperty(globalObject, propertyName, value, shouldThrow)`.
    let descriptor = PropertyDescriptor::new(value, 0);
    Ok(this_object.define_own_property(global_object, property_name, &descriptor, should_throw)?)
}

/// `iteratorProtoConstructorGetter`: `globalObject->iteratorConstructor()`
/// (https://tc39.es/proposal-iterator-helpers/#sec-get-iteratorprototype-constructor).
fn iterator_proto_constructor(global_object: &JSGlobalObject, _this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    Ok(global_object.iterator_constructor().as_value())
}
custom_getter!(iterator_proto_constructor_getter, iterator_proto_constructor);

/// `iteratorProtoConstructorSetter` (https://tc39.es/proposal-iterator-helpers/#sec-set-iteratorprototype-constructor):
/// `shouldThrow` é verdadeiro, o `Err` lança.
fn iterator_proto_constructor_set(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    value: JSValue,
    property_name: &PropertyName,
) -> Result<bool, Thrown> {
    setter_that_ignores_prototype_properties(global_object, this_value, &global_object.iterator_prototype(), property_name, value, true)?;
    Ok(true)
}
custom_setter!(iterator_proto_constructor_setter, iterator_proto_constructor_set);

/// `iteratorProtoToStringTagGetter`: `jsNontrivialString(vm, "Iterator")`.
fn iterator_proto_to_string_tag(global_object: &JSGlobalObject, _this_value: JSValue, _property_name: &PropertyName) -> HostResult {
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(b"Iterator"))))
}
custom_getter!(iterator_proto_to_string_tag_getter, iterator_proto_to_string_tag);

/// `iteratorProtoToStringTagSetter`: o `bool` do C++ é sempre verdadeiro (`JSValue::encode(jsUndefined())`
/// convertido), o `shouldThrow` lança pelo `Err`.
fn iterator_proto_to_string_tag_set(
    global_object: &JSGlobalObject,
    this_value: JSValue,
    value: JSValue,
    property_name: &PropertyName,
) -> Result<bool, Thrown> {
    setter_that_ignores_prototype_properties(global_object, this_value, &global_object.iterator_prototype(), property_name, value, true)?;
    Ok(true)
}
custom_setter!(iterator_proto_to_string_tag_setter, iterator_proto_to_string_tag_set);

/// `iteratorClose(globalObject, iterator)` seguido do `TRY_CLEAR_EXCEPTION`, que os erros de argumento
/// de `forEach` e `includes` fazem antes de lançar o erro deles.
fn close_iterator_then(global_object: &JSGlobalObject, iterator: JSValue, thrown: Thrown) -> Thrown {
    iterator_close(global_object, iterator);
    let vm = global_object.vm();
    if !vm.has_pending_termination_exception() {
        vm.clear_exception();
    }
    thrown
}

/// `RETURN_IF_EXCEPTION` depois de uma operação que lança direto no `VM` (`iteratorClose`).
fn return_if_exception(global_object: &JSGlobalObject) -> Result<(), Thrown> {
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(())
}

/// `iteratorProtoFuncToArray`.
fn iterator_proto_to_array(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = call.this_value();
    if !this_value.is_object() {
        return Err(Thrown::type_error("Iterator.prototype.toArray requires that |this| be an Object."));
    }
    // O `MarkedArgumentBuffer` do C++: os itens já lidos vivem só aqui enquanto o próximo `next` roda JS.
    let marked = crate::runtime::gc::RootScope::new(&[]);
    let mut values = Vec::new();
    for_each_in_iterator_protocol(global_object, this_value, |next_item| {
        marked.append(next_item);
        values.push(next_item);
        Ok(())
    })?;
    Ok(construct_array(global_object.vm(), &global_object.array_structure(), &values).as_value())
}

/// `iteratorProtoFuncForEach`.
fn iterator_proto_for_each(global_object: &JSGlobalObject, call_info: &HostCall) -> HostResult {
    let this_value = call_info.this_value();
    if !this_value.is_object() {
        return Err(Thrown::type_error("Iterator.prototype.forEach requires that |this| be an Object."));
    }
    let callback = call_info.argument(0);
    let call_data = get_call_data(callback);
    if call_data.is_none() {
        return Err(close_iterator_then(
            global_object,
            this_value,
            Thrown::type_error("Iterator.prototype.forEach requires the callback argument to be callable."),
        ));
    }
    let mut counter: u64 = 0;
    for_each_in_iterator_protocol(global_object, this_value, |next_item| {
        let index = js_number(counter as f64);
        counter += 1;
        call(global_object, callback, &call_data, JSValue::undefined(), &[next_item, index])
            .map(|_| ())
            .map_err(thrown_from_llint)
    })?;
    Ok(JSValue::undefined())
}

/// O `toSkip` de `includes`: o segundo argumento, que fecha o iterador e lança se for inválido.
fn includes_to_skip(global_object: &JSGlobalObject, iterator: JSValue, skipped_elements: JSValue) -> Result<u64, Thrown> {
    let fail = |thrown: Thrown| Err(close_iterator_then(global_object, iterator, thrown));
    if skipped_elements.is_undefined() {
        return Ok(0);
    }
    if skipped_elements.is_int32() {
        let as_int32 = skipped_elements.as_int32();
        if as_int32 < 0 {
            return fail(Thrown::range_error(INCLUDES_SKIP_ERROR));
        }
        return Ok(as_int32 as u64);
    }
    if !skipped_elements.is_double() {
        return fail(Thrown::type_error(INCLUDES_SKIP_ERROR));
    }
    let as_double = skipped_elements.as_double();
    let is_infinity = as_double.is_infinite();
    if !is_integer(as_double) && !is_infinity {
        return fail(Thrown::type_error(INCLUDES_SKIP_ERROR));
    }
    let as_uint = truncate_double_to_uint64(as_double);
    if as_uint as f64 == as_double && as_uint <= max_safe_integer_as_uint64() {
        return Ok(as_uint);
    }
    if is_infinity && as_double > 0.0 {
        // "if the 2nd argument is +Infinity or too big, we should consume the iterator to the end."
        return Ok(u64::MAX);
    }
    fail(Thrown::range_error(INCLUDES_SKIP_ERROR))
}

/// `iteratorProtoFuncIncludes`.
fn iterator_proto_includes(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this_value = to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(Thrown::type_error("Iterator.prototype.includes requires that |this| be an Object."));
    }
    let to_skip = includes_to_skip(global_object, this_value, call.argument(1))?;
    let record = iterator_direct(global_object, this_value)?;
    let search_element = call.argument(0);
    let mut skipped: u64 = 0;
    loop {
        let Some(next) = iterator_step(global_object, record)? else {
            return Ok(js_boolean(false));
        };
        if skipped < to_skip {
            skipped += 1;
            continue;
        }
        let next_value = iterator_value(global_object, next)?;
        if same_value_zero(next_value, search_element) {
            iterator_close(global_object, record.iterator);
            return_if_exception(global_object)?;
            return Ok(js_boolean(true));
        }
    }
}

/// `value.toString(globalObject)` do `join`: `Err(Pending)` se a conversão lança (objeto com
/// `toPrimitive` do usuário, `Symbol`).
fn value_to_wtf_string(global_object: &JSGlobalObject, value: JSValue) -> Result<WtfString, Thrown> {
    let string = value.to_wtf_string();
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(string)
}

/// `iteratorProtoFuncJoin`.
fn iterator_proto_join(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let this_value = to_this_strict(call.this_value());
    if !this_value.is_object() {
        return Err(Thrown::type_error("Iterator.prototype.join requires that |this| be an Object."));
    }
    let separator_value = call.argument(0);
    let separator = if separator_value.is_undefined() {
        WtfString::from_latin1(b",")
    } else {
        // `abruptCloseIterator`: a exceção do `toString` fica pendente depois do `iteratorClose`.
        value_to_wtf_string(global_object, separator_value).inspect_err(|_| iterator_close(global_object, this_value))?
    };
    let record = iterator_direct(global_object, this_value)?;
    let mut parts: Vec<(WtfString, u64)> = Vec::new();
    while let Some(next) = iterator_step(global_object, record)? {
        let next_value = iterator_value(global_object, next)?;
        if next_value.is_undefined_or_null() {
            parts.push((WtfString::from_latin1(b""), 1));
            continue;
        }
        parts.push((value_to_wtf_string(global_object, next_value).inspect_err(|_| iterator_close(global_object, this_value))?, 1));
    }
    Ok(JSValue::from_js_string(js_string(vm, &join_runs_with_separator(&parts, &separator))))
}

host_function!(iterator_proto_func_iterator, iterator_proto_iterator);
host_function!(iterator_proto_func_to_array, iterator_proto_to_array);
host_function!(iterator_proto_func_for_each, iterator_proto_for_each);
host_function!(iterator_proto_func_includes, iterator_proto_includes);
host_function!(iterator_proto_func_join, iterator_proto_join);

/// A `JSFunction` de `m_iteratorProtoSymbolIteratorFunction.initLater(...)` do `JSGlobalObject`
/// (`JSFunction::create(vm, owner, 0, "[Symbol.iterator]", iteratorProtoFuncIterator, Public,
/// IteratorIntrinsic)`); o `finishCreation` do protótipo a instala e o global a guarda.
pub fn create_symbol_iterator_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        0,
        &WtfString::from_latin1(b"[Symbol.iterator]"),
        iterator_proto_func_iterator,
        ImplementationVisibility::Public,
        Intrinsic::IteratorIntrinsic,
        call_host_function_as_constructor,
    )
}

/// `class JSIteratorPrototype final : public JSNonFinalObject`.
pub struct JSIteratorPrototype;

impl JSIteratorPrototype {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)` (`JSIteratorPrototypeInlines.h`).
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ObjectType, JSIteratorPrototype::STRUCTURE_FLAGS),
            &JS_ITERATOR_PROTOTYPE_S_INFO,
        )
    }

    /// `create(vm, globalObject, structure)`: `JSIteratorPrototype(vm, structure)` e `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: &StructureRef) -> JSObjectRef {
        let prototype = JSObject::allocate(vm, structure);
        JSIteratorPrototype::finish_creation(&prototype, vm, global_object);
        prototype
    }

    /// `finishCreation(vm, globalObject)`, na ordem do C++ (ver as LACUNAS do cabeçalho).
    fn finish_creation(prototype: &JSObject, vm: &VM, global_object: &JSGlobalObject) {
        prototype.finish_creation(vm);
        let names = &vm.property_names;
        let builtin_names = names.builtin_names();

        prototype.put_direct_without_transition(
            vm,
            &PropertyName::from_identifier(&names.iterator_symbol),
            create_symbol_iterator_function(vm, global_object).as_value(),
            DONT_ENUM,
        );

        // `constructor` (`iteratorProtoConstructorGetter/Setter`).
        // https://tc39.es/proposal-iterator-helpers/#sec-iteratorprototype.constructor
        prototype.put_direct_custom_getter_setter_without_transition(
            vm,
            &PropertyName::from_identifier(&names.constructor),
            &CustomGetterSetter::create(vm, iterator_proto_constructor_getter, Some(iterator_proto_constructor_setter)),
            DONT_ENUM | CUSTOM_ACCESSOR,
        );
        // https://tc39.es/proposal-iterator-helpers/#sec-iteratorprototype-@@tostringtag
        prototype.put_direct_custom_getter_setter_without_transition(
            vm,
            &PropertyName::from_identifier(&names.to_string_tag_symbol),
            &CustomGetterSetter::create(vm, iterator_proto_to_string_tag_getter, Some(iterator_proto_to_string_tag_setter)),
            DONT_ENUM | CUSTOM_ACCESSOR,
        );

        let native = |name: &Identifier, length: u32, function: NativeFunction, visibility: ImplementationVisibility| {
            put_direct_native_function_without_transition(
                vm,
                global_object,
                prototype,
                name,
                length,
                function,
                visibility,
                Intrinsic::NoIntrinsic,
                DONT_ENUM,
            );
        };
        let builtin = |index: BuiltinCodeIndex| {
            put_direct_builtin_function_without_transition(
                vm,
                global_object,
                prototype,
                public_name(builtin_names, index),
                index,
                DONT_ENUM,
            );
        };

        native(&names.to_array, 0, iterator_proto_func_to_array, ImplementationVisibility::Private);
        native(&names.for_each, 1, iterator_proto_func_for_each, ImplementationVisibility::Private);
        for index in [
            BuiltinCodeIndex::JsIteratorPrototypeSomeCode,
            BuiltinCodeIndex::JsIteratorPrototypeEveryCode,
            BuiltinCodeIndex::JsIteratorPrototypeFindCode,
            BuiltinCodeIndex::JsIteratorPrototypeReduceCode,
            BuiltinCodeIndex::JsIteratorPrototypeMapCode,
            BuiltinCodeIndex::JsIteratorPrototypeFilterCode,
            BuiltinCodeIndex::JsIteratorPrototypeTakeCode,
            BuiltinCodeIndex::JsIteratorPrototypeDropCode,
            BuiltinCodeIndex::JsIteratorPrototypeFlatMapCode,
        ] {
            builtin(index);
        }
        if Options::use_iterator_chunking() {
            builtin(BuiltinCodeIndex::JsIteratorPrototypeChunksCode);
            builtin(BuiltinCodeIndex::JsIteratorPrototypeWindowsCode);
        }
        if Options::use_iterator_includes() {
            native(builtin_names.includes_public_name(), 1, iterator_proto_func_includes, ImplementationVisibility::Public);
        }
        if Options::use_iterator_join() {
            native(&names.join, 1, iterator_proto_func_join, ImplementationVisibility::Public);
        }
        if Options::use_explicit_resource_management() {
            put_direct_builtin_function_without_transition(
                vm,
                global_object,
                prototype,
                &names.dispose_symbol,
                BuiltinCodeIndex::JsIteratorPrototypeDisposeCode,
                DONT_ENUM,
            );
        }
        prototype.structure().set_may_be_prototype(true);
    }
}
