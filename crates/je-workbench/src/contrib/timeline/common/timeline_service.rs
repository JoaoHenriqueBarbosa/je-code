//! Porte de `src/vs/workbench/contrib/timeline/common/timelineService.ts`: o [`TimelineService`], que guarda os
//! provedores de timeline de arquivos e do workspace, repassa os eventos de mudança e entrega as timelines
//! ordenadas.
//!
//! Divergências:
//! - Injeção: `(@ILogService, @IViewsService, @IConfigurationService, @IContextKeyService)` é [`TimelineService::new`].
//!   `viewsService`, `configurationService` e `contextKeyService` são `protected` no original (as subclasses os
//!   usam) e ficam guardados aqui.
//! - `extends Disposable` é o [`Disposable`] de `je-base` (campo `base`): os emissores e os mapas de inscrições são
//!   registrados nele e descartados por [`IDisposable::dispose`].
//! - `Map<string, Provider>` é [`ProviderMap`], que guarda a ordem de inserção (a de `getSources`); trocar o valor de
//!   uma chave existente mantém a posição, como o `Map`.
//! - `a.source.localeCompare(b.source, undefined, { numeric: true, sensitivity: 'base' })` é o [`Collator`] com as
//!   mesmas opções.
//! - `registerTimelineProvider` devolve um `{ dispose }` sem rastreio (o literal do original): [`plain_disposable`].
//! - `TimelineHasProviderContext` e `TimelineHasWorkspaceProviderContext` são as funções
//!   [`timeline_has_provider_context`] e [`timeline_has_workspace_provider_context`].

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::collator::{Collator, CollatorOptions, Sensitivity};
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{plain_disposable, Disposable, DisposableMap, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::log::{LogServiceRef, Logger};

use super::timeline::{
    same_provider, ITimelineService, Timeline, TimelineChangeEvent, TimelineEditable, TimelineItemIdentifier, TimelineOptions, TimelineProvider,
    TimelineProvidersChangeEvent, TimelineRequest, TimelineScheme, TimelineSource, WorkspaceTimelineProvider, TIMELINE_PANE_ID,
};
use crate::contrib::files::browser_files::EditableData;
use crate::services::views::views_service::IViewsService;

thread_local! {
    static HAS_PROVIDER_CONTEXT: RawContextKey = RawContextKey::new("timelineHasProvider", false, ContextKeyMeta::None);
    static HAS_WORKSPACE_PROVIDER_CONTEXT: RawContextKey = RawContextKey::new("timelineHasWorkspaceProvider", false, ContextKeyMeta::None);
}

/// `TimelineHasProviderContext`.
pub fn timeline_has_provider_context() -> RawContextKey {
    HAS_PROVIDER_CONTEXT.with(Clone::clone)
}

/// `TimelineHasWorkspaceProviderContext`.
pub fn timeline_has_workspace_provider_context() -> RawContextKey {
    HAS_WORKSPACE_PROVIDER_CONTEXT.with(Clone::clone)
}

/// Um `Map<string, V>` com a ordem de inserção.
struct ProviderMap<V: Clone> {
    entries: RefCell<Vec<(String, V)>>,
}

impl<V: Clone> ProviderMap<V> {
    fn new() -> Self {
        Self { entries: RefCell::new(Vec::new()) }
    }

    fn get(&self, id: &str) -> Option<V> {
        self.entries.borrow().iter().find(|(key, _)| key == id).map(|(_, value)| value.clone())
    }

    fn has(&self, id: &str) -> bool {
        self.entries.borrow().iter().any(|(key, _)| key == id)
    }

    fn set(&self, id: &str, value: V) {
        let mut entries = self.entries.borrow_mut();
        match entries.iter_mut().find(|(key, _)| key == id) {
            Some(slot) => slot.1 = value,
            None => entries.push((id.to_owned(), value)),
        }
    }

    fn delete(&self, id: &str) {
        self.entries.borrow_mut().retain(|(key, _)| key != id);
    }

    fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    fn values(&self) -> Vec<V> {
        self.entries.borrow().iter().map(|(_, value)| value.clone()).collect()
    }
}

/// `TimelineService`.
pub struct TimelineService {
    this: Weak<TimelineService>,
    base: Rc<Disposable>,

    log_service: LogServiceRef,
    pub views_service: Rc<dyn IViewsService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub context_key_service: Rc<dyn IContextKeyService>,

    on_did_change_providers: Rc<Emitter<TimelineProvidersChangeEvent>>,
    on_did_change_timeline: Rc<Emitter<TimelineChangeEvent>>,
    on_did_change_uri: Rc<Emitter<Option<URI>>>,
    on_did_change_editable: Rc<Emitter<()>>,

    editable: RefCell<Option<TimelineEditable>>,

    has_provider_context: Rc<dyn IContextKey>,
    has_workspace_provider_context: Rc<dyn IContextKey>,
    providers: ProviderMap<Rc<dyn TimelineProvider>>,
    provider_subscriptions: Rc<DisposableMap<String, dyn IDisposable>>,
    workspace_providers: ProviderMap<Rc<dyn WorkspaceTimelineProvider>>,
    workspace_provider_subscriptions: Rc<DisposableMap<String, dyn IDisposable>>,
}

impl TimelineService {
    pub fn new(
        log_service: LogServiceRef,
        views_service: Rc<dyn IViewsService>,
        configuration_service: Rc<dyn IConfigurationService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_providers = Emitter::new();
        base.register(on_did_change_providers.clone());
        let on_did_change_timeline = Emitter::new();
        base.register(on_did_change_timeline.clone());
        let on_did_change_uri = Emitter::new();
        base.register(on_did_change_uri.clone());
        let on_did_change_editable = Emitter::new();
        base.register(on_did_change_editable.clone());
        let provider_subscriptions = DisposableMap::<String, dyn IDisposable>::new();
        base.register(provider_subscriptions.clone());
        let workspace_provider_subscriptions = DisposableMap::<String, dyn IDisposable>::new();
        base.register(workspace_provider_subscriptions.clone());

        let has_provider_context = timeline_has_provider_context().bind_to(&*context_key_service);
        let has_workspace_provider_context = timeline_has_workspace_provider_context().bind_to(&*context_key_service);

        let service = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            log_service,
            views_service,
            configuration_service,
            context_key_service,
            on_did_change_providers,
            on_did_change_timeline,
            on_did_change_uri,
            on_did_change_editable,
            editable: RefCell::new(None),
            has_provider_context,
            has_workspace_provider_context,
            providers: ProviderMap::new(),
            provider_subscriptions,
            workspace_providers: ProviderMap::new(),
            workspace_provider_subscriptions,
        });
        service.update_has_provider_context();
        service
    }

    fn fire_provider_removed(&self, id: &str) {
        // A file and a workspace timeline provider can share an id, so when one of them remains, its source is reloaded
        if self.providers.has(id) || self.workspace_providers.has(id) {
            self.on_did_change_providers.fire(TimelineProvidersChangeEvent { removed: Some(vec![id.to_owned()]), added: Some(vec![id.to_owned()]) });
        } else {
            self.on_did_change_providers.fire(TimelineProvidersChangeEvent { removed: Some(vec![id.to_owned()]), added: None });
        }
    }

    fn update_has_provider_context(&self) {
        self.has_provider_context.set(Dynamic::Bool(self.providers.len() != 0 || self.workspace_providers.len() != 0));
        self.has_workspace_provider_context.set(Dynamic::Bool(self.workspace_providers.len() != 0));
    }
}

impl IDisposable for TimelineService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `provider.scheme` aceita o `uri`?
fn scheme_matches(scheme: &TimelineScheme, uri: &URI) -> bool {
    let uri_scheme = uri.scheme().to_string_lossy();
    match scheme {
        TimelineScheme::One(scheme) => scheme == "*" || *scheme == uri_scheme,
        TimelineScheme::Many(schemes) => schemes.contains(&uri_scheme),
    }
}

/// O `result.then(...)` de `getTimeline`: troca o `source` dos itens e os ordena pelo mais recente, desempatando pelo
/// `source` (numérico, sem diferenciar acento nem caixa).
fn order_timeline(id: &str, timeline: Option<Timeline>) -> Option<Timeline> {
    let mut timeline = timeline?;
    for item in &mut timeline.items {
        item.source = id.to_owned();
    }
    let collator = Collator::new(None, CollatorOptions { numeric: true, sensitivity: Sensitivity::Base, ..CollatorOptions::default() });
    timeline.items.sort_by(|a, b| {
        let difference = b.timestamp - a.timestamp;
        if difference != 0.0 && !difference.is_nan() {
            return if difference < 0.0 { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        let (a_source, b_source): (Vec<u16>, Vec<u16>) = (a.source.encode_utf16().collect(), b.source.encode_utf16().collect());
        collator.compare(&b_source, &a_source).cmp(&0)
    });
    Some(timeline)
}

impl ITimelineService for TimelineService {
    fn on_did_change_providers(&self) -> Event<TimelineProvidersChangeEvent> {
        self.on_did_change_providers.event()
    }

    fn on_did_change_timeline(&self) -> Event<TimelineChangeEvent> {
        self.on_did_change_timeline.event()
    }

    fn on_did_change_uri(&self) -> Event<Option<URI>> {
        self.on_did_change_uri.event()
    }

    fn on_did_change_editable(&self) -> Event<()> {
        self.on_did_change_editable.event()
    }

    fn editable(&self) -> Option<TimelineEditable> {
        self.editable.borrow().clone()
    }

    fn get_sources(&self) -> Vec<TimelineSource> {
        let mut sources: Vec<TimelineSource> = Vec::new();
        let candidates = self
            .providers
            .values()
            .into_iter()
            .map(|provider| (provider.id(), provider.label()))
            .chain(self.workspace_providers.values().into_iter().map(|provider| (provider.id(), provider.label())));
        for (id, label) in candidates {
            if !sources.iter().any(|source| source.id == id) {
                sources.push(TimelineSource { id, label });
            }
        }
        sources
    }

    fn has_workspace_providers(&self) -> bool {
        self.workspace_providers.len() != 0
    }

    fn get_timeline(&self, id: &str, uri: Option<&URI>, options: &TimelineOptions, token_source: CancellationTokenSource) -> Option<TimelineRequest> {
        let described = uri.map_or_else(|| "workspace".to_owned(), |uri| uri.to_string(false).to_string_lossy());
        self.log_service.trace(&format!("TimelineService#getTimeline({id}): uri={described}"));

        let result = match uri {
            None => {
                let provider = self.workspace_providers.get(id)?;
                provider.provide_workspace_timeline(options, token_source.token())
            }
            Some(uri) => {
                let provider = self.providers.get(id)?;
                if !scheme_matches(&provider.scheme(), uri) {
                    return None;
                }
                provider.provide_timeline(uri, options, token_source.token())
            }
        };

        let source = id.to_owned();
        Some(TimelineRequest {
            result: result.then({
                let source = source.clone();
                move |timeline| order_timeline(&source, timeline)
            }),
            options: options.clone(),
            source,
            token_source,
            uri: uri.cloned(),
        })
    }

    fn register_timeline_provider(&self, provider: Rc<dyn TimelineProvider>) -> DisposableRef {
        let id = provider.id();
        self.log_service.trace(&format!("TimelineService#registerTimelineProvider: id={id}"));

        let existing = self.providers.get(&id);
        if let Some(existing) = existing {
            // For now to deal with https://github.com/microsoft/vscode/issues/89553 allow any overwritting here (still will be blocked in the Extension Host)
            // TODO@eamodio: Ultimately will need to figure out a way to unregister providers when the Extension Host restarts/crashes
            // throw new Error(`Timeline Provider ${id} already exists.`);
            let _ = errors::catch(|| existing.dispose());
        }

        self.providers.set(&id, provider.clone());

        self.update_has_provider_context();

        if let Some(on_did_change) = provider.on_did_change() {
            let emitter = self.on_did_change_timeline.clone();
            let source = id.clone();
            let subscription = on_did_change.listen(move |uris| emitter.fire(TimelineChangeEvent { id: source.clone(), workspace: false, uris: uris.clone() }));
            self.provider_subscriptions.set(id.clone(), subscription, false);
        }
        self.on_did_change_providers.fire(TimelineProvidersChangeEvent { added: Some(vec![id.clone()]), removed: None });

        let this = self.this.clone();
        plain_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            let current = this.providers.get(&id);
            if current.is_some_and(|current| same_provider(&current, &provider)) {
                this.unregister_timeline_provider(&id);
            }
        })
    }

    fn unregister_timeline_provider(&self, id: &str) {
        self.log_service.trace(&format!("TimelineService#unregisterTimelineProvider: id={id}"));

        if !self.providers.has(id) {
            return;
        }

        self.providers.delete(id);
        self.provider_subscriptions.delete_and_dispose(&id.to_owned());

        self.update_has_provider_context();

        self.fire_provider_removed(id);
    }

    fn register_workspace_timeline_provider(&self, provider: Rc<dyn WorkspaceTimelineProvider>) -> DisposableRef {
        let id = provider.id();
        self.log_service.trace(&format!("TimelineService#registerWorkspaceTimelineProvider: id={id}"));

        // Like file and folder timeline providers, replace any existing provider (still will be blocked in the Extension Host)
        let existing = self.workspace_providers.get(&id);
        if let Some(existing) = existing {
            let _ = errors::catch(|| existing.dispose());
        }

        self.workspace_providers.set(&id, provider.clone());

        self.update_has_provider_context();

        if let Some(on_did_change) = provider.on_did_change() {
            let emitter = self.on_did_change_timeline.clone();
            let source = id.clone();
            let subscription = on_did_change.listen(move |_| emitter.fire(TimelineChangeEvent { id: source.clone(), workspace: true, uris: None }));
            self.workspace_provider_subscriptions.set(id.clone(), subscription, false);
        }
        self.on_did_change_providers.fire(TimelineProvidersChangeEvent { added: Some(vec![id.clone()]), removed: None });

        let this = self.this.clone();
        plain_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            let current = this.workspace_providers.get(&id);
            if current.is_some_and(|current| same_provider(&current, &provider)) {
                this.unregister_workspace_timeline_provider(&id);
            }
        })
    }

    fn unregister_workspace_timeline_provider(&self, id: &str) {
        self.log_service.trace(&format!("TimelineService#unregisterWorkspaceTimelineProvider: id={id}"));

        if !self.workspace_providers.has(id) {
            return;
        }

        self.workspace_providers.delete(id);
        self.workspace_provider_subscriptions.delete_and_dispose(&id.to_owned());

        self.update_has_provider_context();

        self.fire_provider_removed(id);
    }

    fn set_uri(&self, uri: Option<URI>) -> Promise<()> {
        let views_service = self.views_service.clone();
        let emitter = self.on_did_change_uri.clone();
        async_fn(async move {
            // Wait for the view to be created and visible, otherwise it would miss the new uri or skip loading it
            views_service.open_view(TIMELINE_PANE_ID, true).await?;
            emitter.fire(uri);
            Ok(())
        })
    }

    fn set_editable(&self, item: TimelineItemIdentifier, data: Option<EditableData>) {
        *self.editable.borrow_mut() = data.map(|data| TimelineEditable { item, data });
        self.on_did_change_editable.fire(());
    }
}
