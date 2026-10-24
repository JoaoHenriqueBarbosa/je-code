//! Porte de `src/vs/workbench/contrib/notebook/browser/services/notebookKernelServiceImpl.ts`: o `NotebookKernelService`, que
//! guarda os kernels registrados, a ligação entre notebook e kernel (com o cache LRU persistido no armazenamento do workspace),
//! as afinidades por notebook, as tarefas de detecção de kernel, as ações de origem (do menu `NotebookKernelSource`) e os
//! provedores de ações de origem.
//!
//! Divergências:
//! - O `@INotebookService`, o `@IStorageService`, o `@IMenuService` e o `@IContextKeyService` do construtor são recebidos por
//!   [`NotebookKernelService::new`], que devolve o serviço em um `Rc` (os ouvintes guardam um `Weak`).
//! - `Map<string, KernelInfo>` (ordem de inserção) é um `HashMap` mais o `time` de cada `KernelInfo` (o `_logicClock`), e a
//!   iteração ordena por `time`, o que dá a mesma ordem.
//! - `LRUCache<string, string>` é o [`LRUCache`] do porte; `JSON.stringify(cache)` (a `toJSON` do cache, a lista de pares
//!   `[chave, valor]`) e o `JSON.parse` do `fromJSON` são feitos à mão com o `json_quote` e o `parse_str`.
//! - `a.kernel.label.localeCompare(b.kernel.label)` compara os rótulos sem diferenciar maiúsculas (e desempata pelo texto
//!   exato), uma aproximação do `Intl.Collator` do motor.
//! - `runWhenWindowIdle(getActiveWindow(), callback, 100)` é o `run_when_global_idle`.
//! - `SourceAction.runAction` é assíncrono: a execução é o [`Promise`] de `action.run`, e a falha vira o `console.warn`
//!   do original (`eprintln!`). O contexto do `action.run` (`{ uri, $mid: MarshalledId.NotebookActionContext }`) é um objeto
//!   com o `uri` como `UriComponents`.
//! - `getKernelSourceActions2` junta as listas com `Promise.all` e achata na ordem dos provedores.
//! - `INotebookKernelService.selectKernelForNotebook(kernel, ...)` aceita `Option` (o `undefined` que o `onWillRemoveNotebookDocument`
//!   passa).
//! - Igualdade de kernel (`===`) é a identidade do `Rc` (o endereço do objeto).
//! - `Array.from(this._notebookBindings)` do descarte do kernel registrado é a cópia dos pares do cache antes do laço.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::actions::ActionRef;
use je_base::async_::run_when_global_idle;
use je_base::dynamic::{Dynamic, ObjectRef, json_quote};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::map::{LRUCache, ResourceMap};
use je_base::marshalling_ids::MarshalledId;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::actions::actions::{IMenu, IMenuService, MenuAction, MenuActionOptions};
use je_platform::actions::menu_ids;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_storage::StorageValue;

use crate::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
use crate::contrib::notebook::common::notebook_kernel_service::{
    IKernelSourceActionProvider, INotebookKernel, INotebookKernelDetectionTask, INotebookKernelMatchResult, INotebookKernelService, INotebookSourceActionChangeEvent,
    INotebookTextModelLike, ISelectedNotebooksChangeEvent, ISourceAction, NotebookTextModelLike,
};
use crate::contrib::notebook::common::notebook_service::INotebookService;

thread_local! {
    /// `KernelInfo._logicClock`.
    static LOGIC_CLOCK: Cell<u64> = const { Cell::new(0) };
}

/// `kernel === other`.
fn same_kernel(a: &Rc<dyn INotebookKernel>, b: &Rc<dyn INotebookKernel>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `KernelInfo`.
struct KernelInfo {
    kernel: Rc<dyn INotebookKernel>,
    time: u64,
    notebook_priorities: RefCell<ResourceMap<i32>>,
}

impl KernelInfo {
    fn new(kernel: Rc<dyn INotebookKernel>) -> Rc<Self> {
        let time = LOGIC_CLOCK.with(|clock| {
            let time = clock.get();
            clock.set(time + 1);
            time
        });
        Rc::new(Self { kernel, time, notebook_priorities: RefCell::new(ResourceMap::new()) })
    }

    fn priority_of(&self, notebook: &URI) -> Option<i32> {
        self.notebook_priorities.borrow().get(notebook).copied()
    }
}

/// `NotebookTextModelLikeId`.
struct NotebookTextModelLikeId;

impl NotebookTextModelLikeId {
    fn str(k: &dyn INotebookTextModelLike) -> String {
        format!("{}/{}", k.notebook_type(), k.uri().to_string(false).to_string_lossy())
    }

    fn obj(s: &str) -> NotebookTextModelLike {
        let idx = s.find('/').unwrap_or(0);
        NotebookTextModelLike { notebook_type: s[..idx].to_string(), uri: URI::parse(&s[idx + 1..], false) }
    }
}

/// `SourceAction`.
struct SourceAction {
    base: Rc<Disposable>,
    action: ActionRef,
    model: NotebookTextModelLike,
    is_primary: bool,
    execution: Rc<RefCell<Option<Promise<()>>>>,
    on_did_change_state: Rc<Emitter<()>>,
}

impl SourceAction {
    fn new(action: ActionRef, model: NotebookTextModelLike, is_primary: bool) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_state = Emitter::<()>::new();
        base.register(on_did_change_state.clone());
        Rc::new(Self { base, action, model, is_primary, execution: Rc::new(RefCell::new(None)), on_did_change_state })
    }

    /// `_runAction()`.
    fn run_action_impl(&self) -> Promise<()> {
        let uri = &self.model.uri;
        let components = ObjectRef::new();
        components.set(&"scheme".into(), Dynamic::String(uri.scheme().clone()));
        components.set(&"authority".into(), Dynamic::String(uri.authority().clone()));
        components.set(&"path".into(), Dynamic::String(uri.path().clone()));
        components.set(&"query".into(), Dynamic::String(uri.query().clone()));
        components.set(&"fragment".into(), Dynamic::String(uri.fragment().clone()));
        let context = ObjectRef::new();
        context.set(&"uri".into(), Dynamic::Object(components));
        context.set(&"$mid".into(), Dynamic::Number(f64::from(MarshalledId::NotebookActionContext as i32)));

        self.action.run(&[Dynamic::Object(context)]).then(|_| ()).catch(|error: Error| {
            eprintln!("Kernel source command failed: {error}");
        })
    }
}

impl ISourceAction for SourceAction {
    fn action(&self) -> ActionRef {
        self.action.clone()
    }

    fn on_did_change_state(&self) -> Event<()> {
        self.on_did_change_state.event()
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }

    fn execution(&self) -> Option<Promise<()>> {
        self.execution.borrow().clone()
    }

    fn run_action(&self) -> Promise<()> {
        if let Some(execution) = self.execution() {
            return execution;
        }

        let execution = self.run_action_impl();
        *self.execution.borrow_mut() = Some(execution.clone());
        self.on_did_change_state.fire(());
        let cell = self.execution.clone();
        let on_did_change_state = self.on_did_change_state.clone();
        execution.then(move |_| {
            *cell.borrow_mut() = None;
            on_did_change_state.fire(());
        })
    }
}

impl IDisposable for SourceAction {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "SourceAction"
    }
}

/// `KernelInfoCache extends DisposableStore`: o `DisposableStore` mais as ações.
struct KernelInfoCache {
    store: Rc<DisposableStore>,
    actions: RefCell<Vec<Rc<dyn ISourceAction>>>,
}

impl KernelInfoCache {
    fn new() -> Rc<Self> {
        Rc::new(Self { store: DisposableStore::new(), actions: RefCell::new(Vec::new()) })
    }
}

impl IDisposable for KernelInfoCache {
    fn dispose(&self) {
        self.store.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "KernelInfoCache"
    }
}

/// `MenuItemAction | SubmenuItemAction` como `IAction`.
fn menu_action_to_action(action: &MenuAction) -> ActionRef {
    match action {
        MenuAction::Item(item) => item.clone(),
        MenuAction::Submenu(submenu) => submenu.as_action(),
    }
}

/// `NotebookKernelService`.
pub struct NotebookKernelService {
    this: Weak<NotebookKernelService>,
    base: Rc<Disposable>,

    notebook_service: Rc<dyn INotebookService>,
    storage_service: Rc<dyn StorageService>,
    menu_service: Rc<dyn IMenuService>,
    context_key_service: Rc<dyn IContextKeyService>,

    kernels: RefCell<HashMap<String, Rc<KernelInfo>>>,
    notebook_bindings: LRUCache<String, String>,

    on_did_change_notebook_kernel_binding: Rc<Emitter<ISelectedNotebooksChangeEvent>>,
    on_did_add_kernel: Rc<Emitter<Rc<dyn INotebookKernel>>>,
    on_did_remove_kernel: Rc<Emitter<Rc<dyn INotebookKernel>>>,
    on_did_change_notebook_affinity: Rc<Emitter<()>>,
    on_did_change_source_actions: Rc<Emitter<INotebookSourceActionChangeEvent>>,
    on_did_notebook_variables_change: Rc<Emitter<URI>>,
    kernel_sources: Rc<DisposableMap<String, KernelInfoCache>>,
    kernel_detection_tasks: RefCell<HashMap<String, Vec<Rc<dyn INotebookKernelDetectionTask>>>>,
    on_did_change_kernel_detection_tasks: Rc<Emitter<String>>,
    kernel_source_action_providers: RefCell<HashMap<String, Vec<Rc<IKernelSourceActionProvider>>>>,
    persist_soon_handle: Rc<MutableDisposable<dyn IDisposable>>,
}

const STORAGE_NOTEBOOK_BINDING: &str = "notebook.controller2NotebookBindings";

impl NotebookKernelService {
    pub fn new(
        notebook_service: Rc<dyn INotebookService>,
        storage_service: Rc<dyn StorageService>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_notebook_kernel_binding = Emitter::<ISelectedNotebooksChangeEvent>::new();
        let on_did_add_kernel = Emitter::<Rc<dyn INotebookKernel>>::new();
        let on_did_remove_kernel = Emitter::<Rc<dyn INotebookKernel>>::new();
        let on_did_change_notebook_affinity = Emitter::<()>::new();
        let on_did_change_source_actions = Emitter::<INotebookSourceActionChangeEvent>::new();
        let on_did_notebook_variables_change = Emitter::<URI>::new();
        let kernel_sources = DisposableMap::<String, KernelInfoCache>::new();
        let on_did_change_kernel_detection_tasks = Emitter::<String>::new();
        let persist_soon_handle = MutableDisposable::<dyn IDisposable>::new();
        base.register(on_did_change_notebook_kernel_binding.clone());
        base.register(on_did_add_kernel.clone());
        base.register(on_did_remove_kernel.clone());
        base.register(on_did_change_notebook_affinity.clone());
        base.register(on_did_change_source_actions.clone());
        base.register(on_did_notebook_variables_change.clone());
        base.register(kernel_sources.clone());
        base.register(on_did_change_kernel_detection_tasks.clone());
        base.register(persist_soon_handle.clone());

        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base,
            notebook_service: notebook_service.clone(),
            storage_service: storage_service.clone(),
            menu_service,
            context_key_service,
            kernels: RefCell::new(HashMap::new()),
            notebook_bindings: LRUCache::new(1000, 0.7),
            on_did_change_notebook_kernel_binding,
            on_did_add_kernel,
            on_did_remove_kernel,
            on_did_change_notebook_affinity,
            on_did_change_source_actions,
            on_did_notebook_variables_change,
            kernel_sources,
            kernel_detection_tasks: RefCell::new(HashMap::new()),
            on_did_change_kernel_detection_tasks,
            kernel_source_action_providers: RefCell::new(HashMap::new()),
            persist_soon_handle,
        });

        // auto associate kernels to new notebook documents, also emit event when
        // a notebook has been closed (but don't update the memento)
        let weak = Rc::downgrade(&this);
        this.base.register(notebook_service.on_did_add_notebook_document().listen(move |notebook| {
            if let Some(this) = weak.upgrade() {
                this.try_auto_bind_notebook(&**notebook, None);
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(notebook_service.on_will_remove_notebook_document().listen(move |notebook| {
            let Some(this) = weak.upgrade() else { return };
            let notebook: &dyn INotebookTextModelLike = &**notebook;
            let id = NotebookTextModelLikeId::str(notebook);
            let kernel_id = this.notebook_bindings.get(&id);
            if kernel_id.is_some() && notebook.uri().scheme().to_string_lossy() == "untitled" {
                this.select_kernel_for_notebook(None, notebook);
            }
            this.kernel_sources.delete_and_dispose(&id);
        }));

        // restore from storage
        let stored = storage_service.get(STORAGE_NOTEBOOK_BINDING, StorageScope::Workspace, Some("[]")).unwrap_or_else(|| "[]".to_string());
        let mut parse_errors = Vec::new();
        // ignore
        if let Some(Value::Array(entries)) = parse_str(&stored, &mut parse_errors).filter(|_| parse_errors.is_empty()) {
            let data: Vec<(String, String)> = entries
                .into_iter()
                .filter_map(|entry| match entry {
                    Value::Array(pair) => match (pair.first(), pair.get(1)) {
                        (Some(Value::String(key)), Some(Value::String(value))) => Some((key.to_string_lossy(), value.to_string_lossy())),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            this.notebook_bindings.from_json(data);
        }

        this
    }

    /// `_persistMementos()`.
    fn persist_mementos(&self) {
        let weak = self.this.clone();
        self.persist_soon_handle.set(Some(run_when_global_idle(
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                let pairs: Vec<String> = this
                    .notebook_bindings
                    .to_json()
                    .iter()
                    .map(|(key, value)| format!("[{},{}]", json_quote(&JsString::from(key.as_str())), json_quote(&JsString::from(value.as_str()))))
                    .collect();
                this.storage_service.store(STORAGE_NOTEBOOK_BINDING, StorageValue::String(format!("[{}]", pairs.join(","))), StorageScope::Workspace, StorageTarget::Machine);
            },
            Some(100),
        )));
    }

    /// `_score(kernel, notebook)`.
    fn score(kernel: &Rc<dyn INotebookKernel>, notebook: &dyn INotebookTextModelLike) -> i32 {
        let view_type = kernel.view_type();
        if view_type == "*" {
            5
        } else if view_type == notebook.notebook_type() {
            10
        } else {
            0
        }
    }

    /// Os `KernelInfo` na ordem de inserção do `Map`.
    fn kernel_infos(&self) -> Vec<Rc<KernelInfo>> {
        let mut infos: Vec<Rc<KernelInfo>> = self.kernels.borrow().values().cloned().collect();
        infos.sort_by_key(|info| info.time);
        infos
    }

    /// `_tryAutoBindNotebook(notebook, onlyThisKernel?)`.
    fn try_auto_bind_notebook(&self, notebook: &dyn INotebookTextModelLike, only_this_kernel: Option<&Rc<dyn INotebookKernel>>) {
        let Some(id) = self.notebook_bindings.get(&NotebookTextModelLikeId::str(notebook)) else {
            // no kernel associated
            return;
        };
        let existing_kernel = self.kernels.borrow().get(&id).cloned();
        let Some(existing_kernel) = existing_kernel.filter(|existing| Self::score(&existing.kernel, notebook) != 0) else {
            // associated kernel not known, not matching
            return;
        };
        if only_this_kernel.is_none_or(|only| same_kernel(&existing_kernel.kernel, only)) {
            self.on_did_change_notebook_kernel_binding.fire(ISelectedNotebooksChangeEvent { notebook: notebook.uri(), old_kernel: None, new_kernel: Some(existing_kernel.kernel.id()) });
        }
    }

    /// `loadActionsFromMenu(menu, document)` de `getSourceActions`.
    fn load_actions_from_menu(&self, menu: &Rc<dyn IMenu>, document: &NotebookTextModelLike, info: &Rc<KernelInfoCache>, action_disposables: &Rc<DisposableStore>) {
        action_disposables.clear();
        let groups = menu.get_actions(Some(&MenuActionOptions { arg: None, args: None, should_forward_args: true, render_short_title: false }));
        let mut source_actions: Vec<Rc<dyn ISourceAction>> = Vec::new();
        for (group_name, actions) in groups {
            let is_primary = group_name.starts_with("primary");
            for action in actions {
                let source_action = action_disposables.add_rc(SourceAction::new(menu_action_to_action(&action), document.clone(), is_primary));
                let weak = self.this.clone();
                let notebook = document.uri.clone();
                let view_type = document.notebook_type.clone();
                action_disposables.add(source_action.on_did_change_state.event().listen(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_change_source_actions.fire(INotebookSourceActionChangeEvent { notebook: Some(notebook.clone()), view_type: view_type.clone() });
                    }
                }));
                source_actions.push(source_action);
            }
        }
        *info.actions.borrow_mut() = source_actions;
        self.on_did_change_source_actions.fire(INotebookSourceActionChangeEvent { notebook: Some(document.uri.clone()), view_type: document.notebook_type.clone() });
    }
}

impl INotebookKernelService for NotebookKernelService {
    fn on_did_add_kernel(&self) -> Event<Rc<dyn INotebookKernel>> {
        self.on_did_add_kernel.event()
    }

    fn on_did_remove_kernel(&self) -> Event<Rc<dyn INotebookKernel>> {
        self.on_did_remove_kernel.event()
    }

    fn on_did_change_selected_notebooks(&self) -> Event<ISelectedNotebooksChangeEvent> {
        self.on_did_change_notebook_kernel_binding.event()
    }

    fn on_did_change_notebook_affinity(&self) -> Event<()> {
        self.on_did_change_notebook_affinity.event()
    }

    fn on_did_notebook_variables_update(&self) -> Event<URI> {
        self.on_did_notebook_variables_change.event()
    }

    fn notify_variables_change(&self, notebook_uri: &URI) {
        self.on_did_notebook_variables_change.fire(notebook_uri.clone());
    }

    fn register_kernel(&self, kernel: Rc<dyn INotebookKernel>) -> DisposableRef {
        if self.kernels.borrow().contains_key(&kernel.id()) {
            errors::throw(Error::new(format!("NOTEBOOK CONTROLLER with id '{}' already exists", kernel.id())));
        }

        self.kernels.borrow_mut().insert(kernel.id(), KernelInfo::new(kernel.clone()));
        self.on_did_add_kernel.fire(kernel.clone());

        // auto associate the new kernel to existing notebooks it was
        // associated to in the past.
        for notebook in self.notebook_service.get_notebook_text_models() {
            self.try_auto_bind_notebook(&*notebook, Some(&kernel));
        }

        let weak = self.this.clone();
        to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let removed = this.kernels.borrow_mut().remove(&kernel.id());
            if removed.is_some() {
                this.on_did_remove_kernel.fire(kernel.clone());
            }
            for (key, candidate) in this.notebook_bindings.to_json() {
                if candidate == kernel.id() {
                    this.on_did_change_notebook_kernel_binding.fire(ISelectedNotebooksChangeEvent {
                        notebook: NotebookTextModelLikeId::obj(&key).uri,
                        old_kernel: Some(kernel.id()),
                        new_kernel: None,
                    });
                }
            }
        })
    }

    fn get_matching_kernel(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelMatchResult {
        // all applicable kernels
        struct Candidate {
            kernel: Rc<dyn INotebookKernel>,
            instance_affinity: i32,
            score: i32,
        }
        let mut kernels: Vec<Candidate> = Vec::new();
        for info in self.kernel_infos() {
            let score = Self::score(&info.kernel, notebook);
            if score != 0 {
                kernels.push(Candidate {
                    score,
                    kernel: info.kernel.clone(),
                    instance_affinity: info.priority_of(&notebook.uri()).unwrap_or(1 /* vscode.NotebookControllerPriority.Default */),
                });
            }
        }

        kernels.sort_by(|a, b| {
            b.instance_affinity
                .cmp(&a.instance_affinity)
                .then(a.score.cmp(&b.score))
                .then_with(|| {
                    let (a_label, b_label) = (a.kernel.label(), b.kernel.label());
                    a_label.to_lowercase().cmp(&b_label.to_lowercase()).then(a_label.cmp(&b_label))
                })
        });
        let all: Vec<Rc<dyn INotebookKernel>> = kernels.iter().map(|candidate| candidate.kernel.clone()).collect();

        // bound kernel
        let selected_id = self.notebook_bindings.get(&NotebookTextModelLikeId::str(notebook));
        let selected = selected_id.and_then(|id| self.kernels.borrow().get(&id).map(|info| info.kernel.clone()));
        let suggestions = kernels.iter().filter(|item| item.instance_affinity > 1).map(|item| item.kernel.clone()).collect();
        let hidden = kernels.iter().filter(|item| item.instance_affinity < 0).map(|item| item.kernel.clone()).collect();
        INotebookKernelMatchResult { all, selected, suggestions, hidden }
    }

    fn get_selected_or_suggested_kernel(&self, notebook: &dyn INotebookTextModelLike) -> Option<Rc<dyn INotebookKernel>> {
        let info = self.get_matching_kernel(notebook);
        if info.selected.is_some() {
            return info.selected;
        }

        let uri = notebook.uri();
        let preferred: Vec<Rc<dyn INotebookKernel>> = info
            .all
            .iter()
            .filter(|kernel| self.kernels.borrow().get(&kernel.id()).and_then(|kernel_info| kernel_info.priority_of(&uri)) == Some(2 /* vscode.NotebookControllerPriority.Preferred */))
            .cloned()
            .collect();
        if preferred.len() == 1 {
            return preferred.into_iter().next();
        }

        if info.all.len() == 1 { info.all.into_iter().next() } else { None }
    }

    // a notebook has one kernel, a kernel has N notebooks
    // notebook <-1----N-> kernel
    fn select_kernel_for_notebook(&self, kernel: Option<&Rc<dyn INotebookKernel>>, notebook: &dyn INotebookTextModelLike) {
        let key = NotebookTextModelLikeId::str(notebook);
        let old_kernel = self.notebook_bindings.get(&key);
        let new_kernel = kernel.map(|kernel| kernel.id());
        if old_kernel != new_kernel {
            match &new_kernel {
                Some(id) => {
                    self.notebook_bindings.set(key, id.clone());
                }
                None => {
                    self.notebook_bindings.delete(&key);
                }
            }
            self.on_did_change_notebook_kernel_binding.fire(ISelectedNotebooksChangeEvent { notebook: notebook.uri(), old_kernel, new_kernel });
            self.persist_mementos();
        }
    }

    fn preselect_kernel_for_notebook(&self, kernel: &Rc<dyn INotebookKernel>, notebook: &dyn INotebookTextModelLike) {
        let key = NotebookTextModelLikeId::str(notebook);
        let old_kernel = self.notebook_bindings.get(&key);
        if old_kernel.as_deref() != Some(kernel.id().as_str()) {
            self.notebook_bindings.set(key, kernel.id());
            self.persist_mementos();
        }
    }

    fn update_kernel_notebook_affinity(&self, kernel: &Rc<dyn INotebookKernel>, notebook: &URI, preference: Option<i32>) {
        let info = self.kernels.borrow().get(&kernel.id()).cloned();
        let Some(info) = info else {
            errors::throw(Error::new(format!("UNKNOWN kernel '{}'", kernel.id())));
        };
        match preference {
            None => {
                info.notebook_priorities.borrow_mut().delete(notebook);
            }
            Some(preference) => {
                info.notebook_priorities.borrow_mut().set(notebook.clone(), preference);
            }
        }
        self.on_did_change_notebook_affinity.fire(());
    }

    fn get_running_source_actions(&self, notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn ISourceAction>> {
        let id = NotebookTextModelLikeId::str(notebook);
        match self.kernel_sources.get(&id) {
            Some(existing_info) => existing_info.actions.borrow().iter().filter(|action| action.execution().is_some()).cloned().collect(),
            None => Vec::new(),
        }
    }

    fn get_source_actions(&self, notebook: &dyn INotebookTextModelLike, context_key_service: Option<Rc<dyn IContextKeyService>>) -> Vec<Rc<dyn ISourceAction>> {
        let context_key_service = context_key_service.unwrap_or_else(|| self.context_key_service.clone());
        let id = NotebookTextModelLikeId::str(notebook);

        if let Some(existing_info) = self.kernel_sources.get(&id) {
            return existing_info.actions.borrow().clone();
        }

        let info = KernelInfoCache::new();
        let source_menu = self.menu_service.create_menu(&menu_ids::notebook_kernel_source(), context_key_service, None);
        info.store.add(source_menu.clone());
        let action_disposables = DisposableStore::new();
        info.store.add(action_disposables.clone());
        self.kernel_sources.set(id, info.clone(), false);

        let document = NotebookTextModelLike { uri: notebook.uri(), notebook_type: notebook.notebook_type() };

        let weak = self.this.clone();
        let (menu, info_ref, disposables_ref, document_ref) = (source_menu.clone(), info.clone(), action_disposables.clone(), document.clone());
        info.store.add(source_menu.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.load_actions_from_menu(&menu, &document_ref, &info_ref, &disposables_ref);
            }
        }));

        self.load_actions_from_menu(&source_menu, &document, &info, &action_disposables);

        let actions = info.actions.borrow().clone();
        actions
    }

    fn register_notebook_kernel_detection_task(&self, task: Rc<dyn INotebookKernelDetectionTask>) -> DisposableRef {
        let notebook_type = task.notebook_type();
        self.kernel_detection_tasks.borrow_mut().entry(notebook_type.clone()).or_default().push(task.clone());
        self.on_did_change_kernel_detection_tasks.fire(notebook_type.clone());
        let weak = self.this.clone();
        to_disposable(move || {
            let Some(this) = weak.upgrade() else { return };
            let removed = {
                let mut all = this.kernel_detection_tasks.borrow_mut();
                let tasks = all.entry(notebook_type.clone()).or_default();
                match tasks.iter().position(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), Rc::as_ptr(&task))) {
                    Some(idx) => {
                        tasks.remove(idx);
                        true
                    }
                    None => false,
                }
            };
            if removed {
                this.on_did_change_kernel_detection_tasks.fire(notebook_type.clone());
            }
        })
    }

    fn on_did_change_kernel_detection_tasks(&self) -> Event<String> {
        self.on_did_change_kernel_detection_tasks.event()
    }

    fn get_kernel_detection_tasks(&self, notebook: &dyn INotebookTextModelLike) -> Vec<Rc<dyn INotebookKernelDetectionTask>> {
        self.kernel_detection_tasks.borrow().get(&notebook.notebook_type()).cloned().unwrap_or_default()
    }

    fn on_did_change_source_actions(&self) -> Event<INotebookSourceActionChangeEvent> {
        self.on_did_change_source_actions.event()
    }

    fn register_kernel_source_action_provider(&self, view_type: &str, provider: Rc<IKernelSourceActionProvider>) -> DisposableRef {
        self.kernel_source_action_providers.borrow_mut().entry(view_type.to_string()).or_default().push(provider.clone());
        self.on_did_change_source_actions.fire(INotebookSourceActionChangeEvent { notebook: None, view_type: view_type.to_string() });

        let event_emitter_disposable = provider.on_did_change_source_actions.as_ref().map(|event| {
            let weak = self.this.clone();
            let view_type = view_type.to_string();
            event.listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_source_actions.fire(INotebookSourceActionChangeEvent { notebook: None, view_type: view_type.clone() });
                }
            })
        });

        let weak = self.this.clone();
        let view_type = view_type.to_string();
        to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                let mut all = this.kernel_source_action_providers.borrow_mut();
                let providers = all.entry(view_type.clone()).or_default();
                if let Some(idx) = providers.iter().position(|candidate| Rc::ptr_eq(candidate, &provider)) {
                    providers.remove(idx);
                }
            }

            if let Some(event_emitter_disposable) = &event_emitter_disposable {
                event_emitter_disposable.dispose();
            }
        })
    }

    /// Get kernel source actions from providers.
    fn get_kernel_source_actions2(&self, notebook: &dyn INotebookTextModelLike) -> Promise<Vec<INotebookKernelSourceAction>> {
        let providers = self.kernel_source_action_providers.borrow().get(&notebook.notebook_type()).cloned().unwrap_or_default();
        let promises: Vec<Promise<Vec<INotebookKernelSourceAction>>> = providers.iter().map(|provider| (provider.provide_kernel_source_actions)()).collect();
        Promise::all(promises).then(|actions| actions.into_iter().flatten().collect())
    }
}

impl IDisposable for NotebookKernelService {
    fn dispose(&self) {
        self.kernels.borrow_mut().clear();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookKernelService"
    }
}
