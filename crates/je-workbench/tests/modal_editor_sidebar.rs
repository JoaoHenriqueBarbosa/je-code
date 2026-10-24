//! Porte de `src/vs/workbench/test/browser/parts/editor/modalEditorSidebar.test.ts` (suíte `Modal Editor Sidebar`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestModalEditorSidebarHost` do original é um modelo mínimo que "imita" o `createSidebar` e o `updateOptions` do
//!   `ModalEditorPart` sem DOM. Aqui o estado e as decisões são os mesmos, e a aritmética de largura (a largura inicial, o
//!   redimensionamento, o reset, o `clampWidth` e a largura mínima do modal) são as funções de produção de
//!   `modal_editor_part`, em vez de uma cópia. As constantes `MODAL_MIN_WIDTH`, `MODAL_SIDEBAR_MIN_WIDTH` e
//!   `MODAL_SIDEBAR_DEFAULT_WIDTH` também são as de produção (o original repete as suas, com a padrão em 220 enquanto a
//!   produção usa 260; nenhum caso depende do valor, todos o comparam com a constante).
//! - `MockContextKeyService` é o `ContextKeyService` de `je-platform` sobre o `TestConfigurationService`, como nos demais
//!   testes portados. `{}` (o contêiner de mentira) é `Rc<()>` como `Rc<dyn Any>`.
//! - `class extends Disposable` com `this._register(...)` é o `Disposable` do campo `base`, e
//!   `disposables.add(host)` é o `add_rc` do guarda de vazamentos.
//! - `IModalEditorSidebar` é o `ModalEditorSidebar` do `je-platform`, cujo `render` é um `Rc<dyn Fn>`.
//! - `assert.deepStrictEqual` de objetos é a comparação de tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, to_disposable};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::opener::editor_types::{LayoutDimension, ModalEditorPartOptions, ModalEditorSidebar};
use je_workbench::browser::parts::editor::modal_editor_part::{
    MODAL_MIN_WIDTH, MODAL_SIDEBAR_DEFAULT_WIDTH, MODAL_SIDEBAR_MIN_WIDTH, clamped_sidebar_width, effective_min_width, initial_sidebar_width, reset_sidebar_width,
    resized_sidebar_width,
};

/// `TestModalEditorSidebarHost`.
struct TestModalEditorSidebarHost {
    base: Rc<Disposable>,
    on_did_resize: Rc<Emitter<()>>,
    on_did_layout: Rc<Emitter<LayoutDimension>>,
    content_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    context_key_service: Rc<ContextKeyService>,
    sidebar_width: Cell<f64>,
    has_sidebar: Cell<bool>,
    sidebar_visible: Cell<bool>,
    render_count: Cell<usize>,
    /// Container width the modal occupies (simulates container.clientWidth).
    container_width: Cell<f64>,
    /// Remembered sidebar width from previous modal session (mirrors editorPart.sidebarWidth).
    custom_width: Cell<Option<f64>>,
}

impl TestModalEditorSidebarHost {
    /// `constructor(customWidth?, sidebarHidden?)`.
    fn new(guard: &LeakGuard, custom_width: Option<f64>, sidebar_hidden: Option<bool>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_resize: Rc<Emitter<()>> = Emitter::new();
        let on_did_layout: Rc<Emitter<LayoutDimension>> = Emitter::new();
        let content_disposable: Rc<MutableDisposable<dyn IDisposable>> = MutableDisposable::new();
        let context_key_service = ContextKeyService::new(TestConfigurationService::new(None));
        base.register(on_did_resize.clone());
        base.register(on_did_layout.clone());
        base.register(content_disposable.clone());
        base.register(context_key_service.clone());
        guard.add_rc(Rc::new(Self {
            base,
            on_did_resize,
            on_did_layout,
            content_disposable,
            context_key_service,
            sidebar_width: Cell::new(MODAL_SIDEBAR_DEFAULT_WIDTH),
            has_sidebar: Cell::new(false),
            sidebar_visible: Cell::new(!sidebar_hidden.unwrap_or(false)),
            render_count: Cell::new(0),
            container_width: Cell::new(800.0),
            custom_width: Cell::new(custom_width),
        }))
    }

    fn on_did_resize(&self) -> Event<()> {
        self.on_did_resize.event()
    }

    fn sidebar_width(&self) -> f64 {
        self.sidebar_width.get()
    }

    fn has_sidebar(&self) -> bool {
        self.has_sidebar.get()
    }

    fn sidebar_visible(&self) -> bool {
        self.sidebar_visible.get()
    }

    fn render_count(&self) -> usize {
        self.render_count.get()
    }

    fn custom_width(&self) -> Option<f64> {
        self.custom_width.get()
    }

    fn set_container_width(&self, width: f64) {
        self.container_width.set(width);
    }

    // --- sidebar management (mirrors createSidebar / updateContent) ---------

    fn add_sidebar(&self, content: &ModalEditorSidebar) {
        self.has_sidebar.set(true);
        self.sidebar_width.set(initial_sidebar_width(self.custom_width.get()));
        self.render_content(content);
    }

    fn update_sidebar_content(&self, content: &ModalEditorSidebar) {
        self.content_disposable.clear();
        self.render_content(content);
    }

    fn remove_sidebar(&self) {
        self.has_sidebar.set(false);
        self.sidebar_width.set(0.0);
        self.content_disposable.clear();
    }

    fn toggle_sidebar_visible(&self) {
        self.sidebar_visible.set(!self.sidebar_visible.get());
        self.on_did_resize.fire(());
    }

    /// Returns actual width taking visibility into account (mirrors getWidth in controller).
    fn effective_sidebar_width(&self) -> f64 {
        if self.sidebar_visible.get() { self.sidebar_width.get() } else { 0.0 }
    }

    fn render_content(&self, content: &ModalEditorSidebar) {
        self.render_count.set(self.render_count.get() + 1);
        let render = content.render.as_ref().expect("o conteúdo da barra lateral tem render");
        let container: Rc<dyn Any> = Rc::new(()) /* stub container */;
        let context_key_service: Rc<dyn IContextKeyService> = self.context_key_service.clone();
        self.content_disposable.set(Some(render(container, self.on_did_layout.event(), context_key_service)));
    }

    // --- resize (mirrors sash logic) ----------------------------------------

    fn resize_sidebar(&self, delta: f64) {
        let width = resized_sidebar_width(self.sidebar_width.get(), delta, self.container_width.get());
        self.sidebar_width.set(width);
        self.custom_width.set(Some(width));
        self.on_did_resize.fire(());
    }

    fn reset_sidebar_width(&self) {
        self.sidebar_width.set(reset_sidebar_width(self.container_width.get()));
        self.custom_width.set(None);
        self.on_did_resize.fire(());
    }

    fn clamp_width(&self, modal_width: f64) {
        if let Some(width) = clamped_sidebar_width(self.sidebar_width.get(), modal_width) {
            self.sidebar_width.set(width);
            self.custom_width.set(None);
            self.on_did_resize.fire(());
        }
    }

    // --- min-size computation (mirrors create method) -----------------------

    fn effective_min_width(&self) -> f64 {
        effective_min_width(self.has_sidebar.get())
    }

    // --- option propagation (mirrors updateOptions behaviour) ---------------

    fn update_options(&self, options: &ModalEditorPartOptions) {
        if let Some(sidebar) = &options.sidebar {
            if !self.has_sidebar.get() {
                self.add_sidebar(sidebar);
            } else {
                self.update_sidebar_content(sidebar);
            }
        }
        // sidebar explicitly removed when key is absent and host has one: nothing to do
    }

    fn layout(&self, height: f64) {
        self.on_did_layout.fire(LayoutDimension { height, width: self.sidebar_width.get() });
    }
}

impl IDisposable for TestModalEditorSidebarHost {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `render: (_container, _onDidLayout) => ({ dispose: () => { } })`.
fn stub_sidebar_content() -> ModalEditorSidebar {
    ModalEditorSidebar { render: Some(Rc::new(|_container, _on_did_layout, _context_key_service| to_disposable(|| {}))), ..ModalEditorSidebar::default() }
}

/// `{ sidebar }` de `host.updateOptions({ sidebar: ... })`.
fn sidebar_options(sidebar: ModalEditorSidebar) -> ModalEditorPartOptions {
    ModalEditorPartOptions { sidebar: Some(sidebar), ..ModalEditorPartOptions::default() }
}

/// `let fired = false; disposables.add(host.onDidResize(() => { fired = true; }))`.
fn track_resize(guard: &LeakGuard, host: &TestModalEditorSidebarHost) -> Rc<Cell<bool>> {
    let fired = Rc::new(Cell::new(false));
    let flag = fired.clone();
    guard.add(host.on_did_resize().listen(move |_| flag.set(true)));
    fired
}

// --- option propagation -------------------------------------------------

#[test]
fn add_sidebar_sets_has_sidebar_and_default_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.add_sidebar(&stub_sidebar_content());

    assert_eq!((host.has_sidebar(), host.sidebar_width(), host.render_count()), (true, MODAL_SIDEBAR_DEFAULT_WIDTH, 1));
}

#[test]
fn remove_sidebar_clears_sidebar_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.add_sidebar(&stub_sidebar_content());
    host.remove_sidebar();

    assert_eq!((host.has_sidebar(), host.sidebar_width(), host.render_count()), (false, 0.0, 1));
}

#[test]
fn update_sidebar_content_disposes_previous_content_and_re_renders() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    let first_disposed = Rc::new(Cell::new(false));
    let disposed = first_disposed.clone();
    let first_content = ModalEditorSidebar {
        render: Some(Rc::new(move |_container, _on_did_layout, _context_key_service| {
            let disposed = disposed.clone();
            to_disposable(move || disposed.set(true))
        })),
        ..ModalEditorSidebar::default()
    };
    host.add_sidebar(&first_content);

    let second_rendered = Rc::new(Cell::new(false));
    let rendered = second_rendered.clone();
    let second_content = ModalEditorSidebar {
        render: Some(Rc::new(move |_container, _on_did_layout, _context_key_service| -> DisposableRef {
            rendered.set(true);
            to_disposable(|| {})
        })),
        ..ModalEditorSidebar::default()
    };
    host.update_sidebar_content(&second_content);

    assert_eq!((first_disposed.get(), second_rendered.get(), host.render_count()), (true, true, 2));
}

#[test]
fn update_options_adds_sidebar_when_not_present() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.update_options(&sidebar_options(stub_sidebar_content()));

    assert_eq!((host.has_sidebar(), host.render_count()), (true, 1));
}

#[test]
fn update_options_updates_sidebar_content_when_already_present() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.add_sidebar(&stub_sidebar_content());
    host.update_options(&sidebar_options(stub_sidebar_content()));

    assert_eq!((host.has_sidebar(), host.render_count()), (true, 2));
}

// --- min-size constraints -----------------------------------------------

#[test]
fn effective_min_width_accounts_for_sidebar() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    let without_sidebar = host.effective_min_width();

    host.add_sidebar(&stub_sidebar_content());
    let with_sidebar = host.effective_min_width();

    assert_eq!((without_sidebar, with_sidebar), (MODAL_MIN_WIDTH, MODAL_MIN_WIDTH + MODAL_SIDEBAR_MIN_WIDTH));
}

#[test]
fn effective_min_width_reverts_after_sidebar_removal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.add_sidebar(&stub_sidebar_content());
    host.remove_sidebar();

    assert_eq!(host.effective_min_width(), MODAL_MIN_WIDTH);
}

// --- resize constraints -------------------------------------------------

#[test]
fn resize_sidebar_clamps_to_min_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(-9999.0);

    assert_eq!(host.sidebar_width(), MODAL_SIDEBAR_MIN_WIDTH);
}

#[test]
fn resize_sidebar_clamps_to_max_width_container_minus_modal_min() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(800.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(9999.0);

    assert_eq!(host.sidebar_width(), 800.0 - MODAL_MIN_WIDTH);
}

#[test]
fn resize_sidebar_applies_delta_within_bounds() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(30.0);

    assert_eq!(host.sidebar_width(), MODAL_SIDEBAR_DEFAULT_WIDTH + 30.0);
}

#[test]
fn resize_sidebar_fires_on_did_resize() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    let fired = track_resize(&guard, &host);

    host.resize_sidebar(10.0);

    assert!(fired.get());
}

#[test]
fn reset_sidebar_width_restores_default_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(100.0);
    host.reset_sidebar_width();

    assert_eq!(host.sidebar_width(), MODAL_SIDEBAR_DEFAULT_WIDTH);
}

#[test]
fn reset_sidebar_width_clamps_if_container_shrunk() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    // Shrink container so that default width exceeds max
    host.set_container_width(MODAL_MIN_WIDTH + MODAL_SIDEBAR_MIN_WIDTH);
    host.reset_sidebar_width();

    assert_eq!(host.sidebar_width(), MODAL_SIDEBAR_MIN_WIDTH);
}

// --- width persistence ---------------------------------------------------

#[test]
fn add_sidebar_restores_custom_width_when_present() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(300.0), None);
    host.set_container_width(1000.0);

    host.add_sidebar(&stub_sidebar_content());

    assert_eq!(host.sidebar_width(), 300.0);
}

#[test]
fn add_sidebar_uses_default_width_when_no_custom_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);

    host.add_sidebar(&stub_sidebar_content());

    assert_eq!(host.sidebar_width(), MODAL_SIDEBAR_DEFAULT_WIDTH);
}

#[test]
fn resize_sidebar_sets_custom_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(50.0);

    assert_eq!(host.custom_width(), Some(MODAL_SIDEBAR_DEFAULT_WIDTH + 50.0));
}

#[test]
fn reset_sidebar_width_clears_custom_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(50.0);
    host.reset_sidebar_width();

    assert_eq!(host.custom_width(), None);
}

// --- clampWidth ---------------------------------------------------------

#[test]
fn clamp_width_resets_to_default_when_sidebar_is_too_wide_for_modal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(500.0), None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    assert_eq!(host.sidebar_width(), 500.0);

    host.clamp_width(800.0); // 500 + 400 (MODAL_MIN_WIDTH) > 800, default fits

    assert_eq!((host.sidebar_width(), host.custom_width()), (MODAL_SIDEBAR_DEFAULT_WIDTH, None));
}

#[test]
fn clamp_width_keeps_width_when_sidebar_fits_within_modal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(300.0), None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.clamp_width(1000.0); // 300 + 400 (MODAL_MIN_WIDTH) <= 1000

    assert_eq!((host.sidebar_width(), host.custom_width()), (300.0, Some(300.0)));
}

#[test]
fn clamp_width_fires_on_did_resize_when_clamping() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(500.0), None);
    host.add_sidebar(&stub_sidebar_content());

    let fired = track_resize(&guard, &host);

    host.clamp_width(600.0);

    assert!(fired.get());
}

#[test]
fn clamp_width_does_not_fire_on_did_resize_when_not_clamping() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(200.0), None);
    host.add_sidebar(&stub_sidebar_content());

    let fired = track_resize(&guard, &host);

    host.clamp_width(1000.0);

    assert!(!fired.get());
}

#[test]
fn clamp_width_uses_constrained_width_when_modal_is_very_narrow() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, Some(400.0), None);
    host.add_sidebar(&stub_sidebar_content());

    host.clamp_width(500.0); // 400 + 400 > 500, default + 400 > 500 too

    assert_eq!((host.sidebar_width(), host.custom_width()), (MODAL_SIDEBAR_MIN_WIDTH, None));
}

// --- layout propagation -------------------------------------------------

#[test]
fn layout_fires_on_did_layout_with_current_dimensions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    // Capture layout event by re-adding content that tracks it
    let layouts: Rc<RefCell<Vec<LayoutDimension>>> = Rc::new(RefCell::new(Vec::new()));
    let tracked = layouts.clone();
    let tracked_content = ModalEditorSidebar {
        render: Some(Rc::new(move |_container, on_did_layout: Event<LayoutDimension>, _context_key_service| {
            let tracked = tracked.clone();
            on_did_layout.listen(move |dimension| tracked.borrow_mut().push(*dimension))
        })),
        ..ModalEditorSidebar::default()
    };
    host.update_sidebar_content(&tracked_content);

    host.layout(500.0);

    assert_eq!(*layouts.borrow(), vec![LayoutDimension { height: 500.0, width: MODAL_SIDEBAR_DEFAULT_WIDTH }]);
}

// --- sidebar visibility -------------------------------------------------

#[test]
fn sidebar_is_visible_by_default() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    assert_eq!((host.sidebar_visible(), host.effective_sidebar_width()), (true, MODAL_SIDEBAR_DEFAULT_WIDTH));
}

#[test]
fn toggle_sidebar_visible_hides_sidebar_and_returns_zero_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    host.toggle_sidebar_visible();

    assert_eq!((host.sidebar_visible(), host.effective_sidebar_width()), (false, 0.0));
}

#[test]
fn toggle_sidebar_visible_twice_restores_sidebar() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    host.toggle_sidebar_visible();
    host.toggle_sidebar_visible();

    assert_eq!((host.sidebar_visible(), host.effective_sidebar_width()), (true, MODAL_SIDEBAR_DEFAULT_WIDTH));
}

#[test]
fn toggle_sidebar_visible_fires_on_did_resize() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.add_sidebar(&stub_sidebar_content());

    let fired = track_resize(&guard, &host);

    host.toggle_sidebar_visible();

    assert!(fired.get());
}

#[test]
fn sidebar_hidden_state_persists_via_constructor() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, Some(true));
    host.add_sidebar(&stub_sidebar_content());

    assert_eq!((host.sidebar_visible(), host.effective_sidebar_width()), (false, 0.0));
}

#[test]
fn hidden_sidebar_preserves_width_for_when_restored() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let host = TestModalEditorSidebarHost::new(&guard, None, None);
    host.set_container_width(1000.0);
    host.add_sidebar(&stub_sidebar_content());

    host.resize_sidebar(50.0);
    host.toggle_sidebar_visible();

    assert_eq!((host.effective_sidebar_width(), host.sidebar_width()), (0.0, MODAL_SIDEBAR_DEFAULT_WIDTH + 50.0));

    host.toggle_sidebar_visible();

    assert_eq!((host.effective_sidebar_width(), host.sidebar_width()), (MODAL_SIDEBAR_DEFAULT_WIDTH + 50.0, MODAL_SIDEBAR_DEFAULT_WIDTH + 50.0));
}
