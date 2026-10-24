//! Porte de `src/vs/workbench/api/browser/mainThreadNotebookKernels.ts`: a ponte entre o `INotebookKernelService` do workbench e
//! os controladores de notebook do host de extensões (kernels, execuções de célula e de notebook, tarefas de detecção, ações de
//! origem, variáveis e mensagens do renderizador).
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadNotebookKernels)` e o `IExtHostContext` do construtor: o registro de clientes
//!   não existe ainda; o construtor recebe o [`IMainContext`] (que dá o `getProxy`) e as cinco dependências decoradas.
//! - `INotebookEditor` é o [`NotebookEditorWidget`]. O `DisposableMap<INotebookEditor, IDisposable>` dos editores é um mapa por
//!   id de editor (o `getId()` é único), com a mesma troca e descarte; o `$postMessage` percorre os editores na ordem de entrada.
//! - `MainThreadKernel` (classe abstrata) e a subclasse anônima de `$addKernel` são um struct só, [`MainThreadKernel`]: os três
//!   métodos abstratos chamam o proxy com o `handle` do kernel e o `Weak` do `that`. Os campos mutáveis ficam em células.
//! - `$updateKernel(handle, data: Partial<INotebookKernelDto2>)` recebe o [`INotebookKernelDto2`] inteiro (ver
//!   `ext_host_protocol`): o `label` é sempre aplicado, e os campos `Option` só quando presentes.
//! - `$updateNotebookPriority(handle, notebook, value)` converte o `NotebookControllerAffinity2` no número do `preference`.
//! - A ação de origem devolvida por `provideKernelSourceActions` mantém o `documentation` como veio (os `UriComponents` do
//!   [`INotebookKernelSourceAction`] não têm uma forma revivida; o `URI.revive` do original é, aqui, a identidade).
//! - Os `throw` de `$createExecution`, `$createNotebookExecution` e do ramo de `$postMessage` (que é `async`, então vira uma
//!   promessa rejeitada) são `errors::throw` ou [`Promise::rejected`]; o `try/catch` com `onUnexpectedError` é `errors::catch`.
//! - `emitter instanceof Emitter` de `$emitNotebookKernelSourceActionsChangeEvent` é sempre verdadeiro: o mapa só guarda emissores.
//! - O `provideVariables` do kernel cria o `AsyncIterableProducer` cujo executor registra o emissor em `variableRequestMap`, espera
//!   `$provideVariables` e tira o emissor do mapa no `finally`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{AsyncIterableEmitter, AsyncIterableProducer, async_fn};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_extensions::manifest::ExtensionIdentifier;

use super::main_thread_notebook_dto::{from_cell_execute_complete_dto, from_cell_execute_update_dto};
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookKernelsShape, ICellExecuteUpdateDto, ICellExecutionCompleteDto, IMainContext, INotebookKernelDto2, MainThreadNotebookKernelsShape,
    SerializableObjectWithBuffers, VariablesRequestKindDto, VariablesResult,
};
use crate::api::common::ext_host_types::NotebookControllerAffinity2;
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::notebook::browser::services::notebook_editor_service::INotebookEditorService;
use crate::contrib::notebook::common::notebook_common::INotebookKernelSourceAction;
use crate::contrib::notebook::common::notebook_execution_state_service::{INotebookCellExecution, INotebookExecution, INotebookExecutionStateService};
use crate::contrib::notebook::common::notebook_kernel_service::{
    IKernelSourceActionProvider, INotebookKernel, INotebookKernelChangeEvent, INotebookKernelDetectionTask, INotebookKernelService, INotebookTextModelLike,
};
use crate::contrib::notebook::common::notebook_service::INotebookService;

/// `MainThreadKernel` e a subclasse anônima de `$addKernel`.
pub struct MainThreadKernel {
    handle: u32,
    that: Weak<MainThreadNotebookKernels>,
    language_service: Rc<dyn ILanguageService>,
    on_did_change: Rc<Emitter<INotebookKernelChangeEvent>>,
    preloads: Vec<(URI, Vec<String>)>,

    id: String,
    view_type: String,
    extension: ExtensionIdentifier,
    implements_interrupt: Cell<bool>,
    label: RefCell<String>,
    description: RefCell<Option<String>>,
    detail: RefCell<Option<String>>,
    supported_languages: RefCell<Vec<String>>,
    implements_execution_order: Cell<bool>,
    has_variable_provider: Cell<bool>,
    local_resource_root: URI,
}

impl MainThreadKernel {
    fn new(handle: u32, that: Weak<MainThreadNotebookKernels>, data: &INotebookKernelDto2, language_service: Rc<dyn ILanguageService>) -> Rc<MainThreadKernel> {
        let supported_languages = match &data.supported_languages {
            Some(languages) if !languages.is_empty() => languages.clone(),
            _ => language_service.get_registered_language_ids(),
        };
        Rc::new(MainThreadKernel {
            handle,
            that,
            on_did_change: Emitter::new(),
            preloads: data.preloads.as_ref().map(|preloads| preloads.iter().map(|u| (u.uri.clone(), u.provides.clone())).collect()).unwrap_or_default(),
            id: data.id.clone(),
            view_type: data.notebook_type.clone(),
            extension: data.extension_id.clone(),
            implements_interrupt: Cell::new(data.supports_interrupt.unwrap_or(false)),
            label: RefCell::new(data.label.clone()),
            description: RefCell::new(data.description.clone()),
            detail: RefCell::new(data.detail.clone()),
            supported_languages: RefCell::new(supported_languages),
            implements_execution_order: Cell::new(data.supports_execution_order.unwrap_or(false)),
            has_variable_provider: Cell::new(data.has_variable_provider.unwrap_or(false)),
            local_resource_root: data.extension_location.clone(),
            language_service,
        })
    }

    /// `update(data)`.
    fn update(&self, data: &INotebookKernelDto2) {
        let mut event = INotebookKernelChangeEvent::default();
        *self.label.borrow_mut() = data.label.clone();
        event.label = true;
        if let Some(description) = &data.description {
            *self.description.borrow_mut() = Some(description.clone());
            event.description = true;
        }
        if let Some(detail) = &data.detail {
            *self.detail.borrow_mut() = Some(detail.clone());
            event.detail = true;
        }
        if let Some(supported_languages) = &data.supported_languages {
            *self.supported_languages.borrow_mut() = if supported_languages.is_empty() { self.language_service.get_registered_language_ids() } else { supported_languages.clone() };
            event.supported_languages = true;
        }
        if let Some(supports_execution_order) = data.supports_execution_order {
            self.implements_execution_order.set(supports_execution_order);
            event.has_execution_order = true;
        }
        if let Some(supports_interrupt) = data.supports_interrupt {
            self.implements_interrupt.set(supports_interrupt);
            event.has_interrupt_handler = true;
        }
        if let Some(has_variable_provider) = data.has_variable_provider {
            self.has_variable_provider.set(has_variable_provider);
            event.has_variable_provider = true;
        }
        self.on_did_change.fire(event);
    }

    /// O `that` do `$addKernel`.
    fn that(&self) -> Rc<MainThreadNotebookKernels> {
        self.that.upgrade().expect("o MainThreadNotebookKernels está vivo enquanto o kernel roda")
    }
}

impl IDisposable for MainThreadKernel {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

impl INotebookKernel for MainThreadKernel {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn on_did_change(&self) -> Event<INotebookKernelChangeEvent> {
        self.on_did_change.event()
    }

    fn extension(&self) -> ExtensionIdentifier {
        self.extension.clone()
    }

    fn local_resource_root(&self) -> URI {
        self.local_resource_root.clone()
    }

    /// `get preloadUris()`.
    fn preload_uris(&self) -> Vec<URI> {
        self.preloads.iter().map(|(uri, _)| uri.clone()).collect()
    }

    /// `get preloadProvides()`.
    fn preload_provides(&self) -> Vec<String> {
        self.preloads.iter().flat_map(|(_, provides)| provides.iter().cloned()).collect()
    }

    fn label(&self) -> String {
        self.label.borrow().clone()
    }

    fn description(&self) -> Option<String> {
        self.description.borrow().clone()
    }

    fn detail(&self) -> Option<String> {
        self.detail.borrow().clone()
    }

    fn supported_languages(&self) -> Vec<String> {
        self.supported_languages.borrow().clone()
    }

    fn implements_interrupt(&self) -> Option<bool> {
        Some(self.implements_interrupt.get())
    }

    fn implements_execution_order(&self) -> Option<bool> {
        Some(self.implements_execution_order.get())
    }

    fn has_variable_provider(&self) -> Option<bool> {
        Some(self.has_variable_provider.get())
    }

    fn execute_notebook_cells_request(&self, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        let (that, handle, uri) = (self.that(), self.handle, uri.clone());
        async_fn(async move { that.proxy.rpc_execute_cells(handle, &uri, handles).await })
    }

    fn cancel_notebook_cell_execution(&self, uri: &URI, handles: Vec<i64>) -> Promise<()> {
        let (that, handle, uri) = (self.that(), self.handle, uri.clone());
        async_fn(async move { that.proxy.rpc_cancel_cells(handle, &uri, handles).await })
    }

    fn provide_variables(
        &self,
        notebook_uri: &URI,
        parent_id: Option<u32>,
        kind: VariablesRequestKindDto,
        start: usize,
        token: CancellationToken,
    ) -> Rc<AsyncIterableProducer<VariablesResult>> {
        let that = self.that();
        let handle = self.handle;
        let request_id = format!("{handle}variables{}", that.variable_request_index.get());
        that.variable_request_index.set(that.variable_request_index.get() + 1);
        let notebook_uri = notebook_uri.clone();

        AsyncIterableProducer::new(
            move |emitter: AsyncIterableEmitter<VariablesResult>| {
                that.variable_request_map.borrow_mut().insert(request_id.clone(), emitter);

                MaybePromise::Promise(async_fn(async move {
                    let provided = that.proxy.rpc_provide_variables(handle, &request_id, &notebook_uri, parent_id, kind, start, token).await;
                    that.variable_request_map.borrow_mut().remove(&request_id);
                    provided
                }))
            },
            None,
        )
    }
}

/// `MainThreadKernelDetectionTask`.
struct MainThreadKernelDetectionTask {
    notebook_type: String,
}

impl INotebookKernelDetectionTask for MainThreadKernelDetectionTask {
    fn notebook_type(&self) -> String {
        self.notebook_type.clone()
    }
}

/// `MainThreadNotebookKernels`.
pub struct MainThreadNotebookKernels {
    this: Weak<MainThreadNotebookKernels>,
    editors: RefCell<IndexMap<String, (Rc<dyn NotebookEditorWidget>, DisposableRef)>>,
    disposables: Rc<DisposableStore>,

    kernels: RefCell<HashMap<u32, (Rc<MainThreadKernel>, Rc<DisposableStore>)>>,
    kernel_detection_tasks: RefCell<HashMap<u32, (Rc<MainThreadKernelDetectionTask>, DisposableRef)>>,
    kernel_source_action_providers: RefCell<HashMap<u32, (Rc<IKernelSourceActionProvider>, DisposableRef)>>,
    kernel_source_action_providers_event_registrations: RefCell<HashMap<u32, Rc<Emitter<()>>>>,

    proxy: Rc<dyn ExtHostNotebookKernelsShape>,

    executions: Rc<RefCell<HashMap<u32, Rc<dyn INotebookCellExecution>>>>,
    notebook_executions: Rc<RefCell<HashMap<u32, Rc<dyn INotebookExecution>>>>,

    language_service: Rc<dyn ILanguageService>,
    notebook_kernel_service: Rc<dyn INotebookKernelService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    notebook_service: Rc<dyn INotebookService>,

    variable_request_index: Cell<u32>,
    variable_request_map: RefCell<HashMap<String, AsyncIterableEmitter<VariablesResult>>>,
}

impl MainThreadNotebookKernels {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        language_service: Rc<dyn ILanguageService>,
        notebook_kernel_service: Rc<dyn INotebookKernelService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
        notebook_service: Rc<dyn INotebookService>,
        notebook_editor_service: Rc<dyn INotebookEditorService>,
    ) -> Rc<MainThreadNotebookKernels> {
        let service = Rc::new_cyclic(|this| MainThreadNotebookKernels {
            this: this.clone(),
            editors: RefCell::new(IndexMap::new()),
            disposables: DisposableStore::new(),
            kernels: RefCell::new(HashMap::new()),
            kernel_detection_tasks: RefCell::new(HashMap::new()),
            kernel_source_action_providers: RefCell::new(HashMap::new()),
            kernel_source_action_providers_event_registrations: RefCell::new(HashMap::new()),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_KERNELS),
            executions: Rc::default(),
            notebook_executions: Rc::default(),
            language_service,
            notebook_kernel_service,
            notebook_execution_state_service,
            notebook_service,
            variable_request_index: Cell::new(0),
            variable_request_map: RefCell::new(HashMap::new()),
        });

        for editor in notebook_editor_service.list_notebook_editors() {
            service.on_editor_add(editor);
        }
        let this = service.this.clone();
        service.disposables.add(notebook_editor_service.on_did_add_notebook_editor().listen(move |editor: &Rc<dyn NotebookEditorWidget>| {
            if let Some(this) = this.upgrade() {
                this.on_editor_add(editor.clone());
            }
        }));
        let this = service.this.clone();
        service.disposables.add(notebook_editor_service.on_did_remove_notebook_editor().listen(move |editor: &Rc<dyn NotebookEditorWidget>| {
            if let Some(this) = this.upgrade() {
                this.on_editor_remove(editor);
            }
        }));

        let (executions, notebook_executions) = (service.executions.clone(), service.notebook_executions.clone());
        service.disposables.add(to_disposable(move || {
            // EH shut down, complete all executions started by this EH
            let executions: Vec<Rc<dyn INotebookCellExecution>> = executions.borrow().values().cloned().collect();
            for execution in executions {
                execution.complete(Default::default());
            }
            let notebook_executions: Vec<Rc<dyn INotebookExecution>> = notebook_executions.borrow().values().cloned().collect();
            for execution in notebook_executions {
                execution.complete();
            }
        }));

        let this = service.this.clone();
        service.disposables.add(service.notebook_kernel_service.on_did_change_selected_notebooks().listen(move |e| {
            let Some(this) = this.upgrade() else { return };
            let kernels: Vec<(u32, Rc<MainThreadKernel>)> = this.kernels.borrow().iter().map(|(handle, (kernel, _))| (*handle, kernel.clone())).collect();
            for (handle, kernel) in kernels {
                if e.old_kernel.as_deref() == Some(kernel.id.as_str()) {
                    this.proxy.rpc_accept_notebook_association(handle, &e.notebook, false);
                } else if e.new_kernel.as_deref() == Some(kernel.id.as_str()) {
                    this.proxy.rpc_accept_notebook_association(handle, &e.notebook, true);
                }
            }
        }));

        service
    }

    // --- kernel ipc

    fn on_editor_add(&self, editor: Rc<dyn NotebookEditorWidget>) {
        let this = self.this.clone();
        let listening_editor = editor.clone();
        let ipc_listener = editor.on_did_receive_message().listen(move |e| {
            let Some(this) = this.upgrade() else { return };
            let Some(text_model) = listening_editor.text_model().filter(|_| listening_editor.has_model()) else {
                return;
            };
            let Some(selected) = this.notebook_kernel_service.get_matching_kernel(&*text_model as &dyn INotebookTextModelLike).selected else {
                return;
            };
            let candidates: Vec<(u32, Rc<MainThreadKernel>)> = this.kernels.borrow().iter().map(|(handle, (kernel, _))| (*handle, kernel.clone())).collect();
            for (handle, candidate) in candidates {
                if std::ptr::addr_eq(Rc::as_ptr(&candidate), Rc::as_ptr(&selected)) {
                    this.proxy.rpc_accept_kernel_message_from_renderer(handle, &listening_editor.get_id(), e.message.clone());
                    break;
                }
            }
        });
        let previous = self.editors.borrow_mut().insert(editor.get_id(), (editor, ipc_listener));
        if let Some((_, previous_listener)) = previous {
            previous_listener.dispose();
        }
    }

    fn on_editor_remove(&self, editor: &Rc<dyn NotebookEditorWidget>) {
        let removed = self.editors.borrow_mut().shift_remove(&editor.get_id());
        if let Some((_, listener)) = removed {
            listener.dispose();
        }
    }

    // --- Cell execution

    /// O que `$createExecution` e `$createNotebookExecution` fazem antes de criar a execução: acha o notebook, confere o kernel.
    fn check_selected_kernel(&self, raw_uri: &URI, controller_id: &str) -> URI {
        let uri = raw_uri.clone();
        let Some(notebook) = self.notebook_service.get_notebook_text_model(&uri) else {
            errors::throw(Error::new(format!("Notebook not found: {}", uri.to_string(false).to_string_lossy())));
        };

        let kernel = self.notebook_kernel_service.get_matching_kernel(&*notebook as &dyn INotebookTextModelLike);
        match &kernel.selected {
            Some(selected) if selected.id() == controller_id => {}
            selected => {
                let selected_id = selected.as_ref().map(|selected| selected.id()).unwrap_or_else(|| "undefined".to_owned());
                errors::throw(Error::new(format!("Kernel is not selected: {selected_id} !== {controller_id}")));
            }
        }
        uri
    }
}

impl IDisposable for MainThreadNotebookKernels {
    fn dispose(&self) {
        self.disposables.dispose();
        let registrations: Vec<Rc<DisposableStore>> = self.kernels.borrow().values().map(|(_, registration)| registration.clone()).collect();
        for registration in registrations {
            registration.dispose();
        }
        let registrations: Vec<DisposableRef> = self.kernel_detection_tasks.borrow().values().map(|(_, registration)| registration.clone()).collect();
        for registration in registrations {
            registration.dispose();
        }
        let registrations: Vec<DisposableRef> = self.kernel_source_action_providers.borrow().values().map(|(_, registration)| registration.clone()).collect();
        for registration in registrations {
            registration.dispose();
        }
        let editors: Vec<(Rc<dyn NotebookEditorWidget>, DisposableRef)> = self.editors.borrow_mut().drain(..).map(|(_, entry)| entry).collect();
        for (_, listener) in editors {
            listener.dispose();
        }
    }
}

impl MainThreadNotebookKernelsShape for MainThreadNotebookKernels {
    fn rpc_post_message(&self, handle: u32, editor_id: Option<&str>, message: Dynamic) -> Promise<bool> {
        let Some(kernel) = self.kernels.borrow().get(&handle).map(|(kernel, _)| kernel.clone()) else {
            return Promise::rejected(Error::new("kernel already disposed"));
        };
        let mut did_send = false;
        let editors: Vec<Rc<dyn NotebookEditorWidget>> = self.editors.borrow().values().map(|(editor, _)| editor.clone()).collect();
        for editor in editors {
            let Some(text_model) = editor.text_model().filter(|_| editor.has_model()) else {
                continue;
            };
            let selected = self.notebook_kernel_service.get_matching_kernel(&*text_model as &dyn INotebookTextModelLike).selected;
            if !selected.is_some_and(|selected| std::ptr::addr_eq(Rc::as_ptr(&selected), Rc::as_ptr(&kernel))) {
                // different kernel
                continue;
            }
            match editor_id {
                None => {
                    // all editors
                    editor.post_message(message.clone());
                    did_send = true;
                }
                Some(editor_id) if editor.get_id() == editor_id => {
                    // selected editors
                    editor.post_message(message.clone());
                    did_send = true;
                    break;
                }
                Some(_) => {}
            }
        }
        Promise::resolved(did_send)
    }

    fn rpc_receive_variable(&self, request_id: &str, variable: VariablesResult) {
        let emitter = self.variable_request_map.borrow().get(request_id).cloned();
        if let Some(emitter) = emitter {
            emitter.emit_one(variable);
        }
    }

    // --- kernel adding/updating/removal

    fn rpc_add_kernel(&self, handle: u32, data: INotebookKernelDto2) -> Promise<()> {
        let kernel = MainThreadKernel::new(handle, self.this.clone(), &data, self.language_service.clone());

        let disposables = DisposableStore::new();
        self.disposables.add(disposables.clone());
        // Ensure _kernels is up to date before we register a kernel.
        self.kernels.borrow_mut().insert(handle, (kernel.clone(), disposables.clone()));
        let registered: Rc<dyn INotebookKernel> = kernel;
        disposables.add(self.notebook_kernel_service.register_kernel(registered));
        Promise::resolved(())
    }

    fn rpc_update_kernel(&self, handle: u32, data: INotebookKernelDto2) {
        let kernel = self.kernels.borrow().get(&handle).map(|(kernel, _)| kernel.clone());
        if let Some(kernel) = kernel {
            kernel.update(&data);
        }
    }

    fn rpc_remove_kernel(&self, handle: u32) {
        let tuple = self.kernels.borrow_mut().remove(&handle);
        if let Some((_, registration)) = tuple {
            registration.dispose();
        }
    }

    fn rpc_update_notebook_priority(&self, handle: u32, notebook: &URI, value: Option<NotebookControllerAffinity2>) {
        let kernel = self.kernels.borrow().get(&handle).map(|(kernel, _)| kernel.clone());
        if let Some(kernel) = kernel {
            let kernel: Rc<dyn INotebookKernel> = kernel;
            self.notebook_kernel_service.update_kernel_notebook_affinity(&kernel, notebook, value.map(NotebookControllerAffinity2::value));
        }
    }

    // --- Cell execution

    fn rpc_create_execution(&self, handle: u32, controller_id: &str, raw_uri: &URI, cell_handle: i64) {
        let uri = self.check_selected_kernel(raw_uri, controller_id);
        let execution = self.notebook_execution_state_service.create_cell_execution(&uri, cell_handle);
        execution.confirm();
        self.executions.borrow_mut().insert(handle, execution);
    }

    fn rpc_update_execution(&self, handle: u32, data: SerializableObjectWithBuffers<Vec<ICellExecuteUpdateDto>>) {
        let updates = data.value;
        if let Err(e) = errors::catch(|| {
            let execution = self.executions.borrow().get(&handle).cloned();
            if let Some(execution) = execution {
                execution.update(updates.iter().map(from_cell_execute_update_dto).collect());
            }
        }) {
            on_unexpected_error(&e);
        }
    }

    fn rpc_complete_execution(&self, handle: u32, data: SerializableObjectWithBuffers<ICellExecutionCompleteDto>) {
        if let Err(e) = errors::catch(|| {
            let execution = self.executions.borrow().get(&handle).cloned();
            if let Some(execution) = execution {
                execution.complete(from_cell_execute_complete_dto(&data.value));
            }
        }) {
            on_unexpected_error(&e);
        }
        self.executions.borrow_mut().remove(&handle);
    }

    // --- Notebook execution

    fn rpc_create_notebook_execution(&self, handle: u32, controller_id: &str, raw_uri: &URI) {
        let uri = self.check_selected_kernel(raw_uri, controller_id);
        let execution = self.notebook_execution_state_service.create_execution(&uri);
        execution.confirm();
        self.notebook_executions.borrow_mut().insert(handle, execution);
    }

    fn rpc_begin_notebook_execution(&self, handle: u32) {
        if let Err(e) = errors::catch(|| {
            let execution = self.notebook_executions.borrow().get(&handle).cloned();
            if let Some(execution) = execution {
                execution.begin();
            }
        }) {
            on_unexpected_error(&e);
        }
    }

    fn rpc_complete_notebook_execution(&self, handle: u32) {
        if let Err(e) = errors::catch(|| {
            let execution = self.notebook_executions.borrow().get(&handle).cloned();
            if let Some(execution) = execution {
                execution.complete();
            }
        }) {
            on_unexpected_error(&e);
        }
        self.notebook_executions.borrow_mut().remove(&handle);
    }

    // --- notebook kernel detection task

    fn rpc_add_kernel_detection_task(&self, handle: u32, notebook_type: &str) -> Promise<()> {
        let kernel_detection_task = Rc::new(MainThreadKernelDetectionTask { notebook_type: notebook_type.to_owned() });
        let registration = self.notebook_kernel_service.register_notebook_kernel_detection_task(kernel_detection_task.clone());
        self.kernel_detection_tasks.borrow_mut().insert(handle, (kernel_detection_task, registration));
        Promise::resolved(())
    }

    fn rpc_remove_kernel_detection_task(&self, handle: u32) {
        let tuple = self.kernel_detection_tasks.borrow_mut().remove(&handle);
        if let Some((_, registration)) = tuple {
            registration.dispose();
        }
    }

    // --- notebook kernel source action provider

    fn rpc_add_kernel_source_action_provider(&self, handle: u32, event_handle: u32, notebook_type: &str) -> Promise<()> {
        let proxy = self.proxy.clone();
        let mut kernel_source_action_provider = IKernelSourceActionProvider {
            view_type: notebook_type.to_owned(),
            on_did_change_source_actions: None,
            provide_kernel_source_actions: Rc::new(move || {
                let proxy = proxy.clone();
                async_fn(async move {
                    let actions = proxy.rpc_provide_kernel_source_actions(handle, CancellationToken::None).await?;

                    Ok(actions
                        .into_iter()
                        .map(|action| INotebookKernelSourceAction {
                            label: action.label,
                            command: action.command,
                            description: action.description,
                            detail: action.detail,
                            documentation: action.documentation,
                        })
                        .collect())
                })
            }),
        };

        // O `typeof eventHandle === 'number'` do original é sempre verdadeiro com o `u32` do porte.
        let emitter: Rc<Emitter<()>> = Emitter::new();
        self.kernel_source_action_providers_event_registrations.borrow_mut().insert(event_handle, emitter.clone());
        kernel_source_action_provider.on_did_change_source_actions = Some(emitter.event());

        let provider = Rc::new(kernel_source_action_provider);
        let registration = self.notebook_kernel_service.register_kernel_source_action_provider(notebook_type, provider.clone());
        self.kernel_source_action_providers.borrow_mut().insert(handle, (provider, registration));
        Promise::resolved(())
    }

    fn rpc_remove_kernel_source_action_provider(&self, handle: u32, event_handle: u32) {
        let tuple = self.kernel_source_action_providers.borrow_mut().remove(&handle);
        if let Some((_, registration)) = tuple {
            registration.dispose();
        }
        self.kernel_source_action_providers_event_registrations.borrow_mut().remove(&event_handle);
    }

    fn rpc_emit_notebook_kernel_source_actions_change_event(&self, event_handle: u32) {
        let emitter = self.kernel_source_action_providers_event_registrations.borrow().get(&event_handle).cloned();
        if let Some(emitter) = emitter {
            emitter.fire(());
        }
    }

    fn rpc_variables_updated(&self, notebook_uri: &URI) {
        self.notebook_kernel_service.notify_variables_change(notebook_uri);
    }
}
