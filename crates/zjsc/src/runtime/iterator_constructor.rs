//! Porte de `runtime/JSIteratorConstructor.{h,cpp}` e `JSIterator.{h,cpp}`: o construtor global `Iterator`
//! (um `InternalFunction`, `LinkTimeConstant::Iterator`), a instância `JSIterator` das subclasses de
//! `Iterator`, e a parte do `JSGlobalObject::init` que liga os iteradores auxiliares
//! (`m_iteratorStructure`, `m_iteratorHelperPrototype`, `m_iteratorHelperStructure`,
//! `m_wrapForValidIteratorStructure`, `m_iteratorConstructor` e os `LinkTimeConstant`
//! `iteratorHelperCreate` e `wrapForValidIteratorCreate`): `install_iterator_classes`.
//!
//! O QUE ESTÁ AQUI: `Iterator.from` (`JSIteratorConstructor.js`), `Iterator.concat` (atrás de
//! `useIteratorSequencing`) e `Iterator.zip` e `Iterator.zipKeyed` (atrás de `useJointIteration`) são
//! `JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION`, registrados por `put_direct_builtin_function_without_transition`.
//! `Iterator.prototype.map`, `filter`, `take`, `drop`, `flatMap`, `reduce`, `toArray`, `forEach`, `some`,
//! `every` e `find` estão em `iterator_prototype.rs`; o `Iterator Helper` em `js_iterator_helper.rs`.
//!
//! DIVERGÊNCIAS:
//! - Os membros do `JSGlobalObject` que este arquivo preenche moram em `IteratorGlobalData`
//!   (`JSGlobalObject::iterator_data`), como `FunctionKindGlobalData`; os `LinkTimeConstant` nascem junto
//!   (o C++ os cria por `initLater`).
//! - `JSIterator` é um `JSObject` comum com a `Structure` de `JSIteratorType`, sem campos próprios.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::builtins_source::{public_name, BuiltinCodeIndex};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::{constructor_cannot_be_called_as_function, create_native_collection_constructor, native_constructor_structure};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::iterator_helper_prototype::JSIteratorHelperPrototype;
use crate::runtime::js_function::{put_direct_builtin_function_without_transition, JSFunctionRef, JS_FUNCTION_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_iterator_helper::create_iterator_helper_create_function;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrap_for_valid_iterator::{create_wrap_for_valid_iterator_create_function, JSWrapForValidIterator};
use crate::runtime::options_list::Options;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::runtime::wrap_for_valid_iterator_prototype::WrapForValidIteratorPrototype;

/// `const ClassInfo JSIterator::s_info`.
pub static JS_ITERATOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Iterator", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSIterator final : public JSNonFinalObject`.
pub struct JSIterator;

impl JSIterator {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::JSIteratorType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_ITERATOR_S_INFO,
        )
    }

    /// `create(vm, structure)`: `JSIterator(vm, structure)` e `finishCreation(vm)`.
    pub fn create(vm: &VM, structure: &StructureRef) -> JSObjectRef {
        let instance = JSObject::allocate(vm, structure);
        instance.finish_creation(vm);
        instance
    }
}

/// `const ClassInfo JSIteratorConstructor::s_info`.
pub static JS_ITERATOR_CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `callIterator`.
fn call_iterator_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    constructor_cannot_be_called_as_function("Iterator")
}

/// `constructIterator` (https://tc39.es/proposal-iterator-helpers/#sec-iterator).
fn construct_iterator_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let new_target = call.new_target();
    if new_target == JSValue::from_cell(call.callee()) {
        return Err(Thrown::type_error("Iterator cannot be constructed directly"));
    }

    let iterator_structure =
        get_derived_structure_in_realm(global_object, new_target, call.callee(), |realm| realm.iterator_structure())?;
    Ok(JSIterator::create(vm, &iterator_structure).as_value())
}

crate::host_function!(call_iterator, call_iterator_body);
crate::host_function!(construct_iterator, construct_iterator_body);

/// `class JSIteratorConstructor final : public InternalFunction`: espaço de nomes de `create`. No bun é um
/// `JSFunction` sobre `NativeExecutable` (`ownKeys`: `length,name,prototype,from,concat,zip,zipKeyed`).
pub struct JSIteratorConstructor;

impl JSIteratorConstructor {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        native_constructor_structure(vm, global_object, prototype, &JS_ITERATOR_CONSTRUCTOR_S_INFO)
    }

    /// `create(vm, globalObject, structure, iteratorPrototype)`: `length` 0, o nome `Iterator`, `prototype`
    /// e os builtins estáticos.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef, iterator_prototype: &JSObject) -> JSFunctionRef {
        let constructor = create_native_collection_constructor(
            vm,
            global_object,
            structure,
            iterator_prototype,
            "Iterator",
            0,
            call_iterator,
            construct_iterator,
            false,
        );

        let builtin_names = vm.property_names.builtin_names();
        let builtin = |index: BuiltinCodeIndex| {
            put_direct_builtin_function_without_transition(
                vm,
                global_object,
                &constructor,
                public_name(builtin_names, index),
                index,
                DONT_ENUM,
            );
        };
        builtin(BuiltinCodeIndex::JsIteratorConstructorFromCode);
        if Options::use_iterator_sequencing() {
            builtin(BuiltinCodeIndex::JsIteratorConstructorConcatCode);
        }
        if Options::use_joint_iteration() {
            builtin(BuiltinCodeIndex::JsIteratorConstructorZipCode);
            builtin(BuiltinCodeIndex::JsIteratorConstructorZipKeyedCode);
        }
        constructor
    }
}

/// Os membros do `JSGlobalObject` que este arquivo preenche (ver as DIVERGÊNCIAS do cabeçalho).
#[derive(Debug, Default)]
pub struct IteratorGlobalData {
    /// `m_iteratorConstructor`.
    pub(crate) iterator_constructor: Option<JSFunctionRef>,
    /// `m_iteratorStructure`.
    pub(crate) iterator_structure: Option<StructureRef>,
    /// `m_iteratorHelperPrototype`.
    pub(crate) iterator_helper_prototype: Option<JSObjectRef>,
    /// `m_iteratorHelperStructure`.
    pub(crate) iterator_helper_structure: Option<StructureRef>,
    /// `m_wrapForValidIteratorStructure`.
    pub(crate) wrap_for_valid_iterator_structure: Option<StructureRef>,
}

impl JSGlobalObject {
    /// `iteratorConstructor()`: invariante do `init(vm)`.
    pub fn iterator_constructor(&self) -> JSFunctionRef {
        self.iterator_data.borrow().iterator_constructor.clone().expect("JSGlobalObject sem iteratorConstructor")
    }

    /// `iteratorStructure()`: invariante do `init(vm)`.
    pub fn iterator_structure(&self) -> StructureRef {
        self.iterator_data.borrow().iterator_structure.clone().expect("JSGlobalObject sem iteratorStructure")
    }

    /// `iteratorHelperPrototype()`: invariante do `init(vm)`.
    pub fn iterator_helper_prototype(&self) -> JSObjectRef {
        self.iterator_data.borrow().iterator_helper_prototype.clone().expect("JSGlobalObject sem iteratorHelperPrototype")
    }

    /// `iteratorHelperStructure()`: invariante do `init(vm)`.
    pub fn iterator_helper_structure(&self) -> StructureRef {
        self.iterator_data.borrow().iterator_helper_structure.clone().expect("JSGlobalObject sem iteratorHelperStructure")
    }

    /// `wrapForValidIteratorStructure()`: invariante do `init(vm)`.
    pub fn wrap_for_valid_iterator_structure(&self) -> StructureRef {
        self.iterator_data
            .borrow()
            .wrap_for_valid_iterator_structure
            .clone()
            .expect("JSGlobalObject sem wrapForValidIteratorStructure")
    }
}

/// A parte de `JSGlobalObject::init` de `JSGlobalObject.cpp` que cria `m_iteratorStructure`,
/// `m_iteratorHelperPrototype` e `m_iteratorHelperStructure` (linhas 1428 a 1431), a estrutura de
/// `JSWrapForValidIterator` (1457), o `Iterator` (1601 a 1604) e os dois `LinkTimeConstant` de criação (1954 a
/// 1960). Depende de `m_iteratorPrototype` e de `m_functionPrototype`, que `init` já criou.
pub fn install_iterator_classes(vm: &VM, global_object: &JSGlobalObject, function_prototype: JSValue) {
    let iterator_prototype = global_object.iterator_prototype();

    let iterator_structure = JSIterator::create_structure(vm, global_object, iterator_prototype.as_value());

    let iterator_helper_prototype_structure =
        JSIteratorHelperPrototype::create_structure(vm, global_object, iterator_prototype.as_value());
    let iterator_helper_prototype = JSIteratorHelperPrototype::create(vm, global_object, &iterator_helper_prototype_structure);
    let iterator_helper_structure = crate::runtime::js_iterator_helper::JSIteratorHelper::create_structure(
        vm,
        Some(global_object),
        iterator_helper_prototype.as_value(),
    );

    let wrap_for_valid_iterator_prototype_structure =
        WrapForValidIteratorPrototype::create_structure(vm, global_object, iterator_prototype.as_value());
    let wrap_for_valid_iterator_prototype =
        WrapForValidIteratorPrototype::create(vm, global_object, &wrap_for_valid_iterator_prototype_structure);
    let wrap_for_valid_iterator_structure =
        JSWrapForValidIterator::create_structure(vm, Some(global_object), wrap_for_valid_iterator_prototype.as_value());

    let iterator_constructor_structure = JSIteratorConstructor::create_structure(vm, global_object, function_prototype);
    let iterator_constructor = JSIteratorConstructor::create(vm, global_object, iterator_constructor_structure, &iterator_prototype);

    {
        let mut data = global_object.iterator_data.borrow_mut();
        data.iterator_structure = Some(iterator_structure);
        data.iterator_helper_prototype = Some(iterator_helper_prototype);
        data.iterator_helper_structure = Some(iterator_helper_structure);
        data.wrap_for_valid_iterator_structure = Some(wrap_for_valid_iterator_structure);
        data.iterator_constructor = Some(JSFunctionRef::clone(&iterator_constructor));
    }

    global_object.set_link_time_constant(LinkTimeConstant::Iterator, iterator_constructor.as_value());
    global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Iterator")), iterator_constructor.as_value(), DONT_ENUM);
    global_object.set_link_time_constant(
        LinkTimeConstant::WrapForValidIteratorCreate,
        create_wrap_for_valid_iterator_create_function(vm, global_object).as_value(),
    );
    global_object.set_link_time_constant(
        LinkTimeConstant::IteratorHelperCreate,
        create_iterator_helper_create_function(vm, global_object).as_value(),
    );
}
