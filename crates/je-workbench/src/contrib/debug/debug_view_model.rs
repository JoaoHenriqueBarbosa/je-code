//! Porte de `src/vs/workbench/contrib/debug/common/debugViewModel.ts`: o `ViewModel` da depuração (a sessão, a thread e o
//! quadro de pilha em foco, a expressão selecionada, a visualização escolhida) e as chaves de contexto que ele mantém.
//!
//! Notas de tradução:
//! - `IContextKeyService.bufferChangeEvents` + `RawContextKey.bindTo` são o serviço de chaves de contexto do je-platform
//!   (`create_key`); as constantes `CONTEXT_*` são funções que devolvem a [`RawContextKey`] (registrada sem descrição
//!   central, `ContextKeyMeta::Hide`) e [`context_key`] devolve o nome.
//! - O foco guarda `Rc` concretos ([`StackFrame`], [`Thread`], sessão completa); a identidade que o original compara com `!==`
//!   é `Rc::ptr_eq` (e `None` com `None` é igual).
//! - `WeakMap<IExpression, IExpression>` (`visualized`) é um mapa pelo `getId()` da expressão original.
//! - O `IViewModel` mínimo de `debug.rs` (usado pelo `MainThreadDebugService`) NÃO é implementado aqui: ele devolve
//!   `Rc<dyn IThread>`/`Rc<dyn IStackFrame>`, e o `DebugService` real (não portado) é quem faria a ponte.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

use super::debug::IDebugSession;
use super::debug_model::{StackFrame, Thread, has_multiple_stack_frames};
use super::debug_types::{IDebugSessionFull, IExpression, IExpressionContainer, capability};
use super::debug_utils::is_session_attach;

macro_rules! context_keys {
    ($($function:ident, $name:expr;)*) => {
        $(
            /// A chave de contexto de mesmo nome do original.
            pub fn $function() -> RawContextKey {
                RawContextKey::new($name, false, ContextKeyMeta::Hide)
            }
        )*
    };
}

context_keys! {
    context_expression_selected, "expressionSelected";
    context_loaded_scripts_supported, "loadedScriptsSupported";
    context_step_back_supported, "stepBackSupported";
    context_focused_session_is_attach, "focusedSessionIsAttach";
    context_focused_session_is_no_debug, "focusedSessionIsNoDebug";
    context_restart_frame_supported, "restartFrameSupported";
    context_step_into_targets_supported, "stepIntoTargetsSupported";
    context_jump_to_cursor_supported, "jumpToCursorSupported";
    context_set_variable_supported, "debugSetVariableSupported";
    context_set_data_breakpoint_bytes_supported, "debugSetDataBreakpointAddressSupported";
    context_set_expression_supported, "debugSetExpressionSupported";
    context_multi_session_debug, "multiSessionDebug";
    context_terminate_debuggee_supported, "terminateDebuggeeSupported";
    context_suspend_debuggee_supported, "suspendDebuggeeSupported";
    context_terminate_threads_supported, "terminateThreadsSupported";
    context_disassemble_request_supported, "disassembleRequestSupported";
    context_focused_stack_frame_has_instruction_pointer_reference, "focusedStackFrameHasInstructionReference";
    context_thread_has_multiple_stack_frames, "threadHasMultipleStackFrames";
}

/// `CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES.key`.
pub const CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY: &str = "threadHasMultipleStackFrames";

/// `{ expression, settingWatch }`.
#[derive(Clone)]
pub struct SelectedExpression {
    pub expression: Rc<dyn IExpression>,
    pub setting_watch: bool,
}

/// `{ thread, explicit, session }` de `onDidFocusThread`.
#[derive(Clone)]
pub struct ThreadFocus {
    pub thread: Option<Rc<Thread>>,
    pub explicit: bool,
    pub session: Option<Rc<dyn IDebugSessionFull>>,
}

/// `{ stackFrame, explicit, session }` de `onDidFocusStackFrame`.
#[derive(Clone)]
pub struct StackFrameFocus {
    pub stack_frame: Option<Rc<StackFrame>>,
    pub explicit: bool,
    pub session: Option<Rc<dyn IDebugSessionFull>>,
}

/// `{ original, replacement }` de `onDidChangeVisualization`.
#[derive(Clone)]
pub struct VisualizationChange {
    pub original: Rc<dyn IExpression>,
    pub replacement: Rc<dyn IExpression>,
}

fn same<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b)),
        _ => false,
    }
}

/// `ViewModel`.
pub struct ViewModel {
    store: Rc<DisposableStore>,
    pub first_session_start: Cell<bool>,
    focused_stack_frame: RefCell<Option<Rc<StackFrame>>>,
    focused_session: RefCell<Option<Rc<dyn IDebugSessionFull>>>,
    focused_thread: RefCell<Option<Rc<Thread>>>,
    selected_expression: RefCell<Option<SelectedExpression>>,
    on_did_focus_session: Rc<Emitter<Option<Rc<dyn IDebugSessionFull>>>>,
    on_did_focus_thread: Rc<Emitter<ThreadFocus>>,
    on_did_focus_stack_frame: Rc<Emitter<StackFrameFocus>>,
    on_did_select_expression: Rc<Emitter<Option<SelectedExpression>>>,
    on_did_evaluate_lazy_expression: Rc<Emitter<Rc<dyn IExpressionContainer>>>,
    on_will_update_views: Rc<Emitter<()>>,
    on_did_change_visualization: Rc<Emitter<VisualizationChange>>,
    visualized: RefCell<HashMap<String, Rc<dyn IExpression>>>,
    preferred_visualizers: RefCell<HashMap<String, String>>,
    context_key_service: Rc<dyn IContextKeyService>,
    expression_selected_context_key: Rc<dyn IContextKey>,
    loaded_scripts_supported_context_key: Rc<dyn IContextKey>,
    step_back_supported_context_key: Rc<dyn IContextKey>,
    focused_session_is_attach: Rc<dyn IContextKey>,
    focused_session_is_no_debug: Rc<dyn IContextKey>,
    restart_frame_supported_context_key: Rc<dyn IContextKey>,
    step_into_targets_supported: Rc<dyn IContextKey>,
    jump_to_cursor_supported: Rc<dyn IContextKey>,
    set_variable_supported: Rc<dyn IContextKey>,
    set_data_breakpoint_at_byte_supported: Rc<dyn IContextKey>,
    set_expression_supported: Rc<dyn IContextKey>,
    multi_session_debug: Rc<dyn IContextKey>,
    terminate_debuggee_supported: Rc<dyn IContextKey>,
    suspend_debuggee_supported: Rc<dyn IContextKey>,
    terminate_threads_supported: Rc<dyn IContextKey>,
    disassemble_request_supported: Rc<dyn IContextKey>,
    focused_stack_frame_has_instruction_pointer_reference: Rc<dyn IContextKey>,
    thread_has_multiple_stack_frames: Rc<dyn IContextKey>,
}

fn emitter<T: 'static>(store: &Rc<DisposableStore>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    store.add(emitter.clone());
    emitter
}

impl ViewModel {
    pub fn new(context_key_service: Rc<dyn IContextKeyService>) -> Rc<Self> {
        let store = DisposableStore::new();
        let service = context_key_service.clone();
        let bind = |key: RawContextKey| key.bind_to(&*service);
        let keys: RefCell<Option<Vec<Rc<dyn IContextKey>>>> = RefCell::new(None);
        context_key_service.buffer_change_events(&|| {
            *keys.borrow_mut() = Some(vec![
                bind(context_expression_selected()),
                bind(context_loaded_scripts_supported()),
                bind(context_step_back_supported()),
                bind(context_focused_session_is_attach()),
                bind(context_focused_session_is_no_debug()),
                bind(context_restart_frame_supported()),
                bind(context_step_into_targets_supported()),
                bind(context_jump_to_cursor_supported()),
                bind(context_set_variable_supported()),
                bind(context_set_data_breakpoint_bytes_supported()),
                bind(context_set_expression_supported()),
                bind(context_multi_session_debug()),
                bind(context_terminate_debuggee_supported()),
                bind(context_suspend_debuggee_supported()),
                bind(context_terminate_threads_supported()),
                bind(context_disassemble_request_supported()),
                bind(context_focused_stack_frame_has_instruction_pointer_reference()),
                bind(context_thread_has_multiple_stack_frames()),
            ]);
        });
        let mut keys = keys.into_inner().expect("chaves de contexto").into_iter();
        let mut next = || keys.next().expect("chave de contexto");

        Rc::new(Self {
            first_session_start: Cell::new(true),
            focused_stack_frame: RefCell::new(None),
            focused_session: RefCell::new(None),
            focused_thread: RefCell::new(None),
            selected_expression: RefCell::new(None),
            on_did_focus_session: emitter(&store),
            on_did_focus_thread: emitter(&store),
            on_did_focus_stack_frame: emitter(&store),
            on_did_select_expression: emitter(&store),
            on_did_evaluate_lazy_expression: emitter(&store),
            on_will_update_views: emitter(&store),
            on_did_change_visualization: emitter(&store),
            visualized: RefCell::new(HashMap::new()),
            preferred_visualizers: RefCell::new(HashMap::new()),
            context_key_service,
            expression_selected_context_key: next(),
            loaded_scripts_supported_context_key: next(),
            step_back_supported_context_key: next(),
            focused_session_is_attach: next(),
            focused_session_is_no_debug: next(),
            restart_frame_supported_context_key: next(),
            step_into_targets_supported: next(),
            jump_to_cursor_supported: next(),
            set_variable_supported: next(),
            set_data_breakpoint_at_byte_supported: next(),
            set_expression_supported: next(),
            multi_session_debug: next(),
            terminate_debuggee_supported: next(),
            suspend_debuggee_supported: next(),
            terminate_threads_supported: next(),
            disassemble_request_supported: next(),
            focused_stack_frame_has_instruction_pointer_reference: next(),
            thread_has_multiple_stack_frames: next(),
            store,
        })
    }

    pub fn get_id(&self) -> String {
        "root".to_owned()
    }

    pub fn focused_session(&self) -> Option<Rc<dyn IDebugSessionFull>> {
        self.focused_session.borrow().clone()
    }

    pub fn focused_thread(&self) -> Option<Rc<Thread>> {
        self.focused_thread.borrow().clone()
    }

    pub fn focused_stack_frame(&self) -> Option<Rc<StackFrame>> {
        self.focused_stack_frame.borrow().clone()
    }

    pub fn set_focus(&self, stack_frame: Option<Rc<StackFrame>>, thread: Option<Rc<Thread>>, session: Option<Rc<dyn IDebugSessionFull>>, explicit: bool) {
        let should_emit_for_stack_frame = !same(&self.focused_stack_frame.borrow(), &stack_frame);
        let should_emit_for_session = !same(&self.focused_session.borrow(), &session);
        let should_emit_for_thread = !same(&self.focused_thread.borrow(), &thread);

        *self.focused_stack_frame.borrow_mut() = stack_frame.clone();
        *self.focused_thread.borrow_mut() = thread.clone();
        *self.focused_session.borrow_mut() = session.clone();

        self.context_key_service.buffer_change_events(&|| {
            let supports = |key: &str| Dynamic::Bool(session.as_ref().is_some_and(|s| capability(&s.capabilities(), key)));
            self.loaded_scripts_supported_context_key.set(supports("supportsLoadedSourcesRequest"));
            self.step_back_supported_context_key.set(supports("supportsStepBack"));
            self.restart_frame_supported_context_key.set(supports("supportsRestartFrame"));
            self.step_into_targets_supported.set(supports("supportsStepInTargetsRequest"));
            self.jump_to_cursor_supported.set(supports("supportsGotoTargetsRequest"));
            self.set_variable_supported.set(supports("supportsSetVariable"));
            self.set_data_breakpoint_at_byte_supported.set(supports("supportsDataBreakpointBytes"));
            self.set_expression_supported.set(supports("supportsSetExpression"));
            self.terminate_debuggee_supported.set(supports("supportTerminateDebuggee"));
            self.suspend_debuggee_supported.set(supports("supportSuspendDebuggee"));
            self.terminate_threads_supported.set(supports("supportsTerminateThreadsRequest"));
            self.disassemble_request_supported.set(supports("supportsDisassembleRequest"));
            self.focused_stack_frame_has_instruction_pointer_reference
                .set(Dynamic::Bool(stack_frame.as_ref().is_some_and(|f| f.instruction_pointer_reference.as_ref().is_some_and(|r| !r.is_empty()))));
            self.update_focused_thread_has_multiple_stack_frames();
            let attach = session.as_ref().is_some_and(|s| {
                let session: Rc<dyn IDebugSession> = s.clone();
                is_session_attach(&session)
            });
            self.focused_session_is_attach.set(Dynamic::Bool(attach));
            self.focused_session_is_no_debug.set(Dynamic::Bool(
                session.as_ref().is_some_and(|s| s.configuration().get("noDebug").is_some_and(|v| !matches!(v, serde_json::Value::Null | serde_json::Value::Bool(false)))),
            ));
        });

        if should_emit_for_session {
            self.on_did_focus_session.fire(session.clone());
        }

        // should not call onDidFocusThread if onDidFocusStackFrame is called.
        if should_emit_for_stack_frame {
            self.on_did_focus_stack_frame.fire(StackFrameFocus { stack_frame, explicit, session });
        } else if should_emit_for_thread {
            self.on_did_focus_thread.fire(ThreadFocus { thread, explicit, session });
        }
    }

    pub fn update_focused_thread_has_multiple_stack_frames(&self) {
        let thread = self.focused_thread.borrow().clone();
        self.thread_has_multiple_stack_frames.set(Dynamic::Bool(has_multiple_stack_frames(thread.as_deref())));
    }

    pub fn on_did_focus_session(&self) -> Event<Option<Rc<dyn IDebugSessionFull>>> {
        self.on_did_focus_session.event()
    }

    pub fn on_did_focus_thread(&self) -> Event<ThreadFocus> {
        self.on_did_focus_thread.event()
    }

    pub fn on_did_focus_stack_frame(&self) -> Event<StackFrameFocus> {
        self.on_did_focus_stack_frame.event()
    }

    pub fn on_did_change_visualization(&self) -> Event<VisualizationChange> {
        self.on_did_change_visualization.event()
    }

    pub fn get_selected_expression(&self) -> Option<SelectedExpression> {
        self.selected_expression.borrow().clone()
    }

    pub fn set_selected_expression(&self, expression: Option<Rc<dyn IExpression>>, setting_watch: bool) {
        let selected = expression.clone().map(|expression| SelectedExpression { expression, setting_watch });
        *self.selected_expression.borrow_mut() = selected.clone();
        self.expression_selected_context_key.set(Dynamic::Bool(expression.is_some()));
        self.on_did_select_expression.fire(selected);
    }

    pub fn on_did_select_expression(&self) -> Event<Option<SelectedExpression>> {
        self.on_did_select_expression.event()
    }

    pub fn on_did_evaluate_lazy_expression(&self) -> Event<Rc<dyn IExpressionContainer>> {
        self.on_did_evaluate_lazy_expression.event()
    }

    pub fn update_views(&self) {
        self.on_will_update_views.fire(());
    }

    pub fn on_will_update_views(&self) -> Event<()> {
        self.on_will_update_views.event()
    }

    pub fn is_multi_session_view(&self) -> bool {
        matches!(self.multi_session_debug.get(), Dynamic::Bool(true))
    }

    pub fn set_multi_session_view(&self, is_multi_session_view: bool) {
        self.multi_session_debug.set(Dynamic::Bool(is_multi_session_view));
    }

    /// `setVisualizedExpression(original, visualized)`: `visualized` leva o `treeId`.
    pub fn set_visualized_expression(&self, original: Rc<dyn IExpression>, visualized: Option<(Rc<dyn IExpression>, String)>) {
        let original_id = original.get_id();
        let current = self.visualized.borrow().get(&original_id).cloned().unwrap_or_else(|| original.clone());
        let key = Self::get_preferred_visualized_key(&*original);
        match &visualized {
            Some((expression, tree_id)) => {
                self.visualized.borrow_mut().insert(original_id, expression.clone());
                self.preferred_visualizers.borrow_mut().insert(key, tree_id.clone());
            }
            None => {
                self.visualized.borrow_mut().remove(&original_id);
                self.preferred_visualizers.borrow_mut().remove(&key);
            }
        }
        self.on_did_change_visualization
            .fire(VisualizationChange { original: current, replacement: visualized.map(|(e, _)| e).unwrap_or(original) });
    }

    /// `getVisualizedExpression(expression)`: a expressão visualizada ou o id da árvore preferida.
    pub fn get_visualized_expression(&self, expression: &Rc<dyn IExpression>) -> Option<Result<Rc<dyn IExpression>, String>> {
        if let Some(visualized) = self.visualized.borrow().get(&expression.get_id()) {
            return Some(Ok(visualized.clone()));
        }
        self.preferred_visualizers.borrow().get(&Self::get_preferred_visualized_key(&**expression)).cloned().map(Err)
    }

    pub fn evaluate_lazy_expression(self: &Rc<Self>, expression: Rc<dyn IExpressionContainer>) -> Promise<()> {
        let this = self.clone();
        je_base::async_::async_fn(async move {
            expression.evaluate_lazy().await?;
            this.on_did_evaluate_lazy_expression.fire(expression);
            Ok(())
        })
    }

    fn get_preferred_visualized_key(expr: &dyn IExpression) -> String {
        let joined = [expr.name(), expr.expr_type().unwrap_or_default(), expr.memory_reference().is_some_and(|r| !r.is_empty()).to_string()].join("\0");
        serde_json::to_string(&joined).expect("texto serializável")
    }
}

impl IDisposable for ViewModel {
    fn dispose(&self) {
        self.store.dispose();
    }
}
