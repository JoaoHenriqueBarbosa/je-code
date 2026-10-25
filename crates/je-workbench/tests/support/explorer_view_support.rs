//! O `setup` da suíte `context menu` de `explorerView.test.ts`, montando o [`ExplorerView`] real com a árvore real.
//!
//! Divergências do `setup` do original:
//! - **`workbenchInstantiationService`.** Os serviços são os de teste do porte (`TestContextService`, `TestFileService`,
//!   `LabelService` real, `TestConfigurationService` com `{ explorer: { autoReveal: false, compactFolders: false } }`...) e os
//!   dublês dos que o original troca por `stub` (`IViewDescriptorService`, `IExplorerService`, `IContextMenuService`,
//!   `IEditorResolverService`). O que o original não define lança `TypeError: <membro> is not a function` ([`not_defined`]).
//! - **A árvore de teste.** O original cria a sua `WorkbenchCompressibleAsyncDataTree` com renderer e fonte de dados próprios e a
//!   injeta na vista com `stubInstance`. O porte cria a árvore em `createTree` com o `FilesRenderer`, a `ExplorerDataSource` e o
//!   `FilesFilter` reais (não há `stubInstance`), então a vista desenha as linhas de verdade: os nós das pastas ficam com
//!   `isDirectoryResolved` verdadeiro para a fonte de dados não ir ao disco, e o `isIncompressible` do renderer de teste do original
//!   (`!item.isDirectory`) é o `ExplorerCompressionDelegate`.
//! - **`renderForTesting(container)`.** `renderBody` é o [`ViewPaneDelegate::render_body`] da vista, chamado direto no contêiner.
//! - **`getContextMenuTarget` e `dispatchEvent`.** O evento de menu de contexto que a árvore emite é o
//!   [`ExplorerContextMenuEvent`] entregue a [`ExplorerView::on_context_menu`]: o item (ou nenhum), o `BrowserEvent` (botão
//!   secundário, ou o `F10` com `shift` do teclado) e o nó alvo.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, none};
use je_base::nls::LocalizedString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::actions::menu_service::MenuService;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuMenuDelegate, IContextMenuService};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::list::list_service::ListService;
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::uri_file;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::storage::StorageService;
use je_platform::workspace::WorkspaceIdentifier;
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;
use je_render::Rgba;
use je_ui::{Modifiers, NodeId, PointerButton, Role, Ui};
use je_widgets::count_badge::ICountBadgeStyles;
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_label::IconLabelStyles;
use je_widgets::input_box::InputBoxStyles;
use je_widgets::hover_api::get_base_layer_hover_delegate;
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::BrowserEvent;
use je_widgets::progress_bar::IProgressBarOptions;
use je_widgets::tree::tree_types::TreeMouseEventTarget;
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::browser::labels::ResourceLabelsServices;
use je_workbench::browser::parts::views::view_pane::{IViewPaneOptions, IViewPaneToolbar, IViewWelcomeView, ViewPaneServices, ViewPaneToolbarArgs};
use je_workbench::browser::workbench_tree::WorkbenchTreeServices;
use je_workbench::common::views::{
    IViewContainerModel, IViewContentDescriptor, IViewDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc, ViewContainersChange,
    ViewDescriptorRc, ViewVisibilityState, ViewsContainerChange, ViewsLocationChange,
};
use je_workbench::contrib::files::browser::views::explorer_view::{
    ExplorerContextMenuEvent, ExplorerRendererServices, ExplorerView, ExplorerViewServices, IExplorerResourceContextKey, IExplorerViewContainerDelegate, IExplorerViewPaneOptions,
};
use je_workbench::contrib::files::browser::views::explorer_viewer_renderer::{FileIconThemeInfo, InlineEditServices};
use je_workbench::contrib::files::browser_files::{EditableItem, ExplorerBulkEditOptions, ExplorerReveal, IExplorerService, IExplorerView};
use je_workbench::contrib::files::common_files::{LexicographicOptions, SortOrder, SortOrderConfiguration};
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::services::editor::editor_groups_service::PreferredGroup;
use je_workbench::services::editor::editor_resolver_service::{
    EditorAssociations, EditorInputFactoryObject, EditorMatches, EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions,
    EditorResolverServiceGetEditorsOptions, GlobPattern, IEditorResolverService, RegisteredEditorInfo, RegisteredEditorOptions, RegisteredEditorRegistrationInfo, ResolvedEditor,
};
use je_workbench::services::notebook::common::notebook_document_service::{INotebookDocument, INotebookDocumentService};
use je_workbench::services::workspaces::workspace_editing::{DidEnterWorkspaceEvent, IWorkspaceEditingService};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use je_workbench::common::editor::UntypedEditorInput;

use crate::anything_quick_access_support::{RecordingNotificationService, TestOpenerService, TestSearchService};
use crate::common::LeakGuard;
use crate::editor_input_services::{TestAccessibilityService, WorkbenchServices, uri_identity_service};
use crate::extension_service_doubles::TestDialogService;
use crate::main_thread_support::StubTextFileService;
use crate::notification_doubles::{EmptyThemeService, NoCommandService, NoKeybindingService, TestLayoutService};
use crate::test_clipboard_service::TestClipboardService;
use crate::support::text_file_support::TestDecorationsService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `TestProgressService` do `workbenchInstantiationService`: roda a tarefa na hora, com `Progress.None`.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// `instantiationService.stub(IContextMenuService, { showContextMenu })`: guarda o delegado do último menu mostrado.
pub struct RecordingContextMenuService {
    menu: RefCell<Option<IContextMenuMenuDelegate>>,
}

impl RecordingContextMenuService {
    fn new() -> Rc<Self> {
        Rc::new(Self { menu: RefCell::new(None) })
    }
}

impl IContextMenuService for RecordingContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        not_defined("onDidShowContextMenu")
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        not_defined("onDidHideContextMenu")
    }

    fn show_context_menu(&self, request: ContextMenuRequest) {
        match request {
            // assert.ok(hasKey(delegate, { menuActionOptions: true }))
            ContextMenuRequest::Menu(delegate) => {
                assert!(delegate.menu_action_options.is_some());
                *self.menu.borrow_mut() = Some(delegate);
            }
            ContextMenuRequest::Delegate(_) => panic!("o menu de contexto do explorador tem as opções do menu"),
        }
    }
}

/// `instantiationService.stub(IExplorerService, { roots, registerView, findClosest, findClosestRoot, isEditable, getEditable })`. O
/// `sortOrderConfiguration` é o padrão que a `ExplorerDataSource` e o `FileSorter` reais leem (no original eles são `stubInstance`).
struct StubExplorerService {
    root: Rc<ExplorerItem>,
}

impl IExplorerService for StubExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        vec![self.root.clone()]
    }

    fn sort_order_configuration(&self) -> SortOrderConfiguration {
        SortOrderConfiguration { sort_order: SortOrder::Default, lexicographic_options: LexicographicOptions::Default, reverse: false }
    }

    fn get_context(&self, _respect_multi_selection: bool, _ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        not_defined("getContext")
    }

    fn get_editable(&self) -> Option<EditableItem> {
        None
    }

    fn is_editable(&self, _stat: Option<&Rc<ExplorerItem>>) -> bool {
        false
    }

    fn find_closest(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        Some(self.root.clone())
    }

    fn find_closest_root(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        Some(self.root.clone())
    }

    fn set_to_copy(&self, _stats: Vec<Rc<ExplorerItem>>, _cut: bool) -> Promise<()> {
        not_defined("setToCopy")
    }

    fn apply_bulk_edit(&self, _edit: Vec<ResourceFileEdit>, _options: ExplorerBulkEditOptions) -> Promise<()> {
        not_defined("applyBulkEdit")
    }

    fn select(&self, _resource: &URI, _reveal: Option<ExplorerReveal>) -> Promise<()> {
        not_defined("select")
    }

    fn register_view(&self, _context_and_refresh_provider: Rc<dyn IExplorerView>) {}
}

/// `instantiationService.stub(IEditorResolverService, { getEditors: () => [] })`.
struct NoEditors;

impl IEditorResolverService for NoEditors {
    fn get_associations_for_resource(&self, _resource: &URI) -> EditorAssociations {
        not_defined("getAssociationsForResource")
    }

    fn get_editor_matches(&self, _resource: &URI, _options: Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches {
        not_defined("getEditorMatches")
    }

    fn set_default_editor(&self, _resource: &URI, _editor_id: &str, _for_diff_editor: Option<bool>) {
        not_defined("setDefaultEditor")
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
    ) -> je_base::lifecycle::DisposableRef {
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

/// O `IWorkspaceEditingService` que o `workbenchInstantiationService` entrega: nenhum membro é usado pelo caso.
struct UnusedWorkspaceEditingService;

impl IWorkspaceEditingService for UnusedWorkspaceEditingService {
    fn on_did_enter_workspace(&self) -> Event<DidEnterWorkspaceEvent> {
        Event::none()
    }

    fn add_folders(&self, _folders: Vec<WorkspaceFolderCreationData>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("addFolders")
    }

    fn remove_folders(&self, _folders: Vec<URI>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("removeFolders")
    }

    fn update_folders(&self, _index: f64, _delete_count: Option<f64>, _folders_to_add: Option<Vec<WorkspaceFolderCreationData>>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("updateFolders")
    }

    fn enter_workspace(&self, _path: &URI) -> Promise<()> {
        not_defined("enterWorkspace")
    }

    fn create_and_enter_workspace(&self, _folders: Vec<WorkspaceFolderCreationData>, _path: Option<URI>) -> Promise<()> {
        not_defined("createAndEnterWorkspace")
    }

    fn save_and_enter_workspace(&self, _path: &URI) -> Promise<()> {
        not_defined("saveAndEnterWorkspace")
    }

    fn copy_workspace_settings(&self, _to_workspace: &WorkspaceIdentifier) -> Promise<()> {
        not_defined("copyWorkspaceSettings")
    }

    fn pick_new_workspace_path(&self) -> Promise<Option<URI>> {
        not_defined("pickNewWorkspacePath")
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

/// `upcastPartial<IViewDescriptorService>({ onDidChangeLocation: Event.None, getViewLocationById: () => ViewContainerLocation.Sidebar })`.
struct StubViewDescriptorService;

impl IViewDescriptorService for StubViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        not_defined("viewContainers")
    }

    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        Event::none()
    }

    fn get_default_view_container(&self, _location: ViewContainerLocation) -> Option<ViewContainerRc> {
        not_defined("getDefaultViewContainer")
    }

    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerById")
    }

    fn is_view_container_removed_permanently(&self, _id: &str) -> bool {
        not_defined("isViewContainerRemovedPermanently")
    }

    fn get_default_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getDefaultViewContainerLocation")
    }

    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getViewContainerLocation")
    }

    fn get_view_containers_by_location(&self, _location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        not_defined("getViewContainersByLocation")
    }

    fn get_view_container_model(&self, _view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        not_defined("getViewContainerModel")
    }

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        not_defined("onDidChangeContainerLocation")
    }

    fn move_view_container_to_location(&self, _view_container: &ViewContainerRc, _location: ViewContainerLocation, _requested_index: Option<usize>, _reason: Option<&str>) {
        not_defined("moveViewContainerToLocation")
    }

    fn get_view_container_badge_enablement_state(&self, _id: &str) -> bool {
        not_defined("getViewContainerBadgeEnablementState")
    }

    fn set_view_container_badge_enablement_state(&self, _id: &str, _badges_enabled: bool) {
        not_defined("setViewContainerBadgeEnablementState")
    }

    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc> {
        let name = LocalizedString { original: "Explorer".to_string(), value: "Explorer".to_string() };
        Some(Rc::new(IViewDescriptor::new(id, name)))
    }

    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerByViewId")
    }

    fn get_default_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getDefaultContainerById")
    }

    fn get_view_location_by_id(&self, _id: &str) -> Option<ViewContainerLocation> {
        Some(ViewContainerLocation::Sidebar)
    }

    fn can_move_views(&self) -> bool {
        not_defined("canMoveViews")
    }

    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        Event::none()
    }

    fn move_views_to_container(&self, _views: &[ViewDescriptorRc], _view_container: &ViewContainerRc, _visibility_state: Option<ViewVisibilityState>, _reason: Option<&str>) {
        not_defined("moveViewsToContainer")
    }

    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        Event::none()
    }

    fn move_view_to_location(&self, _view: &ViewDescriptorRc, _location: ViewContainerLocation, _reason: Option<&str>) {
        not_defined("moveViewToLocation")
    }

    fn reset(&self) {
        not_defined("reset")
    }
}

/// A barra de ferramentas do cabeçalho: a vista de teste não desenha o cabeçalho.
struct NoToolbar;

impl IDisposable for NoToolbar {
    fn dispose(&self) {}
}

impl IViewPaneToolbar for NoToolbar {
    fn set_actions(&self, _primary_actions: &[ActionRef], _secondary_actions: &[ActionRef]) {}

    fn set_context(&self, _context: Option<Dynamic>) {}
}

/// A mensagem de boas-vindas: o `ExplorerView` de teste nunca a mostra.
struct NoWelcomeView;

impl IDisposable for NoWelcomeView {
    fn dispose(&self) {}
}

impl IViewWelcomeView for NoWelcomeView {
    fn render(&self, _contents: &[Rc<IViewContentDescriptor>]) {}

    fn layout(&self, _height: f64, _width: f64, _wide: bool) {}

    fn focus(&self) {}
}

/// O `ResourceContextKey` do `createInstance(ResourceContextKey)`.
struct NoResourceContext;

impl IDisposable for NoResourceContext {
    fn dispose(&self) {}
}

impl IExplorerResourceContextKey for NoResourceContext {
    fn set(&self, _resource: Option<&URI>) {}
}

/// `delegate: { willOpenElement: () => { }, didOpenElement: () => { } }`.
struct NoopContainerDelegate;

impl IExplorerViewContainerDelegate for NoopContainerDelegate {
    fn will_open_element(&self, _event: Option<&BrowserEvent>) {}

    fn did_open_element(&self, _event: Option<&BrowserEvent>) {}
}

/// O estilo do texto do cabeçalho e dos rótulos.
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

/// O `setup` da suíte `context menu`: a raiz `/workspace` com `folder` (e `first.txt`, `second.txt`) e `target.txt`.
pub struct ExplorerViewHarness {
    pub view: Rc<ExplorerView>,
    pub ui: Ui,
    pub container: NodeId,
    pub root: Rc<ExplorerItem>,
    pub folder: Rc<ExplorerItem>,
    pub first: Rc<ExplorerItem>,
    pub second: Rc<ExplorerItem>,
    pub target: Rc<ExplorerItem>,
    menu_service: Rc<RecordingContextMenuService>,
    workbench: WorkbenchServices,
    config: Rc<TestConfigurationService>,
}

impl ExplorerViewHarness {
    /// `createItem(path, isDirectory)`: `new ExplorerItem(URI.file(path), fileService, configService, NullFilesConfigurationService,
    /// undefined, isDirectory)`.
    pub fn create_item(&self, path: &str, is_directory: bool, parent: Option<&Rc<ExplorerItem>>) -> Rc<ExplorerItem> {
        create_item(&self.workbench, &self.config, path, is_directory, parent)
    }

    pub fn new(guard: &LeakGuard) -> Self {
        let config = TestConfigurationService::new(Some(Dynamic::object([(
            "explorer",
            Dynamic::object([("autoReveal", Dynamic::Bool(false)), ("compactFolders", Dynamic::Bool(false))]),
        )])));
        let config_service: Rc<dyn IConfigurationService> = config.clone();
        let workbench = WorkbenchServices::new(guard, &config);

        let root = create_item(&workbench, &config, "/workspace", true, None);
        let folder = create_item(&workbench, &config, "/workspace/folder", true, None);
        let first = create_item(&workbench, &config, "/workspace/folder/first.txt", false, None);
        let second = create_item(&workbench, &config, "/workspace/folder/second.txt", false, None);
        let target = create_item(&workbench, &config, "/workspace/target.txt", false, None);
        root.add_child(&folder);
        root.add_child(&target);
        folder.add_child(&first);
        folder.add_child(&second);

        let ui = Ui::new();
        let context_keys = guard.add_rc(ContextKeyService::new(config_service.clone()));
        let context_keys: Rc<dyn IContextKeyService> = context_keys;
        let storage: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        let command_service: Rc<dyn ICommandService> = Rc::new(NoCommandService);
        let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(NoKeybindingService);
        let menu_service = RecordingContextMenuService::new();
        let lifecycle_service = guard.add_rc(TestLifecycleService::new());

        let pane = ViewPaneServices {
            ui: ui.clone(),
            keybinding_service: keybinding_service.clone(),
            context_menu_service: menu_service.clone(),
            configuration_service: config_service.clone(),
            context_key_service: context_keys.clone(),
            view_descriptor_service: Rc::new(StubViewDescriptorService),
            menu_service: MenuService::new(command_service.clone(), keybinding_service, storage.clone()),
            opener_service: TestOpenerService::new(),
            hover_service: get_base_layer_hover_delegate(),
            lifecycle_service,
            accessible_view_information_service: None,
            header_styles: label_styles(),
            description_foreground: Rgba::opaque(0x99, 0x99, 0x99),
            progress_bar_options: IProgressBarOptions::default(),
            create_toolbar: Rc::new(|_args: ViewPaneToolbarArgs| -> Rc<dyn IViewPaneToolbar> { Rc::new(NoToolbar) }),
            create_action_view_item: Rc::new(|_, _, _| None),
            create_welcome_view: Rc::new(|_| -> Rc<dyn IViewWelcomeView> { Rc::new(NoWelcomeView) }),
        };

        let language_service = guard.add_rc(LanguageService::new(false));
        let model_service = guard.add_rc(ModelService::new(
            config_service.clone(),
            je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService::new(config_service.clone()),
            UndoRedoService::for_tests(),
        ));
        let resource_labels = ResourceLabelsServices {
            ui: ui.clone(),
            configuration_service: config_service.clone(),
            model_service,
            workspace_service: workbench.context_service.clone(),
            language_service,
            decorations_service: Rc::new(TestDecorationsService),
            theme_service: Rc::new(EmptyThemeService),
            label_service: workbench.label_service.clone(),
            text_file_service: Rc::new(StubTextFileService),
            notebook_document_service: Rc::new(NoNotebookDocuments),
            icon_label_styles: Rc::new(|| IconLabelStyles { text: label_styles(), disabled_foreground: None }),
        };

        let services = ExplorerViewServices {
            pane,
            context_service: workbench.context_service.clone(),
            progress_service: Rc::new(TestProgressService),
            notification_service: RecordingNotificationService::new(),
            editor_service: Rc::new(TestEditorService),
            editor_resolver_service: Rc::new(NoEditors),
            layout_service: Rc::new(TestLayoutService),
            label_service: workbench.label_service.clone(),
            telemetry_service: Rc::new(TestNotificationTelemetryService::new()),
            explorer_service: Rc::new(StubExplorerService { root: root.clone() }),
            storage_service: storage,
            // instantiationService.stub(IClipboardService, new TestClipboardService())
            clipboard_service: Rc::new(TestClipboardService::new()),
            file_service: workbench.file_service.clone(),
            uri_identity_service: uri_identity_service(),
            command_service,
            accessibility_service: Rc::new(TestAccessibilityService),
            environment_service: TestEnvironmentService::new(),
            files_config_service: workbench.files_configuration_service.clone(),
            search_service: Rc::new(TestSearchService),
            dialog_service: Rc::new(TestDialogService),
            workspace_editing_service: Rc::new(UnusedWorkspaceEditingService),
            tree: WorkbenchTreeServices {
                context_key_service: context_keys,
                list_service: ListService::new(),
                configuration_service: config_service,
                default_list_styles: Rc::new(IListStyles::default),
            },
            resource_labels: Rc::new(resource_labels),
            renderer: ExplorerRendererServices {
                inline_edit: InlineEditServices { context_view_provider: None, context_menu_service: menu_service.clone(), input_box_styles: Rc::new(InputBoxStyles::default) },
                file_icon_theme: Rc::new(FileIconThemeInfo::default),
                badge_styles: ICountBadgeStyles::default(),
            },
            resource_context: Rc::new(NoResourceContext),
            on_did_color_theme_change: Event::none(),
            bind_file_icon_theme_scope: Rc::new(|_| none()),
            list_override_styles: Rc::new(|_| IListStyles::default()),
            external_file_drop: Rc::new(|_| Promise::resolved(())),
        };

        let view = guard.add_rc(ExplorerView::new(
            IExplorerViewPaneOptions {
                pane: IViewPaneOptions {
                    id: "testExplorerContextMenu".to_string(),
                    pane: je_widgets::paneview::PaneOptions { title: "Explorer".to_string(), ..Default::default() },
                    ..IViewPaneOptions::default()
                },
                delegate: Some(Rc::new(NoopContainerDelegate)),
            },
            services,
        ));

        let root_node = ui.root();
        let container = create_el(&mut ui.tree_mut(), root_node, Role::Generic, "explorer-context-menu-test");
        // view.renderForTesting(container)
        je_workbench::browser::parts::views::view_pane::ViewPaneDelegate::render_body(&*view, container);
        // tree.layout(300, 400)
        je_workbench::browser::parts::views::view_pane::ViewPaneDelegate::layout_body(&*view, 300.0, 400.0);

        Self { view, ui, container, root, folder, first, second, target, menu_service, workbench, config }
    }

    /// `await tree.setInput(root); await tree.expand(folder)` do `setup`: a vista visível com a entrada da árvore e a pasta aberta.
    pub async fn setup(&self) {
        self.view.set_visible(true);
        self.view.set_tree_input().await.unwrap();
        self.view.tree().widget().expand(&self.folder, false).await.unwrap();
    }

    /// `menu`: o último delegado entregue ao `showContextMenu`.
    pub fn menu(&self) -> Option<IContextMenuMenuDelegate> {
        self.menu_service.menu.borrow().clone()
    }

    /// `menu?.menuActionOptions?.arg` quando é um `URI`.
    pub fn menu_target(&self) -> Option<URI> {
        let arg = self.menu()?.menu_action_options?.arg?;
        <URI as ApiValue>::from_dynamic(&arg).ok()
    }

    /// `menu?.getActionsContext?.()`: a lista de recursos.
    pub fn menu_selection(&self) -> Option<Vec<URI>> {
        let context = (self.menu()?.get_actions_context?)(None)?;
        let Dynamic::Array(items) = context else {
            panic!("o contexto das ações do menu é uma lista de recursos");
        };
        Some(items.to_vec().iter().map(|item| <URI as ApiValue>::from_dynamic(item).expect("um recurso")).collect())
    }

    /// O `contextmenu` do ponteiro (botão secundário) ou do teclado (`F10` com `shift`) sobre `target` (`None` é a própria árvore).
    pub fn context_menu_event(&self, item: Option<&Rc<ExplorerItem>>, keyboard: bool, target: Option<NodeId>) -> ExplorerContextMenuEvent {
        let browser_event = BrowserEvent {
            modifiers: Modifiers { shift: keyboard, ..Modifiers::default() },
            button: if keyboard { PointerButton::Primary } else { PointerButton::Secondary },
            target,
            click_count: if keyboard { 0 } else { 1 },
            is_keyboard: keyboard,
            force_event: false,
        };
        ExplorerContextMenuEvent { element: item.cloned(), browser_event, target: TreeMouseEventTarget::Element }
    }

    /// `showContextMenu(item, browserEvent)`: `tree.setFocus(item ? [item] : [])` e o `contextmenu` no alvo; resolve quando o
    /// menu foi mostrado.
    pub fn show_context_menu(&self, item: Option<&Rc<ExplorerItem>>, keyboard: bool) -> Promise<()> {
        self.view.tree().set_focus(&item.cloned().into_iter().collect::<Vec<_>>(), None);
        self.view.on_context_menu(&self.context_menu_event(item, keyboard, None))
    }

    /// `dom.append(getContextMenuTarget(target), $('input'))`: um campo de texto dentro da árvore.
    pub fn append_input(&self) -> NodeId {
        let tree_element = self.view.tree().get_html_element();
        create_el(&mut self.ui.tree_mut(), tree_element, Role::TextInput, "input")
    }
}

/// `createItem` do `setup`.
fn create_item(workbench: &WorkbenchServices, config: &Rc<TestConfigurationService>, path: &str, is_directory: bool, parent: Option<&Rc<ExplorerItem>>) -> Rc<ExplorerItem> {
    let item = ExplorerItem::new(
        uri_file(path),
        workbench.file_service.clone(),
        config.clone(),
        workbench.files_configuration_service.clone(),
        parent,
        Some(is_directory),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    if is_directory {
        item.set_directory_resolved(true);
    }
    item
}
