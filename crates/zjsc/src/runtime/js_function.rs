//! Porte de `runtime/JSFunction.h` (e do que `JSFunction.cpp`/`JSFunctionInlines.h` definem para criar
//! uma função de JavaScript e lê-la pelo interpretador: `create`,
//! `createWithInvalidatedReallocationWatchpoint`, `selectStructureForNewFuncExp`, `executable`,
//! `jsExecutable`, `isHostFunction`, `scope`, os dados de rara e as quatro `createStructure`).
//!
//! Os `reifyLazy*`, `getOwnPropertySlot`, `deleteProperty`, `defineOwnProperty`, `originalName`/
//! `originalLength` e `constructPrototypeObject` moram em `js_function_reify.rs` (mais um `impl
//! JSFunction`); o `JSBoundFunction` mora em `js_bound_function.rs`.
//!
//! `put` e `getOwnSpecialPropertyNames` moram em `js_function_reify.rs`. Fora desta fatia, e por quê:
//! `name`/`displayName`/`calculatedDisplayName`, `prototypeForConstruction`, `getCallData`/`getConstructData` e
//! `setFunctionName`. `JSRemoteFunction` mora em `js_remote_function.rs` (campo `remote`, como o `bound`).
//!
//! DIVERGÊNCIAS: `m_executableOrRareData` (ponteiro etiquetado com `rareDataTag`) é o enum
//! `ExecutableOrRareData`. `ASSERT_ENABLED`/`assertTypeInfoFlagInvariants` some. `visitChildren` some.
//! `JSBoundFunction` é subclasse de `JSFunction` no C++ e compartilha o `CellEntry::Function` (para
//! `as_js_function`, `getCallData` e o resto do porte continuarem alcançando a célula): os dados
//! próprios dele moram no campo `bound`, e `inherits<JSBoundFunction>` é `as_bound_function().is_some()`.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_custom_accessor_function::CustomAccessorFunction;
use crate::runtime::js_bound_function::JSBoundFunction;
use crate::runtime::js_remote_function::JSRemoteFunction;
use crate::runtime::native_executable::NativeExecutableRef;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::executable::ExecutableBaseRef;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_object::JSObject;
use crate::runtime::property_name::PropertyName;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::PropertyAdditionMode;
use crate::runtime::native_function::NativeFunction;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::exception_helpers::create_not_a_constructor_error;
use crate::runtime::js_string::JSStringRef;
use crate::runtime::js_string_builder::js_make_nontrivial_string;
use crate::runtime::throw_scope::{throw_vm_error, ThrowScope};
use crate::runtime::js_value::EncodedJSValue;
use crate::runtime::function_executable::FunctionExecutable;
use crate::runtime::function_rare_data::{FunctionRareData, FunctionRareDataRef};
use crate::runtime::script_executable::ScriptExecutableRef;

/// `FunctionExecutable*`: o `FunctionExecutable` compartilhado, como `ScriptExecutableRef::Function`.
pub type FunctionExecutableRef = Rc<RefCell<FunctionExecutable>>;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_callee::{JSCallee, JS_CALLEE_S_INFO};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_scope::{JSScope, JSScopeRef};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::js_type_info::{
    TypeInfo, OVERRIDES_GET_CALL_DATA, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::vm::VM;

/// `const ClassInfo JSFunction::s_info`.
pub static JS_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_CALLEE_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
/// `const ClassInfo JSStrictFunction::s_info`.
pub static JS_STRICT_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
/// `const ClassInfo JSSloppyFunction::s_info`.
pub static JS_SLOPPY_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
/// `const ClassInfo JSArrowFunction::s_info`.
pub static JS_ARROW_FUNCTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&JS_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `m_executableOrRareData`: o `ExecutableBase*` ou, com `rareDataTag`, o `FunctionRareData*`.
#[derive(Clone)]
enum ExecutableOrRareData {
    Executable(ExecutableBaseRef),
    RareData(FunctionRareDataRef),
}

/// `class JSFunction : public JSCallee`.
pub struct JSFunction {
    base: JSCallee,
    executable_or_rare_data: RefCell<ExecutableOrRareData>,
    /// Os campos do `JSBoundFunction` (ver a DIVERGÊNCIA do cabeçalho); vazio nas demais funções.
    bound: OnceCell<JSBoundFunction>,
    /// Os campos do `JSRemoteFunction` (mesma DIVERGÊNCIA do `bound`); vazio nas demais funções.
    remote: OnceCell<JSRemoteFunction>,
    /// Os `m_internalFields` do `JSFunctionWithFields` (ver `js_function_with_fields.rs`); vazio nas demais.
    pub(crate) fields: OnceCell<[Cell<JSValue>; 2]>,
    /// Os campos do `JSCustomGetterFunction`/`JSCustomSetterFunction` (`js_custom_accessor_function.rs`);
    /// vazio nas demais.
    pub(crate) custom_accessor: OnceCell<CustomAccessorFunction>,
}

/// Referência compartilhada, o `JSFunction*` do C++.
pub type JSFunctionRef = Rc<JSFunction>;

/// O `Debug` não desce no escopo nem no executável: o escopo pode ser o `JSGlobalObject`, que guarda
/// funções (ciclo), e o executável não implementa `Debug`.
impl std::fmt::Debug for JSFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSFunction").field("cell_id", &self.base.cell_id()).field("bound", &self.bound.get().is_some()).finish_non_exhaustive()
    }
}

impl std::ops::Deref for JSFunction {
    type Target = JSCallee;

    fn deref(&self) -> &JSCallee {
        &self.base
    }
}

impl JSFunction {
    /// `visitChildren` de `JSFunction` e das subclasses que o porte guarda nela (`JSBoundFunction`,
    /// `JSRemoteFunction`, `JSFunctionWithFields`). O escopo é `Rc`, já raiz.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        if let Some(bound) = self.bound.get() {
            bound.visit_children(marker);
        }
        if let Some(remote) = self.remote.get() {
            remote.visit_children(marker);
        }
        if let Some(fields) = self.fields.get() {
            marker.values(&[fields[0].get(), fields[1].get()]);
        }
    }
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnSpecialPropertyNames | OverridesGetCallData | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSCallee::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_GET_CALL_DATA
        | OVERRIDES_PUT;

    /// `selectStructureForNewFuncExp(globalObject, executable)`.
    pub fn select_structure_for_new_func_exp(global_object: &JSGlobalObject, executable: &FunctionExecutableRef) -> StructureRef {
        let executable = executable.borrow();
        debug_assert!(!executable.is_host_function());
        let is_builtin = executable.is_builtin_function();
        // Arrow functions will never have a prototype, so no need to check
        if executable.is_arrow_function() {
            return global_object.arrow_function_structure(is_builtin);
        }
        if executable.is_in_strict_context() {
            if executable.has_prototype_property() {
                return global_object.strict_function_structure(is_builtin);
            }
            return global_object.strict_method_structure(is_builtin);
        }
        if executable.has_prototype_property() {
            return global_object.sloppy_function_structure(is_builtin);
        }
        global_object.sloppy_method_structure(is_builtin)
    }

    /// `createImpl(vm, executable, scope, structure)`; `JSFunction(vm, executable, scope, structure)`
    /// é `Base(vm, scope, structure)` mais o `m_executableOrRareData`.
    pub(crate) fn create_impl(vm: &VM, executable: &FunctionExecutableRef, scope: JSScopeRef, structure: StructureRef) -> JSFunctionRef {
        let function = Rc::new(JSFunction {
            base: JSCallee::new(vm, scope, structure),
            executable_or_rare_data: RefCell::new(ExecutableOrRareData::Executable(ExecutableBaseRef::Script(
                ScriptExecutableRef::Function(Rc::clone(executable)),
            ))),
            bound: OnceCell::new(),
            remote: OnceCell::new(),
            fields: OnceCell::new(),
            custom_accessor: OnceCell::new(),
        });
        cell_registry::set(function.cell_id(), CellEntry::Function(Rc::clone(&function)));
        function
    }

    /// `create(vm, globalObject, executable, scope)`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, executable: &FunctionExecutableRef, scope: JSScopeRef) -> JSFunctionRef {
        let structure = JSFunction::select_structure_for_new_func_exp(global_object, executable);
        JSFunction::create_with_structure(vm, global_object, executable, scope, structure)
    }

    /// `create(vm, globalObject, executable, scope, structure)`.
    pub fn create_with_structure(
        vm: &VM,
        _global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
        structure: StructureRef,
    ) -> JSFunctionRef {
        let result = JSFunction::create_impl(vm, executable, scope, structure);
        executable.borrow_mut().notify_creation(vm, &result, "Allocating a function");
        result
    }

    /// `create(vm, globalObject, length, name, nativeFunction, implementationVisibility, intrinsic,
    /// nativeConstructor, signature)`: a função nativa. `signature` (DOMJIT) não existe. O chamador que
    /// no C++ omite o construtor passa `call_host_function_as_constructor`; os `name`/`length` ficam no
    /// `NativeExecutable` (as propriedades materializam preguiçosamente, o `reifyLazy*` ainda não foi
    /// portado).
    #[allow(clippy::too_many_arguments)]
    pub fn create_native(
        vm: &VM,
        global_object: &JSGlobalObject,
        length: u32,
        name: &WtfString,
        native_function: NativeFunction,
        implementation_visibility: ImplementationVisibility,
        intrinsic: Intrinsic,
        native_constructor: NativeFunction,
    ) -> JSFunctionRef {
        let structure = global_object.host_function_structure();
        JSFunction::create_native_with_structure(
            vm,
            global_object,
            structure,
            length,
            name,
            native_function,
            implementation_visibility,
            intrinsic,
            native_constructor,
        )
    }

    /// `create_native` numa `Structure` própria, irmã da `hostFunctionStructure()`: a função que ganha propriedades na
    /// instalação (o `prototype` de um construtor, um método pendurado, um `name` reificado) não pode transitar da
    /// estrutura compartilhada das funções de host. No C++ esses objetos são `InternalFunction` ou subclasses com
    /// estrutura própria, e a `hostFunctionStructure()` segue folha (`Leaf` no dump do bytecode, medido no bun 1.4.2).
    #[allow(clippy::too_many_arguments)]
    pub fn create_native_with_own_structure(
        vm: &VM,
        global_object: &JSGlobalObject,
        length: u32,
        name: &WtfString,
        native_function: NativeFunction,
        implementation_visibility: ImplementationVisibility,
        intrinsic: Intrinsic,
        native_constructor: NativeFunction,
    ) -> JSFunctionRef {
        let prototype = global_object.host_function_structure().stored_prototype();
        let structure = JSFunction::create_structure(vm, global_object, prototype);
        JSFunction::create_native_with_structure(
            vm,
            global_object,
            structure,
            length,
            name,
            native_function,
            implementation_visibility,
            intrinsic,
            native_constructor,
        )
    }

    /// `create_native` com a `Structure` escolhida pelo chamador (as subclasses de `JSFunction` com
    /// `HasStaticPropertyTable`, como `NumberConstructor`, trocam o `ClassInfo` e as flags).
    #[allow(clippy::too_many_arguments)]
    pub fn create_native_with_structure(
        vm: &VM,
        global_object: &JSGlobalObject,
        structure: StructureRef,
        length: u32,
        name: &WtfString,
        native_function: NativeFunction,
        implementation_visibility: ImplementationVisibility,
        intrinsic: Intrinsic,
        native_constructor: NativeFunction,
    ) -> JSFunctionRef {
        let executable =
            vm.get_host_function_with_intrinsic(native_function, implementation_visibility, intrinsic, native_constructor, length, name);
        let realm = structure.realm().expect("a hostFunctionStructure sempre tem realm");
        debug_assert!(std::ptr::eq(&*realm, global_object));
        let function = Rc::new(JSFunction {
            base: JSCallee::new(vm, JSScopeRef::GlobalObject(realm), structure),
            executable_or_rare_data: RefCell::new(ExecutableOrRareData::Executable(ExecutableBaseRef::Native(executable))),
            bound: OnceCell::new(),
            remote: OnceCell::new(),
            fields: OnceCell::new(),
            custom_accessor: OnceCell::new(),
        });
        cell_registry::set(function.cell_id(), CellEntry::Function(Rc::clone(&function)));
        function.finish_creation(vm);
        function
    }

    /// O construtor de `JSBoundFunction`: `Base(vm, executable, globalObject, structure)` (a função de
    /// host com o `globalObject` da `Structure` por escopo), mais `finishCreation`. Quem chama é
    /// `JSBoundFunction::create`.
    pub(crate) fn create_bound(
        vm: &VM,
        executable: NativeExecutableRef,
        structure: StructureRef,
        bound: JSBoundFunction,
    ) -> JSFunctionRef {
        JSFunction::create_derived(vm, executable, structure, OnceCell::from(bound), OnceCell::new())
    }

    /// O construtor de `JSRemoteFunction`: o mesmo `Base(vm, executable, globalObject, structure)` de
    /// `create_bound`. Quem chama é `JSRemoteFunction::try_create`.
    pub(crate) fn create_remote(
        vm: &VM,
        executable: NativeExecutableRef,
        structure: StructureRef,
        remote: JSRemoteFunction,
    ) -> JSFunctionRef {
        JSFunction::create_derived(vm, executable, structure, OnceCell::new(), OnceCell::from(remote))
    }

    /// O que `create_bound` e `create_remote` têm em comum: a função de host com o `globalObject` da
    /// `Structure` por escopo, registrada, mais `finishCreation`.
    fn create_derived(
        vm: &VM,
        executable: NativeExecutableRef,
        structure: StructureRef,
        bound: OnceCell<JSBoundFunction>,
        remote: OnceCell<JSRemoteFunction>,
    ) -> JSFunctionRef {
        let realm = structure.realm().expect("a Structure de função derivada sempre tem realm");
        let function = Rc::new(JSFunction {
            base: JSCallee::new(vm, JSScopeRef::GlobalObject(realm), structure),
            executable_or_rare_data: RefCell::new(ExecutableOrRareData::Executable(ExecutableBaseRef::Native(executable))),
            bound,
            remote,
            fields: OnceCell::new(),
            custom_accessor: OnceCell::new(),
        });
        cell_registry::set(function.cell_id(), CellEntry::Function(Rc::clone(&function)));
        function.finish_creation(vm);
        function
    }

    /// `dynamicDowncast<JSBoundFunction>(this)` / `inherits<JSBoundFunction>()`.
    pub fn as_bound_function(&self) -> Option<&JSBoundFunction> {
        self.bound.get()
    }

    /// `dynamicDowncast<JSRemoteFunction>(this)` / `inherits<JSRemoteFunction>()`.
    pub fn as_remote_function(&self) -> Option<&JSRemoteFunction> {
        self.remote.get()
    }

    /// `isRemoteFunction()`.
    pub fn is_remote_function(&self) -> bool {
        self.remote.get().is_some()
    }

    /// `createWithInvalidatedReallocationWatchpoint(vm, globalObject, executable, scope)`.
    pub fn create_with_invalidated_reallocation_watchpoint(
        vm: &VM,
        global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
    ) -> JSFunctionRef {
        let structure = JSFunction::select_structure_for_new_func_exp(global_object, executable);
        JSFunction::create_with_invalidated_reallocation_watchpoint_and_structure(vm, global_object, executable, scope, structure)
    }

    /// `createWithInvalidatedReallocationWatchpoint(vm, globalObject, executable, scope, structure)`.
    pub fn create_with_invalidated_reallocation_watchpoint_and_structure(
        vm: &VM,
        _global_object: &JSGlobalObject,
        executable: &FunctionExecutableRef,
        scope: JSScopeRef,
        structure: StructureRef,
    ) -> JSFunctionRef {
        debug_assert!(executable.borrow_mut().singleton().has_been_invalidated());
        JSFunction::create_impl(vm, executable, scope, structure)
    }

    /// `executable()`.
    pub fn executable(&self) -> ExecutableBaseRef {
        match &*self.executable_or_rare_data.borrow() {
            ExecutableOrRareData::RareData(rare_data) => rare_data.executable(),
            ExecutableOrRareData::Executable(executable) => executable.clone(),
        }
    }

    /// `jsExecutable()`: só vale para função que não é de host.
    pub fn js_executable(&self) -> FunctionExecutableRef {
        debug_assert!(!self.is_host_function());
        match self.executable() {
            ExecutableBaseRef::Script(ScriptExecutableRef::Function(executable)) => executable,
            _ => unreachable!("JSFunction::jsExecutable em função de host"),
        }
    }

    /// `isHostFunction()`: `ExecutableBase::isHostFunction()` (`type() == NativeExecutableType`).
    pub fn is_host_function(&self) -> bool {
        matches!(self.executable(), ExecutableBaseRef::Native(_))
    }

    /// `rareData()`.
    pub fn rare_data(&self) -> Option<FunctionRareDataRef> {
        match &*self.executable_or_rare_data.borrow() {
            ExecutableOrRareData::RareData(rare_data) => Some(Rc::clone(rare_data)),
            ExecutableOrRareData::Executable(_) => None,
        }
    }

    /// `ensureRareData(vm)`.
    pub fn ensure_rare_data(&self, vm: &VM) -> FunctionRareDataRef {
        if let Some(rare_data) = self.rare_data() {
            return rare_data;
        }
        self.allocate_rare_data(vm)
    }

    /// `allocateRareData(vm)`.
    fn allocate_rare_data(&self, vm: &VM) -> FunctionRareDataRef {
        let executable = match &*self.executable_or_rare_data.borrow() {
            ExecutableOrRareData::Executable(executable) => executable.clone(),
            ExecutableOrRareData::RareData(_) => unreachable!("allocateRareData com rareDataTag ligada"),
        };
        let rare_data = FunctionRareData::create(vm, executable);
        *self.executable_or_rare_data.borrow_mut() = ExecutableOrRareData::RareData(Rc::clone(&rare_data));
        rare_data
    }

    /// `scopeUnchecked()` e `scope()`: no C++ o primeiro vale para função de host (valor arbitrário).
    pub fn scope_unchecked(&self) -> Option<JSScopeRef> {
        self.base.scope()
    }

    /// `toString(globalObject)`: `None` é o `nullptr` com exceção pendente.
    pub fn to_string(&self, global_object: &JSGlobalObject) -> Option<JSStringRef> {
        if let Some(bound) = self.as_bound_function() {
            let name = bound.name(global_object.vm(), global_object).value();
            return js_make_nontrivial_string(global_object, &[&"function ", &name, &"() { [native code] }"]);
        }
        if let Some(remote) = self.as_remote_function() {
            let name = remote.name_string();
            return js_make_nontrivial_string(global_object, &[&"function ", &name, &"() { [native code] }"]);
        }
        if self.is_host_function() {
            let ExecutableBaseRef::Native(native) = self.executable() else {
                unreachable!("isHostFunction com executável que não é NativeExecutable");
            };
            let result = native.borrow_mut().to_string(global_object);
            return result;
        }
        FunctionExecutable::to_string(&self.js_executable(), global_object)
    }

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_FUNCTION_S_INFO)
    }
}

/// O corpo comum das quatro `createStructure` (`JSFunctionInlines.h`): `Structure::create` com
/// `TypeInfo(JSFunctionType, StructureFlags)` e o `info()` da classe. As quatro `StructureFlags`
/// valem o mesmo (`JSFunction::StructureFlags`).
pub(crate) fn function_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue, class_info: &'static ClassInfo) -> StructureRef {
    Structure::create(
        vm,
        Some(global_object),
        prototype,
        TypeInfo::new(JSType::JSFunctionType, JSFunction::STRUCTURE_FLAGS),
        class_info,
    )
}

/// `class JSStrictFunction final : public JSFunction`: só existe para ter o seu `ClassInfo` e a sua
/// `Structure` (as instâncias são `JSFunction`, no mesmo subespaço).
pub struct JSStrictFunction;

impl JSStrictFunction {
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_STRICT_FUNCTION_S_INFO)
    }
}

/// `class JSSloppyFunction final : public JSFunction`.
pub struct JSSloppyFunction;

impl JSSloppyFunction {
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_SLOPPY_FUNCTION_S_INFO)
    }
}

/// `class JSArrowFunction final : public JSFunction`.
pub struct JSArrowFunction;

impl JSArrowFunction {
    pub const STRUCTURE_FLAGS: u32 = JSFunction::STRUCTURE_FLAGS;

    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> StructureRef {
        function_structure(vm, global_object, prototype, &JS_ARROW_FUNCTION_S_INFO)
    }
}

/// `JSObject::putDirectNativeFunctionWithoutTransition(vm, globalObject, name, length, function,
/// implementationVisibility, intrinsic, attributes)` (`JSC_NATIVE_INTRINSIC_FUNCTION_WITHOUT_TRANSITION`):
/// cria a função nativa e a grava em `object` sem transição de `Structure` (`putDirectWithoutTransition`), como o
/// `finishCreation` dos construtores e protótipos: a estrutura do `Array` segue `Unknown`, não `PropertyAddition`.
#[allow(clippy::too_many_arguments)]
pub fn put_direct_native_function_without_transition(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &Identifier,
    length: u32,
    function: NativeFunction,
    implementation_visibility: ImplementationVisibility,
    intrinsic: Intrinsic,
    attributes: u32,
) -> JSFunctionRef {
    put_native_function(
        vm,
        global_object,
        object,
        name,
        name.string().string(),
        length,
        function,
        implementation_visibility,
        intrinsic,
        attributes,
        PropertyAdditionMode::WithoutStructureTransition,
    )
}

/// A função nativa gravada com `putDirect` (com transição) e com o `name` da função (o que `fn.name` devolve)
/// diferente da chave: `Buffer.from` tem `name` vazio e `toLocaleString` tem `name` "toString".
#[allow(clippy::too_many_arguments)]
pub fn put_direct_native_function_with_display_name(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    key: &Identifier,
    display_name: &WtfString,
    length: u32,
    function: NativeFunction,
    implementation_visibility: ImplementationVisibility,
    intrinsic: Intrinsic,
    attributes: u32,
) -> JSFunctionRef {
    put_native_function(
        vm,
        global_object,
        object,
        key,
        display_name,
        length,
        function,
        implementation_visibility,
        intrinsic,
        attributes,
        PropertyAdditionMode::WithStructureTransition,
    )
}

/// Cria a função nativa e a grava em `object` na chave `key`, com ou sem transição de `Structure` (`mode`).
#[allow(clippy::too_many_arguments)]
fn put_native_function(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    key: &Identifier,
    display_name: &WtfString,
    length: u32,
    function: NativeFunction,
    implementation_visibility: ImplementationVisibility,
    intrinsic: Intrinsic,
    attributes: u32,
    mode: PropertyAdditionMode,
) -> JSFunctionRef {
    let native_function = JSFunction::create_native(
        vm,
        global_object,
        length,
        display_name,
        function,
        implementation_visibility,
        intrinsic,
        call_host_function_as_constructor,
    );
    let key = PropertyName::from_identifier(key);
    match mode {
        PropertyAdditionMode::WithoutStructureTransition => object.put_direct_without_transition(vm, &key, native_function.as_value(), attributes),
        PropertyAdditionMode::WithStructureTransition => {
            object.put_direct(vm, &key, native_function.as_value(), attributes);
        }
    }
    native_function
}

/// `JSFunction::create(vm, globalObject, xxxCodeGenerator(vm), globalObject)`: o builtin JS `index` numa
/// `JSFunction` cujo escopo é o global (o `INIT_PRIVATE_GLOBAL` dos `LinkTimeConstant` escritos em JS).
pub fn create_builtin_function(vm: &VM, global_object: &JSGlobalObject, index: BuiltinCodeIndex) -> JSFunctionRef {
    let executable = vm.builtin_executables().code_generator(vm, index);
    let scope = JSScope::from_cell_id(global_object.cell_id()).expect("o JSGlobalObject é um JSScope registrado");
    JSFunction::create(vm, global_object, &executable, scope)
}

/// `JSObject::putDirectBuiltinFunctionWithoutTransition(vm, globalObject, name, functionExecutable,
/// attributes)` (`JSC_BUILTIN_FUNCTION_WITHOUT_TRANSITION`): liga o builtin JS `index` (o
/// `xxxCodeGenerator(vm)`) numa `JSFunction` cujo escopo é o global e a grava em `object`.
pub fn put_direct_builtin_function_without_transition(
    vm: &VM,
    global_object: &JSGlobalObject,
    object: &JSObject,
    name: &Identifier,
    index: BuiltinCodeIndex,
    attributes: u32,
) -> JSFunctionRef {
    let function = create_builtin_function(vm, global_object, index);
    object.put_direct_without_transition(vm, &PropertyName::from_identifier(name), function.as_value(), attributes);
    function
}

/// `JSC_DEFINE_HOST_FUNCTION(callHostFunctionAsConstructor)`: o `[[Construct]]` padrão de função
/// nativa que não é construtora. Serve também de marcador (`m_functionForConstruct ==
/// callHostFunctionAsConstructor`): `throwVMError(globalObject, scope,
/// createNotAConstructorError(globalObject, callFrame->jsCallee()))`.
pub fn call_host_function_as_constructor(global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let global_object: &JSGlobalObject = global_object;
    let mut scope = ThrowScope::new(global_object.vm());
    let callee = JSValue::from_cell(call_frame.js_callee());
    throw_vm_error(global_object, &mut scope, create_not_a_constructor_error(global_object, callee)).encode()
}
