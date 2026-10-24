//! Tradução parcial de `debugger/Debugger.h` e `Debugger.cpp`: o que `JSGlobalObject` e os
//! executables consultam (`isInteractivelyDebugging`, `sourceParsed`).
//!
//! DIVERGÊNCIAS: o `Debugger::Observer` e o `dispatchFunctionToObservers` ainda não foram portados,
//! então não há observador registrado e `canDispatchFunctionToObservers()` é sempre falso:
//! `sourceParsed` faz o `return` antecipado do C++ (`if (!canDispatchFunctionToObservers()) return;`),
//! sem tocar em `m_reportedSourceIDs`. Os ramos que criam `Debugger::Script` e chamam
//! `didParseSource`/`failedToParseSource` entram junto com o `Observer`.

use std::cell::Cell;
use std::rc::Rc;

use crate::parser::source_provider::SourceProvider;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class Debugger`.
#[derive(Debug, Default)]
pub struct Debugger {
    /// `m_breakpointsActivated`.
    breakpoints_activated: Cell<bool>,
}

impl Debugger {
    /// `isInteractivelyDebugging() const`.
    pub fn is_interactively_debugging(&self) -> bool {
        self.breakpoints_activated.get()
    }

    /// `setBreakpointsActivated(bool)`, a parte que grava `m_breakpointsActivated`.
    pub fn set_breakpoints_activated(&self, activated: bool) {
        self.breakpoints_activated.set(activated);
    }

    /// `registerCodeBlock(CodeBlock*)`.
    ///
    /// DIVERGÊNCIA: o C++ só chama `toggleBreakpoint` para cada breakpoint já posto no fonte do
    /// bloco, e recalcula o `DebuggerParseData`/`m_breakpointID` do `Script`; nada disso existe até
    /// o `Observer` e os breakpoints serem portados (`m_breakpoints` está vazio), então não há
    /// efeito a aplicar. `set_breakpoints_activated`/`is_interactively_debugging` já valem.
    pub fn register_code_block(&self, _code_block: &crate::bytecode::code_block::CodeBlockRef) {}

    /// `canDispatchFunctionToObservers()`: sem `Observer` portado, nunca há quem receba.
    fn can_dispatch_function_to_observers(&self) -> bool {
        false
    }

    /// `sourceParsed(JSGlobalObject*, SourceProvider*, int errorLine, const String& errorMessage)`.
    pub fn source_parsed(
        &self,
        _global_object: &JSGlobalObject,
        _source_provider: &Rc<dyn SourceProvider>,
        _error_line: i32,
        _error_message: &WtfString,
    ) {
        // Preemptively check whether we can dispatch so that we don't do any unnecessary allocations.
        if !self.can_dispatch_function_to_observers() {
            return;
        }
    }
}
