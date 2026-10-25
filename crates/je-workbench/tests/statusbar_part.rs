//! Porte de `src/vs/workbench/test/browser/parts/statusbar/statusbarPart.test.ts` (suíte `StatusbarPart`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso; `store.add(...)` vira
//!   `guard.add_rc` / `guard.add`.
//! - **DOM vira estado.** O `container` (`document.createElement('div')`) é um [`ViewElement`], e `container.style.backgroundColor` é o
//!   `background-color` do estilo inline, no texto que o CSSOM devolve (`rgb(...)`), ou vazio quando a propriedade não existe.
//! - **`TestMainStatusbarPart`.** `windowHasFocus` é o `Cell` lido pelo `has_focus` da janela alvo (o `hasWindowFocus` sobrescrito), e o
//!   `updateStyles` contado é o gancho `on_update_styles` da parte.
//! - **`TestFloatingPanelsLayoutService`.** `floatingPanelsEnabled` e `modernUICompact` são o par de chaves do `TestLayoutService`
//!   (`set_floating_panels_enabled` e `set_modern_ui_compact`, por fio de teste).
//! - **`TestHostService`** é o dublê de `support/part_doubles.rs`; o `activeWindowEmitter` do caso 3 é o emissor `active_window`, e o
//!   `mainWindow.vscodeWindowId` é `MAIN_WINDOW.0`.
//! - **`fireConfigChange`** é `fire_configuration_change`: o literal `{ affectsConfiguration: candidate => candidate === key }` é o
//!   `ConfigurationChangeEvent` real da chave.
//! - **`instantiationService.stub(IHoverService, new class extends mock<IHoverService>() { })`** é o item de barra e a fábrica de
//!   itens que as partes sem entradas nunca chamam (a fábrica falha com `not expected`).
//! - **Caso 1.** O `IHoverService` do original é substituído por um `setupManagedHover` que guarda as opções iniciais e as
//!   atualizadas. O `HoverService` do porte é concreto, então o caso observa o mesmo contrato no `ManagedHoverWidget` que ele cria
//!   ao mostrar a dica: a primeira atualização leva as opções iniciais (com o `onDidShow`) e a seguinte, depois de `item.update`
//!   com a dica comum, leva as opções restauradas (nenhuma). Mostrar a dica é `show_managed_hover`, que espera o temporizador de 0 ms.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "support/part_doubles.rs"]
mod part_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use configuration_event_support::fire_configuration_change;
use je_base::async_::timeout;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::{
    HoverDelay, HoverDelegateTarget, HoverDomEventKind, HoverDomHandler, HoverDomTarget, HoverService, HoverServiceDeps, IHoverContextView, IHoverContextViewFactory, IHoverDelegate,
    IHoverDom, IHoverOptions, IHoverWidgetFactory, IHoverWidgetHandle, IManagedHoverOptions, IManagedHoverWidget, IManagedHoverWidgetFactory, ManagedHoverContent,
};
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};
use je_platform::notification::INotificationService;
use je_platform::storage::StorageService;
use je_platform::workspace::WorkbenchState;
use je_workbench::browser::parts::statusbar::statusbar_item::{StatusbarEntryItem, StatusbarItemEvent, StatusbarItemEventKind, StatusbarItemServices, StatusbarItemView, StatusbarProgressIcon};
use je_workbench::browser::parts::statusbar::statusbar_part::{
    STATUS_BAR_BACKGROUND, STATUS_BAR_INACTIVE_BACKGROUND, STATUS_BAR_NO_FOLDER_BACKGROUND, StatusbarContextMenuEvent, StatusbarPart, StatusbarPartDeps, StatusbarPartView,
};
use je_workbench::services::layout::layout_service::{IWorkbenchLayoutService, LayoutSettings, MAIN_WINDOW};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::services::statusbar::statusbar::{StatusbarCommand, StatusbarEntry, StatusbarStyleOverride, StatusbarTooltip, TooltipContent, TooltipWithCommands};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;
use je_workbench::test::common::workbench_test_services::{TestContextService, TestStorageService};
use notification_doubles::{EmptyThemeService, NoCommandService, NoKeybindingService, TestLayoutService, set_floating_panels_enabled, set_modern_ui_compact};
use part_doubles::{TestContextMenuService, TestHostService, main_window};
use test_color_theme_service::{TestColorTheme, TestThemeService};

// region: dublês

/// O DOM do `HoverService`: só os atributos (o `custom-hover` do `setupManagedHover`); nenhum ouvinte é entregue.
#[derive(Default)]
struct AttributeDom {
    attributes: RefCell<Vec<(usize, String, String)>>,
}

fn key(element: &Rc<ViewElement>) -> usize {
    Rc::as_ptr(element) as usize
}

impl IHoverDom for AttributeDom {
    fn add_listener(&self, _target: HoverDomTarget, _kind: HoverDomEventKind, _capture: bool, _handler: HoverDomHandler) -> DisposableRef {
        none()
    }
    fn active_element(&self) -> Option<Rc<ViewElement>> {
        None
    }
    fn is_ancestor_of_active_element(&self, _element: &Rc<ViewElement>) -> bool {
        false
    }
    fn focus(&self, _element: &Rc<ViewElement>) {}
    fn has_attribute(&self, element: &Rc<ViewElement>, name: &str) -> bool {
        self.get_attribute(element, name).is_some()
    }
    fn get_attribute(&self, element: &Rc<ViewElement>, name: &str) -> Option<String> {
        self.attributes.borrow().iter().find(|(k, n, _)| *k == key(element) && n == name).map(|(_, _, value)| value.clone())
    }
    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str) {
        let mut attributes = self.attributes.borrow_mut();
        attributes.retain(|(k, n, _)| !(*k == key(element) && n == name));
        attributes.push((key(element), name.to_owned(), value.to_owned()));
    }
    fn remove_attribute(&self, element: &Rc<ViewElement>, name: &str) {
        self.attributes.borrow_mut().retain(|(k, n, _)| !(*k == key(element) && n == name));
    }
    fn is_editable_element(&self, _element: &Rc<ViewElement>) -> bool {
        false
    }
    fn observe_intersection(&self, _element: &Rc<ViewElement>, _on_change: Rc<dyn Fn(bool)>) -> Option<DisposableRef> {
        None
    }
}

struct FixedLayoutService {
    container: Rc<ViewElement>,
}

impl ILayoutService for FixedLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn active_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn get_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        Event::none()
    }
    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 1280.0, inner_height: 800.0, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

struct SilentAccessibilityService;

impl IAccessibilityService for SilentAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        false
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// Nenhum caso cria um hover de verdade: o `HoverService` só entrega o hover gerenciável à fábrica do caso.
struct NoHoverWidgets;

impl IHoverWidgetFactory for NoHoverWidgets {
    fn create(&self, _options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle> {
        unreachable!("o caso não cria hovers simples")
    }
}

struct NoContextViews;

impl IHoverContextViewFactory for NoContextViews {
    fn create(&self, _container: Rc<ViewElement>) -> Rc<dyn IHoverContextView> {
        unreachable!("o caso não cria visões de contexto")
    }
}

/// O `ManagedHoverWidget` do `setupManagedHover`, que guarda o `onDidShow` de cada `update`.
struct RecordingManagedHoverWidget {
    shown: Rc<RefCell<Vec<Option<Rc<dyn Fn()>>>>>,
    disposed: Cell<bool>,
}

impl IDisposable for RecordingManagedHoverWidget {
    fn dispose(&self) {
        self.disposed.set(true);
    }
}

impl IManagedHoverWidget for RecordingManagedHoverWidget {
    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    fn update(&self, _content: ManagedHoverContent, _focus: Option<bool>, options: Option<IManagedHoverOptions>) -> Promise<()> {
        self.shown.borrow_mut().push(options.and_then(|options| options.on_did_show));
        Promise::resolved(())
    }
}

struct RecordingManagedHoverWidgets {
    shown: Rc<RefCell<Vec<Option<Rc<dyn Fn()>>>>>,
}

impl IManagedHoverWidgetFactory for RecordingManagedHoverWidgets {
    fn create(&self, _delegate: &Rc<dyn IHoverDelegate>, _target: Rc<HoverDelegateTarget>, _fade_in_animation: bool) -> Rc<dyn IManagedHoverWidget> {
        Rc::new(RecordingManagedHoverWidget { shown: self.shown.clone(), disposed: Cell::new(false) })
    }
}

/// O `WorkbenchHoverDelegate` das partes e do item: sem atraso.
struct InstantHoverDelegate;

impl IHoverDelegate for InstantHoverDelegate {
    fn delay(&self) -> HoverDelay {
        HoverDelay::Fixed(0.0)
    }
}

/// O que o item faz com o DOM: nada.
struct NoItemView;

impl StatusbarItemView for NoItemView {
    fn add_listener(&self, _element: &Rc<ViewElement>, _kind: StatusbarItemEventKind, _handler: Rc<dyn Fn(&StatusbarItemEvent)>) -> DisposableRef {
        none()
    }
    fn add_touch_target(&self, _element: &Rc<ViewElement>) -> DisposableRef {
        none()
    }
    fn set_attribute(&self, _element: &Rc<ViewElement>, _name: &str, _value: &str) {}
    fn append_content(&self, _container: &Rc<ViewElement>, _content: &Rc<ViewElement>) {}
    fn render_label(&self, _label_container: &Rc<ViewElement>, _progress: Option<StatusbarProgressIcon>, _text: &str) {}
}

/// O que a parte faz com o DOM: nada além do estado dos `ViewElement`s.
struct NoPartView;

impl StatusbarPartView for NoPartView {
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn insert_before(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>, _reference: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn clear_node(&self, _parent: &Rc<ViewElement>) {}
    fn remove(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
    }
    fn set_id(&self, _element: &Rc<ViewElement>, _id: &str) {}
    fn set_tab_index(&self, _element: &Rc<ViewElement>, _index: i32) {}
    fn add_context_menu_listener(&self, _element: &Rc<ViewElement>, _handler: Rc<dyn Fn(&StatusbarContextMenuEvent)>) -> DisposableRef {
        none()
    }
    fn add_touch_target(&self, _element: &Rc<ViewElement>) -> DisposableRef {
        none()
    }
    fn add_hover_listeners(&self, _element: &Rc<ViewElement>, _on_over: Rc<dyn Fn()>, _on_out: Rc<dyn Fn()>) -> DisposableRef {
        none()
    }
    fn is_ancestor_of_active_element(&self, _element: &Rc<ViewElement>) -> bool {
        false
    }
    fn focus(&self, _element: &Rc<ViewElement>) {}
    fn set_style_sheet(&self, _container: &Rc<ViewElement>, _css: &str) {}
}

// endregion

/// A parte criada, a configuração, o foco do documento da janela e o contador de `updateStyles` do `TestMainStatusbarPart`.
struct Fixture {
    part: Rc<StatusbarPart>,
    configuration_service: Rc<TestConfigurationService>,
    window_has_focus: Rc<Cell<bool>>,
    update_styles_calls: Rc<Cell<usize>>,
}

/// O `new TestMainStatusbarPart(instantiationService, themeService, contextService, storageService, layoutService,
/// contextMenuService, contextKeyService, configurationService, hostService)` dos casos.
fn create_part(guard: &LeakGuard, theme_service: Rc<TestThemeService>, context_service: Rc<TestContextService>, host_service: Rc<TestHostService>) -> Fixture {
    let configuration_service = TestConfigurationService::new(None);
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    guard.add_rc(theme_service.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    let context_key_service = guard.add_rc(ContextKeyService::new(configuration.clone()));
    let storage = guard.add_rc(TestStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let layout_service: Rc<dyn IWorkbenchLayoutService> = Rc::new(TestLayoutService);

    let window_has_focus = Rc::new(Cell::new(true));
    let focus = window_has_focus.clone();
    let update_styles_calls = Rc::new(Cell::new(0));
    let calls = update_styles_calls.clone();

    let part = guard.add_rc(StatusbarPart::new_main(StatusbarPartDeps {
        window: main_window(Rc::new(move || focus.get())),
        theme_service,
        context_service,
        storage_service,
        layout_service,
        context_menu_service: Rc::new(TestContextMenuService),
        context_key_service,
        configuration_service: configuration,
        host_service,
        command_service: Rc::new(NoCommandService),
        view: Rc::new(NoPartView),
        item_services: Rc::new(|| panic!("not expected")),
        hover_delegate: Rc::new(InstantHoverDelegate),
        toggle_statusbar_visibility: Rc::new(|| panic!("not expected")),
        on_update_styles: Some(Rc::new(move || calls.set(calls.get() + 1))),
    }));

    Fixture { part, configuration_service, window_has_focus, update_styles_calls }
}

fn background(container: &Rc<ViewElement>) -> String {
    container.style_get_property("background-color").unwrap_or_default()
}

fn theme(colors: &[(&str, &str)]) -> Rc<TestThemeService> {
    TestThemeService::new(TestColorTheme::new(colors))
}

#[test]
fn forwards_tooltip_on_did_show_through_native_status_entry_updates() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let shown: Rc<RefCell<Vec<Option<Rc<dyn Fn()>>>>> = Rc::new(RefCell::new(Vec::new()));
        let container = ViewElement::with_classes(&["div"]);
        let configuration: Rc<dyn IConfigurationService> = TestConfigurationService::new(None);
        let layout_service: Rc<dyn ILayoutService> = Rc::new(FixedLayoutService { container: container.clone() });
        let hover_service = guard.add_rc(HoverService::new(HoverServiceDeps {
            dom: Rc::new(AttributeDom::default()),
            configuration_service: configuration,
            on_did_show_context_menu: Event::none(),
            keybinding_service: Rc::new(NoKeybindingService),
            layout_service,
            accessibility_service: Rc::new(SilentAccessibilityService),
            hover_widgets: Rc::new(NoHoverWidgets),
            context_views: Rc::new(NoContextViews),
            managed_hover_widgets: Rc::new(RecordingManagedHoverWidgets { shown: shown.clone() }),
        }));
        let storage = guard.add_rc(TestStorageService::new());
        let storage_service: Rc<dyn StorageService> = Rc::new(storage);
        let notification_service = guard.add_rc(NotificationService::new(storage_service));
        let notification_service: Rc<dyn INotificationService> = notification_service;

        let on_did_show: Rc<dyn Fn()> = Rc::new(|| {});
        let mut entry = StatusbarEntry::new("Status", "$(copilot)", "Status");
        entry.command = Some(StatusbarCommand::ShowTooltip);
        let mut with_tooltip = entry.clone();
        with_tooltip.tooltip = Some(StatusbarTooltip::WithCommands(Rc::new(TooltipWithCommands {
            content: TooltipContent::Text("Sale".to_owned()),
            commands: Vec::new(),
            on_did_show: Some(on_did_show.clone()),
        })));
        let item = guard.add_rc(StatusbarEntryItem::new(
            StatusbarItemServices {
                command_service: Rc::new(NoCommandService),
                hover_service: hover_service.clone(),
                notification_service,
                telemetry_service: Rc::new(TestNotificationTelemetryService::new()),
                theme_service: Rc::new(EmptyThemeService),
                view: Rc::new(NoItemView),
            },
            container.clone(),
            with_tooltip,
            Rc::new(InstantHoverDelegate),
        ));

        // Mostra a dica: o hover gerenciável recebe as opções iniciais (as que `setupManagedHover` guardou).
        hover_service.show_managed_hover(&container);
        timeout(0).await.unwrap();
        let mut normal_status = entry;
        normal_status.tooltip = Some(StatusbarTooltip::Content(TooltipContent::Text("Normal status".to_owned())));
        item.update(normal_status);

        let shown = shown.borrow();
        let callback = shown.first().and_then(Clone::clone);
        let callback_is_on_did_show = callback.as_ref().is_some_and(|callback| Rc::ptr_eq(callback, &on_did_show));
        let restored_options_is_undefined = shown.get(1).is_some_and(Option::is_none);
        assert_eq!((callback_is_on_did_show, restored_options_is_undefined), (true, true));
    });
}

#[test]
fn initializes_the_inactive_background_before_receiving_focus_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let colors = [(STATUS_BAR_BACKGROUND, "#111111"), (STATUS_BAR_INACTIVE_BACKGROUND, "#222222")];
    let create_focus_test_part = |host_service: Rc<TestHostService>, window_has_focus: bool| {
        let fixture = create_part(&guard, theme(&colors), TestContextService::new(None, None), guard.add_rc(host_service));
        fixture.window_has_focus.set(window_has_focus);
        let container = ViewElement::new();
        fixture.part.create(&container);
        (fixture, container)
    };

    let host_service = TestHostService::new();
    host_service.set_focus(false);
    let (_unfocused_host_part, unfocused_host) = create_focus_test_part(host_service, false);
    let (_unfocused_window_part, unfocused_window) = create_focus_test_part(TestHostService::new(), false);

    assert_eq!(
        (background(&unfocused_host), background(&unfocused_window)),
        ("rgb(34, 34, 34)".to_owned(), "rgb(34, 34, 34)".to_owned())
    );
}

#[test]
fn keeps_window_specific_backgrounds_when_the_host_regains_focus() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host_service = guard.add_rc(TestHostService::new());
    let colors = [(STATUS_BAR_BACKGROUND, "#111111"), (STATUS_BAR_INACTIVE_BACKGROUND, "#222222")];
    let fixture = create_part(&guard, theme(&colors), TestContextService::new(None, None), host_service.clone());
    fixture.window_has_focus.set(true);
    let container = ViewElement::new();
    fixture.part.create(&container);
    let backgrounds = || background(&container);
    let initially = backgrounds();

    host_service.set_focus(false);
    let blurred = backgrounds();
    host_service.set_focus(true);
    let returned_to_main = backgrounds();

    host_service.active_window.fire(MAIN_WINDOW.0 + 1);
    let switched_to_auxiliary = backgrounds();
    host_service.set_focus(false);
    host_service.set_focus(true);
    let returned_to_auxiliary = backgrounds();
    host_service.active_window.fire(MAIN_WINDOW.0);
    let switched_back_to_main = backgrounds();

    assert_eq!(
        (initially, blurred, returned_to_main, switched_to_auxiliary, returned_to_auxiliary, switched_back_to_main),
        (
            "rgb(17, 17, 17)".to_owned(),
            "rgb(34, 34, 34)".to_owned(),
            "rgb(17, 17, 17)".to_owned(),
            "rgb(34, 34, 34)".to_owned(),
            "rgb(34, 34, 34)".to_owned(),
            "rgb(17, 17, 17)".to_owned(),
        )
    );
}

#[test]
fn configuration_changes_update_styles_only_after_the_part_is_created() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host_service = guard.add_rc(TestHostService::new());
    let fixture = create_part(&guard, TestThemeService::new_default(), TestContextService::new(None, None), host_service);

    fire_configuration_change(&fixture.configuration_service, &[LayoutSettings::ModernUi.as_str()]);
    let before_create = fixture.update_styles_calls.get();
    fixture.part.create(&ViewElement::new());
    let after_create = fixture.update_styles_calls.get();
    fire_configuration_change(&fixture.configuration_service, &["unrelated.setting"]);
    let after_unrelated_change = fixture.update_styles_calls.get();
    fire_configuration_change(&fixture.configuration_service, &[LayoutSettings::ModernUi.as_str()]);

    assert_eq!(
        (before_create, after_create, after_unrelated_change, fixture.update_styles_calls.get()),
        (0, 1, 1, 2)
    );
}

#[test]
fn modern_ui_reserves_compact_vertical_status_bar_padding() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    set_floating_panels_enabled(false);
    set_modern_ui_compact(false);
    let host_service = guard.add_rc(TestHostService::new());
    let fixture = create_part(&guard, TestThemeService::new_default(), TestContextService::new(None, None), host_service);
    let part = &fixture.part;

    let default_constraints = (part.minimum_height(), part.maximum_height());
    set_floating_panels_enabled(true);
    let modern_ui_constraints = (part.minimum_height(), part.maximum_height());
    set_modern_ui_compact(true);
    let compact_modern_ui_constraints = (part.minimum_height(), part.maximum_height());
    set_floating_panels_enabled(false);
    set_modern_ui_compact(false);

    assert_eq!((default_constraints, modern_ui_constraints, compact_modern_ui_constraints), ((22.0, 22.0), (28.0, 28.0), (26.0, 26.0)));
}

#[test]
fn uses_the_inactive_background_when_the_window_loses_focus() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host_service = guard.add_rc(TestHostService::new());
    let theme_service = theme(&[
        (STATUS_BAR_BACKGROUND, "#111111"),
        (STATUS_BAR_INACTIVE_BACKGROUND, "#222222"),
        (STATUS_BAR_NO_FOLDER_BACKGROUND, "#333333"),
    ]);
    let fixture = create_part(&guard, theme_service.clone(), TestContextService::new(None, None), host_service.clone());
    let container = ViewElement::new();
    fixture.part.create(&container);
    let active_background = background(&container);

    host_service.set_focus(false);
    let inactive_background = background(&container);

    host_service.set_focus(true);
    let restored_background = background(&container);

    theme_service.set_theme(TestColorTheme::new(&[(STATUS_BAR_BACKGROUND, "#333333"), (STATUS_BAR_NO_FOLDER_BACKGROUND, "#444444")]));
    host_service.set_focus(false);
    let fallback_background = background(&container);

    assert_eq!(
        (active_background, inactive_background, restored_background, fallback_background),
        ("rgb(17, 17, 17)".to_owned(), "rgb(34, 34, 34)".to_owned(), "rgb(17, 17, 17)".to_owned(), "rgb(51, 51, 51)".to_owned())
    );
}

#[test]
fn keeps_state_backgrounds_when_the_window_is_inactive() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Not imported from the debug contribution to avoid depending on a higher layer here.
    let debugging_background = "statusBar.debuggingBackground";
    let colors = [
        (STATUS_BAR_BACKGROUND, "#111111"),
        (STATUS_BAR_INACTIVE_BACKGROUND, "#222222"),
        (STATUS_BAR_NO_FOLDER_BACKGROUND, "#333333"),
        (debugging_background, "#444444"),
    ];

    let create_part_with = |context_service: Rc<TestContextService>| {
        let host_service = guard.add_rc(TestHostService::new());
        let fixture = create_part(&guard, theme(&colors), context_service, host_service.clone());
        let container = ViewElement::new();
        fixture.part.create(&container);
        (fixture, host_service, container)
    };

    // A style override (as the debugger registers) must survive losing focus.
    let (debugging, debugging_host, debugging_container) = create_part_with(TestContextService::new(None, None));
    let debugging_override = debugging.part.override_style(Rc::new(StatusbarStyleOverride { priority: 10.0, background: Some(debugging_background.to_owned()), ..StatusbarStyleOverride::default() }));
    debugging_host.set_focus(false);
    let while_debugging = background(&debugging_container);
    debugging_override.dispose();

    // The no folder background communicates state too and must survive losing focus.
    let context_service = TestContextService::new(None, None);
    context_service.stub_workbench_state(Some(WorkbenchState::Empty));
    let (_empty, empty_host, empty_container) = create_part_with(context_service);
    empty_host.set_focus(false);
    let while_empty = background(&empty_container);

    assert_eq!((while_debugging, while_empty), ("rgb(68, 68, 68)".to_owned(), "rgb(51, 51, 51)".to_owned()));
}
