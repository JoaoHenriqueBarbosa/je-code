//! Porte de `src/vs/workbench/contrib/output/browser/outputLinkProvider.ts`: o `OutputLinkProvider`, que registra no
//! `ILanguageFeaturesService` o provedor de links dos modelos de saída (`Log`) e de log (`log`) enquanto o workspace tiver
//! pastas, e o `OutputLinkWorkerClient`, que calcula os links.
//!
//! Notas de tradução:
//! - **`IWebWorkerService` e o `WorkerTextModelSyncClient` não existem no porte** (o `WebWorkerDescriptor` aponta para o
//!   módulo `outputLinkComputerMain.js`, que o porte não carrega). O cálculo de links é uma função pura do texto do modelo e das
//!   pastas do workspace ([`OutputLinkComputer`], de `common/output_link_computer.rs`), então o "worker" é o próprio
//!   [`OutputLinkComputer`] no mesmo thread: `$setWorkspaceFolders` é [`OutputLinkComputer::set_workspace_folders`] e o
//!   `$computeLinks(uri)` lê o texto do modelo no `IModelService` (o que o `ensureSyncedResources` garante no original: um
//!   modelo que não está lá devolve nenhum link, como o `if (!model) return []` do `OutputLinkComputer`). O `dispose()` do
//!   cliente descarta o computador; o agendador de 3 minutos de inatividade continua descartando o "worker".
//! - **`ITextModel`** é o [`TextModel`] concreto; `IDisposable | undefined` é `Option<DisposableRef>`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::language_features_service::{LanguageFeaturesService, LinkProvider, LinksList};
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::languages::ILink;
use je_editor::language_providers::ProviderResult;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_platform::workspace::IWorkspaceContextService;

use crate::contrib::output::common::output_link_computer::OutputLinkComputer;
use crate::services::output::output::{LOG_MODE_ID, OUTPUT_MODE_ID};

/// `OutputLinkProvider.DISPOSE_WORKER_TIME`: dispose worker after 3 minutes of inactivity.
const DISPOSE_WORKER_TIME: u64 = 3 * 60 * 1000;

/// `OutputLinkProvider`.
pub struct OutputLinkProvider {
    me: Weak<OutputLinkProvider>,
    context_service: Rc<dyn IWorkspaceContextService>,
    model_service: Rc<dyn IModelService>,
    language_features_service: Rc<LanguageFeaturesService>,
    store: Rc<DisposableStore>,
    worker: RefCell<Option<Rc<OutputLinkWorkerClient>>>,
    dispose_worker_scheduler: Rc<RunOnceScheduler>,
    link_provider_registration: RefCell<Option<DisposableRef>>,
}

impl OutputLinkProvider {
    pub fn new(
        context_service: Rc<dyn IWorkspaceContextService>,
        model_service: Rc<dyn IModelService>,
        language_features_service: Rc<LanguageFeaturesService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<OutputLinkProvider>| {
            let weak = me.clone();
            Self {
                me: me.clone(),
                context_service,
                model_service,
                language_features_service,
                store: DisposableStore::new(),
                worker: RefCell::new(None),
                dispose_worker_scheduler: RunOnceScheduler::new(
                    move || {
                        if let Some(this) = weak.upgrade() {
                            this.dispose_worker();
                        }
                    },
                    DISPOSE_WORKER_TIME,
                ),
                link_provider_registration: RefCell::new(None),
            }
        });
        this.store.add(this.dispose_worker_scheduler.clone());

        this.register_listeners();
        this.update_link_provider_worker();
        this
    }

    fn register_listeners(&self) {
        let weak = self.me.clone();
        self.store.add(self.context_service.on_did_change_workspace_folders().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_link_provider_worker();
            }
        }));
    }

    fn update_link_provider_worker(&self) {
        // Setup link provider depending on folders being opened or not
        let folders = self.context_service.get_workspace().folders();
        if !folders.is_empty() {
            if self.link_provider_registration.borrow().is_none() {
                let selector = LanguageSelector::array([
                    LanguageSelector::Filter(LanguageFilter { language: Some(OUTPUT_MODE_ID.to_owned()), scheme: Some("*".to_owned()), ..Default::default() }),
                    LanguageSelector::Filter(LanguageFilter { language: Some(LOG_MODE_ID.to_owned()), scheme: Some("*".to_owned()), ..Default::default() }),
                ]);
                let registration = self.language_features_service.link_provider.register(selector, Rc::new(OutputLinks { provider: self.me.clone() }));
                *self.link_provider_registration.borrow_mut() = Some(registration);
            }
        } else if let Some(registration) = self.link_provider_registration.borrow_mut().take() {
            registration.dispose();
        }

        // Dispose worker to recreate with folders on next provideLinks request
        self.dispose_worker();
        self.dispose_worker_scheduler.cancel();
    }

    fn get_or_create_worker(&self) -> Rc<OutputLinkWorkerClient> {
        self.dispose_worker_scheduler.schedule(None);

        self.worker.borrow_mut().get_or_insert_with(|| OutputLinkWorkerClient::new(self.context_service.clone(), self.model_service.clone())).clone()
    }

    fn provide_links(&self, model_uri: &URI) -> Promise<Vec<ILink>> {
        self.get_or_create_worker().provide_links(model_uri)
    }

    fn dispose_worker(&self) {
        if let Some(worker) = self.worker.borrow_mut().take() {
            worker.dispose();
        }
    }
}

impl IDisposable for OutputLinkProvider {
    fn dispose(&self) {
        self.dispose_worker();
        if let Some(registration) = self.link_provider_registration.borrow_mut().take() {
            registration.dispose();
        }
        self.store.dispose();
    }
}

/// O objeto `{ provideLinks: async model => ... }` que o `OutputLinkProvider` registra.
struct OutputLinks {
    provider: Weak<OutputLinkProvider>,
}

impl LinkProvider for OutputLinks {
    fn provide_links(&self, model: &Rc<TextModel>, _token: &CancellationToken) -> ProviderResult<Rc<LinksList>> {
        let Some(provider) = self.provider.upgrade() else {
            return Promise::resolved(None);
        };
        // `links && { links }`: a lista de links sempre existe (vazia, se não há nenhum).
        provider.provide_links(&model.uri()).then(|links| Some(Rc::new(LinksList::new(links, || { /* sem recurso a liberar */ }))))
    }
}

/// `OutputLinkWorkerClient`.
struct OutputLinkWorkerClient {
    worker_client: RefCell<OutputLinkComputer>,
    model_service: Rc<dyn IModelService>,
}

impl OutputLinkWorkerClient {
    fn new(context_service: Rc<dyn IWorkspaceContextService>, model_service: Rc<dyn IModelService>) -> Rc<Self> {
        let client = Rc::new(Self { worker_client: RefCell::new(OutputLinkComputer::new()), model_service });
        // `_ensureWorkspaceFolders()`
        client.worker_client.borrow_mut().set_workspace_folders(context_service.get_workspace().folders().iter().map(|folder| folder.uri.to_string(false).to_string_lossy()).collect());
        client
    }

    fn provide_links(&self, model_uri: &URI) -> Promise<Vec<ILink>> {
        // `$computeLinks(uri)`: um modelo que o worker não tem não produz links.
        let Some(model) = self.model_service.get_model(model_uri) else {
            return Promise::resolved(Vec::new());
        };
        Promise::resolved(self.worker_client.borrow().compute_links(&model.get_value()))
    }

    fn dispose(&self) {
        // O computador é descartado com o cliente (`this._workerClient` e o sincronizador são `_register`ados).
        *self.worker_client.borrow_mut() = OutputLinkComputer::new();
    }
}
