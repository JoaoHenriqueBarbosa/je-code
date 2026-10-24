//! Porte de `runtime/JSLexicalEnvironment.h`, `JSLexicalEnvironment.cpp` e `JSLexicalEnvironmentInlines.h`.
//!
//! DIVERGÊNCIAS: as variáveis ficam num `Vec<JSValue>` do tamanho `symbolTable()->scopeSize()` em vez
//! de logo depois do objeto (`allocationSize`, `offsetOfVariables`, `offsetOfVariable` existem só para
//! o layout de memória que o LLInt de offsets usa; o interpretador em Rust lê por `variable_at`).
//! `visitChildren` e `analyzeHeap` somem com o GC. `getOwnPropertySlot`, `put`, `deleteProperty` e
//! `getOwnSpecialPropertyNames` escrevem em `PropertySlot`/`PropertyNameArrayBuilder` e chamam
//! `JSObject`: entram com o `JSObject`. A parte de tabela de símbolos delas (`symbol_table_get`,
//! `symbol_table_put`) já está em `js_symbol_table_object`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_scope::JSScopeRef;
use crate::runtime::js_symbol_table_object::{JSSymbolTableObject, SymbolTableObjectVariables, JS_SYMBOL_TABLE_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::{js_null, js_tdz_value, js_undefined, JSValue};
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::symbol_table::SymbolTableRef;
use crate::runtime::js_type_info::{TypeInfo, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT};
use crate::runtime::vm::VM;

/// `const ClassInfo JSLexicalEnvironment::s_info`.
pub static JS_LEXICAL_ENVIRONMENT_S_INFO: ClassInfo = ClassInfo {
    class_name: "JSLexicalEnvironment",
    parent_class: Some(&JS_SYMBOL_TABLE_OBJECT_S_INFO),
    static_prop_hash_table: None, inherits_js_type_range: None,
};

/// `class JSLexicalEnvironment : public JSSymbolTableObject`.
#[derive(Debug)]
pub struct JSLexicalEnvironment {
    base: JSSymbolTableObject,
    variables: RefCell<Vec<JSValue>>,
}

pub type JSLexicalEnvironmentRef = Rc<JSLexicalEnvironment>;

impl std::ops::Deref for JSLexicalEnvironment {
    type Target = JSSymbolTableObject;

    fn deref(&self) -> &JSSymbolTableObject {
        &self.base
    }
}

impl JSLexicalEnvironment {
    /// `visitChildren`: as variáveis.
    pub(crate) fn visit_variables(&self, marker: &mut crate::runtime::gc::Marker) {
        match self.variables.try_borrow() {
            Ok(variables) => marker.values(&variables),
            Err(_) => marker.abort(),
        }
    }

    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnSpecialPropertyNames | OverridesPut`.
    pub const STRUCTURE_FLAGS: u32 =
        JSSymbolTableObject::STRUCTURE_FLAGS | OVERRIDES_GET_OWN_PROPERTY_SLOT | OVERRIDES_PUT;

    /// `JSLexicalEnvironment(vm, structure, currentScope, symbolTable, initialValue)`.
    pub(crate) fn new(
        vm: &VM,
        structure: StructureRef,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
    ) -> JSLexicalEnvironment {
        debug_assert!(initial_value == js_undefined() || initial_value == js_tdz_value());
        // Filling this with undefined/TDZEmptyValue is useful because that's what variables start out as.
        let scope_size = symbol_table.borrow().scope_size() as usize;
        JSLexicalEnvironment {
            base: JSSymbolTableObject::with_symbol_table(vm, structure, current_scope, symbol_table),
            variables: RefCell::new(vec![initial_value; scope_size]),
        }
    }

    /// `create(vm, structure, currentScope, symbolTable, initialValue)`.
    pub fn create(
        vm: &VM,
        structure: StructureRef,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
    ) -> JSLexicalEnvironmentRef {
        let result = Rc::new(JSLexicalEnvironment::new(vm, structure, current_scope, symbol_table, initial_value));
        cell_registry::set(result.cell_id(), CellEntry::Scope(JSScopeRef::LexicalEnvironment(Rc::clone(&result))));
        result
    }

    /// `create(vm, globalObject, currentScope, symbolTable, initialValue)`.
    pub fn create_in_global_object(
        vm: &VM,
        global_object: &JSGlobalObject,
        current_scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
        initial_value: JSValue,
    ) -> JSLexicalEnvironmentRef {
        let structure = global_object.activation_structure();
        JSLexicalEnvironment::create(vm, structure, current_scope, symbol_table, initial_value)
    }

    /// `createStructure(vm, globalObject)`.
    pub fn create_structure(vm: &VM, global_object: &JSGlobalObject) -> StructureRef {
        Structure::create(
            vm,
            Some(global_object),
            js_null(),
            TypeInfo::new(JSType::LexicalEnvironmentType, JSLexicalEnvironment::STRUCTURE_FLAGS),
            &JS_LEXICAL_ENVIRONMENT_S_INFO,
        )
    }
}

impl SymbolTableObjectVariables for JSLexicalEnvironment {
    fn symbol_table_object(&self) -> &JSSymbolTableObject {
        &self.base
    }

    /// `isValidScopeOffset(offset)`: `!!offset && offset.offset() < symbolTable()->scopeSize()`.
    fn is_valid_scope_offset(&self, offset: ScopeOffset) -> bool {
        offset.is_valid() && offset.offset() < self.base.symbol_table().borrow().scope_size()
    }

    /// `variableAt(offset).get()`.
    fn variable_at(&self, offset: ScopeOffset) -> JSValue {
        debug_assert!(self.is_valid_scope_offset(offset));
        self.variables.borrow()[offset.offset() as usize]
    }

    fn set_variable_at(&self, offset: ScopeOffset, value: JSValue) {
        debug_assert!(self.is_valid_scope_offset(offset));
        self.variables.borrow_mut()[offset.offset() as usize] = value;
    }
}
