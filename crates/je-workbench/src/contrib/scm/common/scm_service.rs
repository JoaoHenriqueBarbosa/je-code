//! Porte de `src/vs/workbench/contrib/scm/common/scmService.ts`: o `SCMService` (registro dos provedores e dos
//! repositórios), o `SCMRepository`, o `SCMInput` com o histórico navegável e o `SCMInputHistory` que o persiste no
//! armazenamento do workspace.
//!
//! Notas de tradução:
//! - **`extends Disposable`** (`SCMInput`): a struct guarda um [`Disposable`] por dentro, com os emissores e o ouvinte do
//!   histórico registrados. O que o original escreve com `this` num fecho segura um `Weak` do objeto, que nasce em
//!   `Rc::new_cyclic` (`SCMRepository`) ou logo depois de `Rc::new` (`SCMInput`, `SCMInputHistory`, `SCMService`).
//! - **`SCMInput.repository`** e **`SCMRepository.input`** se referenciam; o `SCMInput` guarda um `Weak<SCMRepository>`
//!   (o repositório nasce em `Rc::new_cyclic`, e o construtor do input lê o provedor que o chamador passa, não o
//!   `repository.provider`, que ainda não existe nesse ponto).
//! - **`historyNavigator`** é compartilhado entre o `SCMInputHistory` e o `SCMInput` (o mesmo objeto nos dois), então é um
//!   `Rc<RefCell<HistoryNavigator2>>`; nenhuma referência do `RefCell` atravessa um `fire`.
//! - **`getObject<[string, URI, string[]][]>('scm.history', ...)`**: o armazenamento devolve `json::Value`, e o URI (que o
//!   `JSON.parse` do original deixa como objeto simples de componentes) é reconstruído a partir dos componentes, para a
//!   chave do `ResourceMap` ser a do URI de verdade. Uma entrada malformada (que o original faria lançar na
//!   desestruturação) ou com histórico vazio (que lançaria no construtor do `HistoryNavigator2`) é ignorada.
//! - **A regex `/^scm\/input:([^:]+):(.+)$/`** é [`parse_legacy_key`]; o `.` não casa terminador de linha.
//! - **`legacyHistory.history as string[]`**: uma entrada que não seja texto é ignorada na migração.
//! - **`getRepository(idOrResource)`** são dois métodos (ver `scm.rs`), e `_repositories` (usado nos testes) é o campo
//!   [`SCMService::repositories_by_id`].
//! - **`registerSingleton`** não é portado: o registro de singletons é feito por quem monta os serviços.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::history::HistoryNavigator2;
use je_base::json::Value;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::network::schemas;
use je_base::observable::base::Observable;
use je_base::observable::run_on_change;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::log::{LogService, Logger};
use je_platform::resources::uri_file;
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;
use je_storage::StorageValue;
use je_storage::marshalling::{parse, uri_to_value};

use super::scm::{
    IInputValidation, IInputValidator, ISCMHistoryProvider, ISCMInput, ISCMInputChangeEvent, ISCMProvider, ISCMRepository, ISCMService, InputValidationMessage,
    InputValidationType, SCMInputChangeReason,
};

const SCM_HISTORY_KEY: &str = "scm.history";
const LEGACY_KEY_PREFIX: &str = "scm/input:";
const HISTORY_CAPACITY: usize = 100;

/// O `HistoryNavigator2<string>` compartilhado entre o `SCMInputHistory` e o `SCMInput`.
type HistoryNavigator = Rc<RefCell<HistoryNavigator2<String, String>>>;

/// `new HistoryNavigator2(history, 100)`.
fn new_history_navigator(history: &[String]) -> HistoryNavigator {
    Rc::new(RefCell::new(HistoryNavigator2::new(history, HISTORY_CAPACITY, |value: &String| value.clone())))
}

/// Um emissor que morre junto com o `base`.
fn register_emitter<T: 'static>(base: &Disposable) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `SCMInput`.
pub struct SCMInput {
    base: Rc<Disposable>,
    repository: Weak<SCMRepository>,

    value: RefCell<String>,
    on_did_change: Rc<Emitter<ISCMInputChangeEvent>>,

    placeholder: RefCell<String>,
    on_did_change_placeholder: Rc<Emitter<String>>,

    enabled: Cell<bool>,
    on_did_change_enablement: Rc<Emitter<bool>>,

    visible: Cell<bool>,
    on_did_change_visibility: Rc<Emitter<bool>>,

    on_did_change_focus: Rc<Emitter<()>>,

    on_did_change_validation_message: Rc<Emitter<IInputValidation>>,
    on_did_clear_validation: Rc<Emitter<()>>,

    validate_input: RefCell<IInputValidator>,
    on_did_change_validate_input: Rc<Emitter<()>>,

    history_navigator: HistoryNavigator,
    did_change_history: Cell<bool>,
}

impl SCMInput {
    /// `new SCMInput(repository, history)`: `provider` é o `repository.provider`.
    pub fn new(repository: Weak<SCMRepository>, provider: &Rc<dyn ISCMProvider>, history: &Rc<SCMInputHistory>) -> Rc<Self> {
        let base = Disposable::new();
        let root_uri = provider.root_uri();

        let history_navigator = match &root_uri {
            Some(root_uri) => history.get_history(&provider.label(), root_uri),
            // in memory only
            None => new_history_navigator(&[String::new()]),
        };
        let value = history_navigator.borrow().current();

        let input: Rc<SCMInput> = Rc::new(Self {
            repository,
            value: RefCell::new(value),
            on_did_change: register_emitter(&base),
            placeholder: RefCell::new(String::new()),
            on_did_change_placeholder: register_emitter(&base),
            enabled: Cell::new(true),
            on_did_change_enablement: register_emitter(&base),
            visible: Cell::new(true),
            on_did_change_visibility: register_emitter(&base),
            on_did_change_focus: register_emitter(&base),
            on_did_change_validation_message: register_emitter(&base),
            on_did_clear_validation: register_emitter(&base),
            validate_input: RefCell::new(Rc::new(|_value: &str, _cursor_position: usize| Promise::resolved(None))),
            on_did_change_validate_input: register_emitter(&base),
            history_navigator,
            did_change_history: Cell::new(false),
            base,
        });

        if root_uri.is_some() {
            let me = Rc::downgrade(&input);
            input.base.register(history.on_will_save_history().listen(move |event| {
                let Some(this) = me.upgrade() else { return };

                let is_at_end = this.history_navigator.borrow().is_at_end();
                if is_at_end {
                    this.save_value();
                }

                if this.did_change_history.get() {
                    event.history_did_indeed_change();
                }

                this.did_change_history.set(false);
            }));
        }

        input
    }

    fn save_value(&self) {
        let current = self.value.borrow().clone();
        let old_value = self.history_navigator.borrow_mut().replace_last(current.clone());
        self.did_change_history.set(self.did_change_history.get() || old_value != current);
    }
}

impl ISCMInput for SCMInput {
    fn repository(&self) -> Rc<dyn ISCMRepository> {
        self.repository.upgrade().expect("o repositório vive enquanto o seu input é usado")
    }

    fn value(&self) -> String {
        self.value.borrow().clone()
    }

    fn set_value(&self, value: &str, transient: bool, reason: Option<SCMInputChangeReason>) {
        if *self.value.borrow() == value {
            return;
        }

        if !transient {
            let current = self.value.borrow().clone();
            let mut navigator = self.history_navigator.borrow_mut();
            navigator.replace_last(current);
            navigator.add(value.to_string());
            drop(navigator);
            self.did_change_history.set(true);
        }

        *self.value.borrow_mut() = value.to_string();
        self.on_did_change.fire(ISCMInputChangeEvent { value: value.to_string(), reason });
    }

    fn on_did_change(&self) -> Event<ISCMInputChangeEvent> {
        self.on_did_change.event()
    }

    fn placeholder(&self) -> String {
        self.placeholder.borrow().clone()
    }

    fn set_placeholder(&self, placeholder: &str) {
        *self.placeholder.borrow_mut() = placeholder.to_string();
        self.on_did_change_placeholder.fire(placeholder.to_string());
    }

    fn on_did_change_placeholder(&self) -> Event<String> {
        self.on_did_change_placeholder.event()
    }

    fn validate_input(&self) -> IInputValidator {
        self.validate_input.borrow().clone()
    }

    fn set_validate_input(&self, validate_input: IInputValidator) {
        *self.validate_input.borrow_mut() = validate_input;
        self.on_did_change_validate_input.fire(());
    }

    fn on_did_change_validate_input(&self) -> Event<()> {
        self.on_did_change_validate_input.event()
    }

    fn enabled(&self) -> bool {
        self.enabled.get()
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
        self.on_did_change_enablement.fire(enabled);
    }

    fn on_did_change_enablement(&self) -> Event<bool> {
        self.on_did_change_enablement.event()
    }

    fn visible(&self) -> bool {
        self.visible.get()
    }

    fn set_visible(&self, visible: bool) {
        self.visible.set(visible);
        self.on_did_change_visibility.fire(visible);
    }

    fn on_did_change_visibility(&self) -> Event<bool> {
        self.on_did_change_visibility.event()
    }

    fn set_focus(&self) {
        self.on_did_change_focus.fire(());
    }

    fn on_did_change_focus(&self) -> Event<()> {
        self.on_did_change_focus.event()
    }

    fn show_validation_message(&self, message: InputValidationMessage, r#type: InputValidationType) {
        self.on_did_change_validation_message.fire(IInputValidation { message, r#type });
    }

    fn on_did_change_validation_message(&self) -> Event<IInputValidation> {
        self.on_did_change_validation_message.event()
    }

    fn clear_validation(&self) {
        self.on_did_clear_validation.fire(());
    }

    fn on_did_clear_validation(&self) -> Event<()> {
        self.on_did_clear_validation.event()
    }

    fn show_next_history_value(&self) {
        let is_at_end = self.history_navigator.borrow().is_at_end();
        if is_at_end {
            return;
        }

        let has_value = self.history_navigator.borrow().has(&self.value());
        if !has_value {
            self.save_value();
            self.history_navigator.borrow().reset_cursor();
        }

        let value = self.history_navigator.borrow().next();
        self.set_value(&value, true, Some(SCMInputChangeReason::HistoryNext));
    }

    fn show_previous_history_value(&self) {
        let is_at_end = self.history_navigator.borrow().is_at_end();
        if is_at_end {
            self.save_value();
        } else {
            let has_value = self.history_navigator.borrow().has(&self.value());
            if !has_value {
                self.save_value();
                self.history_navigator.borrow().reset_cursor();
            }
        }

        let value = self.history_navigator.borrow().previous();
        self.set_value(&value, true, Some(SCMInputChangeReason::HistoryPrevious));
    }
}

impl IDisposable for SCMInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `SCMRepository`.
pub struct SCMRepository {
    id: String,
    provider: Rc<dyn ISCMProvider>,
    disposables: Rc<DisposableStore>,
    input: Rc<SCMInput>,
    selected: Cell<bool>,
    on_did_change_selection: Rc<Emitter<bool>>,
}

impl SCMRepository {
    pub fn new(id: String, provider: Rc<dyn ISCMProvider>, disposables: Rc<DisposableStore>, input_history: &Rc<SCMInputHistory>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            input: SCMInput::new(me.clone(), &provider, input_history),
            id,
            provider,
            disposables,
            selected: Cell::new(false),
            on_did_change_selection: Emitter::new(),
        })
    }

    pub fn selected(&self) -> bool {
        self.selected.get()
    }

    pub fn on_did_change_selection(&self) -> Event<bool> {
        self.on_did_change_selection.event()
    }

    pub fn set_selected(&self, selected: bool) {
        if self.selected.get() == selected {
            return;
        }

        self.selected.set(selected);
        self.on_did_change_selection.fire(selected);
    }
}

impl ISCMRepository for SCMRepository {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn provider(&self) -> Rc<dyn ISCMProvider> {
        self.provider.clone()
    }

    fn input(&self) -> Rc<dyn ISCMInput> {
        self.input.clone()
    }
}

impl IDisposable for SCMRepository {
    fn dispose(&self) {
        self.disposables.dispose();
        self.on_did_change_selection.dispose();
        self.input.dispose();
        self.provider.dispose();
    }
}

/// `WillSaveHistoryEvent`.
#[derive(Default)]
pub struct WillSaveHistoryEvent {
    did_change_history: Cell<bool>,
}

impl WillSaveHistoryEvent {
    pub fn did_change_history(&self) -> bool {
        self.did_change_history.get()
    }

    pub fn history_did_indeed_change(&self) {
        self.did_change_history.set(true);
    }
}

/// `SCMInputHistory`.
pub struct SCMInputHistory {
    disposables: Rc<DisposableStore>,
    histories: RefCell<OrderedMap<String, ResourceMap<HistoryNavigator>>>,
    on_will_save_history: Rc<Emitter<Rc<WillSaveHistoryEvent>>>,
    storage_service: Rc<dyn StorageService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl SCMInputHistory {
    pub fn new(storage_service: Rc<dyn StorageService>, workspace_context_service: Rc<dyn IWorkspaceContextService>) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let on_will_save_history = disposables.add_rc(Emitter::new());

        let this = Rc::new(Self { disposables, histories: RefCell::new(OrderedMap::new()), on_will_save_history, storage_service, workspace_context_service });

        for (provider_label, root_uri, history) in this.read_entries() {
            let navigator = new_history_navigator(&history);
            this.with_provider_histories(&provider_label, |provider_histories| {
                provider_histories.set(root_uri, navigator);
            });
        }

        if this.migrate_storage() {
            this.save_to_storage();
        }

        let me = Rc::downgrade(&this);
        this.disposables.add(this.storage_service.on_did_change_value(StorageScope::Workspace, Some(SCM_HISTORY_KEY), &this.disposables).listen(move |event| {
            if event.external && event.key == SCM_HISTORY_KEY {
                let Some(this) = me.upgrade() else { return };

                for (provider_label, uri, raw_history) in this.read_entries() {
                    let history = this.get_history(&provider_label, &uri);

                    for value in raw_history.into_iter().rev() {
                        history.borrow_mut().prepend(value);
                    }
                }
            }
        }));

        let me = Rc::downgrade(&this);
        this.disposables.add(this.storage_service.on_will_save_state().listen(move |_| {
            let Some(this) = me.upgrade() else { return };

            let event = Rc::new(WillSaveHistoryEvent::default());
            this.on_will_save_history.fire(event.clone());

            if event.did_change_history() {
                this.save_to_storage();
            }
        }));

        this
    }

    pub fn on_will_save_history(&self) -> Event<Rc<WillSaveHistoryEvent>> {
        self.on_will_save_history.event()
    }

    /// `getObject<[string, URI, string[]][]>('scm.history', StorageScope.WORKSPACE, [])`.
    fn read_entries(&self) -> Vec<(String, URI, Vec<String>)> {
        match self.storage_service.get_object(SCM_HISTORY_KEY, StorageScope::Workspace, Some(Value::Array(Vec::new()))) {
            Some(Value::Array(entries)) => entries.iter().filter_map(history_entry_from_value).collect(),
            _ => Vec::new(),
        }
    }

    fn save_to_storage(&self) {
        let mut raw: Vec<Value> = Vec::new();

        for (provider_label, provider_histories) in self.histories.borrow().iter() {
            for (root_uri, history) in provider_histories.entries() {
                let history = history.borrow();
                if !(history.size() == 1 && history.current().is_empty()) {
                    let entries: Vec<Value> = history.iter().map(|value| Value::from(value.as_str())).collect();
                    raw.push(Value::Array(vec![Value::from(provider_label.as_str()), uri_to_value(root_uri), Value::Array(entries)]));
                }
            }
        }

        self.storage_service.store(SCM_HISTORY_KEY, StorageValue::Object(Value::Array(raw)), StorageScope::Workspace, StorageTarget::User);
    }

    /// O `ResourceMap` do rótulo do provedor, criado se faltar.
    fn with_provider_histories<R>(&self, provider_label: &str, f: impl FnOnce(&mut ResourceMap<HistoryNavigator>) -> R) -> R {
        let mut histories = self.histories.borrow_mut();
        let key = provider_label.to_string();

        if !histories.contains_key(&key) {
            histories.insert(key.clone(), ResourceMap::new());
        }

        f(histories.get_mut(&key).expect("acabou de inserir"))
    }

    pub fn get_history(&self, provider_label: &str, root_uri: &URI) -> HistoryNavigator {
        self.with_provider_histories(provider_label, |provider_histories| {
            if let Some(history) = provider_histories.get(root_uri) {
                return history.clone();
            }

            let history = new_history_navigator(&[String::new()]);
            provider_histories.set(root_uri.clone(), history.clone());
            history
        })
    }

    // Migrates from Application scope storage to Workspace scope.
    // TODO@joaomoreno: Change from January 2024 onwards such that the only code is to remove all `scm/input:` storage keys
    fn migrate_storage(&self) -> bool {
        let mut did_something_change = false;
        let machine_keys: Vec<String> =
            self.storage_service.keys(StorageScope::Application, StorageTarget::Machine).into_iter().filter(|key| key.starts_with(LEGACY_KEY_PREFIX)).collect();

        for key in machine_keys {
            match errors::catch(|| self.migrate_legacy_key(&key)) {
                Ok(changed) => did_something_change |= changed,
                Err(_) => self.storage_service.remove(&key, StorageScope::Application),
            }
        }

        did_something_change
    }

    /// O corpo do `try` de `migrateStorage`: devolve se a chave migrou (o `didSomethingChange = true`).
    fn migrate_legacy_key(&self, key: &str) -> bool {
        let legacy_history = parse(&self.storage_service.get(key, StorageScope::Application, Some("")).unwrap_or_default());
        let legacy_object = match &legacy_history {
            Value::Object(object) => Some(object),
            _ => None,
        };
        let history = match legacy_object.and_then(|object| object.get("history")) {
            Some(Value::Array(history)) => Some(history),
            _ => None,
        };
        let has_timestamp = matches!(legacy_object.and_then(|object| object.get("timestamp")), Some(Value::Number(timestamp)) if timestamp.is_finite() && timestamp.fract() == 0.0);

        let (Some((provider_label, root_path)), Some(history), true) = (parse_legacy_key(key), history, has_timestamp) else {
            self.storage_service.remove(key, StorageScope::Application);
            return false;
        };

        let root_uri = uri_file(root_path);

        if self.workspace_context_service.get_workspace_folder(&root_uri).is_some() {
            let navigator = self.get_history(provider_label, &root_uri);

            for entry in history.iter().rev() {
                if let Value::String(entry) = entry {
                    navigator.borrow_mut().prepend(entry.to_string_lossy());
                }
            }

            self.storage_service.remove(key, StorageScope::Application);
            return true;
        }

        false
    }
}

impl IDisposable for SCMInputHistory {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// `/^scm\/input:([^:]+):(.+)$/`: o rótulo do provedor e o caminho da raiz.
fn parse_legacy_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix(LEGACY_KEY_PREFIX)?;
    let (provider_label, root_path) = rest.split_once(':')?;

    // `.` não casa terminador de linha
    if provider_label.is_empty() || root_path.is_empty() || root_path.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }

    Some((provider_label, root_path))
}

/// `[string, URI, string[]]`.
fn history_entry_from_value(value: &Value) -> Option<(String, URI, Vec<String>)> {
    let Value::Array(parts) = value else { return None };
    let [Value::String(provider_label), root_uri, Value::Array(history)] = parts.as_slice() else { return None };

    let history = history
        .iter()
        .map(|entry| match entry {
            Value::String(entry) => Some(entry.to_string_lossy()),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;

    if history.is_empty() {
        return None;
    }

    Some((provider_label.to_string_lossy(), uri_from_value(root_uri)?, history))
}

/// O `URI` do objeto de componentes que o `URI#toJSON` grava.
fn uri_from_value(value: &Value) -> Option<URI> {
    let Value::Object(object) = value else { return None };
    let part = |key: &str| match object.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    };

    Some(URI::from(&UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") }, false))
}

/// `SCMService`.
pub struct SCMService {
    me: Weak<SCMService>,
    log_service: Rc<dyn LogService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,

    /// `_repositories` (usado nos testes).
    pub repositories_by_id: RefCell<OrderedMap<String, Rc<dyn ISCMRepository>>>,

    input_history: Rc<SCMInputHistory>,
    provider_count: Rc<dyn IContextKey>,
    history_provider_count: Rc<dyn IContextKey>,

    on_did_add_provider: Rc<Emitter<Rc<dyn ISCMRepository>>>,
    on_did_remove_provider: Rc<Emitter<Rc<dyn ISCMRepository>>>,
}

impl SCMService {
    pub fn new(
        log_service: Rc<dyn LogService>,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
        context_key_service: Rc<dyn IContextKeyService>,
        storage_service: Rc<dyn StorageService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let input_history = SCMInputHistory::new(storage_service, workspace_context_service);

        let provider_count = context_key_service.create_key("scm.providerCount", Dynamic::Number(0.0));
        let history_provider_count = context_key_service.create_key("scm.historyProviderCount", Dynamic::Number(0.0));

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            log_service,
            uri_identity_service,
            repositories_by_id: RefCell::new(OrderedMap::new()),
            input_history,
            provider_count,
            history_provider_count,
            on_did_add_provider: Emitter::new(),
            on_did_remove_provider: Emitter::new(),
        })
    }

    /// `Array.from(this._repositories.values()).filter(r => !!r.provider.historyProvider.get()).length`.
    fn count_history_providers(&self) -> usize {
        let repositories = self.repositories();
        repositories.iter().filter(|repository| repository.provider().history_provider().get().is_some()).count()
    }

    fn update_counts(&self) {
        self.provider_count.set(Dynamic::Number(self.repositories_by_id.borrow().len() as f64));
        self.history_provider_count.set(Dynamic::Number(self.count_history_providers() as f64));
    }
}

impl ISCMService for SCMService {
    fn on_did_add_repository(&self) -> Event<Rc<dyn ISCMRepository>> {
        self.on_did_add_provider.event()
    }

    fn on_did_remove_repository(&self) -> Event<Rc<dyn ISCMRepository>> {
        self.on_did_remove_provider.event()
    }

    fn repositories(&self) -> Vec<Rc<dyn ISCMRepository>> {
        self.repositories_by_id.borrow().values().cloned().collect()
    }

    fn repository_count(&self) -> usize {
        self.repositories_by_id.borrow().len()
    }

    fn register_scm_provider(&self, provider: Rc<dyn ISCMProvider>) -> Rc<dyn ISCMRepository> {
        self.log_service.trace("SCMService#registerSCMProvider");

        let provider_id = provider.id();
        if self.repositories_by_id.borrow().contains_key(&provider_id) {
            errors::throw(Error::new(format!("SCM Provider {provider_id} already exists.")));
        }

        let disposables = DisposableStore::new();
        let repository = SCMRepository::new(provider_id.clone(), provider.clone(), disposables.clone(), &self.input_history);
        let repository_ref: Rc<dyn ISCMRepository> = repository.clone();

        let me = self.me.clone();
        let removed_id = provider_id.clone();
        let removed_repository = Rc::downgrade(&repository);
        disposables.add(to_disposable(move || {
            let Some(this) = me.upgrade() else { return };

            this.repositories_by_id.borrow_mut().remove(&removed_id);
            if let Some(repository) = removed_repository.upgrade() {
                let repository: Rc<dyn ISCMRepository> = repository;
                this.on_did_remove_provider.fire(repository);
            }

            this.update_counts();
        }));

        self.repositories_by_id.borrow_mut().insert(provider_id, repository_ref.clone());

        let me = self.me.clone();
        disposables.add(run_on_change::<Option<Rc<dyn ISCMHistoryProvider>>, ()>(provider.history_provider(), move |_, _, _| {
            if let Some(this) = me.upgrade() {
                this.history_provider_count.set(Dynamic::Number(this.count_history_providers() as f64));
            }
        }));

        self.update_counts();

        self.on_did_add_provider.fire(repository_ref.clone());

        repository_ref
    }

    fn get_repository(&self, id: &str) -> Option<Rc<dyn ISCMRepository>> {
        self.repositories_by_id.borrow().get(&id.to_string()).cloned()
    }

    fn get_repository_by_resource(&self, resource: &URI) -> Option<Rc<dyn ISCMRepository>> {
        let scheme = resource.scheme().to_string_lossy();
        if scheme != schemas::FILE && scheme != schemas::VSCODE_REMOTE {
            return None;
        }

        let ext_uri = self.uri_identity_service.ext_uri();
        let mut best_repository: Option<Rc<dyn ISCMRepository>> = None;
        let mut best_match_length = usize::MAX;

        for repository in self.repositories() {
            let provider = repository.provider();
            if provider.is_hidden() {
                continue;
            }

            let Some(root) = provider.root_uri() else { continue };
            let Some(path) = ext_uri.relative_path(&root, resource) else { continue };

            let length = path.encode_utf16().count();
            if !path.is_empty() && !path.starts_with("..") && length < best_match_length {
                best_repository = Some(repository);
                best_match_length = length;
            }
        }

        best_repository
    }
}
