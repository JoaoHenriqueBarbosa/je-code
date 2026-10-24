//! Porte de `src/vs/workbench/contrib/tasks/common/problemCollectors.ts`: os coletores de problemas que leem a saída de uma
//! tarefa linha a linha, casam com os `ProblemMatcher`s e entregam os marcadores ao `IMarkerService`
//! (`AbstractProblemCollector`, `StartStopProblemCollector` e `WatchingProblemCollector`).
//!
//! Notas de tradução:
//! - **Herança.** `AbstractProblemCollector` é a struct [`AbstractProblemCollector`] (compartilhada em `Rc`); os dois coletores
//!   concretos guardam essa base no campo `base` e fornecem o `processLineInternal`. `processLine` encadeia as linhas na cauda
//!   (`tail`) com [`AbstractProblemCollector::enqueue`].
//! - **Casadores de linha.** `matchers` (`INumberDictionary<ILineMatcher[]>`) é um mapa por tamanho do casamento; o
//!   `activeMatcher` é o par (tamanho, posição) do casador no mapa, porque `handle` e `next` mudam o casador.
//! - **Mapas.** Os `Map` aninhados do original (`resourcesToClean`, `markers`, `deliveredMarkers`, `applyToByOwner`) são
//!   `OrderedMap`, que mantém a ordem de inserção; `IStringDictionary<boolean>` de `openModels` é um conjunto de textos.
//! - **`async`.** `processLineInternal`, `shouldApplyMatch` e `tryBegin` são [`async_fn`]; um erro numa `await` rejeita a
//!   promessa, como o `throw` do original.
//! - **`IProblemCollectorEvent`.** `capturedVariables` (`ReadonlyMap<string, string>` feito de `Object.entries(matches.groups)`)
//!   é `Vec<(String, Option<String>)>` com os grupos nomeados na ordem do padrão (o grupo que não participou é `None`, o
//!   `undefined` do original).
//! - **`IModelService.onModelRemoved`.** O contrato do editor ganhou o evento (padrão: nunca dispara).
//! - **Ouvintes de modelo.** `onModelAdded` e `onModelRemoved` ficam no `modelListeners` e saem do `dispose`. O ouvinte
//!   temporário de `onMarkerChanged` (debounce de 500 ms com `flushOnListenerRemove`) é descartado depois de 600 ms.
//! - **`Date.now()`** é `runtime::date_now`; o registro lento da regexp usa o `source` do padrão.
//! - **`getResource(file, matcher)`** é chamado sem `IFileService`, como o original.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::event::{DebounceOptions, Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::symbols::Delay;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::services_model::IModelService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogService;
use je_platform::markers::markers::{IMarkerService, Marker as IMarker, MarkerData, MarkerReadOptions, MarkerSeverity};

use super::problem_matcher::{create_line_matcher, get_resource, ApplyToKind, ILineMatcher, ProblemMatch, ProblemMatcher, WatchingPattern};

/// `ProblemCollectorEventKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemCollectorEventKind {
    BackgroundProcessingBegins,
    BackgroundProcessingEnds,
}

impl ProblemCollectorEventKind {
    /// O texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            ProblemCollectorEventKind::BackgroundProcessingBegins => "backgroundProcessingBegins",
            ProblemCollectorEventKind::BackgroundProcessingEnds => "backgroundProcessingEnds",
        }
    }
}

/// `IProblemCollectorEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct IProblemCollectorEvent {
    pub kind: ProblemCollectorEventKind,
    pub captured_variables: Option<Vec<(String, Option<String>)>>,
}

impl IProblemCollectorEvent {
    /// `IProblemCollectorEvent.create`.
    pub fn create(kind: ProblemCollectorEventKind, captured_variables: Option<Vec<(String, Option<String>)>>) -> Self {
        Self { kind, captured_variables }
    }
}

/// `IProblemMatcher`.
pub trait IProblemMatcher {
    fn process_line(&self, line: &str);
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn merge_apply_to(current: ApplyToKind, value: ApplyToKind) -> ApplyToKind {
    if current == value || current == ApplyToKind::AllDocuments {
        return current;
    }
    ApplyToKind::AllDocuments
}

/// `AbstractProblemCollector`.
pub struct AbstractProblemCollector {
    this: Weak<AbstractProblemCollector>,
    base: Rc<Disposable>,

    pub problem_matchers: Vec<ProblemMatcher>,
    marker_service: Rc<dyn IMarkerService>,
    log_service: Option<Rc<dyn LogService>>,

    matchers: RefCell<HashMap<usize, Vec<Box<dyn ILineMatcher>>>>,
    active_matcher: Cell<Option<(usize, usize)>>,
    number_of_matches: Cell<usize>,
    max_marker_severity: Cell<Option<MarkerSeverity>>,
    buffer: RefCell<Vec<String>>,
    buffer_length: Cell<usize>,
    open_models: RefCell<HashSet<String>>,
    model_listeners: Rc<DisposableStore>,
    tail: RefCell<Option<Promise<()>>>,

    // [owner] -> ApplyToKind
    apply_to_by_owner: RefCell<OrderedMap<String, ApplyToKind>>,
    // [owner] -> [resource] -> URI
    resources_to_clean: RefCell<OrderedMap<String, OrderedMap<String, URI>>>,
    // [owner] -> [resource] -> [markerkey] -> markerData
    markers: RefCell<OrderedMap<String, OrderedMap<String, OrderedMap<String, MarkerData>>>>,
    // [owner] -> [resource] -> number;
    delivered_markers: RefCell<OrderedMap<String, OrderedMap<String, usize>>>,

    on_did_state_change: Rc<Emitter<IProblemCollectorEvent>>,
    on_did_find_first_match: Rc<Emitter<()>>,
    on_did_find_errors: Rc<Emitter<Vec<IMarker>>>,
    on_did_request_invalidate_last_marker: Rc<Emitter<()>>,
}

impl AbstractProblemCollector {
    pub fn new(
        problem_matchers: Vec<ProblemMatcher>,
        marker_service: Rc<dyn IMarkerService>,
        model_service: Rc<dyn IModelService>,
        file_service: Option<Rc<dyn IFileService>>,
        log_service: Option<Rc<dyn LogService>>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_find_first_match = Emitter::new();
        base.register(on_did_find_first_match.clone());
        let on_did_find_errors = Emitter::new();
        base.register(on_did_find_errors.clone());
        let on_did_request_invalidate_last_marker = Emitter::new();
        base.register(on_did_request_invalidate_last_marker.clone());
        let on_did_state_change = Emitter::new();
        base.register(on_did_state_change.clone());

        let mut buffer_length = 1;
        let mut matchers: HashMap<usize, Vec<Box<dyn ILineMatcher>>> = HashMap::new();
        for problem_matcher in &problem_matchers {
            let matcher = create_line_matcher(problem_matcher.clone(), file_service.clone(), log_service.clone());
            let length = matcher.match_length();
            if length > buffer_length {
                buffer_length = length;
            }
            matchers.entry(length).or_default().push(matcher);
        }

        let mut apply_to_by_owner: OrderedMap<String, ApplyToKind> = OrderedMap::new();
        for problem_matcher in &problem_matchers {
            let current = apply_to_by_owner.get(&problem_matcher.owner).copied();
            match current {
                None => {
                    apply_to_by_owner.insert(problem_matcher.owner.clone(), problem_matcher.apply_to);
                }
                Some(current) => {
                    apply_to_by_owner.insert(problem_matcher.owner.clone(), merge_apply_to(current, problem_matcher.apply_to));
                }
            }
        }

        let model_listeners = DisposableStore::new();
        let collector = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            problem_matchers,
            marker_service,
            log_service,
            matchers: RefCell::new(matchers),
            active_matcher: Cell::new(None),
            number_of_matches: Cell::new(0),
            max_marker_severity: Cell::new(None),
            buffer: RefCell::new(Vec::new()),
            buffer_length: Cell::new(buffer_length),
            open_models: RefCell::new(HashSet::new()),
            model_listeners: model_listeners.clone(),
            tail: RefCell::new(None),
            apply_to_by_owner: RefCell::new(apply_to_by_owner),
            resources_to_clean: RefCell::new(OrderedMap::new()),
            markers: RefCell::new(OrderedMap::new()),
            delivered_markers: RefCell::new(OrderedMap::new()),
            on_did_state_change,
            on_did_find_first_match,
            on_did_find_errors,
            on_did_request_invalidate_last_marker,
        });

        let this = collector.this.clone();
        model_service.on_model_added().listen_in(
            move |model| {
                if let Some(this) = this.upgrade() {
                    this.open_models.borrow_mut().insert(model.uri().to_string(false).to_string_lossy());
                }
            },
            &model_listeners,
        );
        let this = collector.this.clone();
        model_service.on_model_removed().listen_in(
            move |model| {
                if let Some(this) = this.upgrade() {
                    this.open_models.borrow_mut().remove(&model.uri().to_string(false).to_string_lossy());
                }
            },
            &model_listeners,
        );
        for model in model_service.get_models() {
            collector.open_models.borrow_mut().insert(model.uri().to_string(false).to_string_lossy());
        }

        collector
    }

    fn self_rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o coletor está vivo enquanto é usado")
    }

    pub fn on_did_state_change(&self) -> Event<IProblemCollectorEvent> {
        self.on_did_state_change.event()
    }

    pub fn on_did_find_first_match(&self) -> Event<()> {
        self.on_did_find_first_match.event()
    }

    pub fn on_did_find_errors(&self) -> Event<Vec<IMarker>> {
        self.on_did_find_errors.event()
    }

    pub fn on_did_request_invalidate_last_marker(&self) -> Event<()> {
        self.on_did_request_invalidate_last_marker.event()
    }

    pub fn fire_state_change(&self, event: IProblemCollectorEvent) {
        self.on_did_state_change.fire(event);
    }

    pub fn fire_first_match(&self) {
        self.on_did_find_first_match.fire(());
    }

    pub fn fire_errors(&self, markers: Vec<IMarker>) {
        self.on_did_find_errors.fire(markers);
    }

    pub fn fire_invalidate_last_marker(&self) {
        self.on_did_request_invalidate_last_marker.fire(());
    }

    pub fn marker_service(&self) -> &Rc<dyn IMarkerService> {
        &self.marker_service
    }

    pub fn log_service(&self) -> Option<&Rc<dyn LogService>> {
        self.log_service.as_ref()
    }

    pub fn apply_to_owners(&self) -> Vec<String> {
        self.apply_to_by_owner.borrow().iter().map(|(owner, _)| owner.clone()).collect()
    }

    /// O encadeamento de `processLine`: a linha nova espera a cauda anterior terminar.
    pub fn enqueue(&self, process: impl FnOnce() -> Promise<()> + 'static) {
        let old_tail = self.tail.borrow().clone();
        let tail = match old_tail {
            Some(old_tail) => old_tail.then_promise(move |_| process()),
            None => process(),
        };
        *self.tail.borrow_mut() = Some(tail);
    }

    pub fn number_of_matches(&self) -> usize {
        self.number_of_matches.get()
    }

    pub fn max_marker_severity(&self) -> Option<MarkerSeverity> {
        self.max_marker_severity.get()
    }

    pub fn try_find_marker(&self, line: &str) -> Option<ProblemMatch> {
        if let Some((length, index)) = self.active_matcher.get() {
            let result = self.matchers.borrow_mut().get_mut(&length).and_then(|candidates| candidates[index].next(line));
            if let Some(result) = result {
                self.capture_match(&result);
                return Some(result);
            }
            self.clear_buffer();
            self.active_matcher.set(None);
        }
        {
            let mut buffer = self.buffer.borrow_mut();
            if buffer.len() < self.buffer_length.get() {
                buffer.push(line.to_string());
            } else {
                let end = buffer.len() - 1;
                for i in 0..end {
                    buffer[i] = buffer[i + 1].clone();
                }
                buffer[end] = line.to_string();
            }
        }

        let result = self.try_matchers();
        if result.is_some() {
            self.clear_buffer();
        }
        result
    }

    pub fn should_apply_match(&self, result: &ProblemMatch) -> Promise<bool> {
        let this = self.self_rc();
        let resource = result.resource.clone();
        match result.description.apply_to {
            ApplyToKind::AllDocuments => Promise::resolved(true),
            ApplyToKind::OpenDocuments => async_fn(async move {
                let resource = resource.await?;
                Ok(this.open_models.borrow().contains(&resource.to_string(false).to_string_lossy()))
            }),
            ApplyToKind::ClosedDocuments => async_fn(async move {
                let resource = resource.await?;
                Ok(!this.open_models.borrow().contains(&resource.to_string(false).to_string_lossy()))
            }),
        }
    }

    fn try_matchers(&self) -> Option<ProblemMatch> {
        self.active_matcher.set(None);
        let buffer = self.buffer.borrow().clone();
        let length = buffer.len();
        for start_index in 0..length {
            let key = length - start_index;
            let mut matchers = self.matchers.borrow_mut();
            let Some(candidates) = matchers.get_mut(&key) else {
                continue;
            };
            for (index, matcher) in candidates.iter_mut().enumerate() {
                let result = matcher.handle(&buffer, start_index);
                if let Some(found) = result.r#match {
                    self.capture_match(&found);
                    if result.r#continue {
                        self.active_matcher.set(Some((key, index)));
                    }
                    return Some(found);
                }
            }
        }
        None
    }

    fn capture_match(&self, found: &ProblemMatch) {
        self.number_of_matches.set(self.number_of_matches.get() + 1);
        let severity = found.marker.severity;
        if self.max_marker_severity.get().is_none_or(|max| severity.0 > max.0) {
            self.max_marker_severity.set(Some(severity));
        }
    }

    fn clear_buffer(&self) {
        let mut buffer = self.buffer.borrow_mut();
        if !buffer.is_empty() {
            *buffer = Vec::new();
        }
    }

    pub fn record_resources_to_clean(&self, owner: &str) {
        let markers = self.marker_service.read(Some(&MarkerReadOptions { owner: Some(owner.to_string()), ..MarkerReadOptions::default() }));
        let mut resources_to_clean = self.resources_to_clean.borrow_mut();
        let resource_set_to_clean = Self::resource_set_to_clean(&mut resources_to_clean, owner);
        for marker in markers {
            resource_set_to_clean.insert(marker.resource.to_string(false).to_string_lossy(), marker.resource.clone());
        }
    }

    pub fn record_resource_to_clean(&self, owner: &str, resource: &URI) {
        let mut resources_to_clean = self.resources_to_clean.borrow_mut();
        Self::resource_set_to_clean(&mut resources_to_clean, owner).insert(resource.to_string(false).to_string_lossy(), resource.clone());
    }

    pub fn remove_resource_to_clean(&self, owner: &str, resource: &str) {
        if let Some(resource_set) = self.resources_to_clean.borrow_mut().get_mut(&owner.to_string()) {
            resource_set.remove(&resource.to_string());
        }
    }

    fn resource_set_to_clean<'a>(
        resources_to_clean: &'a mut OrderedMap<String, OrderedMap<String, URI>>,
        owner: &str,
    ) -> &'a mut OrderedMap<String, URI> {
        let owner = owner.to_string();
        if !resources_to_clean.contains_key(&owner) {
            resources_to_clean.insert(owner.clone(), OrderedMap::new());
        }
        resources_to_clean.get_mut(&owner).expect("o conjunto foi criado acima")
    }

    pub fn clean_all_markers(&self) {
        let resources_to_clean = std::mem::replace(&mut *self.resources_to_clean.borrow_mut(), OrderedMap::new());
        for (owner, value) in resources_to_clean.iter() {
            self.clean_markers_of(owner, value);
        }
    }

    pub fn clean_markers(&self, owner: &str) {
        let to_clean = self.resources_to_clean.borrow_mut().remove(&owner.to_string());
        if let Some(to_clean) = to_clean {
            self.clean_markers_of(owner, &to_clean);
        }
    }

    fn clean_markers_of(&self, owner: &str, to_clean: &OrderedMap<String, URI>) {
        let mut uris: Vec<URI> = Vec::new();
        let apply_to = self.apply_to_by_owner.borrow().get(&owner.to_string()).copied();
        let open_models = self.open_models.borrow();
        for (uri_as_string, uri) in to_clean.iter() {
            let is_open = open_models.contains(uri_as_string);
            if apply_to == Some(ApplyToKind::AllDocuments)
                || (apply_to == Some(ApplyToKind::OpenDocuments) && is_open)
                || (apply_to == Some(ApplyToKind::ClosedDocuments) && !is_open)
            {
                uris.push(uri.clone());
            }
        }
        drop(open_models);
        self.marker_service.remove(owner, &uris);
    }

    pub fn record_marker(&self, marker: MarkerData, owner: &str, resource_as_string: &str) {
        let mut markers = self.markers.borrow_mut();
        let owner = owner.to_string();
        if !markers.contains_key(&owner) {
            markers.insert(owner.clone(), OrderedMap::new());
        }
        let markers_per_owner = markers.get_mut(&owner).expect("o mapa do dono foi criado acima");
        let resource_as_string = resource_as_string.to_string();
        if !markers_per_owner.contains_key(&resource_as_string) {
            markers_per_owner.insert(resource_as_string.clone(), OrderedMap::new());
        }
        let markers_per_resource = markers_per_owner.get_mut(&resource_as_string).expect("o mapa do recurso foi criado acima");
        let key = marker.make_key_optional_message(false);
        match markers_per_resource.get(&key) {
            None => {
                markers_per_resource.insert(key, marker);
            }
            Some(existing_marker) => {
                // Most likely https://github.com/microsoft/vscode/issues/77475
                // Heuristic dictates that when the key is the same and message is smaller, we have hit this limitation.
                if units(&existing_marker.message).len() < units(&marker.message).len() && Platform::detect().is_windows {
                    markers_per_resource.insert(key, marker);
                }
            }
        }
    }

    pub fn report_markers(&self) {
        let markers = self.markers.borrow().clone();
        for (owner, markers_per_owner) in markers.iter() {
            for (resource, markers) in markers_per_owner.iter() {
                self.deliver_markers_per_owner_and_resource_resolved(owner, resource, markers);
            }
        }
    }

    pub fn deliver_markers_per_owner_and_resource(&self, owner: &str, resource: &str) {
        let markers_per_resource = {
            let markers = self.markers.borrow();
            let Some(markers_per_owner) = markers.get(&owner.to_string()) else {
                return;
            };
            let Some(markers_per_resource) = markers_per_owner.get(&resource.to_string()) else {
                return;
            };
            markers_per_resource.clone()
        };
        self.deliver_markers_per_owner_and_resource_resolved(owner, resource, &markers_per_resource);
    }

    fn deliver_markers_per_owner_and_resource_resolved(&self, owner: &str, resource: &str, markers: &OrderedMap<String, MarkerData>) {
        let delivered = {
            let mut delivered_markers = self.delivered_markers.borrow_mut();
            let owner = owner.to_string();
            if !delivered_markers.contains_key(&owner) {
                delivered_markers.insert(owner.clone(), OrderedMap::new());
            }
            delivered_markers.get(&owner).and_then(|reported| reported.get(&resource.to_string()).copied())
        };
        if Some(markers.len()) != delivered {
            let to_set: Vec<MarkerData> = markers.values().cloned().collect();
            self.marker_service.change_one(owner, &URI::parse(resource, false), to_set, None);
            if let Some(reported) = self.delivered_markers.borrow_mut().get_mut(&owner.to_string()) {
                reported.insert(resource.to_string(), markers.len());
            }
        }
    }

    pub fn clean_marker_caches(&self) {
        self.number_of_matches.set(0);
        self.max_marker_severity.set(None);
        self.markers.borrow_mut().clear();
        self.delivered_markers.borrow_mut().clear();
    }

    pub fn done(&self) {
        self.report_markers();
        self.clean_all_markers();
    }

    pub fn model_listeners(&self) -> &Rc<DisposableStore> {
        &self.model_listeners
    }
}

impl IDisposable for AbstractProblemCollector {
    fn dispose(&self) {
        self.base.dispose();
        self.model_listeners.dispose();
    }
}

/// `ProblemHandlingStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemHandlingStrategy {
    Clean,
}

/// `StartStopProblemCollector`.
pub struct StartStopProblemCollector {
    this: Weak<StartStopProblemCollector>,
    pub base: Rc<AbstractProblemCollector>,
    current_owner: RefCell<Option<String>>,
    current_resource: RefCell<Option<String>>,
    has_started: Cell<bool>,
}

impl StartStopProblemCollector {
    pub fn new(
        problem_matchers: Vec<ProblemMatcher>,
        marker_service: Rc<dyn IMarkerService>,
        model_service: Rc<dyn IModelService>,
        _strategy: ProblemHandlingStrategy,
        file_service: Option<Rc<dyn IFileService>>,
        log_service: Option<Rc<dyn LogService>>,
    ) -> Rc<Self> {
        let base = AbstractProblemCollector::new(problem_matchers.clone(), marker_service, model_service, file_service, log_service);
        let mut owners: Vec<String> = Vec::new();
        for description in &problem_matchers {
            if !owners.contains(&description.owner) {
                owners.push(description.owner.clone());
            }
        }
        for owner in &owners {
            base.record_resources_to_clean(owner);
        }
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            current_owner: RefCell::new(None),
            current_resource: RefCell::new(None),
            has_started: Cell::new(false),
        })
    }

    fn process_line_internal(&self, line: String) -> Promise<()> {
        let this = self.this.upgrade().expect("o coletor está vivo enquanto é usado");
        async_fn(async move {
            if !this.has_started.get() {
                this.has_started.set(true);
                this.base.fire_state_change(IProblemCollectorEvent::create(ProblemCollectorEventKind::BackgroundProcessingBegins, None));
            }
            let Some(marker_match) = this.base.try_find_marker(&line) else {
                return Ok(());
            };

            let owner = marker_match.description.owner.clone();
            let resource = marker_match.resource.clone().await?;
            let resource_as_string = resource.to_string(false).to_string_lossy();
            this.base.remove_resource_to_clean(&owner, &resource_as_string);
            let should_apply_match = this.base.should_apply_match(&marker_match).await?;
            if should_apply_match {
                this.base.record_marker(marker_match.marker.clone(), &owner, &resource_as_string);
                let changed = this.current_owner.borrow().as_deref() != Some(owner.as_str())
                    || this.current_resource.borrow().as_deref() != Some(resource_as_string.as_str());
                if changed {
                    let current_owner = this.current_owner.borrow().clone();
                    let current_resource = this.current_resource.borrow().clone();
                    if let (Some(current_owner), Some(current_resource)) = (current_owner, current_resource) {
                        this.base.deliver_markers_per_owner_and_resource(&current_owner, &current_resource);
                    }
                    *this.current_owner.borrow_mut() = Some(owner);
                    *this.current_resource.borrow_mut() = Some(resource_as_string);
                }
            }
            Ok(())
        })
    }
}

impl IProblemMatcher for StartStopProblemCollector {
    fn process_line(&self, line: &str) {
        let this = self.this.upgrade().expect("o coletor está vivo enquanto é usado");
        let line = line.to_string();
        self.base.enqueue(move || this.process_line_internal(line));
    }
}

impl IDisposable for StartStopProblemCollector {
    fn dispose(&self) {
        self.base.dispose();
    }
}

struct BackgroundPatterns {
    key: String,
    matcher: ProblemMatcher,
    begin: WatchingPattern,
    end: WatchingPattern,
}

/// `WatchingProblemCollector`.
pub struct WatchingProblemCollector {
    this: Weak<WatchingProblemCollector>,
    pub base: Rc<AbstractProblemCollector>,

    background_patterns: Vec<BackgroundPatterns>,

    // workaround for https://github.com/microsoft/vscode/issues/44018
    active_background_matchers: RefCell<HashSet<String>>,

    // Current State
    current_owner: RefCell<Option<String>>,
    current_resource: RefCell<Option<String>>,

    lines: RefCell<Vec<String>>,
    pub begin_patterns: Vec<Rc<JsRegExp>>,
}

/// `regexp.exec(line)` com o aviso de execução lenta.
fn exec_timed(base: &AbstractProblemCollector, regexp: &JsRegExp, line: &[u16], kind: &str) -> Option<zyarr::Match> {
    let start = runtime::date_now();
    let matches = regexp.exec(line);
    let elapsed = runtime::date_now().saturating_sub(start);
    if elapsed > 5 {
        if let Some(log_service) = base.log_service() {
            log_service.trace(&format!("ProblemMatcher: slow {kind} regexp took {elapsed}ms to execute {}", String::from_utf16_lossy(regexp.source())));
        }
    }
    matches
}

fn group_text(line: &[u16], matches: &zyarr::Match, index: usize) -> Option<String> {
    matches.captures.get(index).and_then(|range| range.clone()).map(|range| String::from_utf16_lossy(&line[range]))
}

impl WatchingProblemCollector {
    pub fn new(
        problem_matchers: Vec<ProblemMatcher>,
        marker_service: Rc<dyn IMarkerService>,
        model_service: Rc<dyn IModelService>,
        file_service: Option<Rc<dyn IFileService>>,
        log_service: Option<Rc<dyn LogService>>,
    ) -> Rc<Self> {
        let base = AbstractProblemCollector::new(problem_matchers.clone(), marker_service.clone(), model_service.clone(), file_service, log_service);
        let mut background_patterns: Vec<BackgroundPatterns> = Vec::new();
        let mut begin_patterns: Vec<Rc<JsRegExp>> = Vec::new();
        for matcher in &problem_matchers {
            if let Some(watching) = &matcher.watching {
                background_patterns.push(BackgroundPatterns {
                    key: generate_uuid(),
                    matcher: matcher.clone(),
                    begin: watching.begins_pattern.clone(),
                    end: watching.ends_pattern.clone(),
                });
                begin_patterns.push(watching.begins_pattern.regexp.clone());
            }
        }

        let collector = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            background_patterns,
            active_background_matchers: RefCell::new(HashSet::new()),
            current_owner: RefCell::new(None),
            current_resource: RefCell::new(None),
            lines: RefCell::new(Vec::new()),
            begin_patterns,
        });
        collector.reset_current_resource();

        let this = collector.this.clone();
        collector.base.model_listeners().add(model_service.on_model_removed().listen(move |model_event| {
            let model_uri = model_event.uri();
            let marker_changed: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
            let listener_this = this.clone();
            let listener_marker_service = marker_service.clone();
            let listener_uri = model_uri.clone();
            let subscription = marker_service
                .on_marker_changed()
                .debounce(
                    |last: Option<Vec<URI>>, event: &Vec<URI>| {
                        let mut merged = last.unwrap_or_default();
                        merged.extend(event.iter().cloned());
                        merged
                    },
                    DebounceOptions { delay: Delay::Millis(500), leading: false, flush_on_listener_remove: true, leak_warning_threshold: None },
                    None,
                )
                .listen(move |marker_event: &Vec<URI>| {
                    if marker_event.is_empty() {
                        return;
                    }
                    let model_event_uri_str = listener_uri.to_string(false).to_string_lossy();
                    let still_has_markers = !listener_marker_service
                        .read(Some(&MarkerReadOptions { resource: Some(listener_uri.clone()), ..MarkerReadOptions::default() }))
                        .is_empty();
                    if !marker_event.iter().any(|uri| uri.to_string(false).to_string_lossy() == model_event_uri_str) || still_has_markers {
                        return;
                    }
                    let Some(collector) = listener_this.upgrade() else {
                        return;
                    };
                    let old_lines: Vec<String> = collector.lines.borrow().clone();
                    let _ = async_fn(async move {
                        for line in old_lines {
                            collector.process_line_internal(line, false).await?;
                        }
                        Ok(())
                    });
                });
            *marker_changed.borrow_mut() = Some(subscription);

            // Dispose the debounced listener after timeout - no need to register it since
            // it's only used temporarily and will be disposed below
            runtime::set_timeout(
                move || {
                    let pending = marker_changed.borrow_mut().take();
                    if let Some(pending) = pending {
                        pending.dispose();
                    }
                },
                600,
            );
        }));

        collector
    }

    fn self_rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o coletor está vivo enquanto é usado")
    }

    pub fn about_to_start(&self) {
        for background in &self.background_patterns {
            if background.matcher.watching.as_ref().is_some_and(|watching| watching.active_on_start) {
                self.active_background_matchers.borrow_mut().insert(background.key.clone());
                self.base.fire_state_change(IProblemCollectorEvent::create(ProblemCollectorEventKind::BackgroundProcessingBegins, None));
                self.base.record_resources_to_clean(&background.matcher.owner);
            }
        }
    }

    fn process_line_internal(&self, line: String, record_line: bool) -> Promise<()> {
        let this = self.self_rc();
        async_fn(async move {
            if this.try_begin(line.clone(), record_line).await? || this.try_finish(&line, record_line) {
                return Ok(());
            }
            if record_line {
                this.lines.borrow_mut().push(line.clone());
            }
            let Some(marker_match) = this.base.try_find_marker(&line) else {
                return Ok(());
            };
            let resource = marker_match.resource.clone().await?;
            let owner = marker_match.description.owner.clone();
            let resource_as_string = resource.to_string(false).to_string_lossy();
            this.base.remove_resource_to_clean(&owner, &resource_as_string);
            let should_apply_match = this.base.should_apply_match(&marker_match).await?;
            if should_apply_match {
                this.base.record_marker(marker_match.marker.clone(), &owner, &resource_as_string);
                let changed = this.current_owner.borrow().as_deref() != Some(owner.as_str())
                    || this.current_resource.borrow().as_deref() != Some(resource_as_string.as_str());
                if changed {
                    this.report_markers_for_current_resource();
                    *this.current_owner.borrow_mut() = Some(owner);
                    *this.current_resource.borrow_mut() = Some(resource_as_string);
                }
            }
            Ok(())
        })
    }

    pub fn force_delivery(&self) {
        self.report_markers_for_current_resource();
    }

    fn try_begin(&self, line: String, record_line: bool) -> Promise<bool> {
        let this = self.self_rc();
        async_fn(async move {
            let mut result = false;
            let line_units = units(&line);
            for background in &this.background_patterns {
                let matches = exec_timed(&this.base, &background.begin.regexp, &line_units, "begin");
                if let Some(matches) = matches {
                    if this.active_background_matchers.borrow().contains(&background.key) {
                        continue;
                    }
                    this.active_background_matchers.borrow_mut().insert(background.key.clone());
                    result = true;
                    this.base.fire_first_match();
                    if record_line {
                        *this.lines.borrow_mut() = vec![line.clone()];
                    }
                    this.base.fire_state_change(IProblemCollectorEvent::create(ProblemCollectorEventKind::BackgroundProcessingBegins, None));
                    this.base.clean_marker_caches();
                    this.reset_current_resource();
                    let owner = background.matcher.owner.clone();
                    let file = background.begin.file.and_then(|index| group_text(&line_units, &matches, index)).filter(|file| !file.is_empty());
                    if let Some(file) = file {
                        let resource = get_resource(&file, &background.matcher, None);
                        this.base.record_resource_to_clean(&owner, &resource.await?);
                    } else {
                        this.base.record_resources_to_clean(&owner);
                    }
                }
            }
            Ok(result)
        })
    }

    fn try_finish(&self, line: &str, record_line: bool) -> bool {
        let mut result = false;
        let line_units = units(line);
        for background in &self.background_patterns {
            let matches = exec_timed(&self.base, &background.end.regexp, &line_units, "end");
            if let Some(matches) = matches {
                if self.base.number_of_matches() > 0 {
                    self.base
                        .fire_errors(self.base.marker_service().read(Some(&MarkerReadOptions { owner: Some(background.matcher.owner.clone()), ..MarkerReadOptions::default() })));
                } else {
                    self.base.fire_invalidate_last_marker();
                }
                if self.active_background_matchers.borrow_mut().remove(&background.key) {
                    self.reset_current_resource();
                    let captured_variables = (!matches.groups.is_empty()).then(|| {
                        matches
                            .groups
                            .iter()
                            .map(|(name, range)| (name.clone(), range.clone().map(|range| String::from_utf16_lossy(&line_units[range]))))
                            .collect::<Vec<_>>()
                    });
                    self.base.fire_state_change(IProblemCollectorEvent::create(ProblemCollectorEventKind::BackgroundProcessingEnds, captured_variables));
                    result = true;
                    if record_line {
                        self.lines.borrow_mut().push(line.to_string());
                    }
                    let owner = background.matcher.owner.clone();
                    self.base.clean_markers(&owner);
                    self.base.clean_marker_caches();
                }
            }
        }
        result
    }

    fn reset_current_resource(&self) {
        self.report_markers_for_current_resource();
        *self.current_owner.borrow_mut() = None;
        *self.current_resource.borrow_mut() = None;
    }

    fn report_markers_for_current_resource(&self) {
        let current_owner = self.current_owner.borrow().clone();
        let current_resource = self.current_resource.borrow().clone();
        if let (Some(current_owner), Some(current_resource)) = (current_owner, current_resource) {
            self.base.deliver_markers_per_owner_and_resource(&current_owner, &current_resource);
        }
    }

    pub fn done(&self) {
        for owner in self.base.apply_to_owners() {
            self.base.record_resources_to_clean(&owner);
        }
        self.base.done();
    }

    pub fn is_watching(&self) -> bool {
        !self.background_patterns.is_empty()
    }
}

impl IProblemMatcher for WatchingProblemCollector {
    fn process_line(&self, line: &str) {
        let this = self.self_rc();
        let line = line.to_string();
        self.base.enqueue(move || this.process_line_internal(line, true));
    }
}

impl IDisposable for WatchingProblemCollector {
    fn dispose(&self) {
        self.base.dispose();
    }
}
