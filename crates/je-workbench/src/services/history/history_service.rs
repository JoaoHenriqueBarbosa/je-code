//! Porte de `src/vs/workbench/services/history/browser/historyService.ts`: o [`HistoryService`] (histórico de editores
//! abertos, navegação para frente e para trás, reabrir o último editor fechado, próximo e anterior editor usado), as pilhas de
//! navegação ([`EditorNavigationStack`] e [`EditorNavigationStacks`]) e o `EditorHelper` ([`EditorHelper`]).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original. O
//!   `IEditorService` é o contrato [`IEditorService`] (o original o tipa `EditorServiceImpl` só para ler
//!   `onDidOpenEditorFail`, `onDidCloseEditor` e `onDidMostRecentlyActiveEditorsChange`, que o contrato do porte já tem).
//!   `registerSingleton(IHistoryService, HistoryService, InstantiationType.Eager)` é [`register`].
//! - **`IWorkbenchLayoutService` e o mouse.** A navegação pelos botões 4 e 5 do mouse registra `mousedown` e `mouseup` no
//!   contêiner DOM (`onDidAddContainer`). A UI nativa não tem contêiner DOM: o `HistoryService` não depende do serviço de
//!   layout, e o corpo de `onMouseDownOrUp` é [`HistoryService::on_mouse_down_or_up`], que a camada de entrada nativa chama
//!   (a leitura de `workbench.editor.mouseBackForwardToNavigate` que decide registrar o ouvinte vira a guarda do método).
//!   O método devolve se o evento foi tratado (o `EventHelper.stop`).
//! - **`WindowIdleValue(mainWindow, ...)`.** É o [`GlobalIdleValue`] (o porte tem uma janela principal só).
//! - **Uniões de entrada.** `EditorInput | IResourceEditorInput` é [`EditorInputOrUntyped`]; o argumento
//!   `EditorInput | IResourceEditorInput | FileChangesEvent | FileOperationEvent` dos métodos do `EditorHelper` é o
//!   [`EditorMatchTarget`], e o `EditorInput | FileChangesEvent | FileOperationEvent | GroupIdentifier` do `remove` da pilha é o
//!   [`NavigationRemoval`]. As sobrecargas de `remove` do serviço (`EditorInput`, `FileChangesEvent`, `FileOperationEvent`) são
//!   o [`EditorMatchTarget`].
//! - **Identidade de objetos.** As chaves de `Map<EditorInput, ...>` e `DisposableMap<EditorInput, ...>` são o [`ById`] (o
//!   endereço do `Rc`). Os ouvintes de `onWillDispose` guardam um `Weak` da entrada para a entrada não segurar a si mesma.
//! - **`Selection` serializável.** `JSON.stringify` do `IResourceEditorInput` do histórico grava os campos de
//!   `IResourceEditorInput`, `ITextResourceEditorInput` e `IUntitledTextResourceEditorInput` (`resource`, `label`,
//!   `description`, `contents`, `encoding`, `languageId`, `forceUntitled`, `forceFile`) e as `options` (as de `IEditorOptions` e a
//!   seleção do `ITextEditorOptions`); o `auxiliary` e o `modal` (limites de janela e funções de renderização) não são gravados.
//! - **Métodos novos de teste.** [`HistoryService::editor_history_listeners_size_for_testing`] expõe o tamanho do mapa privado
//!   `editorHistoryListeners` que o caso `editor dispose clears history listener` lê por `as unknown as`.
//! - **`async`.** As funções `async` do original são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`.
//!   As promessas que o original não aguarda são descartadas.
//! - **`move`.** `EditorNavigationStack.move` é [`EditorNavigationStack::move_`] (palavra reservada).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{GlobalIdleValue, async_fn};
use je_base::dynamic::{Dynamic, Truthy, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::{self, Emitter, Event};
use je_base::json::{Object as JsonObject, Value as JsonValue};
use je_base::lifecycle::{ById, Disposable, DisposableMap, DisposableRef, DisposableStore, IDisposable, id_of, to_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_instantiation::{InstantiationService, Injectable, register_singleton, service_impl, InstantiationType};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::files::files::{FileChangeType, FileChangesEvent, FileOperation, FileOperationEvent};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogLevel, LogService, Logger};
use je_platform::opener::editor_types::{
    EditorActivation, EditorOpenSource, EditorOptions, EditorOverride, EditorResolution, TextEditorSelection,
};
use je_platform::path::path_service::IPathService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_sync::abstract_synchronizer::parse_json;
use je_platform::workspace::IWorkspaceContextService;
use je_platform::workspaces::workspaces::IWorkspacesService;
use je_storage::StorageValue;

use super::history::{GoFilter, GoScope, I_HISTORY_SERVICE, IHistoryService, MOUSE_BACK_FORWARD_NAVIGATION_SETTING};
use crate::common::editor::editor_group_model::EditorOrIndex;
use crate::common::editor::{
    EditorCloseContext, EditorCloseEvent, EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorPaneSelectionChangeEvent, EditorPaneSelectionChangeReason,
    EditorPaneSelectionCompareResult, EditorResourceAccessor, EditorResourceAccessorOptions, EditorUri, EditorsOrder, GroupIdentifier, GroupModelChangeKind,
    IEditorPane, IEditorPaneSelection, SchemeFilter, SideBySideEditor, TextEditorOptions, TextEditorSelectionRevealType, UntypedEditorInput,
    is_editor_pane_with_selection, is_resource_editor_input, side_by_side_input_sides,
};
use crate::common::editor::editor_input::EditorInput;
use crate::common::resources::{GetExpression, ResourceGlobMatcher, ShouldUpdate};
use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_service::IEditorService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};
use crate::services::search::search::get_excludes_value;

const NAVIGATION_SCOPE_SETTING: &str = "workbench.editor.navigationScope";
const MAX_RECENTLY_CLOSED_EDITORS: usize = 20;
const MAX_HISTORY_ITEMS: usize = 200;
const HISTORY_STORAGE_KEY: &str = "history.entries";
const MAX_STACK_SIZE: usize = 50;

/// `FILES_EXCLUDE_CONFIG`.
const FILES_EXCLUDE_CONFIG: &str = "files.exclude";
/// `SEARCH_EXCLUDE_CONFIG`.
const SEARCH_EXCLUDE_CONFIG: &str = "search.exclude";

/// O mapa de listeners de descarte de entradas (`DisposableMap<EditorInput, DisposableStore>`).
type EditorDisposables = Rc<DisposableMap<ById<dyn EditorInput>, DisposableStore>>;

/// `resource?.toString()`.
fn uri_string(resource: Option<&URI>) -> String {
    resource.map_or_else(|| "undefined".to_string(), |resource| resource.to_string(false).to_string_lossy())
}

/// `editor.resource` de uma entrada tipada ou sem tipo.
fn resource_of(editor: &EditorInputOrUntyped) -> Option<URI> {
    match editor {
        EditorInputOrUntyped::Typed(editor) => editor.resource(),
        EditorInputOrUntyped::Untyped(editor) => editor.resource.clone(),
    }
}

/// `isEditorInput(editor)`.
fn is_typed(editor: &EditorInputOrUntyped) -> bool {
    matches!(editor, EditorInputOrUntyped::Typed(_))
}

//#region Serialização do histórico

/// `ISerializedEditorHistoryEntry`: `{ editor: { ...editor, resource: string } }`.
fn serialize_history_entry(editor: &UntypedEditorInput, resource: &URI) -> Dynamic {
    let mut entries: Vec<(&'static str, Dynamic)> = Vec::new();
    if let Some(options) = &editor.options {
        entries.push(("options", serialize_options(options)));
    }
    for (key, value) in [("label", &editor.label), ("description", &editor.description), ("contents", &editor.contents), ("encoding", &editor.encoding), ("languageId", &editor.language_id)] {
        if let Some(value) = value {
            entries.push((key, Dynamic::string(value)));
        }
    }
    for (key, value) in [("forceUntitled", editor.force_untitled), ("forceFile", editor.force_file)] {
        if let Some(value) = value {
            entries.push((key, Dynamic::Bool(value)));
        }
    }
    entries.push(("resource", Dynamic::string(&resource.to_string(false).to_string_lossy())));

    Dynamic::object([("editor", Dynamic::object(entries))])
}

/// As `IEditorOptions` e a seleção do `ITextEditorOptions` como o objeto que o `JSON.stringify` grava.
fn serialize_options(options: &TextEditorOptions) -> Dynamic {
    let base = &options.base;
    let mut entries: Vec<(&'static str, Dynamic)> = Vec::new();
    for (key, value) in [
        ("preserveFocus", base.preserve_focus),
        ("forceReload", base.force_reload),
        ("revealIfVisible", base.reveal_if_visible),
        ("revealIfOpened", base.reveal_if_opened),
        ("pinned", base.pinned),
        ("sticky", base.sticky),
        ("inactive", base.inactive),
        ("ignoreError", base.ignore_error),
        ("isExplicit", base.is_explicit),
        ("transient", base.transient),
    ] {
        if let Some(value) = value {
            entries.push((key, Dynamic::Bool(value)));
        }
    }
    if let Some(activation) = base.activation {
        entries.push(("activation", Dynamic::Number(f64::from(activation as i32))));
    }
    if let Some(index) = base.index {
        entries.push(("index", Dynamic::Number(index)));
    }
    match &base.override_ {
        Some(EditorOverride::Id(id)) => entries.push(("override", Dynamic::String(id.clone()))),
        Some(EditorOverride::Resolution(resolution)) => entries.push(("override", Dynamic::Number(f64::from(*resolution as i32)))),
        None => {}
    }
    if let Some(source) = base.source {
        entries.push(("source", Dynamic::Number(f64::from(source as i32))));
    }
    if let Some(view_state) = &base.view_state {
        entries.push(("viewState", view_state.clone()));
    }
    if let Some(selection) = &options.selection {
        let mut selection_entries = vec![("startLineNumber", Dynamic::Number(selection.start_line_number)), ("startColumn", Dynamic::Number(selection.start_column))];
        if let Some(end_line_number) = selection.end_line_number {
            selection_entries.push(("endLineNumber", Dynamic::Number(end_line_number)));
        }
        if let Some(end_column) = selection.end_column {
            selection_entries.push(("endColumn", Dynamic::Number(end_column)));
        }
        entries.push(("selection", Dynamic::object(selection_entries)));
    }
    if let Some(reveal_type) = options.selection_reveal_type {
        entries.push(("selectionRevealType", Dynamic::Number(f64::from(reveal_type as i32))));
    }
    if let Some(source) = &options.selection_source {
        entries.push(("selectionSource", Dynamic::string(source)));
    }

    Dynamic::object(entries)
}

fn json_bool(object: &JsonObject, key: &str) -> Option<bool> {
    match object.get(key) {
        Some(JsonValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn json_number(object: &JsonObject, key: &str) -> Option<f64> {
    match object.get(key) {
        Some(JsonValue::Number(value)) => Some(*value),
        _ => None,
    }
}

fn json_string(object: &JsonObject, key: &str) -> Option<String> {
    match object.get(key) {
        Some(JsonValue::String(value)) => Some(value.to_string_lossy()),
        _ => None,
    }
}

/// O inverso de [`serialize_options`].
fn deserialize_options(object: &JsonObject) -> TextEditorOptions {
    let mut options = TextEditorOptions::default();
    {
        let base: &mut EditorOptions = &mut options.base;
        base.preserve_focus = json_bool(object, "preserveFocus");
        base.force_reload = json_bool(object, "forceReload");
        base.reveal_if_visible = json_bool(object, "revealIfVisible");
        base.reveal_if_opened = json_bool(object, "revealIfOpened");
        base.pinned = json_bool(object, "pinned");
        base.sticky = json_bool(object, "sticky");
        base.inactive = json_bool(object, "inactive");
        base.ignore_error = json_bool(object, "ignoreError");
        base.is_explicit = json_bool(object, "isExplicit");
        base.transient = json_bool(object, "transient");
        base.index = json_number(object, "index");
        base.activation = json_number(object, "activation").and_then(|activation| match activation as i32 {
            1 => Some(EditorActivation::Activate),
            2 => Some(EditorActivation::Restore),
            3 => Some(EditorActivation::Preserve),
            _ => None,
        });
        base.override_ = match object.get("override") {
            Some(JsonValue::String(id)) => Some(EditorOverride::Id(id.clone())),
            Some(JsonValue::Number(resolution)) if *resolution as i32 == EditorResolution::ExclusiveOnly as i32 => Some(EditorOverride::Resolution(EditorResolution::ExclusiveOnly)),
            _ => None,
        };
        base.source = json_number(object, "source").and_then(|source| match source as i32 {
            0 => Some(EditorOpenSource::Api),
            1 => Some(EditorOpenSource::User),
            _ => None,
        });
        base.view_state = object.get("viewState").map(Dynamic::from_json);
    }
    if let Some(JsonValue::Object(selection)) = object.get("selection")
        && let (Some(start_line_number), Some(start_column)) = (json_number(selection, "startLineNumber"), json_number(selection, "startColumn"))
    {
        options.selection = Some(TextEditorSelection {
            start_line_number,
            start_column,
            end_line_number: json_number(selection, "endLineNumber"),
            end_column: json_number(selection, "endColumn"),
        });
    }
    options.selection_reveal_type = json_number(object, "selectionRevealType").and_then(|reveal_type| match reveal_type as i32 {
        0 => Some(TextEditorSelectionRevealType::Center),
        1 => Some(TextEditorSelectionRevealType::CenterIfOutsideViewport),
        2 => Some(TextEditorSelectionRevealType::NearTop),
        3 => Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
        _ => None,
    });
    options.selection_source = json_string(object, "selectionSource");

    options
}

/// `URI.parse(resource)` do formato novo (texto) ou `URI.from(resource)` do formato antigo (`URI.toJSON()`).
fn deserialize_resource(resource: &JsonValue) -> Option<Result<URI, Error>> {
    match resource {
        JsonValue::String(resource) => Some(errors::catch(|| URI::parse(resource.clone(), false))),
        JsonValue::Object(components) => {
            let component = |key: &str| json_string(components, key).map(JsString::from);
            let components = UriComponents { scheme: component("scheme"), authority: component("authority"), path: component("path"), query: component("query"), fragment: component("fragment") };

            Some(errors::catch(|| URI::from(&components, false)))
        }
        _ => None,
    }
}

//#endregion

/// `IRecentlyClosedEditor`.
struct RecentlyClosedEditor {
    editor_id: Option<String>,
    editor: UntypedEditorInput,
    resource: Option<URI>,
    associated_resources: Vec<URI>,
    index: usize,
    sticky: bool,
    /// Identifies the batch of editors that were closed together (e.g. via "Close All Editors" or "Close Others"). Editors
    /// sharing the same batch identifier are reopened together by "Reopen Closed Editor".
    batch_id: u64,
}

/// O argumento `EditorInput | IResourceEditorInput | FileChangesEvent | FileOperationEvent` dos métodos do [`EditorHelper`].
#[derive(Clone)]
pub enum EditorMatchTarget {
    Editor(EditorInputOrUntyped),
    FileChanges(Rc<FileChangesEvent>),
    FileOperation(FileOperationEvent),
}

impl From<EditorInputOrUntyped> for EditorMatchTarget {
    fn from(editor: EditorInputOrUntyped) -> Self {
        EditorMatchTarget::Editor(editor)
    }
}

impl From<&EditorInputRef> for EditorMatchTarget {
    fn from(editor: &EditorInputRef) -> Self {
        EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(editor.clone()))
    }
}

impl From<Rc<FileChangesEvent>> for EditorMatchTarget {
    fn from(event: Rc<FileChangesEvent>) -> Self {
        EditorMatchTarget::FileChanges(event)
    }
}

impl From<FileOperationEvent> for EditorMatchTarget {
    fn from(event: FileOperationEvent) -> Self {
        EditorMatchTarget::FileOperation(event)
    }
}

/// O argumento `EditorInput | FileChangesEvent | FileOperationEvent | GroupIdentifier` do `remove` da pilha de navegação.
#[derive(Clone)]
pub enum NavigationRemoval {
    Target(EditorMatchTarget),
    Group(GroupIdentifier),
}

impl From<EditorMatchTarget> for NavigationRemoval {
    fn from(target: EditorMatchTarget) -> Self {
        NavigationRemoval::Target(target)
    }
}

impl From<&EditorInputRef> for NavigationRemoval {
    fn from(editor: &EditorInputRef) -> Self {
        NavigationRemoval::Target(editor.into())
    }
}

impl From<FileOperationEvent> for NavigationRemoval {
    fn from(event: FileOperationEvent) -> Self {
        NavigationRemoval::Target(event.into())
    }
}

impl From<FileChangesEvent> for NavigationRemoval {
    fn from(event: FileChangesEvent) -> Self {
        NavigationRemoval::Target(Rc::new(event).into())
    }
}

impl From<Rc<FileChangesEvent>> for NavigationRemoval {
    fn from(event: Rc<FileChangesEvent>) -> Self {
        NavigationRemoval::Target(event.into())
    }
}

impl From<GroupIdentifier> for NavigationRemoval {
    fn from(group_id: GroupIdentifier) -> Self {
        NavigationRemoval::Group(group_id)
    }
}

/// Os contextos do histórico: as chaves `canNavigateBack`, `canNavigateForward` e as demais.
struct HistoryContextKeys {
    can_navigate_back: Rc<dyn IContextKey>,
    can_navigate_forward: Rc<dyn IContextKey>,
    can_navigate_back_in_navigations: Rc<dyn IContextKey>,
    can_navigate_forward_in_navigations: Rc<dyn IContextKey>,
    can_navigate_to_last_navigation_location: Rc<dyn IContextKey>,
    can_navigate_back_in_edits: Rc<dyn IContextKey>,
    can_navigate_forward_in_edits: Rc<dyn IContextKey>,
    can_navigate_to_last_edit_location: Rc<dyn IContextKey>,
    can_reopen_closed_editor: Rc<dyn IContextKey>,
}

impl HistoryContextKeys {
    fn new(context_key_service: &dyn IContextKeyService) -> Self {
        let bind = |key: &str, description: String| RawContextKey::new(key, false, ContextKeyMeta::Description(description)).bind_to(context_key_service);

        Self {
            can_navigate_back: bind("canNavigateBack", localize("canNavigateBack", "Whether it is possible to navigate back in editor history", &[])),
            can_navigate_forward: bind("canNavigateForward", localize("canNavigateForward", "Whether it is possible to navigate forward in editor history", &[])),
            can_navigate_back_in_navigations: bind(
                "canNavigateBackInNavigationLocations",
                localize("canNavigateBackInNavigationLocations", "Whether it is possible to navigate back in editor navigation locations history", &[]),
            ),
            can_navigate_forward_in_navigations: bind(
                "canNavigateForwardInNavigationLocations",
                localize("canNavigateForwardInNavigationLocations", "Whether it is possible to navigate forward in editor navigation locations history", &[]),
            ),
            can_navigate_to_last_navigation_location: bind(
                "canNavigateToLastNavigationLocation",
                localize("canNavigateToLastNavigationLocation", "Whether it is possible to navigate to the last editor navigation location", &[]),
            ),
            can_navigate_back_in_edits: bind(
                "canNavigateBackInEditLocations",
                localize("canNavigateBackInEditLocations", "Whether it is possible to navigate back in editor edit locations history", &[]),
            ),
            can_navigate_forward_in_edits: bind(
                "canNavigateForwardInEditLocations",
                localize("canNavigateForwardInEditLocations", "Whether it is possible to navigate forward in editor edit locations history", &[]),
            ),
            can_navigate_to_last_edit_location: bind(
                "canNavigateToLastEditLocation",
                localize("canNavigateToLastEditLocation", "Whether it is possible to navigate to the last editor edit location", &[]),
            ),
            can_reopen_closed_editor: bind("canReopenClosedEditor", localize("canReopenClosedEditor", "Whether it is possible to reopen the last closed editor", &[])),
        }
    }
}

/// O que o serviço guarda por conjunto de pilhas escopado (`{ stack, disposable }`).
#[derive(Clone)]
struct ScopedStacks {
    stack: Rc<dyn IEditorNavigationStacks>,
    disposable: Rc<DisposableStore>,
}

/// Os serviços injetados no [`HistoryService`], na ordem do construtor do original (sem o `IWorkbenchLayoutService`).
type HistoryServiceDeps = (
    Rc<dyn IEditorService>,
    Rc<dyn IEditorGroupsService>,
    Rc<dyn IWorkspaceContextService>,
    Rc<dyn StorageService>,
    Rc<dyn IConfigurationService>,
    Rc<dyn IFileService>,
    Rc<dyn IWorkspacesService>,
    Rc<InstantiationService>,
    Rc<dyn IContextKeyService>,
    Rc<dyn LogService>,
);

/// `HistoryService`.
pub struct HistoryService {
    this: Weak<HistoryService>,
    base: Rc<Disposable>,

    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    storage_service: Rc<dyn StorageService>,
    configuration_service: Rc<dyn IConfigurationService>,
    file_service: Rc<dyn IFileService>,
    workspaces_service: Rc<dyn IWorkspacesService>,
    instantiation_service: Rc<InstantiationService>,
    context_key_service: Rc<dyn IContextKeyService>,
    log_service: Rc<dyn LogService>,

    active_editor_listeners: Rc<DisposableStore>,
    last_active_editor: RefCell<Option<EditorIdentifier>>,

    editor_helper: Rc<EditorHelper>,

    context_keys: HistoryContextKeys,

    // region: Editor History Navigation (limit: 50)
    on_did_change_editor_navigation_stack: Rc<Emitter<()>>,
    default_scoped_editor_navigation_stack: RefCell<Option<Rc<dyn IEditorNavigationStacks>>>,
    editor_group_scoped_navigation_stacks: RefCell<HashMap<GroupIdentifier, ScopedStacks>>,
    editor_scoped_navigation_stacks: RefCell<HashMap<GroupIdentifier, HashMap<ById<dyn EditorInput>, ScopedStacks>>>,
    editor_navigation_scope: Cell<GoScope>,

    // region: Navigation: Next/Previous Used Editor
    recently_used_editors_stack: RefCell<Option<Vec<EditorIdentifier>>>,
    recently_used_editors_stack_index: Cell<i64>,
    recently_used_editors_in_group_stack: RefCell<Option<Vec<EditorIdentifier>>>,
    recently_used_editors_in_group_stack_index: Cell<i64>,
    navigating_in_recently_used_editors_stack: Cell<bool>,
    navigating_in_recently_used_editors_in_group_stack: Cell<bool>,

    // region: File: Reopen Closed Editor (limit: 20)
    recently_closed_editors: RefCell<Vec<RecentlyClosedEditor>>,
    ignore_editor_close_event: Cell<bool>,
    recently_closed_editors_batch_id: Cell<u64>,
    recently_closed_editors_batch_scheduled: Cell<bool>,

    // region: Go to: Recently Opened Editor (limit: 200, persisted)
    history: RefCell<Option<Vec<EditorInputOrUntyped>>>,
    editor_history_listeners: EditorDisposables,
    resource_exclude_matcher: Rc<GlobalIdleValue<Rc<ResourceGlobMatcher>>>,
}

impl Injectable for HistoryService {
    type Args = ();
    type Deps = HistoryServiceDeps;

    fn create(
        _args: Self::Args,
        (editor_service, editor_group_service, context_service, storage_service, configuration_service, file_service, workspaces_service, instantiation_service, context_key_service, log_service): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let editor_helper = instantiation_service.create_instance::<EditorHelper>(());
        let context_keys = HistoryContextKeys::new(&*context_key_service);

        let active_editor_listeners = DisposableStore::new();
        base.register(active_editor_listeners.clone());
        let on_did_change_editor_navigation_stack = Emitter::new();
        base.register(on_did_change_editor_navigation_stack.clone());
        let editor_history_listeners: EditorDisposables = DisposableMap::new();
        base.register(editor_history_listeners.clone());

        let this = Rc::new_cyclic(|this: &Weak<HistoryService>| {
            // `resourceExcludeMatcher`
            let resource_exclude_matcher = {
                let (idle_this, idle_base, idle_configuration, idle_context) = (this.clone(), base.clone(), configuration_service.clone(), context_service.clone());
                let idle_value = Rc::new(GlobalIdleValue::new(move || {
                    let expression_configuration = idle_configuration.clone();
                    let get_expression: GetExpression = Rc::new(move |root: Option<&URI>| {
                        let configuration = match root {
                            Some(root) => expression_configuration.get_value(None, Some(&ConfigurationOverrides { resource: Some(root.clone()), ..ConfigurationOverrides::default() })),
                            None => expression_configuration.get_value(None, None),
                        };

                        Some(get_excludes_value(&configuration, true).unwrap_or_else(|| Dynamic::object(Vec::<(&str, Dynamic)>::new())))
                    });
                    let should_update: ShouldUpdate = Rc::new(|event| event.affects_configuration(FILES_EXCLUDE_CONFIG, None) || event.affects_configuration(SEARCH_EXCLUDE_CONFIG, None));

                    let matcher = ResourceGlobMatcher::new(get_expression, should_update, idle_context, idle_configuration);
                    idle_base.register(matcher.clone());

                    idle_base.register(matcher.on_expression_change().listen(move |_| {
                        if let Some(this) = idle_this.upgrade() {
                            this.remove_excluded_from_history();
                        }
                    }));

                    matcher
                }));

                let registered = idle_value.clone();
                base.register(to_disposable(move || registered.dispose()));

                idle_value
            };

            Self {
                this: this.clone(),
                base: base.clone(),
                editor_service,
                editor_group_service,
                context_service,
                storage_service,
                configuration_service,
                file_service,
                workspaces_service,
                instantiation_service,
                context_key_service,
                log_service,
                active_editor_listeners,
                last_active_editor: RefCell::new(None),
                editor_helper,
                context_keys,
                on_did_change_editor_navigation_stack,
                default_scoped_editor_navigation_stack: RefCell::new(None),
                editor_group_scoped_navigation_stacks: RefCell::new(HashMap::new()),
                editor_scoped_navigation_stacks: RefCell::new(HashMap::new()),
                editor_navigation_scope: Cell::new(GoScope::Default),
                recently_used_editors_stack: RefCell::new(None),
                recently_used_editors_stack_index: Cell::new(0),
                recently_used_editors_in_group_stack: RefCell::new(None),
                recently_used_editors_in_group_stack_index: Cell::new(0),
                navigating_in_recently_used_editors_stack: Cell::new(false),
                navigating_in_recently_used_editors_in_group_stack: Cell::new(false),
                recently_closed_editors: RefCell::new(Vec::new()),
                ignore_editor_close_event: Cell::new(false),
                recently_closed_editors_batch_id: Cell::new(0),
                recently_closed_editors_batch_scheduled: Cell::new(false),
                history: RefCell::new(None),
                editor_history_listeners,
                resource_exclude_matcher,
            }
        });

        this.register_listeners();

        // if the service is created late enough that an editor is already opened make sure to trigger the
        // onActiveEditorChanged() to track the editor properly (fixes https://github.com/microsoft/vscode/issues/59908)
        if this.editor_service.active_editor_pane().is_some() {
            this.on_did_active_editor_change();
        }

        this
    }
}

service_impl!(HistoryService => dyn IHistoryService, disposable);

/// `registerSingleton(IHistoryService, HistoryService, InstantiationType.Eager)`.
pub fn register() {
    register_singleton::<HistoryService>(I_HISTORY_SERVICE, InstantiationType::Eager);
}

impl IDisposable for HistoryService {
    fn dispose(&self) {
        self.base.dispose();

        if let Some(stack) = self.default_scoped_editor_navigation_stack.borrow_mut().take() {
            stack.dispose();
        }

        let group_stacks: Vec<ScopedStacks> = self.editor_group_scoped_navigation_stacks.borrow().values().cloned().collect();
        for stack in group_stacks {
            stack.disposable.dispose();
        }

        let editor_stacks: Vec<ScopedStacks> = self.editor_scoped_navigation_stacks.borrow().values().flat_map(|editors| editors.values().cloned()).collect();
        for stack in editor_stacks {
            stack.disposable.dispose();
        }

        self.editor_history_listeners.clear_and_dispose_all();
    }
}

impl HistoryService {
    fn rc(&self) -> Rc<HistoryService> {
        self.this.upgrade().expect("o HistoryService está vivo enquanto é usado")
    }

    /// `this._register(event(handler))` com o serviço por `Weak`.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&Rc<HistoryService>, &T) + 'static) {
        let weak = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    fn register_listeners(&self) {
        // Editor changes
        self.listen(self.editor_service.on_did_active_editor_change(), |this, _| this.on_did_active_editor_change());
        self.listen(self.editor_service.on_did_open_editor_fail(), |this, event| this.remove(&EditorMatchTarget::from(&event.editor)));
        self.listen(self.editor_service.on_did_close_editor(), |this, event| this.on_did_close_editor(event));
        self.listen(self.editor_service.on_did_most_recently_active_editors_change(), |this, _| this.handle_editor_event_in_recent_editors_stack());

        // Editor group changes
        self.listen(self.editor_group_service.on_did_remove_group(), |this, group| this.on_did_remove_group(group));

        // File changes
        self.listen(self.file_service.on_did_files_change(), |this, event| this.on_did_files_change(&EditorMatchTarget::FileChanges(event.clone())));
        self.listen(self.file_service.on_did_run_operation(), |this, event| this.on_did_files_change(&EditorMatchTarget::FileOperation(event.clone())));

        // Storage
        self.listen(self.storage_service.on_will_save_state(), |this, _| this.save_state());

        // Configuration
        self.register_editor_navigation_scope_change_listener();

        // Context keys
        self.listen(self.on_did_change_editor_navigation_stack(), |this, _| this.update_context_keys());
        self.listen(self.editor_group_service.on_did_change_active_group(), |this, _| this.update_context_keys());
    }

    fn on_did_close_editor(&self, e: &EditorCloseEvent) {
        self.handle_editor_close_event_in_history(e);
        self.handle_editor_close_event_in_reopen(e);
    }

    /// `onMouseDownOrUp(event, isMouseDown)`: support to navigate in history when mouse buttons 4/5 are pressed. We want to
    /// trigger this on mouse down for a faster experience but we also need to prevent mouse up from triggering the default
    /// which is to navigate in the browser history. Só vale com `workbench.editor.mouseBackForwardToNavigate` ligado (no
    /// original o ouvinte só é registrado nesse caso). Devolve se o evento foi tratado (e deve ser parado).
    pub fn on_mouse_down_or_up(&self, button: i32, is_mouse_down: bool) -> bool {
        if !self.configuration_service.get_value(Some(MOUSE_BACK_FORWARD_NAVIGATION_SETTING), None).is_truthy() {
            return false;
        }

        match button {
            3 => {
                if is_mouse_down {
                    drop(self.go_back(None));
                }
                true
            }
            4 => {
                if is_mouse_down {
                    drop(self.go_forward(None));
                }
                true
            }
            _ => false,
        }
    }

    fn on_did_remove_group(&self, group: &Rc<dyn IEditorGroup>) {
        self.handle_editor_group_remove_in_navigation_stacks(group);
    }

    fn on_did_active_editor_change(&self) {
        let active_editor_group = self.editor_group_service.active_group();
        let active_editor_pane = active_editor_group.active_editor_pane();

        if let Some(last_active_editor) = &*self.last_active_editor.borrow()
            && self.editor_helper.matches_editor_identifier(last_active_editor, active_editor_pane.as_ref())
        {
            return; // return if the active editor is still the same
        }

        // Remember as last active editor (can be undefined if none opened)
        *self.last_active_editor.borrow_mut() = active_editor_pane
            .as_ref()
            .and_then(|pane| pane.input().map(|input| EditorIdentifier { editor: input, group_id: pane.group_id() }));

        // Dispose old listeners
        self.active_editor_listeners.clear();

        // Handle editor change unless the editor is transient. In that case setup a listener to see if the transient editor
        // becomes non-transient (https://github.com/microsoft/vscode/issues/211769)
        let is_transient = |pane: &Rc<dyn IEditorPane>| pane.input().is_some_and(|input| active_editor_group.is_transient(&EditorOrIndex::Editor(input)));
        match &active_editor_pane {
            Some(pane) if is_transient(pane) => {
                self.log_service.trace(&format!(
                    "[History]: ignoring transient editor change until becoming non-transient (editor: {}}})",
                    uri_string(pane.input().and_then(|input| input.resource()).as_ref())
                ));

                let transient_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                let listener = active_editor_group.on_did_model_change().listen({
                    let (weak, group, pane, transient_listener) = (self.this.clone(), active_editor_group.clone(), pane.clone(), transient_listener.clone());
                    move |e| {
                        let pane_input = pane.input();
                        let is_pane_input = match (&e.editor, &pane_input) {
                            (Some(editor), Some(input)) => id_of(editor) == id_of(input),
                            _ => false,
                        };
                        if e.kind == GroupModelChangeKind::EditorTransient && is_pane_input && !pane_input.is_some_and(|input| group.is_transient(&EditorOrIndex::Editor(input))) {
                            if let Some(listener) = transient_listener.borrow().as_ref() {
                                listener.dispose();
                            }

                            if let Some(this) = weak.upgrade() {
                                this.handle_active_editor_change(&group, Some(&pane));
                            }
                        }
                    }
                });
                *transient_listener.borrow_mut() = Some(listener.clone());

                self.active_editor_listeners.add(listener);
            }
            _ => self.handle_active_editor_change(&active_editor_group, active_editor_pane.as_ref()),
        }

        // Listen to selection changes unless the editor is transient
        if is_editor_pane_with_selection(active_editor_pane.as_ref())
            && let Some(pane) = &active_editor_pane
            && let Some(on_did_change_selection) = pane.on_did_change_selection()
        {
            let (weak, group, pane) = (self.this.clone(), active_editor_group.clone(), pane.clone());
            self.active_editor_listeners.add(on_did_change_selection.listen(move |e| {
                let Some(this) = weak.upgrade() else {
                    return;
                };

                if !pane.input().is_some_and(|input| group.is_transient(&EditorOrIndex::Editor(input))) {
                    this.handle_active_editor_selection_change_event(&group, &pane, *e);
                } else {
                    this.log_service.trace(&format!(
                        "[History]: ignoring transient editor selection change (editor: {}}})",
                        uri_string(pane.input().and_then(|input| input.resource()).as_ref())
                    ));
                }
            }));
        }

        // Context keys
        self.update_context_keys();
    }

    fn on_did_files_change(&self, event: &EditorMatchTarget) {
        match event {
            // External file changes (watcher)
            EditorMatchTarget::FileChanges(event) => {
                if event.got_deleted() {
                    self.remove(&EditorMatchTarget::FileChanges(event.clone()));
                }
            }

            // Internal file changes (e.g. explorer)
            EditorMatchTarget::FileOperation(event) => {
                // Delete
                if event.is_operation(FileOperation::Delete) {
                    self.remove(&EditorMatchTarget::FileOperation(event.clone()));
                }
                // Move
                else if event.is_operation(FileOperation::Move) && event.target.as_ref().is_some_and(|target| target.is_file) {
                    self.move_(event);
                }
            }

            EditorMatchTarget::Editor(_) => {}
        }
    }

    fn handle_active_editor_change(&self, group: &Rc<dyn IEditorGroup>, editor_pane: Option<&Rc<dyn IEditorPane>>) {
        self.handle_active_editor_change_in_history(editor_pane);
        self.handle_active_editor_change_in_navigation_stacks(group, editor_pane);
    }

    fn handle_active_editor_selection_change_event(&self, group: &Rc<dyn IEditorGroup>, editor_pane: &Rc<dyn IEditorPane>, event: EditorPaneSelectionChangeEvent) {
        self.handle_active_editor_selection_change_in_navigation_stacks(group, editor_pane, event);
    }

    fn move_(&self, event: &FileOperationEvent) {
        self.move_in_history(event);
        self.move_in_editor_navigation_stacks(event);
    }

    /// `remove(arg1)`: as sobrecargas de `EditorInput`, `FileChangesEvent` e `FileOperationEvent`.
    fn remove(&self, arg1: &EditorMatchTarget) {
        self.remove_from_history_target(arg1);
        self.remove_from_editor_navigation_stacks(arg1);
        self.remove_from_recently_closed_editors(arg1);
        self.remove_from_recently_opened(arg1);
    }

    fn remove_from_recently_opened(&self, arg1: &EditorMatchTarget) {
        let resource = match arg1 {
            EditorMatchTarget::Editor(editor @ EditorInputOrUntyped::Typed(_)) => EditorResourceAccessor::get_original_uri(Some(editor), None),
            EditorMatchTarget::Editor(EditorInputOrUntyped::Untyped(editor)) => editor.resource.clone(),
            // Ignore for now (recently opened are most often out of workspace files anyway for which there are no file events)
            EditorMatchTarget::FileChanges(_) => None,
            EditorMatchTarget::FileOperation(event) => Some(event.resource.clone()),
        };

        if let Some(resource) = resource {
            drop(self.workspaces_service.remove_recently_opened(vec![resource]));
        }
    }

    //#region History Context Keys

    /// `updateContextKeys()`.
    pub fn update_context_keys(&self) {
        self.context_key_service.buffer_change_events(&|| {
            let active_stack = self.get_stack(None, None);
            let keys = &self.context_keys;
            let set = |key: &Rc<dyn IContextKey>, value: bool| key.set(Dynamic::Bool(value));

            set(&keys.can_navigate_back, active_stack.can_go_back(Some(GoFilter::None)));
            set(&keys.can_navigate_forward, active_stack.can_go_forward(Some(GoFilter::None)));

            set(&keys.can_navigate_back_in_navigations, active_stack.can_go_back(Some(GoFilter::Navigation)));
            set(&keys.can_navigate_forward_in_navigations, active_stack.can_go_forward(Some(GoFilter::Navigation)));
            set(&keys.can_navigate_to_last_navigation_location, active_stack.can_go_last(Some(GoFilter::Navigation)));

            set(&keys.can_navigate_back_in_edits, active_stack.can_go_back(Some(GoFilter::Edits)));
            set(&keys.can_navigate_forward_in_edits, active_stack.can_go_forward(Some(GoFilter::Edits)));
            set(&keys.can_navigate_to_last_edit_location, active_stack.can_go_last(Some(GoFilter::Edits)));

            set(&keys.can_reopen_closed_editor, !self.recently_closed_editors.borrow().is_empty());
        });
    }

    //#endregion

    //#region Editor History Navigation (limit: 50)

    /// `onDidChangeEditorNavigationStack`.
    pub fn on_did_change_editor_navigation_stack(&self) -> Event<()> {
        self.on_did_change_editor_navigation_stack.event()
    }

    /// `handleEditorNavigationScopeChange()`.
    fn handle_editor_navigation_scope_change(&self) {
        // Ensure to start fresh when setting changes
        self.dispose_editor_navigation_stacks();

        // Update scope
        let configured_scope = self.configuration_service.get_value(Some(NAVIGATION_SCOPE_SETTING), None);
        let scope = match &configured_scope {
            Dynamic::String(scope) if scope.to_string_lossy() == "editorGroup" => GoScope::EditorGroup,
            Dynamic::String(scope) if scope.to_string_lossy() == "editor" => GoScope::Editor,
            _ => GoScope::Default,
        };
        self.editor_navigation_scope.set(scope);
    }

    fn register_editor_navigation_scope_change_listener(&self) {
        self.listen(self.configuration_service.on_did_change_configuration(), |this, event| {
            if event.affects_configuration(NAVIGATION_SCOPE_SETTING, None) {
                this.handle_editor_navigation_scope_change();
            }
        });

        self.handle_editor_navigation_scope_change();
    }

    /// `getStack(group = this.editorGroupService.activeGroup, editor = group.activeEditor)`: os valores ausentes (`undefined`)
    /// valem o padrão.
    fn get_stack(&self, group: Option<&Rc<dyn IEditorGroup>>, editor: Option<EditorInputRef>) -> Rc<dyn IEditorNavigationStacks> {
        let group = group.cloned().unwrap_or_else(|| self.editor_group_service.active_group());
        let editor = editor.or_else(|| group.active_editor());

        let weak = self.this.clone();
        let fire_on_change = move || {
            let weak = weak.clone();
            move |_: &()| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_editor_navigation_stack.fire(());
                }
            }
        };

        match self.editor_navigation_scope.get() {
            // Per Editor
            GoScope::Editor => {
                let Some(editor) = editor else {
                    return Rc::new(NoOpEditorNavigationStacks);
                };

                let existing = self.editor_scoped_navigation_stacks.borrow().get(&group.id()).and_then(|stacks| stacks.get(&ById(editor.clone()))).map(|entry| entry.stack.clone());
                if let Some(stack) = existing {
                    return stack;
                }

                let disposable = DisposableStore::new();
                let stack = self.instantiation_service.create_instance::<EditorNavigationStacks>(GoScope::Editor);
                disposable.add_rc(stack.clone());
                disposable.add(stack.on_did_change().listen(fire_on_change()));

                let stack: Rc<dyn IEditorNavigationStacks> = stack;
                self.editor_scoped_navigation_stacks.borrow_mut().entry(group.id()).or_default().insert(ById(editor), ScopedStacks { stack: stack.clone(), disposable });

                stack
            }

            // Per Editor Group
            GoScope::EditorGroup => {
                let existing = self.editor_group_scoped_navigation_stacks.borrow().get(&group.id()).map(|entry| entry.stack.clone());
                if let Some(stack) = existing {
                    return stack;
                }

                let disposable = DisposableStore::new();
                let stack = self.instantiation_service.create_instance::<EditorNavigationStacks>(GoScope::EditorGroup);
                disposable.add_rc(stack.clone());
                disposable.add(stack.on_did_change().listen(fire_on_change()));

                let stack: Rc<dyn IEditorNavigationStacks> = stack;
                self.editor_group_scoped_navigation_stacks.borrow_mut().insert(group.id(), ScopedStacks { stack: stack.clone(), disposable });

                stack
            }

            // Global
            GoScope::Default => {
                let existing = self.default_scoped_editor_navigation_stack.borrow().clone();
                if let Some(stack) = existing {
                    return stack;
                }

                let stack = self.instantiation_service.create_instance::<EditorNavigationStacks>(GoScope::Default);
                self.base.register(stack.on_did_change().listen(fire_on_change()));

                let stack: Rc<dyn IEditorNavigationStacks> = stack;
                *self.default_scoped_editor_navigation_stack.borrow_mut() = Some(stack.clone());

                stack
            }
        }
    }

    fn handle_active_editor_change_in_navigation_stacks(&self, group: &Rc<dyn IEditorGroup>, editor_pane: Option<&Rc<dyn IEditorPane>>) {
        self.get_stack(Some(group), editor_pane.and_then(|pane| pane.input())).handle_active_editor_change(editor_pane);
    }

    fn handle_active_editor_selection_change_in_navigation_stacks(&self, group: &Rc<dyn IEditorGroup>, editor_pane: &Rc<dyn IEditorPane>, event: EditorPaneSelectionChangeEvent) {
        self.get_stack(Some(group), editor_pane.input()).handle_active_editor_selection_change(editor_pane, event);
    }

    fn handle_editor_close_event_in_history(&self, e: &EditorCloseEvent) {
        let mut editor_scoped = self.editor_scoped_navigation_stacks.borrow_mut();
        let Some(editors) = editor_scoped.get_mut(&e.group_id) else {
            return;
        };

        if let Some(editor_stack) = editors.remove(&ById(e.editor.clone())) {
            editor_stack.disposable.dispose();
        }

        if editors.is_empty() {
            editor_scoped.remove(&e.group_id);
        }
    }

    fn handle_editor_group_remove_in_navigation_stacks(&self, group: &Rc<dyn IEditorGroup>) {
        // Global
        let default_stack = self.default_scoped_editor_navigation_stack.borrow().clone();
        if let Some(stack) = default_stack {
            stack.remove(&NavigationRemoval::Group(group.id()));
        }

        // Editor groups
        let editor_group_stack = self.editor_group_scoped_navigation_stacks.borrow_mut().remove(&group.id());
        if let Some(editor_group_stack) = editor_group_stack {
            editor_group_stack.disposable.dispose();
        }
    }

    fn clear_editor_navigation_stacks(&self) {
        self.with_each_editor_navigation_stack(|stack| stack.clear());
    }

    fn remove_from_editor_navigation_stacks(&self, arg1: &EditorMatchTarget) {
        let removal = NavigationRemoval::Target(arg1.clone());
        self.with_each_editor_navigation_stack(|stack| stack.remove(&removal));
    }

    fn move_in_editor_navigation_stacks(&self, event: &FileOperationEvent) {
        self.with_each_editor_navigation_stack(|stack| stack.move_(event));
    }

    fn with_each_editor_navigation_stack(&self, f: impl Fn(&Rc<dyn IEditorNavigationStacks>)) {
        let mut stacks: Vec<Rc<dyn IEditorNavigationStacks>> = Vec::new();

        // Global
        if let Some(stack) = &*self.default_scoped_editor_navigation_stack.borrow() {
            stacks.push(stack.clone());
        }

        // Per editor group
        stacks.extend(self.editor_group_scoped_navigation_stacks.borrow().values().map(|entry| entry.stack.clone()));

        // Per editor
        stacks.extend(self.editor_scoped_navigation_stacks.borrow().values().flat_map(|entries| entries.values().map(|entry| entry.stack.clone())));

        for stack in &stacks {
            f(stack);
        }
    }

    fn dispose_editor_navigation_stacks(&self) {
        // Global
        let default_stack = self.default_scoped_editor_navigation_stack.borrow_mut().take();
        if let Some(stack) = default_stack {
            stack.dispose();
        }

        // Per Editor group
        let group_stacks: Vec<ScopedStacks> = self.editor_group_scoped_navigation_stacks.borrow_mut().drain().map(|(_, stack)| stack).collect();
        for stack in group_stacks {
            stack.disposable.dispose();
        }

        // Per Editor
        let editor_stacks: Vec<ScopedStacks> = self.editor_scoped_navigation_stacks.borrow_mut().drain().flat_map(|(_, stacks)| stacks.into_values()).collect();
        for stack in editor_stacks {
            stack.disposable.dispose();
        }
    }

    //#endregion

    //#region Navigation: Next/Previous Used Editor

    fn do_navigate_in_recently_used_editors_stack(&self, editor_identifier: Option<EditorIdentifier>, group_id: Option<GroupIdentifier>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            if let Some(editor_identifier) = editor_identifier {
                let across_groups = !group_id.is_some_and(|group_id| this.editor_group_service.get_group(group_id).is_some());

                if across_groups {
                    this.navigating_in_recently_used_editors_stack.set(true);
                } else {
                    this.navigating_in_recently_used_editors_in_group_stack.set(true);
                }

                let group = this.editor_group_service.get_group(editor_identifier.group_id).unwrap_or_else(|| this.editor_group_service.active_group());
                let result = group.open_editor(&editor_identifier.editor, None).await;
                if across_groups {
                    this.navigating_in_recently_used_editors_stack.set(false);
                } else {
                    this.navigating_in_recently_used_editors_in_group_stack.set(false);
                }
                result?;
            }

            Ok(())
        })
    }

    fn ensure_recently_used_stack(&self, index_modifier: impl Fn(i64) -> i64, group_id: Option<GroupIdentifier>) -> (Vec<EditorIdentifier>, i64) {
        let group = group_id.and_then(|group_id| self.editor_group_service.get_group(group_id));

        let (editors, index) = match &group {
            // Across groups
            None => (
                self.recently_used_editors_stack.borrow().clone().unwrap_or_else(|| self.editor_service.get_editors(EditorsOrder::MostRecentlyActive)),
                self.recently_used_editors_stack_index.get(),
            ),

            // Within group
            Some(group) => (
                self.recently_used_editors_in_group_stack
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| group.get_editors(EditorsOrder::MostRecentlyActive, None).into_iter().map(|editor| EditorIdentifier { group_id: group.id(), editor }).collect()),
                self.recently_used_editors_in_group_stack_index.get(),
            ),
        };

        // Adjust index
        let mut new_index = index_modifier(index);
        if new_index < 0 {
            new_index = 0;
        } else if new_index > editors.len() as i64 - 1 {
            new_index = editors.len() as i64 - 1;
        }

        // Remember index and editors
        if group.is_none() {
            *self.recently_used_editors_stack.borrow_mut() = Some(editors.clone());
            self.recently_used_editors_stack_index.set(new_index);
        } else {
            *self.recently_used_editors_in_group_stack.borrow_mut() = Some(editors.clone());
            self.recently_used_editors_in_group_stack_index.set(new_index);
        }

        (editors, new_index)
    }

    fn handle_editor_event_in_recent_editors_stack(&self) {
        // Drop all-editors stack unless navigating in all editors
        if !self.navigating_in_recently_used_editors_stack.get() {
            *self.recently_used_editors_stack.borrow_mut() = None;
            self.recently_used_editors_stack_index.set(0);
        }

        // Drop in-group-editors stack unless navigating in group
        if !self.navigating_in_recently_used_editors_in_group_stack.get() {
            *self.recently_used_editors_in_group_stack.borrow_mut() = None;
            self.recently_used_editors_in_group_stack_index.set(0);
        }
    }

    //#endregion

    //#region File: Reopen Closed Editor (limit: 20)

    fn handle_editor_close_event_in_reopen(&self, event: &EditorCloseEvent) {
        if self.ignore_editor_close_event.get() {
            return; // blocked
        }

        let editor = &event.editor;
        if event.context == EditorCloseContext::Replace || event.context == EditorCloseContext::Move {
            return; // ignore if editor was replaced or moved
        }

        if !editor.can_reopen() {
            return; // only editors that can be reopened
        }

        let Some(untyped_editor) = editor.to_untyped(None) else {
            return; // we need a untyped editor to restore from going forward
        };

        let typed = EditorInputOrUntyped::Typed(editor.clone());
        let mut associated_resources: Vec<URI> = Vec::new();
        match EditorResourceAccessor::get_original_uri_sides(Some(&typed), Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Both), filter_by_scheme: None })) {
            Some(EditorUri::Resource(resource)) => associated_resources.push(resource),
            Some(EditorUri::Sides { primary, secondary }) => associated_resources.extend([primary, secondary].into_iter().flatten()),
            None => {}
        }

        // Remove from list of recently closed before...
        self.remove_from_recently_closed_editors(&EditorMatchTarget::from(editor));

        // ...adding it as last recently closed
        let batch_id = self.current_recently_closed_editors_batch_id();
        {
            let mut recently_closed_editors = self.recently_closed_editors.borrow_mut();
            recently_closed_editors.push(RecentlyClosedEditor {
                editor_id: editor.editor_id(),
                editor: untyped_editor,
                resource: EditorResourceAccessor::get_original_uri(Some(&typed), None),
                associated_resources,
                index: event.index,
                sticky: event.sticky,
                batch_id,
            });

            // Bounding
            if recently_closed_editors.len() > MAX_RECENTLY_CLOSED_EDITORS {
                recently_closed_editors.remove(0);
            }
        }

        // Context
        self.context_keys.can_reopen_closed_editor.set(Dynamic::Bool(true));
    }

    fn current_recently_closed_editors_batch_id(&self) -> u64 {
        // All editors that are closed within the same synchronous turn (e.g. "Close All Editors" or "Close Others") share the
        // same batch identifier so that they are reopened together. We open a new batch on the first close event and reset it
        // on the next microtask, after all synchronously fired close events have been handled.
        if !self.recently_closed_editors_batch_scheduled.get() {
            self.recently_closed_editors_batch_scheduled.set(true);
            self.recently_closed_editors_batch_id.set(self.recently_closed_editors_batch_id.get() + 1);

            let weak = self.this.clone();
            runtime::queue_microtask(move || {
                if let Some(this) = weak.upgrade() {
                    this.recently_closed_editors_batch_scheduled.set(false);
                }
            });
        }

        self.recently_closed_editors_batch_id.get()
    }

    fn take_last_closed_editors_batch(&self) -> Vec<RecentlyClosedEditor> {
        let mut recently_closed_editors = self.recently_closed_editors.borrow_mut();
        let Some(last_closed_editor) = recently_closed_editors.last() else {
            return Vec::new();
        };
        let batch_id = last_closed_editor.batch_id;

        // Collect all trailing editors that belong to the same batch. They are contiguous at the end of the list because
        // editors are appended in the order they are closed.
        let mut batch: Vec<RecentlyClosedEditor> = Vec::new();
        while recently_closed_editors.last().is_some_and(|editor| editor.batch_id == batch_id) {
            batch.insert(0, recently_closed_editors.pop().expect("o último editor existe"));
        }

        batch
    }

    fn do_reopen_last_closed_editors(&self, last_closed_editors: Vec<RecentlyClosedEditor>) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Reopen all editors of the batch in the order they were originally closed
            let mut any_reopened = false;
            for last_closed_editor in last_closed_editors {
                let editor_pane = this.do_reopen_last_closed_editor(last_closed_editor).await?;
                if editor_pane.is_some() {
                    any_reopened = true;
                }
            }

            // Fix for https://github.com/microsoft/vscode/issues/67882
            // If none of the editors in the batch could be reopened, make sure to try the previous batch. The failing editors
            // have already been removed from the list of recently closed editors to prevent endless loops.
            if !any_reopened && !this.recently_closed_editors.borrow().is_empty() {
                return this.reopen_last_closed_editor().await;
            }

            Ok(())
        })
    }

    fn do_reopen_last_closed_editor(&self, last_closed_editor: RecentlyClosedEditor) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let this = self.rc();

        async_fn(async move {
            let mut options = EditorOptions {
                pinned: Some(true),
                sticky: Some(last_closed_editor.sticky),
                index: Some(last_closed_editor.index as f64),
                ignore_error: Some(true),
                ..EditorOptions::default()
            };

            // Special sticky handling: remove the index property from options if that would result in sticky state to not
            // preserve or apply wrongly.
            let active_group = this.editor_group_service.active_group();
            let index = EditorOrIndex::Index(last_closed_editor.index as i64);
            if (last_closed_editor.sticky && !active_group.is_sticky(&index)) || (!last_closed_editor.sticky && active_group.is_sticky(&index)) {
                options.index = None;
            }

            // Re-open editor unless already opened
            let mut editor_pane: Option<Rc<dyn IEditorPane>> = None;
            if !active_group.contains(&EditorInputOrUntyped::Untyped(last_closed_editor.editor.clone()), None) {
                // Fix for https://github.com/microsoft/vscode/issues/107850
                // If opening an editor fails, it is possible that we get another editor-close event as a result. But we really
                // do want to ignore that in our list of recently closed editors to prevent endless loops.
                this.ignore_editor_close_event.set(true);

                let mut merged = last_closed_editor.editor.options.clone().unwrap_or_default();
                merged.base.pinned = options.pinned;
                merged.base.sticky = options.sticky;
                merged.base.index = options.index;
                merged.base.ignore_error = options.ignore_error;
                let result = this.editor_service.open_editor_in(UntypedEditorInput { options: Some(merged), ..last_closed_editor.editor.clone() }, None).await;
                this.ignore_editor_close_event.set(false);
                editor_pane = result?;
            }

            Ok(editor_pane)
        })
    }

    fn remove_from_recently_closed_editors(&self, arg1: &EditorMatchTarget) {
        let retained: Vec<RecentlyClosedEditor> = std::mem::take(&mut *self.recently_closed_editors.borrow_mut())
            .into_iter()
            .filter(|recently_closed_editor| {
                if let EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(editor)) = arg1
                    && recently_closed_editor.editor_id != editor.editor_id()
                {
                    return true; // keep: different editor identifiers
                }

                if recently_closed_editor.resource.as_ref().is_some_and(|resource| self.editor_helper.matches_file(resource, arg1)) {
                    return false; // remove: editor matches directly
                }

                if recently_closed_editor.associated_resources.iter().any(|associated_resource| self.editor_helper.matches_file(associated_resource, arg1)) {
                    return false; // remove: an associated resource matches
                }

                true // keep
            })
            .collect();

        let is_empty = retained.is_empty();
        *self.recently_closed_editors.borrow_mut() = retained;

        // Update context
        self.context_keys.can_reopen_closed_editor.set(Dynamic::Bool(!is_empty));
    }

    //#endregion

    //#region Go to: Recently Opened Editor (limit: 200, persisted)

    /// `editorHistoryListeners.size` (o `DisposableMap` que o caso `editor dispose clears history listener` lê).
    pub fn editor_history_listeners_size_for_testing(&self) -> usize {
        self.editor_history_listeners.len()
    }

    fn handle_active_editor_change_in_history(&self, editor_pane: Option<&Rc<dyn IEditorPane>>) {
        // Ensure we have not configured to exclude input and don't track invalid inputs
        let Some(editor) = editor_pane.and_then(|pane| pane.input()) else {
            return;
        };
        let editor = EditorInputOrUntyped::Typed(editor);
        if is_disposed(&editor) || !self.include_in_history(&editor) {
            return;
        }

        // Remove any existing entry and add to the beginning
        self.remove_from_history_target(&EditorMatchTarget::Editor(editor.clone()));
        self.add_to_history(&editor, true);
    }

    fn add_to_history(&self, editor: &EditorInputOrUntyped, insert_first: bool) {
        self.ensure_history_loaded();

        let Some(history_input) = self.editor_helper.prefer_resource_editor_input(editor) else {
            return;
        };

        // Insert based on preference and respect max entries setting
        let popped = {
            let mut history = self.history.borrow_mut();
            let history = history.as_mut().expect("o histórico foi carregado");
            if insert_first {
                history.insert(0, history_input.clone());
            } else {
                history.push(history_input.clone());
            }

            if history.len() > MAX_HISTORY_ITEMS { history.pop() } else { None }
        };
        if let Some(popped) = popped {
            self.editor_helper.clear_on_editor_dispose(&EditorMatchTarget::Editor(popped), &self.editor_history_listeners);
        }

        // React to editor input disposing
        if let EditorInputOrUntyped::Typed(editor) = editor {
            let weak = self.this.clone();
            self.editor_helper.on_editor_dispose(
                editor,
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.update_history_on_editor_dispose(&history_input);
                    }
                },
                &self.editor_history_listeners,
            );
        }
    }

    fn update_history_on_editor_dispose(&self, editor: &EditorInputOrUntyped) {
        match editor {
            EditorInputOrUntyped::Typed(typed_editor) => match side_by_side_input_sides(typed_editor) {
                // Any non side-by-side editor input gets removed directly on dispose
                None => {
                    self.remove_from_history_target(&EditorMatchTarget::Editor(editor.clone()));
                }

                // Side-by-side editors get special treatment: we try to distill the possibly untyped resource inputs from both
                // sides to be able to offer these entries from the history to the user still unless they are excluded.
                Some((primary, secondary)) => {
                    let mut resource_inputs: Vec<EditorInputOrUntyped> = Vec::new();
                    let side_inputs = if primary.matches(&EditorInputOrUntyped::Typed(secondary.clone())) { vec![primary] } else { vec![primary, secondary] };
                    for side_input in side_inputs {
                        let candidate_resource_input = self.editor_helper.prefer_resource_editor_input(&EditorInputOrUntyped::Typed(side_input));
                        if let Some(candidate) = candidate_resource_input
                            && is_resource_editor_input(Some(&candidate))
                            && self.include_in_history(&candidate)
                        {
                            resource_inputs.push(candidate);
                        }
                    }

                    // Insert the untyped resource inputs where our disposed side-by-side editor input is in the history stack
                    self.replace_in_history(editor, resource_inputs);
                }
            },

            EditorInputOrUntyped::Untyped(_) => {
                // Remove any editor that should not be included in history
                if !self.include_in_history(editor) {
                    self.remove_from_history_target(&EditorMatchTarget::Editor(editor.clone()));
                }
            }
        }
    }

    fn include_in_history(&self, editor: &EditorInputOrUntyped) -> bool {
        match editor {
            EditorInputOrUntyped::Typed(_) => true, // include any non files
            EditorInputOrUntyped::Untyped(editor) => match &editor.resource {
                Some(resource) => !self.resource_exclude_matcher.value().matches(resource, None),
                None => true,
            },
        }
    }

    fn remove_excluded_from_history(&self) {
        self.ensure_history_loaded();

        let history = self.current_history();
        let filtered: Vec<EditorInputOrUntyped> = history
            .into_iter()
            .filter(|entry| {
                let include = self.include_in_history(entry);

                // Cleanup any listeners associated with the input when removing from history
                if !include {
                    self.editor_helper.clear_on_editor_dispose(&EditorMatchTarget::Editor(entry.clone()), &self.editor_history_listeners);
                }

                include
            })
            .collect();

        *self.history.borrow_mut() = Some(filtered);
    }

    fn move_in_history(&self, event: &FileOperationEvent) {
        if event.is_operation(FileOperation::Move) {
            let removed = self.remove_from_history_target(&EditorMatchTarget::FileOperation(event.clone()));
            if removed && let Some(target) = &event.target {
                self.add_to_history(&EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(target.resource.clone())), true);
            }
        }
    }

    /// `removeFromHistory(arg1)` com o argumento de qualquer forma.
    fn remove_from_history_target(&self, arg1: &EditorMatchTarget) -> bool {
        let mut removed = false;

        self.ensure_history_loaded();

        let history = self.current_history();
        let filtered: Vec<EditorInputOrUntyped> = history
            .into_iter()
            .filter(|entry| {
                let matches = self.editor_helper.matches_editor(arg1, entry);

                // Cleanup any listeners associated with the input when removing from history
                if matches {
                    self.editor_helper.clear_on_editor_dispose(arg1, &self.editor_history_listeners);
                    removed = true;
                }

                !matches
            })
            .collect();

        *self.history.borrow_mut() = Some(filtered);

        removed
    }

    fn replace_in_history(&self, editor: &EditorInputOrUntyped, replacements: Vec<EditorInputOrUntyped>) {
        self.ensure_history_loaded();

        let mut replaced = false;

        let mut new_history: Vec<EditorInputOrUntyped> = Vec::new();
        for entry in self.current_history() {
            // Entry matches and is going to be disposed + replaced
            if self.editor_helper.matches_editor(&EditorMatchTarget::Editor(editor.clone()), &entry) {
                // Cleanup any listeners associated with the input when replacing from history
                self.editor_helper.clear_on_editor_dispose(&EditorMatchTarget::Editor(editor.clone()), &self.editor_history_listeners);

                // Insert replacements but only once
                if !replaced {
                    new_history.extend(replacements.iter().cloned());
                    replaced = true;
                }
            }
            // Entry does not match, but only add it if it didn't match our replacements already
            else if !replacements.iter().any(|replacement| self.editor_helper.matches_editor(&EditorMatchTarget::Editor(replacement.clone()), &entry)) {
                new_history.push(entry);
            }
        }

        // If the target editor to replace was not found, make sure to insert the replacements to the end to ensure we got them
        if !replaced {
            new_history.extend(replacements);
        }

        *self.history.borrow_mut() = Some(new_history);
    }

    /// `this.history` (carregado por [`HistoryService::ensure_history_loaded`]).
    fn current_history(&self) -> Vec<EditorInputOrUntyped> {
        self.history.borrow().clone().unwrap_or_default()
    }

    fn ensure_history_loaded(&self) {
        if self.history.borrow().is_some() {
            return;
        }

        // Until history is loaded, it is just empty
        *self.history.borrow_mut() = Some(Vec::new());

        // We want to seed history from opened editors too as well as previous stored state, so we need to wait for the editor
        // groups being ready
        if self.editor_group_service.is_ready() {
            self.load_history();
        } else {
            let this = self.rc();
            drop(async_fn(async move {
                this.editor_group_service.when_ready().await?;
                this.load_history();

                Ok(())
            }));
        }
    }

    fn load_history(&self) {
        // Init as empty before adding - since we are about to populate the history from opened editors, we capture the right
        // order here.
        *self.history.borrow_mut() = Some(Vec::new());

        // All stored editors from previous session
        let stored_editor_history = self.load_history_from_storage();

        // All restored editors from previous session in reverse editor from least to most recently used.
        let mut opened_editors_lru = self.editor_service.get_editors(EditorsOrder::MostRecentlyActive);
        opened_editors_lru.reverse();

        // We want to merge the opened editors from the last session with the stored editors from the last session. Because not
        // all editors can be serialised we want to make sure to include all opened editors too. Opened editors should always
        // be first in the history

        let mut handled_editors: HashSet<String /* resource + editorId */> = HashSet::new();

        // Add all opened editors first
        for EditorIdentifier { editor, .. } in opened_editors_lru {
            let typed = EditorInputOrUntyped::Typed(editor.clone());
            if !self.include_in_history(&typed) {
                continue;
            }

            // Make sure to skip duplicates from the editors LRU
            if let Some(resource) = editor.resource() {
                let history_entry_id = format!("{}/{}", resource.to_string(false).to_string_lossy(), editor.editor_id().unwrap_or_else(|| "undefined".to_string()));
                if handled_editors.contains(&history_entry_id) {
                    continue; // already added
                }

                handled_editors.insert(history_entry_id);
            }

            // Add into history
            self.add_to_history(&typed, true);
        }

        // Add remaining from storage if not there already. We check on resource and `editorId` (from `override`) to figure out
        // if the editor has been already added.
        for editor in stored_editor_history {
            let override_ = match editor.options.as_ref().and_then(|options| options.base.override_.as_ref()) {
                Some(EditorOverride::Id(id)) => id.to_string_lossy(),
                Some(EditorOverride::Resolution(resolution)) => (*resolution as i32).to_string(),
                None => "undefined".to_string(),
            };
            let history_entry_id = format!("{}/{}", uri_string(editor.resource.as_ref()), override_);
            let entry = EditorInputOrUntyped::Untyped(editor);
            if !handled_editors.contains(&history_entry_id) && self.include_in_history(&entry) {
                handled_editors.insert(history_entry_id);
                self.add_to_history(&entry, false /* at the end */);
            }
        }
    }

    fn load_history_from_storage(&self) -> Vec<UntypedEditorInput> {
        let mut entries: Vec<UntypedEditorInput> = Vec::new();

        let Some(entries_raw) = self.storage_service.get(HISTORY_STORAGE_KEY, StorageScope::Workspace, None).filter(|entries_raw| !entries_raw.is_empty()) else {
            return entries;
        };

        let entries_parsed = match parse_json(&entries_raw) {
            Ok(entries_parsed) => entries_parsed,
            Err(error) => {
                errors::on_unexpected_error(&error); // https://github.com/microsoft/vscode/issues/99075
                return entries;
            }
        };

        // `for (const entryParsed of entriesParsed)` lança para um valor que não é iterável
        let JsonValue::Array(entries_parsed) = entries_parsed else {
            errors::on_unexpected_error(&Error::type_error("entriesParsed is not iterable"));
            return entries;
        };

        for entry_parsed in &entries_parsed {
            let JsonValue::Object(entry_parsed) = entry_parsed else {
                continue; // unexpected data format
            };
            let Some(JsonValue::Object(editor)) = entry_parsed.get("editor") else {
                continue; // unexpected data format
            };
            let Some(resource) = editor.get("resource").filter(|resource| match resource {
                JsonValue::Null | JsonValue::Bool(false) => false,
                JsonValue::String(text) => !text.to_string_lossy().is_empty(),
                _ => true,
            }) else {
                continue; // unexpected data format
            };

            match deserialize_resource(resource) {
                // from 1.67.x: URI is stored efficiently as URI.toString(); until 1.66.x: URI was stored very verbose as URI.toJSON()
                Some(Ok(resource)) => entries.push(UntypedEditorInput {
                    resource: Some(resource),
                    options: match editor.get("options") {
                        Some(JsonValue::Object(options)) => Some(deserialize_options(options)),
                        _ => None,
                    },
                    label: json_string(editor, "label"),
                    description: json_string(editor, "description"),
                    contents: json_string(editor, "contents"),
                    encoding: json_string(editor, "encoding"),
                    language_id: json_string(editor, "languageId"),
                    force_untitled: json_bool(editor, "forceUntitled"),
                    force_file: json_bool(editor, "forceFile"),
                    ..UntypedEditorInput::default()
                }),
                Some(Err(error)) => errors::on_unexpected_error(&error), // do not fail entire history when one entry fails
                None => {}
            }
        }

        entries
    }

    fn save_state(&self) {
        let Some(history) = self.history.borrow().clone() else {
            return; // nothing to save because history was not used
        };

        let mut entries: Vec<Dynamic> = Vec::new();
        for editor in &history {
            // only save resource editor inputs
            let EditorInputOrUntyped::Untyped(editor) = editor else {
                continue;
            };
            if !editor.is_resource_editor_input() {
                continue;
            }
            let Some(resource) = &editor.resource else {
                continue;
            };

            entries.push(serialize_history_entry(editor, resource));
        }

        self.storage_service.store(
            HISTORY_STORAGE_KEY,
            StorageValue::String(json_stringify(&Dynamic::array(entries)).unwrap_or_else(|| "[]".to_string())),
            StorageScope::Workspace,
            StorageTarget::Machine,
        );
    }

    //#endregion
}

/// `editor.isDisposed()` de uma entrada tipada (a sem tipo nunca é descartada).
fn is_disposed(editor: &EditorInputOrUntyped) -> bool {
    match editor {
        EditorInputOrUntyped::Typed(editor) => editor.is_disposed(),
        EditorInputOrUntyped::Untyped(_) => false,
    }
}

impl IHistoryService for HistoryService {
    fn go_forward(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(None, None).go_forward(filter)
    }

    fn go_back(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(None, None).go_back(filter)
    }

    fn go_previous(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(None, None).go_previous(filter)
    }

    fn go_last(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(None, None).go_last(filter)
    }

    fn reopen_last_closed_editor(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            // Reopen the last batch of editors that were closed together
            let last_closed_editors = this.take_last_closed_editors_batch();
            let reopen_closed_editor_promise = (!last_closed_editors.is_empty()).then(|| this.do_reopen_last_closed_editors(last_closed_editors));

            // Update context
            this.context_keys.can_reopen_closed_editor.set(Dynamic::Bool(!this.recently_closed_editors.borrow().is_empty()));

            match reopen_closed_editor_promise {
                Some(promise) => promise.await,
                None => Ok(()),
            }
        })
    }

    fn get_history(&self) -> Vec<EditorInputOrUntyped> {
        self.ensure_history_loaded();

        self.current_history()
    }

    fn remove_from_history(&self, input: &EditorInputOrUntyped) {
        self.remove_from_history_target(&EditorMatchTarget::Editor(input.clone()));
    }

    fn get_last_active_workspace_root(&self, scheme_filter: Option<&str>, authority_filter: Option<&str>) -> Option<URI> {
        let matches_filters = |resource: &URI| {
            scheme_filter.is_none_or(|scheme| scheme.is_empty() || resource.scheme().to_string_lossy() == scheme)
                && authority_filter.is_none_or(|authority| authority.is_empty() || resource.authority().to_string_lossy() == authority)
        };

        // No Folder: return early
        let folders = self.context_service.get_workspace().folders();
        if folders.is_empty() {
            return None;
        }

        // Single Folder: return early
        if folders.len() == 1 {
            let resource = &folders[0].uri;
            return matches_filters(resource).then(|| resource.clone());
        }

        // Multiple folders: find the last active one
        for input in self.get_history() {
            let EditorInputOrUntyped::Untyped(input) = input else {
                continue;
            };
            let Some(input_resource) = &input.resource else {
                continue;
            };

            if !matches_filters(input_resource) {
                continue;
            }

            if let Some(resource_workspace) = self.context_service.get_workspace_folder(input_resource) {
                return Some(resource_workspace.uri);
            }
        }

        // Fallback to first workspace matching scheme filter if any
        folders.iter().map(|folder| &folder.uri).find(|resource| matches_filters(*resource)).cloned()
    }

    fn get_last_active_file(&self, scheme_filter: &str, authority_filter: Option<&str>) -> Option<URI> {
        for input in self.get_history() {
            let resource = match &input {
                EditorInputOrUntyped::Typed(_) => {
                    EditorResourceAccessor::get_original_uri(Some(&input), Some(&EditorResourceAccessorOptions { support_side_by_side: None, filter_by_scheme: Some(SchemeFilter::Single(scheme_filter.to_string())) }))
                }
                EditorInputOrUntyped::Untyped(input) => input.resource.clone(),
            };

            if let Some(resource) = resource
                && resource.scheme().to_string_lossy() == scheme_filter
                && authority_filter.is_none_or(|authority| authority.is_empty() || resource.authority().to_string_lossy() == authority)
            {
                return Some(resource);
            }
        }

        None
    }

    fn open_next_recently_used_editor(&self, group: Option<GroupIdentifier>) -> Promise<()> {
        let (stack, index) = self.ensure_recently_used_stack(|index| index - 1, group);

        self.do_navigate_in_recently_used_editors_stack(usize::try_from(index).ok().and_then(|index| stack.get(index).cloned()), group)
    }

    fn open_previously_used_editor(&self, group: Option<GroupIdentifier>) -> Promise<()> {
        let (stack, index) = self.ensure_recently_used_stack(|index| index + 1, group);

        self.do_navigate_in_recently_used_editors_stack(usize::try_from(index).ok().and_then(|index| stack.get(index).cloned()), group)
    }

    fn clear(&self) {
        // History
        self.clear_recently_opened();

        // Navigation (next, previous)
        self.clear_editor_navigation_stacks();

        // Recently closed editors
        self.recently_closed_editors.borrow_mut().clear();

        // Context Keys
        self.update_context_keys();
    }

    fn clear_recently_opened(&self) {
        *self.history.borrow_mut() = Some(Vec::new());

        self.editor_history_listeners.clear_and_dispose_all();
    }
}

/// `EditorSelectionState`.
struct EditorSelectionState {
    editor_identifier: EditorIdentifier,
    selection: Option<Rc<dyn IEditorPaneSelection>>,
    reason: Option<EditorPaneSelectionChangeReason>,
}

impl EditorSelectionState {
    fn new(editor_identifier: EditorIdentifier, selection: Option<Rc<dyn IEditorPaneSelection>>, reason: Option<EditorPaneSelectionChangeReason>) -> Self {
        Self { editor_identifier, selection, reason }
    }

    fn justifies_new_navigation_entry(&self, other: &EditorSelectionState) -> bool {
        if self.editor_identifier.group_id != other.editor_identifier.group_id {
            return true; // different group
        }

        if !self.editor_identifier.editor.matches(&EditorInputOrUntyped::Typed(other.editor_identifier.editor.clone())) {
            return true; // different editor
        }

        let (Some(selection), Some(other_selection)) = (&self.selection, &other.selection) else {
            return true; // unknown selections
        };

        let result = selection.compare(other_selection.as_ref());

        if result == EditorPaneSelectionCompareResult::Similar
            && matches!(other.reason, Some(EditorPaneSelectionChangeReason::Navigation | EditorPaneSelectionChangeReason::Jump))
        {
            // let navigation sources win even if the selection is `SIMILAR` (e.g. "Go to definition" should add a history entry)
            return true;
        }

        result == EditorPaneSelectionCompareResult::Different
    }
}

/// `IEditorNavigationStacks`.
pub trait IEditorNavigationStacks: IDisposable {
    fn on_did_change(&self) -> Event<()>;

    fn can_go_forward(&self, filter: Option<GoFilter>) -> bool;
    fn go_forward(&self, filter: Option<GoFilter>) -> Promise<()>;
    fn can_go_back(&self, filter: Option<GoFilter>) -> bool;
    fn go_back(&self, filter: Option<GoFilter>) -> Promise<()>;
    fn go_previous(&self, filter: Option<GoFilter>) -> Promise<()>;
    fn can_go_last(&self, filter: Option<GoFilter>) -> bool;
    fn go_last(&self, filter: Option<GoFilter>) -> Promise<()>;

    fn handle_active_editor_change(&self, editor_pane: Option<&Rc<dyn IEditorPane>>);
    fn handle_active_editor_selection_change(&self, editor_pane: &Rc<dyn IEditorPane>, event: EditorPaneSelectionChangeEvent);

    fn clear(&self);
    fn remove(&self, arg1: &NavigationRemoval);
    /// `move(event)`.
    fn move_(&self, event: &FileOperationEvent);
}

/// `EditorNavigationStacks`.
pub struct EditorNavigationStacks {
    base: Rc<Disposable>,
    selections_stack: Rc<EditorNavigationStack>,
    edits_stack: Rc<EditorNavigationStack>,
    navigations_stack: Rc<EditorNavigationStack>,
    on_did_change: Event<()>,
}

impl Injectable for EditorNavigationStacks {
    type Args = GoScope;
    type Deps = (Rc<InstantiationService>,);

    fn create(scope: Self::Args, (instantiation_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let selections_stack = instantiation_service.create_instance::<EditorNavigationStack>((GoFilter::None, scope));
        base.register(selections_stack.clone());
        let edits_stack = instantiation_service.create_instance::<EditorNavigationStack>((GoFilter::Edits, scope));
        base.register(edits_stack.clone());
        let navigations_stack = instantiation_service.create_instance::<EditorNavigationStack>((GoFilter::Navigation, scope));
        base.register(navigations_stack.clone());

        let on_did_change = event::any(vec![selections_stack.on_did_change(), edits_stack.on_did_change(), navigations_stack.on_did_change()]);

        Rc::new(Self { base, selections_stack, edits_stack, navigations_stack, on_did_change })
    }
}

impl IDisposable for EditorNavigationStacks {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorNavigationStacks {
    fn stacks(&self) -> [&Rc<EditorNavigationStack>; 3] {
        [&self.selections_stack, &self.edits_stack, &self.navigations_stack]
    }

    fn get_stack(&self, filter: Option<GoFilter>) -> &Rc<EditorNavigationStack> {
        match filter.unwrap_or(GoFilter::None) {
            GoFilter::None => &self.selections_stack,
            GoFilter::Edits => &self.edits_stack,
            GoFilter::Navigation => &self.navigations_stack,
        }
    }
}

impl IEditorNavigationStacks for EditorNavigationStacks {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.clone()
    }

    fn can_go_forward(&self, filter: Option<GoFilter>) -> bool {
        self.get_stack(filter).can_go_forward()
    }

    fn go_forward(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(filter).go_forward()
    }

    fn can_go_back(&self, filter: Option<GoFilter>) -> bool {
        self.get_stack(filter).can_go_back()
    }

    fn go_back(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(filter).go_back()
    }

    fn go_previous(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(filter).go_previous()
    }

    fn can_go_last(&self, filter: Option<GoFilter>) -> bool {
        self.get_stack(filter).can_go_last()
    }

    fn go_last(&self, filter: Option<GoFilter>) -> Promise<()> {
        self.get_stack(filter).go_last()
    }

    fn handle_active_editor_change(&self, editor_pane: Option<&Rc<dyn IEditorPane>>) {
        // Always send to selections navigation stack
        self.selections_stack.notify_navigation(editor_pane, None);
    }

    fn handle_active_editor_selection_change(&self, editor_pane: &Rc<dyn IEditorPane>, event: EditorPaneSelectionChangeEvent) {
        let previous = self.selections_stack.current();

        // Always send to selections navigation stack
        self.selections_stack.notify_navigation(Some(editor_pane), Some(event));

        // Check for edits
        if event.reason == EditorPaneSelectionChangeReason::Edit {
            self.edits_stack.notify_navigation(Some(editor_pane), Some(event));
        }
        // Check for navigations
        //
        // Note: ignore if selections navigation stack is navigating because in that case we do not want to receive repeated
        // entries in the navigation stack.
        else if matches!(event.reason, EditorPaneSelectionChangeReason::Navigation | EditorPaneSelectionChangeReason::Jump) && !self.selections_stack.is_navigating() {
            // A "JUMP" navigation selection change always has a source and target. As such, we add the previous entry of the
            // selections navigation stack so that our navigation stack receives both entries unless the user is currently
            // navigating.
            if event.reason == EditorPaneSelectionChangeReason::Jump
                && !self.navigations_stack.is_navigating()
                && let Some(previous) = previous
            {
                self.navigations_stack.add_or_replace(previous.group_id, previous.editor, previous.selection, false);
            }

            self.navigations_stack.notify_navigation(Some(editor_pane), Some(event));
        }
    }

    fn clear(&self) {
        for stack in self.stacks() {
            stack.clear();
        }
    }

    fn remove(&self, arg1: &NavigationRemoval) {
        for stack in self.stacks() {
            stack.remove(arg1);
        }
    }

    fn move_(&self, event: &FileOperationEvent) {
        for stack in self.stacks() {
            stack.move_(event);
        }
    }
}

/// `NoOpEditorNavigationStacks`.
struct NoOpEditorNavigationStacks;

impl IDisposable for NoOpEditorNavigationStacks {
    fn dispose(&self) {}
}

impl IEditorNavigationStacks for NoOpEditorNavigationStacks {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn can_go_forward(&self, _filter: Option<GoFilter>) -> bool {
        false
    }

    fn go_forward(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn can_go_back(&self, _filter: Option<GoFilter>) -> bool {
        false
    }

    fn go_back(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn go_previous(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn can_go_last(&self, _filter: Option<GoFilter>) -> bool {
        false
    }

    fn go_last(&self, _filter: Option<GoFilter>) -> Promise<()> {
        Promise::resolved(())
    }

    fn handle_active_editor_change(&self, _editor_pane: Option<&Rc<dyn IEditorPane>>) {}

    fn handle_active_editor_selection_change(&self, _editor_pane: &Rc<dyn IEditorPane>, _event: EditorPaneSelectionChangeEvent) {}

    fn clear(&self) {}

    fn remove(&self, _arg1: &NavigationRemoval) {}

    fn move_(&self, _event: &FileOperationEvent) {}
}

/// `IEditorNavigationStackEntry`.
#[derive(Clone)]
pub struct EditorNavigationStackEntry {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputOrUntyped,
    pub selection: Option<Rc<dyn IEditorPaneSelection>>,
}

/// `EditorNavigationStack`.
pub struct EditorNavigationStack {
    this: Weak<EditorNavigationStack>,
    base: Rc<Disposable>,

    on_did_change: Rc<Emitter<()>>,

    map_editor_to_disposable: EditorDisposables,
    map_group_to_disposable: Rc<DisposableMap<GroupIdentifier, dyn IDisposable>>,

    editor_helper: Rc<EditorHelper>,

    stack: RefCell<Vec<EditorNavigationStackEntry>>,

    index: Cell<i64>,
    previous_index: Cell<i64>,

    navigating: Cell<bool>,

    current_selection_state: RefCell<Option<EditorSelectionState>>,

    filter: GoFilter,
    scope: GoScope,
    editor_service: Rc<dyn IEditorService>,
    editor_group_service: Rc<dyn IEditorGroupsService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for EditorNavigationStack {
    type Args = (GoFilter, GoScope);
    type Deps = (Rc<InstantiationService>, Rc<dyn IEditorService>, Rc<dyn IEditorGroupsService>, Rc<dyn LogService>);

    fn create((filter, scope): Self::Args, (instantiation_service, editor_service, editor_group_service, log_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let map_editor_to_disposable: EditorDisposables = DisposableMap::new();
        base.register(map_editor_to_disposable.clone());
        let map_group_to_disposable: Rc<DisposableMap<GroupIdentifier, dyn IDisposable>> = DisposableMap::new();
        base.register(map_group_to_disposable.clone());

        let editor_helper = instantiation_service.create_instance::<EditorHelper>(());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change,
            map_editor_to_disposable,
            map_group_to_disposable,
            editor_helper,
            stack: RefCell::new(Vec::new()),
            index: Cell::new(-1),
            previous_index: Cell::new(-1),
            navigating: Cell::new(false),
            current_selection_state: RefCell::new(None),
            filter,
            scope,
            editor_service,
            editor_group_service,
            log_service,
        });

        this.register_listeners();

        this
    }
}

impl IDisposable for EditorNavigationStack {
    fn dispose(&self) {
        self.clear();

        self.base.dispose();
    }
}

impl EditorNavigationStack {
    fn rc(&self) -> Rc<EditorNavigationStack> {
        self.this.upgrade().expect("a pilha de navegação está viva enquanto é usada")
    }

    /// `current`.
    pub fn current(&self) -> Option<EditorNavigationStackEntry> {
        let index = self.index.get();

        usize::try_from(index).ok().and_then(|index| self.stack.borrow().get(index).cloned())
    }

    /// `set current(entry)`.
    fn set_current(&self, entry: Option<EditorNavigationStackEntry>) {
        if let Some(entry) = entry
            && let Ok(index) = usize::try_from(self.index.get())
        {
            let mut stack = self.stack.borrow_mut();
            if index < stack.len() {
                stack[index] = entry;
            } else {
                // `this.stack[this.index] = entry` além do fim cresce o array com buracos; o índice nunca passa do fim.
                stack.push(entry);
            }
        }
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn register_listeners(&self) {
        let weak = self.this.clone();
        self.base.register(self.on_did_change.event().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.trace_stack();
            }
        }));

        let weak = self.this.clone();
        self.base.register(self.log_service.on_did_change_log_level().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.trace_stack();
            }
        }));

        let weak = self.this.clone();
        self.base.register(self.editor_group_service.on_did_remove_group().listen(move |group| {
            if let Some(this) = weak.upgrade() {
                this.map_group_to_disposable.delete_and_dispose(&group.id());
            }
        }));
    }

    fn is_tracing(&self) -> bool {
        self.log_service.get_level() == LogLevel::Trace
    }

    fn trace_stack(&self) {
        if !self.is_tracing() {
            return;
        }

        let stack = self.stack.borrow().clone();
        let entry_labels: Vec<String> = stack
            .iter()
            .map(|entry| {
                let selection = entry.selection.as_ref().and_then(|selection| selection.log()).unwrap_or_else(|| "<none>".to_string());

                format!("- group: {}, editor: {}, selection: {selection}", entry.group_id, uri_string(resource_of(&entry.editor).as_ref()))
            })
            .collect();

        if entry_labels.is_empty() {
            self.trace(&format!("index: {}, navigating: {}: <empty>", self.index.get(), self.is_navigating()));
        } else {
            self.trace(&format!("index: {}, navigating: {}\n{}\n\t\t\t", self.index.get(), self.is_navigating(), entry_labels.join("\n")));
        }
    }

    fn trace(&self, msg: &str) {
        if !self.is_tracing() {
            return;
        }

        let filter_label = match self.filter {
            GoFilter::None => "global",
            GoFilter::Edits => "edits",
            GoFilter::Navigation => "navigation",
        };
        let scope_label = match self.scope {
            GoScope::Default => "default",
            GoScope::EditorGroup => "editorGroup",
            GoScope::Editor => "editor",
        };

        self.log_service.trace(&format!("[History stack {filter_label}-{scope_label}]: {msg}"));
    }

    /// `trace(msg, editor, event)` com o editor (a forma `null` do original é [`EditorNavigationStack::trace`]).
    fn trace_with_editor(&self, msg: &str, editor: Option<&EditorInputOrUntyped>, event: Option<EditorPaneSelectionChangeEvent>) {
        if !self.is_tracing() {
            return;
        }

        self.trace(&format!("{msg} (editor: {}, event: {})", uri_string(editor.and_then(resource_of).as_ref()), Self::trace_event(event)));
    }

    fn trace_event(event: Option<EditorPaneSelectionChangeEvent>) -> &'static str {
        match event.map(|event| event.reason) {
            None => "<none>",
            Some(EditorPaneSelectionChangeReason::Edit) => "edit",
            Some(EditorPaneSelectionChangeReason::Navigation) => "navigation",
            Some(EditorPaneSelectionChangeReason::Jump) => "jump",
            Some(EditorPaneSelectionChangeReason::Programmatic) => "programmatic",
            Some(EditorPaneSelectionChangeReason::User) => "user",
        }
    }

    fn register_group_listeners(&self, group_id: GroupIdentifier) {
        if self.map_group_to_disposable.has(&group_id) {
            return;
        }

        if let Some(group) = self.editor_group_service.get_group(group_id) {
            let weak = self.this.clone();
            let listener = group.on_will_move_editor().listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_will_move_editor(e);
                }
            });
            self.map_group_to_disposable.set(group_id, listener, false);
        }
    }

    fn on_will_move_editor(&self, e: &crate::common::editor::EditorWillMoveEvent) {
        self.trace_with_editor("onWillMoveEditor()", Some(&EditorInputOrUntyped::Typed(e.editor.clone())), None);

        if self.scope == GoScope::EditorGroup {
            return; // ignore move events if our scope is group based
        }

        let target = EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(e.editor.clone()));
        let entries = self.stack.borrow().clone();
        let matching: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.group_id == e.group_id && self.editor_helper.matches_editor(&target, &entry.editor))
            .map(|(index, _)| index)
            .collect();

        // Update to target group
        let mut stack = self.stack.borrow_mut();
        for index in matching {
            if let Some(entry) = stack.get_mut(index) {
                entry.group_id = e.target;
            }
        }
    }

    //#region Stack Mutation

    /// `notifyNavigation(editorPane, event)`.
    pub fn notify_navigation(&self, editor_pane: Option<&Rc<dyn IEditorPane>>, event: Option<EditorPaneSelectionChangeEvent>) {
        let pane_input = editor_pane.and_then(|pane| pane.input());
        let pane_input_untyped = pane_input.clone().map(EditorInputOrUntyped::Typed);
        self.trace_with_editor("notifyNavigation()", pane_input_untyped.as_ref(), event);

        let is_selection_aware_editor_pane = is_editor_pane_with_selection(editor_pane);
        let valid_editor = pane_input.as_ref().filter(|input| !input.is_disposed());

        // Treat editor changes that happen as part of stack navigation specially we do not want to add a new stack entry as a
        // matter of navigating the stack but we need to keep our currentEditorSelectionState up to date with the navigtion that
        // occurs.
        if self.navigating.get() {
            self.trace_with_editor("notifyNavigation() ignoring (navigating)", pane_input_untyped.as_ref(), event);

            match (editor_pane, valid_editor) {
                (Some(editor_pane), Some(editor)) if is_selection_aware_editor_pane => {
                    self.trace_with_editor("notifyNavigation() updating current selection state", pane_input_untyped.as_ref(), event);

                    *self.current_selection_state.borrow_mut() = Some(EditorSelectionState::new(
                        EditorIdentifier { group_id: editor_pane.group_id(), editor: editor.clone() },
                        editor_pane.get_selection(),
                        event.map(|event| event.reason),
                    ));
                }
                _ => {
                    self.trace_with_editor("notifyNavigation() dropping current selection state", pane_input_untyped.as_ref(), event);

                    *self.current_selection_state.borrow_mut() = None; // we navigated to a non-selection aware or disposed editor
                }
            }
        }
        // Normal navigation not part of stack navigation
        else {
            self.trace_with_editor("notifyNavigation() not ignoring", pane_input_untyped.as_ref(), event);

            match (editor_pane, valid_editor) {
                // Navigation inside selection aware editor
                (Some(editor_pane), Some(editor)) if is_selection_aware_editor_pane => {
                    self.on_selection_aware_editor_navigation(editor_pane.group_id(), editor, editor_pane.get_selection(), event);
                }

                // Navigation to non-selection aware or disposed editor
                _ => {
                    *self.current_selection_state.borrow_mut() = None; // at this time we have no active selection aware editor

                    if let (Some(editor_pane), Some(editor)) = (editor_pane, valid_editor) {
                        self.on_non_selection_aware_editor_navigation(editor_pane.group_id(), editor);
                    }
                }
            }
        }
    }

    fn on_selection_aware_editor_navigation(&self, group_id: GroupIdentifier, editor: &EditorInputRef, selection: Option<Rc<dyn IEditorPaneSelection>>, event: Option<EditorPaneSelectionChangeEvent>) {
        let editor_candidate = EditorInputOrUntyped::Typed(editor.clone());

        if let Some(current) = self.current()
            && current.group_id == group_id
            && selection.is_none()
            && self.editor_helper.matches_editor(&EditorMatchTarget::Editor(current.editor), &editor_candidate)
        {
            return; // do not push same editor input again of same group if we have no valid selection
        }

        self.trace_with_editor("onSelectionAwareEditorNavigation()", Some(&editor_candidate), event);

        let state_candidate = EditorSelectionState::new(EditorIdentifier { group_id, editor: editor.clone() }, selection.clone(), event.map(|event| event.reason));

        // Add to stack if we dont have a current state or this new state justifies a push
        let justifies_new_entry = self.current_selection_state.borrow().as_ref().is_none_or(|current_selection_state| current_selection_state.justifies_new_navigation_entry(&state_candidate));
        if justifies_new_entry {
            self.do_add(group_id, editor_candidate, selection);
        }
        // Otherwise we replace the current stack entry with this one
        else {
            self.do_replace(group_id, editor_candidate, selection);
        }

        // Update our current navigation editor state
        *self.current_selection_state.borrow_mut() = Some(state_candidate);
    }

    fn on_non_selection_aware_editor_navigation(&self, group_id: GroupIdentifier, editor: &EditorInputRef) {
        let editor_candidate = EditorInputOrUntyped::Typed(editor.clone());

        if let Some(current) = self.current()
            && current.group_id == group_id
            && self.editor_helper.matches_editor(&EditorMatchTarget::Editor(current.editor), &editor_candidate)
        {
            return; // do not push same editor input again of same group
        }

        self.trace_with_editor("onNonSelectionAwareEditorNavigation()", Some(&editor_candidate), None);

        self.do_add(group_id, editor_candidate, None);
    }

    fn do_add(&self, group_id: GroupIdentifier, editor: EditorInputOrUntyped, selection: Option<Rc<dyn IEditorPaneSelection>>) {
        if !self.navigating.get() {
            self.add_or_replace(group_id, editor, selection, false);
        }
    }

    fn do_replace(&self, group_id: GroupIdentifier, editor: EditorInputOrUntyped, selection: Option<Rc<dyn IEditorPaneSelection>>) {
        if !self.navigating.get() {
            self.add_or_replace(group_id, editor, selection, true /* force replace */);
        }
    }

    /// `addOrReplace(groupId, editorCandidate, selection, forceReplace)`.
    pub fn add_or_replace(&self, group_id: GroupIdentifier, editor_candidate: EditorInputOrUntyped, selection: Option<Rc<dyn IEditorPaneSelection>>, force_replace: bool) {
        // Ensure we listen to changes in group
        self.register_group_listeners(group_id);

        // Check whether to replace an existing entry or not
        let mut replace = false;
        if let Some(current) = self.current() {
            if force_replace {
                replace = true; // replace if we are forced to
            } else if self.should_replace_stack_entry(&current, &EditorNavigationStackEntry { group_id, editor: editor_candidate.clone(), selection: selection.clone() }) {
                replace = true; // replace if the group & input is the same and selection indicates as such
            }
        }

        let Some(editor) = self.editor_helper.prefer_resource_editor_input(&editor_candidate) else {
            return;
        };

        self.trace_with_editor(if replace { "replace()" } else { "add()" }, Some(&editor), None);

        let new_stack_entry = EditorNavigationStackEntry { group_id, editor: editor.clone(), selection };

        // Replace at current position
        let mut removed_entries: Vec<EditorNavigationStackEntry> = Vec::new();
        if replace {
            if let Some(current) = self.current() {
                removed_entries.push(current);
            }
            self.set_current(Some(new_stack_entry));
        }
        // Add to stack at current position
        else {
            {
                let mut stack = self.stack.borrow_mut();
                let insert_at = usize::try_from(self.index.get() + 1).unwrap_or(0);

                // If we are not at the end of history, we remove anything after
                if stack.len() > insert_at {
                    removed_entries.extend(stack[insert_at..].iter().cloned());
                    stack.truncate(insert_at);
                }

                // Insert entry at index
                stack.insert(insert_at, new_stack_entry);

                // Check for limit
                if stack.len() > MAX_STACK_SIZE {
                    removed_entries.push(stack.remove(0)); // remove first
                    if self.previous_index.get() >= 0 {
                        self.previous_index.set(self.previous_index.get() - 1);
                    }
                } else {
                    drop(stack);
                    self.set_index(self.index.get() + 1, true /* skip event, we fire it later */);
                }
            }
        }

        // Clear editor listeners from removed entries
        for removed_entry in removed_entries {
            self.editor_helper.clear_on_editor_dispose(&EditorMatchTarget::Editor(removed_entry.editor), &self.map_editor_to_disposable);
        }

        // Remove this from the stack unless the stack input is a resource that can easily be restored even when the input gets
        // disposed
        if let EditorInputOrUntyped::Typed(editor) = &editor {
            let weak = self.this.clone();
            let weak_editor = Rc::downgrade(editor);
            self.editor_helper.on_editor_dispose(
                editor,
                move || {
                    if let (Some(this), Some(editor)) = (weak.upgrade(), weak_editor.upgrade()) {
                        this.remove(&NavigationRemoval::from(&editor));
                    }
                },
                &self.map_editor_to_disposable,
            );
        }

        // Event
        self.on_did_change.fire(());
    }

    fn should_replace_stack_entry(&self, entry: &EditorNavigationStackEntry, candidate: &EditorNavigationStackEntry) -> bool {
        if entry.group_id != candidate.group_id {
            return false; // different group
        }

        if !self.editor_helper.matches_editor(&EditorMatchTarget::Editor(entry.editor.clone()), &candidate.editor) {
            return false; // different editor
        }

        let Some(entry_selection) = &entry.selection else {
            return true; // always replace when we have no specific selection yet
        };

        let Some(candidate_selection) = &candidate.selection else {
            return false; // otherwise, prefer to keep existing specific selection over new unspecific one
        };

        // Finally, replace when selections are considered identical
        entry_selection.compare(candidate_selection.as_ref()) == EditorPaneSelectionCompareResult::Identical
    }

    /// `move(event)`.
    pub fn move_(&self, event: &FileOperationEvent) {
        if event.is_operation(FileOperation::Move) {
            let target = EditorMatchTarget::FileOperation(event.clone());
            let entries = self.stack.borrow().clone();
            let matching: Vec<usize> = entries.iter().enumerate().filter(|(_, entry)| self.editor_helper.matches_editor(&target, &entry.editor)).map(|(index, _)| index).collect();

            let mut stack = self.stack.borrow_mut();
            for index in matching {
                if let (Some(entry), Some(moved)) = (stack.get_mut(index), &event.target) {
                    entry.editor = EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(moved.resource.clone()));
                }
            }
        }
    }

    /// `remove(arg1)`.
    pub fn remove(&self, arg1: &NavigationRemoval) {
        let previous_stack_size = self.stack.borrow().len();

        // Remove all stack entries that match `arg1`
        let entries = self.stack.borrow().clone();
        let retained: Vec<EditorNavigationStackEntry> = entries
            .into_iter()
            .filter(|entry| {
                let matches = match arg1 {
                    NavigationRemoval::Group(group_id) => entry.group_id == *group_id,
                    NavigationRemoval::Target(target) => self.editor_helper.matches_editor(target, &entry.editor),
                };

                // Cleanup any listeners associated with the input when removing
                if matches {
                    self.editor_helper.clear_on_editor_dispose(&EditorMatchTarget::Editor(entry.editor.clone()), &self.map_editor_to_disposable);
                }

                !matches
            })
            .collect();
        *self.stack.borrow_mut() = retained;

        if previous_stack_size == self.stack.borrow().len() {
            return; // nothing removed
        }

        // Given we just removed entries, we need to make sure to remove entries that are now identical and next to each other
        // to prevent no-op navigations.
        self.flatten();

        // Reset indeces
        self.index.set(self.stack.borrow().len() as i64 - 1);
        self.previous_index.set(-1);

        // Clear group listener
        if let NavigationRemoval::Group(group_id) = arg1 {
            self.map_group_to_disposable.delete_and_dispose(group_id);
        }

        // Event
        self.on_did_change.fire(());
    }

    fn flatten(&self) {
        let mut flattened_stack: Vec<EditorNavigationStackEntry> = Vec::new();

        let mut previous_entry: Option<EditorNavigationStackEntry> = None;
        let entries = self.stack.borrow().clone();
        for entry in entries {
            if let Some(previous_entry) = &previous_entry
                && self.should_replace_stack_entry(&entry, previous_entry)
            {
                continue; // skip over entry when it is considered the same
            }

            previous_entry = Some(entry.clone());
            flattened_stack.push(entry);
        }

        *self.stack.borrow_mut() = flattened_stack;
    }

    /// `clear()`.
    pub fn clear(&self) {
        self.index.set(-1);
        self.previous_index.set(-1);
        self.stack.borrow_mut().clear();

        self.map_editor_to_disposable.clear_and_dispose_all();
        self.map_group_to_disposable.clear_and_dispose_all();
    }

    //#endregion

    //#region Navigation

    /// `canGoForward()`.
    pub fn can_go_forward(&self) -> bool {
        self.stack.borrow().len() as i64 > self.index.get() + 1
    }

    /// `goForward()`.
    pub fn go_forward(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let navigated = this.maybe_go_current().await?;
            if navigated {
                return Ok(());
            }

            if !this.can_go_forward() {
                return Ok(());
            }

            this.set_index(this.index.get() + 1, false);
            this.navigate().await
        })
    }

    /// `canGoBack()`.
    pub fn can_go_back(&self) -> bool {
        self.index.get() > 0
    }

    /// `goBack()`.
    pub fn go_back(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let navigated = this.maybe_go_current().await?;
            if navigated {
                return Ok(());
            }

            if !this.can_go_back() {
                return Ok(());
            }

            this.set_index(this.index.get() - 1, false);
            this.navigate().await
        })
    }

    /// `goPrevious()`.
    pub fn go_previous(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            let navigated = this.maybe_go_current().await?;
            if navigated {
                return Ok(());
            }

            // If we never navigated, just go back
            if this.previous_index.get() == -1 {
                return this.go_back().await;
            }

            // Otherwise jump to previous stack entry
            this.set_index(this.previous_index.get(), false);
            this.navigate().await
        })
    }

    /// `canGoLast()`.
    pub fn can_go_last(&self) -> bool {
        !self.stack.borrow().is_empty()
    }

    /// `goLast()`.
    pub fn go_last(&self) -> Promise<()> {
        let this = self.rc();

        async_fn(async move {
            if !this.can_go_last() {
                return Ok(());
            }

            this.set_index(this.stack.borrow().len() as i64 - 1, false);
            this.navigate().await
        })
    }

    fn maybe_go_current(&self) -> Promise<bool> {
        let this = self.rc();

        async_fn(async move {
            // When this navigation stack works with a specific filter where not every selection change is added to the stack,
            // we want to first reveal the current selection before attempting to navigate in the stack.

            if this.filter == GoFilter::None {
                return Ok(false); // only applies when  we are a filterd stack
            }

            if this.is_current_selection_active() {
                return Ok(false); // we are at the current navigation stop
            }

            // Go to current selection
            this.navigate().await?;

            Ok(true)
        })
    }

    fn is_current_selection_active(&self) -> bool {
        let Some(current) = self.current() else {
            return false; // we need a current selection
        };
        let Some(current_selection) = &current.selection else {
            return false; // we need a current selection
        };

        let Some(pane) = self.editor_service.active_editor_pane() else {
            return false; // we need an active editor pane with selection support
        };
        if !is_editor_pane_with_selection(Some(&pane)) {
            return false; // we need an active editor pane with selection support
        }

        if pane.group_id() != current.group_id {
            return false; // we need matching groups
        }

        let Some(pane_input) = pane.input() else {
            return false; // we need matching editors
        };
        if !self.editor_helper.matches_editor(&EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(pane_input)), &current.editor) {
            return false; // we need matching editors
        }

        let Some(pane_selection) = pane.get_selection() else {
            return false; // we need a selection to compare with
        };

        pane_selection.compare(current_selection.as_ref()) == EditorPaneSelectionCompareResult::Identical
    }

    fn set_index(&self, new_index: i64, skip_event: bool) {
        self.previous_index.set(self.index.get());
        self.index.set(new_index);

        // Event
        if !skip_event {
            self.on_did_change.fire(());
        }
    }

    fn navigate(&self) -> Promise<()> {
        self.navigating.set(true);

        let this = self.rc();
        async_fn(async move {
            let result = match this.current() {
                Some(current) => this.do_navigate(&current).await.map(|_| ()),
                None => Ok(()),
            };
            this.navigating.set(false);

            result
        })
    }

    fn do_navigate(&self, location: &EditorNavigationStackEntry) -> Promise<Option<Rc<dyn IEditorPane>>> {
        let mut options = TextEditorOptions::default();

        // Apply selection if any
        if let Some(selection) = &location.selection {
            options = selection.restore(&options);
        }

        match &location.editor {
            EditorInputOrUntyped::Typed(editor) => self.editor_service.open_editor_typed(editor, Some(options), Some(PreferredGroup::Id(location.group_id))),
            EditorInputOrUntyped::Untyped(editor) => {
                let mut merged = editor.options.clone().unwrap_or_default();
                if options.selection.is_some() {
                    merged.selection = options.selection;
                }
                if options.selection_reveal_type.is_some() {
                    merged.selection_reveal_type = options.selection_reveal_type;
                }
                if options.selection_source.is_some() {
                    merged.selection_source = options.selection_source.clone();
                }

                self.editor_service.open_editor_in(UntypedEditorInput { options: Some(merged), ..editor.clone() }, Some(PreferredGroup::Id(location.group_id)))
            }
        }
    }

    /// `isNavigating()`.
    pub fn is_navigating(&self) -> bool {
        self.navigating.get()
    }

    //#endregion
}

/// `EditorHelper`.
pub struct EditorHelper {
    uri_identity_service: Rc<dyn IUriIdentityService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    file_service: Rc<dyn IFileService>,
    path_service: Rc<dyn IPathService>,
}

impl Injectable for EditorHelper {
    type Args = ();
    type Deps = (Rc<dyn IUriIdentityService>, Rc<dyn ILifecycleService>, Rc<dyn IFileService>, Rc<dyn IPathService>);

    fn create(_args: Self::Args, (uri_identity_service, lifecycle_service, file_service, path_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { uri_identity_service, lifecycle_service, file_service, path_service })
    }
}

impl EditorHelper {
    /// `preferResourceEditorInput(editor)`: a entrada sem tipo (se o esquema é um dos que o produto controla), senão a própria
    /// entrada tipada; `None` para uma entrada sem tipo de esquema desconhecido.
    pub fn prefer_resource_editor_input(&self, editor: &EditorInputOrUntyped) -> Option<EditorInputOrUntyped> {
        let resource = EditorResourceAccessor::get_original_uri(Some(editor), None);

        // For now, only prefer well known schemes that we control to prevent issues such as
        // https://github.com/microsoft/vscode/issues/85204 from being used as resource inputs resource inputs survive editor
        // disposal and as such are a lot more durable across editor changes and restarts
        let has_valid_resource_editor_input_scheme = resource.as_ref().is_some_and(|resource| {
            let scheme = resource.scheme().to_string_lossy();

            scheme == schemas::FILE || scheme == schemas::VSCODE_REMOTE || scheme == schemas::VSCODE_USER_DATA || scheme == self.path_service.default_uri_scheme()
        });

        // Scheme is valid: prefer the untyped input over the typed input if possible to keep the entry across restarts
        if has_valid_resource_editor_input_scheme {
            if let EditorInputOrUntyped::Typed(input) = editor
                && let Some(untyped_input) = input.to_untyped(None)
            {
                let untyped_input = EditorInputOrUntyped::Untyped(untyped_input);
                if is_resource_editor_input(Some(&untyped_input)) {
                    return Some(untyped_input);
                }
            }

            return Some(editor.clone());
        }

        // Scheme is invalid: allow the editor input for as long as it is not disposed
        is_typed(editor).then(|| editor.clone())
    }

    /// `matchesEditor(arg1, inputB)`.
    pub fn matches_editor(&self, arg1: &EditorMatchTarget, input_b: &EditorInputOrUntyped) -> bool {
        match arg1 {
            EditorMatchTarget::FileChanges(_) | EditorMatchTarget::FileOperation(_) => {
                let EditorInputOrUntyped::Untyped(input_b) = input_b else {
                    return false; // we only support this for `IResourceEditorInputs` that are file based
                };
                let Some(resource) = &input_b.resource else {
                    return false;
                };

                match arg1 {
                    EditorMatchTarget::FileChanges(event) => event.contains(resource, &[FileChangeType::Deleted]),
                    _ => self.matches_file(resource, arg1),
                }
            }

            EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(arg1)) => match input_b {
                EditorInputOrUntyped::Typed(_) => arg1.matches(input_b),
                EditorInputOrUntyped::Untyped(input_b) => input_b.resource.as_ref().is_some_and(|resource| self.matches_file(resource, &EditorMatchTarget::from(arg1))),
            },

            EditorMatchTarget::Editor(EditorInputOrUntyped::Untyped(arg1)) => match input_b {
                EditorInputOrUntyped::Typed(input_b) => arg1.resource.as_ref().is_some_and(|resource| self.matches_file(resource, &EditorMatchTarget::from(input_b))),
                EditorInputOrUntyped::Untyped(input_b) => self.uri_identity_service.ext_uri().is_equal(arg1.resource.as_ref(), input_b.resource.as_ref(), false),
            },
        }
    }

    /// `matchesFile(resource, arg2)`.
    pub fn matches_file(&self, resource: &URI, arg2: &EditorMatchTarget) -> bool {
        match arg2 {
            EditorMatchTarget::FileChanges(event) => event.contains(resource, &[FileChangeType::Deleted]),

            EditorMatchTarget::FileOperation(event) => self.uri_identity_service.ext_uri().is_equal_or_parent(resource, &event.resource, false, false),

            EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(editor)) => {
                let Some(input_resource) = editor.resource() else {
                    return false;
                };

                if self.lifecycle_service.phase() >= LifecyclePhase::Restored && !self.file_service.has_provider(&input_resource) {
                    return false; // make sure to only check this when workbench has restored (for https://github.com/microsoft/vscode/issues/48275)
                }

                self.uri_identity_service.ext_uri().is_equal(Some(&input_resource), Some(resource), false)
            }

            EditorMatchTarget::Editor(EditorInputOrUntyped::Untyped(editor)) => self.uri_identity_service.ext_uri().is_equal(editor.resource.as_ref(), Some(resource), false),
        }
    }

    /// `matchesEditorIdentifier(identifier, editorPane)`.
    pub fn matches_editor_identifier(&self, identifier: &EditorIdentifier, editor_pane: Option<&Rc<dyn IEditorPane>>) -> bool {
        let Some(editor_pane) = editor_pane else {
            return false;
        };

        if identifier.group_id != editor_pane.group_id() {
            return false;
        }

        editor_pane.input().is_some_and(|input| identifier.editor.matches(&EditorInputOrUntyped::Typed(input)))
    }

    /// `onEditorDispose(editor, listener, mapEditorToDispose)`.
    pub fn on_editor_dispose(&self, editor: &EditorInputRef, listener: impl Fn() + 'static, map_editor_to_dispose: &EditorDisposables) {
        let (weak_editor, weak_map) = (Rc::downgrade(editor), Rc::downgrade(map_editor_to_dispose));
        let to_dispose = editor.on_will_dispose().once().listen(move |_| {
            if let (Some(editor), Some(map)) = (weak_editor.upgrade(), weak_map.upgrade()) {
                map.delete_and_dispose(&ById(editor));
            }
            listener();
        });

        let key = ById(editor.clone());
        let disposables = match map_editor_to_dispose.get(&key) {
            Some(disposables) => disposables,
            None => {
                let disposables = DisposableStore::new();
                map_editor_to_dispose.set(key, disposables.clone(), false);
                disposables
            }
        };

        disposables.add(to_dispose);
    }

    /// `clearOnEditorDispose(editor, mapEditorToDispose)`.
    pub fn clear_on_editor_dispose(&self, editor: &EditorMatchTarget, map_editor_to_dispose: &EditorDisposables) {
        let EditorMatchTarget::Editor(EditorInputOrUntyped::Typed(editor)) = editor else {
            return; // only supported when passing in an actual editor input
        };

        map_editor_to_dispose.delete_and_dispose(&ById(editor.clone()));
    }
}
