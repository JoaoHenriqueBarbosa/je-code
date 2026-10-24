//! Porte de `runtime/ScopedArguments.{h,cpp}`: o `arguments` de uma função em que algum parâmetro foi
//! capturado por um fechamento. O objeto guarda os argumentos que sobram (além dos parâmetros declarados)
//! e lê/escreve os parâmetros declarados direto no `JSLexicalEnvironment` da função, pela
//! `ScopedArgumentsTable` do `SymbolTable`.
//!
//! DIVERGÊNCIAS:
//!
//! - `m_table` é uma cópia da tabela (`Vec<ScopeOffset>`) por objeto: o C++ compartilha a tabela do
//!   `SymbolTable` e a clona na primeira escrita (`trySet`), o que dá a mesma visão. Sem `WatchpointSet`:
//!   `clearWatchpointSet` e o `touch` de `setIndexQuickly` não existem.
//! - `length`, `callee` e `@@iterator` materializam sob demanda (`overrideThings`, ver
//!   `js_arguments_objects.rs`). O `length` gravado é `m_table->length()`, como no C++: um objeto com mais
//!   argumentos que parâmetros passa a mostrar o número de parâmetros ali.
//! - `m_storage` é um `Vec<Option<JSValue>>` (`None` é o `clear()` do `WriteBarrier`).
//! - Sem `fastSlice` e `isIteratorProtocolFastAndNonObservable` (otimizações de quem fatia o objeto).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::generic_arguments::{ArgumentsState, MappedArguments};
use crate::runtime::js_arguments_objects::{arguments_structure, put_arguments_specials, CalleeProperty};
use crate::runtime::js_function::JSFunctionRef;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_lexical_environment::JSLexicalEnvironmentRef;
use crate::runtime::js_object::{JSNonFinalObject, JSObject, PutError, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_symbol_table_object::SymbolTableObjectVariables;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::scope_offset::ScopeOffset;

/// `const ClassInfo ScopedArguments::s_info`.
pub static SCOPED_ARGUMENTS_S_INFO: ClassInfo =
    ClassInfo { class_name: "Arguments", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `class ScopedArguments final : public GenericArgumentsImpl<ScopedArguments>`.
pub struct ScopedArguments {
    base: JSNonFinalObject,
    /// `m_totalLength`: os parâmetros declarados mais os que sobram.
    total_length: u32,
    /// `m_table`: o `ScopeOffset` de cada parâmetro declarado; o inválido é o desmapeado.
    table: RefCell<Vec<ScopeOffset>>,
    /// `m_scope`.
    scope: JSLexicalEnvironmentRef,
    /// `m_storage`: os argumentos além da tabela (`total_length - table.len()`).
    storage: RefCell<Vec<Option<JSValue>>>,
    /// `m_hasUnmappedArgument`.
    has_unmapped_argument: Cell<bool>,
    /// `m_callee`.
    callee: JSValue,
    /// `m_overrodeThings`.
    overrode_things: Cell<bool>,
    state: ArgumentsState,
}

/// O `ScopedArguments*`.
pub type ScopedArgumentsRef = Rc<ScopedArguments>;

impl std::fmt::Debug for ScopedArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedArguments").field("cell_id", &self.base.cell_id()).field("total_length", &self.total_length).finish()
    }
}

impl std::ops::Deref for ScopedArguments {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl ScopedArguments {
    /// `visitChildren`: os argumentos fora do escopo e o callee (o escopo é `Rc`, já raiz).
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.callee);
        match self.storage.try_borrow() {
            Ok(storage) => storage.iter().for_each(|value| marker.value(*value)),
            Err(_) => marker.abort(),
        }
    }
    /// `ScopedArguments::createByCopying(globalObject, callFrame, table, scope)`: `arguments` são os
    /// `argumentCount()` argumentos do frame, `callee` o `jsCallee()` e a tabela vem do
    /// `scope->symbolTable()->arguments()`.
    pub fn create_by_copying(
        global_object: &JSGlobalObject,
        arguments: &[JSValue],
        callee: &JSFunctionRef,
        scope: JSLexicalEnvironmentRef,
    ) -> Result<ScopedArgumentsRef, PutError> {
        let vm = global_object.vm();
        let structure = arguments_structure(global_object, JSType::ScopedArgumentsType, &SCOPED_ARGUMENTS_S_INFO);
        let symbol_table = scope.symbol_table();
        let table: Vec<ScopeOffset> = {
            let symbol_table = symbol_table.borrow();
            let offsets = (0..symbol_table.arguments_length()).map(|index| symbol_table.argument_offset(index)).collect();
            offsets
        };
        let total_length = arguments.len() as u32;
        let storage = arguments.iter().skip(table.len()).map(|argument| Some(*argument)).collect();

        let cell_id = cell_registry::reserve();
        let result = Rc::new(ScopedArguments {
            base: JSNonFinalObject::new(vm, structure),
            total_length,
            table: RefCell::new(table),
            scope,
            storage: RefCell::new(storage),
            has_unmapped_argument: Cell::new(false),
            callee: callee.as_value(),
            overrode_things: Cell::new(false),
            state: ArgumentsState::default(),
        });
        result.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::ScopedArguments(Rc::clone(&result)));

        Ok(result)
    }

    /// A célula como `JSValue`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `m_hasUnmappedArgument`.
    pub fn has_unmapped_argument(&self) -> bool {
        self.has_unmapped_argument.get()
    }
}

impl MappedArguments for ScopedArguments {
    fn object(&self) -> &JSObject {
        self
    }

    fn state(&self) -> &ArgumentsState {
        &self.state
    }

    fn internal_length(&self) -> u32 {
        self.total_length
    }

    fn modified_length(&self) -> u32 {
        self.table.borrow().len() as u32
    }

    fn is_mapped_argument(&self, index: u32) -> bool {
        if index >= self.total_length {
            return false;
        }
        let table = self.table.borrow();
        let named_length = table.len();
        if (index as usize) < named_length {
            return table[index as usize].is_valid();
        }
        self.storage.borrow()[index as usize - named_length].is_some()
    }

    fn get_index_quickly(&self, index: u32) -> JSValue {
        debug_assert!(self.is_mapped_argument(index));
        let table = self.table.borrow();
        let named_length = table.len();
        if (index as usize) < named_length {
            return self.scope.variable_at(table[index as usize]);
        }
        self.storage.borrow()[index as usize - named_length].expect("argumento mapeado sem valor")
    }

    fn set_index_quickly(&self, index: u32, value: JSValue) {
        debug_assert!(self.is_mapped_argument(index));
        let table = self.table.borrow();
        let named_length = table.len();
        if (index as usize) < named_length {
            self.scope.set_variable_at(table[index as usize], value);
            return;
        }
        self.storage.borrow_mut()[index as usize - named_length] = Some(value);
    }

    fn unmap_argument(&self, index: u32) -> Result<(), PutError> {
        debug_assert!(index < self.total_length);
        self.has_unmapped_argument.set(true);
        let mut table = self.table.borrow_mut();
        let named_length = table.len();
        if (index as usize) < named_length {
            table[index as usize] = ScopeOffset::default();
        } else {
            self.storage.borrow_mut()[index as usize - named_length] = None;
        }
        Ok(())
    }

    fn callee(&self) -> JSValue {
        self.callee
    }

    fn overrode_things(&self) -> bool {
        self.overrode_things.get()
    }

    /// `overrideThings`: `length` recebe `m_table->length()` (os parâmetros declarados), como no C++.
    fn override_things(&self, global_object: &JSGlobalObject) -> Result<(), PutError> {
        let table_length = self.table.borrow().len() as u32;
        put_arguments_specials(global_object, self, table_length, CalleeProperty::Value { value: self.callee, attributes: DONT_ENUM })?;
        self.overrode_things.set(true);
        Ok(())
    }
}
