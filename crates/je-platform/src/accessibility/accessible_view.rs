//! Porte de `src/vs/platform/accessibility/browser/accessibleView.ts`: o contrato `IAccessibleViewService`, os ids dos
//! provedores, as opções e os provedores de conteúdo (`AccessibleContentProvider` e `ExtensionContentProvider`).
//!
//! Notas de tradução:
//! - **`const enum AccessibleViewProviderId`** é um enum com [`AccessibleViewProviderId::as_str`] (o valor de texto do
//!   original). `AccessibleViewType` e `NavigationType` idem.
//! - **`type` das opções** é o campo `view_type` (`type` é palavra reservada); `position` é [`AccessibleViewPosition`].
//! - **`AccesibleViewContentProvider`** (`AccessibleContentProvider | ExtensionContentProvider`) é o enum
//!   [`AccessibleViewContentProvider`]; o guarda `isIAccessibleViewContentProvider` (que reconhece um objeto pela forma)
//!   é [`is_accessible_view_content_provider`], já que o tipo do enum garante a forma. As interfaces
//!   `IAccessibleViewContentProvider` e `IBasicContentProvider` não viram trait: cada provedor expõe os mesmos campos.
//! - **Construtor de 13 parâmetros** do `AccessibleContentProvider` é [`AccessibleContentProviderArgs`], com os mesmos
//!   campos na mesma ordem; os parâmetros opcionais começam em `None` ([`AccessibleContentProviderArgs::new`]).
//! - **Campos que o chamador atribui depois** (`provider.actions = ...`, `provider.onDispose = ...`) ficam em `RefCell`,
//!   com [`AccessibleContentProvider::actions`], [`AccessibleContentProvider::set_actions`] e
//!   [`AccessibleContentProvider::set_on_dispose`].
//! - **`onOpen`** devolve `IDisposable | void`: é `Option<DisposableRef>`.
//! - **`IAccessibleViewSymbol extends IPickerQuickAccessItem`** é o [`IQuickPickItem`] mais os campos próprios; os métodos de
//!   `IPickerQuickAccessItem` ficam no `extra` do item (ver `PickerItemHandlers`).
//! - **`IKeyboardEvent`** é o `KeyboardEvent` do porte do serviço de atalhos.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_instantiation::service_id;

use crate::keybinding::keybinding::KeyboardEvent;
use crate::quickinput::quick_input::IQuickPickItem;

/// `AccessibleViewProviderId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessibleViewProviderId {
    Terminal,
    TerminalChat,
    TerminalHelp,
    DiffEditor,
    MultiDiffEditorFileList,
    MergeEditor,
    PanelChat,
    CustomizationMigrations,
    CustomizationDiscovery,
    McpServerConfiguration,
    ChatTerminalOutput,
    ChatBackgroundShellOutput,
    ChatThinking,
    InlineChat,
    AgentChat,
    QuickChat,
    InlineCompletions,
    KeybindingsEditor,
    Notebook,
    ReplEditor,
    Editor,
    Hover,
    Notification,
    EmptyEditorHint,
    Comments,
    CommentThread,
    Repl,
    ReplHelp,
    RunAndDebug,
    Walkthrough,
    ReleaseNotes,
    SourceControl,
    EditorFindHelp,
    SearchHelp,
    TerminalFindHelp,
    WebviewFindHelp,
    OutputFindHelp,
    ChatFindHelp,
    ProblemsFilterHelp,
    SessionsChat,
    SessionsStorageCleanup,
    SessionsChanges,
    EmptyFileEditorSearch,
    SessionsListNotification,
    SessionCanvas,
    Survey,
    Automations,
    ConnectionDiagnostics,
    BrowserElementCommenting,
    ChatPetAchievements,
}

impl AccessibleViewProviderId {
    /// O valor de texto do `const enum`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::TerminalChat => "terminal-chat",
            Self::TerminalHelp => "terminal-help",
            Self::DiffEditor => "diffEditor",
            Self::MultiDiffEditorFileList => "multiDiffEditorFileList",
            Self::MergeEditor => "mergeEditor",
            Self::PanelChat => "panelChat",
            Self::CustomizationMigrations => "customizationMigrations",
            Self::CustomizationDiscovery => "customizationDiscovery",
            Self::McpServerConfiguration => "mcpServerConfiguration",
            Self::ChatTerminalOutput => "chatTerminalOutput",
            Self::ChatBackgroundShellOutput => "chatBackgroundShellOutput",
            Self::ChatThinking => "chatThinking",
            Self::InlineChat => "inlineChat",
            Self::AgentChat => "agentChat",
            Self::QuickChat => "quickChat",
            Self::InlineCompletions => "inlineCompletions",
            Self::KeybindingsEditor => "keybindingsEditor",
            Self::Notebook => "notebook",
            Self::ReplEditor => "replEditor",
            Self::Editor => "editor",
            Self::Hover => "hover",
            Self::Notification => "notification",
            Self::EmptyEditorHint => "emptyEditorHint",
            Self::Comments => "comments",
            Self::CommentThread => "commentThread",
            Self::Repl => "repl",
            Self::ReplHelp => "replHelp",
            Self::RunAndDebug => "runAndDebug",
            Self::Walkthrough => "walkthrough",
            Self::ReleaseNotes => "releaseNotes",
            Self::SourceControl => "scm",
            Self::EditorFindHelp => "editorFindHelp",
            Self::SearchHelp => "searchHelp",
            Self::TerminalFindHelp => "terminalFindHelp",
            Self::WebviewFindHelp => "webviewFindHelp",
            Self::OutputFindHelp => "outputFindHelp",
            Self::ChatFindHelp => "chatFindHelp",
            Self::ProblemsFilterHelp => "problemsFilterHelp",
            Self::SessionsChat => "sessionsChat",
            Self::SessionsStorageCleanup => "sessionsStorageCleanup",
            Self::SessionsChanges => "sessionsChanges",
            Self::EmptyFileEditorSearch => "emptyFileEditorSearch",
            Self::SessionsListNotification => "sessionsListNotification",
            Self::SessionCanvas => "sessionCanvas",
            Self::Survey => "survey",
            Self::Automations => "automations",
            Self::ConnectionDiagnostics => "connectionDiagnostics",
            Self::BrowserElementCommenting => "browserElementCommenting",
            Self::ChatPetAchievements => "chatPetAchievements",
        }
    }
}

/// `AccessibleViewType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibleViewType {
    Help,
    View,
}

impl AccessibleViewType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::View => "view",
        }
    }
}

/// `NavigationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationType {
    Previous,
    Next,
}

impl NavigationType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Previous => "previous",
            Self::Next => "next",
        }
    }
}

/// `IAccessibleViewOptions.position`.
///
/// By default, places the cursor on the top line of the accessible view. `InitialBottom` places the cursor on the
/// bottom line initially and returns it to the bottom when the content changes; `InitialBottomPreserve` places it on the
/// bottom line initially and preserves its position when the content changes; `Bottom` places it on the bottom line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibleViewPosition {
    Bottom,
    InitialBottom,
    InitialBottomPreserve,
}

/// `IAccessibleViewOptions`.
#[derive(Clone)]
pub struct IAccessibleViewOptions {
    pub read_more_url: Option<String>,
    /// Defaults to markdown.
    pub language: Option<String>,
    /// `type` no original.
    pub view_type: AccessibleViewType,
    pub position: Option<AccessibleViewPosition>,
    /// Returns a string that will be used as the content of the help dialog instead of the one provided by default.
    pub custom_help: Option<Rc<dyn Fn() -> String>>,
    /// If this provider might want to request to be shown again, provide an ID.
    pub id: Option<AccessibleViewProviderId>,
    /// Keybinding items to configure.
    pub configure_keybinding_items: Option<Vec<IQuickPickItem>>,
    /// Keybinding items that are already configured.
    pub configured_keybinding_items: Option<Vec<IQuickPickItem>>,
}

impl IAccessibleViewOptions {
    /// `{ type }`: as opções só com o tipo.
    pub fn new(view_type: AccessibleViewType) -> Self {
        Self {
            read_more_url: None,
            language: None,
            view_type,
            position: None,
            custom_help: None,
            id: None,
            configure_keybinding_items: None,
            configured_keybinding_items: None,
        }
    }
}

/// `IAccessibleViewSymbol`.
#[derive(Clone, Debug)]
pub struct IAccessibleViewSymbol {
    pub item: IQuickPickItem,
    pub markdown_to_parse: Option<String>,
    pub first_list_item: Option<String>,
    pub line_number: Option<u32>,
    pub end_line_number: Option<u32>,
}

/// `IPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IPosition {
    pub line_number: u32,
    pub column: u32,
}

/// `ICodeBlockActionContext`.
#[derive(Clone)]
pub struct ICodeBlockActionContext {
    pub code: String,
    pub language_id: Option<String>,
    pub code_block_index: usize,
    pub element: Rc<dyn Any>,
}

/// `IAccessibleViewService`.
pub trait IAccessibleViewService {
    /// The provider will be disposed when the view is closed.
    fn show(&self, provider: AccessibleViewContentProvider, position: Option<IPosition>);
    fn show_last_provider(&self, id: AccessibleViewProviderId);
    fn show_accessible_view_help(&self);
    fn next(&self);
    fn previous(&self);
    fn navigate_to_code_block(&self, navigation: NavigationType);
    fn go_to_symbol(&self);
    fn disable_hint(&self);
    fn get_position(&self, id: AccessibleViewProviderId) -> Option<IPosition>;
    fn set_position(&self, position: IPosition, reveal: Option<bool>, select: Option<bool>);
    fn get_last_position(&self) -> Option<IPosition>;
    /// If the setting is enabled, provides the open accessible view hint as a localized string.
    fn get_open_aria_hint(&self, verbosity_setting_key: &str) -> Option<String>;
    fn get_code_block_context(&self) -> Option<ICodeBlockActionContext>;
    fn configure_keybindings(&self, unassigned: bool);
    fn open_help_link(&self);
}

service_id!(pub I_ACCESSIBLE_VIEW_SERVICE: dyn IAccessibleViewService = "accessibleViewService");

/// `AccesibleViewContentProvider`: `AccessibleContentProvider | ExtensionContentProvider`.
#[derive(Clone)]
pub enum AccessibleViewContentProvider {
    Content(Rc<AccessibleContentProvider>),
    Extension(Rc<ExtensionContentProvider>),
}

/// Os parâmetros do construtor de [`AccessibleContentProvider`], na ordem do original.
pub struct AccessibleContentProviderArgs {
    pub id: AccessibleViewProviderId,
    pub options: IAccessibleViewOptions,
    pub provide_content: Rc<dyn Fn() -> String>,
    pub on_close: Rc<dyn Fn()>,
    pub verbosity_setting_key: String,
    pub on_open: Option<Rc<dyn Fn() -> Option<DisposableRef>>>,
    pub actions: Option<Vec<ActionRef>>,
    pub provide_next_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub provide_previous_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub on_did_change_content: Option<Event<()>>,
    pub on_key_down: Option<Rc<dyn Fn(&KeyboardEvent)>>,
    pub get_symbols: Option<Rc<dyn Fn() -> Vec<IAccessibleViewSymbol>>>,
    pub on_did_request_clear_last_provider: Option<Event<AccessibleViewProviderId>>,
}

impl AccessibleContentProviderArgs {
    /// Os cinco parâmetros obrigatórios; os opcionais começam em `None`.
    pub fn new(
        id: AccessibleViewProviderId,
        options: IAccessibleViewOptions,
        provide_content: Rc<dyn Fn() -> String>,
        on_close: Rc<dyn Fn()>,
        verbosity_setting_key: &str,
    ) -> Self {
        Self {
            id,
            options,
            provide_content,
            on_close,
            verbosity_setting_key: verbosity_setting_key.to_owned(),
            on_open: None,
            actions: None,
            provide_next_content: None,
            provide_previous_content: None,
            on_did_change_content: None,
            on_key_down: None,
            get_symbols: None,
            on_did_request_clear_last_provider: None,
        }
    }
}

/// `AccessibleContentProvider`.
pub struct AccessibleContentProvider {
    base: Rc<Disposable>,
    pub id: AccessibleViewProviderId,
    pub options: IAccessibleViewOptions,
    pub provide_content: Rc<dyn Fn() -> String>,
    pub on_close: Rc<dyn Fn()>,
    pub verbosity_setting_key: String,
    pub on_open: Option<Rc<dyn Fn() -> Option<DisposableRef>>>,
    actions: RefCell<Option<Vec<ActionRef>>>,
    pub provide_next_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub provide_previous_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub on_did_change_content: Option<Event<()>>,
    pub on_key_down: Option<Rc<dyn Fn(&KeyboardEvent)>>,
    pub get_symbols: Option<Rc<dyn Fn() -> Vec<IAccessibleViewSymbol>>>,
    pub on_did_request_clear_last_provider: Option<Event<AccessibleViewProviderId>>,
    /// Releases caller-owned state on teardown, including context-view replacement without onClose.
    on_dispose: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
}

impl AccessibleContentProvider {
    pub fn new(args: AccessibleContentProviderArgs) -> Rc<Self> {
        let base = Disposable::new();
        let on_dispose: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        {
            let on_dispose = on_dispose.clone();
            base.register(to_disposable(move || {
                let callback = on_dispose.borrow().clone();
                if let Some(callback) = callback {
                    callback();
                }
            }));
        }
        Rc::new(Self {
            base,
            id: args.id,
            options: args.options,
            provide_content: args.provide_content,
            on_close: args.on_close,
            verbosity_setting_key: args.verbosity_setting_key,
            on_open: args.on_open,
            actions: RefCell::new(args.actions),
            provide_next_content: args.provide_next_content,
            provide_previous_content: args.provide_previous_content,
            on_did_change_content: args.on_did_change_content,
            on_key_down: args.on_key_down,
            get_symbols: args.get_symbols,
            on_did_request_clear_last_provider: args.on_did_request_clear_last_provider,
            on_dispose,
        })
    }

    /// `actions`.
    pub fn actions(&self) -> Option<Vec<ActionRef>> {
        self.actions.borrow().clone()
    }

    /// `provider.actions = actions`.
    pub fn set_actions(&self, actions: Option<Vec<ActionRef>>) {
        *self.actions.borrow_mut() = actions;
    }

    /// `provider.onDispose = callback`.
    pub fn set_on_dispose(&self, callback: impl Fn() + 'static) {
        *self.on_dispose.borrow_mut() = Some(Rc::new(callback));
    }

    /// `provideContent()`.
    pub fn provide_content(&self) -> String {
        (self.provide_content)()
    }
}

impl IDisposable for AccessibleContentProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `isIAccessibleViewContentProvider`: o provedor tem a forma de `IAccessibleViewContentProvider` (id, opções, conteúdo,
/// fechamento e chave de verbosidade), que só o [`AccessibleContentProvider`] tem.
pub fn is_accessible_view_content_provider(provider: &AccessibleViewContentProvider) -> bool {
    matches!(provider, AccessibleViewContentProvider::Content(_))
}

/// Os parâmetros do construtor de [`ExtensionContentProvider`], na ordem do original.
pub struct ExtensionContentProviderArgs {
    pub id: String,
    pub options: IAccessibleViewOptions,
    pub provide_content: Rc<dyn Fn() -> String>,
    pub on_close: Rc<dyn Fn()>,
    pub on_open: Option<Rc<dyn Fn()>>,
    pub provide_next_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub provide_previous_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub actions: Option<Vec<ActionRef>>,
    pub on_did_change_content: Option<Event<()>>,
}

/// `ExtensionContentProvider`.
pub struct ExtensionContentProvider {
    base: Rc<Disposable>,
    pub id: String,
    pub options: IAccessibleViewOptions,
    pub provide_content: Rc<dyn Fn() -> String>,
    pub on_close: Rc<dyn Fn()>,
    pub on_open: Option<Rc<dyn Fn()>>,
    pub provide_next_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub provide_previous_content: Option<Rc<dyn Fn() -> Option<String>>>,
    pub actions: Option<Vec<ActionRef>>,
    pub on_did_change_content: Option<Event<()>>,
}

impl ExtensionContentProvider {
    pub fn new(args: ExtensionContentProviderArgs) -> Rc<Self> {
        Rc::new(Self {
            base: Disposable::new(),
            id: args.id,
            options: args.options,
            provide_content: args.provide_content,
            on_close: args.on_close,
            on_open: args.on_open,
            provide_next_content: args.provide_next_content,
            provide_previous_content: args.provide_previous_content,
            actions: args.actions,
            on_did_change_content: args.on_did_change_content,
        })
    }
}

impl IDisposable for ExtensionContentProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}
