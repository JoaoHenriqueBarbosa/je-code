//! Tradução de `runtime/JSModuleNamespaceObject.h`, `JSModuleNamespaceObjectInlines.h` e
//! `JSModuleNamespaceObject.cpp`: o objeto exótico de namespace de módulo (`import * as ns`).
//!
//! DIVERGÊNCIAS e LACUNAS, e por quê:
//!
//! - O `JSObject` do porte não tem tabela de métodos virtual (`methodTable()->getOwnPropertySlot`, `put`,
//!   `deleteProperty`, `getOwnPropertyNames`, `defineOwnProperty`), e o despacho dos exóticos mora em
//!   arquivos de outros donos. Em vez de sobrescrever os métodos, o namespace é um `JSNonFinalObject`
//!   cujas propriedades próprias são `CustomValue` (o mecanismo de `RegExp.$1` e de `Function.length`
//!   do JSC): cada export vira uma propriedade com atributo `DontDelete` (gravável e enumerável, como
//!   o descritor do namespace da ECMA-262) cujo getter nativo lê o binding vivo no `JSModuleEnvironment`
//!   exportador (`getValue` do C++), lançando o `ReferenceError` de TDZ quando o slot está vazio, e cujo
//!   setter nativo lança o `TypeError` de `ReadonlyPropertyWriteError` (o C++ só lança em modo estrito;
//!   o módulo é sempre estrito). As chaves entram ordenadas por ponto de código (o `std::ranges::sort`
//!   de `JSModuleNamespaceObject`'s construtor), então `[[OwnPropertyKeys]]` sai na ordem da spec, com
//!   `@@toStringTag` (`DontEnum | DontDelete | ReadOnly`) como a única chave símbolo. `preventExtensions`
//!   roda no fim de `create`. Os atributos de `defineOwnProperty` seguem as regras comuns do `JSObject`
//!   (`DontDelete` mais `CustomValue` as faz recusar a redefinição de configuração, que é o que a spec
//!   exige de quase todos os casos; o caso "mesmo valor" do C++ não é conferido).
//! - O getter nativo recebe só o `thisValue`, não o `slotBase`: um objeto que herda de um namespace
//!   (`Object.create(ns).x`) lê `undefined` aqui, o C++ lê o export do `slotBase`.
//! - Nomes de export que são índices (`export { a as "0" }`) não são propriedades da estrutura
//!   (`putDirectCustomAccessor` não aceita índice): o `JSObject` consulta [`exotic_of`] em
//!   `get_own_property_slot_by_index`, `put_by_index` e `delete_property_by_index` (o papel de
//!   `getOwnPropertySlotByIndex`, `putByIndex` e `deletePropertyByIndex` do C++), com a leitura viva do
//!   binding e o `ReferenceError` de TDZ. `[[OwnPropertyKeys]]` (`get_own_property_names`, chamado de
//!   `own_property_names.rs`) lista esses índices na ordem de `m_exports`, como o C++.
//! - `isDeferred` (`import defer`): `ensureDeferredNamespaceEvaluation` (`evaluateSync` do registro) roda na
//!   leitura de um export (o getter nativo), nos ganchos `ByIndex`, no `delete` por nome
//!   (`before_delete_property`) e no `[[OwnPropertyKeys]]`, e `then` não é export num namespace
//!   adiado (`isSymbolLikeNamespaceKey`). LACUNA: o `slot.internalMethodType() == VMInquiry`
//!   do getter nomeado não existe (o getter nativo não vê o `slot`).
//! - `USE(BUN_JSC_ADDITIONS)`: o protótipo com o acessor `__esModule` (`create_namespace_prototype`, medido no
//!   bun 1.4.2) existe; `overrideExports`, `m_isOverridingValue` além do flag do setter e o lazy export de
//!   `SyntheticModuleRecord` não existem.
//! - A `Structure` é a `globalObject->moduleNamespaceObjectStructure()` (`js_global_object.rs`).

use std::rc::Rc;

use crate::runtime::abstract_module_record::{AbstractModuleRecordRef, ModulePhase, Resolution, ResolutionType};
use crate::runtime::host_call::Thrown;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::custom_getter_setter::CustomGetterSetter;
use crate::runtime::error::create_type_error;
use crate::runtime::error_messages::{NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR, READONLY_PROPERTY_WRITE_ERROR};
use crate::runtime::operations::same_value;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::enumeration_mode::DontEnumPropertiesMode;
use crate::runtime::own_property_names::get_own_non_index_property_names;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::exception_helpers::create_tdz_error;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_record::{ModuleResult, Status as RecordStatus};
use crate::runtime::js_object::{JSNonFinalObject, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_symbol_table_object::symbol_table_get;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT};
use crate::runtime::js_value::{js_boolean, EncodedJSValue, JSValue};
use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::structure::Structure;
use crate::runtime::throw_scope::{throw_exception, ThrowScope};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSModuleNamespaceObject::s_info`.
pub static JS_MODULE_NAMESPACE_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "ModuleNamespaceObject", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSModuleNamespaceObject::ExportEntry` (`localName` e `moduleRecord`) mais o nome exportado, a chave
/// do `m_exports`.
#[derive(Debug)]
pub struct NamespaceExport {
    pub export_name: Identifier,
    pub local_name: Identifier,
    pub module_record: AbstractModuleRecordRef,
}

/// `class JSModuleNamespaceObject final : public JSNonFinalObject`.
pub struct JSModuleNamespaceObject {
    base: JSNonFinalObject,
    /// `m_exports`, em ordem de ponto de código.
    exports: Vec<NamespaceExport>,
    /// `m_moduleRecord`.
    module_record: AbstractModuleRecordRef,
    /// `m_isDeferred`.
    is_deferred: bool,
    /// O `__esModule` que o setter do protótipo do bun grava (`m_isOverridingValue`): `true` depois de uma
    /// atribuição de valor verdadeiro.
    es_module_override: std::cell::Cell<bool>,
}

/// O getter `__esModule` do protótipo do namespace (bun 1.4.2): `true` se o `this` é um namespace que
/// recebeu uma atribuição verdadeira, `undefined` em qualquer outro caso (inclusive `this` que não é namespace).
fn es_module_getter(_global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    match JSModuleNamespaceObject::from_value(&call_frame.this_value()) {
        Some(namespace) if namespace.es_module_override.get() => js_boolean(true).encode(),
        _ => JSValue::undefined().encode(),
    }
}

/// O setter `__esModule`: num namespace, um valor verdadeiro liga o override e um falso não faz nada (não
/// desliga); fora de um namespace (um objeto que herda dele, por exemplo) não faz nada. Nunca lança.
fn es_module_setter(_global_object: &JSGlobalObject, call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    if let Some(namespace) = JSModuleNamespaceObject::from_value(&call_frame.this_value()) {
        if call_frame.argument(0).to_boolean() {
            namespace.es_module_override.set(true);
        }
    }
    JSValue::undefined().encode()
}

/// O protótipo do namespace no bun: um objeto de protótipo nulo, extensível, com o único acessor
/// `__esModule` (get e set nativos, ambos de nome `__esModule`, `length` 0 e 1), `DontEnum | DontDelete`.
/// Sem `Symbol.toStringTag` nem `constructor`; é compartilhado por todos os namespaces do realm.
pub fn create_namespace_prototype(vm: &VM, global_object: &JSGlobalObject) -> JSValue {
    let prototype = crate::runtime::js_module_loader::new_null_prototype_object(global_object);
    let name = WtfString::from_latin1(b"__esModule");
    let make = |length: u32, function: crate::runtime::native_function::NativeFunction| {
        JSFunction::create_native(
            vm,
            global_object,
            length,
            &name,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        )
    };
    let accessor = GetterSetter::create_from_values(vm, make(0, es_module_getter).as_value(), make(1, es_module_setter).as_value());
    prototype.put_direct_non_index_accessor(
        vm,
        &PropertyName::from_identifier(&Identifier::from_string(vm, &name)),
        &accessor,
        ACCESSOR | DONT_ENUM | DONT_DELETE,
    );
    prototype.as_value()
}

/// `JSModuleNamespaceObject*`.
pub type JSModuleNamespaceObjectRef = Rc<JSModuleNamespaceObject>;

impl std::fmt::Debug for JSModuleNamespaceObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSModuleNamespaceObject").field("module_record", &self.module_record).finish_non_exhaustive()
    }
}

impl std::ops::Deref for JSModuleNamespaceObject {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

/// `getValue(environment, localName, scopeOffset)`: o valor vivo do binding, `None` (o `JSValue()` do
/// C++) quando o slot ainda está em TDZ ou o nome não está na tabela.
fn binding_value(export: &NamespaceExport) -> Option<JSValue> {
    let environment = export.module_record.module_environment_may_be_null()?;
    let key = export.local_name.impl_()?;
    let (value, _attributes) = symbol_table_get(&*environment, &key)?;
    if value.is_empty() {
        return None;
    }
    Some(value)
}

/// `exportEntry.moduleRecord->getModuleNamespace(globalObject)` quando o export é o `*namespace*` (a
/// materialização que o `getOwnPropertySlotCommon` faz antes de ler o ambiente) e então `getValue`:
/// `Ok(None)` é o slot em TDZ, `Err` a exceção pendente do `getModuleNamespace`.
fn export_binding(global_object: &JSGlobalObject, export: &NamespaceExport) -> Result<Option<JSValue>, ()> {
    if export.local_name == global_object.vm().property_names.star_namespace_private_name {
        // https://tc39.es/ecma262/#sec-module-namespace-exotic-objects-get-p-receiver
        // 10. If binding.[[BindingName]] is "*namespace*", then a. Return ? GetModuleNamespace(targetModule).
        pending_or_panic(export.module_record.get_module_namespace(global_object, ModulePhase::Evaluation, true))?;
    }
    Ok(binding_value(export))
}

/// O `RETURN_IF_EXCEPTION`: a exceção pendente vira `Err(())`; a lacuna do porte é `panic!`.
fn pending_or_panic<T>(result: ModuleResult<T>) -> Result<T, ()> {
    match result {
        Ok(value) => Ok(value),
        Err(Thrown::Unported(what)) => panic!("módulos: {what} ainda não portado"),
        Err(_) => Err(()),
    }
}

/// O `ReferenceError` de TDZ de `getOwnPropertySlotCommon` (`throwVMError(createTDZError(...))`).
fn throw_tdz(global_object: &JSGlobalObject, export: &NamespaceExport) {
    let mut scope = ThrowScope::new(global_object.vm());
    throw_exception(global_object, &mut scope, create_tdz_error(global_object, export.export_name.string()));
}

/// O `GetValueFunc` de cada export: `getOwnPropertySlotCommon` no ramo `Get`.
fn export_getter(global_object: &JSGlobalObject, this_value: EncodedJSValue, property_name: &PropertyName) -> EncodedJSValue {
    let this_value = JSValue::decode(this_value);
    let Some(namespace) = JSModuleNamespaceObject::from_value(&this_value) else {
        return JSValue::undefined().encode();
    };
    let Some(uid) = property_name.uid() else {
        return JSValue::undefined().encode();
    };
    let export = namespace.exports.iter().find(|export| export.export_name.impl_().as_ref() == Some(uid));
    let Some(export) = export else {
        return JSValue::undefined().encode();
    };
    if namespace.is_deferred && namespace.ensure_deferred_namespace_evaluation(global_object).is_err() {
        return JSValue::empty().encode();
    }
    match export_binding(global_object, export) {
        Err(()) => JSValue::empty().encode(),
        Ok(Some(value)) => value.encode(),
        Ok(None) => {
            throw_tdz(global_object, export);
            JSValue::empty().encode()
        }
    }
}

/// O `PutValueFunc` de cada export: `put` lança `ReadonlyPropertyWriteError`.
fn export_setter(global_object: &JSGlobalObject, _this_value: EncodedJSValue, _value: EncodedJSValue, _name: &PropertyName) -> bool {
    let mut scope = ThrowScope::new(global_object.vm());
    let error = create_type_error(global_object, &WtfString::from_latin1(READONLY_PROPERTY_WRITE_ERROR.as_bytes()));
    throw_exception(global_object, &mut scope, error);
    false
}

impl JSModuleNamespaceObject {
    /// `StructureFlags` (a parte que o porte representa).
    pub const STRUCTURE_FLAGS: u32 =
        JSNonFinalObject::STRUCTURE_FLAGS | OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT | IS_IMMUTABLE_PROTOTYPE_EXOTIC_OBJECT;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject, prototype: JSValue) -> crate::runtime::structure::StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            prototype,
            TypeInfo::new(JSType::ModuleNamespaceObjectType, JSModuleNamespaceObject::STRUCTURE_FLAGS),
            &JS_MODULE_NAMESPACE_OBJECT_S_INFO,
        )
    }

    /// `create(globalObject, structure, moduleRecord, resolutions, shouldPreventExtensions, isDeferred)`: o
    /// construtor (que ordena os exports) mais `finishCreation`.
    pub fn create(
        global_object: &JSGlobalObject,
        module_record: &AbstractModuleRecordRef,
        resolutions: Vec<(Identifier, Resolution)>,
        should_prevent_extensions: bool,
        is_deferred: bool,
    ) -> JSModuleNamespaceObjectRef {
        let vm = global_object.vm();

        let mut resolutions = resolutions;
        // `std::ranges::sort(resolutions, WTF::codePointCompareLessThan, ...)`: o UTF-8 compara na ordem
        // dos pontos de código.
        resolutions.sort_by(|a, b| a.0.utf8().cmp(&b.0.utf8()));
        let exports: Vec<NamespaceExport> = resolutions
            .into_iter()
            .filter_map(|(export_name, resolution)| {
                debug_assert!(resolution.type_ == ResolutionType::Resolved);
                let module_record = resolution.module_record?;
                Some(NamespaceExport { export_name, local_name: resolution.local_name, module_record })
            })
            .collect();

        let structure = global_object.module_namespace_object_structure();
        let cell_id = cell_registry::reserve();
        let object = Rc::new(JSModuleNamespaceObject {
            base: JSNonFinalObject::new(vm, structure),
            exports,
            module_record: Rc::clone(module_record),
            is_deferred,
            es_module_override: std::cell::Cell::new(false),
        });
        object.base.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ModuleNamespace(Rc::clone(&object)));

        // `finishCreation`.
        let tag = if is_deferred { "Deferred Module" } else { "Module" };
        object.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.to_string_tag_symbol),
            JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(tag.as_bytes()))),
            DONT_ENUM | DONT_DELETE | READ_ONLY,
        );
        for export in &object.exports {
            let name = PropertyName::from_identifier(&export.export_name);
            // Os exports de nome índice (`export { a as "0" }`) não são propriedades da estrutura: o
            // `getOwnPropertySlotByIndex` e os outros ganchos `ByIndex` os servem (`exotic_of`).
            if name.parse_index().is_some() {
                continue;
            }
            // IsSymbolLikeNamespaceKey: num namespace adiado `then` é uma chave comum (ausente), nunca
            // um export.
            if is_deferred && export.export_name == vm.property_names.then {
                continue;
            }
            let custom = CustomGetterSetter::create(vm, export_getter, Some(export_setter));
            object.put_direct_custom_accessor(vm, &name, &custom, DONT_DELETE);
        }
        if should_prevent_extensions {
            object.prevent_extensions(vm);
        }
        object
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSModuleNamespaceObjectRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::ModuleNamespace(object)) => Some(object),
            _ => None,
        }
    }

    /// O `JSModuleNamespaceObject*` de um `JSValue`.
    pub fn from_value(value: &JSValue) -> Option<JSModuleNamespaceObjectRef> {
        if !value.is_cell() {
            return None;
        }
        JSModuleNamespaceObject::from_cell_id(value.as_cell())
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `moduleRecord()`.
    pub fn module_record(&self) -> &AbstractModuleRecordRef {
        &self.module_record
    }

    /// `m_isDeferred`.
    pub fn is_deferred(&self) -> bool {
        self.is_deferred
    }

    /// Os nomes exportados, em ordem (o `m_exports` do C++).
    pub fn export_names(&self) -> impl Iterator<Item = &Identifier> {
        self.exports.iter().map(|export| &export.export_name)
    }

    /// `ensureDeferredNamespaceEvaluation(globalObject)`: `Err(())` com a exceção pendente.
    fn ensure_deferred_namespace_evaluation(&self, global_object: &JSGlobalObject) -> Result<(), ()> {
        // https://tc39.es/proposal-defer-import-eval/#sec-GetModuleExportsList
        // 1. If O.[[Deferred]] is true, then
        debug_assert!(self.is_deferred);
        // Fast path: if the module's cycle has already successfully evaluated, EvaluateModuleSync would
        // observe a fulfilled promise and return without throwing. We must consult [[CycleRoot]] here
        // because Evaluate() redirects to it.
        if self.module_record.is_cyclic() {
            let root = self.module_record.cycle_root().unwrap_or_else(|| Rc::clone(&self.module_record));
            if root.status() == RecordStatus::Evaluated && root.evaluation_error().is_none() {
                return Ok(());
            }
        }
        // 1.a. Let m be O.[[Module]]. 1.b. Perform ? EvaluateModuleSync(m).
        pending_or_panic(self.module_record.evaluate_sync(global_object))
    }

    /// `isSymbolLikeNamespaceKey(vm, propertyName)`.
    fn is_symbol_like_namespace_key(&self, vm: &VM, property_name: &PropertyName) -> bool {
        property_name.is_symbol() || (self.is_deferred && property_name.uid() == vm.property_names.then.impl_().as_ref())
    }

    /// O prefixo de `deleteProperty(cell, globalObject, propertyName, slot)`: num namespace adiado a
    /// avaliação roda antes do veredito, que o `JSObject` dá pela estrutura (todo export é `DontDelete`, o
    /// `!m_exports.contains(uid)` do C++). A chave símbolo e o `then` adiado seguem o `Base::deleteProperty`.
    pub fn before_delete_property(&self, vm: &VM, property_name: &PropertyName) -> Result<(), PutError> {
        if self.is_deferred && !self.is_symbol_like_namespace_key(vm, property_name) {
            let Some(global_object) = self.structure().realm() else { return Ok(()) };
            if self.ensure_deferred_namespace_evaluation(&global_object).is_err() {
                return Err(PutError::Pending);
            }
        }
        Ok(())
    }

    /// `getOwnPropertyNames(cell, globalObject, propertyNames, mode)`: os exports na ordem de `m_exports`
    /// (os de nome índice inclusive), depois os não índice da estrutura (`@@toStringTag`).
    pub fn get_own_property_names(
        &self,
        vm: &VM,
        property_names: &mut PropertyNameArrayBuilder<'_>,
        mode: DontEnumPropertiesMode,
    ) -> Result<(), Thrown> {
        // https://tc39.es/ecma262/#sec-module-namespace-exotic-objects-ownpropertykeys
        let Some(global_object) = self.structure().realm() else { return Ok(()) };
        let global_object: &JSGlobalObject = &global_object;
        if self.is_deferred && self.ensure_deferred_namespace_evaluation(global_object).is_err() {
            return Err(Thrown::Pending);
        }
        for export in &self.exports {
            if mode == DontEnumPropertiesMode::Exclude {
                // O `then` de um namespace adiado cai no `Base::getOwnPropertySlot`, que não o tem.
                if self.is_deferred && export.export_name == vm.property_names.then {
                    continue;
                }
                // Perform [[GetOwnProperty]] to throw ReferenceError if binding is uninitialized.
                match export_binding(global_object, export) {
                    Err(()) => return Err(Thrown::Pending),
                    Ok(None) => {
                        throw_tdz(global_object, export);
                        return Err(Thrown::Pending);
                    }
                    Ok(Some(_)) => {}
                }
            }
            property_names.add(&export.export_name);
        }
        if property_names.include_symbol_properties() {
            get_own_non_index_property_names(vm, self, property_names, mode);
        }
        Ok(())
    }

    /// O `ExportEntry` cujo nome é o índice `index` (`export { a as "0" }`).
    fn index_export(&self, index: u32) -> Option<&NamespaceExport> {
        self.exports.iter().find(|export| PropertyName::from_identifier(&export.export_name).parse_index() == Some(index))
    }

    /// O início de `getOwnPropertySlotCommon` para um nome que não é índice: num namespace adiado, a chave que não é
    /// de símbolo (nem o `then`) avalia o módulo (fora de `VMInquiry`). Falso com a exceção pendente.
    pub fn prepare_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> bool {
        if !self.is_deferred || self.is_symbol_like_namespace_key(vm, property_name) || slot.internal_method_type() == InternalMethodType::VMInquiry {
            return true;
        }
        let Some(global_object) = self.structure().realm() else { return true };
        self.ensure_deferred_namespace_evaluation(&global_object).is_ok()
    }

    /// `getOwnPropertySlotByIndex(cell, globalObject, index, slot)` (`getOwnPropertySlotCommon` com o nome
    /// `Identifier::from(vm, index)`): `None` quando `index` não é um export, para o `JSObject` comum
    /// responder (é a vez do vetor indexado, vazio num namespace).
    pub fn get_own_property_slot_by_index(&self, index: u32, slot: &mut PropertySlot) -> bool {
        let Some(global_object) = self.structure().realm() else { return false };
        let global_object: &JSGlobalObject = &global_object;
        slot.set_is_tainted_by_opaque_object();

        if self.is_deferred
            && slot.internal_method_type() != InternalMethodType::VMInquiry
            && self.ensure_deferred_namespace_evaluation(global_object).is_err()
        {
            return false;
        }

        let Some(export) = self.index_export(index) else { return false };
        match slot.internal_method_type() {
            InternalMethodType::GetOwnProperty | InternalMethodType::Get => match export_binding(global_object, export) {
                Err(()) => false,
                Ok(None) => {
                    throw_tdz(global_object, export);
                    false
                }
                Ok(Some(value)) => {
                    slot.set_value(self, DONT_DELETE, value);
                    true
                }
            },
            // Do not perform [[Get]] for [[HasProperty]]: it could throw while [[HasProperty]] just returns true.
            InternalMethodType::HasProperty => {
                slot.set_value(self, DONT_DELETE, JSValue::undefined());
                true
            }
            InternalMethodType::VMInquiry => {
                slot.set_value(self, 0, JSValue::undefined());
                false
            }
        }
    }

    /// `put(cell, globalObject, propertyName, value, slot)`: um namespace nunca aceita escrita; em modo
    /// estrito lança `ReadonlyPropertyWriteError`, fora dele devolve `false`.
    /// (http://www.ecma-international.org/ecma-262/6.0/#sec-module-namespace-exotic-objects-set-p-v-receiver)
    pub fn put(&self, should_throw: bool) -> Result<bool, PutError> {
        self.put_by_index(should_throw)
    }

    /// `defineOwnProperty(cell, globalObject, propertyName, descriptor, shouldThrow)`
    /// (https://tc39.es/ecma262/#sec-module-namespace-exotic-objects-defineownproperty-p-desc). `None` é o
    /// passo 1 (chave símbolo ou `then` adiado): segue o `[[DefineOwnProperty]]` ordinário.
    pub fn define_own_property(
        &self,
        vm: &VM,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        should_throw: bool,
    ) -> Option<Result<bool, PutError>> {
        if self.is_symbol_like_namespace_key(vm, property_name) {
            return None;
        }
        let fail = |message: &'static str| if should_throw { Err(PutError::TypeError(message)) } else { Ok(false) };

        // 2. Let current be ? O.[[GetOwnProperty]](P).
        let mut current = PropertyDescriptor::default();
        let is_current_defined = self.get_own_property_descriptor(vm, property_name, &mut current);
        if vm.has_exception() {
            return Some(Err(PutError::Pending));
        }

        // 3. If current is undefined, return false.
        if !is_current_defined {
            return Some(fail(NON_EXTENSIBLE_OBJECT_PROPERTY_DEFINE_ERROR));
        }
        // 4. If IsAccessorDescriptor(Desc) is true, return false.
        if descriptor.is_accessor_descriptor() {
            return Some(fail("Cannot change module namespace object's binding to accessor"));
        }
        // 5. If Desc.[[Writable]] is present and has value false, return false.
        if descriptor.writable_present() && !descriptor.writable() {
            return Some(fail("Cannot change module namespace object's binding to non-writable attribute"));
        }
        // 6. If Desc.[[Enumerable]] is present and has value false, return false.
        if descriptor.enumerable_present() && !descriptor.enumerable() {
            return Some(fail("Cannot replace module namespace object's binding with non-enumerable attribute"));
        }
        // 7. If Desc.[[Configurable]] is present and has value true, return false.
        if descriptor.configurable_present() && descriptor.configurable() {
            return Some(fail("Cannot replace module namespace object's binding with configurable attribute"));
        }
        // 8. If Desc.[[Value]] is present, return SameValue(Desc.[[Value]], current.[[Value]]).
        if !descriptor.value().is_empty() && !same_value(descriptor.value(), current.value()) {
            return Some(fail("Cannot replace module namespace object's binding's value"));
        }
        // 9. Return true.
        Some(Ok(true))
    }

    /// `putByIndex(cell, globalObject, index, value, shouldThrow)`.
    pub fn put_by_index(&self, should_throw: bool) -> Result<bool, PutError> {
        if should_throw {
            return Err(PutError::TypeError(READONLY_PROPERTY_WRITE_ERROR));
        }
        Ok(false)
    }

    /// `deletePropertyByIndex(cell, globalObject, index)`.
    pub fn delete_property_by_index(&self, index: u32) -> Result<bool, PutError> {
        if self.is_deferred {
            let Some(global_object) = self.structure().realm() else { return Ok(true) };
            if self.ensure_deferred_namespace_evaluation(&global_object).is_err() {
                return Err(PutError::Pending);
            }
        }
        Ok(self.index_export(index).is_none())
    }
}

/// O `JSModuleNamespaceObject` que `object` é, para os ganchos `ByIndex` do `JSObject` (a tabela de
/// métodos virtual do C++).
pub fn exotic_of(object: &JSObject) -> Option<JSModuleNamespaceObjectRef> {
    if object.type_() != JSType::ModuleNamespaceObjectType {
        return None;
    }
    JSModuleNamespaceObject::from_cell_id(object.cell_id())
}
