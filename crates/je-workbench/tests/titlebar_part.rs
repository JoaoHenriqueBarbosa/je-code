//! Porte de `src/vs/workbench/test/browser/parts/titlebar/titlebarPart.test.ts` (suítes `TitlebarPart colors` e `Workbench - Titlebar
//! Part`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso; `store.add(...)` vira
//!   `guard.add_rc` / `guard.add`.
//! - **`TestTitlebarPart`.** O `createContentArea` sobrescrito (`this.element = parent; return parent;`) é o
//!   `TitlebarContentArea::ElementOnly`; `installTestMenubar(leftContent)` é `set_left_content` seguido de `install_menubar`.
//! - **`mock<CodeWindow>()`** com `vscodeWindowId` e `document.hasFocus()` é a `TargetWindow` com o `Cell` `focused`.
//! - **`workbenchInstantiationService(undefined, store)`** são os serviços reais de configuração e de chaves de contexto, o
//!   `TestLayoutService` e os dublês de `part_doubles.rs`. O `stubInstance(WindowTitle, { dispose() { } })` é o
//!   [`TitlebarWindowTitle`] que não faz nada, e o `stubInstance(CustomMenubarControl, {...})` é o [`StubMenubar`].
//! - **`root.style.getPropertyValue('--modern-ui-shell-background')`.** O `getContainer` que devolve o `root` é o contêiner da janela
//!   do `TestLayoutService`, e a propriedade é a que o [`RootStyles`] (o `ContainerStyleHost`) guarda.
//! - **`getComputedStyle`.** O caso 2 resolve a cor dos ícones do `.action-toolbar-container` com
//!   `titlebar_action_toolbar_foreground`: as variáveis `--vscode-titleBar-activeForeground` e `--vscode-titleBar-inactiveForeground` do
//!   `workbench.style.setProperty` são as cores do tema do caso, e a `color` do `.monaco-workbench` é a herdada. O caso 3 resolve a
//!   `opacity` do `.titlebar-container` com `titlebar_container_opacity`, que lê as classes `inactive` e `menubar-focused` da parte.
//! - **`Object.create(BrowserTitlebarPart.prototype)` com `Reflect.set`.** O método privado `updateTitleBarToolBarOverflow` só lê
//!   `rootContainer`, `centerAdjacentToolBarElement` e `updateToolBarElement`; é a função `update_title_bar_tool_bar_overflow` sobre
//!   os três elementos. `createMeasuredElement` é o `ViewElement` com as fontes de `clientWidth` e `scrollWidth`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "support/part_doubles.rs"]
mod part_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, LeakGuard};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::platform::Platform;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::view_element::ViewElement;
use je_theme::color::Color;
use je_workbench::browser::parts::part_support::ContainerStyleHost;
use je_workbench::browser::parts::titlebar::titlebar_part::{
    ActionToolbarMenusUpdate, MODERN_UI_INACTIVE_SHELL_BACKGROUND, MODERN_UI_SHELL_BACKGROUND, MODERN_UI_SHELL_BACKGROUND_PROPERTY, TITLE_BAR_ACTIVE_BACKGROUND,
    TITLE_BAR_ACTIVE_FOREGROUND, TITLE_BAR_INACTIVE_BACKGROUND, TITLE_BAR_INACTIVE_FOREGROUND, TitleProperties, TitleVariable, TitlebarContentArea, TitlebarContextMenu,
    TitlebarContextMenuEvent, TitlebarEditorGroups, TitlebarMenubar, TitlebarPart, TitlebarPartDeps, TitlebarPartView, TitlebarToolbars, TitlebarWindowTitle,
    titlebar_action_toolbar_foreground, titlebar_container_opacity, update_title_bar_tool_bar_overflow,
};
use je_workbench::browser::parts::part_support::TargetWindow;
use je_workbench::contrib::modern_ui::resolved_style::css_rgb;
use je_workbench::services::editor::editor_groups_service::{EditorPartOptions, EditorPartOptionsChangeEvent};
use je_workbench::services::layout::layout_service::{IWorkbenchLayoutService, LayoutContainerId, MAIN_WINDOW, TitleBarHost, WindowId};
use notification_doubles::TestLayoutService;
use part_doubles::TestHostService;
use test_color_theme_service::{TestColorTheme, TestThemeService};

// region: dublês

/// O `style` do contêiner da janela (o `root` do original): as propriedades personalizadas que a parte escreve.
#[derive(Default)]
struct RootStyles {
    properties: RefCell<Vec<(String, String)>>,
}

impl RootStyles {
    /// `root.style.getPropertyValue(name)`: o texto vazio para a propriedade que nunca foi escrita.
    fn get_property_value(&self, name: &str) -> String {
        self.properties.borrow().iter().find(|(candidate, _)| candidate == name).map(|(_, value)| value.clone()).unwrap_or_default()
    }
}

impl ContainerStyleHost for RootStyles {
    fn set_property(&self, _container: LayoutContainerId, name: &str, value: &str) {
        let mut properties = self.properties.borrow_mut();
        properties.retain(|(candidate, _)| candidate != name);
        if !value.is_empty() {
            properties.push((name.to_owned(), value.to_owned()));
        }
    }
}

/// O `mock<IEditorGroupsContainer>()` com `onDidChangeEditorPartOptions = Event.None`.
struct NoEditorGroups;

impl TitlebarEditorGroups for NoEditorGroups {
    fn on_did_change_editor_part_options(&self) -> Event<EditorPartOptionsChangeEvent> {
        Event::none()
    }

    fn part_options(&self) -> EditorPartOptions {
        unreachable!("TypeError: Cannot read properties of undefined (reading 'editorActionsLocation')")
    }
}

/// O `WindowTitle` de `stubInstance(WindowTitle, { dispose() { } })`.
struct StubWindowTitle;

impl IDisposable for StubWindowTitle {
    fn dispose(&self) {}
}

impl TitlebarWindowTitle for StubWindowTitle {
    fn value(&self) -> String {
        String::new()
    }
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }
    fn update_properties(&self, _properties: &TitleProperties) {}
    fn register_variables(&self, _variables: &[TitleVariable]) {}
}

/// O `CustomMenubarControl` de `stubInstance(...)`: `onVisibilityChange: Event.None`, `onFocusStateChange: focusStateEmitter.event`,
/// `create(parent) { return parent; }` e `dispose() { }`.
struct StubMenubar {
    focus_state: Rc<Emitter<bool>>,
}

impl IDisposable for StubMenubar {
    fn dispose(&self) {}
}

impl TitlebarMenubar for StubMenubar {
    fn on_visibility_change(&self) -> Event<bool> {
        Event::none()
    }
    fn on_focus_state_change(&self) -> Event<bool> {
        self.focus_state.event()
    }
    fn create(&self, _parent: &Rc<ViewElement>) {}
    fn layout(&self, _dimension: Dimension) {}
    fn toggle_focus(&self) {}
}

/// Com o `ElementOnly` nenhuma barra de ferramentas é criada.
struct NoToolbars;

impl TitlebarToolbars for NoToolbars {
    fn create_center_adjacent_toolbar(&self, _element: &Rc<ViewElement>, _on_did_change_menu_items: Rc<dyn Fn()>) -> je_base::lifecycle::DisposableRef {
        unreachable!("o caso não cria barras de ferramentas")
    }
    fn create_update_toolbar(&self, _element: &Rc<ViewElement>, _on_did_change_menu_items: Rc<dyn Fn()>) -> je_base::lifecycle::DisposableRef {
        unreachable!("o caso não cria barras de ferramentas")
    }
    fn create_action_toolbar(&self, _element: &Rc<ViewElement>) {
        unreachable!("o caso não cria barras de ferramentas")
    }
    fn create_action_toolbar_menus(&self, _update: ActionToolbarMenusUpdate) {
        unreachable!("o caso não cria barras de ferramentas")
    }
    fn create_command_center(&self, _title: &Rc<ViewElement>) -> je_base::lifecycle::DisposableRef {
        unreachable!("o caso não cria o centro de comandos")
    }
}

/// O que a parte faz com o DOM: nada além do estado dos `ViewElement`s.
struct NoTitlebarView;

impl TitlebarPartView for NoTitlebarView {
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn prepend(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn remove(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
    }
    fn set_attribute(&self, _element: &Rc<ViewElement>, _name: &str, _value: &str) {}
    fn set_text_content(&self, _element: &Rc<ViewElement>, _text: &str) {}
    fn reset(&self, _element: &Rc<ViewElement>) {}
    fn zoom_factor(&self) -> f64 {
        1.0
    }
    fn wco_titlebar_area_height(&self) -> Option<f64> {
        None
    }
    fn is_rtl_locale(&self) -> bool {
        false
    }
    fn add_context_menu_listener(
        &self,
        _root_container: &Rc<ViewElement>,
        _title: &Rc<ViewElement>,
        _handler: Rc<dyn Fn(TitlebarContextMenu, &TitlebarContextMenuEvent)>,
    ) -> je_base::lifecycle::DisposableRef {
        je_base::lifecycle::none()
    }
    fn focus_first_tabbable(&self, _element: &Rc<ViewElement>) {}
    fn show_context_menu(&self, _menu: TitlebarContextMenu, _event: &TitlebarContextMenuEvent) {}
    fn schedule_tool_bar_overflow_update(&self, _root_container: &Rc<ViewElement>, _run: Rc<dyn Fn()>) {}
}

/// O `TitleBarHost` do `workbenchInstantiationService`: uma barra customizada com o menu clássico.
struct TitleBarHostStub;

impl TitleBarHost for TitleBarHostStub {
    fn is_web(&self) -> bool {
        false
    }
    fn is_macintosh(&self) -> bool {
        false
    }
    fn is_native(&self) -> bool {
        true
    }
    fn has_custom_titlebar(&self) -> bool {
        true
    }
    fn has_native_titlebar(&self) -> bool {
        false
    }
    fn has_native_menu(&self) -> bool {
        false
    }
    fn is_fullscreen(&self, _window: WindowId) -> bool {
        false
    }
    fn is_wco_enabled(&self) -> bool {
        false
    }
    fn is_auxiliary_window(&self, _window: WindowId) -> bool {
        false
    }
    fn get_menu_bar_visibility(&self) -> Option<String> {
        Some("classic".to_owned())
    }
}

// endregion

/// O `instantiationService.createInstance(TestTitlebarPart, Parts.TITLEBAR_PART, targetWindow, groups)` dos casos.
fn create_titlebar_part(
    guard: &LeakGuard,
    window: TargetWindow,
    host_service: Rc<TestHostService>,
    theme_service: Rc<TestThemeService>,
    container_styles: Rc<RootStyles>,
    menubar_focus_state: Option<Rc<Emitter<bool>>>,
) -> Rc<TitlebarPart> {
    let configuration_service = TestConfigurationService::new(None);
    guard.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let configuration: Rc<dyn IConfigurationService> = configuration_service;
    let context_key_service = guard.add_rc(ContextKeyService::new(configuration.clone()));
    guard.add_rc(host_service.clone());
    guard.add_rc(theme_service.clone());
    let layout_service: Rc<dyn IWorkbenchLayoutService> = Rc::new(TestLayoutService);
    let theme: Rc<dyn IThemeService> = theme_service;

    guard.add_rc(TitlebarPart::new_main(TitlebarPartDeps {
        window,
        editor_groups: Rc::new(NoEditorGroups),
        configuration_service: configuration,
        theme_service: theme,
        layout_service,
        context_key_service,
        host_service,
        title_bar_host: Rc::new(TitleBarHostStub),
        platform: Platform::detect(),
        view: Rc::new(NoTitlebarView),
        container_styles,
        menubar_factory: Rc::new(move || {
            let focus_state = menubar_focus_state.clone().expect("o caso não instala a barra de menus");
            Rc::new(StubMenubar { focus_state })
        }),
        toolbars: Rc::new(NoToolbars),
        window_title: Rc::new(StubWindowTitle),
        content_area: TitlebarContentArea::ElementOnly,
        main_titlebar: None,
    }))
}

#[test]
fn resolves_shell_and_title_colors_from_the_target_window_focus() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let focused = Rc::new(Cell::new(false));
    let target_window = TargetWindow { id: MAIN_WINDOW, has_focus: Rc::new({
        let focused = focused.clone();
        move || focused.get()
    }) };
    let host_service = TestHostService::new();
    let theme_service = TestThemeService::new(TestColorTheme::new(&[
        (TITLE_BAR_ACTIVE_BACKGROUND, "#112233"),
        (TITLE_BAR_INACTIVE_BACKGROUND, "#223344"),
        (MODERN_UI_SHELL_BACKGROUND, "#334455"),
        (MODERN_UI_INACTIVE_SHELL_BACKGROUND, "#445566"),
    ]));
    let root = Rc::new(RootStyles::default());
    let part = create_titlebar_part(&guard, target_window, host_service.clone(), theme_service.clone(), root.clone(), None);
    let container = ViewElement::new();
    part.create(&container);
    let colors = || (container.style_get_property("background-color").unwrap_or_default(), root.get_property_value(MODERN_UI_SHELL_BACKGROUND_PROPERTY));
    let initially_inactive = colors();
    focused.set(true);
    host_service.active_window.fire(MAIN_WINDOW.0);
    let active = colors();
    focused.set(false);
    host_service.active_window.fire(MAIN_WINDOW.0 + 1);
    host_service.set_focus(false);
    host_service.set_focus(true);
    let another_window_active = colors();

    theme_service.set_theme(TestColorTheme::new(&[(TITLE_BAR_ACTIVE_BACKGROUND, "#112233"), (MODERN_UI_SHELL_BACKGROUND, "#334455")]));
    let missing_inactive_colors = colors();

    let expected = |title: &str, shell: &str| (title.to_owned(), shell.to_owned());
    assert_eq!(
        (initially_inactive, active, another_window_active, missing_inactive_colors),
        (
            expected("rgb(34, 51, 68)", "#445566"),
            expected("rgb(17, 34, 51)", "#334455"),
            expected("rgb(34, 51, 68)", "#445566"),
            expected("rgb(17, 34, 51)", "#334455"),
        )
    );
}

#[test]
fn uses_title_bar_foreground_colors_for_action_toolbar_icons() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // `workbench.style.color = '#abcdef'` e as variáveis `--vscode-titleBar-*Foreground` do `setProperty`.
    let theme = TestColorTheme::new(&[(TITLE_BAR_ACTIVE_FOREGROUND, "#112233"), (TITLE_BAR_INACTIVE_FOREGROUND, "#445566")]);
    let workbench_color = Color::from_hex("#abcdef");
    let titlebar = ViewElement::with_classes(&["part", "titlebar"]);

    let resolved = |titlebar: &ViewElement| titlebar_action_toolbar_foreground(theme.as_ref(), titlebar, Some(workbench_color.clone())).map(|color| css_rgb(&color));
    let active = resolved(&titlebar);
    titlebar.class_list_add("inactive");
    let inactive = resolved(&titlebar);

    assert_eq!((active, inactive), (Some("rgb(17, 34, 51)".to_owned()), Some("rgb(68, 85, 102)".to_owned())));
}

#[test]
fn updates_inactive_title_bar_opacity_from_menubar_focus_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let titlebar = ViewElement::with_classes(&["part", "titlebar"]);
    let titlebar_container = ViewElement::with_classes(&["titlebar-container"]);
    titlebar_container.set_parent(Some(titlebar.clone()));

    let focus_state_emitter: Rc<Emitter<bool>> = Emitter::new();
    guard.add_rc(focus_state_emitter.clone());
    let window = TargetWindow { id: MAIN_WINDOW, has_focus: Rc::new(|| false) };
    let part = create_titlebar_part(
        &guard,
        window,
        TestHostService::new(),
        TestThemeService::new_default(),
        Rc::new(RootStyles::default()),
        Some(focus_state_emitter.clone()),
    );

    part.create(&titlebar);
    titlebar.class_list_add("inactive");
    part.set_left_content(&titlebar_container);
    part.install_menubar();

    let state = || (titlebar.class_list_contains("menubar-focused"), titlebar_container_opacity(&titlebar));
    let initial = state();
    focus_state_emitter.fire(true);
    let focused = state();
    focus_state_emitter.fire(false);
    let blurred = state();
    focus_state_emitter.fire(true);
    let refocused = state();

    part.uninstall_menubar();
    let uninstalled = state();
    focus_state_emitter.fire(true);
    let after_uninstall_event = state();

    assert_eq!(
        (initial, focused, blurred, refocused, uninstalled, after_uninstall_event),
        ((false, 0.6), (true, 1.0), (false, 0.6), (true, 1.0), (false, 0.6), (false, 0.6))
    );
}

/// `createMeasuredElement(clientWidth, scrollWidth)`.
fn create_measured_element(client_width: impl Fn() -> f64 + 'static, scroll_width: impl Fn() -> f64 + 'static) -> Rc<ViewElement> {
    let element = ViewElement::new();
    element.set_client_width_source(Rc::new(client_width));
    element.set_scroll_width_source(Rc::new(scroll_width));
    element
}

/// `visibleWidth(element, width)`.
fn visible_width(element: &ViewElement, width: f64) -> f64 {
    if element.class_list_contains("overflowing") || element.class_list_contains("has-no-actions") { 0.0 } else { width }
}

#[test]
fn hides_optional_toolbar_groups_in_priority_order() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let root_client_width = Rc::new(Cell::new(130.0));
    let center_adjacent_tool_bar = ViewElement::new();
    let update_tool_bar = ViewElement::new();
    let root = create_measured_element(
        {
            let root_client_width = root_client_width.clone();
            move || root_client_width.get()
        },
        {
            let (center_adjacent_tool_bar, update_tool_bar) = (center_adjacent_tool_bar.clone(), update_tool_bar.clone());
            move || 100.0 + visible_width(&center_adjacent_tool_bar, 30.0) + visible_width(&update_tool_bar, 30.0)
        },
    );
    let overflowing = || [&center_adjacent_tool_bar, &update_tool_bar].map(|element| element.class_list_contains("overflowing"));

    update_title_bar_tool_bar_overflow(&root, Some(&center_adjacent_tool_bar), Some(&update_tool_bar));
    let center_hidden_first = overflowing();

    root_client_width.set(100.0);
    update_title_bar_tool_bar_overflow(&root, Some(&center_adjacent_tool_bar), Some(&update_tool_bar));
    let both_hidden = overflowing();

    root_client_width.set(160.0);
    update_title_bar_tool_bar_overflow(&root, Some(&center_adjacent_tool_bar), Some(&update_tool_bar));
    let both_visible = overflowing();

    assert_eq!((center_hidden_first, both_hidden, both_visible), ([true, false], [true, true], [false, false]));
}
