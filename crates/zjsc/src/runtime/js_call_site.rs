//! `JSCallSite`: o objeto `CallSite` que `Error.prepareStackTrace(error, callSites)` recebe, e a chamada
//! desse gancho (`Error.prepareStackTrace`).
//!
//! O `JSCallSite` e o `CallSitePrototype` não existem em `upstream/JavaScriptCore`: são extensão do `Bun`
//! (`CallSite.cpp`, `CallSitePrototype.cpp`, `ErrorStackTrace.cpp` e o `onComputeErrorInfoJSValue` do
//! `ZigGlobalObject`), cujo fonte não está na árvore. O formato e a semântica seguem o que o `Bun` e o V8
//! documentam, sem conferência contra o fonte.
//!
//! DIVERGÊNCIAS:
//! - Como no `Bun` (medido na 1.4.2), o gancho roda na primeira leitura de `stack`, uma vez por erro
//!   (`ErrorInstance::materialize_stack`); em `Error.captureStackTrace` roda na chamada.
//! - O `JSCallSite` guarda o [`StackFrame`] já resolvido (ver `stack_frame.rs`), não o `CodeBlock` e o
//!   `callee` vivos.
//! - A guarda de reentrada (`PREPARING`) é do V8: um erro criado dentro do gancho usa o texto padrão.
//! - Sem GC, a célula vive até o fim da thread (como todas no `cell_registry`).
//! - O `TypeInfo` é `ObjectType` (o C++ não tem um `JSType` próprio para `CallSite`).

use std::cell::Cell;
use std::rc::Rc;

use crate::llint::{LLIntFailure, LLIntResult};
use crate::runtime::call_data::call_with_error_message;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_name::PropertyName;
use crate::runtime::stack_frame::StackFrame;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSCallSite::s_info`.
pub static JS_CALL_SITE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CallSite", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

const PREPARE_STACK_TRACE_NOT_CALLABLE: &str = "Error.prepareStackTrace is not a function";

thread_local! {
    /// `true` enquanto o gancho `Error.prepareStackTrace` roda (ver as DIVERGÊNCIAS).
    static PREPARING: Cell<bool> = const { Cell::new(false) };
}

/// Fim do programa (`cell_registry::reset_program_state`): um pânico dentro do gancho deixaria a marca ligada.
pub(crate) fn reset_for_program() {
    let _ = PREPARING.try_with(|preparing| preparing.set(false));
}

/// `class JSCallSite final : public JSNonFinalObject`.
pub struct JSCallSite {
    base: JSNonFinalObject,
    frame: StackFrame,
}

/// A referência à célula, o `*` do C++.
pub type JSCallSiteRef = Rc<JSCallSite>;

impl std::ops::Deref for JSCallSite {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSCallSite {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_CALL_SITE_S_INFO,
        )
    }

    /// `create(vm, structure, frame)`.
    pub fn create(vm: &VM, structure: &StructureRef, frame: StackFrame) -> JSCallSiteRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSCallSite { base: JSNonFinalObject::new(vm, Rc::clone(structure)), frame });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::CallSite(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<JSCallSite>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSCallSiteRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::CallSite(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// O frame que o `CallSite` descreve.
    pub fn frame(&self) -> &StackFrame {
        &self.frame
    }
}

/// `Error.prepareStackTrace`, se for chamável. `Err` é a exceção que o getter da propriedade lançou.
pub fn prepare_stack_trace_hook(global_object: &JSGlobalObject) -> LLIntResult<Option<JSValue>> {
    let vm = global_object.vm();
    let Some(constructor) = *global_object.error_constructor.borrow() else {
        return Ok(None);
    };
    let Some(object) = ObjectRef::from_value(&constructor) else {
        return Ok(None);
    };
    let name = PropertyName::from_identifier(&Identifier::from_span(vm, b"prepareStackTrace".as_slice()));
    let hook = object.get(global_object, &name);
    if vm.exception().is_some() {
        return Err(LLIntFailure::Thrown);
    }
    // O `Error.prepareStackTrace` padrão do bun (função nativa) não reformata nada: vale como gancho ausente.
    if let Some(function) = hook.as_js_function() {
        if function.is_host_function()
            && function.as_bound_function().is_none()
            && function.original_name(global_object).is_some_and(|n| crate::interpreter::stack_visitor::to_rust_string(&n.value()) == crate::runtime::error_natives::DEFAULT_PREPARE_STACK_TRACE_NAME)
        {
            return Ok(None);
        }
    }
    Ok(hook.is_callable().then_some(hook))
}

/// `Error.prepareStackTrace(error, callSites)` com `this` igual ao construtor `Error`. `Ok(None)` quando o
/// gancho não está definido (ou já está rodando); `Err` é a exceção que ele lançou, pendente no `VM`.
pub fn prepare_stack_trace(global_object: &JSGlobalObject, error: JSValue, frames: &[StackFrame]) -> LLIntResult<Option<JSValue>> {
    let Some(hook) = prepare_stack_trace_hook(global_object)? else {
        return Ok(None);
    };
    if PREPARING.replace(true) {
        return Ok(None);
    }
    let result = call_hook(global_object, hook, error, frames);
    PREPARING.set(false);
    // Medido no `bun` 1.4.2: um gancho que devolve `null` deixa `stack` como `undefined` (a propriedade existe).
    result.map(|prepared| Some(if prepared.is_null() { JSValue::undefined() } else { prepared }))
}

/// A chamada do gancho: o array de `CallSite` (um por frame) e o `Error` como `this`.
fn call_hook(global_object: &JSGlobalObject, hook: JSValue, error: JSValue, frames: &[StackFrame]) -> LLIntResult<JSValue> {
    let vm = global_object.vm();
    let structure = global_object
        .call_site_structure
        .borrow()
        .clone()
        .expect("init_error_classes cria a estrutura do CallSite antes de qualquer erro");
    // Medido no `bun` 1.4.2 (o `encounteredStrictFrame` do CallSite do Bun): do frame mais recente para o mais
    // antigo, depois do primeiro frame estrito todos são estritos (`getFunction` e `getThis` escondidos). A função
    // nativa (builtin estrito do JSC incluso) não conta como estrita por si.
    let mut encountered_strict = false;
    let call_sites: Vec<JSValue> = frames
        .iter()
        .map(|frame| {
            encountered_strict |= frame.is_strict && !frame.is_native_function();
            let mut frame = frame.clone();
            frame.is_strict = encountered_strict;
            JSCallSite::create(vm, &structure, frame).as_value()
        })
        .collect();
    let array = construct_array(vm, &global_object.array_structure(), &call_sites).as_value();
    let this_value = (*global_object.error_constructor.borrow()).unwrap_or_else(JSValue::undefined);
    call_with_error_message(global_object, hook, this_value, &[error, array], PREPARE_STACK_TRACE_NOT_CALLABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_site_cell_roundtrip() {
        let vm = VM::new();
        let structure = JSCallSite::create_structure(&vm, None, JSValue::Null);
        let frame = StackFrame::resolved(crate::runtime::stack_frame::Location {
            function_name: "f".to_string(),
            source_url: "a.js".to_string(),
            line: 1,
            column: 2,
            has_line_and_column_info: true,
            construct_back_offset: 0,
        });
        let site = JSCallSite::create(&vm, &structure, frame.clone());
        let found = JSCallSite::from_value(&site.as_value()).expect("célula registrada");
        assert_eq!(found.frame(), &frame);
        assert!(JSCallSite::from_value(&JSValue::Undefined).is_none());
    }
}
