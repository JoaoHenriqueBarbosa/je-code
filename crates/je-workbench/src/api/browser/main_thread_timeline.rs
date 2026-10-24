//! Porte de `src/vs/workbench/api/browser/mainThreadTimeline.ts`: o lado do workbench do RPC `ExtHostTimeline`, que
//! registra no [`ITimelineService`] os provedores das extensões e repassa os eventos de mudança.
//!
//! Divergências:
//! - **Injeção.** O construtor `(context, @ILogService, @ITimelineService)` é [`MainThreadTimeline::new`];
//!   `@extHostNamedCustomer(MainContext.MainThreadTimeline)` é o registro do `extHostCustomers.ts`, que não foi portado.
//! - **`extends Disposable`** é o [`Disposable`] de `je-base` (campo `base`), que descarta os dois `DisposableMap` de
//!   emissores.
//! - **`revive<Timeline>(...)`** é a identidade: o proxy já devolve os URIs revividos (ver o protocolo).
//! - O provedor registrado no serviço é um struct próprio ([`MainThreadTimelineProvider`], [`MainThreadWorkspaceTimelineProvider`])
//!   no lugar do literal `{ ...provider, onDidChange, provideTimeline, dispose }`.
//! - **`_providerEmitters`** (campo privado lido pelo teste com `Reflect.get`) tem o acessor
//!   [`MainThreadTimeline::provider_emitters_for_testing`].

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::log::{LogServiceRef, Logger};

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostTimelineShape, IMainContext, MainThreadTimelineShape};
use crate::contrib::timeline::common::timeline::{
    ITimelineService, Timeline, TimelineOptions, TimelineProvider, TimelineProviderDescriptor, TimelineScheme, WorkspaceTimelineProvider,
    WorkspaceTimelineProviderDescriptor,
};

/// Os emissores dos provedores de arquivos e pastas, por id.
type ProviderEmitters = DisposableMap<String, Emitter<Option<Vec<URI>>>>;

/// Os emissores dos provedores do workspace, por id.
type WorkspaceProviderEmitters = DisposableMap<String, Emitter<()>>;

/// O provedor que o `$registerTimelineProvider` registra no serviço.
struct MainThreadTimelineProvider {
    descriptor: TimelineProviderDescriptor,
    on_did_change: Event<Option<Vec<URI>>>,
    proxy: Rc<dyn ExtHostTimelineShape>,
    emitters: Rc<ProviderEmitters>,
}

impl IDisposable for MainThreadTimelineProvider {
    fn dispose(&self) {
        self.emitters.delete_and_dispose(&self.descriptor.id);
    }
}

impl TimelineProvider for MainThreadTimelineProvider {
    fn id(&self) -> String {
        self.descriptor.id.clone()
    }

    fn label(&self) -> String {
        self.descriptor.label.clone()
    }

    fn scheme(&self) -> TimelineScheme {
        self.descriptor.scheme.clone()
    }

    fn on_did_change(&self) -> Option<Event<Option<Vec<URI>>>> {
        Some(self.on_did_change.clone())
    }

    fn provide_timeline(&self, uri: &URI, options: &TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>> {
        self.proxy.rpc_get_timeline(&self.descriptor.id, Some(uri.clone()), options.clone(), token)
    }
}

/// O provedor que o `$registerWorkspaceTimelineProvider` registra no serviço.
struct MainThreadWorkspaceTimelineProvider {
    descriptor: WorkspaceTimelineProviderDescriptor,
    on_did_change: Event<()>,
    proxy: Rc<dyn ExtHostTimelineShape>,
    emitters: Rc<WorkspaceProviderEmitters>,
}

impl IDisposable for MainThreadWorkspaceTimelineProvider {
    fn dispose(&self) {
        self.emitters.delete_and_dispose(&self.descriptor.id);
    }
}

impl WorkspaceTimelineProvider for MainThreadWorkspaceTimelineProvider {
    fn id(&self) -> String {
        self.descriptor.id.clone()
    }

    fn label(&self) -> String {
        self.descriptor.label.clone()
    }

    fn on_did_change(&self) -> Option<Event<()>> {
        Some(self.on_did_change.clone())
    }

    fn provide_workspace_timeline(&self, options: &TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>> {
        self.proxy.rpc_get_timeline(&self.descriptor.id, None, options.clone(), token)
    }
}

/// `MainThreadTimeline`.
pub struct MainThreadTimeline {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostTimelineShape>,
    provider_emitters: Rc<ProviderEmitters>,
    workspace_provider_emitters: Rc<WorkspaceProviderEmitters>,
    log_service: LogServiceRef,
    timeline_service: Rc<dyn ITimelineService>,
}

impl MainThreadTimeline {
    pub fn new(context: &dyn IMainContext, log_service: LogServiceRef, timeline_service: Rc<dyn ITimelineService>) -> Rc<Self> {
        let base = Disposable::new();
        let provider_emitters = ProviderEmitters::new();
        base.register(provider_emitters.clone());
        let workspace_provider_emitters = WorkspaceProviderEmitters::new();
        base.register(workspace_provider_emitters.clone());
        Rc::new(Self {
            base,
            proxy: context.get_proxy(&ExtHostContext::EXT_HOST_TIMELINE),
            provider_emitters,
            workspace_provider_emitters,
            log_service,
            timeline_service,
        })
    }

    /// O `_providerEmitters` privado, que o teste lê com `Reflect.get(customer, '_providerEmitters')`.
    pub fn provider_emitters_for_testing(&self) -> Rc<ProviderEmitters> {
        self.provider_emitters.clone()
    }
}

impl IDisposable for MainThreadTimeline {
    fn dispose(&self) {
        for id in self.provider_emitters.keys() {
            self.rpc_unregister_timeline_provider(&id);
        }
        for id in self.workspace_provider_emitters.keys() {
            self.rpc_unregister_workspace_timeline_provider(&id);
        }
        self.base.dispose();
    }
}

impl MainThreadTimelineShape for MainThreadTimeline {
    fn rpc_register_timeline_provider(&self, provider: TimelineProviderDescriptor) {
        self.log_service.trace(&format!("MainThreadTimeline#registerTimelineProvider: id={}", provider.id));

        let emitters = self.provider_emitters.clone();
        let on_did_change = match emitters.get(&provider.id) {
            Some(emitter) => emitter,
            None => {
                let emitter = Emitter::new();
                emitters.set(provider.id.clone(), emitter.clone(), false);
                emitter
            }
        };

        self.timeline_service.register_timeline_provider(Rc::new(MainThreadTimelineProvider {
            descriptor: provider,
            on_did_change: on_did_change.event(),
            proxy: self.proxy.clone(),
            emitters,
        }));
    }

    fn rpc_unregister_timeline_provider(&self, id: &str) {
        self.log_service.trace(&format!("MainThreadTimeline#unregisterTimelineProvider: id={id}"));

        self.timeline_service.unregister_timeline_provider(id);
        self.provider_emitters.delete_and_dispose(&id.to_owned());
    }

    fn rpc_register_workspace_timeline_provider(&self, provider: WorkspaceTimelineProviderDescriptor) {
        self.log_service.trace(&format!("MainThreadTimeline#registerWorkspaceTimelineProvider: id={}", provider.id));

        let emitters = self.workspace_provider_emitters.clone();
        let on_did_change = match emitters.get(&provider.id) {
            Some(emitter) => emitter,
            None => {
                let emitter = Emitter::new();
                emitters.set(provider.id.clone(), emitter.clone(), false);
                emitter
            }
        };

        self.timeline_service.register_workspace_timeline_provider(Rc::new(MainThreadWorkspaceTimelineProvider {
            descriptor: provider,
            on_did_change: on_did_change.event(),
            proxy: self.proxy.clone(),
            emitters,
        }));
    }

    fn rpc_unregister_workspace_timeline_provider(&self, id: &str) {
        self.log_service.trace(&format!("MainThreadTimeline#unregisterWorkspaceTimelineProvider: id={id}"));

        self.timeline_service.unregister_workspace_timeline_provider(id);
        self.workspace_provider_emitters.delete_and_dispose(&id.to_owned());
    }

    fn rpc_emit_timeline_change_event(&self, id: &str, uris: Option<Vec<URI>>) {
        let described = uris.as_ref().map(|uris| uris.iter().map(|uri| uri.to_string(true).to_string_lossy()).collect::<Vec<_>>().join(", "));
        self.log_service.trace(&format!("MainThreadTimeline#emitChangeEvent: id={id}, uris={}", described.as_deref().unwrap_or("undefined")));

        let emitter = self.provider_emitters.get(&id.to_owned());
        if let Some(emitter) = emitter {
            emitter.fire(uris);
        }
    }

    fn rpc_emit_workspace_timeline_change_event(&self, id: &str) {
        self.log_service.trace(&format!("MainThreadTimeline#emitWorkspaceChangeEvent: id={id}"));

        let emitter = self.workspace_provider_emitters.get(&id.to_owned());
        if let Some(emitter) = emitter {
            emitter.fire(());
        }
    }
}
