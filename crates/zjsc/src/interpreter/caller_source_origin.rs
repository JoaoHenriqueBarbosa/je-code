//! Tradução de `CallFrame::callerSourceOrigin(vm)` (`interpreter/CallFrame.cpp`): a `SourceOrigin` do
//! código que chamou a função nativa em `frame`.
//!
//! DIVERGÊNCIAS: o `CallFrame::callerSourceOrigin` é um método do `CallFrame`, mas o percurso precisa do
//! [`Interpreter`] (a pilha e a tabela de `CodeBlock`), que o `CallFrame` do porte não carrega; por isso é
//! uma função livre sobre o interpretador. O `case Wasm` não existe (ver `stack_visitor.rs`).

use crate::interpreter::call_frame::CallFrame;
use crate::interpreter::interpreter::Interpreter;
use crate::interpreter::stack_visitor::{FrameCodeType, IterationStatus, StackVisitor};
use crate::runtime::script_executable::ScriptExecutableRef;
use crate::runtime::source_origin::SourceOrigin;

/// `callFrame->callerSourceOrigin(vm)`: a origem do primeiro frame, acima de `frame`, que tem código
/// (função que não é builtin privado, eval, módulo ou global). Frames nativos são ignorados, e sem
/// nenhum o resultado é a origem nula.
pub fn caller_source_origin(interpreter: &Interpreter, frame: CallFrame) -> SourceOrigin {
    let mut source_origin = SourceOrigin::default();
    StackVisitor::visit(interpreter, Some(frame), true, |visited| {
        let Some(code_block) = visited.code_block() else {
            return IterationStatus::Continue;
        };
        let code_block = code_block.borrow();
        let owner = code_block.owner_executable();
        if visited.code_type() == FrameCodeType::Function {
            // Skip the builtin functions since they should not pass the source origin to the dynamic code
            // generation calls (`[ "42 + 44" ].forEach(eval)`).
            if let ScriptExecutableRef::Function(function) = owner {
                if function.borrow().is_private_builtin_function() {
                    return IterationStatus::Continue;
                }
            }
        }
        // `ownerExecutable()->sourceOrigin()`: a do `SourceProvider` do código.
        if let Some(provider) = owner.source().provider() {
            source_origin = provider.source_origin().clone();
        }
        IterationStatus::Done
    });
    source_origin
}

#[cfg(test)]
mod tests {
    use crate::api::eval::evaluate_named_script_result;
    use crate::wtf::text::conversion_mode::ConversionMode;

    fn run(source: &str) -> String {
        let value = evaluate_named_script_result(source, "origin_case.js", "R").expect("sem exceção");
        String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
    }

    #[test]
    fn indirect_eval_inherits_caller_origin() {
        let stack = run("globalThis.R = (0, eval)(\"new Error('e').stack\")");
        assert!(stack.contains("file:///origin_case.js"), "{stack}");
    }

    #[test]
    fn function_constructor_inherits_caller_origin() {
        let stack = run("globalThis.R = new Function(\"return new Error('e').stack\")()");
        assert!(stack.contains("at anonymous (file:///origin_case.js"), "{stack}");
    }

    #[test]
    fn eval_through_builtin_skips_native_frames() {
        let stack = run("globalThis.R = ['new Error(\"e\").stack'].map(eval)[0]");
        assert!(stack.contains("file:///origin_case.js"), "{stack}");
    }
}
