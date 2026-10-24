//! Porte de `src/vs/workbench/api/test/browser/mainThreadCustomTextEditorNavigation.test.ts` (suíte
//! `MainThreadCustomTextEditorNavigation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard` criado no início de cada caso; o `store.add(...)` é o
//!   `guard.add_rc(...)`. Os casos `async` rodam em `run_with_faked_timers` (o `timeout(0)` é a macrotarefa do relógio
//!   virtual).
//! - **`mock<ExtHostCustomEditorsShape>()` com `override`** é o [`CustomEditorsProxy`]: os cinco métodos sobrescritos
//!   fazem o que o original faz e os demais falham com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`.
//!   O `calls: unknown[]` é a lista de [`Call`] (cada literal `{ handle, range, selection, preserveFocus }`,
//!   `{ restore, stateId }`, `{ release, stateId }` e `{ dispose }` é uma variante), comparada com `deepStrictEqual`.
//! - **`createTextModel('hello')`** é o `create_text_model` dos testes do editor; o `TextModel` tem `dispose` próprio, que
//!   o guarda chama ao fim.
//! - **`DeferredPromise<void>`** do `captures` é um `Rc<DeferredPromise<()>>` (o `.p` é o campo `promise`); `reveal` é a
//!   promessa que o `$revealCustomTextEditorRange` espera, que `setReveal` troca.
//! - **`() => focuses++`** é um fecho sobre um `Cell<u32>`.
//! - **`await pending.complete()`** é o `await` da promessa que `complete` devolve.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_workbench::api::browser::main_thread_custom_text_editor_navigation::MainThreadCustomTextEditorNavigation;
use je_workbench::api::common::ext_host_protocol::{
    CreateCustomDocumentResult, CustomEditorDiffInitData, CustomEditorResolveInitData, CustomEditorSideBySideDiffInitData, CustomEditorSideBySideDiffWebviewHandles,
    CustomTextEditorNavigationResult, EditorGroupColumn, ExtHostCustomEditorsShape,
};
use je_workbench::contrib::custom_editor::custom_text_editor_navigation::ICustomTextEditorNavigation;
use test_text_model::create_text_model;

/// Um elemento de `calls`.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    /// `{ handle, range, selection, preserveFocus }`.
    Reveal { handle: String, range: Range, selection: Option<Selection>, preserve_focus: bool },
    /// `{ restore: handle, stateId }`.
    Restore { handle: String, state_id: u32 },
    /// `{ release: handle, stateId }`.
    Release { handle: String, state_id: u32 },
    /// `{ dispose: handle }`.
    Dispose { handle: String },
}

/// O objeto do `mock<ExtHostCustomEditorsShape>()` de `createNavigation`, com o estado que o fecho compartilha.
struct CustomEditorsProxy {
    calls: Rc<RefCell<Vec<Call>>>,
    captures: Rc<RefCell<HashMap<u32, Rc<DeferredPromise<()>>>>>,
    reveal_token: Rc<RefCell<Option<CancellationToken>>>,
    reveal: Rc<RefCell<Promise<()>>>,
}

impl ExtHostCustomEditorsShape for CustomEditorsProxy {
    fn rpc_resolve_custom_text_editor_navigation(&self, _handle: &str, _view_type: &str, _resource: &URI, _token: CancellationToken) -> Promise<Option<CustomTextEditorNavigationResult>> {
        panic!("$resolveCustomTextEditorNavigation is not a function")
    }

    fn rpc_dispose_custom_text_editor_navigation(&self, handle: &str) {
        self.calls.borrow_mut().push(Call::Dispose { handle: handle.to_string() });
    }

    fn rpc_reveal_custom_text_editor_range(&self, handle: &str, range: Range, selection: Option<Selection>, preserve_focus: bool, token: CancellationToken) -> Promise<()> {
        self.calls.borrow_mut().push(Call::Reveal { handle: handle.to_string(), range, selection, preserve_focus });
        *self.reveal_token.borrow_mut() = Some(token);
        let reveal = self.reveal.borrow().clone();
        async_fn(async move {
            reveal.await?;
            Ok(())
        })
    }

    fn rpc_capture_custom_text_editor_view_state(&self, _handle: &str, state_id: u32) -> Promise<()> {
        let capture = Rc::new(DeferredPromise::<()>::new());
        let promise = capture.promise.clone();
        self.captures.borrow_mut().insert(state_id, capture);
        promise
    }

    fn rpc_restore_custom_text_editor_view_state(&self, handle: &str, state_id: u32, _token: CancellationToken) -> Promise<()> {
        let calls = self.calls.clone();
        let handle = handle.to_string();
        async_fn(async move {
            calls.borrow_mut().push(Call::Restore { handle, state_id });
            Ok(())
        })
    }

    fn rpc_release_custom_text_editor_view_state(&self, handle: &str, state_id: u32) {
        self.calls.borrow_mut().push(Call::Release { handle: handle.to_string(), state_id });
    }

    fn rpc_resolve_custom_editor(
        &self,
        _resource: &URI,
        _new_webview_handle: &str,
        _view_type: &str,
        _init_data: CustomEditorResolveInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        panic!("$resolveCustomEditor is not a function")
    }

    fn rpc_resolve_custom_editor_inline_diff(
        &self,
        _original_resource: &URI,
        _modified_resource: &URI,
        _new_webview_handle: &str,
        _view_type: &str,
        _init_data: CustomEditorDiffInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        panic!("$resolveCustomEditorInlineDiff is not a function")
    }

    fn rpc_resolve_custom_editor_side_by_side_diff(
        &self,
        _original_resource: &URI,
        _modified_resource: &URI,
        _webview_handles: CustomEditorSideBySideDiffWebviewHandles,
        _view_type: &str,
        _init_data: CustomEditorSideBySideDiffInitData,
        _position: EditorGroupColumn,
        _cancellation: CancellationToken,
    ) -> Promise<()> {
        panic!("$resolveCustomEditorSideBySideDiff is not a function")
    }

    fn rpc_create_custom_document(
        &self,
        _resource: &URI,
        _view_type: &str,
        _backup_id: Option<String>,
        _untitled_document_data: Option<VSBuffer>,
        _cancellation: CancellationToken,
    ) -> Promise<CreateCustomDocumentResult> {
        panic!("$createCustomDocument is not a function")
    }

    fn rpc_dispose_custom_document(&self, _resource: &URI, _view_type: &str) -> Promise<()> {
        panic!("$disposeCustomDocument is not a function")
    }

    fn rpc_undo(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _is_dirty: bool) -> Promise<()> {
        panic!("$undo is not a function")
    }

    fn rpc_redo(&self, _resource: &URI, _view_type: &str, _edit_id: u32, _is_dirty: bool) -> Promise<()> {
        panic!("$redo is not a function")
    }

    fn rpc_revert(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$revert is not a function")
    }

    fn rpc_dispose_edits(&self, _resource: &URI, _view_type: &str, _edit_ids: Vec<u32>) -> Result<(), Error> {
        panic!("$disposeEdits is not a function")
    }

    fn rpc_on_save(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$onSave is not a function")
    }

    fn rpc_on_save_as(&self, _resource: &URI, _view_type: &str, _target_resource: &URI, _cancellation: CancellationToken) -> Promise<()> {
        panic!("$onSaveAs is not a function")
    }

    fn rpc_backup(&self, _resource: &URI, _view_type: &str, _cancellation: CancellationToken) -> Promise<String> {
        panic!("$backup is not a function")
    }

    fn rpc_on_move_custom_editor(&self, _handle: &str, _new_resource: &URI, _view_type: &str) -> Promise<()> {
        panic!("$onMoveCustomEditor is not a function")
    }
}

/// O que `createNavigation()` devolve.
struct Fixture {
    navigation: Rc<MainThreadCustomTextEditorNavigation>,
    calls: Rc<RefCell<Vec<Call>>>,
    captures: Rc<RefCell<HashMap<u32, Rc<DeferredPromise<()>>>>>,
    reveal_token: Rc<RefCell<Option<CancellationToken>>>,
    reveal: Rc<RefCell<Promise<()>>>,
    focuses: Rc<Cell<u32>>,
}

impl Fixture {
    /// `fixture.calls`.
    fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    /// `fixture.captures.get(stateId)!`.
    fn capture(&self, state_id: u32) -> Rc<DeferredPromise<()>> {
        self.captures.borrow().get(&state_id).cloned().expect("captura")
    }
}

fn range() -> Range {
    Range::new(1, 1, 1, 3)
}

fn panel() -> String {
    "panel".to_string()
}

/// `createNavigation()`.
fn create_navigation(guard: &LeakGuard) -> Fixture {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let captures = Rc::new(RefCell::new(HashMap::new()));
    let reveal_token = Rc::new(RefCell::new(None));
    let reveal = Rc::new(RefCell::new(Promise::resolved(())));
    let focuses = Rc::new(Cell::new(0));
    let proxy: Rc<dyn ExtHostCustomEditorsShape> =
        Rc::new(CustomEditorsProxy { calls: calls.clone(), captures: captures.clone(), reveal_token: reveal_token.clone(), reveal: reveal.clone() });
    let model = create_text_model("hello");
    {
        let model = model.clone();
        guard.add(to_disposable(move || model.dispose()));
    }
    let focus_counter = focuses.clone();
    let navigation = guard.add_rc(MainThreadCustomTextEditorNavigation::new(model, None, panel(), proxy, Rc::new(move || focus_counter.set(focus_counter.get() + 1))));
    Fixture { navigation, calls, captures, reveal_token, reveal, focuses }
}

#[test]
fn preview_preserves_focus_and_reveal_focuses_the_originating_panel() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        fixture.navigation.reveal_range(range(), None, true, CancellationToken::None).await.unwrap();
        assert_eq!(fixture.focuses.get(), 0);
        let selection = Selection::new(1, 1, 1, 3);
        fixture.navigation.reveal_range(range(), Some(selection), false, CancellationToken::None).await.unwrap();
        assert_eq!(
            fixture.calls(),
            vec![
                Call::Reveal { handle: panel(), range: range(), selection: None, preserve_focus: true },
                Call::Reveal { handle: panel(), range: range(), selection: Some(selection), preserve_focus: false },
            ]
        );
        assert_eq!(fixture.focuses.get(), 1);
    });
}

#[test]
fn canceled_requests_do_not_reveal_and_disposal_cancels_in_flight_reveal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        fixture.navigation.reveal_range(range(), None, true, CancellationToken::Cancelled).await.unwrap();
        assert_eq!(fixture.calls(), Vec::<Call>::new());
        let pending = Rc::new(DeferredPromise::<()>::new());
        *fixture.reveal.borrow_mut() = pending.promise.clone();
        let reveal = fixture.navigation.reveal_range(range(), None, false, CancellationToken::None);
        fixture.navigation.dispose();
        assert_eq!(fixture.reveal_token.borrow().as_ref().map(CancellationToken::is_cancellation_requested), Some(true));
        pending.complete(()).await.unwrap();
        reveal.await.unwrap();
        assert_eq!(fixture.focuses.get(), 0);
    });
}

#[test]
fn restore_waits_for_asynchronous_capture() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        let state = fixture.navigation.capture_view_state();
        state.dispose();
        assert_eq!(fixture.calls(), Vec::<Call>::new());
        fixture.capture(0).complete(()).await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(fixture.calls(), vec![Call::Restore { handle: panel(), state_id: 0 }]);
    });
}

#[test]
fn committing_a_heading_releases_captures_instead_of_restoring_preview_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        let state = fixture.navigation.capture_view_state();
        fixture.navigation.reveal_range(range(), Some(Selection::new(1, 1, 1, 1)), true, CancellationToken::None).await.unwrap();
        fixture.capture(0).complete(()).await.unwrap();
        state.dispose();
        timeout(0).await.unwrap();
        assert_eq!(
            fixture.calls(),
            vec![
                Call::Release { handle: panel(), state_id: 0 },
                Call::Reveal { handle: panel(), range: range(), selection: Some(Selection::new(1, 1, 1, 1)), preserve_focus: true },
            ]
        );
    });
}

#[test]
fn disposal_invalidates_captures_and_prevents_late_restoration() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        let state = fixture.navigation.capture_view_state();
        fixture.navigation.dispose();
        fixture.capture(0).complete(()).await.unwrap();
        state.dispose();
        timeout(0).await.unwrap();
        assert_eq!(fixture.calls(), vec![Call::Dispose { handle: panel() }]);
    });
}

#[test]
fn a_restore_waiting_for_capture_cannot_overwrite_a_newer_heading_navigation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        let state = fixture.navigation.capture_view_state();
        state.dispose();
        fixture.navigation.reveal_range(range(), Some(Selection::new(1, 1, 1, 1)), true, CancellationToken::None).await.unwrap();
        fixture.capture(0).complete(()).await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(
            fixture.calls(),
            vec![
                Call::Release { handle: panel(), state_id: 0 },
                Call::Reveal { handle: panel(), range: range(), selection: Some(Selection::new(1, 1, 1, 1)), preserve_focus: true },
            ]
        );
    });
}

#[test]
fn a_restore_waiting_for_capture_is_invalidated_when_the_editor_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = create_navigation(&guard);
    run_with_faked_timers(async move {
        let state = fixture.navigation.capture_view_state();
        state.dispose();
        fixture.navigation.dispose();
        fixture.capture(0).complete(()).await.unwrap();
        timeout(0).await.unwrap();
        assert_eq!(fixture.calls(), vec![Call::Dispose { handle: panel() }]);
    });
}

#[test]
fn selection_changes_are_instance_local_and_stop_after_disposal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let first = create_navigation(&guard);
    let second = create_navigation(&guard);
    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    guard.add(first.navigation.on_did_change_selection().listen(move |_| counter.set(counter.get() + 1)));
    let selection = Selection::new(1, 2, 1, 4);
    first.navigation.update_selection(Some(selection));
    assert_eq!(first.navigation.selection(), Some(selection));
    assert_eq!(second.navigation.selection(), None);
    first.navigation.update_selection(None);
    first.navigation.dispose();
    first.navigation.update_selection(Some(selection));
    assert_eq!(changes.get(), 2);
}
