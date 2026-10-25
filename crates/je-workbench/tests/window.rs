//! Porte de `src/vs/workbench/test/browser/window.test.ts` (suíte `Window`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`CodeWindow`.** O `mock<CodeWindow>()` com `vscodeWindowId` 801 e 802 é uma [`CodeWindow`] do modelo de documento
//!   nativo com o identificador atribuído (`ensure_code_window`). Os objetos `{ setTimeout, clearTimeout }` do caso de
//!   `setTimeout` multi-janela são [`CodeWindow`]s sem identificador, com os timers atribuídos (`assign_timeout_fns`).
//! - **`document.createElement('div')`.** São nós do documento da janela principal. O original anexa os dois documentos ao
//!   `document.body` para o `getComputedStyle` valer; o modelo de documento calcula o estilo de uma propriedade personalizada
//!   pelo próprio nó e pelos ancestrais (`computed_style_property_value`), então não há corpo a anexar.
//! - **`TestWindow`.** `protected override enableWindowFocusOnElementFocus(): void { }` não tem o que sobrescrever (o porte não
//!   tem esse passo, ver `browser/window.rs`). `ZoomTestWindow` sobrescreve `enableMultiWindowAwareTimeout` com um corpo vazio
//!   ([`NoMultiWindowTimeout`]). O `TestLayoutService` com `getContainer` trocado é o [`TestLayoutService`] deste arquivo, que
//!   devolve o contêiner (também como contêiner ativo) e o `TestContextMenuService` é o [`TestContextMenuService`], sem menus.
//! - **`dom`.** `{ getWindowsCount, getWindows }` é o [`TestDom`], com a lista de janelas trocável e a contagem fixa opcional.
//! - **`timeout(0)` e `runWithFakedTimers({ useFakeTimers: true }, ...)`.** O caso do zoom roda no relógio virtual, onde o
//!   `timeout` do `common` espera o timer virtual; `await` é `.await.unwrap()`.
//! - **`store`.** É o [`LeakGuard`](common::LeakGuard) do caso (`ensureNoDisposablesAreLeakedInTestSuite`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::browser::browser::{set_zoom_factor, set_zoom_level};
use je_base::browser::dom::{CodeWindow, TimerHandler, ensure_code_window, main_window, track_attributes};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuService};
use je_workbench::browser::window::{BaseWindow, BaseWindowOverrides, NoOverrides, RegisteredCodeWindow, WindowLayoutService, WindowsDom};
use je_workbench::services::host::host::IHostService;
use je_workbench::test::browser::workbench_test_services::TestHostService;

/// `TestContextMenuService`: os eventos nunca disparam e nenhum menu abre.
struct TestContextMenuService;

impl IContextMenuService for TestContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        Event::none()
    }

    fn show_context_menu(&self, _request: ContextMenuRequest) {}
}

/// `new class extends TestLayoutService { override getContainer(): HTMLElement { return container; } }`.
struct TestLayoutService {
    container: je_base::browser::dom::Node,
}

impl WindowLayoutService for TestLayoutService {
    fn get_container(&self, _target_window: &Rc<CodeWindow>) -> je_base::browser::dom::Node {
        self.container.clone()
    }

    fn active_container(&self) -> je_base::browser::dom::Node {
        self.container.clone()
    }
}

/// `dom = { getWindowsCount, getWindows }`: sem `count`, a contagem é o tamanho da lista de janelas.
struct TestDom {
    windows: Rc<RefCell<Vec<RegisteredCodeWindow>>>,
    count: Option<usize>,
}

impl WindowsDom for TestDom {
    fn get_windows_count(&self) -> usize {
        self.count.unwrap_or_else(|| self.windows.borrow().len())
    }

    fn get_windows(&self) -> Vec<RegisteredCodeWindow> {
        self.windows.borrow().clone()
    }
}

/// `override enableMultiWindowAwareTimeout(): void { }` de `ZoomTestWindow`.
struct NoMultiWindowTimeout;

impl BaseWindowOverrides for NoMultiWindowTimeout {
    fn enable_multi_window_aware_timeout(&self, _target_window: &Rc<CodeWindow>, _dom: &Rc<dyn WindowsDom>) {}
}

/// `new TestWindow(window, dom, container)`.
fn test_window(window: &Rc<CodeWindow>, dom: &Rc<dyn WindowsDom>, container: je_base::browser::dom::Node, overrides: &dyn BaseWindowOverrides) -> Rc<BaseWindow> {
    let host_service: Rc<dyn IHostService> = Rc::new(TestHostService);
    let context_menu_service: Rc<dyn IContextMenuService> = Rc::new(TestContextMenuService);
    let layout_service: Rc<dyn WindowLayoutService> = Rc::new(TestLayoutService { container });

    BaseWindow::new(window, Some(dom.clone()), &host_service, &context_menu_service, &layout_service, overrides)
}

#[derive(Debug, PartialEq)]
struct ZoomFactors {
    initial: Vec<String>,
    after_source_zoom: Vec<String>,
    after_auxiliary_zoom: Vec<String>,
    after_dispose: Vec<String>,
}

fn strings(values: [&str; 2]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn keeps_window_zoom_factors_local_when_auxiliary_document_attributes_are_mirrored() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();

    run_with_faked_timers(async move {
        let source_window = CodeWindow::new();
        ensure_code_window(&source_window, 801);
        let auxiliary_window = CodeWindow::new();
        ensure_code_window(&auxiliary_window, 802);

        let document = main_window().document();
        let source_document = document.create_element("div");
        let auxiliary_document = document.create_element("div");
        let source_container = source_document.append_child(&document.create_element("div"));
        let auxiliary_container = auxiliary_document.append_child(&document.create_element("div"));
        store.add(to_disposable({
            let (source_window, auxiliary_window) = (source_window.clone(), auxiliary_window.clone());
            move || {
                for window in [&source_window, &auxiliary_window] {
                    set_zoom_factor(1.0, window);
                    set_zoom_level(0.0, window);
                }
            }
        }));
        set_zoom_factor(0.5, &auxiliary_window);
        set_zoom_level(-1.0, &auxiliary_window);

        let dom: Rc<dyn WindowsDom> = Rc::new(TestDom { windows: Rc::default(), count: Some(2) });
        store.add_rc(test_window(&source_window, &dom, source_container.clone(), &NoMultiWindowTimeout));
        let auxiliary = store.add_rc(test_window(&auxiliary_window, &dom, auxiliary_container.clone(), &NoMultiWindowTimeout));
        store.add(track_attributes(&source_document, &auxiliary_document, None));
        let factors = || -> Vec<String> {
            [&source_container, &auxiliary_container].iter().map(|container| container.computed_style_property_value("--window-zoom-factor")).collect()
        };
        let initial = factors();

        set_zoom_factor(1.2, &source_window);
        set_zoom_level(1.0, &source_window);
        source_document.style_set_property("--window-zoom-factor", "1.2");
        timeout(0).await.unwrap();
        let after_source_zoom = factors();

        set_zoom_factor(0.8, &auxiliary_window);
        set_zoom_level(-2.0, &auxiliary_window);
        let after_auxiliary_zoom = factors();
        auxiliary.dispose();
        set_zoom_factor(1.0, &auxiliary_window);
        set_zoom_level(0.0, &auxiliary_window);

        assert_eq!(
            ZoomFactors { initial, after_source_zoom, after_auxiliary_zoom, after_dispose: factors() },
            ZoomFactors {
                initial: strings(["1", "0.5"]),
                after_source_zoom: strings(["1.2", "0.5"]),
                after_auxiliary_zoom: strings(["1.2", "0.8"]),
                after_dispose: strings(["1.2", "0.8"]),
            }
        );
    });
}

/// `await new Promise<void>((resolve, reject) => window.setTimeout(callback, delay))`: a promessa resolve quando o manipulador
/// roda uma vez, e rejeita se ele rodar duas vezes ou, com `expected_window`, para outra janela (o `windowId` que o timer
/// da janela entrega ao manipulador).
fn wait_for_timeout(window: &Rc<CodeWindow>, called: &Rc<Cell<bool>>, delay: u64, expected_window: Option<f64>) -> Promise<()> {
    let called = called.clone();
    Promise::with_executor(|resolver| {
        window.set_timeout(
            move |args| {
                let window_id_matches = expected_window.is_none_or(|expected| matches!(args.first(), Some(Dynamic::Number(window_id)) if *window_id == expected));
                if !called.get() && window_id_matches {
                    called.set(true);
                    resolver.resolve(());
                } else if called.get() {
                    resolver.reject(Error::new("timeout called twice"));
                } else {
                    resolver.reject(Error::new("timeout called for wrong window"));
                }
            },
            delay,
            &[],
        );
    })
}

#[test]
fn multi_window_aware_set_timeout() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let disposables = DisposableStore::new();

        let windows: Rc<RefCell<Vec<RegisteredCodeWindow>>> = Rc::default();
        let dom: Rc<dyn WindowsDom> = Rc::new(TestDom { windows: windows.clone(), count: None });

        let set_timeout_calls: Rc<RefCell<Vec<u32>>> = Rc::default();
        let clear_timeout_calls: Rc<RefCell<Vec<u32>>> = Rc::default();

        let create_window = |id: u32, slow: bool| -> Rc<CodeWindow> {
            let res = CodeWindow::new();
            res.assign_timeout_fns(
                {
                    let set_timeout_calls = set_timeout_calls.clone();
                    Rc::new(move |callback: TimerHandler, delay: u64, args: &[Dynamic]| -> i64 {
                        set_timeout_calls.borrow_mut().push(id);

                        main_window().set_timeout(move |_| callback(&[Dynamic::Number(f64::from(id))]), if slow { delay * 2 } else { delay }, args)
                    })
                },
                {
                    let clear_timeout_calls = clear_timeout_calls.clone();
                    Rc::new(move |timeout_id: Option<i64>| {
                        clear_timeout_calls.borrow_mut().push(id);

                        main_window().clear_timeout(timeout_id);
                    })
                },
            );

            disposables.add_rc(test_window(&res, &dom, main_window().document().create_element("div"), &NoOverrides));

            res
        };
        let registered = |window: &Rc<CodeWindow>| RegisteredCodeWindow { window: window.clone(), disposables: disposables.clone() };
        let reset = |called: &Rc<Cell<bool>>| {
            called.set(false);
            set_timeout_calls.borrow_mut().clear();
            clear_timeout_calls.borrow_mut().clear();
        };

        let window1 = create_window(1, false);
        *windows.borrow_mut() = vec![registered(&window1)];

        // Window Count: 1

        let called = Rc::new(Cell::new(false));
        wait_for_timeout(&window1, &called, 1, None).await.unwrap();

        assert!(called.get());
        assert_eq!(*set_timeout_calls.borrow(), vec![1]);
        assert_eq!(*clear_timeout_calls.borrow(), Vec::<u32>::new());
        reset(&called);

        wait_for_timeout(&window1, &called, 0, None).await.unwrap();

        assert!(called.get());
        assert_eq!(*set_timeout_calls.borrow(), vec![1]);
        assert_eq!(*clear_timeout_calls.borrow(), Vec::<u32>::new());
        reset(&called);

        // Window Count: 3

        let window2 = create_window(2, false);
        let window3 = create_window(3, false);
        *windows.borrow_mut() = vec![registered(&window2), registered(&window1), registered(&window3)];

        wait_for_timeout(&window1, &called, 1, None).await.unwrap();

        assert!(called.get());
        assert_eq!(*set_timeout_calls.borrow(), vec![2, 1, 3]);
        assert_eq!(*clear_timeout_calls.borrow(), vec![2, 1, 3]);
        reset(&called);

        // Window Count: 2 (1 fast, 1 slow)

        let window2 = create_window(2, true);
        *windows.borrow_mut() = vec![registered(&window2), registered(&window1)];

        wait_for_timeout(&window1, &called, 1, Some(1.0)).await.unwrap();

        assert!(called.get());
        assert_eq!(*set_timeout_calls.borrow(), vec![2, 1]);
        assert_eq!(*clear_timeout_calls.borrow(), vec![2, 1]);
        reset(&called);

        disposables.dispose();
    });

    drop(guard);
}
