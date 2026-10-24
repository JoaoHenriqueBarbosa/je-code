//! Porte de `runtime/InternalFunction.h`, `InternalFunctionInlines.h` e `InternalFunction.cpp`: a
//! célula `InternalFunction` (um `JSNonFinalObject` com `[[Call]]`/`[[Construct]]` nativos), registrada
//! no `cell_registry` como `CellEntry::InternalFunction`.
//!
//! Fora desta fatia, e por quê:
//! - `getCallData`/`getConstructData` devolvem `CallData`, que o porte não tem; entram com ele (a
//!   condição que `getConstructData` testa é `construct_is_default`).
//! - `createSubclassStructure` só no caminho sem `FunctionRareData::internalFunctionAllocationStructure`
//!   (ver `create_subclass_structure`).
//! - `Debugger::didCreateInternalFunction`: o `Debugger` do porte ainda não tem o gancho.
//! - `createFunctionThatMasqueradesAsUndefined` não dispara `masqueradesAsUndefinedWatchpointSet`
//!   (o `JSGlobalObject` do porte não tem o conjunto de watchpoints).
//! - `visitChildren`, `subspaceFor`, `DECLARE_EXPORT_INFO` e os `offsetOf...`: maquinaria de GC e de
//!   layout para o JIT.
//!
//! DIVERGÊNCIAS: `m_globalObject` é `Option` num `RefCell` porque o `FunctionPrototype` nasce antes do
//! global (ver `js_global_object_init.rs`) e recebe o realm depois, com `set_global_object`.
//! `m_functionForConstruct == callHostFunctionAsConstructor` vira a comparação do ponteiro da função
//! com `call_host_function_as_constructor` (`js_function.rs`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::code_specialization_kind::CodeSpecializationKind;
use crate::runtime::js_function::call_host_function_as_constructor;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::host_call::Thrown;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_remote_function::realm_of;
use crate::runtime::proxy_object::ProxyObject;
use crate::runtime::js_string::{js_string, JSStringRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, IMPLEMENTS_DEFAULT_HAS_INSTANCE, IMPLEMENTS_HAS_INSTANCE, MASQUERADES_AS_UNDEFINED, OVERRIDES_GET_CALL_DATA,
};
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::{to_tagged, NativeFunction, TaggedNativeFunction};
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo InternalFunction::s_info`.
pub static INTERNAL_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `enum class InternalFunction::PropertyAdditionMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyAdditionMode {
    WithStructureTransition,
    WithoutStructureTransition,
}

/// `class InternalFunction : public JSNonFinalObject`.
pub struct InternalFunction {
    base: JSNonFinalObject,
    function_for_call: Cell<TaggedNativeFunction>,
    function_for_construct: Cell<TaggedNativeFunction>,
    original_name: RefCell<Option<JSStringRef>>,
    global_object: RefCell<Option<JSGlobalObjectRef>>,
}

/// O `Debug` não desce no `m_globalObject`: o `JSGlobalObject` guarda o `m_functionPrototype`
/// (um `InternalFunction`), então imprimir o realm daria ciclo.
impl std::fmt::Debug for InternalFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalFunction")
            .field("base", &self.base)
            .field("function_for_call", &self.function_for_call.get())
            .field("function_for_construct", &self.function_for_construct.get())
            .field("original_name", &self.original_name)
            .finish_non_exhaustive()
    }
}

/// O `InternalFunction*`.
pub type InternalFunctionRef = Rc<InternalFunction>;

impl std::ops::Deref for InternalFunction {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl InternalFunction {
    /// `StructureFlags = Base::StructureFlags | ImplementsHasInstance | ImplementsDefaultHasInstance | OverridesGetCallData`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS
        | IMPLEMENTS_HAS_INSTANCE
        | IMPLEMENTS_DEFAULT_HAS_INSTANCE
        | OVERRIDES_GET_CALL_DATA;

    /// `createStructure(vm, globalObject, prototype)` (`InternalFunctionInlines.h`).
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS),
            &INTERNAL_FUNCTION_S_INFO,
        )
    }

    /// `InternalFunction(vm, structure, functionForCall, functionForConstruct = nullptr)`: constrói e
    /// registra a célula. O `finishCreation` é separado, como no C++. `None` em `function_for_construct`
    /// é o `nullptr` (vira `callHostFunctionAsConstructor`).
    pub fn new(
        vm: &VM,
        structure: StructureRef,
        function_for_call: NativeFunction,
        function_for_construct: Option<NativeFunction>,
    ) -> InternalFunctionRef {
        let cell_id = cell_registry::reserve();
        let function = Rc::new(InternalFunction::construct(vm, structure, function_for_call, function_for_construct));
        function.base.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::InternalFunction(Rc::clone(&function)));
        debug_assert!(matches!(function.type_(), JSType::InternalFunctionType | JSType::NullSetterFunctionType));
        function
    }

    /// O construtor `InternalFunction(vm, structure, ...)` sem registrar a célula: a subclasse que embute
    /// este valor (`ProxyRevoke`) se registra sozinha com a variante dela e grava o `cell_id` na base.
    pub fn construct(
        vm: &VM,
        structure: StructureRef,
        function_for_call: NativeFunction,
        function_for_construct: Option<NativeFunction>,
    ) -> InternalFunction {
        let global_object = structure.realm();
        InternalFunction {
            base: JSNonFinalObject::new(vm, structure),
            function_for_call: Cell::new(to_tagged(function_for_call)),
            function_for_construct: Cell::new(to_tagged(
                function_for_construct.unwrap_or(call_host_function_as_constructor),
            )),
            original_name: RefCell::new(None),
            global_object: RefCell::new(global_object),
        }
    }

    /// `finishCreation(vm, length, name, nameAdditionMode)`.
    pub fn finish_creation(&self, vm: &VM, length: u32, name: &WtfString, name_addition_mode: PropertyAdditionMode) {
        self.base.finish_creation(vm);

        let name_string = js_string(vm, name);
        *self.original_name.borrow_mut() = Some(Rc::clone(&name_string));
        // The enumeration order is length followed by name. So, we make sure to add the properties in that order.
        let length_name = PropertyName::from_identifier(&vm.property_names.length);
        let name_name = PropertyName::from_identifier(&vm.property_names.name);
        let attributes = READ_ONLY | DONT_ENUM;
        // Com `WithoutStructureTransition` a `Structure` fica `Unknown` (sem `PropertyAddition`), o que o
        // `Structure::dump` mostra no `jneq_ptr` do `new Array(n)`.
        match name_addition_mode {
            PropertyAdditionMode::WithStructureTransition => {
                self.put_direct(vm, &length_name, js_number(length), attributes);
                self.put_direct(vm, &name_name, JSValue::from_js_string(name_string), attributes);
            }
            PropertyAdditionMode::WithoutStructureTransition => {
                self.put_direct_without_transition(vm, &length_name, js_number(length), attributes);
                self.put_direct_without_transition(vm, &name_name, JSValue::from_js_string(name_string), attributes);
            }
        }
    }

    /// `createFunctionThatMasqueradesAsUndefined(vm, globalObject, length, name, nativeFunction)`.
    pub fn create_function_that_masquerades_as_undefined(
        vm: &VM,
        global_object: &JSGlobalObject,
        length: u32,
        name: &WtfString,
        native_function: NativeFunction,
    ) -> InternalFunctionRef {
        let structure = Structure::create(
            vm,
            Some(global_object),
            global_object.object_prototype().as_value(),
            TypeInfo::new(JSType::InternalFunctionType, InternalFunction::STRUCTURE_FLAGS | MASQUERADES_AS_UNDEFINED),
            &INTERNAL_FUNCTION_S_INFO,
        );
        let function = InternalFunction::new(vm, structure, native_function, None);
        function.finish_creation(vm, length, name, PropertyAdditionMode::WithoutStructureTransition);
        function
    }

    /// O `m_originalName` de um construtor que o bun cria já com o nome qualificado (`WebAssembly.Suspending`):
    /// no C++ o nome entra uma vez só em `finishCreation`; aqui a classe é instalada por `IntlClass` com o nome
    /// curto e o qualificado substitui o guardado (o `Function.prototype.toString` lê este).
    pub fn set_original_name(&self, vm: &VM, name: &WtfString) {
        *self.original_name.borrow_mut() = Some(js_string(vm, name));
    }

    /// `name()`: o `m_originalName` não é rope (foi feito de uma `String`).
    pub fn name(&self) -> WtfString {
        match &*self.original_name.borrow() {
            Some(original_name) => original_name.try_get_value(),
            None => WtfString::default(),
        }
    }

    /// `displayName(vm)`.
    pub fn display_name(&self, vm: &VM) -> WtfString {
        let display_name =
            self.get_direct_by_name(vm, &PropertyName::from_identifier(&vm.property_names.display_name));
        if !display_name.is_empty() && display_name.is_string() {
            return display_name.as_js_string().try_get_value();
        }
        WtfString::default()
    }

    /// `calculatedDisplayName(vm)`.
    pub fn calculated_display_name(&self, vm: &VM) -> WtfString {
        let explicit_name = self.display_name(vm);
        if !explicit_name.is_empty() {
            return explicit_name;
        }
        self.name()
    }

    /// `nativeFunctionFor(CodeSpecializationKind)`.
    pub fn native_function_for(&self, kind: CodeSpecializationKind) -> TaggedNativeFunction {
        if kind == CodeSpecializationKind::CodeForCall {
            return self.function_for_call.get();
        }
        debug_assert!(kind == CodeSpecializationKind::CodeForConstruct);
        self.function_for_construct.get()
    }

    /// `setNativeFunctionForDebugger(kind, function)`.
    pub fn set_native_function_for_debugger(&self, kind: CodeSpecializationKind, function: TaggedNativeFunction) {
        if kind == CodeSpecializationKind::CodeForCall {
            self.function_for_call.set(function);
        } else {
            debug_assert!(kind == CodeSpecializationKind::CodeForConstruct);
            self.function_for_construct.set(function);
        }
    }

    /// A condição de `getConstructData`: `m_functionForConstruct == callHostFunctionAsConstructor`
    /// (a função não é construtora).
    pub fn construct_is_default(&self) -> bool {
        self.function_for_construct.get() == to_tagged(call_host_function_as_constructor)
    }

    /// `globalObject()`.
    pub fn global_object(&self) -> JSGlobalObjectRef {
        self.global_object.borrow().clone().expect("InternalFunction sem realm")
    }

    /// Solta o realm no desmonte do programa (`cell_registry::remove_all_of`): o global guarda construtores e o
    /// `FunctionPrototype`, e cada um guarda o global, um ciclo de `Rc` que o `~VM` do C++ não tem.
    pub(crate) fn clear_global_object(&self) {
        let released = self.global_object.borrow_mut().take();
        drop(released);
    }

    /// Completa o `m_globalObject` de quem nasceu antes do realm (o `FunctionPrototype`).
    pub(crate) fn set_global_object(&self, global_object: JSGlobalObjectRef) {
        *self.global_object.borrow_mut() = Some(global_object);
    }

    /// O `InternalFunction*` do `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<InternalFunctionRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::InternalFunction(function)) => Some(function),
            _ => None,
        }
    }

    /// `createSubclassStructure(globalObject, newTarget, baseClass)`, no caminho de `newTarget` que não é
    /// `JSFunction` com perfil de alocação (`FunctionRareData` não existe aqui): lê `newTarget.prototype`
    /// e, sendo um objeto comum (`CellEntry::Object`), cria (ou acha no cache) a estrutura sobre ele a
    /// partir de `base_class`. Outro tipo de protótipo devolve `base_class` (o `StructureCache` só
    /// aceita `JSObjectRef`). O getter de `prototype` que lança é o `RETURN_IF_EXCEPTION`: devolve
    /// `Err(Pending)` com a exceção no `VM`.
    pub fn create_subclass_structure(
        global_object: &JSGlobalObject,
        new_target: &crate::runtime::host_function_support::ObjectRef,
        mut base_class: StructureRef,
    ) -> Result<StructureRef, Thrown> {
        let vm = global_object.vm();
        let prototype_value = new_target.get(global_object, &PropertyName::from_identifier(&vm.property_names.prototype));
        if vm.exception().is_some() {
            return Err(Thrown::Pending);
        }
        // O getter de `.prototype` pode ter causado o bad time: reconfere a estrutura de array.
        let base_global_object = base_class.realm();
        let base_global_object: &JSGlobalObject = base_global_object.as_deref().unwrap_or(global_object);
        if base_global_object.is_having_a_bad_time() && base_global_object.is_original_array_structure(&base_class) {
            base_class = base_global_object.array_structure_for_indexing_type_during_allocation(base_class.indexing_type());
        }
        let JSValue::Cell(cell_id) = prototype_value else {
            return Ok(base_class);
        };
        Ok(match cell_registry::get(cell_id) {
            Some(CellEntry::Object(prototype)) => base_global_object.structure_cache().empty_structure_for_prototype_from_base_structure(
                base_global_object,
                &prototype,
                &base_class,
                crate::runtime::structure_cache::ShouldCacheStructure::Yes,
            ),
            _ => base_class,
        })
    }
}

/// `getFunctionRealm(globalObject, object)` (https://tc39.es/ecma262/#sec-getfunctionrealm): desce pelo
/// alvo de `JSBoundFunction`, `JSRemoteFunction` e `ProxyObject` até o objeto cujo `realm()` vale. Um
/// `Proxy` revogado lança `TypeError`. O `globalObject` do C++ só serve ao `throwTypeError`, que aqui é o
/// `Thrown` devolvido, então o parâmetro não existe. `object` é chamável (`ASSERT(object->isCallable())`).
pub fn get_function_realm(object: JSValue) -> Result<JSGlobalObjectRef, Thrown> {
    // O `ASSERT(object->isCallable())` do C++ vira erro de verdade: o `isCallable` do porte vem do `JSType`
    // do cabeçalho, e um objeto fora dessa lista (ou um valor que nem é objeto) não pode derrubar o processo.
    let mut object = object;
    loop {
        if !object.is_object() {
            return Err(Thrown::type_error("Cannot get function realm from a non-object"));
        }
        if let Some(function) = object.as_js_function() {
            if let Some(bound) = function.as_bound_function() {
                object = bound.target_function();
                continue;
            }
            if let Some(remote) = function.as_remote_function() {
                object = remote.target_function();
                continue;
            }
        }
        if let Some(proxy) = ProxyObject::from_value(&object) {
            if proxy.is_revoked() {
                return Err(Thrown::type_error("Cannot get function realm from revoked Proxy"));
            }
            object = proxy.target();
            continue;
        }
        // `realm_of` lê a `Structure` do objeto: sem realm (célula que o registro não expõe como função)
        // é o mesmo TypeError, não um `expect`.
        let has_realm = object.as_js_function().is_some() || crate::runtime::host_function_support::ObjectRef::from_value(&object)
            .is_some_and(|reference| reference.structure().realm().is_some());
        if !has_realm {
            return Err(Thrown::type_error("Cannot get function realm of an object without a realm"));
        }
        return Ok(realm_of(object));
    }
}

/// `JSC_GET_DERIVED_STRUCTURE` completo: `newTarget == constructor` usa o `globalObject`; senão pega o
/// `getFunctionRealm(newTarget)` (que pode lançar) e `base_class` lê a estrutura base desse realm (o
/// `functionGlobalObject->structureMemberFunctionName()`).
pub fn get_derived_structure_in_realm(
    global_object: &JSGlobalObject,
    new_target: JSValue,
    constructor: usize,
    base_class: impl FnOnce(&JSGlobalObject) -> StructureRef,
) -> Result<StructureRef, Thrown> {
    if new_target == JSValue::from_cell(constructor) {
        return Ok(base_class(global_object));
    }
    let new_target_object =
        crate::runtime::host_function_support::ObjectRef::from_value(&new_target).expect("asObject(newTarget)");
    let function_global_object = get_function_realm(new_target)?;
    InternalFunction::create_subclass_structure(global_object, &new_target_object, base_class(&function_global_object))
}
