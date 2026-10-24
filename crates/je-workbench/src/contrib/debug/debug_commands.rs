//! Porte parcial de `src/vs/workbench/contrib/debug/browser/debugCommands.ts`: os identificadores dos comandos e os rótulos
//! que os renderizadores e as contribuições de menu referenciam.
//!
//! Divergência: o registro das ações e dos comandos (`registerCommandAndKeybindingRule`, as `Action2`, o `CallStackContext`
//! e `setDataBreakpointInfoResponse`) depende do `DebugService` e dos menus e entra com eles; aqui ficam só as constantes.

use je_platform::action::action::{LocalizedString, localize2};

pub const ADD_CONFIGURATION_ID: &str = "debug.addConfiguration";
pub const COPY_ADDRESS_ID: &str = "editor.debug.action.copyAddress";
pub const TOGGLE_BREAKPOINT_ID: &str = "editor.debug.action.toggleBreakpoint";
pub const TOGGLE_INLINE_BREAKPOINT_ID: &str = "editor.debug.action.toggleInlineBreakpoint";
pub const COPY_STACK_TRACE_ID: &str = "debug.copyStackTrace";
pub const REVERSE_CONTINUE_ID: &str = "workbench.action.debug.reverseContinue";
pub const STEP_BACK_ID: &str = "workbench.action.debug.stepBack";
pub const RESTART_SESSION_ID: &str = "workbench.action.debug.restart";
pub const TERMINATE_THREAD_ID: &str = "workbench.action.debug.terminateThread";
pub const STEP_OVER_ID: &str = "workbench.action.debug.stepOver";
pub const STEP_INTO_ID: &str = "workbench.action.debug.stepInto";
pub const STEP_INTO_TARGET_ID: &str = "workbench.action.debug.stepIntoTarget";
pub const STEP_OUT_ID: &str = "workbench.action.debug.stepOut";
pub const PAUSE_ID: &str = "workbench.action.debug.pause";
pub const DISCONNECT_ID: &str = "workbench.action.debug.disconnect";
pub const DISCONNECT_AND_SUSPEND_ID: &str = "workbench.action.debug.disconnectAndSuspend";
pub const STOP_ID: &str = "workbench.action.debug.stop";
pub const RESTART_FRAME_ID: &str = "workbench.action.debug.restartFrame";
pub const CONTINUE_ID: &str = "workbench.action.debug.continue";
pub const FOCUS_REPL_ID: &str = "workbench.debug.action.focusRepl";
pub const JUMP_TO_CURSOR_ID: &str = "debug.jumpToCursor";
pub const FOCUS_SESSION_ID: &str = "workbench.action.debug.focusProcess";
pub const SELECT_AND_START_ID: &str = "workbench.action.debug.selectandstart";
pub const SELECT_DEBUG_CONSOLE_ID: &str = "workbench.action.debug.selectDebugConsole";
pub const SELECT_DEBUG_SESSION_ID: &str = "workbench.action.debug.selectDebugSession";
pub const DEBUG_CONFIGURE_COMMAND_ID: &str = "workbench.action.debug.configure";
pub const DEBUG_START_COMMAND_ID: &str = "workbench.action.debug.start";
pub const DEBUG_RUN_COMMAND_ID: &str = "workbench.action.debug.run";
pub const EDIT_EXPRESSION_COMMAND_ID: &str = "debug.renameWatchExpression";
pub const COPY_WATCH_EXPRESSION_COMMAND_ID: &str = "debug.copyWatchExpression";
pub const SET_EXPRESSION_COMMAND_ID: &str = "debug.setWatchExpression";
pub const REMOVE_EXPRESSION_COMMAND_ID: &str = "debug.removeWatchExpression";
pub const NEXT_DEBUG_CONSOLE_ID: &str = "workbench.action.debug.nextConsole";
pub const PREV_DEBUG_CONSOLE_ID: &str = "workbench.action.debug.prevConsole";
pub const SHOW_LOADED_SCRIPTS_ID: &str = "workbench.action.debug.showLoadedScripts";
pub const CALLSTACK_TOP_ID: &str = "workbench.action.debug.callStackTop";
pub const CALLSTACK_BOTTOM_ID: &str = "workbench.action.debug.callStackBottom";
pub const CALLSTACK_UP_ID: &str = "workbench.action.debug.callStackUp";
pub const CALLSTACK_DOWN_ID: &str = "workbench.action.debug.callStackDown";
pub const ADD_TO_WATCH_ID: &str = "debug.addToWatchExpressions";
pub const COPY_EVALUATE_PATH_ID: &str = "debug.copyEvaluatePath";
pub const COPY_VALUE_ID: &str = "workbench.debug.viewlet.action.copyValue";
pub const BREAK_WHEN_VALUE_CHANGES_ID: &str = "debug.breakWhenValueChanges";
pub const BREAK_WHEN_VALUE_IS_ACCESSED_ID: &str = "debug.breakWhenValueIsAccessed";
pub const BREAK_WHEN_VALUE_IS_READ_ID: &str = "debug.breakWhenValueIsRead";
pub const TOGGLE_EXCEPTION_BREAKPOINTS_ID: &str = "debug.toggleExceptionBreakpoints";

pub const ATTACH_TO_CURRENT_CODE_RENDERER: &str = "debug.attachToCurrentCodeRenderer";

pub const DEBUG_QUICK_ACCESS_PREFIX: &str = "debug ";
pub const DEBUG_CONSOLE_QUICK_ACCESS_PREFIX: &str = "debug consoles ";

/// `DEBUG_COMMAND_CATEGORY`.
pub fn debug_command_category() -> LocalizedString {
    localize2("debug", "Debug")
}

/// `COPY_EVALUATE_PATH_LABEL`.
pub fn copy_evaluate_path_label() -> LocalizedString {
    localize2("copyAsExpression", "Copy as Expression")
}

/// `COPY_VALUE_LABEL`.
pub fn copy_value_label() -> LocalizedString {
    localize2("copyValue", "Copy Value")
}

/// `COPY_ADDRESS_LABEL`.
pub fn copy_address_label() -> LocalizedString {
    localize2("copyAddress", "Copy Address")
}

/// `ADD_TO_WATCH_LABEL`.
pub fn add_to_watch_label() -> LocalizedString {
    localize2("addToWatchExpressions", "Add to Watch")
}
