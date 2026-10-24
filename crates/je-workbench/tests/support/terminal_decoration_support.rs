//! Os dublês do teste do `DecorationAddon` (`decorationAddon.test.ts`): o literal `{ command, marker, timestamp, hasOutput } as
//! ITerminalCommand`, o `IHoverService` do `instantiationService.stub(IHoverService, ...)` e os serviços que o
//! `workbenchInstantiationService` entrega ao addon e que nenhum caso usa (clipboard, opener, menu de contexto e sinal de
//! acessibilidade).
//!
//! Como o objeto vazio do `mock<T>()` do original, um serviço que o caso não usa lança `X is not a function` quando algo o chama.

use std::cell::Cell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, plain_disposable};
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_platform::opener::opener::{
    IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions, OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::quickinput::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInputService, IQuickNavigateConfiguration, IQuickPick, IQuickPickItem, IQuickTree, QuickInputAlignment,
    QuickInputAlignmentSetting, QuickInputAnchor, QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason, QuickInputRef, QuickPickCreateOptions,
    QuickPicks,
};
use je_platform::terminal::capabilities::capabilities::{IBaseTerminalCommand, IMarkProperties, IMarker, ITerminalCommand, TerminalCommandConfidence};
use je_platform::terminal::terminal::{ITerminalOutputMatch, ITerminalOutputMatcher};
use je_theme::color::Color;
use je_workbench::contrib::terminal::browser::xterm::decoration_addon::{
    ContextMenuDelegate, DelayedHoverOptions, IContextMenuService, IDecorationAccessibilitySignalService, IHoverService, TerminalCommandSignal,
};
use je_workbench::contrib::terminal::browser::xterm::xterm_decoration::IDecorationElement;

/// `<membro> is not a function`.
fn unmocked(member: &str) -> ! {
    panic!("{member} is not a function")
}

/// O literal `{ command, marker?, exitCode?, timestamp, hasOutput: () => false } as ITerminalCommand`: o que o literal não define
/// vale `undefined` (`None`), e o `duration` ausente é `NaN` (ver `decoration_styles.rs`).
pub struct DecorationCommandLiteral {
    pub command: String,
    pub marker: Option<Rc<dyn IMarker>>,
    pub exit_code: Option<f64>,
    pub timestamp: f64,
}

impl DecorationCommandLiteral {
    pub fn create(command: &str, marker: Option<Rc<dyn IMarker>>, exit_code: Option<f64>, timestamp: f64) -> Rc<dyn ITerminalCommand> {
        Rc::new(Self { command: command.to_owned(), marker, exit_code, timestamp })
    }
}

impl IBaseTerminalCommand for DecorationCommandLiteral {
    fn command(&self) -> String {
        self.command.clone()
    }

    fn set_command(&self, _command: &str) {
        unmocked("setCommand")
    }

    fn command_line_confidence(&self) -> TerminalCommandConfidence {
        unmocked("commandLineConfidence")
    }

    fn set_command_line_confidence(&self, _confidence: TerminalCommandConfidence) {
        unmocked("setCommandLineConfidence")
    }

    fn is_trusted(&self) -> bool {
        false
    }

    fn timestamp(&self) -> f64 {
        self.timestamp
    }

    fn duration(&self) -> f64 {
        f64::NAN
    }

    fn id(&self) -> Option<String> {
        None
    }

    fn cwd(&self) -> Option<String> {
        None
    }

    fn exit_code(&self) -> Option<f64> {
        self.exit_code
    }

    fn command_start_line_content(&self) -> Option<String> {
        None
    }

    fn mark_properties(&self) -> Option<IMarkProperties> {
        None
    }

    fn executed_x(&self) -> Option<f64> {
        None
    }

    fn start_x(&self) -> Option<f64> {
        None
    }
}

impl ITerminalCommand for DecorationCommandLiteral {
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn marker(&self) -> Option<Rc<dyn IMarker>> {
        self.marker.clone()
    }

    fn end_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn set_end_marker(&self, _marker: Option<Rc<dyn IMarker>>) {
        unmocked("setEndMarker")
    }

    fn executed_marker(&self) -> Option<Rc<dyn IMarker>> {
        None
    }

    fn aliases(&self) -> Option<Vec<Vec<String>>> {
        None
    }

    fn was_replayed(&self) -> Option<bool> {
        None
    }

    fn extract_command_line(&self) -> String {
        unmocked("extractCommandLine")
    }

    fn get_output(&self) -> Option<String> {
        unmocked("getOutput")
    }

    fn get_output_match(&self, _output_matcher: &ITerminalOutputMatcher) -> Option<ITerminalOutputMatch> {
        unmocked("getOutputMatch")
    }

    fn has_output(&self) -> bool {
        false
    }

    fn get_prompt_row_count(&self) -> f64 {
        unmocked("getPromptRowCount")
    }

    fn get_command_row_count(&self) -> f64 {
        unmocked("getCommandRowCount")
    }
}

/// `instantiationService.stub(IHoverService, { setupDelayedHover: () => ({ dispose: () => hoverDisposed = true }) })`.
pub struct StubHoverService {
    pub hover_disposed: Rc<Cell<bool>>,
}

impl IHoverService for StubHoverService {
    fn setup_delayed_hover(&self, _element: Rc<dyn IDecorationElement>, _options: Rc<dyn Fn() -> DelayedHoverOptions>) -> DisposableRef {
        let hover_disposed = self.hover_disposed.clone();
        plain_disposable(move || hover_disposed.set(true))
    }
}

/// O `IContextMenuService` que nenhum caso usa.
pub struct UnusedContextMenuService;

impl IContextMenuService for UnusedContextMenuService {
    fn show_context_menu(&self, _delegate: ContextMenuDelegate) {
        unmocked("showContextMenu")
    }
}

/// O `IAccessibilitySignalService` do `workbenchInstantiationService`: nenhum caso toca sinal.
pub struct SilentAccessibilitySignalService;

impl IDecorationAccessibilitySignalService for SilentAccessibilitySignalService {
    fn play_signal(&self, _signal: TerminalCommandSignal) {}
}

/// O `IClipboardService` que nenhum caso usa.
pub struct UnusedClipboardService;

impl IClipboardService for UnusedClipboardService {
    fn write_text(&self, _text: &str) -> Promise<()> {
        unmocked("writeText")
    }

    fn read_text(&self) -> Promise<String> {
        unmocked("readText")
    }

    fn read_find_text(&self) -> Promise<JsString> {
        unmocked("readFindText")
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        unmocked("writeFindText")
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        unmocked("writeResources")
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        unmocked("readResources")
    }

    fn has_resources(&self) -> Promise<bool> {
        unmocked("hasResources")
    }
}

/// O `IOpenerService` que nenhum caso usa.
pub struct UnusedOpenerService;

impl IOpenerService for UnusedOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        unmocked("registerOpener")
    }

    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        unmocked("registerValidator")
    }

    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        unmocked("registerExternalUriResolver")
    }

    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {
        unmocked("setDefaultExternalOpener")
    }

    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        unmocked("registerExternalOpener")
    }

    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        unmocked("open")
    }

    fn resolve_external_uri(&self, _resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        unmocked("resolveExternalUri")
    }
}

/// O `IQuickInputService` que nenhum caso usa (o `createQuickPick` só roda no menu "Toggle Visibility").
pub struct UnusedQuickInputService;

impl IQuickInputService for UnusedQuickInputService {
    fn input(&self, _options: Option<&IInputOptions>, _token: Option<&CancellationToken>) -> Promise<Option<String>> {
        unmocked("input")
    }

    fn pick(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        unmocked("pick")
    }

    fn back_button(&self) -> QuickInputButtonRef {
        unmocked("backButton")
    }

    fn on_show(&self) -> Event<()> {
        unmocked("onShow")
    }

    fn on_hide(&self) -> Event<()> {
        unmocked("onHide")
    }

    fn alignment(&self) -> IObservable<QuickInputAlignment> {
        unmocked("alignment")
    }

    fn with_quick_input_anchor(
        &self,
        _anchor: Option<QuickInputAnchor>,
        _anchor_position: Option<QuickInputAnchorPosition>,
        _operation: Box<dyn FnOnce() -> Promise<Dynamic>>,
    ) -> Promise<Dynamic> {
        unmocked("withQuickInputAnchor")
    }

    fn pick_many(&self, _picks: QuickPicks, _options: Option<&IPickOptions>, _token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        unmocked("pick")
    }

    fn create_quick_pick(&self, _options: Option<QuickPickCreateOptions>) -> Rc<dyn IQuickPick> {
        unmocked("createQuickPick")
    }

    fn create_input_box(&self) -> Rc<dyn IInputBox> {
        unmocked("createInputBox")
    }

    fn create_quick_tree(&self) -> Rc<dyn IQuickTree> {
        unmocked("createQuickTree")
    }

    fn focus(&self) {
        unmocked("focus")
    }

    fn toggle(&self) {
        unmocked("toggle")
    }

    fn navigate(&self, _next: bool, _quick_navigate: Option<&IQuickNavigateConfiguration>) {
        unmocked("navigate")
    }

    fn back(&self) -> Promise<()> {
        unmocked("back")
    }

    fn accept(&self, _key_mods: Option<IKeyMods>) -> Promise<()> {
        unmocked("accept")
    }

    fn cancel(&self, _reason: Option<QuickInputHideReason>) -> Promise<()> {
        unmocked("cancel")
    }

    fn toggle_hover(&self) {
        unmocked("toggleHover")
    }

    fn current_quick_input(&self) -> Option<QuickInputRef> {
        unmocked("currentQuickInput")
    }

    fn set_alignment(&self, _alignment: QuickInputAlignmentSetting) {
        unmocked("setAlignment")
    }
}

/// O tema sem nenhuma cor definida (o `TestColorTheme` sem mapa).
pub struct EmptyColorTheme;

impl IColorTheme for EmptyColorTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        None
    }
}

/// O `TestThemeService` do `workbenchInstantiationService`: o tema vazio, que nunca muda.
pub struct EmptyThemeService;

impl IThemeService for EmptyThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        Rc::new(EmptyColorTheme)
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        Event::none()
    }
}
