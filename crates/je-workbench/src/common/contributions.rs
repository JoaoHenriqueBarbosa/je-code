//! Porte de `src/vs/workbench/common/contributions.ts`: o contrato `IWorkbenchContribution`, as fases de instanciação
//! (`WorkbenchPhase`) e o `WorkbenchContributionsRegistry`, o ponto de extensão em que cada contribuição declara quando ela
//! nasce: numa fase do ciclo de vida do workbench, sob demanda (`lazy`) ou quando um painel de editor é criado.
//!
//! Divergências:
//! - **Contrato.** `IWorkbenchContribution` é um trait sem membros além do `Any` (que deixa o teste e o chamador descobrirem
//!   o tipo concreto, o `instanceof` do original) e do [`IWorkbenchContribution::disposable`], que é o `isDisposable(instance)`:
//!   a contribuição que precisa de descarte o implementa devolvendo a si mesma.
//! - **Construção.** `registerWorkbenchContribution2(id, ctor, ...)` guarda a classe e o `IInstantiationService.createInstance`
//!   a cria. Aqui o construtor é o [`WorkbenchContributionCtor`], um fecho que recebe o `ServicesAccessor` (o
//!   `invokeFunction` do serviço de instanciação) e devolve a contribuição, que é o que `createInstance` faz ao resolver as
//!   dependências decoradas. O `constructor.name` que o original usa nas mensagens de uma contribuição sem identificador não
//!   existe: a mensagem diz `(anonymous)`.
//! - **Registro.** `Registry.as(Extensions.Workbench)` é o [`workbench_contributions_registry`], a instância por thread (o
//!   mesmo tratamento do `EditorExtensionsRegistry`), e `WorkbenchContributionsRegistry.INSTANCE` é a mesma instância;
//!   [`register_workbench_contribution2`] e [`get_workbench_contribution`] são as funções do módulo sobre ela. Um registro
//!   novo (`new WorkbenchContributionsRegistry()`) é [`WorkbenchContributionsRegistry::new`].
//! - **`start(accessor)`.** O `ServicesAccessor` do porte não entrega o `IInstantiationService` (que o original lê com
//!   `accessor.get(IInstantiationService)`), então [`WorkbenchContributionsRegistry::start`] recebe o serviço de instanciação
//!   e lê os demais serviços por `invokeFunction` nele.
//! - **Erros.** `throw new Error(...)` de `getWorkbenchContribution` é o `Err` do resultado. O construtor que lança vira um
//!   pânico capturado por [`errors::catch`], e `logService.error(message, error)` é `error(message)` com o texto do erro.
//! - **Fases e promessas.** `lifecycleService.when(phase).then(...)` é o `then` da promessa do serviço; `await
//!   this.pendingRestoredContributions.p` da fase `Eventually` é o `then` da promessa dela. `Date.now()` é o relógio do
//!   `runtime`, e `mark` o de `performance`.
//! - **`register_workbench_contribution` e `instantiate`.** O registro por fase (`registerWorkbenchContribution(ctor, phase)`,
//!   sem identificador) aceita as quatro fases, e [`WorkbenchContributionsRegistry::instantiate`] cria as contribuições de uma
//!   fase sem registrar instâncias, para quem monta o workbench sem o ciclo de vida.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{IdleDeadline, run_when_global_idle};
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::performance::mark;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_instantiation::{InstantiationService, ServicesAccessor};
use je_platform::log::{I_LOG_SERVICE, LogService, Logger};

use crate::services::editor::editor_pane_service::{IEditorPaneService, I_EDITOR_PANE_SERVICE};
use crate::services::environment::environment_service::{IWorkbenchEnvironmentService, I_WORKBENCH_ENVIRONMENT_SERVICE};
use crate::services::lifecycle::lifecycle::{ILifecycleService, I_LIFECYCLE_SERVICE, LifecyclePhase};

/// `IWorkbenchContribution`: a workbench contribution that will be loaded when the workbench starts and disposed when the
/// workbench shuts down.
pub trait IWorkbenchContribution: Any {
    /// `isDisposable(instance)`: o descarte da contribuição, que o registro guarda e executa ao desligar o workbench.
    fn disposable(self: Rc<Self>) -> Option<DisposableRef> {
        None
    }
}

/// `WorkbenchPhase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkbenchPhase {
    /// The first phase signals that we are about to startup getting ready.
    ///
    /// Note: doing work in this phase blocks an editor from showing to the user, so please rather consider to use the
    /// other types, preferable `Lazy` to only instantiate the contribution when really needed.
    BlockStartup,
    /// Services are ready and the window is about to restore its UI state.
    ///
    /// Note: doing work in this phase blocks an editor from showing to the user, so please rather consider to use the
    /// other types, preferable `Lazy` to only instantiate the contribution when really needed.
    BlockRestore,
    /// Views, panels and editors have restored. Editors are given a bit of time to restore their contents.
    AfterRestored,
    /// The last phase after views, panels and editors have restored and some time has passed (2-5 seconds).
    Eventually,
}

impl WorkbenchPhase {
    /// `toLifecyclePhase(instantiation)`.
    pub fn lifecycle_phase(self) -> LifecyclePhase {
        match self {
            Self::BlockStartup => LifecyclePhase::Starting,
            Self::BlockRestore => LifecyclePhase::Ready,
            Self::AfterRestored => LifecyclePhase::Restored,
            Self::Eventually => LifecyclePhase::Eventually,
        }
    }
}

/// `toWorkbenchPhase(phase)`.
impl From<LifecyclePhase> for WorkbenchPhase {
    fn from(phase: LifecyclePhase) -> Self {
        match phase {
            LifecyclePhase::Starting => Self::BlockStartup,
            LifecyclePhase::Ready => Self::BlockRestore,
            LifecyclePhase::Restored => Self::AfterRestored,
            LifecyclePhase::Eventually => Self::Eventually,
        }
    }
}

/// `WorkbenchContributionInstantiation`: `WorkbenchPhase | ILazyWorkbenchContributionInstantiation |
/// IOnEditorWorkbenchContributionInstantiation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkbenchContributionInstantiation {
    /// Uma fase do ciclo de vida.
    Phase(WorkbenchPhase),
    /// `{ lazy: true }`: only instantiated when calling `getWorkbenchContribution`.
    Lazy,
    /// `{ editorTypeId }`: instantiated when the corresponding editor is being created.
    OnEditor { editor_type_id: String },
}

impl From<WorkbenchPhase> for WorkbenchContributionInstantiation {
    fn from(phase: WorkbenchPhase) -> Self {
        Self::Phase(phase)
    }
}

/// A classe de uma contribuição (o `IConstructorSignature<IWorkbenchContribution>` do original).
pub type WorkbenchContributionCtor = Rc<dyn Fn(&ServicesAccessor) -> Rc<dyn IWorkbenchContribution>>;

/// `IWorkbenchContributionRegistration`.
struct Registration {
    id: Option<String>,
    ctor: WorkbenchContributionCtor,
}

impl Registration {
    /// `contribution.id ?? contribution.ctor.name`.
    fn name(&self) -> &str {
        self.id.as_deref().unwrap_or("(anonymous)")
    }
}

/// Os serviços que o registro lê em `start`.
struct StartedServices {
    instantiation_service: Rc<InstantiationService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    log_service: Rc<dyn LogService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    editor_pane_service: Rc<dyn IEditorPaneService>,
}

/// O que `doInstantiateWhenIdle` guarda entre as rodadas ociosas: as contribuições e o índice da próxima (`i`).
struct IdleBatch {
    contributions: Vec<Rc<Registration>>,
    next: Cell<usize>,
}

/// `WorkbenchContributionsRegistry`.
pub struct WorkbenchContributionsRegistry {
    this: Weak<WorkbenchContributionsRegistry>,
    base: Rc<Disposable>,
    services: RefCell<Option<Rc<StartedServices>>>,
    contributions_by_phase: RefCell<BTreeMap<LifecyclePhase, Vec<Rc<Registration>>>>,
    contributions_by_editor: RefCell<IndexMap<String, Vec<Rc<Registration>>>>,
    contributions_by_id: RefCell<HashMap<String, Rc<Registration>>>,
    instances_by_id: RefCell<HashMap<String, Rc<dyn IWorkbenchContribution>>>,
    instance_disposables: Rc<DisposableStore>,
    timings_by_phase: RefCell<BTreeMap<LifecyclePhase, Vec<(String, u64)>>>,
    pending_restored_contributions: DeferredPromise<()>,
}

impl WorkbenchContributionsRegistry {
    const BLOCK_BEFORE_RESTORE_WARN_THRESHOLD: u64 = 20;
    const BLOCK_AFTER_RESTORE_WARN_THRESHOLD: u64 = 100;

    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let instance_disposables = DisposableStore::new();
        base.register(instance_disposables.clone());

        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            services: RefCell::new(None),
            contributions_by_phase: RefCell::new(BTreeMap::new()),
            contributions_by_editor: RefCell::new(IndexMap::new()),
            contributions_by_id: RefCell::new(HashMap::new()),
            instances_by_id: RefCell::new(HashMap::new()),
            instance_disposables,
            timings_by_phase: RefCell::new(BTreeMap::new()),
            pending_restored_contributions: DeferredPromise::new(),
        })
    }

    /// `timings`: provides access to the instantiation times of all contributions by lifecycle phase (`[ID, Creation Time]`).
    pub fn timings(&self) -> BTreeMap<LifecyclePhase, Vec<(String, u64)>> {
        self.timings_by_phase.borrow().clone()
    }

    /// `whenRestored`: a promise that resolves when all contributions up to the `Restored` phase have been instantiated.
    pub fn when_restored(&self) -> Promise<()> {
        self.pending_restored_contributions.promise.clone()
    }

    /// `registerWorkbenchContribution2(id, ctor, instantiation)`.
    pub fn register_workbench_contribution2(&self, id: Option<&str>, ctor: WorkbenchContributionCtor, instantiation: impl Into<WorkbenchContributionInstantiation>) {
        let instantiation = instantiation.into();
        let contribution = Rc::new(Registration { id: id.map(str::to_owned), ctor });

        // Instantiate directly if we already have a matching instantiation condition
        let services = self.services.borrow().clone();
        if let Some(services) = &services {
            let direct = match &instantiation {
                WorkbenchContributionInstantiation::Phase(phase) => services.lifecycle_service.phase() >= phase.lifecycle_phase(),
                WorkbenchContributionInstantiation::OnEditor { editor_type_id } => id.is_some() && services.editor_pane_service.did_instantiate_editor_pane(editor_type_id),
                WorkbenchContributionInstantiation::Lazy => false,
            };
            if direct {
                let phase = match &instantiation {
                    WorkbenchContributionInstantiation::Phase(phase) => phase.lifecycle_phase(),
                    _ => services.lifecycle_service.phase(),
                };
                self.safe_create_contribution(services, &contribution, phase);
                return;
            }
        }

        // Otherwise keep contributions by instantiation kind for later instantiation

        // by phase
        if let WorkbenchContributionInstantiation::Phase(phase) = &instantiation {
            self.contributions_by_phase.borrow_mut().entry(phase.lifecycle_phase()).or_default().push(contribution.clone());
        }

        if let Some(id) = id {
            // by id
            let already_registered = self.contributions_by_id.borrow().contains_key(id);
            if already_registered {
                eprintln!("IWorkbenchContributionsRegistry#registerWorkbenchContribution(): Can't register multiple contributions with same id '{id}'");
            } else {
                self.contributions_by_id.borrow_mut().insert(id.to_owned(), contribution.clone());
            }

            // by editor
            if let WorkbenchContributionInstantiation::OnEditor { editor_type_id } = &instantiation {
                self.contributions_by_editor.borrow_mut().entry(editor_type_id.clone()).or_default().push(contribution);
            }
        }
    }

    /// `registerWorkbenchContribution(ctor, phase)`.
    pub fn register_workbench_contribution(&self, ctor: WorkbenchContributionCtor, phase: LifecyclePhase) {
        self.register_workbench_contribution2(None, ctor, WorkbenchPhase::from(phase));
    }

    /// `getWorkbenchContribution(id)`: the contribution is created if not yet done. Fails if called too early (before the
    /// registry started), if no contribution is known for the given identifier, or if the creation failed.
    pub fn get_workbench_contribution(&self, id: &str) -> Result<Rc<dyn IWorkbenchContribution>, Error> {
        if let Some(instance) = self.instances_by_id.borrow().get(id) {
            return Ok(instance.clone());
        }

        let Some(services) = self.services.borrow().clone() else {
            return Err(Error::new(format!("IWorkbenchContributionsRegistry#getContribution('{id}'): cannot be called before registry started")));
        };

        let Some(contribution) = self.contributions_by_id.borrow().get(id).cloned() else {
            return Err(Error::new(format!("IWorkbenchContributionsRegistry#getContribution('{id}'): contribution with that identifier is unknown.")));
        };

        let phase = services.lifecycle_service.phase();
        if phase < LifecyclePhase::Restored {
            services.log_service.warn(&format!("IWorkbenchContributionsRegistry#getContribution('{id}'): contribution instantiated before LifecyclePhase.Restored!"));
        }

        self.safe_create_contribution(&services, &contribution, phase);

        let instance = self.instances_by_id.borrow().get(id).cloned();
        instance.ok_or_else(|| Error::new(format!("IWorkbenchContributionsRegistry#getContribution('{id}'): failed to create contribution.")))
    }

    /// `start(accessor)`: starts the registry by providing the required services.
    pub fn start(self: &Rc<Self>, instantiation_service: &Rc<InstantiationService>) {
        let services = Rc::new(instantiation_service.invoke_function(|accessor| StartedServices {
            instantiation_service: instantiation_service.clone(),
            lifecycle_service: accessor.get(I_LIFECYCLE_SERVICE),
            log_service: accessor.get(I_LOG_SERVICE),
            environment_service: accessor.get(I_WORKBENCH_ENVIRONMENT_SERVICE),
            editor_pane_service: accessor.get(I_EDITOR_PANE_SERVICE),
        }));
        *self.services.borrow_mut() = Some(services.clone());

        // Dispose contributions on shutdown
        self.base.register(services.lifecycle_service.on_did_shutdown().listen({
            let this = self.this.clone();
            move |_| {
                if let Some(this) = this.upgrade() {
                    this.instance_disposables.clear();
                }
            }
        }));

        // Instantiate contributions by phase when they are ready
        for phase in [LifecyclePhase::Starting, LifecyclePhase::Ready, LifecyclePhase::Restored, LifecyclePhase::Eventually] {
            self.instantiate_by_phase(&services, phase);
        }

        // Instantiate contributions by editor when they are created or have been
        let editor_type_ids: Vec<String> = self.contributions_by_editor.borrow().keys().cloned().collect();
        for editor_type_id in editor_type_ids {
            if services.editor_pane_service.did_instantiate_editor_pane(&editor_type_id) {
                self.on_editor(&editor_type_id, &services);
            }
        }
        self.base.register(services.editor_pane_service.on_will_instantiate_editor_pane().listen({
            let this = self.this.clone();
            let services = services.clone();
            move |event| {
                if let Some(this) = this.upgrade() {
                    this.on_editor(&event.editor_type_id, &services);
                }
            }
        }));
    }

    /// Quantas contribuições estão registradas na fase.
    pub fn count(&self, phase: LifecyclePhase) -> usize {
        self.contributions_by_phase.borrow().get(&phase).map_or(0, Vec::len)
    }

    /// Cria as contribuições registradas na fase, na ordem do registro, sem tirá-las do registro nem guardar as instâncias.
    pub fn instantiate(&self, phase: LifecyclePhase, accessor: &ServicesAccessor) -> Vec<Rc<dyn IWorkbenchContribution>> {
        let registrations: Vec<Rc<Registration>> = self.contributions_by_phase.borrow().get(&phase).cloned().unwrap_or_default();
        registrations.iter().map(|registration| (registration.ctor)(accessor)).collect()
    }

    fn on_editor(&self, editor_type_id: &str, services: &Rc<StartedServices>) {
        let contributions = self.contributions_by_editor.borrow_mut().shift_remove(editor_type_id);
        for contribution in contributions.unwrap_or_default() {
            self.safe_create_contribution(services, &contribution, services.lifecycle_service.phase());
        }
    }

    fn instantiate_by_phase(self: &Rc<Self>, services: &Rc<StartedServices>, phase: LifecyclePhase) {
        // Instantiate contributions directly when phase is already reached
        if services.lifecycle_service.phase() >= phase {
            self.do_instantiate_by_phase(services, phase);
        }
        // Otherwise wait for phase to be reached
        else {
            let this = self.this.clone();
            let services_for_phase = services.clone();
            let _ = services.lifecycle_service.when(phase).then(move |()| {
                if let Some(this) = this.upgrade() {
                    this.do_instantiate_by_phase(&services_for_phase, phase);
                }
            });
        }
    }

    fn do_instantiate_by_phase(self: &Rc<Self>, services: &Rc<StartedServices>, phase: LifecyclePhase) {
        let contributions = self.contributions_by_phase.borrow_mut().remove(&phase);
        let Some(contributions) = contributions else {
            return;
        };

        match phase {
            LifecyclePhase::Starting | LifecyclePhase::Ready => {
                // instantiate everything synchronously and blocking
                // measure the time it takes as perf marks for diagnosis

                mark(&format!("code/willCreateWorkbenchContributions/{}", phase as i32), None);

                for contribution in &contributions {
                    self.safe_create_contribution(services, contribution, phase);
                }

                mark(&format!("code/didCreateWorkbenchContributions/{}", phase as i32), None);
            }

            LifecyclePhase::Restored | LifecyclePhase::Eventually => {
                // for the Restored/Eventually-phase we instantiate contributions only when idle. this might take a few
                // idle-busy-cycles but will finish within the timeouts given that, we must ensure to await the
                // contributions from the Restored-phase before we instantiate the Eventually-phase

                if phase == LifecyclePhase::Eventually {
                    let this = self.this.clone();
                    let services = services.clone();
                    let _ = self.pending_restored_contributions.promise.then(move |()| {
                        if let Some(this) = this.upgrade() {
                            this.do_instantiate_when_idle(contributions, &services, phase);
                        }
                    });
                } else {
                    self.do_instantiate_when_idle(contributions, services, phase);
                }
            }
        }
    }

    fn do_instantiate_when_idle(self: &Rc<Self>, contributions: Vec<Rc<Registration>>, services: &Rc<StartedServices>, phase: LifecyclePhase) {
        mark(&format!("code/willCreateWorkbenchContributions/{}", phase as i32), None);

        let forced_timeout = if phase == LifecyclePhase::Eventually { 3000 } else { 500 };
        let batch = Rc::new(IdleBatch { contributions, next: Cell::new(0) });

        Self::run_when_idle(self.this.clone(), batch, services.clone(), phase, forced_timeout);
    }

    /// `runWhenGlobalIdle(instantiateSome, forcedTimeout)`.
    fn run_when_idle(this: Weak<Self>, batch: Rc<IdleBatch>, services: Rc<StartedServices>, phase: LifecyclePhase, forced_timeout: u64) {
        let _ = run_when_global_idle(
            move |idle| {
                if let Some(registry) = this.upgrade() {
                    registry.instantiate_some(&this, &batch, &services, phase, forced_timeout, idle);
                }
            },
            Some(forced_timeout),
        );
    }

    /// `instantiateSome(idle)`.
    fn instantiate_some(&self, this: &Weak<Self>, batch: &Rc<IdleBatch>, services: &Rc<StartedServices>, phase: LifecyclePhase, forced_timeout: u64, idle: IdleDeadline) {
        while batch.next.get() < batch.contributions.len() {
            let contribution = batch.contributions[batch.next.get()].clone();
            batch.next.set(batch.next.get() + 1);
            self.safe_create_contribution(services, &contribution, phase);
            if idle.time_remaining() < 1 {
                // time is up -> reschedule
                Self::run_when_idle(this.clone(), batch.clone(), services.clone(), phase, forced_timeout);
                break;
            }
        }

        if batch.next.get() == batch.contributions.len() {
            mark(&format!("code/didCreateWorkbenchContributions/{}", phase as i32), None);

            if phase == LifecyclePhase::Restored && !self.pending_restored_contributions.is_settled() {
                let _ = self.pending_restored_contributions.complete(());
            }
        }
    }

    fn safe_create_contribution(&self, services: &Rc<StartedServices>, contribution: &Rc<Registration>, phase: LifecyclePhase) {
        if let Some(id) = &contribution.id
            && self.instances_by_id.borrow().contains_key(id)
        {
            return;
        }

        let now = runtime::date_now();

        if let Some(id) = &contribution.id {
            mark(&format!("code/willCreateWorkbenchContribution/{}/{id}", phase as i32), None);
        }

        let created = errors::catch(|| services.instantiation_service.invoke_function(|accessor| (contribution.ctor)(accessor)));
        match created {
            Ok(instance) => {
                if let Some(id) = &contribution.id {
                    self.instances_by_id.borrow_mut().insert(id.clone(), instance.clone());
                    self.contributions_by_id.borrow_mut().remove(id);
                }
                if let Some(disposable) = instance.disposable() {
                    self.instance_disposables.add(disposable);
                }
            }
            Err(error) => services.log_service.error(&format!("Unable to create workbench contribution '{}'. {error}", contribution.name())),
        }

        if let Some(id) = &contribution.id {
            mark(&format!("code/didCreateWorkbenchContribution/{}/{id}", phase as i32), None);
        }

        if contribution.id.is_some() || !services.environment_service.is_built() /* only log out of sources where we have good ctor names */ {
            let time = runtime::date_now().saturating_sub(now);
            let threshold = if phase < LifecyclePhase::Restored { Self::BLOCK_BEFORE_RESTORE_WARN_THRESHOLD } else { Self::BLOCK_AFTER_RESTORE_WARN_THRESHOLD };
            if time > threshold {
                services.log_service.warn(&format!("Creation of workbench contribution '{}' took {time}ms.", contribution.name()));
            }

            if let Some(id) = &contribution.id {
                self.timings_by_phase.borrow_mut().entry(phase).or_default().push((id.clone(), time));
            }
        }
    }
}

impl IDisposable for WorkbenchContributionsRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

thread_local! {
    static WORKBENCH_CONTRIBUTIONS_REGISTRY: Rc<WorkbenchContributionsRegistry> = WorkbenchContributionsRegistry::new();
}

/// `Registry.as<IWorkbenchContributionsRegistry>(Extensions.Workbench)` e `WorkbenchContributionsRegistry.INSTANCE`.
pub fn workbench_contributions_registry() -> Rc<WorkbenchContributionsRegistry> {
    WORKBENCH_CONTRIBUTIONS_REGISTRY.with(Rc::clone)
}

/// `registerWorkbenchContribution2`: register a workbench contribution that will be instantiated based on the
/// `instantiation` property.
pub fn register_workbench_contribution2(id: &str, ctor: WorkbenchContributionCtor, instantiation: impl Into<WorkbenchContributionInstantiation>) {
    workbench_contributions_registry().register_workbench_contribution2(Some(id), ctor, instantiation);
}

/// `getWorkbenchContribution`: provides access to a workbench contribution with a specific identifier. The contribution is
/// created if not yet done. Fails if called too early before the registry has started or if no contribution is known for
/// the given identifier.
pub fn get_workbench_contribution(id: &str) -> Result<Rc<dyn IWorkbenchContribution>, Error> {
    workbench_contributions_registry().get_workbench_contribution(id)
}
