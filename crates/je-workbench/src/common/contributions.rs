//! Porte parcial de `src/vs/workbench/common/contributions.ts`: o contrato `IWorkbenchContribution` e o
//! `WorkbenchContributionsRegistry` (`registerWorkbenchContribution`), o ponto de extensão em que cada contribuição declara em
//! que fase do ciclo de vida do workbench ela nasce.
//!
//! Divergências:
//! - **Contrato.** `IWorkbenchContribution` é um trait sem membros (a interface do original é vazia); a contribuição que
//!   precisa de descarte também implementa `IDisposable`.
//! - **Construção.** `registerWorkbenchContribution(ctor, phase)` guarda a classe e o `IInstantiationService.createInstance`
//!   a cria. Aqui o construtor é o [`WorkbenchContributionCtor`], um fecho que recebe o `ServicesAccessor` (o
//!   `invokeFunction` do serviço de instanciação) e devolve a contribuição, que é o que `createInstance` faz ao resolver as
//!   dependências decoradas.
//! - **Registro.** `Registry.as(Extensions.Workbench)` é o [`workbench_contributions_registry`], a instância por thread (o
//!   mesmo tratamento do `EditorExtensionsRegistry`). O `WorkbenchContributionsRegistry` do original, além de guardar os
//!   registros, os instancia por fase ao iniciar o workbench (`start`, que espera o `ILifecycleService.when(phase)`). Essa
//!   parte depende da montagem do workbench e não foi portada: [`WorkbenchContributionsRegistry::instantiate`] cria as
//!   contribuições de uma fase e quem monta o workbench o chama quando a fase chega.
//! - `registerWorkbenchContribution2` (com `WorkbenchPhase` e as contribuições por id) não foi portado: nenhuma fatia
//!   portada o usa.

use std::cell::RefCell;
use std::rc::Rc;

use je_instantiation::ServicesAccessor;

use crate::services::lifecycle::lifecycle::LifecyclePhase;

/// `IWorkbenchContribution`.
pub trait IWorkbenchContribution {}

/// A classe de uma contribuição (o `IConstructorSignature<IWorkbenchContribution>` do original).
pub type WorkbenchContributionCtor = Rc<dyn Fn(&ServicesAccessor) -> Rc<dyn IWorkbenchContribution>>;

/// `WorkbenchContributionsRegistry`.
#[derive(Default)]
pub struct WorkbenchContributionsRegistry {
    registrations: RefCell<Vec<(LifecyclePhase, WorkbenchContributionCtor)>>,
}

impl WorkbenchContributionsRegistry {
    /// `registerWorkbenchContribution(contribution, phase)`.
    pub fn register_workbench_contribution(&self, contribution: WorkbenchContributionCtor, phase: LifecyclePhase) {
        self.registrations.borrow_mut().push((phase, contribution));
    }

    /// Quantas contribuições estão registradas na fase.
    pub fn count(&self, phase: LifecyclePhase) -> usize {
        self.registrations.borrow().iter().filter(|(registered, _)| *registered == phase).count()
    }

    /// Cria as contribuições registradas na fase, na ordem do registro.
    pub fn instantiate(&self, phase: LifecyclePhase, accessor: &ServicesAccessor) -> Vec<Rc<dyn IWorkbenchContribution>> {
        let constructors: Vec<WorkbenchContributionCtor> = self
            .registrations
            .borrow()
            .iter()
            .filter(|(registered, _)| *registered == phase)
            .map(|(_, constructor)| Rc::clone(constructor))
            .collect();
        constructors.iter().map(|constructor| constructor(accessor)).collect()
    }
}

thread_local! {
    static WORKBENCH_CONTRIBUTIONS_REGISTRY: Rc<WorkbenchContributionsRegistry> = Rc::new(WorkbenchContributionsRegistry::default());
}

/// `Registry.as<IWorkbenchContributionsRegistry>(WorkbenchExtensions.Workbench)`.
pub fn workbench_contributions_registry() -> Rc<WorkbenchContributionsRegistry> {
    WORKBENCH_CONTRIBUTIONS_REGISTRY.with(Rc::clone)
}
