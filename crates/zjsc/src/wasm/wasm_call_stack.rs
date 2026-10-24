//! A pilha de chamadas Wasm que o percurso de `Error.stack` enxerga.
//!
//! No JSC cada função Wasm é um frame real na pilha de máquina (`NativeCallee`), que o `StackVisitor` percorre
//! entre o frame JS que chamou o export e o que o export chamou. Aqui `Instance::run_frames` guarda os quadros
//! num `Vec<Frame>` local, invisível para `Interpreter::get_stack_trace`. Esta pilha espelha esse `Vec`: uma
//! entrada por quadro Wasm vivo, ancorada no frame nativo da função exportada (`WebAssemblyFunction`) que a
//! chamada JS usou. O percurso troca esse frame nativo, que o bun não mostra, pelas entradas ancoradas nele
//! (`at unknown`, uma por função Wasm, a mais interna primeiro).
//!
//! DIVERGÊNCIA: o lugar fiel é o `VM` (o `topCallFrame` e o `EntryFrame` moram lá). `Instance` não tem
//! referência ao `VM`, então a pilha é por thread, como o resto do estado de execução do porte.

use std::cell::{Cell, RefCell};

thread_local! {
    /// Uma entrada por quadro Wasm vivo, do mais externo ao mais interno; o valor é o `registers()` do
    /// `CallFrame` nativo da função exportada que iniciou a execução (`None` sem chamada vinda de JS).
    static ENTRIES: RefCell<Vec<Option<usize>>> = const { RefCell::new(Vec::new()) };
    /// A âncora das entradas empilhadas agora: o frame nativo do export em execução.
    static ANCHOR: Cell<Option<usize>> = const { Cell::new(None) };
    /// A pilha no instante do trap (`unreachable`, acesso fora dos limites...), antes de `run_frames` desempilhar.
    /// O `RuntimeError` só nasce depois, em `wasm_error_to_thrown`, com `ENTRIES` já vazia; no C++ o erro nasce
    /// com os frames wasm ainda na pilha de máquina. Enquanto houver captura, é ela que o percurso enxerga.
    static TRAP_STACK: RefCell<Option<Vec<Option<usize>>>> = const { RefCell::new(None) };
}

/// Fotografa a pilha viva no trap. A primeira captura vale (a mais interna): o erro que sobe pelos quadros de
/// baixo, ou por outra instância, é o mesmo e não substitui a fotografia.
pub fn capture_trap_stack() {
    TRAP_STACK.with(|trap| {
        let mut trap = trap.borrow_mut();
        if trap.is_none() {
            *trap = Some(ENTRIES.with(|entries| entries.borrow().clone()));
        }
    });
}

/// Guarda de [`hold_trap_stack`]: descarta a fotografia do trap ao sair.
pub struct TrapStackScope;

impl Drop for TrapStackScope {
    fn drop(&mut self) {
        TRAP_STACK.with(|trap| trap.borrow_mut().take());
    }
}

/// Mantém a fotografia do trap visível ao percurso de `Error.stack` enquanto a guarda viver (a criação do
/// `RuntimeError`) e a descarta depois.
pub fn hold_trap_stack() -> TrapStackScope {
    TrapStackScope
}

/// Guarda de [`enter_from_js`]: restaura a âncora anterior ao sair.
pub struct AnchorScope {
    previous: Option<usize>,
}

impl Drop for AnchorScope {
    fn drop(&mut self) {
        ANCHOR.with(|anchor| anchor.set(self.previous));
    }
}

/// Declara `native_frame` (o `CallFrame::registers()` do frame nativo da função exportada) como a âncora dos
/// quadros Wasm empilhados enquanto a guarda viver.
pub fn enter_from_js(native_frame: Option<usize>) -> AnchorScope {
    AnchorScope { previous: ANCHOR.with(|anchor| anchor.replace(native_frame)) }
}

/// Guarda de [`enter_frames`]: tira da pilha tudo o que `run_frames` empilhou, qualquer que seja a saída.
pub struct FramesScope {
    base: usize,
}

impl Drop for FramesScope {
    fn drop(&mut self) {
        ENTRIES.with(|entries| entries.borrow_mut().truncate(self.base));
    }
}

/// Empilha `count` quadros (os que `run_frames` recebe) com a âncora corrente.
pub fn enter_frames(count: usize) -> FramesScope {
    let anchor = ANCHOR.with(Cell::get);
    ENTRIES.with(|entries| {
        let mut entries = entries.borrow_mut();
        let base = entries.len();
        if base == 0 {
            // Uma execução nova na pilha vazia: fotografia de trap que ninguém consumiu não vale mais.
            TRAP_STACK.with(|trap| trap.borrow_mut().take());
        }
        entries.extend(std::iter::repeat_n(anchor, count));
        FramesScope { base }
    })
}

/// Um quadro Wasm entrou (`frames.push`).
pub fn push_frame() {
    let anchor = ANCHOR.with(Cell::get);
    ENTRIES.with(|entries| entries.borrow_mut().push(anchor));
}

/// Um quadro Wasm saiu (`frames.pop`).
pub fn pop_frame() {
    ENTRIES.with(|entries| {
        entries.borrow_mut().pop();
    });
}

/// Quantos quadros Wasm vivos estão ancorados no frame nativo `native_frame`.
pub fn frames_anchored_at(native_frame: usize) -> usize {
    let count = |entries: &[Option<usize>]| entries.iter().filter(|anchor| **anchor == Some(native_frame)).count();
    TRAP_STACK.with(|trap| match trap.borrow().as_deref() {
        Some(snapshot) => count(snapshot),
        None => ENTRIES.with(|entries| count(&entries.borrow())),
    })
}
