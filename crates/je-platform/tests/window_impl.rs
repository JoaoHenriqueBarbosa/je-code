//! Porte de `src/vs/platform/windows/test/electron-main/windowImpl.test.ts` (suíte `BaseWindow - window controls
//! overlay`, na ordem do original: quatro janelas sem overlay, dois formatos de overlay, a capacidade preservada
//! (desabilitada e habilitada) e o recuo horizontal fixo do macOS).
//!
//! Divergências:
//! - O `sinon.spy(window, 'updateWindowControls')` não existe: o que o original confere é que `updateWindowControls`
//!   foi chamada (ou não) com `{ height: 35 }`. A chamada sempre grava `windowControlHeight` no estado e, com overlay,
//!   chama `setTitleBarOverlay` (Windows e Linux) ou `setWindowButtonPosition` (macOS); os casos conferem esses
//!   efeitos, que só a chamada produz.
//! - O `TestWindow` (que só expõe o `setWin` e define `id = 1` e `matches`) é a própria [`BaseWindow`], com `id` 1: o
//!   `set_win` já é público.
//! - O `BrowserWindow` do `upcastPartial` é o [`StubNativeWindow`], que guarda as chamadas de `setTitleBarOverlay` e
//!   `setWindowButtonPosition` (o `sinon.stub()` e o `resetHistory`). O `electron` é o [`StubHost`].
//! - `configurationService.setUserConfiguration(...)` do `TestConfigurationService` é síncrono, então os casos não são
//!   assíncronos. `InMemoryTestStateMainService` é o [`InMemoryStateService`], local (como no teste do `BackupMainService`).
//! - `isMacintosh ? test.skip : test` é `#[cfg_attr(target_os = "macos", ignore)]`, e o inverso `ignore` fora do macOS.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, j};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::node::windows_version::os_release;
use je_base::path::{FixedEnvironment, Platform};
use je_base::promise::Promise;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::environment::environment_main_service::EnvironmentMainService;
use je_platform::environment::environment_service::{AbstractNativeEnvironmentService, NativeEnvironmentPaths};
use je_platform::environment::NativeParsedArgs;
use je_platform::log::NullLogService;
use je_platform::product::ProductService;
use je_platform::state::{StateReadService, StateService};
use je_platform::window::electron_main::{Display, ElectronHost, Point, Rectangle};
use je_platform::window::{DEFAULT_CUSTOM_TITLEBAR_HEIGHT, IPoint, get_macos_window_controls_position};
use je_platform::windows::window_impl::{
    BaseWindow, BaseWindowHost, BrowserWindowOptions, DockBadge, DockBadgeManager, NativeBrowserWindow, NativeWindowEvent, TitleBarOverlay, TitleBarOverlayOption, WindowControlsUpdate,
};

const WINDOW_CONTROL_HEIGHT_KEY: &str = "windowControlHeight";

/// `InMemoryTestStateMainService` de `workbenchTestServices.ts`.
#[derive(Default)]
struct InMemoryStateService {
    data: RefCell<HashMap<String, Dynamic>>,
}

impl StateReadService for InMemoryStateService {
    fn get_item(&self, key: &str, _default_value: Dynamic) -> Dynamic {
        self.data.borrow().get(key).cloned().unwrap_or(Dynamic::Undefined)
    }
}

impl StateService for InMemoryStateService {
    fn set_item(&self, key: &str, data: Dynamic) {
        self.data.borrow_mut().insert(key.to_string(), data);
    }

    fn set_items(&self, items: &[(String, Dynamic)]) {
        for (key, data) in items {
            self.data.borrow_mut().insert(key.clone(), data.clone());
        }
    }

    fn remove_item(&self, key: &str) {
        self.data.borrow_mut().remove(key);
    }

    fn close(&self) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O que o original guarda nos `sinon.stub()` de `setTitleBarOverlay` e `setWindowButtonPosition`.
#[derive(Clone, Debug, PartialEq)]
enum NativeCall {
    SetTitleBarOverlay(TitleBarOverlay),
    SetWindowButtonPosition(Option<IPoint>),
}

/// O `upcastPartial<BrowserWindow>({ on, removeListener, setSheetOffset, setWindowButtonPosition, setTitleBarOverlay })`:
/// só esses membros existem, o resto não é chamado pelos casos.
#[derive(Default)]
struct StubNativeWindow {
    calls: RefCell<Vec<NativeCall>>,
}

impl StubNativeWindow {
    /// `stub.resetHistory()`.
    fn reset_history(&self) {
        self.calls.borrow_mut().clear();
    }

    fn calls(&self) -> Vec<NativeCall> {
        self.calls.borrow().clone()
    }
}

impl NativeBrowserWindow for StubNativeWindow {
    fn events(&self) -> Event<NativeWindowEvent> {
        Event::none() // `on: sinon.stub()`
    }

    fn get_position(&self) -> (f64, f64) {
        unimplemented!("fora do upcastPartial")
    }

    fn get_size(&self) -> (f64, f64) {
        unimplemented!("fora do upcastPartial")
    }

    fn set_bounds(&self, _bounds: Rectangle) {
        unimplemented!("fora do upcastPartial")
    }

    fn maximize(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn show(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn restore(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn focus(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn is_minimized(&self) -> bool {
        unimplemented!("fora do upcastPartial")
    }

    fn is_destroyed(&self) -> bool {
        unimplemented!("fora do upcastPartial")
    }

    fn is_full_screen(&self) -> bool {
        unimplemented!("fora do upcastPartial")
    }

    fn is_simple_full_screen(&self) -> bool {
        unimplemented!("fora do upcastPartial")
    }

    fn set_full_screen(&self, _full_screen: bool) {
        unimplemented!("fora do upcastPartial")
    }

    fn set_simple_full_screen(&self, _full_screen: bool) {
        unimplemented!("fora do upcastPartial")
    }

    fn set_sheet_offset(&self, _offset: f64) {}

    fn set_title_bar_overlay(&self, overlay: TitleBarOverlay) {
        self.calls.borrow_mut().push(NativeCall::SetTitleBarOverlay(overlay));
    }

    fn set_window_button_position(&self, position: Option<IPoint>) {
        self.calls.borrow_mut().push(NativeCall::SetWindowButtonPosition(position));
    }

    fn set_overlay_icon(&self, _icon_data_url: Option<&str>, _description: &str) {
        unimplemented!("fora do upcastPartial")
    }

    fn flash_frame(&self, _flash: bool) {
        unimplemented!("fora do upcastPartial")
    }

    fn open_dev_tools(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn focus_web_contents(&self) {
        unimplemented!("fora do upcastPartial")
    }

    fn set_represented_filename(&self, _filename: &str) {
        unimplemented!("fora do upcastPartial")
    }

    fn get_represented_filename(&self) -> String {
        unimplemented!("fora do upcastPartial")
    }

    fn set_document_edited(&self, _edited: bool) {
        unimplemented!("fora do upcastPartial")
    }

    fn is_document_edited(&self) -> bool {
        unimplemented!("fora do upcastPartial")
    }
}

/// O `electron` (`app` e `screen`) que nenhum dos casos exercita além do selo do dock.
struct StubHost;

impl ElectronHost for StubHost {
    fn on_browser_window_blur(&self) -> Event<()> {
        Event::none()
    }

    fn get_all_displays(&self) -> Vec<Display> {
        Vec::new()
    }

    fn get_primary_display(&self) -> Display {
        Display::default()
    }

    fn get_cursor_screen_point(&self) -> Point {
        Point::default()
    }

    fn get_display_nearest_point(&self, _point: Point) -> Display {
        Display::default()
    }

    fn get_display_matching(&self, _rect: Rectangle) -> Display {
        Display::default()
    }
}

impl BaseWindowHost for StubHost {
    fn set_badge_count(&self, _badge: DockBadge) {}

    fn is_app_hidden(&self) -> bool {
        false
    }

    fn focus_app_stealing(&self) {}

    fn bounce_dock_informational(&self) {}

    fn screen_to_dip_point(&self, point: Point) -> Point {
        point
    }

    fn on_display_added(&self) -> Event<Display> {
        Event::none()
    }

    fn work_area(&self, display: &Display) -> Rectangle {
        display.bounds
    }

    fn windows_count_excluding_offscreen(&self) -> usize {
        0
    }
}

/// O `upcastPartial<IEnvironmentMainService>({ args: { _: [] } })`: o serviço do processo principal sobre argumentos vazios.
fn environment_main_service() -> Rc<EnvironmentMainService> {
    let paths = NativeEnvironmentPaths { user_data_dir: "/user-data".to_owned(), home_dir: "/home".to_owned(), tmp_dir: "/tmp".to_owned(), app_root: "/app".to_owned() };
    let base = AbstractNativeEnvironmentService::new(NativeParsedArgs::default(), paths, ProductService::default(), Rc::new(FixedEnvironment::new(Platform::Posix, "/")));
    Rc::new(EnvironmentMainService::new(base, Vec::new()))
}

struct Fixture {
    window: Rc<BaseWindow>,
    native_window: Rc<StubNativeWindow>,
    configuration_service: Rc<TestConfigurationService>,
    state_service: Rc<InMemoryStateService>,
}

/// `createWindow()`.
fn create_window(store: &common::LeakGuard) -> Fixture {
    let configuration_service = TestConfigurationService::new(Some(j("{'window': {'titleBarStyle': 'custom'}}")));
    store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
    let state_service = Rc::new(InMemoryStateService::default());
    let host = Rc::new(StubHost);
    let window = store.add_rc(BaseWindow::new(
        1,
        configuration_service.clone() as Rc<dyn IConfigurationService>,
        state_service.clone(),
        environment_main_service(),
        NullLogService::shared(),
        host.clone(),
        DockBadgeManager::new(host),
    ));
    Fixture { window, native_window: Rc::new(StubNativeWindow::default()), configuration_service, state_service }
}

fn overlay_options(overlay: TitleBarOverlayOption) -> BrowserWindowOptions {
    BrowserWindowOptions { title_bar_style: Some("hidden".to_owned()), title_bar_overlay: Some(overlay), ..Default::default() }
}

/// O efeito de `updateWindowControls({ height: DEFAULT_CUSTOM_TITLEBAR_HEIGHT })` na janela nativa.
fn expected_default_height_calls() -> Vec<NativeCall> {
    if cfg!(target_os = "macos") {
        vec![NativeCall::SetWindowButtonPosition(get_macos_window_controls_position(f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT), &os_release(), None))]
    } else {
        vec![NativeCall::SetTitleBarOverlay(TitleBarOverlay { color: None, symbol_color: None, height: Some(f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT) - 1.0) })]
    }
}

/// `assert.deepStrictEqual(updateWindowControls.args, [])`.
#[track_caller]
fn assert_controls_not_initialized(fixture: &Fixture) {
    assert_eq!(fixture.state_service.get_item(WINDOW_CONTROL_HEIGHT_KEY, Dynamic::Undefined), Dynamic::Undefined);
    assert_eq!(fixture.native_window.calls(), Vec::<NativeCall>::new());
}

/// `assert.deepStrictEqual(updateWindowControls.args, [[{ height: DEFAULT_CUSTOM_TITLEBAR_HEIGHT }]])`.
#[track_caller]
fn assert_controls_initialized_with_default_height(fixture: &Fixture) {
    assert_eq!(fixture.state_service.get_item(WINDOW_CONTROL_HEIGHT_KEY, Dynamic::Undefined), Dynamic::Number(f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT)));
    assert_eq!(fixture.native_window.calls(), expected_default_height_calls());
}

fn does_not_initialize_controls(options: Option<BrowserWindowOptions>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_window(&guard);

    fixture.window.set_win(fixture.native_window.clone(), options.as_ref());

    assert_controls_not_initialized(&fixture);
}

#[test]
fn does_not_initialize_controls_for_unknown_constructor_options() {
    does_not_initialize_controls(None);
}

#[test]
fn does_not_initialize_controls_for_native_titlebar() {
    does_not_initialize_controls(Some(BrowserWindowOptions::default()));
}

#[test]
fn does_not_initialize_controls_for_frameless() {
    does_not_initialize_controls(Some(BrowserWindowOptions { frame: Some(false), ..Default::default() }));
}

#[test]
fn does_not_initialize_controls_for_explicitly_disabled_overlay() {
    does_not_initialize_controls(Some(overlay_options(TitleBarOverlayOption::Flag(false))));
}

fn initializes_controls(title_bar_overlay: TitleBarOverlayOption) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_window(&guard);

    fixture.window.set_win(fixture.native_window.clone(), Some(&overlay_options(title_bar_overlay)));

    assert_controls_initialized_with_default_height(&fixture);
}

#[test]
fn initializes_controls_for_boolean_overlay_options() {
    initializes_controls(TitleBarOverlayOption::Flag(true));
}

#[test]
fn initializes_controls_for_object_overlay_options() {
    initializes_controls(TitleBarOverlayOption::Config { height: Some(29.0) });
}

fn preserves_creation_time_overlay_capability(enabled: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_window(&guard);
    fixture.window.set_win(fixture.native_window.clone(), Some(&overlay_options(TitleBarOverlayOption::Flag(enabled))));
    fixture.native_window.reset_history();

    let controls_style = if enabled { "custom" } else { "native" };
    fixture.configuration_service.set_user_configuration("window", j(&format!("{{'titleBarStyle': 'custom', 'controlsStyle': '{controls_style}'}}")), None);
    fixture.window.update_window_controls(&WindowControlsUpdate {
        height: Some(40.0),
        background_color: Some("#ffffff".to_owned()),
        foreground_color: Some("#000000".to_owned()),
        ..Default::default()
    });

    let expected = if enabled {
        vec![NativeCall::SetTitleBarOverlay(TitleBarOverlay { color: Some("#ffffff".to_owned()), symbol_color: Some("#000000".to_owned()), height: Some(39.0) })]
    } else {
        Vec::new()
    };
    assert_eq!(fixture.native_window.calls(), expected);
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn preserves_the_creation_time_overlay_capability_false_after_settings_change() {
    preserves_creation_time_overlay_capability(false);
}

#[test]
#[cfg_attr(target_os = "macos", ignore)]
fn preserves_the_creation_time_overlay_capability_true_after_settings_change() {
    preserves_creation_time_overlay_capability(true);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
fn honors_a_fixed_horizontal_inset_when_the_title_bar_height_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_window(&guard);
    fixture.window.set_win(fixture.native_window.clone(), Some(&BrowserWindowOptions { title_bar_style: Some("hidden".to_owned()), ..Default::default() }));
    fixture.native_window.reset_history();
    let horizontal_inset = get_macos_window_controls_position(f64::from(DEFAULT_CUSTOM_TITLEBAR_HEIGHT), &os_release(), None).expect("posição").x;

    for height in [35.0, 44.0, 35.0] {
        fixture.window.update_window_controls(&WindowControlsUpdate { height: Some(height), horizontal_inset: Some(horizontal_inset), ..Default::default() });
    }

    let expected: Vec<NativeCall> = [35.0, 44.0, 35.0]
        .into_iter()
        .map(|height| {
            NativeCall::SetWindowButtonPosition(Some(IPoint { x: horizontal_inset, y: get_macos_window_controls_position(height, &os_release(), None).expect("posição").y }))
        })
        .collect();
    assert_eq!(fixture.native_window.calls(), expected);
}
