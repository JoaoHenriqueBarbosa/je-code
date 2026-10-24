//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingPresentation.ts`: o contexto de uma execução, a
//! apresentação que desenha um cenário e o registro de apresentações por `kind`.
//!
//! Divergências:
//! - `targetWindow: Window` é a [`OnboardingWindow`] (sem DOM); `onDidShow?: () => void` é `Option<Rc<dyn Fn()>>`.
//! - O registro de apresentações do original (`OnboardingPresentationRegistry`) e o de passos de sequência
//!   (`OnboardingSequenceStepPresentationRegistry`, em `onboardingSequence.ts`) têm o mesmo corpo, só mudando o texto
//!   do erro de duplicata. Para não repetir, os dois são o [`KindRegistry`] genérico, que recebe a descrição do
//!   texto e a função que lê o `kind`. `IOnboardingPresentationRegistry` (a interface, de uma só implementação)
//!   é o próprio [`KindRegistry`].
//! - O registro global (`onboardingPresentationRegistry`, criado na carga do módulo) é
//!   [`onboarding_presentation_registry`], uma instância por thread, criada no primeiro uso.
//! - `register` devolve o `{ dispose }` literal do original (um disposable sem rastreamento).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, plain_disposable};
use je_base::promise::Promise;

use super::onboarding_scenario::{OnboardingRunResult, OnboardingScenario};
use super::onboarding_target::OnboardingWindow;

/// `IOnboardingRunContext`: o que uma apresentação recebe para uma execução de cenário, sem vazar os detalhes do
/// motor.
#[derive(Clone)]
pub struct OnboardingRunContext {
    /// A janela que hospeda a UI que o cenário mira (em geral a janela principal).
    pub target_window: OnboardingWindow,

    /// Avisa que a apresentação desenhou o primeiro elemento visível. Chamado no máximo uma vez por execução.
    pub on_did_show: Option<Rc<dyn Fn()>>,

    /// Dispara quando o motor quer que a apresentação aborte a execução atual (por exemplo, o aplicativo está
    /// fechando). A apresentação deve resolver o `run` com um resultado abortado.
    pub on_abort: Event<()>,

    /// Limita a resolução de alvos marcados pelo dono a uma instância preparada da UI.
    pub target_scope: Option<String>,
}

/// `IOnboardingPresentation`: desenha um cenário. O motor não conhece o estilo: procura a apresentação pelo `kind`
/// de `IOnboardingScenario.presentation` e delega o desenho a ela.
pub trait IOnboardingPresentation {
    /// O `kind` que esta apresentação trata, por exemplo `spotlight`.
    fn kind(&self) -> &str;

    /// Desenha o cenário e resolve com o resultado quando o usuário (ou o motor) encerra a execução. A
    /// implementação precisa limpar toda a UI e todos os ouvintes, não importa como a execução termine.
    fn run(&self, scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult>;
}

/// O registro que mapeia o `kind` de uma apresentação para a implementação dela.
pub struct KindRegistry<T: ?Sized + 'static> {
    /// O começo do texto do erro de duplicata, por exemplo `An onboarding presentation`.
    description: &'static str,
    kind_of: fn(&T) -> String,
    entries: Rc<RefCell<Vec<(String, Rc<T>)>>>,
    on_did_change: Rc<Emitter<()>>,
}

impl<T: ?Sized + 'static> KindRegistry<T> {
    pub fn new(description: &'static str, kind_of: fn(&T) -> String) -> Self {
        Self { description, kind_of, entries: Rc::new(RefCell::new(Vec::new())), on_did_change: Emitter::new() }
    }

    /// Registra uma apresentação. Lança se o `kind` já estiver registrado.
    pub fn register(&self, presentation: Rc<T>) -> DisposableRef {
        let kind = (self.kind_of)(&presentation);
        if self.entries.borrow().iter().any(|(existing, _)| *existing == kind) {
            errors::throw(Error::new(&format!("{} with kind '{kind}' is already registered.", self.description)));
        }
        self.entries.borrow_mut().push((kind.clone(), presentation.clone()));
        self.on_did_change.fire(());
        let entries = self.entries.clone();
        let on_did_change = self.on_did_change.clone();
        plain_disposable(move || {
            let is_current = entries.borrow().iter().any(|(existing, current)| *existing == kind && Rc::ptr_eq(current, &presentation));
            if is_current {
                entries.borrow_mut().retain(|(existing, _)| *existing != kind);
                on_did_change.fire(());
            }
        })
    }

    /// Busca uma apresentação pelo `kind`.
    pub fn get(&self, kind: &str) -> Option<Rc<T>> {
        self.entries.borrow().iter().find(|(existing, _)| existing == kind).map(|(_, presentation)| presentation.clone())
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }
}

thread_local! {
    static ONBOARDING_PRESENTATION_REGISTRY: Rc<KindRegistry<dyn IOnboardingPresentation>> = Rc::new(KindRegistry::new(
        "An onboarding presentation",
        |presentation| presentation.kind().to_string(),
    ));
}

/// `onboardingPresentationRegistry`: o registro global de apresentações. Elas se registram aqui (em geral na
/// contribuição do workbench) e o motor as resolve ao rodar cenários.
pub fn onboarding_presentation_registry() -> Rc<KindRegistry<dyn IOnboardingPresentation>> {
    ONBOARDING_PRESENTATION_REGISTRY.with(Rc::clone)
}
