//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/links.ts`: os contratos dos detectores, dos abridores e
//! do resolvedor de links do terminal.
//!
//! Decisões de tradução:
//! - `Terminal` do `@xterm/xterm` é o [`ITerminal`] de [`crate::contrib::terminal::xterm_buffer`] (`cols` e
//!   `buffer.active`), e `IBufferLine` e `IBufferRange` são os mesmos módulos.
//! - `Pick<ITerminalProcessManager, 'initialCwd' | 'os' | 'remoteAuthority' | 'userHome'> & { backend?: Pick<ITerminalBackend,
//!   'getWslPath'> }` é a struct [`TerminalLinkProcessManager`], e o `backend` é o [`IWslPathBackend`] (só o `getWslPath`).
//! - `MaybePromise<ITerminalSimpleLink[]>` do `detect` é sempre um [`Promise`] (o resultado dos detectores é assíncrono).
//! - `type` (palavra reservada do Rust) é `type_` em [`ITerminalSimpleLink`]. `TerminalLinkType` (a união do tipo embutido
//!   com o de uma extensão) é o enum [`TerminalLinkType`]. `activate?(text)` é um `Rc<dyn Fn(&str)>`.
//! - `ITextEditorSelection` é o [`TextEditorSelection`] de `je_platform::opener::editor_types`.
//! - `ResolvedLink` (`IResolvedValidLink | null`) é `Option<IResolvedValidLink>`.
//! - O `ITerminalLinkProviderService` é o trait [`ITerminalLinkProviderService`], com o `ITerminalExternalLinkProvider` e o
//!   `ITerminalLink` de `contrib::terminal::browser::terminal`; `linkProviders: ReadonlySet` é um `Vec` na ordem de registro.
//! - Fora do porte: `OmitFirstArg` (um tipo do TypeScript sem forma em Rust).

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::opener::editor_types::TextEditorSelection;
use je_widgets::hover_api::IHoverAction;

use super::terminal_link_parsing::ParsedLink;
use crate::contrib::terminal::browser::terminal::ITerminalExternalLinkProvider;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, IBufferRange, ITerminal};

/// `ITerminalLinkProviderService`.
pub trait ITerminalLinkProviderService {
    /// `linkProviders`.
    fn link_providers(&self) -> Vec<Rc<dyn ITerminalExternalLinkProvider>>;

    /// `onDidAddLinkProvider`.
    fn on_did_add_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>>;
    /// `onDidRemoveLinkProvider`.
    fn on_did_remove_link_provider(&self) -> Event<Rc<dyn ITerminalExternalLinkProvider>>;

    // Currently only a single link provider is supported; the one registered by the ext host
    /// `registerLinkProvider(provider)`.
    fn register_link_provider(&self, provider: Rc<dyn ITerminalExternalLinkProvider>) -> DisposableRef;
}

service_id!(pub I_TERMINAL_LINK_PROVIDER_SERVICE: dyn ITerminalLinkProviderService = "terminalLinkProviderService");

/// `direction` do `getWslPath` do backend: `'unix-to-win' | 'win-to-unix'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslPathDirection {
    UnixToWin,
    WinToUnix,
}

impl WslPathDirection {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnixToWin => "unix-to-win",
            Self::WinToUnix => "win-to-unix",
        }
    }
}

/// `Pick<ITerminalBackend, 'getWslPath'>`.
pub trait IWslPathBackend {
    /// Converte um caminho entre o formato do WSL e o do Windows.
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String>;
}

/// `Pick<ITerminalProcessManager, 'initialCwd' | 'os' | 'remoteAuthority' | 'userHome'> & { backend?: ... }`.
#[derive(Clone)]
pub struct TerminalLinkProcessManager {
    pub initial_cwd: String,
    pub os: Option<OperatingSystem>,
    pub remote_authority: Option<String>,
    pub user_home: Option<String>,
    pub backend: Option<Rc<dyn IWslPathBackend>>,
}

/// `IResolvedValidLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct IResolvedValidLink {
    pub uri: URI,
    pub link: String,
    pub is_directory: bool,
}

/// `ResolvedLink`: `IResolvedValidLink | null`.
pub type ResolvedLink = Option<IResolvedValidLink>;

/// `ITerminalLinkResolver`.
pub trait ITerminalLinkResolver {
    fn resolve_link(&self, process_manager: &TerminalLinkProcessManager, link: &str, uri: Option<URI>) -> Promise<ResolvedLink>;
}

/// `TerminalBuiltinLinkType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalBuiltinLinkType {
    /// The link is validated to be a file on the file system and will open an editor.
    LocalFile,

    /// The link is validated to be a folder on the file system and is outside the workspace. It will
    /// reveal the folder within the explorer.
    LocalFolderOutsideWorkspace,

    /// The link is validated to be a folder on the file system and is within the workspace and will
    /// reveal the folder within the explorer.
    LocalFolderInWorkspace,

    /// A low confidence link which will search for the file in the workspace. If there is a single
    /// match, it will open the file; otherwise, it will present the matches in a quick pick.
    Search,

    /// A link whose text is a valid URI.
    Url,
}

impl TerminalBuiltinLinkType {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalFile => "LocalFile",
            Self::LocalFolderOutsideWorkspace => "LocalFolderOutsideWorkspace",
            Self::LocalFolderInWorkspace => "LocalFolderInWorkspace",
            Self::Search => "Search",
            Self::Url => "Url",
        }
    }
}

/// `ITerminalExternalLinkType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITerminalExternalLinkType {
    pub id: String,
}

/// `TerminalLinkType`: `TerminalBuiltinLinkType | ITerminalExternalLinkType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalLinkType {
    Builtin(TerminalBuiltinLinkType),
    External(ITerminalExternalLinkType),
}

impl From<TerminalBuiltinLinkType> for TerminalLinkType {
    fn from(link_type: TerminalBuiltinLinkType) -> Self {
        Self::Builtin(link_type)
    }
}

/// `ITerminalSimpleLink`.
#[derive(Clone)]
pub struct ITerminalSimpleLink {
    /// The text of the link.
    pub text: String,

    pub parsed_link: Option<ParsedLink>,

    /// The buffer range of the link.
    pub buffer_range: IBufferRange,

    /// The type of link, which determines how it is handled when activated.
    pub type_: TerminalLinkType,

    /// The URI of the link if it has been resolved.
    pub uri: Option<URI>,

    /// An optional full line to be used for context when resolving.
    pub context_line: Option<String>,

    /// The location or selection range of the link.
    pub selection: Option<TextEditorSelection>,

    /// Whether to trim a trailing colon at the end of a path.
    pub disable_trim_colon: Option<bool>,

    /// A hover label to override the default for the type.
    pub label: Option<String>,

    /// An optional set of actions to show in the hover's status bar.
    pub actions: Option<Vec<IHoverAction>>,

    /// An optional method to call when the link is activated. This should be used when there is are
    /// no registered opener for this link type.
    pub activate: Option<Rc<dyn Fn(&str)>>,
}

impl ITerminalSimpleLink {
    /// Um link só com os campos obrigatórios (`text`, `bufferRange` e `type`).
    pub fn new(text: impl Into<String>, buffer_range: IBufferRange, type_: impl Into<TerminalLinkType>) -> Self {
        Self {
            text: text.into(),
            parsed_link: None,
            buffer_range,
            type_: type_.into(),
            uri: None,
            context_line: None,
            selection: None,
            disable_trim_colon: None,
            label: None,
            actions: None,
            activate: None,
        }
    }
}

/// A link detector can search for and return links within the xterm.js buffer. A single link
/// detector can return multiple links of differing types.
pub trait ITerminalLinkDetector {
    /// The xterm.js instance this detector belongs to.
    fn xterm(&self) -> Rc<dyn ITerminal>;

    /// The maximum link length possible for this detector, this puts a cap on how much of a wrapped
    /// line to consider to prevent performance problems.
    fn max_link_length(&self) -> i32;

    /// Detects links within the _wrapped_ line range provided and returns them as an array.
    ///
    /// `lines` are the individual buffer lines that make up the wrapped line. `start_line` is the start of the wrapped
    /// line (it _will not_ be validated that it is indeed the start of a wrapped line) and `end_line` is the end of the
    /// wrapped line (it _will not_ be validated that it is indeed the end of a wrapped line).
    fn detect(&self, lines: &[Rc<dyn IBufferLine>], start_line: i32, end_line: i32) -> Promise<Vec<ITerminalSimpleLink>>;
}

/// `ITerminalLinkOpener`.
pub trait ITerminalLinkOpener {
    fn open(&self, link: &ITerminalSimpleLink) -> Promise<()>;
}
