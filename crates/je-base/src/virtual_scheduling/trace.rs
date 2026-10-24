//! Porte de `src/vs/base/test/common/virtualScheduling/trace.ts`: atribuição causal de trabalho agendado.
//!
//! Um [`Trace`] é um valor imutável que identifica uma cadeia causal. Todo `Trace` que não é raiz tem um
//! `parent`; a cabeça da cadeia não tem. [`Trace::child`] estende a cadeia ao agendar trabalho seguinte.
//! Serve para responder "quem causou isto?" para qualquer evento virtual: depuração, término por dono e
//! atribuição em mensagens de erro.
//!
//! Notas de tradução:
//! - O `Trace` é compartilhado por `Rc` (o original o compara por identidade), e a pilha capturada é um
//!   [`StackSource`], que formata o texto só quando alguém o pede (o `Error` do original, cujo `stack` só
//!   é lido sob demanda). Uma pilha textual é `StackSource::new(|| texto)`.
//! - `Trace.root` (o próprio `Trace` quando não há pai) é [`Trace::root`], que precisa do `Rc` do `Trace`.
//! - Cada `Trace` carrega o [`TraceLike`] equivalente, com a raiz compartilhada pela cadeia inteira, para
//!   que o `build_history_from_tasks` (ver `execution_graph`) agrupe por identidade como no original, onde
//!   o `Trace` satisfaz `TraceLike` estruturalmente ([`Trace::as_trace_like`]).
//! - `TraceContext.instance` (estático) é um por thread ([`TraceContext::instance`]), como o resto do
//!   estado de teste do loop. A raiz sentinela [`root_trace`] também é uma por thread.
//! - O `try/finally` do `runWithTrace` e do `runAsHandler` usa `errors::catch`: a limpeza roda e o erro
//!   (ou pânico) é relançado depois.
//! - `BugIndicatingError` é `Error::bug_indicating`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::errors::{self, Error};
use crate::execution_graph::{TraceLike, TraceRootLike};
use crate::lifecycle::StackSource;

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `Trace`: um elo da cadeia causal.
pub struct Trace {
    pub id: u64,
    root: Option<Rc<Trace>>,
    pub parent: Option<Rc<Trace>>,
    pub label: String,
    pub depth: usize,
    stack: Option<StackSource>,
    like: Rc<TraceLike>,
}

impl Trace {
    pub fn new(parent: Option<Rc<Trace>>, label: impl Into<String>, stack: Option<StackSource>) -> Rc<Self> {
        let label = label.into();
        let like = Rc::new(TraceLike {
            parent: parent.as_ref().map(|p| p.like.clone()),
            root: parent.as_ref().map_or_else(|| Rc::new(TraceRootLike { label: label.clone() }), |p| p.like.root.clone()),
        });
        Rc::new(Self {
            id: ID_COUNTER.fetch_add(1, Ordering::Relaxed) + 1,
            root: parent.as_ref().map(Self::root),
            depth: parent.as_ref().map_or(0, |p| p.depth + 1),
            parent,
            label,
            stack,
            like,
        })
    }

    /// A raiz da cadeia (este próprio `Trace` se não tem pai).
    pub fn root(self: &Rc<Self>) -> Rc<Self> {
        self.root.clone().unwrap_or_else(|| self.clone())
    }

    /// A pilha capturada, formatada agora (só quando pedida).
    pub fn stack(&self) -> Option<String> {
        self.stack.as_ref().map(StackSource::stack)
    }

    pub fn child(self: &Rc<Self>, label: impl Into<String>, stack: Option<StackSource>) -> Rc<Self> {
        Self::new(Some(self.clone()), label, stack)
    }

    /// `"#id label ← #id label ← … ← #id label"`, da folha à raiz.
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("#{} {}", self.id, self.label)];
        let mut current = self.parent.clone();
        while let Some(trace) = current {
            parts.push(format!("#{} {}", trace.id, trace.label));
            current = trace.parent.clone();
        }
        parts.join(" \u{2190} ")
    }

    /// O `TraceLike` desta cadeia (a mesma instância a cada chamada).
    pub fn as_trace_like(&self) -> Rc<TraceLike> {
        self.like.clone()
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

thread_local! {
    static ROOT_TRACE: Rc<Trace> = Trace::new(None, "<root>", None);
}

/// `ROOT_TRACE`: sentinela para "nenhum predecessor causal conhecido".
pub fn root_trace() -> Rc<Trace> {
    ROOT_TRACE.with(Rc::clone)
}

/// `createTraceRoot(label, stack?)`.
pub fn create_trace_root(label: impl Into<String>, stack: Option<StackSource>) -> Rc<Trace> {
    Trace::new(None, label, stack)
}

/// O frame do trace corrente. O original encadeia o anterior (`prev`) mas nunca o lê: o que importa é a
/// identidade do frame, que a guarda do reset compara.
struct Frame {
    trace: Rc<Trace>,
}

fn root_frame() -> Rc<Frame> {
    Rc::new(Frame { trace: root_trace() })
}

/// Quem recebe o `reset` adiado de [`TraceContext::run_as_handler`]: deve chamá-lo depois do fecho de
/// microtarefas que segue o retorno, isto é, na próxima macrotarefa do host.
pub type AfterMicrotaskClosure = Rc<dyn Fn(Box<dyn FnOnce()>)>;

/// `RunAsHandlerOptions`.
///
/// # Por que isto é uma opção por chamada
///
/// `runAsHandler` não pode restaurar o trace anterior de forma síncrona: as microtarefas que `fn`
/// enfileira (inclusive as continuações de `await`) precisam enxergar o trace novo. Então o reset é
/// adiado, mas tem de disparar depois do *fecho* da fila de microtarefas (a microtarefa atual mais todas
/// as que ela enfileira, recursivamente), não só uma rodada. Como o host só roda uma macrotarefa com a
/// fila vazia, qualquer primitiva de macrotarefa serve. Deixar o *chamador* escolher o escoadouro permite
/// que o `VirtualTimeProcessor` use a mesma primitiva dos saltos do embedding (sem corrida entre os
/// saltos dele e o timer do reset), que código sem processador use um `setTimeout(0)` real, e que testes
/// usem um escoadouro determinístico.
#[derive(Clone)]
pub struct RunAsHandlerOptions {
    pub after_microtask_closure: AfterMicrotaskClosure,
}

impl RunAsHandlerOptions {
    pub fn new(after_microtask_closure: impl Fn(Box<dyn FnOnce()>) + 'static) -> Self {
        Self { after_microtask_closure: Rc::new(after_microtask_closure) }
    }
}

/// Guarda o slot mutável do "frame de trace corrente". Crie instâncias novas para isolar testes, ou use
/// [`TraceContext::instance`] para o estado compartilhado.
pub struct TraceContext {
    current: RefCell<Rc<Frame>>,
    is_handler_running: Cell<bool>,
}

thread_local! {
    static INSTANCE: Rc<TraceContext> = TraceContext::new();
}

impl TraceContext {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { current: RefCell::new(root_frame()), is_handler_running: Cell::new(false) })
    }

    /// `TraceContext.instance`.
    pub fn instance() -> Rc<Self> {
        INSTANCE.with(Rc::clone)
    }

    pub fn current_trace(&self) -> Rc<Trace> {
        self.current.borrow().trace.clone()
    }

    /// Instala `t` como corrente durante a duração síncrona de `f` e restaura. Aninhável. As microtarefas
    /// que `f` enfileira e que rodam depois enxergam o trace *restaurado*: use [`Self::run_as_handler`]
    /// quando as continuações devem herdar o trace.
    pub fn run_with_trace<T>(&self, t: Rc<Trace>, f: impl FnOnce() -> T) -> T {
        let prev = self.current.borrow().clone();
        let next = Rc::new(Frame { trace: t });
        *self.current.borrow_mut() = next.clone();
        let result = errors::catch(f);
        let same = Rc::ptr_eq(&self.current.borrow(), &next);
        if !same {
            errors::throw(Error::bug_indicating(Some("runWithTrace: unexpected mutation of current frame.")));
        }
        *self.current.borrow_mut() = prev;
        match result {
            Ok(value) => value,
            Err(error) => errors::throw(error),
        }
    }

    /// Instala `t` como corrente e roda `f`. O trace continua corrente durante o fecho de microtarefas
    /// que segue `f`, de modo que as continuações de `await` dentro de `f` enxergam `t`. O reset é
    /// despachado por `opts.after_microtask_closure`.
    ///
    /// Lança em reentrada síncrona: callbacks de timer nunca se aninham no mesmo quadro da pilha, então
    /// isso só dispara por mau uso.
    pub fn run_as_handler<T>(self: &Rc<Self>, t: Rc<Trace>, f: impl FnOnce() -> T, opts: &RunAsHandlerOptions) -> T {
        if self.is_handler_running.get() {
            errors::throw(Error::new(format!(
                "runAsHandler: re-entrant invocation. current={}, incoming={}",
                self.current_trace().describe(),
                t.describe()
            )));
        }
        let prev = self.current.borrow().clone();
        let next = Rc::new(Frame { trace: t });
        *self.current.borrow_mut() = next.clone();
        self.is_handler_running.set(true);
        let result = errors::catch(f);
        self.is_handler_running.set(false);
        let context = self.clone();
        (opts.after_microtask_closure)(Box::new(move || {
            // Guarda de identidade: outro handler pode ter rodado entre o enfileiramento do reset e o
            // disparo dele. Cada `run_as_handler` cria um frame novo, então a igualdade de referência
            // detecta o reset velho.
            let same = Rc::ptr_eq(&context.current.borrow(), &next);
            if same {
                *context.current.borrow_mut() = prev;
            }
        }));
        match result {
            Ok(value) => value,
            Err(error) => errors::throw(error),
        }
    }

    /// `_resetForTesting()`.
    pub fn reset_for_testing(&self) {
        *self.current.borrow_mut() = root_frame();
        self.is_handler_running.set(false);
    }
}
