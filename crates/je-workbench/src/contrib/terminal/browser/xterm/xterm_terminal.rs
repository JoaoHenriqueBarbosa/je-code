//! Porte lógico de `src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts`: o `XtermTerminal`, que embrulha o
//! emulador (`raw`) com a configuração, a leitura do buffer como texto e a escrita.
//!
//! Fora do porte, porque dependem de DOM, de addons do xterm.js ou de serviços que ainda não existem aqui (a interface
//! `IXtermTerminal` inteira volta com a UI nativa da Fase 6):
//! - os addons de clipboard, progress, search, serialize, image, unicode11 e ligatures, e tudo que passa por eles
//!   (`findNext`, `getContentsAsHtml`, `getCommandOutputAsHtml`, `getRangeAsVT`, `copySelection`, o progresso e os eventos
//!   `onDidChangeFindResults`...); o `MarkNavigationAddon` e o `ShellIntegrationAddon` são criados por quem hospeda o
//!   `XtermTerminal` (`TerminalInstance`);
//! - os `IContextKey` de foco e seleção, o serviço de log (`logLevel` e `logger`) e o `ILayoutService`;
//! - `forceRedraw`, `forceRefresh`, a rolagem, a seleção, `getLongestViewportWrappedLineLength` (precisa do `viewportY`), o
//!   `scrollbar` (`_getScrollbarOptions`, `scrollbarWidth`) e `_updateUnicodeVersion`.
//!
//! Divergências:
//! - **`xtermCtor`.** O construtor do xterm.js recebido pelo original é uma função ([`XtermCtor`]) que recebe as opções
//!   ([`XtermOptions`], o subconjunto de `ITerminalOptions` que o `XtermTerminal` escreve) e devolve o emulador
//!   ([`IXtermRaw`]).
//! - **`raw`.** O `Terminal` do xterm.js é um [`IXtermRaw`], que estende o contrato do emulador sem tela da plataforma
//!   ([`IHeadlessTerminal`], que já traz o buffer, o parser, os marcadores e os eventos) com as opções, o tamanho do buffer
//!   e a escrita. `raw.options` é [`IXtermRaw::options`], um `RefCell` de [`XtermOptions`].
//! - **Injeção.** Os parâmetros decorados do construtor que o porte lê são a tupla `Deps` do [`Injectable`], na ordem do
//!   original (`IConfigurationService`, `IThemeService`, `ITerminalConfigurationService` e `ILifecycleService`), e
//!   `resource`, `xtermCtor` e `options` são os `Args`. O `IThemeService` e o `ILifecycleService` são dependências opcionais
//!   (`Option<Rc<..>>`): um hospedeiro sem eles não tem tema nem descarte das decorações no desligamento, que é o que os
//!   `TerminalInstance` dos testes anteriores à portabilidade do tema esperam. A classe herda de `Disposable`, aqui um
//!   `Disposable` guardado no campo `base`.
//! - **`IXtermTerminalOptions`.** `xtermColorProvider` e `capabilities` são opcionais pelo mesmo motivo. O
//!   `IContextMenuService`, o `IHoverService` e o `IAccessibilitySignalService` do `DecorationAddon` não têm identificador de
//!   serviço no porte (ver `decoration_addon.rs`), então chegam em `decoration_services`; sem eles (e sem `capabilities`) o
//!   `XtermTerminal` não cria o `DecorationAddon`. `shellIntegrationNonce`, `disableShellIntegrationReporting`,
//!   `xtermAddonImporter` (que vira `gpu_renderer_importer`) e `disableOverviewRuler` só alimentam o `ShellIntegrationAddon`
//!   e o `scrollbar`, que estão fora (o `detached` pula os ouvintes de configuração e de tema, como no original, e o do
//!   desligamento).
//! - **`getFont(dom.getActiveWindow(), ...)`.** Não há janela: o `devicePixelRatio` é [`DEVICE_PIXEL_RATIO`].
//! - **`attachToElement(container, options)`.** Não há `HTMLElement`: [`XtermTerminal::attach_to_element`] recebe só as
//!   opções, e o que o original faz com o elemento vira estado nativo. `raw.open(container)` é [`IXtermRaw::open`] (cria o
//!   estado do terminal), e a classe CSS `terminal-font-rendering-grayscale` é o estado de `terminal_font_rendering.rs`. Os
//!   ouvintes de foco e de roda do `textarea` e do `element` e o addon de ligaduras ficam fora (sem DOM e sem addon). O retorno
//!   (`.xterm-screen`) não existe.
//! - **`WebglAddon`.** O renderizador WebGL do xterm.js vira o renderizador por GPU do porte (wgpu): [`IXtermGpuRenderer`], criado
//!   por [`IGpuRendererImporter`] (o `xtermAddonImporter.importAddon('webgl')`) com [`GpuRendererOptions`] (o
//!   `{ customGlyphs }` do addon), ativado em [`IXtermGpuRenderer::activate`] (o `loadAddon`) e descartado em
//!   `_disposeOfWebglRenderer`. A criação, o descarte e a recriação (`_webglAddonLoadId`, `_webglAddonLoading`,
//!   `_webglAddonCustomGlyphs`) seguem o original passo a passo. `_refreshImageAddon` é chamado nos mesmos pontos, mas o addon de
//!   imagens está fora e não há nada a fazer nele.
//! - **`_setCursorBlink`, `_setTextBlinking`, `_setCursorStyle`, `_setCursorStyleInactive`, `_setCursorWidth`.** As guardas
//!   `if (options.x !== value)` só evitam escrita repetida e viram atribuição; o `raw.refresh(0, rows - 1)` do
//!   `_setCursorBlink` pede redesenho ao renderizador e fica com ele (Fase 6). O construtor e `updateConfig` compartilham
//!   [`apply_configuration`], porque escrevem as mesmas opções a partir da mesma configuração.
//! - **`MouseWheelClassifier.INSTANCE.isPhysicalMouseWheel()`.** O classificador sem nenhum evento de roda devolve `false`,
//!   que é o valor lido na construção.
//! - **`getContentsAsText`.** O `throw new Error(...)` é o `Err` de [`XtermTerminal::get_contents_as_text`].
//! - **`getBufferReverseIterator`.** O gerador é um iterador de `String` que já leva o buffer e não empresta o terminal.
//! - **`write(data, callback)`.** O `data` é texto (o `Uint8Array` não tem uso no porte).
//! - **`ITheme`.** É [`XtermTheme`], com os mesmos campos; `toString()` de uma cor é o `Display` do `Color`.
//! - **`_updateFindColors` e `PANEL_BACKGROUND`.** As cores de busca ficam com o addon de busca (fora). `PANEL_BACKGROUND`, que
//!   o original usa no `terminalBackground`, não é lido em nenhum caminho que sobrou.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_instantiation::Injectable;
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::{to_js_string, to_number};
use je_platform::terminal::capabilities::capabilities::{IMarker, ITerminalCapabilityStore, TerminalCapability, TerminalCapabilityImpl};
use je_platform::terminal::xterm_headless::{IBuffer, IHeadlessTerminal};
use je_theme::color::Color;

use super::decoration_addon::{DecorationAddon, DecorationAddonServices, DecorationCopyAsHtmlRequest, DecorationRunCommandRequest, IDecorationAddonTerminal};
use super::terminal_font_rendering::{IFontRenderingTerminal, update_terminal_font_rendering};
use crate::contrib::terminal::browser::terminal::ITerminalConfigurationService;
use crate::contrib::terminal::common::terminal::TerminalFont;
use crate::contrib::terminal::common::terminal_color_registry::{
    ANSI_COLOR_IDENTIFIERS, TERMINAL_CURSOR_BACKGROUND_COLOR, TERMINAL_CURSOR_FOREGROUND_COLOR, TERMINAL_FOREGROUND_COLOR,
    TERMINAL_INACTIVE_SELECTION_BACKGROUND_COLOR, TERMINAL_OVERVIEW_RULER_BORDER_COLOR, TERMINAL_SELECTION_BACKGROUND_COLOR, TERMINAL_SELECTION_FOREGROUND_COLOR,
};
use crate::services::lifecycle::lifecycle::ILifecycleService;

/// `scrollbarSliderActiveBackground` (`platform/theme/common/colorRegistry.ts`).
const SCROLLBAR_SLIDER_ACTIVE_BACKGROUND: &str = "scrollbarSlider.activeBackground";
/// `scrollbarSliderBackground`.
const SCROLLBAR_SLIDER_BACKGROUND: &str = "scrollbarSlider.background";
/// `scrollbarSliderHoverBackground`.
const SCROLLBAR_SLIDER_HOVER_BACKGROUND: &str = "scrollbarSlider.hoverBackground";


/// O `devicePixelRatio` da janela ativa (`dom.getActiveWindow()`).
pub const DEVICE_PIXEL_RATIO: f64 = 1.0;

/// `RenderConstants.SmoothScrollDuration`.
const SMOOTH_SCROLL_DURATION: f64 = 125.0;

/// `TextBlinkConstants.IntervalDuration`.
const TEXT_BLINK_INTERVAL_DURATION: f64 = 600.0;

/// `ITheme` do xterm.js: as cores como texto CSS (`Color.toString()`), `None` onde o original deixa `undefined`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XtermTheme {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub cursor_accent: Option<String>,
    pub selection_background: Option<String>,
    pub selection_inactive_background: Option<String>,
    pub selection_foreground: Option<String>,
    pub overview_ruler_border: Option<String>,
    pub scrollbar_slider_active_background: Option<String>,
    pub scrollbar_slider_background: Option<String>,
    pub scrollbar_slider_hover_background: Option<String>,
    /// As 16 cores ANSI, na ordem de `ansiColorIdentifiers` (`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
    /// `white` e as oito `bright*`).
    pub ansi: [Option<String>; 16],
}

/// O subconjunto de `ITerminalOptions` do xterm.js que o `XtermTerminal` escreve.
#[derive(Clone)]
pub struct XtermOptions {
    pub theme: XtermTheme,
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
            theme: XtermTheme::default(),
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
/// tela: as opções, o tamanho do buffer ativo (`buffer.active.length`) e a escrita (`write(data, callback)`). Estende também o
/// estado de renderização da fonte do elemento ([`IFontRenderingTerminal`]) e o contrato de decorações ([`IDecorationAddonTerminal`]).
pub trait IXtermRaw: IHeadlessTerminal + IFontRenderingTerminal + IDecorationAddonTerminal + IDisposable {
    /// `terminal.options`.
    fn options(&self) -> &RefCell<XtermOptions>;

    /// `terminal.buffer.active.length`: o número de linhas do buffer ativo (o scrollback e a área visível).
    fn buffer_length(&self) -> i32;

    /// `terminal.write(data, callback)`: o `callback` roda depois de o emulador interpretar o texto.
    fn write(&self, data: &str, callback: Option<Box<dyn FnOnce()>>);

    /// `terminal.onTitleChange`: o título que o shell define por sequência (`OSC 0` e `OSC 2`).
    fn on_title_change(&self) -> Event<JsString>;

    /// `terminal.modes.bracketedPasteMode`: se o modo de colagem entre colchetes (`CSI ? 2004 h`) está ligado.
    fn bracketed_paste_mode(&self) -> bool;

    /// `terminal.open(container)`: cria o estado de tela do terminal (o `terminal.element`). Abrir um terminal já aberto não
    /// o recria, como o `open` do xterm.js sobre um elemento que já tem o terminal.
    fn open(&self);

    /// `terminal.clear()`: limpa o buffer ativo, deixando a linha do cursor como a primeira.
    fn clear(&self);

    /// `terminal.input(data)`: entrada como se o usuário a digitasse (dispara `onData`).
    fn input(&self, data: &str);

    /// `terminal.buffer.normal.length`: o número de linhas do buffer normal.
    fn normal_buffer_length(&self) -> i32;
}

/// `xtermCtor`: constrói o emulador a partir das opções.
pub type XtermCtor = Box<dyn FnOnce(&XtermOptions) -> Rc<dyn IXtermRaw>>;

/// `IXtermColorProvider` (`browser/terminal.ts`).
pub trait IXtermColorProvider {
    /// `getBackgroundColor(theme)`.
    fn get_background_color(&self, theme: &dyn IColorTheme) -> Option<Color>;
}

/// `new Addon({ customGlyphs })`: as opções do renderizador por GPU (`IWebglAddonOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuRendererOptions {
    pub custom_glyphs: Option<bool>,
}

/// O `WebglAddon` do original, no porte o renderizador por GPU (wgpu) do terminal.
pub trait IXtermGpuRenderer: IDisposable {
    /// `activate(terminal)`, chamado por `loadAddon`: pode falhar (o `throw` de contexto indisponível).
    fn activate(&self, terminal: Rc<dyn IXtermRaw>) -> Result<(), Error>;

    /// `onContextLoss`.
    fn on_context_loss(&self) -> Event<()>;
}

/// O construtor do addon que `importAddon('webgl')` devolve.
pub type GpuRendererCtor = Rc<dyn Fn(GpuRendererOptions) -> Rc<dyn IXtermGpuRenderer>>;

/// `XtermAddonImporter`, só com `importAddon('webgl')`.
pub trait IGpuRendererImporter {
    fn import_gpu_renderer(&self) -> Promise<GpuRendererCtor>;
}

/// `IXtermAttachToElementOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IXtermAttachToElementOptions {
    /// Whether GPU rendering is enabled.
    pub enable_gpu: bool,
}

impl Default for IXtermAttachToElementOptions {
    fn default() -> Self {
        Self { enable_gpu: true }
    }
}

/// `IXtermTerminalOptions` (só o que o porte lê, ver o cabeçalho).
#[derive(Default)]
pub struct IXtermTerminalOptions {
    /// The columns to initialize the terminal with.
    pub cols: i32,
    /// The rows to initialize the terminal with.
    pub rows: i32,
    /// The color provider for the terminal.
    pub xterm_color_provider: Option<Rc<dyn IXtermColorProvider>>,
    /// The capabilities of the terminal.
    pub capabilities: Option<Rc<dyn ITerminalCapabilityStore>>,
    /// Os serviços do `DecorationAddon` que não têm identificador de serviço no porte.
    pub decoration_services: Option<DecorationAddonServices>,
    /// O `xtermAddonImporter` no que o porte lê dele: o importador do renderizador por GPU. Sem ele não há renderizador.
    pub gpu_renderer_importer: Option<Rc<dyn IGpuRendererImporter>>,
    /// The rows of scrollback to keep, overriding the `terminal.integrated.scrollback` setting.
    pub scrollback: Option<i32>,
    /// When true, skips registering listeners on global singleton services (configuration, theme) to avoid accumulating
    /// listeners when many detached terminals are created concurrently. The caller should use
    /// [`XtermTerminal::update_config`] and [`XtermTerminal::update_theme`] to apply those changes externally.
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

thread_local! {
    /// `XtermTerminal._suggestedRendererType === 'dom'`: o renderizador por GPU falhou ao carregar e o `auto` cai no de software.
    static SUGGESTED_RENDERER_IS_SOFTWARE: Cell<bool> = const { Cell::new(false) };
}

/// `XtermTerminal`: embrulha o xterm com funcionalidade adicional. A interação com o processo fica fora desta classe.
pub struct XtermTerminal {
    base: Rc<Disposable>,
    this: Weak<Self>,
    /// The raw xterm.js instance.
    raw: Rc<dyn IXtermRaw>,
    scrollback: Option<f64>,
    is_physical_mouse_wheel: bool,
    last_input_event: RefCell<Option<JsString>>,
    on_did_dispose: Rc<Emitter<()>>,
    on_did_request_run_command: Rc<Emitter<DecorationRunCommandRequest>>,
    on_did_request_copy_as_html: Rc<Emitter<DecorationCopyAsHtmlRequest>>,
    on_did_request_refresh_dimensions: Rc<Emitter<()>>,

    configuration_service: Rc<dyn IConfigurationService>,
    terminal_configuration_service: Rc<dyn ITerminalConfigurationService>,
    theme_service: Option<Rc<dyn IThemeService>>,
    accessibility_signal_service: Option<Rc<dyn IAccessibilitySignalService>>,
    xterm_color_provider: Option<Rc<dyn IXtermColorProvider>>,
    capabilities: Option<Rc<dyn ITerminalCapabilityStore>>,
    gpu_renderer_importer: Option<Rc<dyn IGpuRendererImporter>>,
    decoration_addon: Option<Rc<DecorationAddon>>,

    attached: Cell<Option<IXtermAttachToElementOptions>>,
    /// `_webglAddon`.
    gpu_renderer: RefCell<Option<Rc<dyn IXtermGpuRenderer>>>,
    /// `_webglContextLossListener`.
    gpu_context_loss_listener: RefCell<Option<DisposableRef>>,
    /// `_webglAddonCustomGlyphs`.
    gpu_renderer_custom_glyphs: Cell<Option<bool>>,
    /// `_webglAddonLoading`.
    gpu_renderer_loading: Cell<bool>,
    /// `_webglAddonLoadId`.
    gpu_renderer_load_id: Cell<u32>,
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

    /// `onDidRequestRunCommand`.
    pub fn on_did_request_run_command(&self) -> Event<DecorationRunCommandRequest> {
        self.on_did_request_run_command.event()
    }

    /// `onDidRequestCopyAsHtml`.
    pub fn on_did_request_copy_as_html(&self) -> Event<DecorationCopyAsHtmlRequest> {
        self.on_did_request_copy_as_html.event()
    }

    /// `onDidRequestRefreshDimensions`.
    pub fn on_did_request_refresh_dimensions(&self) -> Event<()> {
        self.on_did_request_refresh_dimensions.event()
    }

    /// `decorationAddon`: `None` quando o hospedeiro não deu `capabilities` e `decoration_services`.
    pub fn decoration_addon(&self) -> Option<&Rc<DecorationAddon>> {
        self.decoration_addon.as_ref()
    }

    /// `isGpuAccelerated`.
    pub fn is_gpu_accelerated(&self) -> bool {
        self.gpu_renderer.borrow().is_some()
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
        update_terminal_font_rendering(&*self.raw, &config.get_str("fontRendering"));

        if let Some(attached) = self.attached.get()
            && attached.enable_gpu
        {
            if self.should_load_gpu_renderer() {
                let _ = self.enable_gpu_renderer();
            } else {
                self.dispose_of_gpu_renderer();
            }
        }
    }

    /// `attachToElement(container, partialOptions)` (ver o cabeçalho sobre o `container`).
    pub fn attach_to_element(&self, partial_options: Option<IXtermAttachToElementOptions>) {
        let options = partial_options.unwrap_or_default();
        if self.attached.get().is_none() {
            self.raw.open();
        }

        update_terminal_font_rendering(&*self.raw, &self.terminal_configuration_service.config().get_str("fontRendering"));

        // TODO: Move before open so the DOM renderer doesn't initialize
        if options.enable_gpu && self.should_load_gpu_renderer() {
            let _ = self.enable_gpu_renderer();
        }

        self.attached.set(Some(options));
    }

    fn should_load_gpu_renderer(&self) -> bool {
        let gpu_acceleration = self.terminal_configuration_service.config().get_str("gpuAcceleration");
        (gpu_acceleration.strict_equals(&Dynamic::string("auto")) && !SUGGESTED_RENDERER_IS_SOFTWARE.with(Cell::get)) || gpu_acceleration.strict_equals(&Dynamic::string("on"))
    }

    /// `_enableWebglRenderer`: carrega o renderizador por GPU, recriando-o se a opção `customGlyphs` mudou.
    fn enable_gpu_renderer(&self) -> Promise<()> {
        // Currently webgl options can only be specified on addon creation
        if !self.raw.is_open() {
            return Promise::resolved(());
        }
        let custom_glyphs = self.custom_glyphs();
        if (self.gpu_renderer.borrow().is_some() || self.gpu_renderer_loading.get()) && self.gpu_renderer_custom_glyphs.get() == custom_glyphs {
            return Promise::resolved(());
        }
        let Some(importer) = self.gpu_renderer_importer.clone() else { return Promise::resolved(()) };

        // Dispose of existing addon before creating a new one to avoid leaking WebGL contexts
        self.dispose_of_gpu_renderer();

        let load_id = self.gpu_renderer_load_id.get();
        self.gpu_renderer_loading.set(true);
        self.gpu_renderer_custom_glyphs.set(custom_glyphs);

        let weak = self.this.clone();
        let on_import_error = self.this.clone();
        importer
            .import_gpu_renderer()
            .then_promise(move |ctor| {
                let Some(this) = weak.upgrade() else { return Promise::resolved(()) };
                if load_id != this.gpu_renderer_load_id.get() {
                    return Promise::resolved(());
                }

                this.gpu_renderer_loading.set(false);
                if !this.raw.is_open() {
                    this.gpu_renderer_custom_glyphs.set(None);
                    return Promise::resolved(());
                }

                let current_custom_glyphs = this.custom_glyphs();
                if custom_glyphs != current_custom_glyphs {
                    this.gpu_renderer_custom_glyphs.set(None);
                    return this.enable_gpu_renderer();
                }

                let renderer = ctor(GpuRendererOptions { custom_glyphs });
                *this.gpu_renderer.borrow_mut() = Some(renderer.clone());
                match renderer.activate(this.raw.clone()) {
                    Ok(()) => {
                        let weak_for_loss = this.this.clone();
                        *this.gpu_context_loss_listener.borrow_mut() = Some(renderer.on_context_loss().listen(move |_| {
                            if let Some(this) = weak_for_loss.upgrade() {
                                this.dispose_of_gpu_renderer();
                            }
                        }));
                        this.refresh_image_addon();
                        // WebGL renderer cell dimensions differ from the DOM renderer, make sure the terminal gets resized after
                        // the webgl addon is loaded
                        this.on_did_request_refresh_dimensions.fire(());
                    }
                    Err(_) => {
                        SUGGESTED_RENDERER_IS_SOFTWARE.with(|suggested| suggested.set(true));
                        this.dispose_of_gpu_renderer();
                    }
                }
                Promise::resolved(())
            })
            .catch(move |error| {
                if let Some(this) = on_import_error.upgrade()
                    && load_id == this.gpu_renderer_load_id.get()
                {
                    this.gpu_renderer_loading.set(false);
                    this.gpu_renderer_custom_glyphs.set(None);
                }
                je_base::errors::on_unexpected_error(&error);
            })
    }

    /// `this._terminalConfigurationService.config.customGlyphs`: `undefined` quando a configuração não a define.
    fn custom_glyphs(&self) -> Option<bool> {
        match self.terminal_configuration_service.config().get_str("customGlyphs") {
            Dynamic::Bool(value) => Some(value),
            _ => None,
        }
    }

    /// `_refreshImageAddon`: o addon de imagens está fora do porte, não há o que carregar nem descartar.
    fn refresh_image_addon(&self) {}

    /// `_disposeOfWebglRenderer`.
    fn dispose_of_gpu_renderer(&self) {
        self.gpu_renderer_load_id.set(self.gpu_renderer_load_id.get().wrapping_add(1));
        self.gpu_renderer_loading.set(false);
        self.gpu_renderer_custom_glyphs.set(None);
        let listener = self.gpu_context_loss_listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        let renderer = self.gpu_renderer.borrow_mut().take();
        let Some(renderer) = renderer else { return };
        renderer.dispose();
        self.refresh_image_addon();
        // WebGL renderer cell dimensions differ from the DOM renderer, make sure the terminal gets resized after the webgl addon
        // is disposed
        self.on_did_request_refresh_dimensions.fire(());
    }

    /// `clearDecorations`.
    pub fn clear_decorations(&self) {
        if let Some(decoration_addon) = &self.decoration_addon {
            decoration_addon.clear_decorations();
        }
    }

    /// `clearBuffer`.
    pub fn clear_buffer(&self) {
        let is_normal_buffer = self.raw.is_active_buffer_normal();
        // Clearing the alternate buffer leaves the normal buffer's output and markers intact.
        if is_normal_buffer {
            self.clear_decorations();
            if let Some(TerminalCapabilityImpl::CommandDetection(capability)) = self.capability(TerminalCapability::CommandDetection) {
                capability.clear_commands();
            }
            if let Some(TerminalCapabilityImpl::PartialCommandDetection(capability)) = self.capability(TerminalCapability::PartialCommandDetection) {
                capability.clear_commands();
            }
        }
        self.raw.clear();
        if is_normal_buffer {
            // xterm.js does not clear the first prompt, so trigger these to simulate the prompt being written
            if let Some(TerminalCapabilityImpl::CommandDetection(capability)) = self.capability(TerminalCapability::CommandDetection) {
                capability.handle_prompt_start(None);
            }
            if let Some(TerminalCapabilityImpl::CommandDetection(capability)) = self.capability(TerminalCapability::CommandDetection) {
                capability.handle_command_start(None);
            }
        }
        if let Some(accessibility_signal_service) = &self.accessibility_signal_service {
            accessibility_signal_service.play_signal(AccessibilitySignal::Clear);
        }
    }

    fn capability(&self, capability: TerminalCapability) -> Option<TerminalCapabilityImpl> {
        self.capabilities.as_ref().and_then(|capabilities| capabilities.get_capability(capability))
    }

    /// `getXtermTheme`.
    pub fn get_xterm_theme(&self, theme: Option<Rc<dyn IColorTheme>>) -> XtermTheme {
        let theme = theme.or_else(|| self.theme_service.as_ref().map(|theme_service| theme_service.get_color_theme()));
        build_xterm_theme(&self.terminal_configuration_service.config(), theme.as_deref(), self.xterm_color_provider.as_deref())
    }

    /// `_updateTheme`.
    fn apply_theme(&self, theme: Option<Rc<dyn IColorTheme>>) {
        let xterm_theme = self.get_xterm_theme(theme);
        self.raw.options().borrow_mut().theme = xterm_theme;
    }

    /// `updateTheme`: refaz o tema do terminal. Serve para atualizar de fora os terminais `detached`, que não ouvem o serviço de
    /// tema.
    pub fn update_theme(&self) {
        self.apply_theme(None);
    }

    /// `refresh`.
    pub fn refresh(&self) {
        self.apply_theme(None);
        if let Some(decoration_addon) = &self.decoration_addon {
            decoration_addon.refresh_layouts();
        }
    }
}

/// `getXtermTheme(theme)`, com a configuração, o tema e o provedor de fundo já resolvidos. Sem tema nenhuma cor é definida.
fn build_xterm_theme(config: &Dynamic, theme: Option<&dyn IColorTheme>, color_provider: Option<&dyn IXtermColorProvider>) -> XtermTheme {
    let decorations_enabled = to_js_string(&config.get_str("shellIntegration").get_str("decorationsEnabled")).to_string_lossy();
    let hide_overview_ruler = decorations_enabled == "never" || decorations_enabled == "gutter";

    let get_color = |id: &str| theme.and_then(|theme| theme.get_color(id, None));
    let to_css = |color: Option<Color>| color.map(|color| color.to_string());

    let foreground_color = get_color(TERMINAL_FOREGROUND_COLOR);
    let background_color = theme.and_then(|theme| color_provider.and_then(|color_provider| color_provider.get_background_color(theme)));
    let cursor_color = get_color(TERMINAL_CURSOR_FOREGROUND_COLOR).or_else(|| foreground_color.clone());
    let cursor_accent_color = get_color(TERMINAL_CURSOR_BACKGROUND_COLOR).or_else(|| background_color.clone());
    let selection_background_color = get_color(TERMINAL_SELECTION_BACKGROUND_COLOR);
    let selection_inactive_background_color = get_color(TERMINAL_INACTIVE_SELECTION_BACKGROUND_COLOR);
    let selection_foreground_color = get_color(TERMINAL_SELECTION_FOREGROUND_COLOR);

    XtermTheme {
        background: to_css(background_color),
        foreground: to_css(foreground_color),
        cursor: to_css(cursor_color),
        cursor_accent: to_css(cursor_accent_color),
        selection_background: to_css(selection_background_color),
        selection_inactive_background: to_css(selection_inactive_background_color),
        selection_foreground: to_css(selection_foreground_color),
        overview_ruler_border: if hide_overview_ruler { Some("#0000".to_owned()) } else { to_css(get_color(TERMINAL_OVERVIEW_RULER_BORDER_COLOR)) },
        scrollbar_slider_active_background: to_css(get_color(SCROLLBAR_SLIDER_ACTIVE_BACKGROUND)),
        scrollbar_slider_background: to_css(get_color(SCROLLBAR_SLIDER_BACKGROUND)),
        scrollbar_slider_hover_background: to_css(get_color(SCROLLBAR_SLIDER_HOVER_BACKGROUND)),
        ansi: std::array::from_fn(|index| to_css(get_color(ANSI_COLOR_IDENTIFIERS[index]))),
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
    type Deps = (
        Rc<dyn IConfigurationService>,
        Option<Rc<dyn IThemeService>>,
        Rc<dyn ITerminalConfigurationService>,
        Option<Rc<dyn ILifecycleService>>,
        Option<Rc<dyn IAccessibilitySignalService>>,
    );

    fn create(
        args: XtermTerminalArgs,
        (configuration_service, theme_service, terminal_configuration_service, lifecycle_service, accessibility_signal_service): Self::Deps,
    ) -> Rc<Self> {
        let XtermTerminalArgs { xterm_ctor, options } = args;
        let IXtermTerminalOptions { cols, rows, xterm_color_provider, capabilities, decoration_services, gpu_renderer_importer, scrollback, detached } = options;
        let base = Disposable::new();
        let on_did_dispose = Emitter::new();
        base.register(on_did_dispose.clone());
        let on_did_request_run_command = Emitter::new();
        base.register(on_did_request_run_command.clone());
        let on_did_request_copy_as_html = Emitter::new();
        base.register(on_did_request_copy_as_html.clone());
        let on_did_request_refresh_dimensions = Emitter::new();
        base.register(on_did_request_refresh_dimensions.clone());

        let scrollback = scrollback.map(f64::from);
        let font = terminal_configuration_service.get_font(DEVICE_PIXEL_RATIO, None, true);
        let config = terminal_configuration_service.config();
        let editor_options = configuration_service.get_value(Some("editor"), None);

        let is_physical_mouse_wheel = false;
        let mut xterm_options = XtermOptions::defaults(cols, rows);
        xterm_options.theme = build_xterm_theme(
            &config,
            theme_service.as_ref().map(|theme_service| theme_service.get_color_theme()).as_deref(),
            xterm_color_provider.as_deref(),
        );
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

        // Load addons
        let decoration_addon = match (&capabilities, decoration_services) {
            (Some(capabilities), Some(decoration_services)) => {
                let decoration_addon = DecorationAddon::new(capabilities.clone(), decoration_services);
                base.register(decoration_addon.clone());
                Some(decoration_addon)
            }
            _ => None,
        };

        let this = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            raw,
            scrollback,
            is_physical_mouse_wheel,
            last_input_event: RefCell::new(None),
            on_did_dispose,
            on_did_request_run_command,
            on_did_request_copy_as_html,
            on_did_request_refresh_dimensions,
            configuration_service,
            terminal_configuration_service,
            theme_service,
            accessibility_signal_service,
            xterm_color_provider,
            capabilities,
            gpu_renderer_importer,
            decoration_addon,
            attached: Cell::new(None),
            gpu_renderer: RefCell::new(None),
            gpu_context_loss_listener: RefCell::new(None),
            gpu_renderer_custom_glyphs: Cell::new(None),
            gpu_renderer_loading: Cell::new(false),
            gpu_renderer_load_id: Cell::new(0),
        });

        // Skip global service listeners for detached terminals to avoid accumulating listeners when many detached instances
        // exist concurrently.
        if !detached {
            let weak = Rc::downgrade(&this);
            let subscription = this.configuration_service.on_did_change_configuration().listen(move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.affects_configuration("terminal.integrated.gpuAcceleration", None) {
                    SUGGESTED_RENDERER_IS_SOFTWARE.with(|suggested| suggested.set(false));
                }
                if event.affects_configuration("terminal.integrated", None)
                    || event.affects_configuration("editor.fastScrollSensitivity", None)
                    || event.affects_configuration("editor.mouseWheelScrollSensitivity", None)
                    || event.affects_configuration("editor.multiCursorModifier", None)
                {
                    this.update_config();
                }
                if event.affects_configuration("terminal.integrated.shellIntegration.decorationsEnabled", None) {
                    this.apply_theme(None);
                }
            });
            this.base.register(subscription);

            if let Some(theme_service) = &this.theme_service {
                let weak = Rc::downgrade(&this);
                let subscription = theme_service.on_did_color_theme_change().listen(move |theme| {
                    if let Some(this) = weak.upgrade() {
                        this.apply_theme(Some(theme.clone()));
                    }
                });
                this.base.register(subscription);
            }
        }

        // Refire events
        let weak = Rc::downgrade(&this);
        let subscription = this.raw.on_data().listen(move |data| {
            if let Some(this) = weak.upgrade() {
                *this.last_input_event.borrow_mut() = Some(data.clone());
            }
        });
        this.base.register(subscription);

        if let Some(decoration_addon) = &this.decoration_addon {
            let terminal: Rc<dyn IDecorationAddonTerminal> = this.raw.clone();
            decoration_addon.activate(terminal);
            let on_did_request_run_command = this.on_did_request_run_command.clone();
            this.base.register(decoration_addon.on_did_request_run_command().listen(move |event| on_did_request_run_command.fire(event.clone())));
            let on_did_request_copy_as_html = this.on_did_request_copy_as_html.clone();
            this.base.register(decoration_addon.on_did_request_copy_as_html().listen(move |event| on_did_request_copy_as_html.fire(event.clone())));
            if !detached && let Some(lifecycle_service) = lifecycle_service {
                let weak = Rc::downgrade(decoration_addon);
                this.base.register(lifecycle_service.on_will_shutdown().listen(move |_| {
                    if let Some(decoration_addon) = weak.upgrade() {
                        decoration_addon.clear_decorations();
                    }
                }));
            }
        }

        this
    }
}

impl IDisposable for XtermTerminal {
    fn dispose(&self) {
        self.dispose_of_gpu_renderer();
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
