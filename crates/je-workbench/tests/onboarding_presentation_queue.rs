//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingPresentationQueue.test.ts` (suíte `Onboarding
//! presentation isolation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `store.add` vira
//!   `guard.add_rc`.
//! - **`mainWindow`.** É uma [`OnboardingWindow`] por caso (o porte não tem DOM).
//! - **`await`.** As promessas do porte assentam nas microtarefas; cada `await` do original vira
//!   `runtime::run_microtasks()` seguido da conferência do resultado (`Promise::result`).
//! - **`assert.rejects(queued, isCancellationError)`.** É `Some(Err(e))` com `is_cancellation_error(&e)`.
//!   `assert.rejects(..., /Failed/)` confere a mensagem.
//! - **Recorte: o terceiro caso do original** (`guided tryouts wait for ordinary tours without overlapping or changing
//!   their dimming`) NÃO está aqui. Ele monta o `SpotlightPresentation`, o `GuidedTryoutPresentation`, o
//!   `OnboardingScenarioService`, o `ContextKeyService`, o `TestHostService` e o `TestLayoutService`, e lê `.spotlight-overlay`
//!   no DOM. Nenhum deles foi portado ainda (`onboardingService.ts`, `spotlightPresentation.ts`, `spotlightOverlay.ts`,
//!   `guidedTryoutPresentation.ts`, `onboardingPresentation.ts` e o registro de apresentações de tryout); o caso entra com
//!   eles, sem stub, junto com o `teardown(() => Memento.clear(StorageScope.APPLICATION))` da suíte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{Error, is_cancellation_error};
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_workbench::contrib::onboarding::onboarding_presentation_queue::run_with_onboarding_presentation;
use je_workbench::contrib::onboarding::onboarding_target::OnboardingWindow;

#[test]
fn a_cancelled_queued_presentation_never_starts_and_does_not_block_later_work() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let finish = DeferredPromise::<()>::new();
    let active = run_with_onboarding_presentation(&window, &CancellationToken::None, {
        let finish = finish.promise.clone();
        move || finish
    });
    let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
    let launches = Rc::new(Cell::new(0));
    let queued = run_with_onboarding_presentation(&window, &cancellation.token(), {
        let launches = launches.clone();
        move || {
            launches.set(launches.get() + 1);
            Promise::resolved(())
        }
    });
    cancellation.cancel();
    runtime::run_microtasks();
    match queued.result() {
        Some(Err(error)) => assert!(is_cancellation_error(&error), "o erro é de cancelamento: {error}"),
        other => panic!("a apresentação em fila deveria rejeitar com cancelamento, veio {:?}", other.map(|result| result.is_ok())),
    }
    let later = run_with_onboarding_presentation(&window, &CancellationToken::None, {
        let launches = launches.clone();
        move || {
            launches.set(launches.get() + 1);
            Promise::resolved(())
        }
    });
    finish.complete(());
    runtime::run_microtasks();
    assert!(matches!(active.result(), Some(Ok(_))), "a apresentação ativa terminou");
    assert!(matches!(later.result(), Some(Ok(_))), "o trabalho posterior rodou");
    assert_eq!(launches.get(), 1);
}

#[test]
fn a_failed_presentation_releases_its_window_for_later_work() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let failed = run_with_onboarding_presentation(&window, &CancellationToken::None, || {
        Promise::<()>::rejected(Error::new("Failed"))
    });
    runtime::run_microtasks();
    match failed.result() {
        Some(Err(error)) => assert!(error.message.contains("Failed"), "mensagem: {}", error.message),
        other => panic!("a apresentação deveria rejeitar com Failed, veio {:?}", other.map(|result| result.is_ok())),
    }
    let ready = run_with_onboarding_presentation(&window, &CancellationToken::None, || {
        Promise::resolved("ready".to_string())
    });
    runtime::run_microtasks();
    assert_eq!(ready.result().and_then(Result::ok), Some("ready".to_string()));
}
