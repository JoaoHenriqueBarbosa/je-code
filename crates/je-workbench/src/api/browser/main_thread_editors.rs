//! Porte de `src/vs/workbench/api/browser/mainThreadEditors.ts`: o [`MainThreadTextEditors`] (o lado do workbench do RPC
//! `ExtHostEditors`: seleções, opções, decorações, edições, snippets, posição das colunas e diff dos editores) e o
//! [`IMainThreadEditorLocator`].
//!
//! Divergências:
//! - **Injeção.** O construtor `(editorLocator, extHostContext, @ICodeEditorService, ...)` é [`MainThreadTextEditors::new`]
//!   com os oito argumentos na ordem do original. O `editorLocator` é um `Weak<dyn IMainThreadEditorLocator>`: o
//!   `MainThreadDocumentsAndEditors` passa a si mesmo (`this`) enquanto ainda é construído. O `@ICodeEditorService` é o
//!   [`ICodeEditorRegistry`] (as decorações do serviço e o registro de editores; `listDiffEditors` não existe, porque
//!   o editor de diff não tem porte).
//! - **Editor de diff.** `_getTextEditorDiffInformation` e `$getDiffInformation` procuram um `IDiffEditor` com
//!   `listDiffEditors()`; sem editor de diff no porte, sempre vale o ramo do editor de texto (o modelo do
//!   `codeEditor.onDidChangeModel` e o `IQuickDiffModelService`). O ramo do `DiffEditor` não existe.
//! - **Observáveis.** `observableFromEvent`, `derived`, `derivedOpts` e `autorun` são os de `je_base::observable`. O
//!   `ITextEditorDiffInformation[] | undefined` é um `Option<Rc<Vec<...>>>` (a igualdade estrita do `Rc` é a identidade
//!   do array, como o `===` do original); o `equalsFn` de `derivedOpts` é a comparação elemento a elemento de
//!   `isTextEditorDiffInformationEqual` (de `platform/editor/common/editor.ts`, sem porte à parte: a função está aqui).
//! - `toDispose: IDisposable[]` de cada editor é um `DisposableStore`.
//! - **`_editorLocator.findTextEditorIdFor(editorPane)`** recebe o `dyn IEditorPane`; `getIdOfCodeEditor` recebe o
//!   `IActiveCodeEditor` (o editor de código que o painel expõe) e compara a identidade do objeto.
//! - **Métodos `async`** (`$tryShowTextDocument`, `$tryShowEditor`, `$tryHideEditor`) são [`async_fn`]. Os que o original
//!   devolve como `Promise.reject(illegalArgument(...))` são [`Promise::rejected`] com `Illegal argument: TextEditor(id)`.
//! - **`CommandsRegistry.registerCommand('_workbench.revertAllDirty', ...)`** (o fim do arquivo) não foi portada: o handler
//!   é `async` e o contrato de comando do porte ainda não tem como devolver a promessa dele. Fica na lista de pendências.
//! - `INSTANCE_COUNT` (campo estático) é um contador por thread.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::observable::base::{IObservable, IReader, Observable};
use je_base::observable::{autorun, const_observable, derived_opts, observable_from_event, DerivedOptions};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::diff::legacy_lines_diff_computer::LineChange;
use je_editor::diff::range_mapping::LineRangeMapping;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_editor::services::abstract_code_editor_service::ICodeEditorRegistry;
use je_editor::services::decoration_render_options::DecorationRenderOptions;
use je_editor::text_model::TextModel;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::configuration::service::IConfigurationService;
use je_platform::opener::editor_types::{EditorActivation, EditorOptions, EditorOverride, EditorResolution, TextEditorSelection};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::browser::main_thread_editor::MainThreadTextEditor;
use crate::api::common::ext_host_protocol::{
    ApplyEditsOptions, DecorationOptionsDto, EditorGroupColumn, ExtHostContext, ExtHostEditorsShape, IMainContext, ITextEditorDiffInformation, MainThreadTextEditorsShape,
    SnippetOptions, TextDocumentShowOptions, TextEditorConfigurationUpdate, TextEditorPositionData,
};
use crate::api::common::ext_host_types::TextEditorRevealType;
use crate::common::editor::{IEditorPane, TextEditorOptions, UntypedEditorInput};
use crate::contrib::scm::browser::quick_diff_model::{IQuickDiffModelService, QuickDiffProviderKind};
use crate::services::editor::editor_group_column::{column_to_editor_group, editor_group_to_column};
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_service::{ActiveTextEditorControl, EditorControl, IActiveCodeEditor, IEditorService};

/// `IMainThreadEditorLocator`.
pub trait IMainThreadEditorLocator {
    /// `getEditor(id)`.
    fn get_editor(&self, id: &str) -> Option<Rc<MainThreadTextEditor>>;

    /// `findTextEditorIdFor(editorPane)`.
    fn find_text_editor_id_for(&self, editor_pane: &dyn IEditorPane) -> Option<String>;

    /// `getIdOfCodeEditor(codeEditor)`.
    fn get_id_of_code_editor(&self, code_editor: &dyn IActiveCodeEditor) -> Option<String>;
}

thread_local! {
    /// `MainThreadTextEditors.INSTANCE_COUNT`.
    static INSTANCE_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// `Promise.reject(illegalArgument(`TextEditor(${id})`))`.
fn illegal_text_editor<T: Clone + 'static>(id: &str) -> Promise<T> {
    Promise::rejected(Error::new(format!("Illegal argument: TextEditor({id})")))
}

/// Uma das mudanças de `editorChanges`: `{ original, modified, changes }` com os `LineRangeMapping`.
struct EditorChange {
    original: URI,
    modified: URI,
    changes: Vec<LineRangeMapping>,
}

type EditorChanges = Option<Rc<Vec<EditorChange>>>;
type DiffInformation = Option<Rc<Vec<ITextEditorDiffInformation>>>;

/// `isTextEditorDiffInformationEqual(uriIdentityService, diff1, diff2)`.
fn is_text_editor_diff_information_equal(uri_identity_service: &dyn IUriIdentityService, diff1: &ITextEditorDiffInformation, diff2: &ITextEditorDiffInformation) -> bool {
    let ext_uri = uri_identity_service.ext_uri();
    diff1.document_version == diff2.document_version
        && ext_uri.is_equal(diff1.original.as_ref(), diff2.original.as_ref(), false)
        && ext_uri.is_equal(Some(&diff1.modified), Some(&diff2.modified), false)
        && diff1.changes == diff2.changes
}

/// `MainThreadTextEditors`.
pub struct MainThreadTextEditors {
    instance_id: String,
    proxy: Rc<dyn ExtHostEditorsShape>,
    to_dispose: Rc<DisposableStore>,
    text_editors_listeners_map: RefCell<HashMap<String, Vec<DisposableRef>>>,
    editor_position_data: RefCell<Option<TextEditorPositionData>>,
    registered_decoration_types: RefCell<HashSet<String>>,
    editor_locator: Weak<dyn IMainThreadEditorLocator>,
    code_editor_service: Rc<dyn ICodeEditorRegistry>,
    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    configuration_service: Rc<dyn IConfigurationService>,
    quick_diff_model_service: Rc<dyn IQuickDiffModelService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    me: Weak<MainThreadTextEditors>,
}

impl MainThreadTextEditors {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        editor_locator: Weak<dyn IMainThreadEditorLocator>,
        ext_host_context: &Rc<dyn IMainContext>,
        code_editor_service: Rc<dyn ICodeEditorRegistry>,
        editor_service: Rc<dyn IEditorService>,
        editor_group_service: Rc<dyn IEditorGroupsService>,
        configuration_service: Rc<dyn IConfigurationService>,
        quick_diff_model_service: Rc<dyn IQuickDiffModelService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let instance_id = INSTANCE_COUNT.with(|count| {
            count.set(count.get() + 1);
            count.get().to_string()
        });
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_EDITORS);

        let this = Rc::new_cyclic(|me| Self {
            instance_id,
            proxy,
            to_dispose: DisposableStore::new(),
            text_editors_listeners_map: RefCell::new(HashMap::new()),
            editor_position_data: RefCell::new(None),
            registered_decoration_types: RefCell::new(HashSet::new()),
            editor_locator,
            code_editor_service,
            editor_service,
            editor_group_service,
            configuration_service,
            quick_diff_model_service,
            uri_identity_service,
            me: me.clone(),
        });

        let update = {
            let weak = this.me.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    this.update_active_and_visible_text_editors();
                }
            }
        };
        let on_visible = update.clone();
        this.to_dispose.add(this.editor_service.on_did_visible_editors_change().listen(move |_| on_visible()));
        let on_remove = update.clone();
        this.to_dispose.add(this.editor_group_service.on_did_remove_group().listen(move |_| on_remove()));
        this.to_dispose.add(this.editor_group_service.on_did_move_group().listen(move |_| update()));

        this
    }

    /// O `this._editorLocator` (o `MainThreadDocumentsAndEditors`, que contém este objeto).
    fn editor_locator(&self) -> Rc<dyn IMainThreadEditorLocator> {
        self.editor_locator.upgrade().expect("o localizador de editores foi descartado")
    }

    /// `handleTextEditorAdded(textEditor)`.
    pub fn handle_text_editor_added(&self, text_editor: &Rc<MainThreadTextEditor>) {
        let id = text_editor.get_id().to_string();
        let to_dispose = DisposableStore::new();
        let mut disposables: Vec<DisposableRef> = Vec::new();

        let proxy = self.proxy.clone();
        let properties_id = id.clone();
        disposables.push(text_editor.on_properties_changed().listen(move |data| {
            proxy.rpc_accept_editor_properties_changed(&properties_id, data.clone());
        }));

        let diff_information_obs = self.get_text_editor_diff_information(text_editor, &to_dispose);
        let proxy = self.proxy.clone();
        let diff_id = id.clone();
        disposables.push(autorun(move |reader| {
            let diff_information = diff_information_obs.read(reader);
            proxy.rpc_accept_editor_diff_information(&diff_id, diff_information.map(|diff_information| (*diff_information).clone()));
        }));
        disposables.push(to_dispose);

        self.text_editors_listeners_map.borrow_mut().insert(id, disposables);
    }

    /// `handleTextEditorRemoved(id)`.
    pub fn handle_text_editor_removed(&self, id: &str) {
        let disposables = self.text_editors_listeners_map.borrow_mut().remove(id);
        if let Some(disposables) = disposables {
            for disposable in disposables {
                disposable.dispose();
            }
        }
    }

    /// `_updateActiveAndVisibleTextEditors()`.
    fn update_active_and_visible_text_editors(&self) {
        // editor columns
        let editor_position_data = self.get_text_editor_position_data();
        let changed = self.editor_position_data.borrow().as_ref() != Some(&editor_position_data);
        if changed {
            *self.editor_position_data.borrow_mut() = Some(editor_position_data.clone());
            self.proxy.rpc_accept_editor_position_data(editor_position_data);
        }
    }

    /// `_getTextEditorPositionData()`.
    fn get_text_editor_position_data(&self) -> TextEditorPositionData {
        let mut result: TextEditorPositionData = BTreeMap::new();
        let locator = self.editor_locator();
        for editor_pane in self.editor_service.visible_editor_panes() {
            if let Some(id) = locator.find_text_editor_id_for(&*editor_pane).filter(|id| !id.is_empty()) {
                result.insert(id, editor_group_to_column(&*self.editor_group_service, &GroupRef::Id(editor_pane.group_id())));
            }
        }
        result
    }

    /// `_getTextEditorDiffInformation(textEditor, toDispose)` (ver as divergências do módulo).
    fn get_text_editor_diff_information(&self, text_editor: &Rc<MainThreadTextEditor>, to_dispose: &Rc<DisposableStore>) -> IObservable<DiffInformation> {
        let Some(code_editor) = text_editor.get_code_editor() else {
            return const_observable(None);
        };

        let editor_model_obs: IObservable<Option<Rc<TextModel>>> = {
            let code_editor = code_editor.clone();
            observable_from_event(code_editor.on_did_change_model(), move |_| je_editor::code_editor_widget::ICodeEditor::get_model(&*code_editor))
        };

        let editor_changes_obs: IObservable<IObservable<EditorChanges>> = {
            let editor_model_obs = editor_model_obs.clone();
            let quick_diff_model_service = self.quick_diff_model_service.clone();
            let to_dispose = to_dispose.clone();
            derived_opts(DerivedOptions::default(), move |reader: &dyn IReader| -> IObservable<EditorChanges> {
                let Some(editor_model) = editor_model_obs.read(reader) else {
                    return const_observable(None);
                };

                // TextEditor
                let Some(quick_diff_model_ref) = quick_diff_model_service.create_quick_diff_model_reference(&editor_model.uri(), None) else {
                    return const_observable(None);
                };

                let object = quick_diff_model_ref.object.clone();
                to_dispose.add(Rc::new(quick_diff_model_ref));
                let event = object.on_did_change();
                observable_from_event(event, move |_| {
                    let changes: Vec<EditorChange> = object
                        .get_quick_diff_results()
                        .into_iter()
                        .map(|result| EditorChange { original: result.original, modified: result.modified, changes: result.changes2 })
                        .collect();
                    Some(Rc::new(changes))
                })
            })
        };

        let uri_identity_service = self.uri_identity_service.clone();
        let equals_fn: Rc<dyn Fn(&DiffInformation, &DiffInformation) -> bool> = Rc::new(move |diff1, diff2| match (diff1, diff2) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                Rc::ptr_eq(a, b)
                    || (a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| is_text_editor_diff_information_equal(&*uri_identity_service, a, b)))
            }
            _ => false,
        });

        derived_opts(DerivedOptions { equals_fn: Some(equals_fn), ..DerivedOptions::default() }, move |reader: &dyn IReader| -> DiffInformation {
            let editor_model = editor_model_obs.read(reader);
            let editor_changes = editor_changes_obs.read(reader).read(reader);
            let (Some(editor_model), Some(editor_changes)) = (editor_model, editor_changes) else {
                return None;
            };

            let document_version = editor_model.get_version_id();

            Some(Rc::new(
                editor_changes
                    .iter()
                    .map(|change| ITextEditorDiffInformation {
                        document_version,
                        original: Some(change.original.clone()),
                        modified: change.modified.clone(),
                        changes: change
                            .changes
                            .iter()
                            .map(|change| {
                                [
                                    change.original.start_line_number,
                                    change.original.end_line_number_exclusive,
                                    change.modified.start_line_number,
                                    change.modified.end_line_number_exclusive,
                                ]
                            })
                            .collect(),
                    })
                    .collect(),
            ))
        })
    }
}

impl MainThreadTextEditorsShape for MainThreadTextEditors {
    /// `$tryShowTextDocument(resource, options)`.
    fn rpc_try_show_text_document(&self, resource: &URI, options: TextDocumentShowOptions) -> Promise<Option<String>> {
        let uri = resource.clone();

        let editor_options = TextEditorOptions {
            base: EditorOptions {
                preserve_focus: options.preserve_focus,
                pinned: options.pinned,
                // preserve pre 1.38 behaviour to not make group active when preserveFocus: true
                // but make sure to restore the editor to fix https://github.com/microsoft/vscode/issues/79633
                activation: if options.preserve_focus == Some(true) { Some(EditorActivation::Restore) } else { None },
                override_: Some(EditorOverride::Resolution(EditorResolution::ExclusiveOnly)),
                ..EditorOptions::default()
            },
            selection: options.selection.map(|selection| TextEditorSelection {
                start_line_number: f64::from(selection.start_line_number),
                start_column: f64::from(selection.start_column),
                end_line_number: Some(f64::from(selection.end_line_number)),
                end_column: Some(f64::from(selection.end_column)),
            }),
            ..TextEditorOptions::default()
        };

        let input = UntypedEditorInput { resource: Some(uri), options: Some(editor_options), ..UntypedEditorInput::default() };

        let group = column_to_editor_group(&*self.editor_group_service, &*self.configuration_service, options.position);
        let editor_service = self.editor_service.clone();
        let locator = self.editor_locator();
        async_fn(async move {
            let Some(editor) = editor_service.open_editor_in(input, Some(PreferredGroup::Id(group))).await? else {
                return Ok(None);
            };
            // Composite editors are made up of many editors so we return the active one at the time of opening
            let code_editor: Option<Rc<dyn IActiveCodeEditor>> = match editor.get_control() {
                Some(EditorControl::Text(ActiveTextEditorControl::Code(code_editor))) => Some(code_editor),
                Some(EditorControl::Text(ActiveTextEditorControl::Diff { modified, .. })) => Some(modified),
                Some(EditorControl::Composite { active_code_editor }) => active_code_editor,
                _ => None,
            };
            Ok(code_editor.and_then(|code_editor| locator.get_id_of_code_editor(&*code_editor)))
        })
    }

    /// `$registerTextEditorDecorationType(extensionId, key, options)`.
    fn rpc_register_text_editor_decoration_type(&self, extension_id: &ExtensionIdentifier, key: &str, options: DecorationRenderOptions) {
        let key = format!("{}-{key}", self.instance_id);
        self.registered_decoration_types.borrow_mut().insert(key.clone());
        self.code_editor_service.decoration_service().register_decoration_type(&format!("exthost-api-{}", extension_id.value), &key, options, None);
    }

    /// `$removeTextEditorDecorationType(key)`.
    fn rpc_remove_text_editor_decoration_type(&self, key: &str) {
        let key = format!("{}-{key}", self.instance_id);
        self.registered_decoration_types.borrow_mut().remove(&key);
        self.code_editor_service.decoration_service().remove_decoration_type(&key);
    }

    /// `$tryShowEditor(id, position)`.
    fn rpc_try_show_editor(&self, id: &str, position: EditorGroupColumn) -> Promise<()> {
        let main_thread_editor = self.editor_locator().get_editor(id);
        let group = column_to_editor_group(&*self.editor_group_service, &*self.configuration_service, Some(position));
        let editor_service = self.editor_service.clone();
        async_fn(async move {
            if let Some(main_thread_editor) = main_thread_editor {
                let model = main_thread_editor.get_model();
                let input = UntypedEditorInput {
                    resource: Some(model.uri()),
                    options: Some(TextEditorOptions::from(EditorOptions { preserve_focus: Some(false), ..EditorOptions::default() })),
                    ..UntypedEditorInput::default()
                };
                editor_service.open_editor_in(input, Some(PreferredGroup::Id(group))).await?;
            }
            Ok(())
        })
    }

    /// `$tryHideEditor(id)`.
    fn rpc_try_hide_editor(&self, id: &str) -> Promise<()> {
        let main_thread_editor = self.editor_locator().get_editor(id);
        let editor_service = self.editor_service.clone();
        let editor_group_service = self.editor_group_service.clone();
        async_fn(async move {
            if let Some(main_thread_editor) = main_thread_editor {
                let editor_panes = editor_service.visible_editor_panes();
                for editor_pane in editor_panes {
                    if main_thread_editor.matches(&*editor_pane) {
                        if let Some(group) = editor_group_service.get_group(editor_pane.group_id()) {
                            group.close_editor(editor_pane.input().as_ref()).await?;
                        }
                        return Ok(());
                    }
                }
            }
            Ok(())
        })
    }

    /// `$trySetOptions(id, options)`.
    fn rpc_try_set_options(&self, id: &str, options: TextEditorConfigurationUpdate) -> Promise<()> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.set_configuration(options);
        Promise::resolved(())
    }

    /// `$trySetDecorations(id, key, ranges)`.
    fn rpc_try_set_decorations(&self, id: &str, key: &str, ranges: Vec<DecorationOptionsDto>) -> Promise<()> {
        let key = format!("{}-{key}", self.instance_id);
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.set_decorations(&key, &ranges);
        Promise::resolved(())
    }

    /// `$trySetDecorationsFast(id, key, ranges)`.
    fn rpc_try_set_decorations_fast(&self, id: &str, key: &str, ranges: Vec<f64>) -> Promise<()> {
        let key = format!("{}-{key}", self.instance_id);
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.set_decorations_fast(&key, &ranges);
        Promise::resolved(())
    }

    /// `$tryRevealRange(id, range, revealType)`.
    fn rpc_try_reveal_range(&self, id: &str, range: EditorRange, reveal_type: TextEditorRevealType) -> Promise<()> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.reveal_range(range, reveal_type);
        Promise::resolved(())
    }

    /// `$trySetSelections(id, selections)`.
    fn rpc_try_set_selections(&self, id: &str, selections: Vec<EditorSelection>) -> Promise<()> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.set_selections(&selections);
        Promise::resolved(())
    }

    /// `$tryApplyEdits(id, modelVersionId, edits, opts)`.
    fn rpc_try_apply_edits(&self, id: &str, model_version_id: f64, edits: Vec<SingleEditOperation>, opts: ApplyEditsOptions) -> Promise<bool> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        Promise::resolved(editor.apply_edits(model_version_id, edits, opts))
    }

    /// `$tryInsertSnippet(id, modelVersionId, template, ranges, opts)`.
    fn rpc_try_insert_snippet(&self, id: &str, model_version_id: f64, template: &str, selections: &[EditorRange], opts: SnippetOptions) -> Promise<bool> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return illegal_text_editor(id);
        };
        editor.insert_snippet(model_version_id, template, selections, opts)
    }

    /// `$getDiffInformation(id)`.
    fn rpc_get_diff_information(&self, id: &str) -> Promise<Vec<LineChange>> {
        let Some(editor) = self.editor_locator().get_editor(id) else {
            return Promise::rejected(Error::new("No such TextEditor"));
        };

        let Some(code_editor) = editor.get_code_editor() else {
            return Promise::rejected(Error::new("No such CodeEditor"));
        };

        // `listDiffEditors()` é vazio (ver as divergências do módulo).

        let Some(model) = je_editor::code_editor_widget::ICodeEditor::get_model(&*code_editor) else {
            return Promise::resolved(Vec::new());
        };

        let Some(quick_diff_model_ref) = self.quick_diff_model_service.create_quick_diff_model_reference(&model.uri(), None) else {
            return Promise::resolved(Vec::new());
        };

        let primary_quick_diff = quick_diff_model_ref.object.quick_diffs().into_iter().find(|quick_diff| quick_diff.kind == QuickDiffProviderKind::Primary);
        let primary_quick_diff_changes: Vec<LineChange> = quick_diff_model_ref
            .object
            .changes()
            .into_iter()
            .filter(|change| primary_quick_diff.as_ref().is_some_and(|primary| change.provider_id == primary.id))
            .map(|change| change.change)
            .collect();
        quick_diff_model_ref.dispose();
        Promise::resolved(primary_quick_diff_changes)
    }
}

impl IDisposable for MainThreadTextEditors {
    fn dispose(&self) {
        let listeners: Vec<Vec<DisposableRef>> = self.text_editors_listeners_map.borrow_mut().drain().map(|(_, disposables)| disposables).collect();
        for disposables in listeners {
            for disposable in disposables {
                disposable.dispose();
            }
        }
        self.to_dispose.dispose();
        let decoration_types: Vec<String> = self.registered_decoration_types.borrow_mut().drain().collect();
        for decoration_type in decoration_types {
            self.code_editor_service.decoration_service().remove_decoration_type(&decoration_type);
        }
    }
}
