//! Porte de `src/vs/workbench/contrib/terminal/browser/terminalConfigurationService.ts`: o serviço que expõe a configuração
//! do terminal (`terminal.integrated`) já normalizada, a fonte efetiva (`getFont`) e a lista de comandos que pulam o shell.
//!
//! Divergências:
//! - **Injeção.** O parâmetro decorado do construtor (`@IConfigurationService`) é a tupla `Deps` do [`Injectable`]; a
//!   classe herda de `Disposable`, aqui um `Disposable` guardado no campo `base`. O `TerminalFontMetrics` do original
//!   recebe `this` no construtor, então o serviço é criado com `Rc::new_cyclic` e o medidor guarda um `Weak`.
//! - **`protected _fontMetrics` e `_config`.** O `TestTerminalConfigurationService` de `workbenchTestServices.ts` os expõe
//!   (`fontMetrics` e `setConfig`); aqui são os métodos públicos [`TerminalConfigurationService::font_metrics`] e
//!   [`TerminalConfigurationService::set_config`].
//! - **`ITerminalConfiguration`.** É um [`Dynamic`] (objeto com a cópia rasa da seção, `{ ...getValue(section) }`, com
//!   `fontWeight` e `fontWeightBold` normalizados). Os campos lidos têm acesso tipado em funções privadas, com a coerção
//!   do JavaScript (`||`, `parseInt`, `isNaN`).
//! - **DOM.** O elemento `div` de medição, o `HTMLElement` do painel e o `Window` viram [`ITerminalPanelContainer`] e o
//!   `devicePixelRatio` numérico (ver `terminal.rs`). O `try/catch` de `_createCharMeasureElementIfNecessary` (terminal não
//!   anexado) é a ausência do contêiner.
//! - **`clampInt`.** Devolve `None` quando o original devolve o `fallback`, e o chamador aplica o fallback.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::numbers::clamp;
use je_editor::config::font_info::editor_font_defaults;
use je_instantiation::{Injectable, service_impl};
use je_platform::configuration::service::IConfigurationService;
use je_platform::terminal::terminal::{TerminalLocation, TerminalLocationConfigValue};

use super::terminal::{ClientRect, I_TERMINAL_CONFIGURATION_SERVICE, ITerminalConfigurationService, ITerminalPanelContainer, IXtermCore, LinuxDistro};
use crate::contrib::terminal::common::terminal::{
    DEFAULT_BOLD_FONT_WEIGHT, DEFAULT_FONT_WEIGHT, DEFAULT_LETTER_SPACING, FontWeight, MAXIMUM_FONT_WEIGHT, MINIMUM_FONT_WEIGHT, MINIMUM_LETTER_SPACING,
    TERMINAL_CONFIG_SECTION, TerminalFont, default_commands_to_skip_shell, default_line_height,
};

// #region TerminalConfigurationService

/// `TerminalConfigurationService`.
pub struct TerminalConfigurationService {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,

    font_metrics: Rc<TerminalFontMetrics>,
    skip_terminal_commands: RefCell<HashSet<String>>,

    config: RefCell<Dynamic>,
    on_config_changed: Rc<Emitter<()>>,
}

impl Injectable for TerminalConfigurationService {
    type Args = ();
    type Deps = (Rc<dyn IConfigurationService>,);

    fn create(_: (), (configuration_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let on_config_changed = Emitter::new();
        base.register(on_config_changed.clone());

        let this = Rc::new_cyclic(|this: &Weak<Self>| {
            let font_metrics = TerminalFontMetrics::new(this.clone(), configuration_service.clone());
            base.register(font_metrics.clone());
            Self {
                base: base.clone(),
                configuration_service,
                font_metrics,
                skip_terminal_commands: RefCell::new(default_commands_to_skip_shell().into_iter().map(str::to_owned).collect()),
                config: RefCell::new(Dynamic::Undefined),
                on_config_changed,
            }
        });

        let weak = Rc::downgrade(&this);
        let subscription = this.configuration_service.on_did_change_configuration().run_and_subscribe(
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                if event.is_none_or(|event| event.affects_configuration(TERMINAL_CONFIG_SECTION, None)) {
                    this.update_config();
                }
            },
            None,
        );
        this.base.register(subscription);

        this
    }
}

service_impl!(TerminalConfigurationService => dyn ITerminalConfigurationService, disposable);

impl IDisposable for TerminalConfigurationService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TerminalConfigurationService {
    /// `protected _fontMetrics` (o `fontMetrics` do `TestTerminalConfigurationService`).
    pub fn font_metrics(&self) -> &Rc<TerminalFontMetrics> {
        &self.font_metrics
    }

    /// `setConfig` do `TestTerminalConfigurationService`: troca `_config` sem passar pela configuração.
    pub fn set_config(&self, config: Dynamic) {
        *self.config.borrow_mut() = config;
    }

    fn update_config(&self) {
        let config_values = Dynamic::object(self.configuration_service.get_value(Some(TERMINAL_CONFIG_SECTION), None).own_entries());
        let font_weight = normalize_font_weight(&config_values.get_str("fontWeight"), DEFAULT_FONT_WEIGHT);
        let font_weight_bold = normalize_font_weight(&config_values.get_str("fontWeightBold"), DEFAULT_BOLD_FONT_WEIGHT);
        if let Dynamic::Object(config_object) = &config_values {
            config_object.set(&JsString::from("fontWeight"), font_weight_to_dynamic(font_weight));
            config_object.set(&JsString::from("fontWeightBold"), font_weight_to_dynamic(font_weight_bold));
        }
        let commands_to_skip_shell = config_values.get_str("commandsToSkipShell");
        *self.config.borrow_mut() = config_values;

        let mut skip_terminal_commands: HashSet<String> = default_commands_to_skip_shell().into_iter().map(str::to_owned).collect();
        if let Dynamic::Array(commands) = &commands_to_skip_shell {
            for command in commands.to_vec() {
                let command = je_platform::js::to_js_string(&command).to_string_lossy();
                if let Some(excluded) = command.strip_prefix('-') {
                    skip_terminal_commands.remove(excluded);
                    continue;
                }
                skip_terminal_commands.insert(command);
            }
        }
        *self.skip_terminal_commands.borrow_mut() = skip_terminal_commands;
        self.on_config_changed.fire(());
    }
}

impl ITerminalConfigurationService for TerminalConfigurationService {
    fn config(&self) -> Dynamic {
        self.config.borrow().clone()
    }

    fn default_location(&self) -> TerminalLocation {
        if self.config.borrow().get_str("defaultLocation").strict_equals(&Dynamic::string(TerminalLocationConfigValue::Editor.as_str())) {
            return TerminalLocation::Editor;
        }
        TerminalLocation::Panel
    }

    fn on_config_changed(&self) -> Event<()> {
        self.on_config_changed.event()
    }

    fn set_panel_container(&self, panel_container: Rc<dyn ITerminalPanelContainer>) {
        self.font_metrics.set_panel_container(panel_container);
    }

    fn config_font_is_monospace(&self) -> bool {
        self.font_metrics.config_font_is_monospace()
    }

    fn get_font(&self, device_pixel_ratio: f64, xterm_core: Option<&dyn IXtermCore>, exclude_dimensions: bool) -> TerminalFont {
        self.font_metrics.get_font(device_pixel_ratio, xterm_core, exclude_dimensions)
    }

    fn should_command_skip_shell(&self, command_id: &str) -> bool {
        self.skip_terminal_commands.borrow().contains(command_id)
    }
}

/// `_normalizeFontWeight`.
fn normalize_font_weight(input: &Dynamic, default_weight: FontWeight) -> FontWeight {
    if let Dynamic::String(text) = input {
        if *text == "normal" {
            return FontWeight::Normal;
        }
        if *text == "bold" {
            return FontWeight::Bold;
        }
    }
    clamp_int(input, MINIMUM_FONT_WEIGHT, MAXIMUM_FONT_WEIGHT).map_or(default_weight, FontWeight::Number)
}

/// O valor de `fontWeight` como o `getValue` o devolve.
fn font_weight_to_dynamic(weight: FontWeight) -> Dynamic {
    match weight {
        FontWeight::Normal => Dynamic::string("normal"),
        FontWeight::Bold => Dynamic::string("bold"),
        FontWeight::Number(value) => Dynamic::Number(value),
    }
}

// #endregion TerminalConfigurationService

// #region TerminalFontMetrics

/// `FontConstants.MinimumFontSize`.
const MINIMUM_FONT_SIZE: f64 = 6.0;
/// `FontConstants.MaximumFontSize`.
const MAXIMUM_FONT_SIZE: f64 = 100.0;

/// `TerminalFontMetrics`.
pub struct TerminalFontMetrics {
    base: Rc<Disposable>,
    terminal_configuration_service: Weak<TerminalConfigurationService>,
    configuration_service: Rc<dyn IConfigurationService>,

    panel_container: Rc<RefCell<Option<Rc<dyn ITerminalPanelContainer>>>>,
    last_font_measurement: RefCell<Option<TerminalFont>>,

    linux_distro: Cell<LinuxDistro>,
}

impl TerminalFontMetrics {
    fn new(terminal_configuration_service: Weak<TerminalConfigurationService>, configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        let base = Disposable::new();
        let panel_container: Rc<RefCell<Option<Rc<dyn ITerminalPanelContainer>>>> = Rc::new(RefCell::new(None));
        let removable = panel_container.clone();
        base.register(to_disposable(move || {
            if let Some(container) = removable.borrow().as_ref() {
                container.remove_char_measure_element();
            }
        }));
        Rc::new(Self {
            base,
            terminal_configuration_service,
            configuration_service,
            panel_container,
            last_font_measurement: RefCell::new(None),
            linux_distro: Cell::new(LinuxDistro::Unknown),
        })
    }

    /// O campo público `linuxDistro`.
    pub fn linux_distro(&self) -> LinuxDistro {
        self.linux_distro.get()
    }

    /// Atribuição ao campo público `linuxDistro`.
    pub fn set_linux_distro(&self, linux_distro: LinuxDistro) {
        self.linux_distro.set(linux_distro);
    }

    /// `this._terminalConfigurationService.config`.
    fn terminal_config(&self) -> Dynamic {
        match self.terminal_configuration_service.upgrade() {
            Some(service) => service.config(),
            None => Dynamic::Undefined,
        }
    }

    pub fn set_panel_container(&self, panel_container: Rc<dyn ITerminalPanelContainer>) {
        *self.panel_container.borrow_mut() = Some(panel_container);
    }

    /// `config.fontFamily || editorConfig.fontFamily || EDITOR_FONT_DEFAULTS.fontFamily`.
    fn configured_font_family(&self, terminal_config: &Dynamic, editor_config: impl FnOnce() -> Dynamic) -> String {
        let terminal_font_family = terminal_config.get_str("fontFamily");
        if terminal_font_family.is_truthy() {
            return to_text(&terminal_font_family);
        }
        let editor_font_family = editor_config().get_str("fontFamily");
        if editor_font_family.is_truthy() {
            return to_text(&editor_font_family);
        }
        editor_font_defaults().font_family.to_owned()
    }

    pub fn config_font_is_monospace(&self) -> bool {
        let font_size = 15.0;
        let terminal_config = self.terminal_config();
        let font_family = self.configured_font_family(&terminal_config, || self.configuration_service.get_value(Some("editor"), None));
        let i_rect = self.get_bounding_rect_for("i", &font_family, font_size);
        let w_rect = self.get_bounding_rect_for("w", &font_family, font_size);

        // Check for invalid bounds, there is no reason to believe the font is not monospace
        match (i_rect, w_rect) {
            (Some(i_rect), Some(w_rect)) if i_rect.width.is_truthy() && w_rect.width.is_truthy() => i_rect.width == w_rect.width,
            _ => true,
        }
    }

    /// Gets the font information based on the terminal.integrated.fontFamily
    /// terminal.integrated.fontSize, terminal.integrated.lineHeight configuration properties
    pub fn get_font(&self, device_pixel_ratio: f64, xterm_core: Option<&dyn IXtermCore>, exclude_dimensions: bool) -> TerminalFont {
        let defaults = editor_font_defaults();
        let editor_config = self.configuration_service.get_value(Some("editor"), None);
        let terminal_config = self.terminal_config();

        let mut font_family = self.configured_font_family(&terminal_config, || editor_config.clone());
        if font_family.is_empty() {
            font_family = "monospace".to_owned();
        }
        let mut font_size = clamp_int(&terminal_config.get_str("fontSize"), MINIMUM_FONT_SIZE, MAXIMUM_FONT_SIZE).unwrap_or(defaults.font_size);

        // Work around bad font on Fedora/Ubuntu
        if !terminal_config.get_str("fontFamily").is_truthy() {
            if self.linux_distro.get() == LinuxDistro::Fedora {
                font_family = "'DejaVu Sans Mono'".to_owned();
            }
            if self.linux_distro.get() == LinuxDistro::Ubuntu {
                font_family = "'Ubuntu Mono'".to_owned();

                // Ubuntu mono is somehow smaller, so set fontSize a bit larger to get the same perceived size.
                font_size = clamp_int(&Dynamic::Number(font_size + 2.0), MINIMUM_FONT_SIZE, MAXIMUM_FONT_SIZE).unwrap_or(defaults.font_size);
            }
        }

        // Always fallback to monospace, otherwise a proportional font may become the default
        font_family.push_str(", monospace");

        // Always fallback to AppleBraille on macOS, otherwise braille will render with filled and
        // empty circles in all 8 positions, instead of just filled circles
        // See https://github.com/microsoft/vscode/issues/174521
        if je_base::platform::Platform::detect().is_macintosh {
            font_family.push_str(", AppleBraille");
        }

        let letter_spacing_config = terminal_config.get_str("letterSpacing");
        let letter_spacing = if letter_spacing_config.is_truthy() {
            to_number(&letter_spacing_config).floor().max(MINIMUM_LETTER_SPACING)
        } else {
            DEFAULT_LETTER_SPACING
        };
        let line_height_config = terminal_config.get_str("lineHeight");
        let line_height = if line_height_config.is_truthy() { to_number(&line_height_config).max(1.0) } else { default_line_height() };

        if exclude_dimensions {
            return TerminalFont { font_family, font_size, letter_spacing, line_height, char_width: None, char_height: None };
        }

        // Get the character dimensions from xterm if it's available
        if let Some(cell_dims) = xterm_core.and_then(|core| core.css_cell_dimensions())
            && cell_dims.width.is_truthy()
            && cell_dims.height.is_truthy()
        {
            return TerminalFont {
                font_family,
                font_size,
                letter_spacing,
                line_height,
                char_height: Some(cell_dims.height / line_height),
                char_width: Some(cell_dims.width - js_round(letter_spacing) / device_pixel_ratio),
            };
        }

        // Fall back to measuring the font ourselves
        self.measure_font(device_pixel_ratio, font_family, font_size, letter_spacing, line_height)
    }

    /// `_getBoundingRectFor`: `None` quando o terminal não está anexado (o `catch` de `_createCharMeasureElementIfNecessary`).
    fn get_bounding_rect_for(&self, text: &str, font_family: &str, font_size: f64) -> Option<ClientRect> {
        let container = self.panel_container.borrow().clone()?;
        Some(container.measure_text(text, font_family, font_size))
    }

    fn measure_font(&self, device_pixel_ratio: f64, font_family: String, font_size: f64, letter_spacing: f64, line_height: f64) -> TerminalFont {
        let rect = self.get_bounding_rect_for("X", &font_family, font_size);
        let valid_rect = rect.filter(|rect| rect.width.is_truthy() && rect.height.is_truthy());

        // Bounding client rect was invalid, use last font measurement if available.
        if valid_rect.is_none()
            && let Some(last) = self.last_font_measurement.borrow().clone()
        {
            return last;
        }

        let mut measurement = TerminalFont { font_family, font_size, letter_spacing, line_height, char_width: Some(0.0), char_height: Some(0.0) };

        if let Some(rect) = valid_rect {
            measurement.char_height = Some(rect.height.ceil());
            // Char width is calculated differently for DOM and the other renderer types. Refer to
            // how each renderer updates their dimensions in xterm.js
            let gpu_acceleration_off = self.terminal_config().get_str("gpuAcceleration").strict_equals(&Dynamic::string("off"));
            measurement.char_width = Some(if gpu_acceleration_off {
                rect.width
            } else {
                let device_char_width = (rect.width * device_pixel_ratio).floor();
                let device_cell_width = device_char_width + js_round(letter_spacing);
                let css_cell_width = device_cell_width / device_pixel_ratio;
                css_cell_width - js_round(letter_spacing) / device_pixel_ratio
            });
        }

        *self.last_font_measurement.borrow_mut() = Some(measurement.clone());
        measurement
    }
}

impl IDisposable for TerminalFontMetrics {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// #endregion TerminalFontMetrics

// #region Utils

/// `Math.round`: arredonda para cima nos meios (`-2.5` vira `-2`).
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `parseInt(text, 10)`: `NaN` quando não há dígito.
fn parse_int_base10(text: &str) -> f64 {
    let text = text.trim_start_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}');
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let mut value = 0.0;
    let mut any = false;
    for digit in digits.chars().map_while(|c| c.to_digit(10)) {
        value = value * 10.0 + f64::from(digit);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

/// `Number(value)` para os tipos que a configuração traz.
fn to_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        Dynamic::Bool(flag) => f64::from(u8::from(*flag)),
        Dynamic::Null => 0.0,
        Dynamic::String(text) => {
            let text = text.to_string_lossy();
            let trimmed = text.trim();
            if trimmed.is_empty() { 0.0 } else { trimmed.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

/// `String(value)` de um valor truthy da configuração.
fn to_text(value: &Dynamic) -> String {
    je_platform::js::to_js_string(value).to_string_lossy()
}

/// `clampInt(source, minimum, maximum, fallback)`: `None` é o `fallback`.
fn clamp_int(source: &Dynamic, minimum: f64, maximum: f64) -> Option<f64> {
    if source.is_nullish() {
        return None;
    }
    let value = match source {
        Dynamic::String(text) => parse_int_base10(&text.to_string_lossy()),
        other => to_number(other),
    };
    if value.is_nan() {
        return None;
    }
    Some(clamp(value, minimum, maximum))
}

// #endregion Utils
