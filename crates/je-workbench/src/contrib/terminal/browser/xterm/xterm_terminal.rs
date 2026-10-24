//! Porte lógico de `src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts`: o `XtermTerminal`, que embrulha o
//! emulador (`raw`) com a configuração, a leitura do buffer como texto e a escrita.
//!
//! Fora do porte, porque dependem de DOM, de GPU, de addons do xterm.js ou de serviços que ainda não existem aqui (a
//! interface `IXtermTerminal` inteira volta com a UI nativa da Fase 6):
//! - os addons (`MarkNavigationAddon`, `DecorationAddon`, `ShellIntegrationAddon` ligado ao emulador, clipboard, progress,
//!   search, serialize, image, unicode11, ligatures e webgl) e tudo que passa por eles (`clearBuffer`, `findNext`,
//!   `getContentsAsHtml`, `getCommandOutputAsHtml`, `getRangeAsVT`, `copySelection`, `selectMarkedRange`, o progresso e os
//!   eventos `onDidRequestRunCommand`/`onDidChangeFindResults`...);
//! - o tema (`getXtermTheme`, `updateTheme`: dependem do `IThemeService` e do `terminalColorRegistry`), o
//!   `xtermColorProvider`, os `IContextKey` de foco e seleção, o `ILifecycleService` do descarte das decorações, o serviço
//!   de log (`logLevel` e `logger`) e o `ILayoutService`;
//! - `attachToElement`, `refresh`, `forceRedraw`, `forceRefresh`, a rolagem, a seleção, `getLongestViewportWrappedLineLength`
//!   (precisa do `viewportY`), o `scrollbar` (`_getScrollbarOptions`, `scrollbarWidth`), `updateTerminalFontRendering` e o
//!   renderizador webgl dentro de `updateConfig`, e `_updateUnicodeVersion`.
//!
//! Divergências:
//! - **`xtermCtor`.** O construtor do xterm.js recebido pelo original é uma função ([`XtermCtor`]) que recebe as opções
//!   ([`XtermOptions`], o subconjunto de `ITerminalOptions` que o `XtermTerminal` escreve) e devolve o emulador
//!   ([`IXtermRaw`]).
//! - **`raw`.** O `Terminal` do xterm.js é um [`IXtermRaw`], que estende o contrato do emulador sem tela da plataforma
//!   ([`IHeadlessTerminal`], que já traz o buffer, o parser, os marcadores e os eventos) com as opções, o tamanho do buffer
//!   e a escrita. `raw.options` é [`IXtermRaw::options`], um `RefCell` de [`XtermOptions`].
//! - **Injeção.** Os parâmetros decorados do construtor que o porte lê são a tupla `Deps` do [`Injectable`], na ordem do
//!   original (`IConfigurationService` e `ITerminalConfigurationService`), e `resource`, `xtermCtor` e `options` são os
//!   `Args`. A classe herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//! - **`IXtermTerminalOptions`.** Ficam `cols`, `rows`, `scrollback` e `detached`. `xtermColorProvider`, `capabilities`,
//!   `shellIntegrationNonce`, `disableShellIntegrationReporting`, `xtermAddonImporter` e `disableOverviewRuler` só alimentam
//!   os addons, o tema e o `scrollbar`, que estão fora (o `detached` pula o ouvinte de configuração, como no original).
//! - **`getFont(dom.getActiveWindow(), ...)`.** Não há janela: o `devicePixelRatio` é [`DEVICE_PIXEL_RATIO`].
//! - **`_setCursorBlink`, `_setTextBlinking`, `_setCursorStyle`, `_setCursorStyleInactive`, `_setCursorWidth`.** As guardas
//!   `if (options.x !== value)` só evitam escrita repetida e viram atribuição; o `raw.refresh(0, rows - 1)` do
//!   `_setCursorBlink` pede redesenho ao renderizador e fica com ele (Fase 6). O construtor e `updateConfig` compartilham
//!   [`apply_configuration`], porque escrevem as mesmas opções a partir da mesma configuração.
//! - **`MouseWheelClassifier.INSTANCE.isPhysicalMouseWheel()`.** O classificador sem nenhum evento de roda devolve `false`,
//!   que é o valor lido na construção.
//! - **`getContentsAsText`.** O `throw new Error(...)` é o `Err` de [`XtermTerminal::get_contents_as_text`].
//! - **`getBufferReverseIterator`.** O gerador é um iterador de `String` que já leva o buffer e não empresta o terminal.
//! - **`write(data, callback)`.** O `data` é texto (o `Uint8Array` não tem uso no porte).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::{to_js_string, to_number};
use je_platform::terminal::capabilities::capabilities::IMarker;
use je_platform::terminal::xterm_headless::{IBuffer, IHeadlessTerminal};

use crate::contrib::terminal::browser::terminal::ITerminalConfigurationService;
use crate::contrib::terminal::common::terminal::TerminalFont;

/// O `devicePixelRatio` da janela ativa (`dom.getActiveWindow()`).
pub const DEVICE_PIXEL_RATIO: f64 = 1.0;

/// `RenderConstants.SmoothScrollDuration`.
const SMOOTH_SCROLL_DURATION: f64 = 125.0;

/// `TextBlinkConstants.IntervalDuration`.
const TEXT_BLINK_INTERVAL_DURATION: f64 = 600.0;

/// O subconjunto de `ITerminalOptions` do xterm.js que o `XtermTerminal` escreve.
#[derive(Clone)]
pub struct XtermOptions {
    pub cols: i32,
    pub rows: i32,
    pub alt_click_moves_cursor: bool,
    pub scrollback: f64,
    pub draw_bold_text_in_bright_colors: bool,
    pub font_family: String,
    pub font_weight: Dynamic,
    pub font_weight_bold: Dynamic,
    pub font_size: f64,
    pub letter_spacing: f64,
    pub line_height: f64,
    pub minimum_contrast_ratio: f64,
    pub tab_stop_width: f64,
    pub cursor_blink: bool,
    pub blink_interval_duration: f64,
    pub cursor_style: String,
    pub cursor_inactive_style: String,
    pub cursor_width: f64,
    pub mac_option_is_meta: bool,
    pub mac_option_click_forces_selection: bool,
    pub right_click_selects_word: bool,
    pub fast_scroll_sensitivity: f64,
    pub scroll_sensitivity: f64,
    pub scroll_on_erase_in_display: bool,
    pub word_separator: String,
    pub ignore_bracketed_paste_mode: bool,
    pub rescale_overlapping_glyphs: bool,
    pub allow_transparency: bool,
    pub smooth_scroll_duration: f64,
    /// `vtExtensions.kittyKeyboard`.
    pub kitty_keyboard: bool,
    /// `vtExtensions.win32InputMode`.
    pub win32_input_mode: bool,
}

impl XtermOptions {
    /// Os padrões do xterm.js para as opções que o `XtermTerminal` escreve, com o tamanho dado.
    pub fn defaults(cols: i32, rows: i32) -> Self {
        Self {
            cols,
            rows,
            alt_click_moves_cursor: true,
            scrollback: 1000.0,
            draw_bold_text_in_bright_colors: true,
            font_family: "courier-new, courier, monospace".to_owned(),
            font_weight: Dynamic::string("normal"),
            font_weight_bold: Dynamic::string("bold"),
            font_size: 15.0,
            letter_spacing: 0.0,
            line_height: 1.0,
            minimum_contrast_ratio: 1.0,
            tab_stop_width: 8.0,
            cursor_blink: false,
            blink_interval_duration: 0.0,
            cursor_style: "block".to_owned(),
            cursor_inactive_style: "outline".to_owned(),
            cursor_width: 1.0,
            mac_option_is_meta: false,
            mac_option_click_forces_selection: false,
            right_click_selects_word: false,
            fast_scroll_sensitivity: 5.0,
            scroll_sensitivity: 1.0,
            scroll_on_erase_in_display: false,
            word_separator: " ()[]{}',\"`".to_owned(),
            ignore_bracketed_paste_mode: false,
            rescale_overlapping_glyphs: false,
            allow_transparency: false,
            smooth_scroll_duration: 0.0,
            kitty_keyboard: false,
            win32_input_mode: false,
        }
    }
}

/// O `Terminal` do xterm.js (`RawXtermTerminal`) com o que o `XtermTerminal` lê e escreve além do contrato do emulador sem
/// tela: as opções, o tamanho do buffer ativo (`buffer.active.length`) e a escrita (`write(data, callback)`).
pub trait IXtermRaw: IHeadlessTerminal + IDisposable {
    /// `terminal.options`.
    fn options(&self) -> &RefCell<XtermOptions>;

    /// `terminal.buffer.active.length`: o número de linhas do buffer ativo (o scrollback e a área visível).
    fn buffer_length(&self) -> i32;

    /// `terminal.write(data, callback)`: o `callback` roda depois de o emulador interpretar o texto.
    fn write(&self, data: &str, callback: Option<Box<dyn FnOnce()>>);
}

/// `xtermCtor`: constrói o emulador a partir das opções.
pub type XtermCtor = Box<dyn FnOnce(&XtermOptions) -> Rc<dyn IXtermRaw>>;

/// `IXtermTerminalOptions` (só o que o porte lê, ver o cabeçalho).
pub struct IXtermTerminalOptions {
    /// The columns to initialize the terminal with.
    pub cols: i32,
    /// The rows to initialize the terminal with.
    pub rows: i32,
    /// The rows of scrollback to keep, overriding the `terminal.integrated.scrollback` setting.
    pub scrollback: Option<i32>,
    /// When true, skips registering listeners on global singleton services (configuration) to avoid accumulating listeners
    /// when many detached terminals are created concurrently. The caller should use [`XtermTerminal::update_config`] to apply
    /// those changes externally.
    pub detached: bool,
}

/// Os argumentos estáticos do construtor: `resource` (usado só pelo `DecorationAddon`, fora do porte) não entra.
pub struct XtermTerminalArgs {
    pub xterm_ctor: XtermCtor,
    pub options: IXtermTerminalOptions,
}

/// `getFullBufferLineAsString`: a linha com as continuações (quebras automáticas) anexadas, e o índice da primeira.
fn get_full_buffer_line_as_string(line_index: i32, buffer: &dyn IBuffer) -> (Option<String>, i32) {
    let mut line_index = line_index;
    let Some(mut line) = buffer.get_line(line_index) else {
        return (None, line_index);
    };
    let mut line_data = line.translate_to_string(Some(true), None, None).to_string_lossy();
    while line_index > 0 && line.is_wrapped() {
        line_index -= 1;
        let Some(previous) = buffer.get_line(line_index) else {
            break;
        };
        line = previous;
        line_data = line.translate_to_string(Some(false), None, None).to_string_lossy() + &line_data;
    }
    (Some(line_data), line_index)
}

/// `vscodeToXtermCursorStyle`: 'line' is used instead of bar in VS Code to be consistent with editor.cursorStyle.
fn vscode_to_xterm_cursor_style(style: &Dynamic) -> String {
    let style = to_js_string(style).to_string_lossy();
    if style == "line" { "bar".to_owned() } else { style }
}

/// `string | undefined` de uma opção de configuração, no texto que o JavaScript produz.
fn config_text(value: &Dynamic) -> String {
    to_js_string(value).to_string_lossy()
}

/// `XtermTerminal`: embrulha o xterm com funcionalidade adicional. A interação com o processo fica fora desta classe.
pub struct XtermTerminal {
    base: Rc<Disposable>,
    /// The raw xterm.js instance.
    raw: Rc<dyn IXtermRaw>,
    scrollback: Option<f64>,
    is_physical_mouse_wheel: bool,
    last_input_event: RefCell<Option<JsString>>,
    on_did_dispose: Rc<Emitter<()>>,

    configuration_service: Rc<dyn IConfigurationService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
}

impl XtermTerminal {
    pub fn raw(&self) -> &Rc<dyn IXtermRaw> {
        &self.raw
    }

    /// `lastInputEvent`.
    pub fn last_input_event(&self) -> Option<JsString> {
        self.last_input_event.borrow().clone()
    }

    /// `cols`.
    pub fn cols(&self) -> i32 {
        self.raw.cols()
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    /// `getBufferReverseIterator`: as linhas do buffer da última para a primeira, cada uma com as suas continuações, sem as
    /// linhas vazias.
    pub fn get_buffer_reverse_iterator(&self) -> impl Iterator<Item = String> + use<> {
        let buffer = self.raw.buffer_active();
        let mut index = self.raw.buffer_length() - 1;
        std::iter::from_fn(move || {
            while index >= 0 {
                let (line_data, line_index) = get_full_buffer_line_as_string(index, &*buffer);
                match line_data.filter(|line_data| !line_data.is_empty()) {
                    Some(line_data) => {
                        index = line_index - 1;
                        return Some(line_data);
                    }
                    None => index -= 1,
                }
            }
            None
        })
    }

    /// `getContentsAsText`: o texto do buffer entre os marcadores (o buffer inteiro sem eles).
    pub fn get_contents_as_text(&self, start_marker: Option<&dyn IMarker>, end_marker: Option<&dyn IMarker>) -> Result<String, Error> {
        let mut lines: Vec<String> = Vec::new();
        let buffer = self.raw.buffer_active();
        if end_marker.is_some_and(|end_marker| end_marker.line() == -1.0) {
            return Err(Error::new("Cannot get contents of a disposed endMarker"));
        }
        // When the start marker is disposed (scrolled out of the buffer due to scrollback limits), fall back to line 0 to
        // return whatever remains in the buffer rather than losing all output.
        let start_line = match start_marker {
            Some(start_marker) if start_marker.line() != -1.0 => start_marker.line() as i32,
            _ => 0,
        };
        let end_line = end_marker.map_or(self.raw.buffer_length() - 1, |end_marker| end_marker.line() as i32);
        for y in start_line..=end_line {
            lines.push(buffer.get_line(y).map(|line| line.translate_to_string(Some(true), None, None).to_string_lossy()).unwrap_or_default());
        }
        Ok(lines.join("\n"))
    }

    /// `write`.
    pub fn write(&self, data: &str, callback: Option<Box<dyn FnOnce()>>) {
        self.raw.write(data, callback);
    }

    /// `getFont`: a fonte efetiva, com as dimensões da célula medidas pelo serviço (o emulador não expõe o núcleo).
    pub fn get_font(&self) -> TerminalFont {
        self.terminal_configuration_service.get_font(DEVICE_PIXEL_RATIO, None, false)
    }

    /// `updateConfig`: aplica a configuração do terminal às opções do emulador.
    pub fn update_config(&self) {
        let config = self.terminal_configuration_service.config();
        let editor_options = self.configuration_service.get_value(Some("editor"), None);
        apply_configuration(&mut self.raw.options().borrow_mut(), &config, &editor_options, self.scrollback, self.is_physical_mouse_wheel);
    }
}

/// O que o construtor e `updateConfig` escrevem igual nas opções do emulador a partir da configuração do terminal e do
/// editor (`_setCursorBlink`, `_setTextBlinking`, `_setCursorStyle`, `_setCursorStyleInactive`, `_setCursorWidth` e
/// `_updateSmoothScrolling` incluídos).
fn apply_configuration(options: &mut XtermOptions, config: &Dynamic, editor_options: &Dynamic, scrollback: Option<f64>, is_physical_mouse_wheel: bool) {
    options.alt_click_moves_cursor =
        config.get_str("altClickMovesCursor").is_truthy() && editor_options.get_str("multiCursorModifier").strict_equals(&Dynamic::string("alt"));
    options.cursor_blink = config.get_str("cursorBlinking").is_truthy();
    options.blink_interval_duration = if config.get_str("textBlinking").is_truthy() { TEXT_BLINK_INTERVAL_DURATION } else { 0.0 };
    options.cursor_style = vscode_to_xterm_cursor_style(&config.get_str("cursorStyle"));
    options.cursor_inactive_style = vscode_to_xterm_cursor_style(&config.get_str("cursorStyleInactive"));
    options.cursor_width = to_number(&config.get_str("cursorWidth"));
    options.scrollback = scrollback.unwrap_or_else(|| to_number(&config.get_str("scrollback")));
    options.draw_bold_text_in_bright_colors = config.get_str("drawBoldTextInBrightColors").is_truthy();
    options.minimum_contrast_ratio = to_number(&config.get_str("minimumContrastRatio"));
    options.tab_stop_width = to_number(&config.get_str("tabStopWidth"));
    options.fast_scroll_sensitivity = to_number(&config.get_str("fastScrollSensitivity"));
    options.scroll_sensitivity = to_number(&config.get_str("mouseWheelScrollSensitivity"));
    options.mac_option_is_meta = config.get_str("macOptionIsMeta").is_truthy();
    options.mac_option_click_forces_selection = config.get_str("macOptionClickForcesSelection").is_truthy();
    options.right_click_selects_word = config.get_str("rightClickBehavior").strict_equals(&Dynamic::string("selectWord"));
    options.word_separator = config_text(&config.get_str("wordSeparators"));
    options.ignore_bracketed_paste_mode = config.get_str("ignoreBracketedPasteMode").is_truthy();
    options.rescale_overlapping_glyphs = config.get_str("rescaleOverlappingGlyphs").is_truthy();
    options.allow_transparency = config.get_str("enableImages").is_truthy();
    options.kitty_keyboard = config.get_str("enableKittyKeyboardProtocol").is_truthy();
    options.win32_input_mode = config.get_str("enableWin32InputMode").is_truthy();
    options.smooth_scroll_duration = if config.get_str("smoothScrolling").is_truthy() && is_physical_mouse_wheel { SMOOTH_SCROLL_DURATION } else { 0.0 };
}

impl Injectable for XtermTerminal {
    type Args = XtermTerminalArgs;
    type Deps = (Rc<dyn IConfigurationService>, Rc<dyn ITerminalConfigurationService>);

    fn create(args: XtermTerminalArgs, (configuration_service, terminal_configuration_service): Self::Deps) -> Rc<Self> {
        let XtermTerminalArgs { xterm_ctor, options } = args;
        let base = Disposable::new();
        let on_did_dispose = Emitter::new();
        base.register(on_did_dispose.clone());

        let scrollback = options.scrollback.map(f64::from);
        let font = terminal_configuration_service.get_font(DEVICE_PIXEL_RATIO, None, true);
        let config = terminal_configuration_service.config();
        let editor_options = configuration_service.get_value(Some("editor"), None);

        let is_physical_mouse_wheel = false;
        let mut xterm_options = XtermOptions::defaults(options.cols, options.rows);
        xterm_options.font_family = font.font_family;
        xterm_options.font_weight = config.get_str("fontWeight");
        xterm_options.font_weight_bold = config.get_str("fontWeightBold");
        xterm_options.font_size = font.font_size;
        xterm_options.letter_spacing = font.letter_spacing;
        xterm_options.line_height = font.line_height;
        xterm_options.scroll_on_erase_in_display = true;
        apply_configuration(&mut xterm_options, &config, &editor_options, scrollback, is_physical_mouse_wheel);

        let raw = xterm_ctor(&xterm_options);
        base.register(raw.clone());

        let this = Rc::new(Self {
            base,
            raw,
            scrollback,
            is_physical_mouse_wheel,
            last_input_event: RefCell::new(None),
            on_did_dispose,
            configuration_service,
            terminal_configuration_service,
        });

        // Skip global service listeners for detached terminals to avoid accumulating listeners when many detached instances
        // exist concurrently.
        if !options.detached {
            let weak = Rc::downgrade(&this);
            let subscription = this.configuration_service.on_did_change_configuration().listen(move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.affects_configuration("terminal.integrated", None)
                    || event.affects_configuration("editor.fastScrollSensitivity", None)
                    || event.affects_configuration("editor.mouseWheelScrollSensitivity", None)
                    || event.affects_configuration("editor.multiCursorModifier", None)
                {
                    this.update_config();
                }
            });
            this.base.register(subscription);
        }

        // Refire events
        let weak = Rc::downgrade(&this);
        let subscription = this.raw.on_data().listen(move |data| {
            if let Some(this) = weak.upgrade() {
                *this.last_input_event.borrow_mut() = Some(data.clone());
            }
        });
        this.base.register(subscription);

        this
    }
}

impl IDisposable for XtermTerminal {
    fn dispose(&self) {
        self.on_did_dispose.fire(());
        self.base.dispose();
    }
}

/// `getXtermScaledDimensions`: as colunas e as linhas que cabem em `width` x `height` pixels CSS, ou `None` sem as dimensões da
/// célula.
pub fn get_xterm_scaled_dimensions(device_pixel_ratio: f64, font: &TerminalFont, width: f64, height: f64) -> Option<(f64, f64)> {
    let (char_width, char_height) = match (font.char_width, font.char_height) {
        (Some(char_width), Some(char_height)) if char_width != 0.0 && char_height != 0.0 && !char_width.is_nan() && !char_height.is_nan() => (char_width, char_height),
        _ => return None,
    };

    // Because xterm.js converts from CSS pixels to actual pixels through the use of canvas, window.devicePixelRatio needs to be
    // used here in order to be precise. font.charWidth/charHeight alone as insufficient when window.devicePixelRatio changes.
    let scaled_width_available = width * device_pixel_ratio;

    let scaled_char_width = char_width * device_pixel_ratio + font.letter_spacing;
    let cols = (scaled_width_available / scaled_char_width).floor().max(1.0);

    let scaled_height_available = height * device_pixel_ratio;
    let scaled_char_height = (char_height * device_pixel_ratio).ceil();
    let scaled_line_height = (scaled_char_height * font.line_height).floor();
    let rows = (scaled_height_available / scaled_line_height).floor().max(1.0);

    Some((rows, cols))
}
