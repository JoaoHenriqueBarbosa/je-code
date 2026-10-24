//! Porte parcial de `src/vs/workbench/contrib/terminal/common/terminalContextKey.ts`: só a chave `terminalShellType`
//! (`TerminalContextKeys.shellType`), que o `TerminalInstanceService` liga ao serviço de chaves de contexto.
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
