//! Ajudantes dos testes portados: o `utils.ts` do VS Code (`ensureNoDisposablesAreLeakedInTestSuite`,
//! `throwIfDisposablesAreLeaked`), copiados de `crates/je-base/tests/common` (o `utils.ts` é um só), e o
//! `util.ts` do grid em [`grid_util`].
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{
    DisposableRef, DisposableStore, DisposableTracker, IDisposable, set_disposable_tracker,
};
use je_base::promise::Promise;
use je_base::runtime;

thread_local! {
    static UNEXPECTED: RefCell<Vec<Error>> = const { RefCell::new(Vec::new()) };
}

/// Em Node o tratador padrão de erro inesperado relança num `setTimeout`, e o mocha reprova o teste.
/// Aqui o tratador guarda o erro, e o teste reprova ao terminar (ver [`fail_on_unexpected_errors`]).
pub fn record_unexpected_errors() {
    errors::set_unexpected_error_handler(|e| UNEXPECTED.with(|u| u.borrow_mut().push(e.clone())));
}

/// Reprova o teste se algum erro inesperado foi reportado desde a última conferência.
pub fn fail_on_unexpected_errors() {
    let reported = UNEXPECTED.with(|u| std::mem::take(&mut *u.borrow_mut()));
    assert!(
        reported.is_empty(),
        "erros inesperados reportados: {}",
        reported
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// `ensureNoDisposablesAreLeakedInTestSuite()`: o `setup` roda na criação e o `teardown` no `Drop`.
/// Devolve o store que o teste usa para registrar o que precisa ser descartado ao fim.
pub struct LeakGuard {
    store: Rc<DisposableStore>,
    tracker: Rc<DisposableTracker>,
}

pub fn ensure_no_disposables_are_leaked_in_test_suite() -> LeakGuard {
    record_unexpected_errors();
    let store = DisposableStore::new();
    let tracker = Rc::new(DisposableTracker::new());
    set_disposable_tracker(Some(tracker.clone()));
    LeakGuard { store, tracker }
}

impl LeakGuard {
    /// `add`: registra para descartar ao fim do teste.
    pub fn add(&self, o: DisposableRef) -> DisposableRef {
        self.store.add(o)
    }

    pub fn add_rc<T: IDisposable + 'static>(&self, o: Rc<T>) -> Rc<T> {
        self.store.add_rc(o)
    }

    /// O store, para quem precisa dele dentro de um fecho `'static`.
    pub fn store(&self) -> Rc<DisposableStore> {
        self.store.clone()
    }
}

impl Drop for LeakGuard {
    fn drop(&mut self) {
        self.store.dispose();
        set_disposable_tracker(None);
        if std::thread::panicking() {
            return;
        }
        fail_on_unexpected_errors();
        compute_leaking_disposables(&self.tracker, true);
    }
}

fn compute_leaking_disposables(tracker: &DisposableTracker, log_to_console: bool) {
    if let Some(result) = tracker.compute_leaking_disposables() {
        if log_to_console {
            eprintln!("{}", result.details);
        }
        panic!(
            "There are {} undisposed disposables!{}",
            result.leaks.len(),
            result.details
        );
    }
}

/// `throwIfDisposablesAreLeaked(body, logToConsole)`.
pub fn throw_if_disposables_are_leaked(body: impl FnOnce(), log_to_console: bool) {
    let tracker = Rc::new(DisposableTracker::new());
    set_disposable_tracker(Some(tracker.clone()));
    body();
    set_disposable_tracker(None);
    compute_leaking_disposables(&tracker, log_to_console);
}

/// `timeout(ms)` do `async.ts`, sem o cancelamento (nenhum teste portado cancela).
pub fn timeout(ms: u64) -> Promise<()> {
    Promise::with_executor(|resolver| {
        runtime::set_timeout(move || resolver.resolve(()), ms);
    })
}

/// `runWithFakedTimers({}, async () => ...)`: roda o corpo com o relógio virtual, esvazia microtarefas e
/// timers, e reprova se o corpo não terminou (ficou esperando algo que nunca acontece) ou se algo lançou.
pub fn run_with_faked_timers(body: impl Future<Output = ()> + 'static) {
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    runtime::with_faked_timers(|| {
        runtime::spawn_local(async move {
            body.await;
            finished.set(true);
        });
    });
    fail_on_unexpected_errors();
    assert!(done.get(), "o teste assíncrono não terminou");
}

pub mod grid_util;

use je_ui::{NodeId, Role, Tree, Ui};

/// `document.createElement('div')` com `style.position/width/height`: um nó solto de `width` x `height`.
pub fn create_container(ui: &Ui, width: f64, height: f64) -> NodeId {
    let mut tree = ui.tree_mut();
    let container = tree.create_detached(Role::Generic);
    tree.set_bounds(container, je_render::kurbo::Rect::new(0.0, 0.0, width, height));
    container
}

/// O `querySelectorAll` restrito ao que os testes usam: uma cadeia de classes ligadas por `>`, em que a
/// primeira pode estar em qualquer profundidade abaixo de `root` e cada seguinte é filha direta da anterior.
pub fn query_all(tree: &Tree, root: NodeId, chain: &[&str]) -> Vec<NodeId> {
    let (first, rest) = chain.split_first().expect("seletor vazio");
    let mut found: Vec<NodeId> = tree.subtree(root).into_iter().skip(1).filter(|n| tree.has_class(*n, first)).collect();
    for class in rest {
        found = found.into_iter().flat_map(|n| tree.children_with_class(n, class)).collect();
    }
    found
}

/// `assert.throws(fn)`: falha o teste se `f` não lançar.
pub fn assert_throws(f: impl FnOnce()) {
    assert!(je_base::errors::catch(f).is_err(), "deveria lançar");
}
