//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/quickFix/browser/quickFix.ts`: o que o `MainThreadTerminalService` usa do
//! serviço de correções rápidas do terminal (`ITerminalQuickFixService.registerQuickFixProvider`, o provedor, as opções do
//! casador e a correção que o provedor devolve). O resto do arquivo (`onDidRegisterProvider`, `registerCommandSelector`,
//! `extensionQuickFixes`, os seletores e as ações internas) entra quando um consumidor pedir.
//!
//! Divergências:
//! - `ITerminalQuickFix` (a interface base com `type`, `id` e `source`, e as ações que a estendem) é o struct
//!   [`ITerminalQuickFix`] com o [`TerminalQuickFixAction`] no lugar das propriedades espalhadas (`terminalCommand`, `uri`, e os campos de
//!   um comando do VS Code). O `type` (palavra reservada) é o `kind()`, derivado do `TerminalQuickFixAction`.
//! - `ITerminalQuickFixOptions.type` é o [`TerminalQuickFixOptionsType`]; `commandLineMatcher: string | RegExp` é o `LineMatcher` da
//!   plataforma, e `commandExitResult` e `kind` são enums.
//! - `provideTerminalQuickFixes` recebe as `options` por referência mutável, porque o `MainThreadTerminalService` as altera
//!   (`options.outputMatcher.length = 40`) e no original a mudança chega ao dono do objeto.
//! - `providers: Map<string, ITerminalQuickFixProvider>` é um `Vec` de pares na ordem de registro.

use std::rc::Rc;

use je_base::arrays::OneOrMany;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::terminal::{ITerminalOutputMatcher, LineMatcher};

use crate::api::common::ext_host_protocol::ICommandDto;

/// `TerminalQuickFixType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalQuickFixType {
    TerminalCommand = 0,
    Opener = 1,
    Port = 2,
    VscodeCommand = 3,
}

/// A parte específica de cada correção: `ITerminalQuickFixTerminalCommandAction`, `ITerminalQuickFixOpenerAction` e o comando do VS Code
/// (`ITerminalQuickFixCommandAction`, com o `ICommandDto` que o host de extensões envia).
#[derive(Clone, Debug)]
pub enum TerminalQuickFixAction {
    TerminalCommand {
        terminal_command: String,
        // Should this depend on whether alt is held? (a pergunta que o original deixa em aberto)
        should_execute: Option<bool>,
    },
    Opener {
        uri: URI,
    },
    VscodeCommand(ICommandDto),
}

/// `ITerminalQuickFix` com as propriedades da ação.
#[derive(Clone, Debug)]
pub struct ITerminalQuickFix {
    pub id: String,
    pub source: String,
    pub action: TerminalQuickFixAction,
}

impl ITerminalQuickFix {
    /// `type`.
    pub fn kind(&self) -> TerminalQuickFixType {
        match self.action {
            TerminalQuickFixAction::TerminalCommand { .. } => TerminalQuickFixType::TerminalCommand,
            TerminalQuickFixAction::Opener { .. } => TerminalQuickFixType::Opener,
            TerminalQuickFixAction::VscodeCommand(_) => TerminalQuickFixType::VscodeCommand,
        }
    }
}

/// `ITerminalQuickFixOptions.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalQuickFixOptionsType {
    Internal,
    Resolved,
    Unresolved,
}

/// `commandExitResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandExitResult {
    Success,
    Error,
}

/// `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalQuickFixKind {
    Fix,
    Explain,
}

/// `ITerminalQuickFixOptions`.
#[derive(Clone)]
pub struct ITerminalQuickFixOptions {
    pub type_: TerminalQuickFixOptionsType,
    pub id: String,
    pub command_line_matcher: LineMatcher,
    pub output_matcher: Option<ITerminalOutputMatcher>,
    pub command_exit_result: CommandExitResult,
    pub kind: Option<TerminalQuickFixKind>,
}

/// `ITerminalQuickFixProvider`.
pub trait ITerminalQuickFixProvider {
    /// Provides terminal quick fixes. `terminal_command` é o comando para o qual se pedem as correções, `token` indica que o
    /// resultado já não é necessário.
    fn provide_terminal_quick_fixes(
        &self,
        terminal_command: Rc<dyn ITerminalCommand>,
        lines: Option<Vec<String>>,
        options: &mut ITerminalQuickFixOptions,
        token: CancellationToken,
    ) -> Promise<Option<OneOrMany<ITerminalQuickFix>>>;
}

/// `ITerminalQuickFixService`, reduzido ao `registerQuickFixProvider`.
pub trait ITerminalQuickFixService {
    /// `registerQuickFixProvider(id, provider)`.
    fn register_quick_fix_provider(&self, id: &str, provider: Rc<dyn ITerminalQuickFixProvider>) -> DisposableRef;
}

service_id!(pub I_TERMINAL_QUICK_FIX_SERVICE: dyn ITerminalQuickFixService = "terminalQuickFixService");
