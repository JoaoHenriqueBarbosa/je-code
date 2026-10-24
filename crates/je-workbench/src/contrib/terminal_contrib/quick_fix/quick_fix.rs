//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/browser/quickFix.ts`: o contrato do serviço de quick fixes do
//! terminal, os tipos das opções e das ações.
//!
//! Decisões de tradução:
//! - `ITerminalCommandSelector` vive em `platform/terminal/common/terminal.ts` no original e só o quick fix o usa; fica aqui,
//!   com `commandLineMatcher: string | RegExp` como [`LineMatcher`] (o mesmo tipo do `lineMatcher` do `ITerminalOutputMatcher`).
//! - `ITerminalQuickFixOptions` (a interface base, com `type: 'internal' | 'resolved' | 'unresolved'`) é a struct
//!   [`ITerminalQuickFixOptions`]; as três interfaces que a estendem (`Internal`, `ResolvedExtension`,
//!   `UnresolvedExtension`) são o [`QuickFixOption`], que a desreferencia e traz o `handler`, uma enumeração com o callback
//!   de cada tipo.
//! - `TerminalQuickFixActionInternal = IAction | ITerminalQuickFixTerminalCommandAction | ITerminalQuickFixOpenerAction` e o
//!   `ITerminalQuickFix` que as extensões devolvem são a enumeração [`TerminalQuickFixActionInternal`], uma variante por
//!   [`TerminalQuickFixType`]: o `IAction` com `type: Port` do `freePort` é [`ITerminalQuickFixPortAction`], e o
//!   `ITerminalQuickFixCommandAction` (`title`, sem `terminalCommand`) é a variante `VscodeCommand`. O `hasKey(quickFix, { type })`
//!   do original, que descarta o que não tem `type`, é garantido pelo tipo.
//! - Os callbacks são `Rc<dyn Fn>`; um que devolve `T[] | T | undefined` devolve `Vec<T>` (o `undefined` e o vetor vazio têm o
//!   mesmo efeito em quem os consome).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_base::lifecycle::DisposableRef;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::terminal::capabilities::capabilities::ITerminalCommand;
use je_platform::terminal::terminal::{ITerminalOutputMatch, ITerminalOutputMatcher, LineMatcher, RegExpMatchArray};

/// `commandExitResult: 'success' | 'error'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandExitResult {
    Success,
    Error,
}

/// `kind?: 'fix' | 'explain'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickFixKind {
    Fix,
    Explain,
}

/// `ITerminalCommandSelector`: o que uma extensão contribui no ponto de extensão `terminalQuickFixes`.
#[derive(Clone)]
pub struct ITerminalCommandSelector {
    pub id: String,
    pub command_line_matcher: LineMatcher,
    pub output_matcher: Option<ITerminalOutputMatcher>,
    pub command_exit_result: CommandExitResult,
    pub kind: Option<QuickFixKind>,
    /// O `extensionIdentifier` que o serviço acrescenta ao seletor contribuído.
    pub extension_identifier: Option<String>,
}

/// `ITerminalQuickFixService`.
pub trait ITerminalQuickFixService {
    fn on_did_register_provider(&self) -> Event<ITerminalQuickFixProviderSelector>;
    fn on_did_register_command_selector(&self) -> Event<ITerminalCommandSelector>;
    fn on_did_unregister_provider(&self) -> Event<String>;
    fn extension_quick_fixes(&self) -> Promise<Vec<ITerminalCommandSelector>>;
    /// `providers`: o `Map` compartilhado de provedores (o addon lê `providers.get(id)`).
    fn providers(&self) -> Rc<RefCell<HashMap<String, Rc<dyn ITerminalQuickFixProvider>>>>;
    fn register_quick_fix_provider(&self, id: &str, provider: Rc<dyn ITerminalQuickFixProvider>) -> DisposableRef;
    fn register_command_selector(&self, selector: ITerminalCommandSelector);
}

/// `ITerminalQuickFixProviderSelector`.
#[derive(Clone)]
pub struct ITerminalQuickFixProviderSelector {
    pub selector: ITerminalCommandSelector,
    pub provider: Rc<dyn ITerminalQuickFixProvider>,
}

/// `TerminalQuickFixCallback`.
pub type TerminalQuickFixCallback = Rc<dyn Fn(&ITerminalCommandMatchResult) -> Vec<TerminalQuickFixActionInternal>>;

/// `TerminalQuickFixCallbackExtension`.
pub type TerminalQuickFixCallbackExtension =
    Rc<dyn Fn(Rc<dyn ITerminalCommand>, Option<Vec<String>>, &ITerminalQuickFixOptions, CancellationToken) -> Promise<Vec<TerminalQuickFixActionInternal>>>;

/// `ITerminalQuickFixProvider`.
pub trait ITerminalQuickFixProvider {
    /// Provides terminal quick fixes.
    /// `terminal_command`: the command for which to provide quick fixes; `token`: a cancellation token indicating the result
    /// is no longer needed. Devolve os quick fixes, se houver.
    fn provide_terminal_quick_fixes(
        &self,
        terminal_command: Rc<dyn ITerminalCommand>,
        lines: Option<Vec<String>>,
        option: &ITerminalQuickFixOptions,
        token: CancellationToken,
    ) -> Promise<Vec<TerminalQuickFixActionInternal>>;
}

/// `TerminalQuickFixType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalQuickFixType {
    TerminalCommand = 0,
    Opener = 1,
    Port = 2,
    VscodeCommand = 3,
}

/// `type: 'internal' | 'resolved' | 'unresolved'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickFixOptionType {
    Internal,
    Resolved,
    Unresolved,
}

/// O que distingue as três interfaces que estendem `ITerminalQuickFixOptions`.
#[derive(Clone)]
pub enum QuickFixHandler {
    /// `ITerminalQuickFixInternalOptions.getQuickFixes`.
    Internal(TerminalQuickFixCallback),
    /// `ITerminalQuickFixResolvedExtensionOptions.getQuickFixes`.
    Resolved(TerminalQuickFixCallbackExtension),
    /// `ITerminalQuickFixUnresolvedExtensionOptions`: sem callback, o provedor ainda não ativou.
    Unresolved,
}

/// `ITerminalQuickFixOptions`: a interface base, com o `type`.
#[derive(Clone)]
pub struct ITerminalQuickFixOptions {
    pub option_type: QuickFixOptionType,
    pub id: String,
    pub command_line_matcher: LineMatcher,
    pub output_matcher: Option<ITerminalOutputMatcher>,
    pub command_exit_result: CommandExitResult,
    pub kind: Option<QuickFixKind>,
}

/// `ITerminalQuickFixInternalOptions`, `ITerminalQuickFixResolvedExtensionOptions` e
/// `ITerminalQuickFixUnresolvedExtensionOptions`: as opções base mais o callback do tipo. O `type` das opções base é o do
/// `handler`. Desreferencia para as opções base.
#[derive(Clone)]
pub struct QuickFixOption {
    options: ITerminalQuickFixOptions,
    pub handler: QuickFixHandler,
}

impl QuickFixOption {
    pub fn new(
        id: &str,
        command_line_matcher: LineMatcher,
        output_matcher: Option<ITerminalOutputMatcher>,
        command_exit_result: CommandExitResult,
        kind: Option<QuickFixKind>,
        handler: QuickFixHandler,
    ) -> Self {
        let option_type = match handler {
            QuickFixHandler::Internal(_) => QuickFixOptionType::Internal,
            QuickFixHandler::Resolved(_) => QuickFixOptionType::Resolved,
            QuickFixHandler::Unresolved => QuickFixOptionType::Unresolved,
        };
        Self { options: ITerminalQuickFixOptions { option_type, id: id.to_owned(), command_line_matcher, output_matcher, command_exit_result, kind }, handler }
    }

    /// As opções base (o que o original passa onde pede `ITerminalQuickFixOptions`).
    pub fn options(&self) -> &ITerminalQuickFixOptions {
        &self.options
    }
}

impl std::ops::Deref for QuickFixOption {
    type Target = ITerminalQuickFixOptions;

    fn deref(&self) -> &ITerminalQuickFixOptions {
        &self.options
    }
}

/// `ITerminalQuickFixTerminalCommandAction`.
#[derive(Clone)]
pub struct ITerminalQuickFixTerminalCommandAction {
    pub id: String,
    pub source: String,
    pub terminal_command: String,
    /// TODO do original: should this depend on whether alt is held?
    pub should_execute: Option<bool>,
}

/// `ITerminalQuickFixOpenerAction`.
#[derive(Clone)]
pub struct ITerminalQuickFixOpenerAction {
    pub id: String,
    pub source: String,
    pub uri: URI,
}

/// `ITerminalQuickFixCommandAction`.
#[derive(Clone)]
pub struct ITerminalQuickFixCommandAction {
    pub id: String,
    pub source: String,
    pub title: String,
}

/// O `IAction` com `type: TerminalQuickFixType.Port` que o `freePort` devolve.
#[derive(Clone)]
pub struct ITerminalQuickFixPortAction {
    pub id: String,
    pub source: String,
    pub label: String,
    pub tooltip: String,
    pub class: Option<String>,
    pub enabled: bool,
    pub run: Rc<dyn Fn()>,
}

/// `TerminalQuickFixActionInternal` e `ITerminalQuickFix`: uma variante por [`TerminalQuickFixType`].
#[derive(Clone)]
pub enum TerminalQuickFixActionInternal {
    TerminalCommand(ITerminalQuickFixTerminalCommandAction),
    Opener(ITerminalQuickFixOpenerAction),
    Port(ITerminalQuickFixPortAction),
    VscodeCommand(ITerminalQuickFixCommandAction),
}

impl TerminalQuickFixActionInternal {
    /// O `type` do original.
    pub fn fix_type(&self) -> TerminalQuickFixType {
        match self {
            Self::TerminalCommand(_) => TerminalQuickFixType::TerminalCommand,
            Self::Opener(_) => TerminalQuickFixType::Opener,
            Self::Port(_) => TerminalQuickFixType::Port,
            Self::VscodeCommand(_) => TerminalQuickFixType::VscodeCommand,
        }
    }
}

/// `ITerminalCommandMatchResult`.
#[derive(Clone)]
pub struct ITerminalCommandMatchResult {
    pub command_line: String,
    pub command_line_match: RegExpMatchArray,
    pub output_match: Option<ITerminalOutputMatch>,
}

// ---------------------------------------------------------------------------------------------
// Auxiliares de `String`/`RegExp` que o original usa direto (`text.match(matcher)`, `matcher.toString()`).

/// `text.match(matcher)` sem a flag `g`: um texto vira um `RegExp` (o `String#match` o cria), e `lastIndex` é zerado antes.
pub fn match_text(matcher: &LineMatcher, text: &str) -> Option<RegExpMatchArray> {
    let input = JsString::from(text);
    let found = match matcher {
        LineMatcher::Text(pattern) => JsRegExp::from_str(pattern, "").ok()?.exec(input.units()),
        LineMatcher::RegExp(regex) => {
            regex.set_last_index(0);
            regex.exec(input.units())
        }
    };
    found.map(|matched| RegExpMatchArray { input, matched })
}

/// `match[index]`.
pub fn match_capture(found: &RegExpMatchArray, index: usize) -> Option<String> {
    let range = found.matched.captures.get(index)?.clone()?;
    Some(String::from_utf16_lossy(&found.input.units()[range]))
}

/// `match.groups?.[name]`.
pub fn match_group(found: &RegExpMatchArray, name: &str) -> Option<String> {
    let range = found.matched.group(name)?;
    Some(String::from_utf16_lossy(&found.input.units()[range]))
}

/// `matcher.toString()`: o texto, ou `/source/flags` de um `RegExp`.
pub fn matcher_to_string(matcher: &LineMatcher) -> String {
    match matcher {
        LineMatcher::Text(text) => text.clone(),
        LineMatcher::RegExp(regex) => {
            let flags = regex.flags();
            let letters = [
                (flags.has_indices, 'd'),
                (flags.global, 'g'),
                (flags.ignore_case, 'i'),
                (flags.multiline, 'm'),
                (flags.dot_all, 's'),
                (flags.unicode, 'u'),
                (flags.unicode_sets, 'v'),
                (flags.sticky, 'y'),
            ];
            let flags: String = letters.iter().filter(|(on, _)| *on).map(|(_, letter)| *letter).collect();
            format!("/{}/{flags}", String::from_utf16_lossy(regex.source()))
        }
    }
}
