//! Porte de `runtime/Exception.h` e `runtime/Exception.cpp`: a célula `Exception`, registrada no
//! `cell_registry` como `CellEntry::Exception`, de modo que o `op_catch` grave o exception num
//! registrador como `JSValue::from_cell(exception.cell_id())`.
//!
//! DIVERGÊNCIAS:
//!
//! - `m_stack` (`Vector<StackFrame>`) é capturado por `Interpreter::capture_stack_for_exception`
//!   (`interpreter/unwind.rs`) quando o laço vê a exceção pela primeira vez, no frame mais interno,
//!   e não dentro de `create` (o `VM` não enxerga a `CLoopStack`); `create` não recebe o
//!   `StackCaptureAction`.
//! - `tryUnwrapValueForJSTag` e `wrapValueForJSTag` (`ENABLE(WEBASSEMBLY)`) ficam de fora: não há
//!   `JSWebAssemblyException`.
//! - `createStructure` (`TypeInfo(CellType, StructureFlags)`) e o `vm.exceptionStructure` não existem:
//!   o `JSType` vem de `CellEntry::js_type` (`CellType`), o papel do cabeçalho do `JSCell`.
//! - `visitChildren` e `estimatedSize` entram com o `Heap`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::stack_frame::StackFrame;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::vm::VM;

/// `const ClassInfo Exception::s_info`.
pub static EXCEPTION_S_INFO: ClassInfo =
    ClassInfo { class_name: "Exception", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `class Exception final : public JSCell`.
#[derive(Debug)]
pub struct Exception {
    /// O `cell_id` no `cell_registry` (o endereço da célula no C++).
    cell_id: usize,
    /// `WriteBarrier<Unknown> m_value`.
    value: Cell<JSValue>,
    /// `m_didNotifyInspectorOfThrow`.
    did_notify_inspector_of_throw: Cell<bool>,
    /// `Vector<StackFrame> m_stack`, preenchido por `Interpreter::capture_stack_for_exception`.
    stack: RefCell<Vec<StackFrame>>,
    /// Se `m_stack` já foi capturado (o `StackCaptureAction::CaptureStack` do `create` acontece uma
    /// vez, no primeiro `throw`; relançar a mesma `Exception` não recaptura).
    stack_captured: Cell<bool>,
}

/// `Exception*`: a identidade (`Rc`) importa para `m_exception == m_terminationException`.
pub type ExceptionRef = Rc<Exception>;

impl Exception {
    /// `visitChildren`: o valor lançado.
    pub(crate) fn visit_children(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.value(self.value.get());
    }
    /// `Exception::create(VM&, JSValue thrownValue, StackCaptureAction)`.
    pub fn create(_vm: &VM, thrown_value: JSValue) -> ExceptionRef {
        let cell_id = cell_registry::reserve();
        let exception = Rc::new(Exception {
            cell_id,
            value: Cell::new(thrown_value),
            did_notify_inspector_of_throw: Cell::new(false),
            stack: RefCell::new(Vec::new()),
            stack_captured: Cell::new(false),
        });
        cell_registry::set(cell_id, CellEntry::Exception(Rc::clone(&exception)));
        exception
    }

    /// `Exception::info()`.
    pub fn info(&self) -> &'static ClassInfo {
        &EXCEPTION_S_INFO
    }

    /// O `cell_id` da célula, o valor que `JSValue::from_cell` recebe.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// A célula do id, se ele é de um `Exception` (`jsDynamicCast<Exception*>`).
    pub fn from_cell_id(cell_id: usize) -> Option<ExceptionRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::Exception(exception)) => Some(exception),
            _ => None,
        }
    }

    /// `value()`.
    pub fn value(&self) -> JSValue {
        self.value.get()
    }

    /// `didNotifyInspectorOfThrow()`.
    pub fn did_notify_inspector_of_throw(&self) -> bool {
        self.did_notify_inspector_of_throw.get()
    }

    /// `setDidNotifyInspectorOfThrow()`.
    pub fn set_did_notify_inspector_of_throw(&self) {
        self.did_notify_inspector_of_throw.set(true);
    }

    /// `stack()`: `m_stack`.
    pub fn stack(&self) -> Vec<StackFrame> {
        self.stack.borrow().clone()
    }

    /// Se o `m_stack` já foi capturado.
    pub fn stack_captured(&self) -> bool {
        self.stack_captured.get()
    }

    /// Grava o `m_stack` capturado por `Interpreter::getStackTrace`. `Exception::finishCreation` reporta
    /// `m_stack.sizeInBytes()` como memória extra: cada captura soma ao relógio de bytes do coletor, e é isso que faz
    /// um desenrolar que relança por muitos frames (o `catch (error) { throw error }` de `generatorResume`, a cada
    /// nível de uma recursão de geradores) alcançar o limite do ciclo.
    pub fn set_stack(&self, frames: Vec<StackFrame>) {
        crate::runtime::heap_size::report_extra_memory(frames.len() * crate::runtime::heap_size::STACK_FRAME_BYTES);
        *self.stack.borrow_mut() = frames;
        self.stack_captured.set(true);
    }

    /// `m_stack.size()`, sem copiar os frames.
    pub(crate) fn stack_len(&self) -> usize {
        self.stack.try_borrow().map_or(0, |stack| stack.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_type::JSType;

    #[test]
    fn exception_is_a_registered_cell() {
        let vm = VM::new();
        let exception = Exception::create(&vm, crate::runtime::js_value::js_number_i32(7));
        assert_eq!(cell_registry::cell_type(exception.cell_id()), Some(JSType::CellType));
        let found = Exception::from_cell_id(exception.cell_id()).expect("célula registrada");
        assert!(Rc::ptr_eq(&found, &exception));
        assert_eq!(found.value().as_int32(), 7);
        assert!(!found.did_notify_inspector_of_throw());
        found.set_did_notify_inspector_of_throw();
        assert!(exception.did_notify_inspector_of_throw());
    }
}
