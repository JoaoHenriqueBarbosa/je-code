//! Porte de `src/vs/workbench/api/browser/mainThreadNotebookDocumentsAndEditors.ts`: o estado de documentos e editores de notebook
//! (`NotebookAndEditorState` e o `INotebookAndEditorDelta`) e o `MainThreadNotebooksAndEditors`, que o traduz em deltas para o
//! host de extensões e cria o [`MainThreadNotebookDocuments`] e o [`MainThreadNotebookEditors`].
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IInstantiationService, @INotebookService, @INotebookEditorService,
//!   @IEditorService, @IEditorGroupsService, @ILogService)` é [`MainThreadNotebooksAndEditors::new`]. Como o porte não cria as duas
//!   partes por `createInstance`, o construtor recebe também os serviços que elas pedem (o `INotebookEditorModelResolverService`,
//!   o `IUriIdentityService` e o `IConfigurationService`), na ordem em que as partes os recebem. O `@extHostCustomer` não existe: o
//!   objeto é criado direto, e o `extHostContext.set(MainContext.X, ...)` é o `set` do [`IMainContext`].
//! - **Editores.** `IActiveNotebookEditor` é o `INotebookEditor` com modelo (o estado só guarda editores com `hasModel()`); a
//!   identidade (`===`) é a do objeto, comparada pelo endereço. `getViewModel().viewType` é [`INotebookEditor::view_type`].
//! - **Conjuntos e mapas.** O `Set<NotebookTextModel>` é a lista de modelos comparados pela identidade do `Rc`; os `Map<string,
//!   IActiveNotebookEditor>` são listas de pares na ordem de inserção (o que o `diffMaps` e a ordem de `[...map]` exigem).
//! - **`newActiveEditor`.** `string | null | undefined` é `Option<Option<String>>` (`None` é `undefined`); o `activeEditor` do
//!   estado é `Option<String>` (`None` é `null`).
//! - **`_handleEditorAdd`/`_updateState(focusedEditor)`.** Os ouvintes do editor guardam um `Weak` dele e do objeto, para o
//!   editor não prender o `MainThreadNotebooksAndEditors`. O `DisposableMap` dos ouvintes guarda o `combinedDisposable`.
//! - **`textModel`/`activeEditorPane`.** `focusedEditor?.textModel` é `text_model().is_some()`; o painel ativo vem do
//!   `IEditorService::active_editor_pane`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::collections::diff_maps;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, DisposableMap, IDisposable, combined_disposable};
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::{LogArg, LogService, Logger};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_notebook_documents::MainThreadNotebookDocuments;
use crate::api::browser::main_thread_notebook_dto::to_notebook_cell_dto;
use crate::api::browser::main_thread_notebook_editors::MainThreadNotebookEditors;
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookShape, IMainContext, INotebookDocumentsAndEditorsDelta, INotebookEditorAddData, INotebookModelAddedData, MainContext,
    MainThreadNotebookDocumentsShape, MainThreadNotebookEditorsShape, SerializableObjectWithBuffers,
};
use crate::api::common::ext_host_type_converters::CellRange;
use crate::contrib::notebook::browser::notebook_browser::{INotebookEditor, get_notebook_editor_from_editor_pane};
use crate::contrib::notebook::browser::services::notebook_editor_service::INotebookEditorService;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_editor_model_resolver_service::INotebookEditorModelResolverService;
use crate::contrib::notebook::common::notebook_service::INotebookService;
use crate::services::editor::editor_group_column::editor_group_to_column;
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService};
use crate::services::editor::editor_service::IEditorService;

/// `INotebookAndEditorDelta`.
struct INotebookAndEditorDelta {
    removed_documents: Vec<URI>,
    added_documents: Vec<Rc<NotebookTextModel>>,
    removed_editors: Vec<String>,
    added_editors: Vec<Rc<dyn INotebookEditor>>,
    new_active_editor: Option<Option<String>>,
    visible_editors: Option<Vec<String>>,
}

/// O endereço de um objeto compartilhado, para comparar pela identidade (o `===` do original).
fn address<T: ?Sized>(value: &Rc<T>) -> usize {
    Rc::as_ptr(value) as *const () as usize
}

/// `NotebookAndEditorState`.
struct NotebookAndEditorState {
    documents: Vec<Rc<NotebookTextModel>>,
    text_editors: Vec<(String, Rc<dyn INotebookEditor>)>,
    active_editor: Option<String>,
    visible_editors: Vec<(String, Rc<dyn INotebookEditor>)>,
}

impl NotebookAndEditorState {
    /// `NotebookAndEditorState.delta(before, after)`.
    fn delta(before: Option<&NotebookAndEditorState>, after: &NotebookAndEditorState) -> INotebookAndEditorDelta {
        let Some(before) = before else {
            return INotebookAndEditorDelta {
                added_documents: after.documents.clone(),
                removed_documents: Vec::new(),
                added_editors: after.text_editors.iter().map(|(_, editor)| editor.clone()).collect(),
                removed_editors: Vec::new(),
                new_active_editor: None,
                visible_editors: Some(after.visible_editors.iter().map(|(id, _)| id.clone()).collect()),
            };
        };

        // diffSets(before.documents, after.documents), pela identidade
        let removed_documents: Vec<&Rc<NotebookTextModel>> = before.documents.iter().filter(|document| !after.documents.iter().any(|other| Rc::ptr_eq(document, other))).collect();
        let added_documents: Vec<Rc<NotebookTextModel>> = after.documents.iter().filter(|document| !before.documents.iter().any(|other| Rc::ptr_eq(document, other))).cloned().collect();
        let editor_delta = diff_maps(&before.text_editors, &after.text_editors);

        let new_active_editor = if before.active_editor != after.active_editor { Some(after.active_editor.clone()) } else { None };
        let visible_editor_delta = diff_maps(&before.visible_editors, &after.visible_editors);

        INotebookAndEditorDelta {
            added_documents,
            removed_documents: removed_documents.iter().map(|document| document.uri()).collect(),
            added_editors: editor_delta.added,
            removed_editors: editor_delta.removed.iter().map(|removed| removed.get_id()).collect(),
            new_active_editor,
            visible_editors: if visible_editor_delta.added.is_empty() && visible_editor_delta.removed.is_empty() {
                None
            } else {
                Some(after.visible_editors.iter().map(|(id, _)| id.clone()).collect())
            },
        }
    }
}

/// `MainThreadNotebooksAndEditors`.
pub struct MainThreadNotebooksAndEditors {
    me: Weak<MainThreadNotebooksAndEditors>,
    proxy: Rc<dyn ExtHostNotebookShape>,
    disposables: Rc<Disposable>,
    editor_listeners: Rc<DisposableMap<String, dyn IDisposable>>,
    current_state: RefCell<Option<NotebookAndEditorState>>,
    main_thread_notebooks: Rc<MainThreadNotebookDocuments>,
    main_thread_editors: Rc<MainThreadNotebookEditors>,
    notebook_service: Rc<dyn INotebookService>,
    notebook_editor_service: Rc<dyn INotebookEditorService>,
    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    log_service: Rc<dyn LogService>,
}

impl MainThreadNotebooksAndEditors {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        notebook_service: Rc<dyn INotebookService>,
        notebook_editor_service: Rc<dyn INotebookEditorService>,
        editor_service: Rc<dyn IEditorService>,
        editor_group_service: Rc<dyn IEditorGroupsService>,
        log_service: Rc<dyn LogService>,
        notebook_editor_model_resolver_service: Rc<dyn INotebookEditorModelResolverService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let main_thread_notebooks = MainThreadNotebookDocuments::new(ext_host_context, notebook_editor_model_resolver_service, uri_identity_service);
        let main_thread_editors =
            MainThreadNotebookEditors::new(ext_host_context, editor_service.clone(), notebook_editor_service.clone(), editor_group_service.clone(), configuration_service);

        ext_host_context.set(&MainContext::MAIN_THREAD_NOTEBOOK_DOCUMENTS, main_thread_notebooks.clone() as Rc<dyn MainThreadNotebookDocumentsShape>);
        ext_host_context.set(&MainContext::MAIN_THREAD_NOTEBOOK_EDITORS, main_thread_editors.clone() as Rc<dyn MainThreadNotebookEditorsShape>);

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK),
            disposables: Disposable::new(),
            editor_listeners: DisposableMap::new(),
            current_state: RefCell::new(None),
            main_thread_notebooks,
            main_thread_editors,
            notebook_service,
            notebook_editor_service,
            editor_service,
            editor_group_service,
            log_service,
        });

        let weak = this.me.clone();
        this.disposables.register(this.notebook_service.on_will_add_notebook_document().listen(Self::update_state_listener(&weak)));
        this.disposables.register(this.notebook_service.on_did_remove_notebook_document().listen(Self::update_state_listener(&weak)));
        this.disposables.register(this.editor_service.on_did_active_editor_change().listen(Self::update_state_listener(&weak)));
        this.disposables.register(this.editor_service.on_did_visible_editors_change().listen(Self::update_state_listener(&weak)));
        this.disposables.register(this.notebook_editor_service.on_did_add_notebook_editor().listen({
            let weak = weak.clone();
            move |widget| {
                if let (Some(this), Some(editor)) = (weak.upgrade(), Rc::clone(widget).as_notebook_editor()) {
                    this.handle_editor_add(&editor);
                }
            }
        }));
        this.disposables.register(this.notebook_editor_service.on_did_remove_notebook_editor().listen({
            let weak = weak.clone();
            move |widget| {
                if let (Some(this), Some(editor)) = (weak.upgrade(), Rc::clone(widget).as_notebook_editor()) {
                    this.handle_editor_remove(&editor);
                }
            }
        }));
        this.update_state(None);
        this
    }

    /// Um ouvinte de `() => this._updateState()`, para qualquer evento.
    fn update_state_listener<T: 'static>(weak: &Weak<Self>) -> impl Fn(&T) + 'static {
        let weak = weak.clone();
        move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_state(None);
            }
        }
    }

    /// `_handleEditorAdd(editor)`.
    fn handle_editor_add(&self, editor: &Rc<dyn INotebookEditor>) {
        let (weak_this, weak_editor) = (self.me.clone(), Rc::downgrade(editor));
        let on_did_change_model = editor.on_did_change_model().listen({
            let weak_this = weak_this.clone();
            move |_| {
                if let Some(this) = weak_this.upgrade() {
                    this.update_state(None);
                }
            }
        });
        let on_did_focus_widget = editor.on_did_focus_widget().listen(move |_| {
            if let (Some(this), Some(editor)) = (weak_this.upgrade(), weak_editor.upgrade()) {
                this.update_state(Some(&editor));
            }
        });
        self.editor_listeners.set(editor.get_id(), combined_disposable(vec![on_did_change_model, on_did_focus_widget]), false);
        self.update_state(None);
    }

    /// `_handleEditorRemove(editor)`.
    fn handle_editor_remove(&self, editor: &Rc<dyn INotebookEditor>) {
        self.editor_listeners.delete_and_dispose(&editor.get_id());
        self.update_state(None);
    }

    /// `_updateState(focusedEditor?)`.
    fn update_state(&self, focused_editor: Option<&Rc<dyn INotebookEditor>>) {
        let mut editors: Vec<(String, Rc<dyn INotebookEditor>)> = Vec::new();
        let mut visible_editors_map: Vec<(String, Rc<dyn INotebookEditor>)> = Vec::new();

        for widget in self.notebook_editor_service.list_notebook_editors() {
            if let Some(editor) = widget.as_notebook_editor()
                && editor.has_model()
            {
                let id = editor.get_id();
                editors.retain(|(candidate, _)| *candidate != id);
                editors.push((id, editor));
            }
        }

        let active_notebook_editor = get_notebook_editor_from_editor_pane(self.editor_service.active_editor_pane().as_ref());
        let mut active_editor: Option<String> = None;
        if let Some(active_notebook_editor) = &active_notebook_editor {
            active_editor = Some(active_notebook_editor.get_id());
        } else if let Some(focused_editor) = focused_editor
            && focused_editor.text_model().is_some()
        {
            active_editor = Some(focused_editor.get_id());
        }
        if let Some(active) = &active_editor
            && !editors.iter().any(|(id, _)| id == active)
        {
            self.log_service.trace_args(
                "MainThreadNotebooksAndEditors#_updateState: active editor is not in editors list",
                &[
                    LogArg::Value(Dynamic::string(active)),
                    LogArg::Value(Dynamic::array(editors.iter().map(|(id, _)| Dynamic::string(id)))),
                ],
            );
            active_editor = None;
        }

        for editor_pane in self.editor_service.visible_editor_panes() {
            let notebook_editor = get_notebook_editor_from_editor_pane(Some(&editor_pane));
            if let Some(notebook_editor) = notebook_editor
                && notebook_editor.has_model()
            {
                let id = notebook_editor.get_id();
                if editors.iter().any(|(candidate, _)| *candidate == id) {
                    visible_editors_map.retain(|(candidate, _)| *candidate != id);
                    visible_editors_map.push((id, notebook_editor));
                }
            }
        }

        let new_state = NotebookAndEditorState { documents: self.notebook_service.list_notebook_documents(), text_editors: editors, active_editor, visible_editors: visible_editors_map };
        let delta = NotebookAndEditorState::delta(self.current_state.borrow().as_ref(), &new_state);
        self.on_delta(delta);
        *self.current_state.borrow_mut() = Some(new_state);
    }

    /// `_onDelta(delta)`.
    fn on_delta(&self, delta: INotebookAndEditorDelta) {
        if Self::is_delta_empty(&delta) {
            return;
        }

        let dto = INotebookDocumentsAndEditorsDelta {
            removed_documents: Some(delta.removed_documents.clone()),
            removed_editors: Some(delta.removed_editors.clone()),
            new_active_editor: delta.new_active_editor.clone(),
            visible_editors: delta.visible_editors.clone(),
            added_documents: Some(delta.added_documents.iter().map(Self::as_model_add_data).collect()),
            added_editors: Some(delta.added_editors.iter().map(|editor| self.as_editor_add_data(editor)).collect()),
        };

        // send to extension FIRST
        self.proxy.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(dto));

        // handle internally
        self.main_thread_editors.handle_editors_removed(&delta.removed_editors);
        self.main_thread_notebooks.handle_notebooks_removed(&delta.removed_documents);
        self.main_thread_notebooks.handle_notebooks_added(&delta.added_documents);
        self.main_thread_editors.handle_editors_added(&delta.added_editors);
    }

    /// `_isDeltaEmpty(delta)`.
    fn is_delta_empty(delta: &INotebookAndEditorDelta) -> bool {
        if !delta.added_documents.is_empty() {
            return false;
        }
        if !delta.removed_documents.is_empty() {
            return false;
        }
        if !delta.added_editors.is_empty() {
            return false;
        }
        if !delta.removed_editors.is_empty() {
            return false;
        }
        if delta.visible_editors.as_ref().is_some_and(|visible_editors| !visible_editors.is_empty()) {
            return false;
        }
        if delta.new_active_editor.is_some() {
            return false;
        }
        true
    }

    /// `_asModelAddData(e)`.
    fn as_model_add_data(e: &Rc<NotebookTextModel>) -> INotebookModelAddedData {
        INotebookModelAddedData {
            view_type: e.view_type(),
            uri: e.uri(),
            metadata: Some(e.metadata()),
            version_id: e.version_id(),
            cells: e.cells().iter().map(|cell| to_notebook_cell_dto(&**cell)).collect(),
        }
    }

    /// `_asEditorAddData(add)`.
    fn as_editor_add_data(&self, add: &Rc<dyn INotebookEditor>) -> INotebookEditorAddData {
        let pane = self
            .editor_service
            .visible_editor_panes()
            .into_iter()
            .find(|pane| get_notebook_editor_from_editor_pane(Some(pane)).is_some_and(|editor| address(&editor) == address(add)));

        INotebookEditorAddData {
            id: add.get_id(),
            document_uri: add.text_model().expect("o editor ativo tem modelo").uri(),
            selections: add.get_selections().iter().map(|range| CellRange { start: range.start as f64, end: range.end as f64 }).collect(),
            visible_ranges: add.visible_ranges().iter().map(|range| CellRange { start: range.start as f64, end: range.end as f64 }).collect(),
            view_column: pane.map(|pane| editor_group_to_column(&*self.editor_group_service, &GroupRef::Id(pane.group_id()))),
            view_type: add.view_type(),
        }
    }
}

impl IDisposable for MainThreadNotebooksAndEditors {
    fn dispose(&self) {
        self.main_thread_notebooks.dispose();
        self.main_thread_editors.dispose();
        self.disposables.dispose();
        self.editor_listeners.dispose();
    }
}
