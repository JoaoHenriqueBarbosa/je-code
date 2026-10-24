//! Porte de `src/vs/workbench/api/browser/mainThreadDocumentsAndEditors.ts`: o estado de documentos e editores
//! ([`TextEditorSnapshot`], `DocumentAndEditorStateDelta`, `DocumentAndEditorState` e o
//! `MainThreadDocumentAndEditorStateComputer`) e o [`MainThreadDocumentsAndEditors`], que o traduz em deltas para o host
//! de extensões e cria o [`MainThreadDocuments`], o [`MainThreadTextEditors`] e os [`MainThreadTextEditor`].
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IModelService, ...)` é [`MainThreadDocumentsAndEditors::new`] com os
//!   dezesseis argumentos na ordem do original. O `IModelService` é o [`ModelService`] concreto, e o `ICodeEditorService` é
//!   o [`CodeEditorRegistry`] (o registro de editores e as decorações do serviço). O `@extHostCustomer` do registro não
//!   existe: o objeto é criado direto, e o `extHostContext.set(MainContext.X, ...)` é o `set` do [`IMainContext`].
//! - **Editores.** O `ICodeEditor` é o [`CodeEditorWidget`] concreto. A comparação de editores (`candidate ===
//!   snapshot.editor`, `getCodeEditor() === codeEditor`) é pela identidade do objeto: o `IEditorService` devolve o
//!   `IActiveCodeEditor` (o recorte de `ICodeEditor` do workbench), e o endereço do objeto é comparado com o do widget.
//!   `isDiffEditor(activeTextEditorControl)` é a variante `Diff` do controle ativo, de que se usa o lado `modified`.
//! - **`activeEditor`.** `string | null | undefined` no estado é `Option<String>` (`null` é `None`; o estado nunca guarda
//!   `undefined`), e nos campos `oldActiveEditor` e `newActiveEditor` do delta é `Option<Option<String>>` (`None` é `undefined`).
//! - **`documents.add(model)`** de `_updateStateOnModelAdd` muda o `Set` do estado atual (o `Set.add` devolve o mesmo
//!   conjunto): o conjunto é um `Rc<RefCell<..>>` compartilhado entre o estado antigo e o novo, como no original.
//! - **`Event.filter(...)(listener, undefined, store)`** dos eventos de painel é `filter(..).listen(..)` com o
//!   descarte registrado no `DisposableStore`. O `combinedDisposable` dos três ouvintes de cada editor é o
//!   [`combined_disposable`].
//! - `IPaneComposite` (o `panel instanceof AbstractTextEditor` com `isCodeEditor(getControl())`) é
//!   [`IPaneComposite::text_editor_code_control`].
//! - **`getIdOfCodeEditor(codeEditor)`** recebe o `IActiveCodeEditor` e compara o endereço com o do editor de código de
//!   cada [`MainThreadTextEditor`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::lifecycle::{combined_disposable, Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::uri::URI;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::language_feature_registry::should_synchronize_model;
use je_editor::model_service::ModelService;
use je_editor::services::abstract_code_editor_service::{CodeEditorRegistry, ICodeEditorRegistry};
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::i_file_service::IFileService;
use je_platform::path::path_service::IPathService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_documents::MainThreadDocuments;
use crate::api::browser::main_thread_editor::{IFocusTracker, MainThreadTextEditor};
use crate::api::browser::main_thread_editors::{IMainThreadEditorLocator, MainThreadTextEditors};
use crate::api::common::ext_host_protocol::{
    DocumentsAndEditorsDelta, ExtHostContext, ExtHostDocumentsAndEditorsShape, IMainContext, MainContext, MainThreadDocumentsShape, MainThreadTextEditorsShape, ModelAddedData,
    TextEditorAddData,
};
use crate::common::editor::IEditorPane;
use crate::common::views::ViewContainerLocation;
use crate::contrib::scm::browser::quick_diff_model::IQuickDiffModelService;
use crate::services::editor::editor_group_column::editor_group_to_column;
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService};
use crate::services::editor::editor_service::{ActiveTextEditorControl, IActiveCodeEditor, IEditorService};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::panecomposite::panecomposite::IPaneCompositePartService;
use crate::services::textfile::collaborators::IWorkingCopyFileService;
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::textmodel_resolver::resolver_service::ITextModelService;

/// O endereço de um objeto compartilhado, para comparar editores pela identidade (o `===` do original).
fn address<T: ?Sized>(value: &Rc<T>) -> usize {
    Rc::as_ptr(value) as *const () as usize
}

/// `TextEditorSnapshot`.
pub struct TextEditorSnapshot {
    pub editor: Rc<CodeEditorWidget>,
    pub id: String,
}

impl TextEditorSnapshot {
    fn new(editor: Rc<CodeEditorWidget>) -> Self {
        let model = editor.get_model().expect("o editor ativo tem modelo");
        let id = format!("{},{}", editor.get_id(), model.id());
        Self { editor, id }
    }
}

/// `DocumentAndEditorStateDelta`.
pub struct DocumentAndEditorStateDelta {
    pub removed_documents: Vec<Rc<TextModel>>,
    pub added_documents: Vec<Rc<TextModel>>,
    pub removed_editors: Vec<Rc<TextEditorSnapshot>>,
    pub added_editors: Vec<Rc<TextEditorSnapshot>>,
    pub old_active_editor: Option<Option<String>>,
    pub new_active_editor: Option<Option<String>>,
    pub is_empty: bool,
}

impl DocumentAndEditorStateDelta {
    fn new(
        removed_documents: Vec<Rc<TextModel>>,
        added_documents: Vec<Rc<TextModel>>,
        removed_editors: Vec<Rc<TextEditorSnapshot>>,
        added_editors: Vec<Rc<TextEditorSnapshot>>,
        old_active_editor: Option<Option<String>>,
        new_active_editor: Option<Option<String>>,
    ) -> Self {
        let is_empty = removed_documents.is_empty()
            && added_documents.is_empty()
            && removed_editors.is_empty()
            && added_editors.is_empty()
            && old_active_editor == new_active_editor;
        Self { removed_documents, added_documents, removed_editors, added_editors, old_active_editor, new_active_editor, is_empty }
    }
}

impl std::fmt::Display for DocumentAndEditorStateDelta {
    /// `toString()`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let uris = |models: &[Rc<TextModel>]| models.iter().map(|model| model.uri().to_string(true).to_string_lossy()).collect::<Vec<_>>().join(", ");
        let ids = |editors: &[Rc<TextEditorSnapshot>]| editors.iter().map(|editor| editor.id.clone()).collect::<Vec<_>>().join(", ");
        writeln!(f, "DocumentAndEditorStateDelta")?;
        writeln!(f, "\tRemoved Documents: [{}]", uris(&self.removed_documents))?;
        writeln!(f, "\tAdded Documents: [{}]", uris(&self.added_documents))?;
        writeln!(f, "\tRemoved Editors: [{}]", ids(&self.removed_editors))?;
        writeln!(f, "\tAdded Editors: [{}]", ids(&self.added_editors))?;
        let new_active_editor = match &self.new_active_editor {
            None => "undefined".to_string(),
            Some(None) => "null".to_string(),
            Some(Some(id)) => id.clone(),
        };
        writeln!(f, "\tNew Active Editor: {new_active_editor}")
    }
}

/// `DocumentAndEditorState`.
struct DocumentAndEditorState {
    /// `documents: Set<ITextModel>`, na ordem de inserção, compartilhado entre os estados (ver o topo do módulo).
    documents: Rc<RefCell<Vec<Rc<TextModel>>>>,
    /// `textEditors: Map<string, TextEditorSnapshot>`, na ordem de inserção.
    text_editors: Vec<(String, Rc<TextEditorSnapshot>)>,
    active_editor: Option<String>,
}

impl DocumentAndEditorState {
    /// `compute(before, after)`.
    fn compute(before: Option<&DocumentAndEditorState>, after: &DocumentAndEditorState) -> DocumentAndEditorStateDelta {
        let Some(before) = before else {
            return DocumentAndEditorStateDelta::new(
                Vec::new(),
                after.documents.borrow().clone(),
                Vec::new(),
                after.text_editors.iter().map(|(_, editor)| editor.clone()).collect(),
                None,
                Some(after.active_editor.clone()),
            );
        };

        // diffSets(before.documents, after.documents)
        let before_documents = before.documents.borrow();
        let after_documents = after.documents.borrow();
        let removed_documents: Vec<Rc<TextModel>> = before_documents.iter().filter(|model| !after_documents.iter().any(|other| Rc::ptr_eq(model, other))).cloned().collect();
        let added_documents: Vec<Rc<TextModel>> = after_documents.iter().filter(|model| !before_documents.iter().any(|other| Rc::ptr_eq(model, other))).cloned().collect();

        // diffMaps(before.textEditors, after.textEditors)
        let removed_editors: Vec<Rc<TextEditorSnapshot>> =
            before.text_editors.iter().filter(|(key, _)| !after.text_editors.iter().any(|(other, _)| other == key)).map(|(_, editor)| editor.clone()).collect();
        let added_editors: Vec<Rc<TextEditorSnapshot>> =
            after.text_editors.iter().filter(|(key, _)| !before.text_editors.iter().any(|(other, _)| other == key)).map(|(_, editor)| editor.clone()).collect();

        let changed = before.active_editor != after.active_editor;
        let old_active_editor = changed.then(|| before.active_editor.clone());
        let new_active_editor = changed.then(|| after.active_editor.clone());

        DocumentAndEditorStateDelta::new(removed_documents, added_documents, removed_editors, added_editors, old_active_editor, new_active_editor)
    }
}

/// `ActiveEditorOrder`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveEditorOrder {
    Editor,
    Panel,
}

/// `MainThreadDocumentAndEditorStateComputer`.
struct MainThreadDocumentAndEditorStateComputer {
    me: Weak<MainThreadDocumentAndEditorStateComputer>,
    to_dispose: Rc<DisposableStore>,
    to_dispose_on_editor_remove: Rc<DisposableMap<String, dyn IDisposable>>,
    current_state: RefCell<Option<Rc<DocumentAndEditorState>>>,
    active_editor_order: Cell<ActiveEditorOrder>,
    on_did_change_state: Rc<dyn Fn(DocumentAndEditorStateDelta)>,
    model_service: Rc<ModelService>,
    code_editor_service: Rc<CodeEditorRegistry>,
    editor_service: Rc<dyn IEditorService>,
    pane_composite_service: Rc<dyn IPaneCompositePartService>,
}

impl MainThreadDocumentAndEditorStateComputer {
    fn new(
        on_did_change_state: Rc<dyn Fn(DocumentAndEditorStateDelta)>,
        model_service: Rc<ModelService>,
        code_editor_service: Rc<CodeEditorRegistry>,
        editor_service: Rc<dyn IEditorService>,
        pane_composite_service: Rc<dyn IPaneCompositePartService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            to_dispose: DisposableStore::new(),
            to_dispose_on_editor_remove: DisposableMap::new(),
            current_state: RefCell::new(None),
            active_editor_order: Cell::new(ActiveEditorOrder::Editor),
            on_did_change_state,
            model_service,
            code_editor_service,
            editor_service,
            pane_composite_service,
        });

        let weak = this.me.clone();
        let listen = |weak: &Weak<Self>, action: fn(&Self)| {
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    action(&this);
                }
            }
        };

        this.to_dispose.add(this.model_service.on_model_added().listen({
            let weak = weak.clone();
            move |model| {
                if let Some(this) = weak.upgrade() {
                    this.update_state_on_model_add(model);
                }
            }
        }));
        let on_model_removed = listen(&weak, |this| this.update_state(None));
        this.to_dispose.add(this.model_service.on_model_removed().listen(move |_| on_model_removed()));
        let on_active_editor_change = listen(&weak, |this| this.update_state(None));
        this.to_dispose.add(this.editor_service.on_did_active_editor_change().listen(move |_| on_active_editor_change()));

        this.to_dispose.add(this.code_editor_service.on_code_editor_add().listen({
            let weak = weak.clone();
            move |editor| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_add_editor(editor);
                }
            }
        }));
        this.to_dispose.add(this.code_editor_service.on_code_editor_remove().listen({
            let weak = weak.clone();
            move |editor| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_remove_editor(editor);
                }
            }
        }));
        for editor in this.code_editor_service.list_code_editors() {
            this.on_did_add_editor(&editor);
        }

        let on_panel_open = listen(&weak, |this| this.active_editor_order.set(ActiveEditorOrder::Panel));
        this.to_dispose.add(
            this.pane_composite_service
                .on_did_pane_composite_open()
                .filter(|event| event.view_container_location == ViewContainerLocation::Panel, None)
                .listen(move |_| on_panel_open()),
        );
        let on_panel_close = listen(&weak, |this| this.active_editor_order.set(ActiveEditorOrder::Editor));
        this.to_dispose.add(
            this.pane_composite_service
                .on_did_pane_composite_close()
                .filter(|event| event.view_container_location == ViewContainerLocation::Panel, None)
                .listen(move |_| on_panel_close()),
        );
        let on_visible_editors_change = listen(&weak, |this| this.active_editor_order.set(ActiveEditorOrder::Editor));
        this.to_dispose.add(this.editor_service.on_did_visible_editors_change().listen(move |_| on_visible_editors_change()));

        this.update_state(None);
        this
    }

    fn on_did_add_editor(&self, e: &Rc<CodeEditorWidget>) {
        let weak = self.me.clone();
        let on_change_model = {
            let weak = weak.clone();
            e.on_did_change_model().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_state(None);
                }
            })
        };
        let on_focus_text = {
            let weak = weak.clone();
            e.on_did_focus_editor_text().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_state(None);
                }
            })
        };
        let on_focus_widget = {
            let weak = weak.clone();
            let editor = e.clone();
            e.on_did_focus_editor_widget().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_state(Some(&editor));
                }
            })
        };
        self.to_dispose_on_editor_remove.set(e.get_id(), combined_disposable(vec![on_change_model, on_focus_text, on_focus_widget]), false);
        self.update_state(None);
    }

    fn on_did_remove_editor(&self, e: &Rc<CodeEditorWidget>) {
        let id = e.get_id();
        if self.to_dispose_on_editor_remove.has(&id) {
            self.to_dispose_on_editor_remove.delete_and_dispose(&id);
            self.update_state(None);
        }
    }

    fn update_state_on_model_add(&self, model: &Rc<TextModel>) {
        if !should_synchronize_model(model) {
            // ignore
            return;
        }

        let current_state = self.current_state.borrow().clone();
        let Some(current_state) = current_state else {
            // too early
            self.update_state(None);
            return;
        };

        // small (fast) delta
        {
            let mut documents = current_state.documents.borrow_mut();
            if !documents.iter().any(|other| Rc::ptr_eq(other, model)) {
                documents.push(model.clone());
            }
        }
        let new_state = Rc::new(DocumentAndEditorState {
            documents: current_state.documents.clone(),
            text_editors: current_state.text_editors.clone(),
            active_editor: current_state.active_editor.clone(),
        });
        *self.current_state.borrow_mut() = Some(new_state);

        (self.on_did_change_state)(DocumentAndEditorStateDelta::new(Vec::new(), vec![model.clone()], Vec::new(), Vec::new(), None, None));
    }

    fn update_state(&self, widget_focus_candidate: Option<&Rc<CodeEditorWidget>>) {
        // models: ignore too large models
        let mut models: Vec<Rc<TextModel>> = Vec::new();
        for model in self.model_service.get_models() {
            if should_synchronize_model(&model) && !models.iter().any(|other| Rc::ptr_eq(other, &model)) {
                models.push(model);
            }
        }

        // editor: only take those that have a not too large model
        let mut editors: Vec<(String, Rc<TextEditorSnapshot>)> = Vec::new();
        let mut active_editor: Option<String> = None; // Strict null work. This doesn't like being undefined!

        for editor in self.code_editor_service.list_code_editors() {
            if editor.is_simple_widget() {
                continue;
            }
            let is_synchronizable = editor.get_model().is_some_and(|model| {
                editor.has_model()
                    && should_synchronize_model(&model)
                    && !model.is_disposed() // model disposed
                    && self.model_service.get_model(&model.uri()).is_some() // model disposing, the flag didn't flip yet but the model service already removed it
            });
            if is_synchronizable {
                let api_editor = Rc::new(TextEditorSnapshot::new(editor.clone()));
                let id = api_editor.id.clone();
                match editors.iter_mut().find(|(key, _)| *key == id) {
                    Some(entry) => entry.1 = api_editor,
                    None => editors.push((id.clone(), api_editor)),
                }
                if ICodeEditor::has_text_focus(&*editor) || (widget_focus_candidate.is_some_and(|candidate| Rc::ptr_eq(candidate, &editor)) && ICodeEditor::has_widget_focus(&*editor)) {
                    // text focus has priority, widget focus is tricky because multiple
                    // editors might claim widget focus at the same time. therefore we use a
                    // candidate (which is the editor that has raised an widget focus event)
                    // in addition to the widget focus check
                    active_editor = Some(id);
                }
            }
        }

        // active editor: if none of the previous editors had focus we try
        // to match output panels or the active workbench editor with
        // one of editor we have just computed
        if active_editor.is_none() {
            let candidate: Option<usize> = if self.active_editor_order.get() == ActiveEditorOrder::Editor {
                self.get_active_editor_from_editor_part().or_else(|| self.get_active_editor_from_panel())
            } else {
                self.get_active_editor_from_panel().or_else(|| self.get_active_editor_from_editor_part())
            };

            if let Some(candidate) = candidate {
                for (_, snapshot) in &editors {
                    if candidate == address(&snapshot.editor) {
                        active_editor = Some(snapshot.id.clone());
                    }
                }
            }
        }

        // compute new state and compare against old
        let new_state = DocumentAndEditorState { documents: Rc::new(RefCell::new(models)), text_editors: editors, active_editor };
        let delta = {
            let current_state = self.current_state.borrow();
            DocumentAndEditorState::compute(current_state.as_deref(), &new_state)
        };
        if !delta.is_empty {
            *self.current_state.borrow_mut() = Some(Rc::new(new_state));
            (self.on_did_change_state)(delta);
        }
    }

    /// `_getActiveEditorFromPanel()`: a identidade do editor de código do painel ativo.
    fn get_active_editor_from_panel(&self) -> Option<usize> {
        let panel = self.pane_composite_service.get_active_pane_composite(ViewContainerLocation::Panel)?;
        panel.text_editor_code_control().map(|control| address(&control))
    }

    /// `_getActiveEditorFromEditorPart()`: a identidade do controle de texto ativo (o lado `modified` num editor de diff).
    fn get_active_editor_from_editor_part(&self) -> Option<usize> {
        match self.editor_service.active_text_editor_control()? {
            ActiveTextEditorControl::Code(control) => Some(address(&control)),
            ActiveTextEditorControl::Diff { modified, .. } => Some(address(&modified)),
        }
    }
}

impl IDisposable for MainThreadDocumentAndEditorStateComputer {
    fn dispose(&self) {
        self.to_dispose.dispose();
        self.to_dispose_on_editor_remove.dispose();
    }
}

/// `{ onGainedFocus() { }, onLostFocus() { } }`.
struct NoopFocusTracker;

impl IFocusTracker for NoopFocusTracker {
    fn on_gained_focus(&self) {}

    fn on_lost_focus(&self) {}
}

/// `MainThreadDocumentsAndEditors`.
pub struct MainThreadDocumentsAndEditors {
    me: Weak<MainThreadDocumentsAndEditors>,
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostDocumentsAndEditorsShape>,
    main_thread_documents: Rc<MainThreadDocuments>,
    main_thread_editors: Rc<MainThreadTextEditors>,
    text_editors: Rc<DisposableMap<String, MainThreadTextEditor>>,
    model_service: Rc<ModelService>,
    text_file_service: Rc<dyn ITextFileService>,
    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    clipboard_service: Rc<dyn IClipboardService>,
}

impl MainThreadDocumentsAndEditors {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        model_service: Rc<ModelService>,
        text_file_service: Rc<dyn ITextFileService>,
        editor_service: Rc<dyn IEditorService>,
        code_editor_service: Rc<CodeEditorRegistry>,
        file_service: Rc<dyn IFileService>,
        text_model_resolver_service: Rc<dyn ITextModelService>,
        editor_group_service: Rc<dyn IEditorGroupsService>,
        pane_composite_service: Rc<dyn IPaneCompositePartService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        working_copy_file_service: Rc<dyn IWorkingCopyFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        clipboard_service: Rc<dyn IClipboardService>,
        path_service: Rc<dyn IPathService>,
        configuration_service: Rc<dyn IConfigurationService>,
        quick_diff_model_service: Rc<dyn IQuickDiffModelService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_DOCUMENTS_AND_EDITORS);

        let text_editors: Rc<DisposableMap<String, MainThreadTextEditor>> = DisposableMap::new();
        base.register(text_editors.clone());

        let main_thread_documents = MainThreadDocuments::new(
            ext_host_context,
            model_service.clone(),
            text_file_service.clone(),
            file_service,
            text_model_resolver_service,
            environment_service,
            uri_identity_service.clone(),
            working_copy_file_service,
            path_service,
        );
        base.register(main_thread_documents.clone());
        let documents_shape: Rc<dyn MainThreadDocumentsShape> = main_thread_documents.clone();
        ext_host_context.set(&MainContext::MAIN_THREAD_DOCUMENTS, documents_shape);

        let this = Rc::new_cyclic(|me: &Weak<MainThreadDocumentsAndEditors>| {
            let locator: Weak<dyn IMainThreadEditorLocator> = me.clone();
            let main_thread_editors = MainThreadTextEditors::new(
                locator,
                ext_host_context,
                code_editor_service.clone() as Rc<dyn ICodeEditorRegistry>,
                editor_service.clone(),
                editor_group_service.clone(),
                configuration_service,
                quick_diff_model_service,
                uri_identity_service,
            );
            Self {
                me: me.clone(),
                base,
                proxy,
                main_thread_documents,
                main_thread_editors,
                text_editors,
                model_service: model_service.clone(),
                text_file_service,
                editor_service: editor_service.clone(),
                editor_group_service,
                clipboard_service,
            }
        });
        this.base.register(this.main_thread_editors.clone());
        let editors_shape: Rc<dyn MainThreadTextEditorsShape> = this.main_thread_editors.clone();
        ext_host_context.set(&MainContext::MAIN_THREAD_TEXT_EDITORS, editors_shape);

        // It is expected that the ctor of the state computer calls our `_onDelta`.
        let weak = this.me.clone();
        let state_computer = MainThreadDocumentAndEditorStateComputer::new(
            Rc::new(move |delta| {
                if let Some(this) = weak.upgrade() {
                    this.on_delta(delta);
                }
            }),
            model_service,
            code_editor_service,
            editor_service,
            pane_composite_service,
        );
        this.base.register(state_computer);

        this
    }

    fn on_delta(&self, delta: DocumentAndEditorStateDelta) {
        let mut removed_editors: Vec<String> = Vec::new();
        let mut added_editors: Vec<Rc<MainThreadTextEditor>> = Vec::new();

        // removed models
        let removed_documents: Vec<URI> = delta.removed_documents.iter().map(|model| model.uri()).collect();

        // added editors
        for api_editor in &delta.added_editors {
            let model = api_editor.editor.get_model().expect("o editor adicionado tem modelo");
            let main_thread_editor = MainThreadTextEditor::new(
                api_editor.id.clone(),
                model,
                api_editor.editor.clone(),
                Rc::new(NoopFocusTracker),
                self.main_thread_documents.clone(),
                self.model_service.clone(),
                self.clipboard_service.clone(),
            );

            self.text_editors.set(api_editor.id.clone(), main_thread_editor.clone(), false);
            added_editors.push(main_thread_editor);
        }

        // removed editors
        for removed in &delta.removed_editors {
            let id = &removed.id;
            if self.text_editors.get(id).is_some() {
                self.text_editors.delete_and_dispose(id);
                removed_editors.push(id.clone());
            }
        }

        let mut ext_host_delta = DocumentsAndEditorsDelta::default();
        let mut empty = true;
        if let Some(new_active_editor) = &delta.new_active_editor {
            empty = false;
            ext_host_delta.new_active_editor = Some(new_active_editor.clone());
        }
        if !removed_documents.is_empty() {
            empty = false;
            ext_host_delta.removed_documents = Some(removed_documents.clone());
        }
        if !removed_editors.is_empty() {
            empty = false;
            ext_host_delta.removed_editors = Some(removed_editors.clone());
        }
        if !delta.added_documents.is_empty() {
            empty = false;
            ext_host_delta.added_documents = Some(delta.added_documents.iter().map(|model| self.to_model_add_data(model)).collect());
        }
        if !delta.added_editors.is_empty() {
            empty = false;
            ext_host_delta.added_editors = Some(added_editors.iter().map(|editor| self.to_text_editor_add_data(editor)).collect());
        }

        if !empty {
            // first update ext host
            self.proxy.rpc_accept_documents_and_editors_delta(ext_host_delta);

            // second update dependent document/editor states
            for removed_document in &removed_documents {
                self.main_thread_documents.handle_model_removed(removed_document);
            }
            for added_document in &delta.added_documents {
                self.main_thread_documents.handle_model_added(added_document);
            }

            for removed_editor in &removed_editors {
                self.main_thread_editors.handle_text_editor_removed(removed_editor);
            }
            for added_editor in &added_editors {
                self.main_thread_editors.handle_text_editor_added(added_editor);
            }
        }
    }

    /// `_toModelAddData(model)`.
    fn to_model_add_data(&self, model: &Rc<TextModel>) -> ModelAddedData {
        let uri = model.uri();
        ModelAddedData {
            version_id: model.get_version_id(),
            lines: model.get_lines_content(),
            eol: model.get_eol(),
            language_id: model.get_language_id(),
            is_dirty: self.text_file_service.is_dirty(&uri),
            encoding: self.text_file_service.get_encoding(&uri),
            uri,
        }
    }

    /// `_toTextEditorAddData(textEditor)`.
    fn to_text_editor_add_data(&self, text_editor: &Rc<MainThreadTextEditor>) -> TextEditorAddData {
        let props = text_editor.get_properties();
        TextEditorAddData {
            id: text_editor.get_id().to_string(),
            document_uri: text_editor.get_model().uri(),
            options: props.options,
            selections: props.selections,
            visible_ranges: props.visible_ranges,
            editor_position: self.find_editor_position(text_editor),
        }
    }

    /// `_findEditorPosition(editor)`.
    fn find_editor_position(&self, editor: &MainThreadTextEditor) -> Option<f64> {
        for editor_pane in self.editor_service.visible_editor_panes() {
            if editor.matches(&*editor_pane) {
                return Some(editor_group_to_column(&*self.editor_group_service, &GroupRef::Id(editor_pane.group_id())));
            }
        }
        None
    }

    /// `findTextEditorIdFor(editorPane)`.
    pub fn find_text_editor_id_for(&self, editor_pane: &dyn IEditorPane) -> Option<String> {
        for id in self.text_editors.keys() {
            if let Some(editor) = self.text_editors.get(&id) {
                if editor.matches(editor_pane) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// `getIdOfCodeEditor(codeEditor)`.
    pub fn get_id_of_code_editor(&self, code_editor: &dyn IActiveCodeEditor) -> Option<String> {
        let target = code_editor as *const dyn IActiveCodeEditor as *const () as usize;
        for id in self.text_editors.keys() {
            if let Some(editor) = self.text_editors.get(&id) {
                if editor.get_code_editor().is_some_and(|current| address(&current) == target) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// `getEditor(id)`.
    pub fn get_editor(&self, id: &str) -> Option<Rc<MainThreadTextEditor>> {
        self.text_editors.get(&id.to_string())
    }

    /// `_mainThreadDocuments`, para quem monta o contexto do host (e para os testes).
    pub fn main_thread_documents(&self) -> &Rc<MainThreadDocuments> {
        &self.main_thread_documents
    }
}

impl IMainThreadEditorLocator for MainThreadDocumentsAndEditors {
    fn get_editor(&self, id: &str) -> Option<Rc<MainThreadTextEditor>> {
        MainThreadDocumentsAndEditors::get_editor(self, id)
    }

    fn find_text_editor_id_for(&self, editor_pane: &dyn IEditorPane) -> Option<String> {
        MainThreadDocumentsAndEditors::find_text_editor_id_for(self, editor_pane)
    }

    fn get_id_of_code_editor(&self, code_editor: &dyn IActiveCodeEditor) -> Option<String> {
        MainThreadDocumentsAndEditors::get_id_of_code_editor(self, code_editor)
    }
}

impl IDisposable for MainThreadDocumentsAndEditors {
    fn dispose(&self) {
        self.base.dispose();
    }
}
