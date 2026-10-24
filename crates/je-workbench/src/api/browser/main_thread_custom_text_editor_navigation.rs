//! Porte de `src/vs/workbench/api/browser/mainThreadCustomTextEditorNavigation.ts`: a navegação de um editor de texto
//! personalizado do lado do workbench, que repassa ao extension host o pedido de revelar um intervalo e o ciclo de
//! capturar, restaurar e soltar o estado de visão.
//!
//! Divergências:
//! - **Herança.** `extends Disposable` é um [`Disposable`] guardado em `base`; o `_register` é o `register`.
//! - **`this`.** Os métodos `async` e os fechos que o original cria com `this` precisam de um `Rc<Self>`: o objeto guarda
//!   um `Weak` de si (`Rc::new_cyclic`).
//! - **Tipos.** `ITextModel` é o [`TextModel`]; `IRange` e `ISelection` são o [`Range`] e o [`Selection`] (`Copy`);
//!   `WebviewHandle` é `String`; `_focus: () => void` é um `Rc<dyn Fn()>`; o `Set<number>` das capturas é uma lista que
//!   preserva a ordem de inserção (a ordem de iteração do `Set`).
//! - **`async revealRange`** é [`async_fn`]; o `try/catch/finally` é a conferência do `Result` do `await` e o descarte
//!   do ouvinte e da fonte de cancelamento ao fim. Um pânico de `_focus()` cai no mesmo `catch` do original.
//! - **`void captured.then(async () => ...).catch(...)`** é o encadeamento de [`Promise::then_promise`] e
//!   [`Promise::catch`] descartado.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{self, Error, is_cancellation_error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{self, Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;

use crate::api::common::ext_host_protocol::{ExtHostCustomEditorsShape, WebviewHandle};
use crate::contrib::custom_editor::custom_text_editor_navigation::ICustomTextEditorNavigation;

/// `MainThreadCustomTextEditorNavigation`.
pub struct MainThreadCustomTextEditorNavigation {
    me: Weak<MainThreadCustomTextEditorNavigation>,
    base: Rc<Disposable>,
    on_did_change_selection: Rc<Emitter<()>>,
    on_did_dispose: Rc<Emitter<()>>,
    lifetime: CancellationTokenSource,
    states: RefCell<Vec<u32>>,
    next_state: Cell<u32>,
    disposed: Cell<bool>,
    selection: RefCell<Option<Selection>>,
    model: Rc<TextModel>,
    handle: WebviewHandle,
    proxy: Rc<dyn ExtHostCustomEditorsShape>,
    focus: Rc<dyn Fn()>,
}

impl MainThreadCustomTextEditorNavigation {
    pub fn new(model: Rc<TextModel>, selection: Option<Selection>, handle: WebviewHandle, proxy: Rc<dyn ExtHostCustomEditorsShape>, focus: Rc<dyn Fn()>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_selection = Emitter::new();
        base.register(on_did_change_selection.clone());
        let on_did_dispose = Emitter::new();
        base.register(on_did_dispose.clone());
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_did_change_selection,
            on_did_dispose,
            lifetime: CancellationTokenSource::new(None),
            states: RefCell::new(Vec::new()),
            next_state: Cell::new(0),
            disposed: Cell::new(false),
            selection: RefCell::new(selection),
            model,
            handle,
            proxy,
            focus,
        })
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("a navegação está viva enquanto um método dela roda")
    }

    /// `updateSelection(selection)`.
    pub fn update_selection(&self, selection: Option<Selection>) {
        if !self.disposed.get() {
            *self.selection.borrow_mut() = selection;
            self.on_did_change_selection.fire(());
        }
    }

    /// `this._states.delete(stateId)`.
    fn delete_state(&self, state_id: u32) -> bool {
        let mut states = self.states.borrow_mut();
        match states.iter().position(|id| *id == state_id) {
            Some(index) => {
                states.remove(index);
                true
            }
            None => false,
        }
    }

    /// `_handleError(error)`.
    fn handle_error(&self, error: &Error) {
        if !is_cancellation_error(error) {
            on_unexpected_error(error);
        }
    }
}

impl ICustomTextEditorNavigation for MainThreadCustomTextEditorNavigation {
    fn model(&self) -> Rc<TextModel> {
        self.model.clone()
    }

    fn selection(&self) -> Option<Selection> {
        *self.selection.borrow()
    }

    fn on_did_change_selection(&self) -> Event<()> {
        self.on_did_change_selection.event()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    fn reveal_range(&self, range: Range, selection: Option<Selection>, preserve_focus: bool, token: CancellationToken) -> Promise<()> {
        let this = self.this();
        async_fn(async move {
            if this.disposed.get() || token.is_cancellation_requested() {
                return Ok(());
            }
            if selection.is_some() {
                let states = std::mem::take(&mut *this.states.borrow_mut());
                for state_id in states {
                    this.proxy.rpc_release_custom_text_editor_view_state(&this.handle, state_id);
                }
            }
            let cancellation = CancellationTokenSource::new(Some(&this.lifetime.token()));
            let listener = token.on_cancellation_requested().listen({
                let cancellation = cancellation.clone();
                move |_| cancellation.cancel()
            });
            let outcome = match this.proxy.rpc_reveal_custom_text_editor_range(&this.handle, range, selection, preserve_focus, cancellation.token()).await {
                Ok(()) => errors::catch(|| {
                    if !preserve_focus && !cancellation.token().is_cancellation_requested() {
                        (this.focus)();
                    }
                }),
                Err(error) => Err(error),
            };
            if let Err(error) = outcome {
                this.handle_error(&error);
            }
            listener.dispose();
            cancellation.dispose();
            Ok(())
        })
    }

    fn capture_view_state(&self) -> DisposableRef {
        if self.disposed.get() {
            return lifecycle::none();
        }
        let this = self.this();
        let state_id = self.next_state.get();
        self.next_state.set(state_id + 1);
        self.states.borrow_mut().push(state_id);
        let captured = self.proxy.rpc_capture_custom_text_editor_view_state(&self.handle, state_id).catch({
            let this = this.clone();
            move |error| {
                this.delete_state(state_id);
                this.proxy.rpc_release_custom_text_editor_view_state(&this.handle, state_id);
                this.handle_error(&error);
            }
        });
        to_disposable(move || {
            let on_restored = this.clone();
            let on_error = this.clone();
            captured
                .then_promise(move |()| {
                    async_fn(async move {
                        if on_restored.delete_state(state_id) {
                            on_restored.proxy.rpc_restore_custom_text_editor_view_state(&on_restored.handle, state_id, on_restored.lifetime.token()).await?;
                        }
                        Ok(())
                    })
                })
                .catch(move |error| on_error.handle_error(&error));
        })
    }
}

impl IDisposable for MainThreadCustomTextEditorNavigation {
    fn dispose(&self) {
        if self.disposed.get() {
            return;
        }
        self.disposed.set(true);
        self.lifetime.dispose_with(true);
        self.states.borrow_mut().clear();
        self.proxy.rpc_dispose_custom_text_editor_navigation(&self.handle);
        self.on_did_dispose.fire(());
        self.base.dispose();
    }
}
