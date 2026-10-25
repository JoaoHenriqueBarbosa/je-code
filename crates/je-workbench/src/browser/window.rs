//! Porte de `BaseWindow` de `src/vs/workbench/browser/window.ts`: o que toda janela do workbench (principal ou auxiliar)
//! configura ao nascer: o `setTimeout` que vale para todas as janelas abertas, o espelho da tela cheia do host, a classe
//! `context-menu-visible` do contêiner ativo e a variável `--window-zoom-factor` do contêiner da janela.
//!
//! Divergências:
//! - **Janela.** `targetWindow: CodeWindow` é a [`CodeWindow`] do modelo de documento nativo de `je-base`, com os timers
//!   atribuíveis (`window.setTimeout`, `vscodeOriginalSetTimeout`...) e o `document` do modelo. O contêiner é um [`Node`] desse
//!   modelo: [`WindowLayoutService`] é o recorte do `IWorkbenchLayoutService` que a classe lê (`getContainer(window)` e
//!   `activeContainer`), porque o serviço do porte devolve identificadores de contêiner da árvore de widgets, e não nós.
//! - **`dom`.** O parâmetro de teste `dom = { getWindowsCount, getWindows }` é o [`WindowsDom`]; sem ele vale o
//!   [`DefaultWindowsDom`], sobre o registro de janelas de `je-base`, com um `DisposableStore` por janela (o `getWindows` do
//!   original devolve `{ window, disposables }`).
//! - **Foco entre janelas.** `enableWindowFocusOnElementFocus` troca o `HTMLElement.prototype.focus` da janela para trazer a
//!   janela do elemento ao primeiro plano (`hostService.focus`, `window.focus()`). O foco nativo passa pela árvore de widgets,
//!   que não tem protótipo de elemento a trocar, então o passo não existe; com ele saem o `IWorkbenchEnvironmentService` e o
//!   `hasAppFocus` que só ele usa.
//! - **`enableMultiWindowAwareTimeout`.** É um método protegido que o teste sobrescreve com um corpo vazio; aqui é o método
//!   do trait [`BaseWindowOverrides`], cujo padrão é a função [`enable_multi_window_aware_timeout`] (o `super`). Um timer de
//!   `timeout === 0` ou com uma só janela vai direto ao original; `typeof handler === 'string'` não existe (o manipulador é
//!   sempre uma função). `createSingleCallFunction(handler, cleanup)` chama o manipulador uma vez e, em seguida, o descarte
//!   dos timers das outras janelas.
//! - **`BrowserWindow` e `confirmOnShutdown`.** Dependem de `IOpenerService`, `IDialogService`, do `BrowserLifecycleService` e
//!   dos eventos de DOM do navegador (`beforeunload`, `resize`, `wheel`), e entram com os serviços de que dependem.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::browser::browser::{get_zoom_factor, on_did_change_zoom_level, set_fullscreen};
use je_base::browser::dom::{ClearTimeoutFn, CodeWindow, Node, SetTimeoutFn, TimerHandler, get_window_by_id, get_windows, get_windows_count, is_auxiliary_window, main_window};
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_platform::contextview::context_view::IContextMenuService;

use crate::services::host::host::IHostService;

/// `IRegisteredCodeWindow`: a janela e o descarte que a acompanha.
#[derive(Clone)]
pub struct RegisteredCodeWindow {
    pub window: Rc<CodeWindow>,
    pub disposables: Rc<DisposableStore>,
}

/// `dom = { getWindowsCount, getWindows }`: as janelas abertas (para teste).
pub trait WindowsDom {
    fn get_windows_count(&self) -> usize;
    fn get_windows(&self) -> Vec<RegisteredCodeWindow>;
}

/// O `dom` padrão: o registro de janelas de `je-base`.
#[derive(Default)]
pub struct DefaultWindowsDom {
    disposables: RefCell<HashMap<Option<u32>, Rc<DisposableStore>>>,
}

impl WindowsDom for DefaultWindowsDom {
    fn get_windows_count(&self) -> usize {
        get_windows_count()
    }

    fn get_windows(&self) -> Vec<RegisteredCodeWindow> {
        get_windows()
            .into_iter()
            .map(|window| {
                let disposables = self.disposables.borrow_mut().entry(window.vscode_window_id()).or_insert_with(DisposableStore::new).clone();
                RegisteredCodeWindow { window, disposables }
            })
            .collect()
    }
}

/// O recorte do `IWorkbenchLayoutService` que a janela lê.
pub trait WindowLayoutService {
    /// `getContainer(window)`.
    fn get_container(&self, target_window: &Rc<CodeWindow>) -> Node;
    /// `activeContainer`.
    fn active_container(&self) -> Node;
}

/// Os métodos protegidos de `BaseWindow` que uma subclasse sobrescreve.
pub trait BaseWindowOverrides {
    /// `enableMultiWindowAwareTimeout(targetWindow, dom)`.
    fn enable_multi_window_aware_timeout(&self, target_window: &Rc<CodeWindow>, dom: &Rc<dyn WindowsDom>) {
        enable_multi_window_aware_timeout(target_window, dom);
    }
}

/// A janela sem nenhum método sobrescrito.
pub struct NoOverrides;

impl BaseWindowOverrides for NoOverrides {}

/// Os timers de uma chamada de `setTimeout` que valem para todas as janelas: os descartes de cada janela.
type TimeoutDisposables = Rc<RefCell<Vec<(u64, DisposableRef)>>>;

/// `BaseWindow.TIMEOUT_HANDLES` começa em `Number.MIN_SAFE_INTEGER`: tenta não competir com os identificadores do
/// `setTimeout` nativo.
const MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

thread_local! {
    /// `BaseWindow.TIMEOUT_HANDLES`.
    static TIMEOUT_HANDLES: Cell<i64> = const { Cell::new(MIN_SAFE_INTEGER) };
    /// Identificador de cada descarte de timer (o `Set<IDisposable>` do original compara por identidade).
    static TIMEOUT_DISPOSABLE_IDS: Cell<u64> = const { Cell::new(0) };
    /// `BaseWindow.TIMEOUT_DISPOSABLES`.
    static TIMEOUT_DISPOSABLES: RefCell<HashMap<i64, TimeoutDisposables>> = RefCell::new(HashMap::new());
}

/// `dispose(timeoutDisposables)`: cada descarte se tira do conjunto, então o conjunto é copiado antes.
fn dispose_timeout_disposables(timeout_disposables: &TimeoutDisposables) {
    let pending: Vec<DisposableRef> = timeout_disposables.borrow().iter().map(|(_, disposable)| disposable.clone()).collect();
    for disposable in pending {
        disposable.dispose();
    }
}

/// `enableMultiWindowAwareTimeout(targetWindow, dom)`: override `setTimeout` and `clearTimeout` on the provided window to make
/// sure timeouts are dispatched to all opened windows. Some browsers may decide to throttle timeouts in minimized windows, so
/// with this we can ensure the timeout is scheduled without being throttled (unless all windows are minimized).
pub fn enable_multi_window_aware_timeout(target_window: &Rc<CodeWindow>, dom: &Rc<dyn WindowsDom>) {
    let original_set_timeout = target_window.set_timeout_fn();
    let original_clear_timeout = target_window.clear_timeout_fn();
    target_window.define_original_timeout_fns(original_set_timeout.clone(), original_clear_timeout.clone());

    let set_timeout: SetTimeoutFn = {
        let original_set_timeout = original_set_timeout.clone();
        let dom = dom.clone();
        Rc::new(move |handler: TimerHandler, timeout: u64, args: &[Dynamic]| -> i64 {
            if dom.get_windows_count() == 1 || timeout == 0 /* immediates are never throttled */ {
                return original_set_timeout(handler, timeout, args);
            }

            let timeout_disposables: TimeoutDisposables = Rc::default();
            let timeout_handle = TIMEOUT_HANDLES.with(|handles| handles.replace(handles.get() + 1));
            TIMEOUT_DISPOSABLES.with(|all| all.borrow_mut().insert(timeout_handle, timeout_disposables.clone()));

            // `createSingleCallFunction(handler, () => { dispose(timeoutDisposables); TIMEOUT_DISPOSABLES.delete(timeoutHandle); })`
            let did_call = Rc::new(Cell::new(false));
            let handler_fn: Rc<dyn Fn(&[Dynamic])> = {
                let timeout_disposables = timeout_disposables.clone();
                Rc::new(move |args: &[Dynamic]| {
                    if did_call.replace(true) {
                        return;
                    }
                    handler(args);
                    dispose_timeout_disposables(&timeout_disposables);
                    TIMEOUT_DISPOSABLES.with(|all| all.borrow_mut().remove(&timeout_handle));
                })
            };

            for RegisteredCodeWindow { window, disposables } in dom.get_windows() {
                if is_auxiliary_window(&window) && window.document().visibility_state() == "hidden" {
                    continue; // skip over hidden windows (but never over main window)
                }

                // we track didClear in case the browser does not properly clear the timeout
                // this can happen for timeouts on unfocused windows
                let did_clear = Rc::new(Cell::new(false));

                let handle = window.vscode_original_set_timeout().map(|original| {
                    let did_clear = did_clear.clone();
                    let handler_fn = handler_fn.clone();
                    let window_handler: TimerHandler = Rc::new(move |args: &[Dynamic]| {
                        if did_clear.get() {
                            return;
                        }
                        handler_fn(args);
                    });
                    original(window_handler, timeout, args)
                });

                let id = TIMEOUT_DISPOSABLE_IDS.with(|ids| ids.replace(ids.get() + 1));
                // O `timeoutDisposable` se referencia dentro do próprio descarte: o `own` guarda o `Rc` até o descarte rodar.
                let own: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                let timeout_disposable = to_disposable({
                    let did_clear = did_clear.clone();
                    let window = window.clone();
                    let timeout_disposables = timeout_disposables.clone();
                    let disposables = disposables.clone();
                    let own = own.clone();
                    move || {
                        did_clear.set(true);
                        if let Some(clear_timeout) = window.vscode_original_clear_timeout() {
                            clear_timeout(handle);
                        }
                        timeout_disposables.borrow_mut().retain(|(candidate, _)| *candidate != id);
                        // Remove from the window's DisposableStore without re-disposing (we're already inside dispose)
                        let this = own.borrow_mut().take();
                        if let Some(this) = this {
                            disposables.delete_and_leak(&this);
                        }
                    }
                });
                *own.borrow_mut() = Some(timeout_disposable.clone());

                disposables.add(timeout_disposable.clone());
                timeout_disposables.borrow_mut().push((id, timeout_disposable));
            }

            timeout_handle
        })
    };

    let clear_timeout: ClearTimeoutFn = Rc::new(move |timeout_handle: Option<i64>| {
        let timeout_disposables = timeout_handle.and_then(|handle| TIMEOUT_DISPOSABLES.with(|all| all.borrow().get(&handle).cloned()));
        match (timeout_handle, timeout_disposables) {
            (Some(handle), Some(timeout_disposables)) => {
                dispose_timeout_disposables(&timeout_disposables);
                TIMEOUT_DISPOSABLES.with(|all| all.borrow_mut().remove(&handle));
            }
            _ => original_clear_timeout(timeout_handle),
        }
    });

    target_window.assign_timeout_fns(set_timeout, clear_timeout);
}

/// `BaseWindow`.
pub struct BaseWindow {
    base: Rc<Disposable>,
}

impl BaseWindow {
    /// `constructor(targetWindow, dom, hostService, environmentService, contextMenuService, layoutService)`: ver o cabeçalho
    /// do módulo para o que sai (`environmentService`, que só o foco entre janelas lê).
    pub fn new(
        target_window: &Rc<CodeWindow>,
        dom: Option<Rc<dyn WindowsDom>>,
        host_service: &Rc<dyn IHostService>,
        context_menu_service: &Rc<dyn IContextMenuService>,
        layout_service: &Rc<dyn WindowLayoutService>,
        overrides: &dyn BaseWindowOverrides,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let dom = dom.unwrap_or_else(|| -> Rc<dyn WindowsDom> { Rc::new(DefaultWindowsDom::default()) });

        overrides.enable_multi_window_aware_timeout(target_window, &dom);

        Self::register_full_screen_listeners(&base, target_window.vscode_window_id(), host_service);
        Self::register_context_menu_listeners(&base, target_window, context_menu_service, layout_service);
        Self::register_window_zoom_factor(&base, target_window, layout_service);

        Rc::new(Self { base })
    }

    fn register_window_zoom_factor(base: &Rc<Disposable>, target_window: &Rc<CodeWindow>, layout_service: &Rc<dyn WindowLayoutService>) {
        // Auxiliary windows mirror document/body styles, but keep their own workbench styles.
        let container = layout_service.get_container(target_window);
        let update = {
            let target_window = target_window.clone();
            move || container.style_set_property("--window-zoom-factor", &number_to_js_string(get_zoom_factor(&target_window)))
        };
        update();
        let target_window_id = target_window.vscode_window_id();
        base.register(on_did_change_zoom_level().listen(move |window_id| {
            if *window_id == target_window_id {
                update();
            }
        }));
    }

    fn register_full_screen_listeners(base: &Rc<Disposable>, target_window_id: Option<u32>, host_service: &Rc<dyn IHostService>) {
        base.register(host_service.on_did_change_full_screen().listen(move |event| {
            if Some(event.window_id) == target_window_id
                && let Some(target_window) = get_window_by_id(event.window_id)
            {
                set_fullscreen(event.fullscreen, &target_window);
            }
        }));
    }

    fn register_context_menu_listeners(
        base: &Rc<Disposable>,
        target_window: &Rc<CodeWindow>,
        context_menu_service: &Rc<dyn IContextMenuService>,
        layout_service: &Rc<dyn WindowLayoutService>,
    ) {
        if !Rc::ptr_eq(target_window, &main_window()) {
            // we only need to listen in the main window as the code
            // will go by the active container and update accordingly
            return;
        }

        let update = {
            let layout_service = layout_service.clone();
            move |visible: bool| {
                let active_container = layout_service.active_container();
                if visible {
                    active_container.class_list_add("context-menu-visible");
                } else {
                    active_container.class_list_remove("context-menu-visible");
                }
            }
        };
        base.register(context_menu_service.on_did_show_context_menu().listen({
            let update = update.clone();
            move |_| update(true)
        }));
        base.register(context_menu_service.on_did_hide_context_menu().listen(move |_| update(false)));
    }
}

impl IDisposable for BaseWindow {
    fn dispose(&self) {
        self.base.dispose();
    }
}
