//! Porte de `src/vs/workbench/services/progress/test/browser/progressIndicator.test.ts` (suíte `Progress Indicator`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `teardown` (`disposables.clear()`) é o `drop` do `LeakGuard` ao fim do corpo, que também faz o
//!   `ensureNoDisposablesAreLeakedInTestSuite()`.
//! - **`TestProgressBar extends ProgressBar`.** Os indicadores usam a barra pelo trait [`IProgressBar`] (ver o cabeçalho de
//!   `progress_indicator.rs`), e o [`TestProgressBar`] o implementa com os mesmos sete membros que a subclasse do original
//!   sobrescreve (`infinite`, `total`, `hasTotal`, `worked`, `done`/`stop`, `show` e `hide`) e os campos `fTotal`, `fWorked`,
//!   `fInfinite` e `fDone`. Como a subclasse sobrescreve tudo o que os indicadores chamam, a base (o `ProgressBar` sobre o
//!   `div` de `document.createElement`) nunca é exercitada, e o dublê não tem nó nem descarte. `null!` (de `fDone`, `fInfinite`,
//!   `fWorked` e `fTotal`) é `false` e `0`: o caso só os lê por `!!` ou comparando com `true` e com os números.
//! - **`new class extends AbstractProgressScope`.** A subclasse só expõe `onScopeOpened` e `onScopeClosed` (`protected`) como
//!   `testOnScopeOpened` e `testOnScopeClosed`; no porte esses dois métodos são públicos, então o caso usa o
//!   [`AbstractProgressScope`] criado com `('test.scopeId', true)` e chama `on_scope_opened` e `on_scope_closed`.
//! - **`assert.strictEqual(a, b)`.** É `assert_eq!`; `!!x` é o valor lógico do campo.
//! - **`Promise.resolve(null)`.** É `Promise::resolved(())`, e `await testObject.showWhile(p)` é `.await.unwrap()`.
//! - **Relógio.** O original não usa timers falsos: o caso roda em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::promise::Promise;
use je_editor::code_action::platform::{IProgressIndicator, ProgressTotal};
use je_workbench::services::progress::progress_indicator::{AbstractProgressScope, IProgressBar, IProgressScope, ScopedProgressIndicator};

/// `TestProgressBar`.
#[derive(Default)]
struct TestProgressBar {
    f_total: Cell<f64>,
    f_worked: Cell<f64>,
    f_infinite: Cell<bool>,
    f_done: Cell<bool>,
}

impl TestProgressBar {
    /// `override done()`.
    fn done(&self) {
        self.f_done.set(true);

        self.f_infinite.set(false);
        self.f_worked.set(0.0);
        self.f_total.set(0.0);
    }
}

impl IProgressBar for TestProgressBar {
    fn infinite(&self) {
        self.f_done.set(false);
        self.f_infinite.set(true);
    }

    fn total(&self, total: f64) {
        self.f_done.set(false);
        self.f_total.set(total);
    }

    fn has_total(&self) -> bool {
        self.f_total.get() != 0.0
    }

    fn worked(&self, worked: f64) {
        self.f_done.set(false);

        if self.f_worked.get() != 0.0 {
            self.f_worked.set(self.f_worked.get() + worked);
        } else {
            self.f_worked.set(worked);
        }
    }

    fn stop(&self) {
        self.done();
    }

    fn show(&self, _delay: Option<u64>) {}

    fn hide(&self) {}
}

#[test]
fn scoped_progress_indicator() {
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();

        let test_progress_bar = Rc::new(TestProgressBar::default());
        let progress_scope = disposables.add_rc(AbstractProgressScope::new("test.scopeId", true));
        let progress_scope_ref: Rc<dyn IProgressScope> = progress_scope.clone();
        let progress_bar_ref: Rc<dyn IProgressBar> = test_progress_bar.clone();
        let test_object = disposables.add_rc(ScopedProgressIndicator::new(progress_bar_ref, progress_scope_ref));

        // Active: Show (Infinite)
        let mut runner = test_object.show(ProgressTotal::Infinite, None);
        assert_eq!(true, test_progress_bar.f_infinite.get());
        runner.done();
        assert_eq!(true, test_progress_bar.f_done.get());

        // Active: Show (Total / Worked)
        runner = test_object.show(ProgressTotal::Total(100.0), None);
        assert_eq!(false, test_progress_bar.f_infinite.get());
        assert_eq!(100.0, test_progress_bar.f_total.get());
        runner.worked(20.0);
        assert_eq!(20.0, test_progress_bar.f_worked.get());
        runner.total(80.0);
        assert_eq!(80.0, test_progress_bar.f_total.get());
        runner.done();
        assert_eq!(true, test_progress_bar.f_done.get());

        // Inactive: Show (Infinite)
        progress_scope.on_scope_closed("test.scopeId");
        test_object.show(ProgressTotal::Infinite, None);
        assert_eq!(false, test_progress_bar.f_infinite.get());
        progress_scope.on_scope_opened("test.scopeId");
        assert_eq!(true, test_progress_bar.f_infinite.get());

        // Inactive: Show (Total / Worked)
        progress_scope.on_scope_closed("test.scopeId");
        runner = test_object.show(ProgressTotal::Total(100.0), None);
        runner.total(80.0);
        runner.worked(20.0);
        assert_eq!(false, test_progress_bar.f_total.get() != 0.0);
        progress_scope.on_scope_opened("test.scopeId");
        assert_eq!(20.0, test_progress_bar.f_worked.get());
        assert_eq!(80.0, test_progress_bar.f_total.get());

        // Acive: Show While
        let mut p = Promise::resolved(());
        test_object.show_while(&p, None).await.unwrap();
        assert_eq!(true, test_progress_bar.f_done.get());
        progress_scope.on_scope_closed("test.scopeId");
        p = Promise::resolved(());
        test_object.show_while(&p, None).await.unwrap();
        assert_eq!(true, test_progress_bar.f_done.get());
        progress_scope.on_scope_opened("test.scopeId");
        assert_eq!(true, test_progress_bar.f_done.get());

        drop(runner);
        drop(disposables);
    });
}
