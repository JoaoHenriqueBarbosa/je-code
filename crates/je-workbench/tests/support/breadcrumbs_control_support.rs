//! Infraestrutura de `src/vs/workbench/test/browser/parts/editor/breadcrumbsControl.test.ts`: o `setup` da suíte
//! `BreadcrumbsControl`, o `createControl` e os auxiliares (`waitForPicker`, `focusEditor`, `deliverEditorFocus` e
//! `assertPickerFocused`), sobre os dublês que o original monta com `mock<T>()` e com `workbenchInstantiationService`.
//!
//! Divergências do original:
//! - **`workbenchInstantiationService`.** Os serviços são os de teste do porte (`TestContextService`, `TestFileService`, o
//!   `LabelService` real, `TestConfigurationService`, `TestQuickInputService`...), montados direto no [`BreadcrumbsControlServices`]
//!   (o porte injeta por struct, e não por `createInstance`). O `IContextViewService` é o [`NativeContextViewService`] real sobre a
//!   raiz do `Ui` (a janela de 1024 x 768 que o `window.innerWidth` e o `innerHeight` do original medem).
//! - **`mock<T>()`.** Cada dublê implementa só o que o caso usa, e todo outro membro lança `X is not a function`, como chamar um
//!   membro ausente no JavaScript. `quickAccess` e `openEditor` gravam o que recebem (o `quickAccessPrefixes` e o `spy`).
//! - **`spy(BreadcrumbsPicker.prototype, 'show')`.** O `lastCall.returnValue` é [`BreadcrumbsControl::last_picker_show`].
//! - **DOM, `querySelector` e eventos.** O contêiner e o `button` do editor são nós do `Ui`; `querySelector('.classe')` é a busca em
//!   pré-ordem da subárvore ([`Harness::query_selector`]); `click()`, `MouseEvent('click', { altKey })` e `MouseEvent('dblclick')` são o
//!   [`Ui::dispatch`] de um `Click` ou `DoubleClick` com o [`PointerData`] (o `altKey` é o modificador `alt`); `element.focus()` é
//!   [`Ui::focus`], que sempre entrega `focus` e `blur` (a janela nativa tem foco, então os `FocusEvent` manuais que o original
//!   dispara quando `document.hasFocus()` é falso não existem).
//! - **`aria-expanded`.** O `je-ui` não escreve atributos ARIA: o estado da linha é o do twistie dela (a classe `collapsible` sem
//!   `collapsed` é `'true'`, com `collapsed` é `'false'`, e sem `collapsible` não há o atributo), que é o que o renderer da árvore
//!   escreve junto com o atributo no original ([`Harness::aria_expanded`]).
//! - **`mainWindow.document.activeElement`.** É [`Ui::focused`]; `picker.contains(activeElement)` é o `is_ancestor` do nó do seletor.
//! - **`outline.reveal`, `preview` e `captureViewState`.** São as gravações [`Harness::outline_reveals`] e
//!   [`Harness::outline_view_events`]; os elementos `'symbol'` e `'child'` são objetos únicos (a identidade de `Rc` faz o papel da
//!   igualdade de texto do JavaScript).
//! - **`teardown`.** `contextViewService.hideContextView()`, `disposables.clear()`, `container.remove()` e `await timeout(0)` (o
//!   seletor adia o descarte da árvore para o turno seguinte) são [`Harness::teardown`]; o guarda de vazamento confere ao ser largado.
#![allow(dead_code)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::filters::FuzzyScore;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::services::theme_service::IThemeService;
use je_editor::undo_redo_service::UndoRedoService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService, IContextViewService};
use je_platform::files::files::FileStat;
use je_platform::opener::editor_types::{AuxiliaryEditorOptions, EditorOptions, ModalEditorPartOptions};
use je_platform::quickinput::quick_access::{IQuickAccessController, IQuickAccessOptions};
use je_platform::quickinput::quick_input::{IQuickInputService, IQuickPickItem};
use je_platform::resources::uri_file;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{Event as UiEvent, EventKind, Modifiers, NodeId, PointerButton, PointerData, Role, Ui};
use je_widgets::breadcrumbs_widget::{BreadcrumbsItemRef, BreadcrumbsWidget};
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::get_base_layer_hover_delegate;
use je_widgets::icon_label::IconLabelStyles;
use je_widgets::list::list_types::{IListVirtualDelegate, TemplateData};
use je_widgets::tree::data_tree::DataTreeOptions;
use je_widgets::tree::object_tree_model::ObjectTreeModel;
use je_widgets::tree::tree_types::{IDataSource, ITreeModel, ITreeRenderer, InputOrElement, TreeElementRenderDetails, TreeNodeApi};
use je_workbench::browser::labels::ResourceLabelsServices;
use je_workbench::browser::parts::editor::breadcrumbs::{BreadcrumbsService, I_BREADCRUMBS_SERVICE, IBreadcrumbsService};
use je_workbench::browser::parts::editor::breadcrumbs_control::{BreadcrumbsControl, BreadcrumbsControlOptions, BreadcrumbsControlServices};
use je_workbench::browser::parts::editor::breadcrumbs_model::BreadcrumbsModelServices;
use je_workbench::browser::parts::editor::breadcrumbs_picker::BreadcrumbsPickerServices;
use je_workbench::browser::parts::editor::editor::{IEditorGroupView, IEditorGroupsView};
use je_workbench::common::editor::{EditorIdentifier, EditorInputRef, EditorInputWithOptions, EditorsOrder, GroupIdentifier, IEditorPane, TextEditorOptions, UntypedEditorInput};
use je_workbench::contrib::notebook::browser::contrib::outline::notebook_outline::IFileIconTheme;
use je_workbench::services::contextview::native_context_view_service::{NativeContextViewService, native_node};
use je_workbench::services::editor::editor_groups_service::{
    EditorPartOptions, EditorPartOptionsChangeEvent, FindGroupScope, GroupDirection, GroupRef, GroupsOrder, I_EDITOR_GROUPS_SERVICE, IEditorGroup, IEditorGroupsService, IEditorPart,
    IModalEditorPart, PreferredGroup,
};
use je_workbench::services::editor::editor_resolver_service::{
    EditorAssociations, EditorInputFactoryObject, EditorMatches, EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions,
    EditorResolverServiceGetEditorsOptions, GlobPattern, IEditorResolverService, RegisteredEditorInfo, RegisteredEditorOptions, RegisteredEditorRegistrationInfo, ResolvedEditor,
};
use je_workbench::services::editor::editor_service::{
    ActiveTextEditorControl, FindEditorOptions, IEditorService, SaveAllEditorsOptions, SaveEditorsOptions, SaveEditorsResult,
};
use je_workbench::services::layout::layout_service::WindowId;
use je_workbench::services::notebook::common::notebook_document_service::{INotebookDocument, INotebookDocumentService};
use je_workbench::services::outline::outline::{
    BreadcrumbsOutlineElement, IBreadcrumbsDataSource, IOutline, IOutlineComparator, IOutlineCreator, IOutlineListConfig, IOutlineService, IQuickPickDataSource, OutlineChangeEvent,
    OutlineElement, OutlineTarget, OutlineTreeElement, QuickPickOutlineElement,
};
use je_workbench::services::workspaces::workspace_folder_label_service::WorkspaceFolderLabelService;
use je_workbench::test::browser::workbench_test_services::{TestFileEditorInput, TestQuickInputService};
use je_workbench::test::common::workbench_test_services::{FileStatChild, FileStatOptions, TestTextResourcePropertiesService, create_file_stat};

use crate::common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, run_with_real_timers, timeout};
use crate::editor_input_services::{WorkbenchServices, uri_identity_service};
use crate::main_thread_support::StubTextFileService;
use crate::notification_doubles::{EmptyThemeService, NoCommandService};
use crate::support::text_file_support::TestDecorationsService;

/// `X is not a function`: o membro que o objeto parcial do original não define.
fn not_defined(member: &str) -> ! {
    throw(Error::type_error(format!("{member} is not a function")))
}

/// `id = 1`: o grupo do caso (`group`).
pub const GROUP_ID: GroupIdentifier = 1;
/// `id = 2`: o `activeGroup` que o `createControl` cria antes de o caso ativar o grupo.
pub const OTHER_GROUP_ID: GroupIdentifier = 2;
/// `const resource = URI.file('/path/image.svg')`.
pub const RESOURCE_PATH: &str = "/path/image.svg";

//#region Foco e grupos

/// O estado que os dublês de grupo, de painel e de serviço compartilham (as variáveis `activeGroup`, `editorFocusCalls` e
/// `pendingEditorFocus` do `suite`).
pub struct FocusState {
    ui: Ui,
    editor_node: NodeId,
    editor_focus_calls: Cell<usize>,
    /// `pendingEditorFocus.length`: cada entrada é um `() => focusEditor()`.
    pending_editor_focus: Cell<usize>,
    active_group_id: Cell<GroupIdentifier>,
    groups: RefCell<Vec<Rc<MockGroup>>>,
}

impl FocusState {
    /// `focusEditor()`: `editorElement.focus()`.
    pub fn focus_editor(&self) {
        self.ui.focus(self.editor_node);
    }

    /// `deliverEditorFocus()`: entrega as mensagens de foco pendentes (`pendingEditorFocus.splice(0)`).
    pub fn deliver_editor_focus(&self) {
        for _ in 0..self.pending_editor_focus.replace(0) {
            self.focus_editor();
        }
    }

    /// O `focus()` de `group` e o que o painel chama (`group.focus()`).
    fn focus_group(&self, id: GroupIdentifier) {
        self.active_group_id.set(id);
        self.editor_focus_calls.set(self.editor_focus_calls.get() + 1);
        self.focus_editor();
        // Model a webview focus message whose delivery can outlive the focus request.
        self.pending_editor_focus.set(self.pending_editor_focus.get() + 1);
    }

    fn active_group(&self) -> Rc<MockGroup> {
        let id = self.active_group_id.get();
        self.groups.borrow().iter().find(|group| group.id == id).cloned().expect("o grupo ativo existe")
    }
}

/// O `groupsView` do grupo (`new class extends mock<IEditorGroupsView>() { activateGroup(target) { activeGroup = target; ... } }`).
struct MockGroupsView {
    state: Weak<FocusState>,
}

impl IEditorGroupsView for MockGroupsView {
    fn activate_group(&self, group: &Rc<dyn IEditorGroupView>) {
        if let Some(state) = self.state.upgrade() {
            state.active_group_id.set(group.id());
        }
    }

    fn part_options(&self) -> EditorPartOptions {
        not_defined("partOptions")
    }

    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        not_defined("onDidChangeEditorPartOptions")
    }
}

/// O painel (`new class extends mock<IVisibleEditorPane>() { onDidChangeControl = Event.None; focus() { group.focus(); } }`).
struct MockPane {
    state: Weak<FocusState>,
}

impl IEditorPane for MockPane {
    fn input(&self) -> Option<EditorInputRef> {
        not_defined("input")
    }

    fn group_id(&self) -> GroupIdentifier {
        not_defined("group")
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        not_defined("getViewState")
    }

    fn on_did_change_control(&self) -> Event<()> {
        Event::none()
    }

    fn focus(&self) {
        if let Some(state) = self.state.upgrade() {
            state.focus_group(GROUP_ID);
        }
    }
}

/// O grupo (`group`, com `id`, `activeEditor`, `activeEditorPane`, `groupsView` e `focus`) ou o `activeGroup` inicial (só o `id`).
pub struct MockGroup {
    id: GroupIdentifier,
    state: Weak<FocusState>,
    input: Option<EditorInputRef>,
    pane: Option<Rc<dyn IEditorPane>>,
    groups_view: Option<Rc<dyn IEditorGroupsView>>,
    /// `true` no `group`, o único que define `focus`.
    primary: bool,
}

impl IEditorGroup for MockGroup {
    fn id(&self) -> GroupIdentifier {
        self.id
    }

    fn window_id(&self) -> WindowId {
        not_defined("windowId")
    }

    fn active_editor(&self) -> Option<EditorInputRef> {
        match &self.input {
            Some(input) => Some(input.clone()),
            None => not_defined("activeEditor"),
        }
    }

    fn editors(&self) -> Vec<EditorInputRef> {
        not_defined("editors")
    }

    fn is_locked(&self) -> bool {
        not_defined("isLocked")
    }

    fn is_active(&self, _editor: &je_workbench::common::editor::EditorInputOrUntyped) -> bool {
        not_defined("isActive")
    }

    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        match &self.pane {
            Some(pane) => Some(pane.clone()),
            None => not_defined("activeEditorPane"),
        }
    }

    fn focus(&self) {
        if !self.primary {
            not_defined("focus");
        }
        if let Some(state) = self.state.upgrade() {
            state.focus_group(self.id);
        }
    }
}

impl IEditorGroupView for MockGroup {
    fn groups_view(&self) -> Rc<dyn IEditorGroupsView> {
        match &self.groups_view {
            Some(groups_view) => groups_view.clone(),
            None => not_defined("groupsView"),
        }
    }
}

/// `new class extends mock<IEditorGroupsService>() { override get activeGroup() { return activeGroup; } }`.
struct MockGroupsService {
    state: Weak<FocusState>,
}

impl IEditorGroupsService for MockGroupsService {
    fn active_group(&self) -> Rc<dyn IEditorGroup> {
        let state = self.state.upgrade().expect("o estado do caso está vivo");
        state.active_group()
    }

    fn groups(&self) -> Vec<Rc<dyn IEditorGroup>> {
        not_defined("groups")
    }

    fn get_groups(&self, _order: GroupsOrder) -> Vec<Rc<dyn IEditorGroup>> {
        not_defined("getGroups")
    }

    fn get_group(&self, _identifier: GroupIdentifier) -> Option<Rc<dyn IEditorGroup>> {
        not_defined("getGroup")
    }

    fn find_group(&self, _scope: &FindGroupScope) -> Option<Rc<dyn IEditorGroup>> {
        not_defined("findGroup")
    }

    fn add_group(&self, _location: Option<GroupRef>, _direction: GroupDirection) -> Rc<dyn IEditorGroup> {
        not_defined("addGroup")
    }

    fn main_part(&self) -> Rc<dyn IEditorPart> {
        not_defined("mainPart")
    }

    fn create_auxiliary_editor_part(&self, _options: Option<AuxiliaryEditorOptions>) -> Promise<Rc<dyn IEditorPart>> {
        not_defined("createAuxiliaryEditorPart")
    }

    fn create_modal_editor_part(&self, _options: Option<ModalEditorPartOptions>) -> Promise<Rc<dyn IModalEditorPart>> {
        not_defined("createModalEditorPart")
    }

    fn active_modal_editor_part(&self) -> Option<Rc<dyn IModalEditorPart>> {
        not_defined("activeModalEditorPart")
    }
}

//#endregion

//#region Serviços parciais

/// Uma chamada do `openEditor` espiado: `[editor, group]`.
#[derive(Clone)]
pub struct OpenedEditor {
    pub input: UntypedEditorInput,
    /// O `group` (`SIDE_GROUP`, `ACTIVE_GROUP`...) como número; `None` é o `undefined`.
    pub group: Option<GroupIdentifier>,
}

/// `{ resource, options: { preserveFocus, pinned, revealIfVisible } }` do primeiro argumento do `openEditor`.
#[derive(Debug, PartialEq)]
pub struct OpenedEditorView {
    pub resource: Option<String>,
    pub preserve_focus: Option<bool>,
    pub pinned: Option<bool>,
    pub reveal_if_visible: Option<bool>,
    /// Algum outro campo das opções está definido (o `deepStrictEqual` do original o acusaria).
    pub other_options_set: bool,
}

impl OpenedEditor {
    pub fn view(&self) -> OpenedEditorView {
        let (preserve_focus, pinned, reveal_if_visible, other_options_set) = match &self.input.options {
            Some(options) => (options.base.preserve_focus, options.base.pinned, options.base.reveal_if_visible, other_options_set(options)),
            None => (None, None, None, false),
        };
        OpenedEditorView {
            resource: self.input.resource.as_ref().map(|resource| resource.to_string(false).to_string_lossy()),
            preserve_focus,
            pinned,
            reveal_if_visible,
            other_options_set,
        }
    }
}

/// Algum campo das opções fora de `preserveFocus`, `pinned` e `revealIfVisible` está definido.
fn other_options_set(options: &TextEditorOptions) -> bool {
    let base: &EditorOptions = &options.base;
    options.selection.is_some()
        || options.selection_reveal_type.is_some()
        || options.selection_source.is_some()
        || base.activation.is_some()
        || base.force_reload.is_some()
        || base.reveal_if_opened.is_some()
        || base.sticky.is_some()
        || base.index.is_some()
        || base.inactive.is_some()
        || base.ignore_error.is_some()
        || base.override_.is_some()
        || base.source.is_some()
        || base.is_explicit.is_some()
        || base.view_state.is_some()
        || base.transient.is_some()
        || base.auxiliary.is_some()
        || base.modal.is_some()
}

/// O `IEditorService` do `workbenchInstantiationService` (`TestEditorService`) com o `openEditor` espiado (`spy(editorService,
/// 'openEditor')`, que chama o original e guarda os argumentos): a gravação vale desde o início do caso.
#[derive(Default)]
pub struct RecordingEditorService {
    opened: RefCell<Vec<OpenedEditor>>,
    /// O que o caso faz a cada `openEditor` (por exemplo, trocar o grupo ativo como o `openEditor` do `mock` dele faz), depois de a
    /// chamada ser gravada.
    on_open: RefCell<Option<Rc<dyn Fn()>>>,
}

impl RecordingEditorService {
    pub fn opened(&self) -> Vec<OpenedEditor> {
        self.opened.borrow().clone()
    }

    /// Registra o que roda depois de cada `openEditor` gravado.
    pub fn on_open(&self, hook: Rc<dyn Fn()>) {
        *self.on_open.borrow_mut() = Some(hook);
    }
}

const TEST_EDITOR_SERVICE: &str = "o TestEditorService só define openEditor para estes casos";

impl IEditorService for RecordingEditorService {
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        Vec::new()
    }

    fn active_editor(&self) -> Option<je_workbench::common::editor::EditorInputOrUntyped> {
        None
    }

    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl> {
        None
    }

    fn active_text_editor_language_id(&self) -> Option<String> {
        None
    }

    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.open_editor_in(editor, None)
    }

    fn open_editor_in(&self, editor: UntypedEditorInput, group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        self.opened.borrow_mut().push(OpenedEditor { input: editor, group: group.and_then(|group| group.as_number()) });
        let hook = self.on_open.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
        Promise::resolved(None)
    }

    fn find_editors(&self, _resource: &URI, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        Vec::new()
    }

    fn save(&self, _editors: Vec<EditorIdentifier>, _options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("{TEST_EDITOR_SERVICE}")
    }

    fn save_all(&self, _options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult> {
        unreachable!("{TEST_EDITOR_SERVICE}")
    }

    fn get_editors(&self, _order: EditorsOrder) -> Vec<EditorIdentifier> {
        Vec::new()
    }

    fn open_editors(&self, _editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        unreachable!("{TEST_EDITOR_SERVICE}")
    }

    fn is_visible(&self, _editor: &EditorInputRef) -> bool {
        false
    }
}

/// `new class extends mock<IQuickAccessController>() { override show(prefix) { quickAccessPrefixes.push(prefix); } }`.
struct RecordingQuickAccess {
    prefixes: Rc<RefCell<Vec<String>>>,
}

impl IQuickAccessController for RecordingQuickAccess {
    fn show(&self, value: &str, _options: Option<IQuickAccessOptions>) {
        self.prefixes.borrow_mut().push(value.to_owned());
    }

    fn pick(&self, _value: &str, _options: Option<IQuickAccessOptions>) -> Promise<Option<Vec<IQuickPickItem>>> {
        not_defined("pick")
    }
}

/// O `IContextMenuService` que nenhum caso usa.
struct UnusedContextMenuService;

impl IContextMenuService for UnusedContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        not_defined("onDidShowContextMenu")
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        not_defined("onDidHideContextMenu")
    }

    fn show_context_menu(&self, _request: ContextMenuRequest) {
        not_defined("showContextMenu")
    }
}

/// `getEditorMatches` do `IEditorResolverService` parcial.
pub type GetEditorMatchesFn = Box<dyn Fn(&URI, Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches>;

/// `setDefaultEditor` do `IEditorResolverService` parcial.
pub type SetDefaultEditorFn = Box<dyn Fn(&URI, &str, Option<bool>)>;

/// O `mock<IEditorResolverService>()` com o que o caso sobrescreve (`getEditorMatches` e `setDefaultEditor`); todo outro membro
/// lança `X is not a function`. O seletor de tipo de editor das migalhas só o lê com `showEditorTypePicker` ligado.
#[derive(Default)]
pub struct PartialEditorResolverService {
    pub get_editor_matches: Option<GetEditorMatchesFn>,
    pub set_default_editor: Option<SetDefaultEditorFn>,
}

impl IEditorResolverService for PartialEditorResolverService {
    fn get_associations_for_resource(&self, _resource: &URI) -> EditorAssociations {
        not_defined("getAssociationsForResource")
    }

    fn get_editor_matches(&self, resource: &URI, options: Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches {
        match &self.get_editor_matches {
            Some(get_editor_matches) => get_editor_matches(resource, options),
            None => not_defined("getEditorMatches"),
        }
    }

    fn set_default_editor(&self, resource: &URI, editor_id: &str, for_diff_editor: Option<bool>) {
        match &self.set_default_editor {
            Some(set_default_editor) => set_default_editor(resource, editor_id, for_diff_editor),
            None => not_defined("setDefaultEditor"),
        }
    }

    fn remove_diff_editor_association(&self, _resource: &URI, _filename_pattern: &str) {
        not_defined("removeDiffEditorAssociation")
    }

    fn on_did_change_editor_registrations(&self) -> Event<()> {
        Event::none()
    }

    fn buffer_change_events(&self, _callback: &dyn Fn()) {
        not_defined("bufferChangeEvents")
    }

    fn register_editor(
        &self,
        _glob_pattern: GlobPattern,
        _editor_info: RegisteredEditorRegistrationInfo,
        _options: RegisteredEditorOptions,
        _editor_factory_object: EditorInputFactoryObject,
    ) -> DisposableRef {
        not_defined("registerEditor")
    }

    fn resolve_editor(&self, _editor: &UntypedEditorInput, _preferred_group: Option<&PreferredGroup>) -> Promise<ResolvedEditor> {
        not_defined("resolveEditor")
    }

    fn get_editors(&self, _resource: &URI, _options: Option<&EditorResolverServiceGetEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        Vec::new()
    }

    fn get_all_editors(&self, _options: Option<&EditorResolverServiceGetAllEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        not_defined("getAllEditors")
    }

    fn get_binary_diff_fallback_editor(&self, _resource: &URI) -> Option<String> {
        not_defined("getBinaryDiffFallbackEditor")
    }

    fn get_all_user_associations(&self) -> EditorAssociations {
        not_defined("getAllUserAssociations")
    }
}

/// O `INotebookDocumentService` que os rótulos consultam: nenhum caderno aberto.
struct NoNotebookDocuments;

impl INotebookDocumentService for NoNotebookDocuments {
    fn get_notebook(&self, _uri: &URI) -> Option<Rc<dyn INotebookDocument>> {
        None
    }

    fn add_notebook_document(&self, _document: Rc<dyn INotebookDocument>) {}

    fn remove_notebook_document(&self, _document: Rc<dyn INotebookDocument>) {}
}

/// O estilo do texto dos rótulos.
fn label_styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles {
        foreground: Rgba::opaque(0xcc, 0xcc, 0xcc),
        highlight_foreground: None,
        highlight_weight: 700,
        font_size: 13.0,
        font_weight: 400,
        line_height: Some(22.0),
        italic: false,
        strikethrough: false,
    }
}

//#endregion

//#region O outline do caso

type OutlineModel = ObjectTreeModel<OutlineTreeElement, FuzzyScore>;
type OutlineNode = <OutlineModel as ITreeModel>::Node;

/// `breadcrumbsDataSource: { getBreadcrumbElements: () => [{ element: 'symbol', label: 'symbol' }] }`.
struct SymbolBreadcrumbs {
    symbol: OutlineElement,
}

impl IBreadcrumbsDataSource for SymbolBreadcrumbs {
    fn get_breadcrumb_elements(&self) -> Vec<BreadcrumbsOutlineElement> {
        vec![BreadcrumbsOutlineElement { element: self.symbol.clone(), label: "symbol".to_owned() }]
    }
}

/// `quickPickDataSource: { getQuickPickElements: () => [] }`.
struct NoQuickPickElements;

impl IQuickPickDataSource for NoQuickPickElements {
    fn get_quick_pick_elements(&self) -> Vec<QuickPickOutlineElement> {
        Vec::new()
    }
}

/// `treeDataSource: { getChildren: element => element === outline ? ['symbol'] : nestedOutline && element === 'symbol' ? ['child'] : [] }`.
struct SymbolDataSource {
    symbol: OutlineElement,
    child: OutlineElement,
    nested: bool,
}

impl IDataSource<Rc<dyn IOutline>, OutlineTreeElement> for SymbolDataSource {
    fn get_children(&self, element: &InputOrElement<Rc<dyn IOutline>, OutlineTreeElement>) -> Vec<OutlineTreeElement> {
        match element {
            InputOrElement::Input(_) => vec![OutlineTreeElement(self.symbol.clone())],
            InputOrElement::Element(element) if self.nested && *element == OutlineTreeElement(self.symbol.clone()) => vec![OutlineTreeElement(self.child.clone())],
            InputOrElement::Element(_) => Vec::new(),
        }
    }
}

/// `delegate: { getHeight: () => 22, getTemplateId: () => 'symbol' }`.
struct SymbolDelegate;

impl IListVirtualDelegate<OutlineTreeElement> for SymbolDelegate {
    fn get_height(&self, _element: &OutlineTreeElement) -> f64 {
        22.0
    }

    fn get_template_id(&self, _element: &OutlineTreeElement) -> String {
        "symbol".to_owned()
    }
}

/// `renderers: [{ templateId: 'symbol', renderTemplate: node => node, renderElement: (node, _index, template) => {
/// template.textContent = node.element; }, disposeTemplate: () => { } }]`: o `template` é o nó e o texto é o rótulo dele.
struct SymbolRenderer {
    ui: Ui,
}

impl ITreeRenderer<OutlineModel> for SymbolRenderer {
    fn template_id(&self) -> String {
        "symbol".to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container)
    }

    fn render_element(&self, node: &Rc<OutlineNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let Some(container) = template_data.downcast_ref::<NodeId>() else { return };
        let Some(element) = node.element() else { return };
        let text = element.0.downcast_ref::<String>().cloned().unwrap_or_default();
        self.ui.tree_mut().set_label(*container, text);
    }

    fn dispose_template(&self, _template_data: &TemplateData) {}
}

/// `comparator: { compareByPosition: () => 0, compareByName: () => 0, compareByType: () => 0 }`.
struct EqualComparator;

impl IOutlineComparator for EqualComparator {
    fn compare_by_position(&self, _a: &OutlineElement, _b: &OutlineElement) -> f64 {
        0.0
    }

    fn compare_by_type(&self, _a: &OutlineElement, _b: &OutlineElement) -> f64 {
        0.0
    }

    fn compare_by_name(&self, _a: &OutlineElement, _b: &OutlineElement) -> f64 {
        0.0
    }
}

/// O `IOutline<string>` de `createControl`.
struct TestOutline {
    uri: URI,
    config: IOutlineListConfig,
    symbol: OutlineElement,
    reveals: Rc<RefCell<Vec<bool>>>,
    view_events: Rc<RefCell<Vec<String>>>,
    store: Rc<DisposableStore>,
}

impl TestOutline {
    fn create(
        ui: &Ui,
        uri: URI,
        nested: bool,
        reveals: Rc<RefCell<Vec<bool>>>,
        view_events: Rc<RefCell<Vec<String>>>,
        store: Rc<DisposableStore>,
    ) -> Rc<dyn IOutline> {
        let symbol: OutlineElement = Rc::new("symbol".to_owned());
        let child: OutlineElement = Rc::new("child".to_owned());
        let config = IOutlineListConfig {
            breadcrumbs_data_source: Rc::new(SymbolBreadcrumbs { symbol: symbol.clone() }),
            delegate: Rc::new(SymbolDelegate),
            renderers: vec![Rc::new(SymbolRenderer { ui: ui.clone() })],
            tree_data_source: Rc::new(SymbolDataSource { symbol: symbol.clone(), child, nested }),
            comparator: Rc::new(EqualComparator),
            options: Rc::new(|| DataTreeOptions::<OutlineTreeElement, FuzzyScore>::default()),
            quick_pick_data_source: Rc::new(NoQuickPickElements),
        };
        Rc::new(Self { uri, config, symbol, reveals, view_events, store })
    }
}

impl IDisposable for TestOutline {
    fn dispose(&self) {}
}

impl IOutline for TestOutline {
    fn uri(&self) -> Option<URI> {
        Some(self.uri.clone())
    }

    fn config(&self) -> &IOutlineListConfig {
        &self.config
    }

    fn outline_kind(&self) -> String {
        "test".to_owned()
    }

    fn is_empty(&self) -> bool {
        false
    }

    fn active_element(&self) -> Option<OutlineElement> {
        Some(self.symbol.clone())
    }

    fn on_did_change(&self) -> Event<OutlineChangeEvent> {
        Event::none()
    }

    /// `reveal: (_element, _options, sideBySide) => { outlineReveals.push(sideBySide); }`.
    fn reveal(&self, _entry: &OutlineElement, _options: &EditorOptions, side_by_side: bool, _select: bool) -> MaybePromise<()> {
        self.reveals.borrow_mut().push(side_by_side);
        MaybePromise::Value(())
    }

    /// `preview: () => { outlineViewEvents.push('preview'); return toDisposable(() => outlineViewEvents.push('clearPreview')); }`.
    fn preview(&self, _entry: &OutlineElement) -> DisposableRef {
        self.view_events.borrow_mut().push("preview".to_owned());
        let events = self.view_events.clone();
        to_disposable(move || events.borrow_mut().push("clearPreview".to_owned()))
    }

    /// `captureViewState: () => disposables.add(toDisposable(() => outlineViewEvents.push('restore')))`.
    fn capture_view_state(&self) -> DisposableRef {
        let events = self.view_events.clone();
        self.store.add(to_disposable(move || events.borrow_mut().push("restore".to_owned())))
    }
}

/// `new class extends mock<IOutlineService>() { override readonly onDidChange = Event.None; override async createOutline() {
/// return withOutline ? outline : undefined; } }`.
struct TestOutlineService {
    outline: Option<Rc<dyn IOutline>>,
}

impl IOutlineService for TestOutlineService {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn can_create_outline(&self, _editor: &dyn IEditorPane) -> bool {
        not_defined("canCreateOutline")
    }

    fn create_outline(&self, _editor: Rc<dyn IEditorPane>, _target: OutlineTarget, _token: CancellationToken) -> Promise<Option<Rc<dyn IOutline>>> {
        Promise::resolved(self.outline.clone())
    }

    fn register_outline_creator(&self, _creator: Rc<dyn IOutlineCreator>) -> DisposableRef {
        not_defined("registerOutlineCreator")
    }

    fn notify_outline_changed(&self) {
        not_defined("notifyOutlineChanged")
    }
}

//#endregion

//#region A listagem de arquivos

/// `createFileStat(uri, false, false, true, false, children)`: uma pasta com os filhos dados.
pub fn directory_stat(uri: &URI, children: Vec<FileStatChild>) -> FileStat {
    create_file_stat(
        uri,
        &FileStatOptions { readonly: false, is_file: Some(false), is_directory: Some(true), is_symbolic_link: Some(false), children: Some(children), executable: None },
    )
    .into_stat()
}

/// `{ resource }`: um filho que só tem o recurso (arquivo).
pub fn file_child(resource: &URI) -> FileStatChild {
    FileStatChild { resource: resource.clone(), is_file: None, is_directory: None, is_symbolic_link: None, executable: None }
}

/// `{ resource: folder, isFile: false, isDirectory: true }`.
pub fn folder_child(resource: &URI) -> FileStatChild {
    FileStatChild { resource: resource.clone(), is_file: Some(false), is_directory: Some(true), is_symbolic_link: None, executable: None }
}

//#endregion

/// `createFileStat(...)` com o portão: `async resolve(uri) { await fileRead.p; return createFileStat(uri, false, false, true, false,
/// [{ resource }]); }`.
fn gate_resolve(file_service: &je_workbench::test::common::workbench_test_services::TestFileService, file_read: Rc<DeferredPromise<()>>, resource: URI) {
    file_service.stub_resolve(move |uri| {
        let gate = file_read.promise.clone();
        let (uri, resource) = (uri.clone(), resource.clone());
        async_fn(async move {
            gate.await?;
            Ok(directory_stat(&uri, vec![file_child(&resource)]))
        })
    });
}

/// A configuração do caso: `{ breadcrumbs: { filePath: 'on', symbolPath: 'on', useQuickPick: false, icons: true },
/// explorer: { decorations: { colors: false, badges: false } } }`.
fn configuration_object() -> Dynamic {
    Dynamic::object([
        (
            "breadcrumbs",
            Dynamic::object([
                ("filePath", Dynamic::string("on")),
                ("symbolPath", Dynamic::string("on")),
                ("useQuickPick", Dynamic::Bool(false)),
                ("icons", Dynamic::Bool(true)),
            ]),
        ),
        ("explorer", Dynamic::object([("decorations", Dynamic::object([("colors", Dynamic::Bool(false)), ("badges", Dynamic::Bool(false))]))])),
    ])
}

/// O que o caso espera de `destination`/`reveal`: os `sideBySide` do outline ou o grupo do `openEditor`.
#[derive(Debug, PartialEq)]
pub enum Destination {
    /// `outlineReveals`.
    Reveals(Vec<bool>),
    /// `openEditor.<chamada>.args[1]`, como o número do grupo (`SIDE_GROUP`, `ACTIVE_GROUP`) ou `undefined`.
    Group(Option<GroupIdentifier>),
}

/// O relógio do caso: o do mocha (`Real`) ou o de `runWithFakedTimers({ useFakeTimers: true }, ...)` (`Faked`).
#[derive(Clone, Copy)]
pub enum Timers {
    Real,
    Faked,
}

/// Roda o caso: o `setup` (o [`Harness`]), o corpo e o `teardown`, e só então larga o guarda de vazamento.
pub fn run_case<F, Fut>(timers: Timers, case: F)
where
    F: FnOnce(Rc<Harness>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let body = async move {
        let harness = Rc::new(Harness::new());
        case(harness.clone()).await;
        harness.teardown().await;
    };
    match timers {
        Timers::Real => run_with_real_timers(body),
        Timers::Faked => run_with_faked_timers(body),
    }
}

/// Os [`BreadcrumbsControlServices`] que o `workbenchInstantiationService` dos casos monta: os serviços reais que o porte tem
/// (`LabelService`, `TestFileService`, a configuração do caso...) e os dublês `mock<T>()` do que o caso não usa. O `setup` do
/// `BreadcrumbsControl` e o do `EditorHeaderControl` (que cria uma `BreadcrumbsControlFactory`) montam os mesmos serviços.
#[allow(clippy::too_many_arguments)]
pub fn breadcrumbs_control_services(
    guard: &LeakGuard,
    ui: &Ui,
    configuration_service: Rc<dyn IConfigurationService>,
    workbench: &WorkbenchServices,
    context_keys: Rc<dyn IContextKeyService>,
    context_view_service: Rc<dyn IContextViewService>,
    quick_input: Rc<dyn IQuickInputService>,
    editor_service: Rc<dyn IEditorService>,
    breadcrumbs_service: Rc<dyn IBreadcrumbsService>,
) -> BreadcrumbsControlServices {
    let theme_service: Rc<dyn IThemeService> = Rc::new(EmptyThemeService);
    let language_service = guard.add_rc(LanguageService::new(false));
    let model_service = guard.add_rc(ModelService::new(
        configuration_service.clone(),
        TestTextResourcePropertiesService::new(configuration_service.clone()),
        UndoRedoService::for_tests(),
    ));
    let resource_labels = Rc::new(ResourceLabelsServices {
        ui: ui.clone(),
        configuration_service: configuration_service.clone(),
        model_service,
        workspace_service: workbench.context_service.clone(),
        language_service,
        decorations_service: Rc::new(TestDecorationsService),
        theme_service: theme_service.clone(),
        label_service: workbench.label_service.clone(),
        text_file_service: Rc::new(StubTextFileService),
        notebook_document_service: Rc::new(NoNotebookDocuments),
        icon_label_styles: Rc::new(|| IconLabelStyles { text: label_styles(), disabled_foreground: None }),
    });

    let picker_services = Rc::new(BreadcrumbsPickerServices {
        ui: ui.clone(),
        theme_service: theme_service.clone(),
        configuration_service: configuration_service.clone(),
        text_resource_configuration_service: workbench.text_resource_configuration_service.clone(),
        workspace_service: workbench.context_service.clone(),
        editor_service: editor_service.clone(),
        file_service: workbench.file_service.clone(),
        resource_labels: resource_labels.clone(),
        file_icon_theme: Rc::new(|| IFileIconTheme { has_file_icons: false, has_folder_icons: false, hides_explorer_arrows: false }),
        on_did_file_icon_theme_change: Event::none(),
    });

    let model_services = BreadcrumbsModelServices {
        configuration_service: configuration_service.clone(),
        workspace_service: workbench.context_service.clone(),
        workspace_folder_label_service: Rc::new(WorkspaceFolderLabelService),
        outline_service: Rc::new(TestOutlineService { outline: None }),
        label_service: workbench.label_service.clone(),
        uri_identity_service: uri_identity_service(),
    };

    BreadcrumbsControlServices {
        ui: ui.clone(),
        context_key_service: context_keys,
        context_view_service,
        context_menu_service: Rc::new(UnusedContextMenuService),
        quick_input_service: quick_input,
        file_service: workbench.file_service.clone(),
        editor_service,
        editor_resolver_service: Rc::new(PartialEditorResolverService::default()),
        command_service: Rc::new(NoCommandService),
        label_service: workbench.label_service.clone(),
        configuration_service,
        hover_service: get_base_layer_hover_delegate(),
        breadcrumbs_service,
        theme_service,
        model_services,
        picker_services,
        resource_labels_services: resource_labels,
    }
}

/// O `setup` da suíte e os auxiliares dela.
pub struct Harness {
    pub guard: LeakGuard,
    pub ui: Ui,
    /// `workbenchInstantiationService`: o serviço de arquivos (o `resolve` é trocado pelo caso) e os demais de teste.
    pub workbench: WorkbenchServices,
    pub configuration_service: Rc<TestConfigurationService>,
    /// `fileRead`.
    pub file_read: Rc<DeferredPromise<()>>,
    pub editor_service: Rc<RecordingEditorService>,
    pub state: Rc<FocusState>,
    container: NodeId,
    context_view_service: Rc<NativeContextViewService>,
    breadcrumbs_service: Rc<BreadcrumbsService>,
    instantiation_service: Rc<InstantiationService>,
    base_services: BreadcrumbsControlServices,
    quick_access_prefixes: Rc<RefCell<Vec<String>>>,
    outline_reveals: Rc<RefCell<Vec<bool>>>,
    outline_view_events: Rc<RefCell<Vec<String>>>,
    control: RefCell<Option<Rc<BreadcrumbsControl>>>,
    widget: RefCell<Option<Rc<BreadcrumbsWidget>>>,
}

impl Harness {
    /// O `setup`: a configuração, os serviços, o contêiner e o `button` do editor.
    pub fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let ui = Ui::new();
        let root = ui.root();
        ui.tree_mut().set_bounds(root, Rect::new(0.0, 0.0, 1024.0, 768.0));

        let configuration = TestConfigurationService::new(Some(configuration_object()));
        let configuration_service: Rc<dyn IConfigurationService> = configuration.clone();
        let workbench = WorkbenchServices::new(&guard, &configuration);

        // `fileService: () => new class extends TestFileService { override async resolve(uri) { await fileRead.p; ... } }`
        let file_read = Rc::new(DeferredPromise::new());
        gate_resolve(&workbench.file_service, file_read.clone(), uri_file(RESOURCE_PATH));

        let context_keys = guard.add_rc(ContextKeyService::new(configuration_service.clone()));
        let context_keys: Rc<dyn IContextKeyService> = context_keys;
        let context_view_service = guard.add_rc(NativeContextViewService::new(&ui, root));
        let breadcrumbs_service = Rc::new(BreadcrumbsService::new());

        // `instantiationService.stub(IQuickInputService, new class extends mock<IQuickInputService>() { quickAccess })`
        let quick_input = TestQuickInputService::new();
        let quick_access_prefixes: Rc<RefCell<Vec<String>>> = Rc::default();
        quick_input.stub_quick_access(Rc::new(RecordingQuickAccess { prefixes: quick_access_prefixes.clone() }));

        let editor_service = Rc::new(RecordingEditorService::default());

        let (container, editor_node) = {
            let mut tree = ui.tree_mut();
            let container = create_el(&mut tree, root, Role::Generic, "");
            let editor_node = create_el(&mut tree, container, Role::Button, "");
            (container, editor_node)
        };
        let state = Rc::new(FocusState {
            ui: ui.clone(),
            editor_node,
            editor_focus_calls: Cell::new(0),
            pending_editor_focus: Cell::new(0),
            active_group_id: Cell::new(OTHER_GROUP_ID),
            groups: RefCell::new(Vec::new()),
        });

        let groups_service: Rc<dyn IEditorGroupsService> = Rc::new(MockGroupsService { state: Rc::downgrade(&state) });
        let breadcrumbs_service_dyn: Rc<dyn IBreadcrumbsService> = breadcrumbs_service.clone();
        let instantiation_service = InstantiationService::new(
            ServiceCollection::new().with(I_EDITOR_GROUPS_SERVICE, groups_service).with(I_BREADCRUMBS_SERVICE, breadcrumbs_service_dyn),
        );
        guard.add_rc(instantiation_service.clone());

        let base_services = breadcrumbs_control_services(
            &guard,
            &ui,
            configuration_service,
            &workbench,
            context_keys,
            context_view_service.clone(),
            quick_input,
            editor_service.clone(),
            breadcrumbs_service.clone(),
        );

        Self {
            guard,
            ui,
            workbench,
            configuration_service: configuration,
            file_read,
            editor_service,
            state,
            container,
            context_view_service,
            breadcrumbs_service,
            instantiation_service,
            base_services,
            quick_access_prefixes,
            outline_reveals: Rc::default(),
            outline_view_events: Rc::default(),
            control: RefCell::new(None),
            widget: RefCell::new(None),
        }
    }

    /// `resource`.
    pub fn resource(&self) -> URI {
        uri_file(RESOURCE_PATH)
    }

    /// `teardown`: `contextViewService.hideContextView()`, `disposables.clear()`, `container.remove()` e `await timeout(0)` (o
    /// seletor adia o descarte da árvore).
    pub async fn teardown(&self) {
        self.context_view_service.hide_context_view(None);
        self.guard.store().dispose();
        self.ui.remove(self.container);
        timeout(0).await.unwrap();
    }

    /// `createControl(withOutline, nestedOutline)`.
    pub async fn create_control(&self, with_outline: bool, nested_outline: bool) {
        let outline = with_outline.then(|| {
            TestOutline::create(&self.ui, self.resource(), nested_outline, self.outline_reveals.clone(), self.outline_view_events.clone(), self.guard.store())
        });
        let mut services = self.base_services.clone();
        services.model_services.outline_service = Rc::new(TestOutlineService { outline });

        let input = self.guard.add_rc(TestFileEditorInput::new(self.resource(), "testEditorInput"));
        let input: EditorInputRef = input;
        let pane: Rc<dyn IEditorPane> = Rc::new(MockPane { state: Rc::downgrade(&self.state) });
        let groups_view: Rc<dyn IEditorGroupsView> = Rc::new(MockGroupsView { state: Rc::downgrade(&self.state) });
        let group = Rc::new(MockGroup { id: GROUP_ID, state: Rc::downgrade(&self.state), input: Some(input), pane: Some(pane), groups_view: Some(groups_view), primary: true });
        let other = Rc::new(MockGroup { id: OTHER_GROUP_ID, state: Rc::downgrade(&self.state), input: None, pane: None, groups_view: None, primary: false });
        *self.state.groups.borrow_mut() = vec![group.clone(), other];
        self.state.active_group_id.set(OTHER_GROUP_ID);

        let options = BreadcrumbsControlOptions {
            show_file_icons: false,
            show_symbol_icons: true,
            show_decoration_colors: false,
            show_placeholder: false,
            drag_editor: false,
            ..BreadcrumbsControlOptions::default()
        };
        let group_view: Rc<dyn IEditorGroupView> = group;
        let control = self.guard.add_rc(BreadcrumbsControl::new(self.container, options, group_view, services));
        control.update();
        control.layout(Some((600.0, BreadcrumbsControl::HEIGHT)));
        let widget = self.breadcrumbs_service.get_widget(GROUP_ID).expect("o controle registrou o widget do grupo");
        let model = control.model().expect("o controle criou o modelo");
        let updated = model.on_did_update().to_promise(None);
        *self.control.borrow_mut() = Some(control);
        *self.widget.borrow_mut() = Some(widget);
        updated.await.unwrap();
        self.focus_editor();
    }

    /// `control`.
    pub fn control(&self) -> Rc<BreadcrumbsControl> {
        self.control.borrow().clone().expect("o controle foi criado")
    }

    /// `widget`.
    pub fn widget(&self) -> Rc<BreadcrumbsWidget> {
        self.widget.borrow().clone().expect("o controle foi criado")
    }

    /// `widget.getItems()`.
    pub fn items(&self) -> Vec<BreadcrumbsItemRef> {
        self.widget().get_items()
    }

    /// `widget.getItems().at(-1)`.
    pub fn last_item(&self) -> BreadcrumbsItemRef {
        self.items().last().cloned().expect("a migalha tem itens")
    }

    /// `widget.setSelection(widget.getItems().at(-1), payload)`.
    pub fn select_last(&self, payload: Rc<dyn Any>) {
        let item = self.last_item();
        self.widget().set_selection(Some(&item), Some(payload));
    }

    /// `control.domNode.querySelector('.monaco-breadcrumb-item:last-child').click()`.
    pub fn click_last_item(&self) {
        let node = *self.widget().item_nodes().last().expect("a migalha tem itens");
        self.click(node, false);
    }

    /// `node.click()` ou `node.dispatchEvent(new MouseEvent('click', { altKey, bubbles: true }))`.
    pub fn click(&self, node: NodeId, alt_key: bool) {
        self.ui.dispatch(&mut UiEvent::new(EventKind::Click, node).with_pointer(pointer(alt_key, 0)));
    }

    /// `node.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`.
    pub fn double_click(&self, node: NodeId) {
        self.ui.dispatch(&mut UiEvent::new(EventKind::DoubleClick, node).with_pointer(pointer(false, 2)));
    }

    /// `await fileRead.complete()`.
    pub async fn complete_file_read(&self) {
        self.file_read.complete(()).await.unwrap();
    }

    /// `waitForPicker()`: `await pickerShow.lastCall.returnValue`.
    pub async fn wait_for_picker(&self) {
        let show = self.control().last_picker_show().expect("nenhum seletor foi mostrado");
        show.await.unwrap();
    }

    /// `focusEditor()`.
    pub fn focus_editor(&self) {
        self.state.focus_editor();
    }

    /// `deliverEditorFocus()`.
    pub fn deliver_editor_focus(&self) {
        self.state.deliver_editor_focus();
    }

    /// `activeGroup.id`.
    pub fn active_group_id(&self) -> GroupIdentifier {
        self.state.active_group_id.get()
    }

    /// `activeGroup = group`.
    pub fn activate_group(&self, id: GroupIdentifier) {
        self.state.active_group_id.set(id);
    }

    /// `editorFocusCalls`.
    pub fn editor_focus_calls(&self) -> usize {
        self.state.editor_focus_calls.get()
    }

    /// `mainWindow.document.activeElement === editorElement`.
    pub fn editor_focused(&self) -> bool {
        self.ui.focused() == Some(self.state.editor_node)
    }

    /// As chamadas do `openEditor` espiado.
    pub fn opened_editors(&self) -> Vec<OpenedEditor> {
        self.editor_service.opened()
    }

    /// `outlineReveals`.
    pub fn outline_reveals(&self) -> Vec<bool> {
        self.outline_reveals.borrow().clone()
    }

    /// `outlineViewEvents`.
    pub fn outline_view_events(&self) -> Vec<String> {
        self.outline_view_events.borrow().clone()
    }

    /// `quickAccessPrefixes`.
    pub fn quick_access_prefixes(&self) -> Vec<String> {
        self.quick_access_prefixes.borrow().clone()
    }

    /// `await configurationService.setUserConfiguration('breadcrumbs.useQuickPick', true)`.
    pub async fn use_quick_pick(&self) {
        self.configuration_service.set_user_configuration("breadcrumbs.useQuickPick", Dynamic::Bool(true), None).await.unwrap();
    }

    /// `contextViewService.getContextViewElement()`.
    pub fn context_view_node(&self) -> NodeId {
        native_node(&self.context_view_service.get_context_view_element()).expect("o elemento da visão de contexto é um nó do Ui")
    }

    /// `root.querySelector('.classe')`: o primeiro descendente (em pré-ordem) com a classe.
    pub fn query_selector(&self, root: NodeId, class: &str) -> Option<NodeId> {
        let tree = self.ui.tree();
        let found = tree.subtree(root).into_iter().skip(1).find(|node| tree.has_class(*node, class));
        found
    }

    /// `contextViewService.getContextViewElement().querySelector('.monaco-breadcrumbs-picker')`.
    pub fn picker_element(&self) -> Option<NodeId> {
        self.query_selector(self.context_view_node(), "monaco-breadcrumbs-picker")
    }

    /// `contextViewService.getContextViewElement().querySelector('.monaco-list-row')`.
    pub fn list_row(&self) -> Option<NodeId> {
        self.query_selector(self.context_view_node(), "monaco-list-row")
    }

    /// `row.getAttribute('aria-expanded')`: ver as divergências do módulo.
    pub fn aria_expanded(&self, row: NodeId) -> Option<&'static str> {
        let twistie = self.query_selector(row, "monaco-tl-twistie")?;
        let tree = self.ui.tree();
        let collapsible = tree.has_class(twistie, "collapsible");
        let collapsed = tree.has_class(twistie, "collapsed");
        collapsible.then_some(if collapsed { "false" } else { "true" })
    }

    /// `picker.contains(mainWindow.document.activeElement)`.
    pub fn picker_focused(&self) -> bool {
        let (Some(picker), Some(focused)) = (self.picker_element(), self.ui.focused()) else {
            return false;
        };
        let contains = self.ui.tree().is_ancestor(picker, focused);
        contains
    }

    /// `assertPickerFocused()`.
    pub fn assert_picker_focused(&self) {
        assert_eq!((self.active_group_id(), self.editor_focus_calls(), self.picker_focused()), (GROUP_ID, 0, true));
    }

    /// `CommandsRegistry.getCommand(id)!.handler(accessor)` dentro de `instantiationService.invokeFunction`.
    pub fn invoke_command(&self, id: &str) {
        self.instantiation_service.invoke_function(|accessor| {
            let command = je_platform::commands::commands_registry().get_command(id).expect("o comando está registrado");
            (command.handler)(accessor, &[]);
        });
    }
}

/// O `PointerData` de um clique do mouse (`x` e `y` zero, como o `pageX` e o `pageY` de um evento sintético).
fn pointer(alt_key: bool, click_count: u32) -> PointerData {
    PointerData { x: 0.0, y: 0.0, button: PointerButton::Primary, modifiers: Modifiers { alt: alt_key, ..Modifiers::default() }, click_count }
}
