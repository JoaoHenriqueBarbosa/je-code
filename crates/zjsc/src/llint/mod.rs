//! Porte de `JavaScriptCore/llint`: o interpretador de bytecode.
//!
//! O C++ gera o interpretador (`LowLevelInterpreter*.asm`, traduzido pelo `offlineasm` para
//! assembly nativo, ou para o C++ do CLoop). Rust seguro não executa código gerado, então o laço
//! de despacho é um `match` sobre o `OpcodeID` (`CONVENTIONS.md`, item 4): cada braço traduz o
//! handler do `.asm` (caminho rápido) e chama o equivalente do `LLIntSlowPaths.cpp` ou do
//! `CommonSlowPaths.cpp` onde o handler cairia no caminho lento.
//!
//! - `llint_data`: constantes do `.asm` e classificação dos pontos de entrada.
//! - `llint_entrypoint`, `llint_jit_code`: `LLIntEntrypoint.cpp` e o `JITCode` do LLInt.
//! - `dispatch`: o laço, os handlers e o prólogo/arity check de função.
//! - `slow_paths`: os slow paths de `LLIntSlowPaths.cpp` e de `CommonSlowPaths.cpp` que o laço usa.
//! - `slow_paths_arith`: os slow paths aritméticos, bit a bit e de comparação.
//!
//! Chamada JS para JS é recursão do próprio laço: o `call` do `.asm` (`cloopCallJSFunction` no
//! CLoop) vira uma chamada de `Interpreter::llint_execute` sobre o frame que o chamador montou na
//! `CLoopStack`. O frame vive na pilha de registradores exatamente como no C++ (cabeçalho em
//! `CallFrameSlot`, locais em índices negativos, frame do callee dentro da extensão do chamador).

pub mod dispatch;
pub mod dispatch_ext;
pub mod handlers_accessor;
pub mod handlers_arguments;
pub mod handlers_array;
pub mod handlers_async;
pub mod handlers_enumerator;
pub mod handlers_iterator;
pub mod handlers_misc;
pub mod handlers_object;
pub mod handlers_private_brand;
pub mod handlers_scope;
pub mod llint_data;
pub mod llint_entrypoint;
pub mod llint_jit_code;
pub mod slow_paths;
pub mod slow_paths_arith;
pub mod slow_paths_object;
pub mod slow_paths_control;
pub mod slow_paths_generator;
pub mod slow_paths_jump;
pub mod varargs;

use crate::bytecode::code_block::CodeBlockRef;
use crate::bytecode::opcode::OpcodeID;
use crate::interpreter::call_frame::CallFrame;
use crate::runtime::js_global_object::JSGlobalObjectRef;

/// Por que a execução de um frame terminou sem valor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LLIntFailure {
    /// Exceção JS pendente no `VM`: o C++ devolve `JSValue()` (vazio) e quem chamou consulta
    /// `vm.exception()`.
    Thrown,
    /// O opcode ainda não tem handler neste porte. Não existe no C++ (todo opcode tem handler):
    /// é a lacuna deste porte, devolvida em vez de fabricar um comportamento.
    UnportedOpcode(OpcodeID),
    /// Caminho de um handler que depende de tipo ainda ausente (descrição na mensagem).
    Unported(&'static str),
}

pub type LLIntResult<T> = Result<T, LLIntFailure>;

/// O estado que o `.asm` guarda nos registradores `cfr` e `PB` e que os slow paths recebem como
/// `callFrame` e `codeBlock`/`globalObject` (`LLINT_BEGIN`).
#[derive(Clone)]
pub struct LLIntFrame {
    /// `cfr`.
    pub call_frame: CallFrame,
    /// `callFrame->codeBlock()`.
    pub code_block: CodeBlockRef,
    /// `codeBlock->globalObject()`.
    pub global_object: JSGlobalObjectRef,
}
