//! Porte de `runtime/JSModuleEnvironment.h` (`JSModuleEnvironment.cpp` traz `getOwnPropertySlot`,
//! `getOwnSpecialPropertyNames`, `put` e `deleteProperty`, que escrevem em `PropertySlot` e
//! `PropertyNameArrayBuilder` e entram com o `JSObject`, como no `js_lexical_environment`).
//!
//! DIVERGÊNCIAS:
//!
//! - `m_moduleRecord`, o `WriteBarrier<AbstractModuleRecord>` logo depois das variáveis
//!   (`offsetOfModuleRecord`, `allocationSize`, `moduleRecordSlot`), é um
//!   `Option<AbstractModuleRecordRef>` (`None` é o `nullptr`). `moduleRecord()` o devolve.
//! - `create(vm, globalObject, ...)` é `create_for_global_object`, sobre o `JSGlobalObject::moduleEnvironmentStructure()`
//!   (o `LazyProperty` de `js_global_object.rs`, criado na primeira leitura como o `initLater` do C++).
//! - O registro do escopo é o central (`CellEntry::Scope(JSScopeRef::ModuleEnvironment)`), feito em `create`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::abstract_module_record::{AbstractModuleRecordRef, ResolutionType};
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::current_realm::try_current_global_object;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_lexical_environment::{JSLexicalEnvironment, JS_LEXICAL_ENVIRONMENT_S_INFO};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_symbol_table_object::{
    symbol_table_get, symbol_table_put, JSSymbolTableObject, SymbolTableObjectVariables, SymbolTablePut,
};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES, OVERRIDES_PUT,
};
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol_table::SymbolTableRef;
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `const ClassInfo JSModuleEnvironment::s_info`.
pub static JS_MODULE_ENVIRONMENT_S_INFO: ClassInfo = ClassInfo {
    class_name: "JSModuleEnvironment",
    parent_class: Some(&JS_LEXICAL_ENVIRONMENT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSModuleEnvironment final : public JSLexicalEnvironment`.
#[derive(Debug)]
pub struct JSModuleEnvironment {
    base: JSLexicalEnvironment,
    /// `moduleRecordSlot()`: o `AbstractModuleRecord` (`None` é o `nullptr`).
    module_record: RefCell<Option<AbstractModuleRecordRef>>,
}

/// `JSModuleEnvironment*`.
pub type JSModuleEnvironmentRef = Rc<JSModuleEnvironment>;

impl std::ops::Deref for JSModuleEnvironment {
    type Target = JSLexicalEnvironment;

    fn deref(&self) -> &JSLexicalEnvironment {
        &self.base
    }
}

impl JSModuleEnvironment {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnSpecialPropertyNames | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 = JSLexicalEnvironment::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES
        | OVERRIDES_PUT;

    /// `JSModuleEnvironment(vm, structure, currentScope, symbolTable, initialValue, moduleRecord)`.
    fn new(
        vm: &VM,
        structure: StructureRef,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
        module_record: Option<AbstractModuleRecordRef>,
    ) -> JSModuleEnvironment {
        JSModuleEnvironment {
            base: JSLexicalEnvironment::new(vm, structure, current_scope, symbol_table, initial_value),
            module_record: RefCell::new(module_record),
        }
    }

    /// `create(vm, globalObject, currentScope, symbolTable, initialValue, moduleRecord)`: a `Structure` é a
    /// `globalObject->moduleEnvironmentStructure()`.
    pub fn create_for_global_object(
        vm: &VM,
        global_object: &JSGlobalObject,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
        module_record: Option<AbstractModuleRecordRef>,
    ) -> JSModuleEnvironmentRef {
        JSModuleEnvironment::create(vm, global_object.module_environment_structure(), current_scope, symbol_table, initial_value, module_record)
    }

    /// `create(vm, structure, currentScope, symbolTable, initialValue, moduleRecord)`.
    pub fn create(
        vm: &VM,
        structure: StructureRef,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
        module_record: Option<AbstractModuleRecordRef>,
    ) -> JSModuleEnvironmentRef {
        let result = Rc::new(JSModuleEnvironment::new(vm, structure, current_scope, symbol_table, initial_value, module_record));
        cell_registry::set(result.cell_id(), CellEntry::Scope(JSScopeRef::ModuleEnvironment(Rc::clone(&result))));
        result
    }

    /// `createStructure(vm, globalObject)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            js_null(),
            TypeInfo::new(JSType::ModuleEnvironmentType, JSModuleEnvironment::STRUCTURE_FLAGS),
            &JS_MODULE_ENVIRONMENT_S_INFO,
        )
    }

    /// `moduleRecord()`.
    pub fn module_record(&self) -> Option<AbstractModuleRecordRef> {
        self.module_record.borrow().clone()
    }

    /// O `resolveImport` que `getOwnPropertySlot`, `put` e `deleteProperty` do C++ fazem primeiro: o
    /// ambiente do módulo exportador e o nome local do binding lá, quando `key` é um import resolvido.
    fn resolve_import_binding(&self, key: &UniquedKey) -> Option<(JSModuleEnvironmentRef, Identifier)> {
        let record = self.module_record()?;
        let global_object = try_current_global_object()?;
        let resolution = record.resolve_import(&global_object, &Identifier::from_uid(global_object.vm(), Some(key)));
        if resolution.type_ != ResolutionType::Resolved {
            return None;
        }
        // When resolveImport resolves the resolution, the imported module environment must have the binding.
        let imported_environment = resolution.module_record?.module_environment_may_be_null()?;
        Some((imported_environment, resolution.local_name))
    }

    /// `JSModuleEnvironment::getOwnPropertySlot` como `symbolTableGet`: o import resolvido lê o slot do
    /// ambiente exportador (com os atributos dele); o resto é o `symbolTableGet` do `JSSymbolTableObject`.
    pub fn symbol_table_get_with_imports(&self, key: &UniquedKey) -> Option<(JSValue, u32)> {
        if let Some((imported_environment, local_name)) = self.resolve_import_binding(key) {
            return symbol_table_get(&*imported_environment, &local_name.impl_()?);
        }
        symbol_table_get(self, key)
    }

    /// `JSModuleEnvironment::put`: todo binding importado é imutável (o `TypeError` de
    /// `ReadonlyPropertyWriteError`); o resto é o `symbolTablePut` da base.
    pub fn symbol_table_put_with_imports(
        &self,
        key: &UniquedKey,
        value: JSValue,
        should_throw_read_only_error: bool,
        ignore_read_only_errors: bool,
    ) -> SymbolTablePut {
        if self.resolve_import_binding(key).is_some() {
            return SymbolTablePut::ReadOnly { should_throw: true };
        }
        symbol_table_put(self, key, value, should_throw_read_only_error, ignore_read_only_errors)
    }
}

impl SymbolTableObjectVariables for JSModuleEnvironment {
    fn symbol_table_object(&self) -> &JSSymbolTableObject {
        self.base.symbol_table_object()
    }

    fn is_valid_scope_offset(&self, offset: ScopeOffset) -> bool {
        self.base.is_valid_scope_offset(offset)
    }

    fn variable_at(&self, offset: ScopeOffset) -> JSValue {
        self.base.variable_at(offset)
    }

    fn set_variable_at(&self, offset: ScopeOffset, value: JSValue) {
        self.base.set_variable_at(offset, value)
    }
}
