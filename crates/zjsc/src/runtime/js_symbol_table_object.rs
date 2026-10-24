//! Porte de `runtime/JSSymbolTableObject.h`, `JSSymbolTableObject.cpp` e `JSSymbolTableObjectInlines.h`.
//!
//! DIVERGÊNCIAS:
//!
//! - `symbolTableGet`/`symbolTablePut` falam com `PropertySlot`, `PropertyDescriptor` e `ThrowScope`,
//!   ainda não portados. `symbol_table_get` devolve o valor e os atributos que o C++ grava no slot
//!   (`slot.setValue(object, attributes | DontDelete, value)`); a segunda sobrecarga (descritor)
//!   devolve também a entrada rápida. `symbol_table_put` devolve o que o C++ faz em três saídas
//!   (`false` sem tocar em nada, `true` com `putResult`, e o `throwTypeError(ReadonlyPropertyWriteError)`
//!   que fica a cargo de quem chama, que tem o `ThrowScope`).
//! - `symbolTablePutTouchWatchpointSet` e `symbolTablePutInvalidateWatchpointSet` só diferem no
//!   `InlineWatchpointSet` da entrada (`VariableWriteFireDetail::touch` contra `invalidate`), que
//!   serve ao JIT e não tem comportamento observável; as duas colapsam em `symbol_table_put`.
//! - `deleteProperty` e `getOwnSpecialPropertyNames` (que escrevem em `PropertyNameArrayBuilder` e
//!   chamam `JSObject::deleteProperty`) entram com o `JSObject`.
//! - `SymbolTable::notifyCreation` (singleton de watchpoint) não existe no `symbol_table.rs`; o
//!   `JSSymbolTableObject(vm, structure, scope, symbolTable)` e o `setSymbolTable` não o chamam.

use std::cell::RefCell;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_scope::{JSScope, JSScopeRef, JS_SCOPE_S_INFO};
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_DELETE;
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::structure::StructureRef;
use crate::runtime::symbol_table::{SymbolTableEntryFast, SymbolTableRef};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

/// `const ClassInfo JSSymbolTableObject::s_info`.
pub static JS_SYMBOL_TABLE_OBJECT_S_INFO: ClassInfo =
    ClassInfo { class_name: "SymbolTableObject", parent_class: Some(&JS_SCOPE_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class JSSymbolTableObject : public JSScope`.
#[derive(Debug)]
pub struct JSSymbolTableObject {
    base: JSScope,
    /// `m_symbolTable`: nulo só entre o construtor e o `setSymbolTable` do `finishCreation`.
    symbol_table: RefCell<Option<SymbolTableRef>>,
}

impl std::ops::Deref for JSSymbolTableObject {
    type Target = JSScope;

    fn deref(&self) -> &JSScope {
        &self.base
    }
}

impl JSSymbolTableObject {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnSpecialPropertyNames`.
    pub const STRUCTURE_FLAGS: u32 = JSScope::STRUCTURE_FLAGS | crate::runtime::js_type_info::OVERRIDES_GET_OWN_SPECIAL_PROPERTY_NAMES;

    /// `JSSymbolTableObject(VM&, Structure*, JSScope*)`.
    pub(crate) fn new(vm: &VM, structure: StructureRef, scope: Option<JSScopeRef>) -> JSSymbolTableObject {
        JSSymbolTableObject { base: JSScope::new(vm, structure, scope), symbol_table: RefCell::new(None) }
    }

    /// `JSSymbolTableObject(VM&, Structure*, JSScope*, SymbolTable*)`.
    pub(crate) fn with_symbol_table(
        vm: &VM,
        structure: StructureRef,
        scope: Option<JSScopeRef>,
        symbol_table: SymbolTableRef,
    ) -> JSSymbolTableObject {
        JSSymbolTableObject { base: JSScope::new(vm, structure, scope), symbol_table: RefCell::new(Some(symbol_table)) }
    }

    /// `symbolTable()`: invariante do `finishCreation` (o C++ a dereferencia sem checar).
    pub fn symbol_table(&self) -> SymbolTableRef {
        self.symbol_table.borrow().clone().expect("JSSymbolTableObject sem SymbolTable")
    }

    /// `setSymbolTable(vm, symbolTable)`.
    pub(crate) fn set_symbol_table(&self, symbol_table: SymbolTableRef) {
        debug_assert!(self.symbol_table.borrow().is_none());
        *self.symbol_table.borrow_mut() = Some(symbol_table);
    }
}

/// O que `symbolTableGet`/`symbolTablePut` pedem do objeto: `isValidScopeOffset` e `variableAt`
/// (`JSLexicalEnvironment` e `JSSegmentedVariableObject` os definem cada um com seu armazenamento).
pub trait SymbolTableObjectVariables {
    /// A base `JSSymbolTableObject` (para `symbolTable()`).
    fn symbol_table_object(&self) -> &JSSymbolTableObject;

    fn is_valid_scope_offset(&self, offset: ScopeOffset) -> bool;

    /// `variableAt(offset).get()`.
    fn variable_at(&self, offset: ScopeOffset) -> JSValue;

    /// `variableAt(offset).set(vm, object, value)`.
    fn set_variable_at(&self, offset: ScopeOffset, value: JSValue);
}

/// `symbolTableGet(object, propertyName, slot)`: `Some((valor, atributos))` é o `true` com o slot
/// preenchido (`entry.getAttributes() | PropertyAttribute::DontDelete`).
pub fn symbol_table_get<T: SymbolTableObjectVariables + ?Sized>(object: &T, key: &UniquedKey) -> Option<(JSValue, u32)> {
    let symbol_table = object.symbol_table_object().symbol_table();
    let entry = symbol_table.borrow().get(key);
    if entry.is_null() {
        return None;
    }

    let offset = entry.scope_offset();
    // Defend against the inspector asking for a var after it has been optimized out.
    if !object.is_valid_scope_offset(offset) {
        return None;
    }

    Some((object.variable_at(offset), entry.get_attributes() | DONT_DELETE))
}

/// `symbolTableGet(object, propertyName, entry, descriptor)`: a entrada rápida e o par
/// `(valor, atributos)` do `descriptor.setDescriptor(value, attributes | DontDelete)`.
pub fn symbol_table_get_descriptor<T: SymbolTableObjectVariables + ?Sized>(
    object: &T,
    key: &UniquedKey,
) -> Option<(SymbolTableEntryFast, JSValue, u32)> {
    let symbol_table = object.symbol_table_object().symbol_table();
    let entry = symbol_table.borrow().get(key);
    if entry.is_null() {
        return None;
    }

    let offset = entry.scope_offset();
    if !object.is_valid_scope_offset(offset) {
        return None;
    }

    Some((entry, object.variable_at(offset), entry.get_attributes() | DONT_DELETE))
}

/// O desfecho de `symbolTablePut` (`bool` de retorno, `putResult` e o `throwTypeError` do C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolTablePut {
    /// `return false`: a chave não está na tabela (ou o deslocamento não vale), nada foi escrito.
    NotFound,
    /// `return true` com `putResult = true`: a variável foi escrita.
    Stored,
    /// `return true` com `putResult = false`: a entrada é somente leitura. Se `should_throw`, quem
    /// chama lança `TypeError` com `ReadonlyPropertyWriteError`.
    ReadOnly { should_throw: bool },
}

/// `symbolTablePutTouchWatchpointSet` e `symbolTablePutInvalidateWatchpointSet` (as duas, ver o topo).
pub fn symbol_table_put<T: SymbolTableObjectVariables + ?Sized>(
    object: &T,
    key: &UniquedKey,
    value: JSValue,
    should_throw_read_only_error: bool,
    ignore_read_only_errors: bool,
) -> SymbolTablePut {
    let symbol_table = object.symbol_table_object().symbol_table();
    let fast_entry = symbol_table.borrow().get(key);
    if fast_entry.is_null() {
        return SymbolTablePut::NotFound;
    }
    if fast_entry.is_read_only() && !ignore_read_only_errors {
        return SymbolTablePut::ReadOnly { should_throw: should_throw_read_only_error };
    }

    let offset = fast_entry.scope_offset();

    // Defend against the inspector asking for a var after it has been optimized out.
    if !object.is_valid_scope_offset(offset) {
        return SymbolTablePut::NotFound;
    }

    object.set_variable_at(offset, value);
    SymbolTablePut::Stored
}
