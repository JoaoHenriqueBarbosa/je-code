//! Porte de `src/vs/workbench/contrib/onboarding/browser/onboardingPresentationQueue.ts`: serializa as
//! apresentações de onboarding dentro de uma janela, sem compartilhar o agendamento nem a persistência delas.
//!
//! Divergências:
//! - `Window` é a [`OnboardingWindow`] (sem DOM), chave por identidade do `SequencerByKey<Window>`.
//! - `async () => T` é `FnOnce() -> Promise<T>`; o `throw new CancellationError()` do corpo da tarefa é a promessa
//!   rejeitada com [`Error::cancellation`], que é o que o `throw` dentro de uma tarefa assíncrona produz.
//! - O `SequencerByKey` de módulo do original é por thread (`thread_local!`).

use je_base::async_::{SequencerByKey, race_cancellation_error};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::Promise;

use super::onboarding_target::OnboardingWindow;

thread_local! {
    static PRESENTATIONS: SequencerByKey<OnboardingWindow> = SequencerByKey::new();
}

/// Serializa apresentações dentro de uma janela sem compartilhar o agendamento nem a persistência delas.
pub fn run_with_onboarding_presentation<T: Clone + 'static>(
    target_window: &OnboardingWindow,
    token: &CancellationToken,
    run: impl FnOnce() -> Promise<T> + 'static,
) -> Promise<T> {
    let task_token = token.clone();
    let queued = PRESENTATIONS.with(|presentations| {
        presentations.queue(target_window.clone(), move || {
            if task_token.is_cancellation_requested() {
                return Promise::rejected(Error::cancellation());
            }
            run()
        })
    });
    race_cancellation_error(&queued, token)
}
