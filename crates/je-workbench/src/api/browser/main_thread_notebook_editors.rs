//! Porte de `src/vs/workbench/api/browser/mainThreadNotebookEditors.ts`: o `MainThreadNotebookEditors`, o lado do workbench do RPC
//! `ExtHostNotebookEditors`: repassa ao host de extensões os intervalos visíveis, a seleção e a coluna de cada editor de
//! notebook, e atende `$tryShowNotebookDocument`, `$tryRevealRange` e `$trySetSelections`.
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IEditorService, @INotebookEditorService, @IEditorGroupsService,
//!   @IConfigurationService)` é [`MainThreadNotebookEditors::new`] com os cinco argumentos na ordem do original (o
//!   `IExtHostContext` é o [`IMainContext`]). O objeto nasce como `Rc` (`Rc::new_cyclic`) porque os ouvintes dos eventos guardam um
//!   `Weak` dele.
//! - **Editores.** O `INotebookEditor` é o contrato de `notebook_browser` (o widget é UI e não tem porte). O
//!   `INotebookEditorService` entrega widgets; o editor de cada um vem de `NotebookEditorWidget::as_notebook_editor`.
//! - **`INotebookEditorOptions`.** O `TextEditorOptions` da entrada sem tipo não tem `cellSelections`: ele vai no `viewState`
//!   (ver [`INotebookEditorOptions::to_text_editor_options`]), e o `label` das opções é o `label` da entrada.
//! - **`$tryShowNotebookDocument`.** A mensagem de erro `Notebook Editor creation failure for document ${JSON.stringify(resource)}`
//!   serializa os componentes do recurso (`scheme`, `authority`, `path`, `query`, `fragment`, só os não vazios), sem o cache
//!   (`$mid`, `external`, `fsPath`) que o objeto do RPC carrega no original.
//! - **`$tryRevealRange` e `$trySetSelections`.** Os `ICellRange` do protocolo são os [`CellRange`] (`f64`) e viram os `i64` do
//!   modelo. `$trySetSelections` devolve uma promessa já resolvida (o `void` do original). O `cellAt(range.start)` ausente
//!   (índice que o `getLength` já excluiu) encerra sem fazer nada.
//! - **`_currentViewColumnInfo`.** `INotebookEditorViewColumnInfo` é um `BTreeMap`, então o `equals` do original (que ignora a
//!   ordem das chaves) é a igualdade do mapa; o estado inicial é `None` (`undefined`), que difere de qualquer resultado, até do
//!   vazio.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::opener::editor_types::{EditorActivation, EditorOptions, EditorOverride};

use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookEditorsShape, INotebookDocumentShowOptions, INotebookEditorPropertiesChangeData, INotebookEditorViewColumnInfo,
    INotebookSelectionChangeEvent, INotebookVisibleRangesEvent, IMainContext, MainThreadNotebookEditorsShape,
};
use crate::api::common::ext_host_type_converters::CellRange;
use crate::api::common::ext_host_types::NotebookEditorRevealType;
use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::contrib::notebook::browser::notebook_browser::{INotebookEditor, INotebookEditorOptions, get_notebook_editor_from_editor_pane};
use crate::contrib::notebook::browser::services::notebook_editor_service::INotebookEditorService;
use crate::contrib::notebook::common::notebook_range::ICellRange;
use crate::services::editor::editor_group_column::{column_to_editor_group, editor_group_to_column};
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_service::IEditorService;

/// `MainThreadNotebook` (a classe local do original): o editor e o que o escuta.
struct MainThreadNotebook {
    #[allow(dead_code)]
    editor: Rc<dyn INotebookEditor>,
    disposables: Rc<DisposableStore>,
}

impl MainThreadNotebook {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// O `ICellRange` do modelo como o do protocolo.
fn to_cell_range(range: &ICellRange) -> CellRange {
    CellRange { start: range.start as f64, end: range.end as f64 }
}

/// O `ICellRange` do protocolo como o do modelo.
fn from_cell_range(range: &CellRange) -> ICellRange {
    ICellRange { start: range.start as i64, end: range.end as i64 }
}

/// `JSON.stringify(resource)` dos componentes do recurso (ver as divergências do módulo).
fn stringify_components(resource: &URI) -> String {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    for (key, value) in [
        ("scheme", resource.scheme().to_string_lossy()),
        ("authority", resource.authority().to_string_lossy()),
        ("path", resource.path().to_string_lossy()),
        ("query", resource.query().to_string_lossy()),
        ("fragment", resource.fragment().to_string_lossy()),
    ] {
        if !value.is_empty() {
            entries.push((key, Dynamic::string(&value)));
        }
    }
    json_stringify(&Dynamic::object(entries)).unwrap_or_default()
}

/// `MainThreadNotebookEditors`.
pub struct MainThreadNotebookEditors {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostNotebookEditorsShape>,
    main_thread_editors: RefCell<HashMap<String, Rc<MainThreadNotebook>>>,
    current_view_column_info: RefCell<Option<INotebookEditorViewColumnInfo>>,
    editor_service: Rc<dyn IEditorService>,
    notebook_editor_service: Rc<dyn INotebookEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl MainThreadNotebookEditors {
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        editor_service: Rc<dyn IEditorService>,
        notebook_editor_service: Rc<dyn INotebookEditorService>,
        editor_group_service: Rc<dyn IEditorGroupsService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK_EDITORS),
            main_thread_editors: RefCell::new(HashMap::new()),
            current_view_column_info: RefCell::new(None),
            editor_service,
            notebook_editor_service,
            editor_group_service,
            configuration_service,
        });

        let weak: Weak<Self> = Rc::downgrade(&this);
        this.base.register(this.editor_service.on_did_active_editor_change().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_editor_view_columns();
                }
            }
        }));
        this.base.register(this.editor_group_service.on_did_remove_group().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_editor_view_columns();
                }
            }
        }));
        this.base.register(this.editor_group_service.on_did_move_group().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_editor_view_columns();
            }
        }));
        this
    }

    /// `handleEditorsAdded(editors)`.
    pub fn handle_editors_added(&self, editors: &[Rc<dyn INotebookEditor>]) {
        for editor in editors {
            let editor_disposables = DisposableStore::new();
            editor_disposables.add(editor.on_did_change_visible_ranges().listen({
                let (proxy, editor) = (self.proxy.clone(), Rc::downgrade(editor));
                move |_| {
                    if let Some(editor) = editor.upgrade() {
                        proxy.rpc_accept_editor_properties_changed(
                            &editor.get_id(),
                            INotebookEditorPropertiesChangeData {
                                visible_ranges: Some(INotebookVisibleRangesEvent { ranges: editor.visible_ranges().iter().map(to_cell_range).collect() }),
                                selections: None,
                            },
                        );
                    }
                }
            }));

            editor_disposables.add(editor.on_did_change_selection().listen({
                let (proxy, editor) = (self.proxy.clone(), Rc::downgrade(editor));
                move |_| {
                    if let Some(editor) = editor.upgrade() {
                        proxy.rpc_accept_editor_properties_changed(
                            &editor.get_id(),
                            INotebookEditorPropertiesChangeData {
                                visible_ranges: None,
                                selections: Some(INotebookSelectionChangeEvent { selections: editor.get_selections().iter().map(to_cell_range).collect() }),
                            },
                        );
                    }
                }
            }));

            let wrapper = Rc::new(MainThreadNotebook { editor: editor.clone(), disposables: editor_disposables });
            self.main_thread_editors.borrow_mut().insert(editor.get_id(), wrapper);
        }
    }

    /// `handleEditorsRemoved(editorIds)`.
    pub fn handle_editors_removed(&self, editor_ids: &[String]) {
        for id in editor_ids {
            let removed = self.main_thread_editors.borrow_mut().remove(id);
            if let Some(removed) = removed {
                removed.dispose();
            }
        }
    }

    /// `_updateEditorViewColumns()`.
    fn update_editor_view_columns(&self) {
        let mut result = INotebookEditorViewColumnInfo::new();
        for editor_pane in self.editor_service.visible_editor_panes() {
            let candidate = get_notebook_editor_from_editor_pane(Some(&editor_pane));
            if let Some(candidate) = candidate
                && self.main_thread_editors.borrow().contains_key(&candidate.get_id())
            {
                result.insert(candidate.get_id(), editor_group_to_column(&*self.editor_group_service, &GroupRef::Id(editor_pane.group_id())));
            }
        }
        if self.current_view_column_info.borrow().as_ref() != Some(&result) {
            *self.current_view_column_info.borrow_mut() = Some(result.clone());
            self.proxy.rpc_accept_editor_view_columns(result);
        }
    }

    /// `this._notebookEditorService.getNotebookEditor(id)` como `INotebookEditor`.
    fn get_notebook_editor(&self, id: &str) -> Option<Rc<dyn INotebookEditor>> {
        self.notebook_editor_service.get_notebook_editor(id)?.as_notebook_editor()
    }
}

impl IDisposable for MainThreadNotebookEditors {
    fn dispose(&self) {
        self.base.dispose();
        let editors: Vec<Rc<MainThreadNotebook>> = self.main_thread_editors.borrow().values().cloned().collect();
        for editor in editors {
            editor.dispose();
        }
    }
}

impl MainThreadNotebookEditorsShape for MainThreadNotebookEditors {
    fn rpc_try_show_notebook_document(&self, resource: &URI, view_type: &str, options: INotebookDocumentShowOptions) -> Promise<String> {
        let editor_options = INotebookEditorOptions {
            base: TextEditorOptions {
                base: EditorOptions {
                    preserve_focus: options.preserve_focus,
                    pinned: options.pinned,
                    // preserve pre 1.38 behaviour to not make group active when preserveFocus: true
                    // but make sure to restore the editor to fix https://github.com/microsoft/vscode/issues/79633
                    activation: if options.preserve_focus == Some(true) { Some(EditorActivation::Restore) } else { None },
                    override_: Some(EditorOverride::Id(view_type.into())),
                    ..EditorOptions::default()
                },
                ..TextEditorOptions::default()
            },
            cell_selections: options.selections.as_ref().map(|selections| selections.iter().map(from_cell_range).collect()),
            label: options.label.clone(),
        };

        let input = UntypedEditorInput {
            resource: Some(resource.clone()),
            options: Some(editor_options.to_text_editor_options()),
            label: editor_options.label.clone(),
            ..UntypedEditorInput::default()
        };
        let group = column_to_editor_group(&*self.editor_group_service, &*self.configuration_service, options.position);
        let editor_service = self.editor_service.clone();
        let resource = resource.clone();
        async_fn(async move {
            let editor_pane = editor_service.open_editor_in(input, Some(PreferredGroup::Id(group))).await?;
            let notebook_editor = get_notebook_editor_from_editor_pane(editor_pane.as_ref());

            match notebook_editor {
                Some(notebook_editor) => Ok(notebook_editor.get_id()),
                None => Err(Error::new(format!("Notebook Editor creation failure for document {}", stringify_components(&resource)))),
            }
        })
    }

    fn rpc_try_reveal_range(&self, id: &str, range: CellRange, reveal_type: NotebookEditorRevealType) -> Promise<()> {
        let Some(notebook_editor) = self.get_notebook_editor(id) else {
            return Promise::resolved(());
        };
        if !notebook_editor.has_model() {
            return Promise::resolved(());
        }

        let range = from_cell_range(&range);
        if range.start >= notebook_editor.get_length() as i64 {
            return Promise::resolved(());
        }

        let Some(cell) = notebook_editor.cell_at(range.start as usize) else {
            return Promise::resolved(());
        };

        match reveal_type {
            NotebookEditorRevealType::Default => {
                notebook_editor.reveal_cell_range_in_view(range);
                Promise::resolved(())
            }
            NotebookEditorRevealType::InCenter => {
                notebook_editor.reveal_in_center(&cell);
                Promise::resolved(())
            }
            NotebookEditorRevealType::InCenterIfOutsideViewport => notebook_editor.reveal_in_center_if_outside_viewport(&cell),
            NotebookEditorRevealType::AtTop => {
                notebook_editor.reveal_in_view_at_top(&cell);
                Promise::resolved(())
            }
        }
    }

    fn rpc_try_set_selections(&self, id: &str, ranges: Vec<CellRange>) -> Promise<()> {
        let Some(editor) = self.get_notebook_editor(id) else {
            return Promise::resolved(());
        };

        let ranges: Vec<ICellRange> = ranges.iter().map(from_cell_range).collect();
        editor.set_selections(ranges.clone());

        if let Some(first) = ranges.first() {
            editor.set_focus(ICellRange { start: first.start, end: first.start + 1 });
        }
        Promise::resolved(())
    }
}
