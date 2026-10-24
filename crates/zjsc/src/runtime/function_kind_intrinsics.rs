//! Porte de `GeneratorFunctionPrototype.{h,cpp}`, `AsyncFunctionPrototype.{h,cpp}` e
//! `AsyncGeneratorFunctionPrototype.{h,cpp}` (os três `JSNonFinalObject` que só diferem no `ClassInfo`, e
//! cujo `finishCreation` é `@@toStringTag`) e da parte do `JSGlobalObject::init` que os liga ao
//! `Function` e às funções geradoras (`JSGlobalObject.cpp`, `m_asyncIteratorPrototype` e
//! `m_asyncGeneratorPrototype`, `m_asyncFromSyncIteratorStructure`, o `FunctionConstructor`, os três
//! `*FunctionConstructor` e `*FunctionPrototype`, o `constructor` de `Function.prototype`, a propriedade
//! global `Function` e os `LinkTimeConstant` `asyncGeneratorPrototypeNext` e
//! `asyncIteratorPrototypeSymbolAsyncIterator`): `install_function_kind_intrinsics`.
//!
//! DIVERGÊNCIAS:
//! - `m_asyncIteratorPrototype`, `m_generatorFunctionPrototype`, `m_asyncFunctionPrototype`,
//!   `m_asyncGeneratorFunctionPrototype`, `m_functionConstructor`, `m_asyncFromSyncIteratorStructure` e
//!   `m_asyncGeneratorStructure` moram em `FunctionKindGlobalData` (`JSGlobalObject::function_kind_data`),
//!   como os dados de promessa (`PromiseGlobalData`); as estruturas das funções
//!   (`generatorFunctionStructure()` etc.) continuam sendo os `LazyProperty` de `js_global_object.rs`, que
//!   agora leem o protótipo daqui.
//! - `m_asyncFromSyncIteratorProtoNextFunction` (`LazyProperty`) nasce junto do protótipo.
//! - `m_generatorStructure` e `m_asyncFunctionGeneratorStructure` também moram em `FunctionKindGlobalData`.
//! - `LinkTimeConstant::asyncFromSyncIteratorCreate` (`IteratorOperations.cpp`) nasce em
//!   `iterator_operations.rs` e é instalado aqui, de uma vez (o C++ usa `initLater`).

use std::rc::Rc;

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::parser::parser_modes::FunctionConstructionMode;
use crate::runtime::async_from_sync_iterator_prototype::{
    create_async_from_sync_iterator_proto_next_function, AsyncFromSyncIteratorPrototype,
};
use crate::runtime::async_generator_prototype::{create_async_generator_prototype_next_function, AsyncGeneratorPrototype};
use crate::runtime::async_iterator_prototype::{create_async_iterator_proto_func_async_iterator, AsyncIteratorPrototype};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::function_constructor::{create_function_construction_constructor, create_function_constructor_structure};
use crate::runtime::identifier::Identifier;
use crate::runtime::internal_function::{InternalFunction, InternalFunctionRef};
use crate::runtime::iterator_operations::create_async_from_sync_iterator_create_function;
use crate::runtime::js_async_from_sync_iterator::JSAsyncFromSyncIterator;
use crate::runtime::js_async_function_generator::JSAsyncFunctionGenerator;
use crate::runtime::js_async_generator::JSAsyncGenerator;
use crate::runtime::js_generator::JSGenerator;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// Define um dos três protótipos de função de tipo especial: o `ClassInfo`, `createStructure` e `create`
/// (`Base::finishCreation` e `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`).
macro_rules! define_function_kind_prototype {
    ($name:ident, $info:ident, $class_name:literal) => {
        /// `const ClassInfo ::s_info`.
        pub static $info: ClassInfo =
            ClassInfo { class_name: $class_name, parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

        #[doc = concat!("`class ", $class_name, "Prototype final : public JSNonFinalObject`.")]
        pub struct $name;

        impl $name {
            /// `StructureFlags = Base::StructureFlags`.
            pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

            /// `createStructure(vm, globalObject, prototype)` (`...Inlines.h`).
            pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
                Structure::create(vm, Some(global_object), prototype, TypeInfo::new(JSType::ObjectType, $name::STRUCTURE_FLAGS), &$info)
            }

            /// `create(vm, structure)`: o construtor e `finishCreation(vm)`.
            pub fn create(vm: &VM, structure: &StructureRef) -> JSObjectRef {
                let prototype = JSObject::allocate(vm, structure);
                prototype.finish_creation(vm);
                // `JSC_TO_STRING_TAG_WITHOUT_TRANSITION()`.
                put_to_string_tag(vm, &prototype, $info.class_name);
                prototype.structure().set_may_be_prototype(true);
                prototype
            }
        }
    };
}

define_function_kind_prototype!(GeneratorFunctionPrototype, GENERATOR_FUNCTION_PROTOTYPE_S_INFO, "GeneratorFunction");
define_function_kind_prototype!(AsyncFunctionPrototype, ASYNC_FUNCTION_PROTOTYPE_S_INFO, "AsyncFunction");
define_function_kind_prototype!(AsyncGeneratorFunctionPrototype, ASYNC_GENERATOR_FUNCTION_PROTOTYPE_S_INFO, "AsyncGeneratorFunction");

/// Os membros do `JSGlobalObject` que este arquivo preenche (ver as DIVERGÊNCIAS do cabeçalho).
#[derive(Debug, Default)]
pub struct FunctionKindGlobalData {
    /// `m_functionConstructor`.
    pub(crate) function_constructor: Option<InternalFunctionRef>,
    /// `m_asyncIteratorPrototype`.
    pub(crate) async_iterator_prototype: Option<JSObjectRef>,
    /// `m_generatorFunctionPrototype`.
    pub(crate) generator_function_prototype: Option<JSObjectRef>,
    /// `m_asyncFunctionPrototype`.
    pub(crate) async_function_prototype: Option<JSObjectRef>,
    /// `m_asyncGeneratorFunctionPrototype`.
    pub(crate) async_generator_function_prototype: Option<JSObjectRef>,
    /// `m_asyncFromSyncIteratorStructure`.
    pub(crate) async_from_sync_iterator_structure: Option<StructureRef>,
    /// `m_asyncFromSyncIteratorProtoNextFunction`: o `next` de `%AsyncFromSyncIteratorPrototype%`.
    pub(crate) async_from_sync_iterator_proto_next_function: Option<JSValue>,
    /// `m_asyncGeneratorStructure`.
    pub(crate) async_generator_structure: Option<StructureRef>,
    /// `m_generatorStructure`: `JSGenerator::createStructure(vm, this, m_generatorPrototype.get())`.
    pub(crate) generator_structure: Option<StructureRef>,
    /// `m_asyncFunctionGeneratorStructure`: idem, com `JSAsyncFunctionGenerator` e o mesmo `m_generatorPrototype`.
    pub(crate) async_function_generator_structure: Option<StructureRef>,
}

impl JSGlobalObject {
    /// O protótipo escolhido por `select` como `JSValue`, ou `null` antes de
    /// `install_function_kind_intrinsics` (o que os `LazyProperty` de estrutura de função usam).
    pub(crate) fn function_kind_prototype_value(&self, select: impl FnOnce(&FunctionKindGlobalData) -> Option<JSObjectRef>) -> JSValue {
        select(&self.function_kind_data.borrow()).map_or_else(js_null, |prototype| prototype.as_value())
    }

    /// `functionConstructor()`: invariante do `init(vm)`.
    pub fn function_constructor(&self) -> InternalFunctionRef {
        self.function_kind_data.borrow().function_constructor.clone().expect("JSGlobalObject sem functionConstructor")
    }

    /// `asyncIteratorPrototype()`: invariante do `init(vm)`.
    pub fn async_iterator_prototype(&self) -> JSObjectRef {
        self.function_kind_data.borrow().async_iterator_prototype.clone().expect("JSGlobalObject sem asyncIteratorPrototype")
    }

    /// `generatorFunctionPrototype()`: invariante do `init(vm)`.
    pub fn generator_function_prototype(&self) -> JSObjectRef {
        self.function_kind_data.borrow().generator_function_prototype.clone().expect("JSGlobalObject sem generatorFunctionPrototype")
    }

    /// `asyncFunctionPrototype()`: invariante do `init(vm)`.
    pub fn async_function_prototype(&self) -> JSObjectRef {
        self.function_kind_data.borrow().async_function_prototype.clone().expect("JSGlobalObject sem asyncFunctionPrototype")
    }

    /// `asyncGeneratorFunctionPrototype()`: invariante do `init(vm)`.
    pub fn async_generator_function_prototype(&self) -> JSObjectRef {
        self.function_kind_data
            .borrow()
            .async_generator_function_prototype
            .clone()
            .expect("JSGlobalObject sem asyncGeneratorFunctionPrototype")
    }

    /// `asyncFromSyncIteratorStructure()`: invariante do `init(vm)`.
    pub fn async_from_sync_iterator_structure(&self) -> StructureRef {
        self.function_kind_data
            .borrow()
            .async_from_sync_iterator_structure
            .clone()
            .expect("JSGlobalObject sem asyncFromSyncIteratorStructure")
    }

    /// `asyncFromSyncIteratorPrototypeNextFunction()`: invariante do `init(vm)`.
    pub fn async_from_sync_iterator_prototype_next_function(&self) -> JSValue {
        self.function_kind_data
            .borrow()
            .async_from_sync_iterator_proto_next_function
            .expect("JSGlobalObject sem asyncFromSyncIteratorPrototypeNextFunction")
    }

    /// `asyncGeneratorStructure()`: invariante do `init(vm)`.
    pub fn async_generator_structure(&self) -> StructureRef {
        self.function_kind_data.borrow().async_generator_structure.clone().expect("JSGlobalObject sem asyncGeneratorStructure")
    }

    /// `generatorStructure()`: invariante do `init(vm)`.
    pub fn generator_structure(&self) -> StructureRef {
        self.function_kind_data.borrow().generator_structure.clone().expect("JSGlobalObject sem generatorStructure")
    }

    /// `asyncFunctionGeneratorStructure()`: invariante do `init(vm)`.
    pub fn async_function_generator_structure(&self) -> StructureRef {
        self.function_kind_data
            .borrow()
            .async_function_generator_structure
            .clone()
            .expect("JSGlobalObject sem asyncFunctionGeneratorStructure")
    }
}

/// O par de `GeneratorFunction` e `AsyncGeneratorFunction`: `generatorPrototype->constructor =
/// generatorFunctionPrototype` e `generatorFunctionPrototype->prototype = generatorPrototype`, ambos
/// `DontEnum|ReadOnly`.
fn link_generator_prototype(vm: &VM, generator_prototype: &JSObject, function_prototype: &JSObject) {
    let attributes = DONT_ENUM | READ_ONLY;
    generator_prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.constructor),
        function_prototype.as_value(),
        attributes,
    );
    function_prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.prototype),
        generator_prototype.as_value(),
        attributes,
    );
}

/// O `*FunctionConstructor` do modo, sobre o `InternalFunction` cujo protótipo é o construtor `Function`, e o
/// `constructor` do protótipo apontando para ele (`DontEnum|ReadOnly`).
fn install_function_kind_constructor(
    vm: &VM,
    global_object: &JSGlobalObject,
    function_constructor: &InternalFunctionRef,
    mode: FunctionConstructionMode,
    function_prototype: &JSObject,
) {
    let structure = InternalFunction::create_structure(vm, Some(global_object), function_constructor.as_value());
    let constructor = create_function_construction_constructor(vm, structure, mode, function_prototype);
    function_prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.constructor),
        constructor.as_value(),
        DONT_ENUM | READ_ONLY,
    );
}

/// O trecho do `JSGlobalObject::init` (`JSGlobalObject.cpp`) das funções de tipo especial e dos iteradores
/// assíncronos; precisa do `generator_prototype` (já instalado pelo `init`), do `Function.prototype`, do
/// `Object.prototype` e do `%IteratorPrototype%`, e vem antes de qualquer leitura das estruturas
/// `generatorFunctionStructure()`, `asyncFunctionStructure()` e `asyncGeneratorFunctionStructure()`.
pub fn install_function_kind_intrinsics(
    vm: &VM,
    global_object: &JSGlobalObject,
    object_prototype: JSValue,
    function_prototype: &InternalFunctionRef,
    iterator_prototype: JSValue,
) {
    // Os `LinkTimeConstant` que os protótipos assíncronos leem na criação.
    global_object.set_link_time_constant(
        LinkTimeConstant::AsyncGeneratorPrototypeNext,
        create_async_generator_prototype_next_function(vm, global_object).as_value(),
    );
    global_object.set_link_time_constant(
        LinkTimeConstant::AsyncIteratorPrototypeSymbolAsyncIterator,
        create_async_iterator_proto_func_async_iterator(vm, global_object).as_value(),
    );

    global_object.set_link_time_constant(
        LinkTimeConstant::AsyncFromSyncIteratorCreate,
        create_async_from_sync_iterator_create_function(vm, global_object).as_value(),
    );

    // m_asyncIteratorPrototype e m_asyncGeneratorPrototype.
    let async_iterator_prototype = AsyncIteratorPrototype::create(
        vm,
        global_object,
        &AsyncIteratorPrototype::create_structure(vm, global_object, object_prototype),
    );
    let async_generator_prototype = AsyncGeneratorPrototype::create(
        vm,
        global_object,
        &AsyncGeneratorPrototype::create_structure(vm, global_object, async_iterator_prototype.as_value()),
    );
    *global_object.async_generator_prototype.borrow_mut() = Some(Rc::clone(&async_generator_prototype));

    // m_asyncFromSyncIteratorStructure e m_asyncFromSyncIteratorProtoNextFunction.
    let async_from_sync_iterator_proto_next_function = create_async_from_sync_iterator_proto_next_function(vm, global_object).as_value();
    let async_from_sync_iterator_prototype = AsyncFromSyncIteratorPrototype::create(
        vm,
        global_object,
        &AsyncFromSyncIteratorPrototype::create_structure(vm, global_object, iterator_prototype),
        async_from_sync_iterator_proto_next_function,
    );
    let async_from_sync_iterator_structure =
        JSAsyncFromSyncIterator::create_structure(vm, Some(global_object), async_from_sync_iterator_prototype.as_value());

    // O `FunctionConstructor`.
    let function_constructor_structure = create_function_constructor_structure(vm, global_object, function_prototype.as_value());
    let function_constructor =
        create_function_construction_constructor(vm, function_constructor_structure, FunctionConstructionMode::Function, function_prototype);

    // GeneratorFunction.
    let generator_function_prototype =
        GeneratorFunctionPrototype::create(vm, &GeneratorFunctionPrototype::create_structure(vm, global_object, function_prototype.as_value()));
    install_function_kind_constructor(vm, global_object, &function_constructor, FunctionConstructionMode::Generator, &generator_function_prototype);
    link_generator_prototype(vm, &global_object.generator_prototype(), &generator_function_prototype);

    // AsyncFunction.
    let async_function_prototype =
        AsyncFunctionPrototype::create(vm, &AsyncFunctionPrototype::create_structure(vm, global_object, function_prototype.as_value()));
    install_function_kind_constructor(vm, global_object, &function_constructor, FunctionConstructionMode::Async, &async_function_prototype);

    // AsyncGeneratorFunction.
    let async_generator_function_prototype = AsyncGeneratorFunctionPrototype::create(
        vm,
        &AsyncGeneratorFunctionPrototype::create_structure(vm, global_object, function_prototype.as_value()),
    );
    install_function_kind_constructor(
        vm,
        global_object,
        &function_constructor,
        FunctionConstructionMode::AsyncGenerator,
        &async_generator_function_prototype,
    );
    link_generator_prototype(vm, &async_generator_prototype, &async_generator_function_prototype);
    let async_generator_structure = JSAsyncGenerator::create_structure(vm, Some(global_object), async_generator_prototype.as_value());

    // `m_functionPrototype->putDirectWithoutTransition(constructor, functionConstructor, DontEnum)` e a
    // propriedade global `Function`.
    function_prototype.put_direct_without_transition(
        vm,
        &PropertyName::from_identifier(&vm.property_names.constructor),
        function_constructor.as_value(),
        DONT_ENUM,
    );
    global_object.put_direct(
        vm,
        &PropertyName::from_identifier(&Identifier::from_span(vm, b"Function")),
        function_constructor.as_value(),
        DONT_ENUM,
    );

    *global_object.function_kind_data.borrow_mut() = FunctionKindGlobalData {
        function_constructor: Some(function_constructor),
        async_iterator_prototype: Some(async_iterator_prototype),
        generator_function_prototype: Some(generator_function_prototype),
        async_function_prototype: Some(async_function_prototype),
        async_generator_function_prototype: Some(async_generator_function_prototype),
        async_from_sync_iterator_structure: Some(async_from_sync_iterator_structure),
        async_from_sync_iterator_proto_next_function: Some(async_from_sync_iterator_proto_next_function),
        async_generator_structure: Some(async_generator_structure),
        generator_structure: Some(JSGenerator::create_structure(vm, Some(global_object), global_object.generator_prototype().as_value())),
        async_function_generator_structure: Some(JSAsyncFunctionGenerator::create_structure(
            vm,
            Some(global_object),
            global_object.generator_prototype().as_value(),
        )),
    };
}

#[cfg(test)]
mod tests {
    use crate::api::eval::new_global_object;
    use crate::runtime::js_type::JSType;

    /// `m_generatorStructure`, `m_asyncFunctionGeneratorStructure` e `m_asyncGeneratorStructure` têm o
    /// protótipo que o `JSGlobalObject::init` do C++ lhes dá (e não `null`), e o tipo da célula certo.
    #[test]
    fn generator_structures_carry_their_prototypes() {
        let (_vm, global_object) = new_global_object();
        let generator = global_object.generator_structure();
        assert_eq!(generator.stored_prototype(), global_object.generator_prototype().as_value());
        assert_eq!(generator.type_(), JSType::JSGeneratorType);

        let async_function_generator = global_object.async_function_generator_structure();
        assert_eq!(async_function_generator.stored_prototype(), global_object.generator_prototype().as_value());
        assert_eq!(async_function_generator.type_(), JSType::JSAsyncFunctionGeneratorType);

        let async_generator = global_object.async_generator_structure();
        assert_eq!(async_generator.stored_prototype(), global_object.async_generator_prototype().as_value());
        assert_eq!(async_generator.type_(), JSType::JSAsyncGeneratorType);
    }
}
