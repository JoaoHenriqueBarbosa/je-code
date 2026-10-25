//! Dublês dos serviços que os testes de `workbench/test/browser/notifications*.test.ts` montam com o
//! `workbenchInstantiationService(undefined, store)`: o mesmo conjunto de `notifications_list.rs`, em um só lugar para os
//! testes de toasts, telemetria e visão acessível.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::JsString;
use je_base::actions::ActionRunner;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::keybindings::{Keybinding, ResolvedKeybinding};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, none, plain_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::actions::actions::{IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuCreateOptions, MenuId};
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::{IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::keybinding::{IKeybindingService, KeybindingsSchemaContribution, KeyboardEvent};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::notification::INotificationService;
use je_platform::opener::opener::{IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri};
use je_platform::storage::StorageService;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::telemetry::ITelemetryService;
use je_render::Rgba;
use je_theme::color::Color;
use je_ui::Ui;
use je_widgets::button::{ButtonStyles, ContextMenuDelegate, IContextMenuProvider as IButtonContextMenuProvider};
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider};
use je_widgets::grid::Direction;
use je_widgets::gridview::ViewDimensions;
use je_widgets::progress_bar::IProgressBarStyles;
use je_workbench::browser::parts::notifications::notifications_actions::WorkbenchNotificationActionProvider;
use je_workbench::browser::parts::notifications::notifications_viewer::{NotificationActionProvider, NotificationRenderer, NotificationRendererServices, NotificationStyles};
use crate::support::test_editor_group_view::TestEditorGroupView;
use je_workbench::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService};
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::test::browser::workbench_test_services::TestEditorGroupsService;
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use je_workbench::services::layout::layout_service::{
    AddedContainer, ContainerLayout, ILayoutService, IWorkbenchLayoutService, LayoutContainerId, LayoutOffsetInfo, LayoutPart, PanelAlignment, PartVisibilityChangeEvent, Parts, Position, WindowId,
    WindowMaximizedChange,
};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;

/// `new MockKeybindingService()`: sem atalho nenhum.
pub struct NoKeybindingService;

impl IKeybindingService for NoKeybindingService {
    fn in_chord_mode(&self) -> bool {
        false
    }
    fn on_did_update_keybindings(&self) -> Event<()> {
        Event::none()
    }
    fn resolve_keybinding(&self, _keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn resolve_keyboard_event(&self, _keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        panic!("o caso não resolve eventos de teclado")
    }
    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn dispatch_event(&self, _e: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        false
    }
    fn soft_dispatch(&self, _keyboard_event: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        ResolutionResult::NoMatchingKb
    }
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
    fn dispatch_by_user_settings_label(&self, _user_settings_label: &str, _target: Option<Rc<dyn IContextKeyServiceTarget>>) {}
    fn lookup_keybindings(&self, _command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn lookup_keybinding(&self, _command_id: &str, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }
    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn custom_keybindings_count(&self) -> usize {
        0
    }
    fn might_produce_printable_character(&self, _event: &KeyboardEvent) -> bool {
        false
    }
    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        plain_disposable(|| {})
    }
    fn toggle_logging(&self) -> bool {
        false
    }
    fn append_keybinding(&self, label: &str, _command_id: Option<&str>, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> String {
        label.to_owned()
    }
    fn dump_debug_info(&self) -> String {
        String::new()
    }
    fn dump_debug_info_json(&self) -> String {
        String::new()
    }
}

/// O `IOpenerService` que guarda o que `open` recebe (`opened.push(resource.toString())`).
#[derive(Default)]
pub struct RecordingOpener {
    pub opened: RefCell<Vec<String>>,
}

impl IOpenerService for RecordingOpener {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unreachable!("o caso não registra opener")
    }
    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unreachable!("o caso não registra validador")
    }
    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unreachable!("o caso não registra resolvedor")
    }
    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unreachable!("o caso não troca o opener externo")
    }
    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unreachable!("o caso não registra opener externo")
    }
    fn open(&self, resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        let text = match resource {
            OpenResource::Uri(uri) => uri.to_string(false).to_string_lossy(),
            OpenResource::Text(text) => text.to_string_lossy(),
        };
        self.opened.borrow_mut().push(text);
        Promise::resolved(true)
    }
    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unreachable!("o caso não resolve URI externo")
    }
}

/// O `ICommandService` do `workbenchInstantiationService`: nenhum caso desta suíte executa comando.
pub struct NoCommandService;

impl ICommandService for NoCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, _args: Vec<Dynamic>) -> Promise<Dynamic> {
        panic!("executeCommand({command_id}) não é esperado nesta suíte")
    }
}

/// O `IClipboardService` que só o menu de copiar usaria.
pub struct NoClipboard;

impl IClipboardService for NoClipboard {
    fn write_text(&self, _text: &str) -> Promise<()> {
        Promise::resolved(())
    }
    fn read_text(&self) -> Promise<String> {
        panic!("readText is not a function")
    }
    fn read_find_text(&self) -> Promise<JsString> {
        panic!("readFindText is not a function")
    }
    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        panic!("writeFindText is not a function")
    }
    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        panic!("writeResources is not a function")
    }
    fn read_resources(&self) -> Promise<Vec<URI>> {
        panic!("readResources is not a function")
    }
    fn has_resources(&self) -> Promise<bool> {
        panic!("hasResources is not a function")
    }
}

/// O `IContextMenuService` dos dois contratos de menu de contexto do `je-widgets`: nenhum caso abre um menu.
pub struct NoContextMenu;

impl IContextMenuProvider for NoContextMenu {
    fn show_context_menu(&self, _delegate: Rc<dyn IContextMenuDelegate>) {
        panic!("showContextMenu is not a function")
    }
}

impl IButtonContextMenuProvider for NoContextMenu {
    fn show_context_menu(&self, _delegate: ContextMenuDelegate) {
        panic!("showContextMenu is not a function")
    }
}

/// O `TestThemeService` do `workbenchInstantiationService`: o tema vazio, que nunca muda.
pub struct EmptyColorTheme;

impl IColorTheme for EmptyColorTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        None
    }
}

pub struct EmptyThemeService;

impl IThemeService for EmptyThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        Rc::new(EmptyColorTheme)
    }
    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        Event::none()
    }
}

/// O `IMenuService` do `workbenchInstantiationService`: o menu da posição só é lido ao abrir o dropdown.
pub struct NoMenuService;

impl IMenuService for NoMenuService {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        unreachable!("TypeError: createMenu is not a function")
    }
    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        unreachable!("TypeError: getMenuActions is not a function")
    }
    fn get_menu_contexts(&self, _id: &MenuId) -> HashSet<String> {
        unreachable!("TypeError: getMenuContexts is not a function")
    }
    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {
        unreachable!("TypeError: resetHiddenStates is not a function")
    }
}

/// O `IAccessibilitySignalService` de teste: não toca som.
pub struct SilentAccessibilitySignals;

impl IAccessibilitySignalService for SilentAccessibilitySignals {
    fn play_signal(&self, _signal: AccessibilitySignal) {}
}

/// O `TestLayoutService` do `workbenchTestServices.ts`: tudo visível, sem tamanho de parte.
pub struct TestLayoutService;

pub const BODY: LayoutContainerId = LayoutContainerId(1);

impl ILayoutService for TestLayoutService {
    fn on_did_layout_main_container(&self) -> Event<Dimension> {
        Event::none()
    }
    fn on_did_layout_container(&self) -> Event<ContainerLayout> {
        Event::none()
    }
    fn on_did_layout_active_container(&self) -> Event<Dimension> {
        Event::none()
    }
    fn on_did_add_container(&self) -> Event<AddedContainer> {
        Event::none()
    }
    fn on_did_change_active_container(&self) -> Event<()> {
        Event::none()
    }
    fn main_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }
    fn active_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }
    fn main_container(&self) -> LayoutContainerId {
        BODY
    }
    fn active_container(&self) -> LayoutContainerId {
        BODY
    }
    fn containers(&self) -> Vec<LayoutContainerId> {
        vec![BODY]
    }
    fn get_container(&self, _window: WindowId) -> LayoutContainerId {
        BODY
    }
    fn when_container_styles_loaded(&self, _window: WindowId) -> Option<Promise<()>> {
        None
    }
    fn main_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }
    fn active_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }
    fn focus(&self) {}
}

impl IWorkbenchLayoutService for TestLayoutService {
    fn on_did_change_zen_mode(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_window_maximized(&self) -> Event<WindowMaximizedChange> {
        Event::none()
    }
    fn on_did_change_main_editor_centered_layout(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_panel_position(&self) -> Event<String> {
        Event::none()
    }
    fn on_did_change_panel_alignment(&self) -> Event<PanelAlignment> {
        Event::none()
    }
    fn on_did_change_part_visibility(&self) -> Event<PartVisibilityChangeEvent> {
        Event::none()
    }
    fn on_did_change_notifications_visibility(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_auxiliary_bar_maximized(&self) -> Event<()> {
        Event::none()
    }
    fn opened_default_editors(&self) -> bool {
        false
    }
    fn layout(&self) {}
    fn is_restored(&self) -> bool {
        true
    }
    fn when_restored(&self) -> Promise<()> {
        Promise::resolved(())
    }
    fn has_focus(&self, _part: Parts) -> bool {
        false
    }
    fn is_floating_panels_enabled(&self) -> bool {
        false
    }
    fn is_modern_ui_compact(&self) -> bool {
        false
    }
    fn focus_part(&self, _part: Parts, _target_window: Option<WindowId>) {}
    fn get_container_of_part(&self, _target_window: WindowId, _part: Option<Parts>) -> Option<LayoutContainerId> {
        Some(BODY)
    }
    fn is_visible(&self, _part: Parts, _target_window: Option<WindowId>) -> bool {
        true
    }
    fn set_part_hidden(&self, _hidden: bool, _part: Parts) {}
    fn is_secondary_side_bar_visible(&self) -> bool {
        false
    }
    fn toggle_secondary_side_bar(&self) {}
    fn toggle_maximized_panel(&self) {}
    fn is_panel_maximized(&self) -> bool {
        false
    }
    fn toggle_maximized_auxiliary_bar(&self) {}
    fn set_auxiliary_bar_maximized(&self, _maximized: bool) -> bool {
        false
    }
    fn is_auxiliary_bar_maximized(&self) -> bool {
        false
    }
    fn has_main_window_border(&self) -> bool {
        false
    }
    fn get_main_window_border_radius(&self) -> Option<String> {
        None
    }
    fn get_side_bar_position(&self) -> Position {
        Position::Left
    }
    fn toggle_menu_bar(&self) {}
    fn get_panel_position(&self) -> Position {
        Position::Left
    }
    fn set_panel_position(&self, _position: Position) {}
    fn get_panel_alignment(&self) -> PanelAlignment {
        PanelAlignment::Center
    }
    fn set_panel_alignment(&self, _alignment: PanelAlignment) {}
    fn get_maximum_editor_dimensions(&self, _container: LayoutContainerId) -> Dimension {
        panic!("not implemented")
    }
    fn toggle_zen_mode(&self) {}
    fn is_main_editor_layout_centered(&self) -> bool {
        false
    }
    fn center_main_editor_layout(&self, _active: bool) {}
    fn get_size(&self, _part: Parts) -> ViewDimensions {
        panic!("Method not implemented.")
    }
    fn set_size(&self, _part: Parts, _size: ViewDimensions) {
        panic!("Method not implemented.")
    }
    fn resize_part(&self, _part: Parts, _size_change_width: f64, _size_change_height: f64) {}
    fn register_part(&self, _part: Rc<dyn LayoutPart>) -> DisposableRef {
        none()
    }
    fn is_window_maximized(&self, _target_window: WindowId) -> bool {
        false
    }
    fn update_window_maximized_state(&self, _target_window: WindowId, _maximized: bool) {}
    fn get_visible_neighbor_part(&self, _part: Parts, _direction: Direction) -> Option<Parts> {
        None
    }
}


/// `new TestEditorGroupsService([new TestEditorGroupView(0)])`, como o `workbenchInstantiationService`.
pub fn test_editor_groups_service() -> Rc<dyn IEditorGroupsService> {
    let group: Rc<dyn IEditorGroup> = TestEditorGroupView::new(0);
    TestEditorGroupsService::new(Some(vec![group]))
}

/// O `TestLifecycleService` do `workbenchInstantiationService`, registrado no `store` do caso (o original o põe no
/// `disposables` do `workbenchInstantiationService(undefined, store)`).
pub fn test_lifecycle_service(store: &DisposableStore) -> Rc<dyn ILifecycleService> {
    store.add_rc(TestLifecycleService::new())
}

/// Os serviços do renderer e da lista (`workbenchInstantiationService(undefined, store)`), com o opener dado.
pub fn renderer_services_in(store: &DisposableStore, ui: &Ui, opener: Rc<dyn IOpenerService>) -> NotificationRendererServices {
    let storage = store.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let notification_service = store.add_rc(NotificationService::new(storage_service));
    let notification_service: Rc<dyn INotificationService> = notification_service;
    let telemetry_service: Rc<dyn ITelemetryService> = Rc::new(TestNotificationTelemetryService::new());
    renderer_services_with(ui, opener, notification_service, telemetry_service, Rc::new(NoContextMenu), Rc::new(NoContextMenu), Rc::new(NoCommandService))
}

/// Os serviços do renderer e da lista com cada dublê escolhido pelo caso (o `instantiation.stub(...)` do original).
pub fn renderer_services_with(
    ui: &Ui,
    opener: Rc<dyn IOpenerService>,
    notification_service: Rc<dyn INotificationService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    dropdown_context_menu: Rc<dyn IContextMenuProvider>,
    button_context_menu: Rc<dyn IButtonContextMenuProvider>,
    command_service: Rc<dyn ICommandService>,
) -> NotificationRendererServices {
    let configuration_service: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
    let actions: Rc<dyn NotificationActionProvider> = WorkbenchNotificationActionProvider::new(command_service, Rc::new(NoClipboard));
    NotificationRendererServices {
        ui: ui.clone(),
        opener_service: opener,
        keybinding_service: Rc::new(NoKeybindingService),
        configuration_service,
        telemetry_service,
        notification_service,
        dropdown_context_menu,
        button_context_menu,
        actions,
        styles: NotificationStyles {
            foreground: Rgba::default(),
            link_foreground: Rgba::default(),
            info_icon_foreground: Rgba::default(),
            warning_icon_foreground: Rgba::default(),
            error_icon_foreground: Rgba::default(),
            button_styles: ButtonStyles::default(),
            progress_bar_styles: IProgressBarStyles::default(),
        },
    }
}

/// O `suiteSetup` da suíte `NotificationRenderer`: um renderer descartável cria as ações `static` do
/// `NotificationTemplateRenderer` antes de o guarda de vazamentos ligar (o rastreador é por fio, então cada caso repete).
pub fn warm_up_static_actions() {
    let store = DisposableStore::new();
    let ui = Ui::new();
    let services = renderer_services_in(&store, &ui, Rc::new(RecordingOpener::default()));
    let runner = store.add_rc(ActionRunner::new());
    let _renderer = NotificationRenderer::new(runner, None, services);
    store.dispose();
}
