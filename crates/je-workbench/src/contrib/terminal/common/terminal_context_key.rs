//! Porte parcial de `src/vs/workbench/contrib/terminal/common/terminalContextKey.ts`: as chaves `terminalShellType`
//! (`TerminalContextKeys.shellType`, que o `TerminalInstanceService` liga ao serviço de chaves de contexto), `terminalIsOpen`,
//! `terminalCount`, `terminalProcessSupported` e `terminalHasBeenCreated` (que o `TerminalService` liga).
//!
//! O resto do arquivo (as demais chaves, as expressões negadas e o `shouldShowViewInlineActions`) entra quando um consumidor
//! pedir.
//!
//! Divergências:
//! - **`export const shellType = new RawContextKey(...)`.** O `RawContextKey` do porte registra a chave numa lista global a cada
//!   `new`; para o registro acontecer uma vez só (como o módulo do original, avaliado uma vez), a chave é criada na primeira
//!   leitura e guardada por thread. [`shell_type_bind_to`] é o `TerminalContextKeys.shellType.bindTo(service)`.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};

/// `TerminalContextKeyStrings.ShellType`.
pub const SHELL_TYPE_CONTEXT_KEY: &str = "terminalShellType";

thread_local! {
    /// `TerminalContextKeys.shellType`: the shell type of the active terminal, this is set if the type can be detected.
    static SHELL_TYPE: RawContextKey = RawContextKey::new(
        SHELL_TYPE_CONTEXT_KEY,
        Dynamic::Undefined,
        ContextKeyMeta::Info {
            ty: "string".to_owned(),
            description: localize("terminalShellTypeContextKey", "The shell type of the active terminal, this is set if the type can be detected.", &[]),
        },
    );
}

/// `TerminalContextKeys.shellType.bindTo(contextKeyService)`.
pub fn shell_type_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    SHELL_TYPE.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeyStrings.IsOpen`.
pub const IS_OPEN_CONTEXT_KEY: &str = "terminalIsOpen";

/// `TerminalContextKeyStrings.Count`.
pub const COUNT_CONTEXT_KEY: &str = "terminalCount";

/// `TerminalContextKeyStrings.ProcessSupported`.
pub const PROCESS_SUPPORTED_CONTEXT_KEY: &str = "terminalProcessSupported";

/// `TerminalContextKeyStrings.TerminalHasBeenCreated`.
pub const TERMINAL_HAS_BEEN_CREATED_CONTEXT_KEY: &str = "terminalHasBeenCreated";

thread_local! {
    /// `TerminalContextKeys.isOpen`: whether the terminal view is open (`RawContextKey<boolean>`, escondida da lista central).
    static IS_OPEN: RawContextKey = RawContextKey::new(IS_OPEN_CONTEXT_KEY, Dynamic::Bool(false), ContextKeyMeta::Hide);

    /// `TerminalContextKeys.count`: the current number of terminals.
    static COUNT: RawContextKey = RawContextKey::new(
        COUNT_CONTEXT_KEY,
        Dynamic::Number(0.0),
        ContextKeyMeta::Description(localize("terminalCountContextKey", "The current number of terminals.", &[])),
    );

    /// `TerminalContextKeys.processSupported`: whether terminal processes can be launched in the current workspace.
    static PROCESS_SUPPORTED: RawContextKey = RawContextKey::new(
        PROCESS_SUPPORTED_CONTEXT_KEY,
        Dynamic::Bool(false),
        ContextKeyMeta::Description(localize("terminalProcessSupportedContextKey", "Whether terminal processes can be launched in the current workspace.", &[])),
    );

    /// `TerminalContextKeys.terminalHasBeenCreated`: whether a terminal has been created in this window (escondida da lista central).
    static TERMINAL_HAS_BEEN_CREATED: RawContextKey = RawContextKey::new(TERMINAL_HAS_BEEN_CREATED_CONTEXT_KEY, Dynamic::Bool(false), ContextKeyMeta::Hide);
}

/// `TerminalContextKeys.isOpen.bindTo(service)`.
pub fn is_open_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    IS_OPEN.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeys.count.bindTo(service)`.
pub fn count_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    COUNT.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeys.processSupported.bindTo(service)`.
pub fn process_supported_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    PROCESS_SUPPORTED.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeys.terminalHasBeenCreated.bindTo(service)`.
pub fn terminal_has_been_created_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    TERMINAL_HAS_BEEN_CREATED.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeyStrings.GroupCount`.
pub const GROUP_COUNT_CONTEXT_KEY: &str = "terminalGroupCount";

/// `TerminalContextKeyStrings.SplitPaneActive`.
pub const SPLIT_PANE_ACTIVE_CONTEXT_KEY: &str = "terminalSplitPaneActive";

thread_local! {
    /// `TerminalContextKeys.groupCount`: the current number of terminal groups (escondida da lista central).
    static GROUP_COUNT: RawContextKey = RawContextKey::new(GROUP_COUNT_CONTEXT_KEY, Dynamic::Number(0.0), ContextKeyMeta::Hide);

    /// `TerminalContextKeys.splitTerminalActive`: whether the active terminal is a split pane.
    static SPLIT_TERMINAL_ACTIVE: RawContextKey = RawContextKey::new(
        SPLIT_PANE_ACTIVE_CONTEXT_KEY,
        Dynamic::Bool(false),
        ContextKeyMeta::Description(localize("splitPaneActive", "Whether the active terminal is a split pane.", &[])),
    );
}

/// `TerminalContextKeys.groupCount.bindTo(service)`.
pub fn group_count_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    GROUP_COUNT.with(|key| key.bind_to(context_key_service))
}

/// `TerminalContextKeys.splitTerminalActive.bindTo(service)`.
pub fn split_terminal_active_bind_to(context_key_service: &dyn IContextKeyService) -> Rc<dyn IContextKey> {
    SPLIT_TERMINAL_ACTIVE.with(|key| key.bind_to(context_key_service))
}
