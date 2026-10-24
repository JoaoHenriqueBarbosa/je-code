//! Porte de `runtime/JSArrayBuffer.h` e `JSArrayBuffer.cpp`: o `JSArrayBuffer` (o objeto JS que embrulha
//! um `ArrayBuffer`, de `ArrayBuffer` ou de `SharedArrayBuffer`), mais o que o `JSGlobalObject` guarda das
//! classes `ArrayBuffer`, `SharedArrayBuffer` e `DataView` (`ArrayBufferRealm`); o `JSValue::toIndex`
//! mora em `js_value_conversions.rs`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `JSGlobalObject` do porte não tem `m_arrayBufferStructure`, `m_sharedArrayBufferStructure` nem
//!   `m_typedArrayDataView` (os `LazyClassStructure`): o `ArrayBufferRealm` os guarda, num campo só
//!   (`array_buffer_realm`), e `init` os cria na ordem do `JSGlobalObject::init`. O
//!   `arrayBufferSpeciesGetterSetter` mora no construtor de cada classe (ver `array_buffer_constructor.rs`).
//! - `subspaceFor`, `visitChildrenImpl`, `estimatedSize`, `vm.heap.addReference` e o `m_associatedWasmMemoryWrapper`
//!   (WebAssembly) somem. O `registerWrapper` do `TypedArrayController` (`SimpleTypedArrayController`) é o
//!   `ArrayBuffer::set_wrapper`, e o `toJS` dele é `to_js_array_buffer`.
//! - `toJS` usa o `globalObject` léxico no lugar do `realm()` da visão (o mesmo, com um realm só).

use std::rc::Rc;

use crate::runtime::array_buffer::{ArrayBufferRef, ArrayBufferSharingMode};
use crate::runtime::array_buffer_constructor::ArrayBufferConstructor;
use crate::runtime::array_buffer_prototype::ArrayBufferPrototype;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::data_view_constructor::DataViewConstructor;
use crate::runtime::data_view_prototype::DataViewPrototype;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JSObjectRef, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSArrayBuffer::s_info`.
pub static JS_ARRAY_BUFFER_S_INFO: ClassInfo =
    ClassInfo { class_name: "ArrayBuffer", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSArrayBuffer final : public JSNonFinalObject`.
pub struct JSArrayBuffer {
    base: JSNonFinalObject,
    /// `m_impl`.
    impl_: ArrayBufferRef,
}

/// O `JSArrayBuffer*`.
pub type JSArrayBufferRef = Rc<JSArrayBuffer>;

impl std::fmt::Debug for JSArrayBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSArrayBuffer").field("cell_id", &self.base.cell_id()).field("impl", &self.impl_).finish()
    }
}

impl std::ops::Deref for JSArrayBuffer {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSArrayBuffer {
    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `create(vm, structure, buffer)`: o construtor, o `finishCreation` e o registro do embrulho no
    /// `ArrayBuffer` (o `registerWrapper` do `TypedArrayController`).
    pub fn create(vm: &VM, structure: &StructureRef, buffer: ArrayBufferRef) -> JSArrayBufferRef {
        let cell_id = cell_registry::reserve();
        let object = Rc::new(JSArrayBuffer { base: JSNonFinalObject::new(vm, Rc::clone(structure)), impl_: buffer });
        object.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ArrayBuffer(Rc::clone(&object)));
        object.impl_.set_wrapper(cell_id);
        debug_assert_eq!(object.type_(), JSType::ArrayBufferType);
        object
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ArrayBufferType, JSArrayBuffer::STRUCTURE_FLAGS),
            &JS_ARRAY_BUFFER_S_INFO,
        )
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSArrayBufferRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ArrayBuffer(object)) => Some(object),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSArrayBuffer>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSArrayBufferRef> {
        match value {
            JSValue::Cell(cell_id) => JSArrayBuffer::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `impl()`.
    pub fn impl_(&self) -> &ArrayBufferRef {
        &self.impl_
    }

    /// `toWrapped(vm, value)`: o buffer não compartilhado e de comprimento fixo.
    pub fn to_wrapped(value: JSValue) -> Option<ArrayBufferRef> {
        let result = to_unshared_array_buffer(value)?;
        if result.is_resizable_or_growable_shared() {
            return None;
        }
        Some(result)
    }

    /// `toWrappedAllowShared(vm, value)`.
    pub fn to_wrapped_allow_shared(value: JSValue) -> Option<ArrayBufferRef> {
        let result = to_possibly_shared_array_buffer(value)?;
        if result.is_resizable_or_growable_shared() {
            return None;
        }
        Some(result)
    }
}

/// `toPossiblySharedArrayBuffer(vm, value)` (que é também `toWrappedAllowSharedAndResizable`).
pub fn to_possibly_shared_array_buffer(value: JSValue) -> Option<ArrayBufferRef> {
    JSArrayBuffer::from_value(&value).map(|wrapper| Rc::clone(wrapper.impl_()))
}

/// `toUnsharedArrayBuffer(vm, value)`.
pub fn to_unshared_array_buffer(value: JSValue) -> Option<ArrayBufferRef> {
    to_possibly_shared_array_buffer(value).filter(|buffer| !buffer.is_shared())
}

/// `SimpleTypedArrayController::toJS(lexicalGlobalObject, globalObject, native)`: o embrulho que o buffer
/// já tem, ou um novo com a estrutura da classe do modo dele.
pub fn to_js_array_buffer(global_object: &JSGlobalObject, native: &ArrayBufferRef) -> JSArrayBufferRef {
    if let Some(buffer) = native.wrapper().and_then(JSArrayBuffer::from_cell_id) {
        return buffer;
    }

    // The JSArrayBuffer::create function will register the wrapper in finishCreation.
    let structure = global_object.array_buffer_realm.array_buffer_structure(native.sharing_mode());
    JSArrayBuffer::create(global_object.vm(), &structure, Rc::clone(native))
}

/// `LazyClassStructure`: o protótipo, a estrutura das instâncias e o construtor de uma classe.
#[derive(Clone, Debug)]
pub struct ClassStructure {
    pub prototype: JSObjectRef,
    pub structure: StructureRef,
    pub constructor: JSFunctionRef,
}

/// O que o `JSGlobalObject` guarda de `ArrayBuffer`, `SharedArrayBuffer` e `DataView` (ver as
/// DIVERGÊNCIAS do cabeçalho).
#[derive(Debug, Default)]
pub struct ArrayBufferRealm {
    /// `m_arrayBufferStructure` e `m_sharedArrayBufferStructure`, pelo `ArrayBufferSharingMode`.
    array_buffer: std::cell::RefCell<[Option<ClassStructure>; 2]>,
    /// `m_typedArrayDataView` (`typedArrayPrototype(TypeDataView)`, `typedArrayConstructor(TypeDataView)` e a
    /// estrutura `typedArrayStructure(TypeDataView, false)`).
    data_view: std::cell::RefCell<Option<ClassStructure>>,
    /// `m_resizableOrGrowableSharedTypedArrayDataViewStructure`.
    resizable_or_growable_shared_data_view_structure: std::cell::RefCell<Option<StructureRef>>,
    /// Os doze `TypedArray` e o `%TypedArray%` (`m_typedArrayProto`, `m_typedArraySuperConstructor`, os
    /// `m_typedArrayXxx` e as estruturas sobre buffer redimensionável).
    pub typed_arrays: crate::runtime::typed_array_realm::TypedArrayRealm,
}

impl ArrayBufferRealm {
    /// A classe `ArrayBuffer` (ou `SharedArrayBuffer`) já criada por `init`.
    fn array_buffer_class(&self, sharing_mode: ArrayBufferSharingMode) -> ClassStructure {
        self.array_buffer.borrow()[sharing_mode.index()].clone().expect("JSGlobalObject sem a classe ArrayBuffer")
    }

    /// `arrayBufferStructure(sharingMode)`.
    pub fn array_buffer_structure(&self, sharing_mode: ArrayBufferSharingMode) -> StructureRef {
        self.array_buffer_class(sharing_mode).structure
    }

    /// `arrayBufferPrototype(sharingMode)`.
    pub fn array_buffer_prototype(&self, sharing_mode: ArrayBufferSharingMode) -> JSObjectRef {
        self.array_buffer_class(sharing_mode).prototype
    }

    /// `arrayBufferConstructor(sharingMode)`.
    pub fn array_buffer_constructor(&self, sharing_mode: ArrayBufferSharingMode) -> JSFunctionRef {
        self.array_buffer_class(sharing_mode).constructor
    }

    /// `typedArrayStructureWithTypedArrayType<TypeDataView>()` e a variante `resizableOrGrowableShared...`.
    pub fn data_view_structure(&self, resizable_or_growable_shared: bool) -> StructureRef {
        if resizable_or_growable_shared {
            return self
                .resizable_or_growable_shared_data_view_structure
                .borrow()
                .clone()
                .expect("JSGlobalObject sem a classe DataView");
        }
        self.data_view.borrow().clone().expect("JSGlobalObject sem a classe DataView").structure
    }

    /// `init` do `m_arrayBufferStructure`/`m_sharedArrayBufferStructure` (`LazyClassStructure`): o
    /// protótipo com `didBecomePrototype`, a estrutura das instâncias, o construtor e o `constructor` do
    /// protótipo (`DontEnum`).
    fn init_array_buffer(
        &self,
        vm: &VM,
        global_object: &JSGlobalObject,
        sharing_mode: ArrayBufferSharingMode,
        object_prototype: &JSObjectRef,
        function_prototype: JSValue,
    ) -> JSFunctionRef {
        let prototype_structure = ArrayBufferPrototype::create_structure(vm, global_object, object_prototype.as_value());
        let prototype = ArrayBufferPrototype::create(vm, global_object, &prototype_structure, sharing_mode);
        prototype.did_become_prototype(vm);
        let structure = JSArrayBuffer::create_structure(vm, Some(global_object), prototype.as_value());
        let constructor_structure = ArrayBufferConstructor::create_structure(vm, global_object, function_prototype, sharing_mode);
        let constructor = ArrayBufferConstructor::create(vm, global_object, constructor_structure, &prototype, sharing_mode);
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.constructor),
            constructor.as_value(),
            DONT_ENUM,
        );
        self.array_buffer.borrow_mut()[sharing_mode.index()] =
            Some(ClassStructure { prototype, structure, constructor: Rc::clone(&constructor) });
        constructor
    }

    /// `init` do `m_typedArrayDataView` e do `m_resizableOrGrowableSharedTypedArrayDataViewStructure`.
    fn init_data_view(&self, vm: &VM, global_object: &JSGlobalObject, object_prototype: &JSObjectRef, function_prototype: JSValue) -> JSFunctionRef {
        let prototype_structure = DataViewPrototype::create_structure(vm, global_object, object_prototype.as_value());
        let prototype = DataViewPrototype::create(vm, global_object, &prototype_structure);
        prototype.did_become_prototype(vm);
        let structure = JSDataView::create_structure(vm, Some(global_object), prototype.as_value());
        let resizable_structure = JSDataView::create_resizable_or_growable_shared_structure(vm, Some(global_object), prototype.as_value());
        let constructor_structure = DataViewConstructor::create_structure(vm, global_object, function_prototype);
        let constructor = DataViewConstructor::create(vm, global_object, constructor_structure, &prototype);
        prototype.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.constructor),
            constructor.as_value(),
            DONT_ENUM,
        );
        DataViewPrototype::put_to_string_tag_tail(&prototype, vm);
        *self.data_view.borrow_mut() = Some(ClassStructure { prototype, structure, constructor: Rc::clone(&constructor) });
        *self.resizable_or_growable_shared_data_view_structure.borrow_mut() = Some(resizable_structure);
        constructor
    }

    /// As três classes e as propriedades globais `ArrayBuffer`, `SharedArrayBuffer` (só com
    /// `install_shared_array_buffer`, o `Options::useSharedArrayBuffer()`) e `DataView`, todas `DontEnum`.
    /// `JSGlobalObject::init` chama isto depois de criar o `Object.prototype` e o `Function.prototype`.
    pub fn init(
        &self,
        vm: &VM,
        global_object: &JSGlobalObject,
        object_prototype: &JSObjectRef,
        function_prototype: JSValue,
        install_shared_array_buffer: bool,
    ) {
        let names = &vm.property_names;
        let array_buffer =
            self.init_array_buffer(vm, global_object, ArrayBufferSharingMode::Default, object_prototype, function_prototype);
        let shared_array_buffer =
            self.init_array_buffer(vm, global_object, ArrayBufferSharingMode::Shared, object_prototype, function_prototype);
        let data_view = self.init_data_view(vm, global_object, object_prototype, function_prototype);
        self.typed_arrays.init(vm, global_object, object_prototype, function_prototype);

        let mut globals = vec![(names.array_buffer.clone(), array_buffer), (Identifier::from_span(vm, b"DataView"), data_view)];
        if install_shared_array_buffer {
            globals.push((names.shared_array_buffer.clone(), shared_array_buffer));
        }
        for (name, constructor) in globals {
            global_object.put_direct(vm, &PropertyName::from_identifier(&name), constructor.as_value(), DONT_ENUM);
        }
    }
}
