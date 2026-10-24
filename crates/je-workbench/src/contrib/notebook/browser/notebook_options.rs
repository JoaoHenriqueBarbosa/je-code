//! Porte de `src/vs/workbench/contrib/notebook/browser/notebookOptions.ts`: as opções de layout e de exibição do editor de
//! notebook, lidas da configuração, e as contas de medida que os view models de célula fazem com elas.
//!
//! Divergências:
//! - `NotebookLayoutConfiguration & NotebookDisplayOptions` (interseção de duas interfaces) é uma struct só,
//!   [`NotebookLayoutConfiguration`], com os campos das duas; `getDisplayOptions` devolve a mesma struct.
//! - `cellToolbarLocation: string | { [key: string]: string }` é [`CellToolbarLocation`]; `ShowCellStatusBarType` guarda o
//!   texto lido da configuração (`None` quando a chave não existe, o `undefined` do original, que não é `visible` nem
//!   `visibleAfterExecute` e portanto esconde a barra de status).
//! - O `ICodeEditorService` não existe no porte (o editor de código de célula é Fase 6): o ouvinte de
//!   `onDecorationTypeRegistered` que aumenta o `editorTopPadding` quando uma decoração `::before`/`::after` tem `top` não
//!   é portado, e o `_editorTopPadding` fica no valor inicial (12). `targetWindow` e `PixelRatio` também não existem.
//! - `_computeOutputLineHeight` com `lineHeight === 0` lê a altura de linha do editor pelo `FontMeasurements`, que mede a
//!   fonte na janela; sem janela, o porte usa a altura de linha pelo tamanho da fonte do editor (`editor.fontSize`, ou 14
//!   quando ausente) vezes o `GOLDEN_LINE_HEIGHT_RATIO` (1,35) arredondado, que é o que `createBareFontInfoFromRawSettings`
//!   calcula para `editor.lineHeight` zero.
//! - Os valores de configuração ausentes seguem a semântica do `undefined` do JavaScript: números ausentes são `NaN` (o que
//!   o `Math.round(undefined)` dá) e booleanos ausentes caem nos `??` do original.
//! - `previousModelToCompare` (um `observableValue` só lido pelo editor de diff visual) não é portado.
//! - `updateOptions` monta o `IConfigurationChangeEvent` sintético do original como o par (chave afetada) já resolvido, em
//!   [`NotebookOptions::update_configuration_for_keys`], porque o evento de configuração real tem muito estado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;

use crate::contrib::notebook::common::notebook_common::{
    InteractiveWindowCollapseCodeCells, NotebookCellCollapseState, NotebookCellDefaultCollapseConfig, NotebookCellInternalMetadata, NotebookSetting,
};
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;

const SCROLLABLE_ELEMENT_PADDING_TOP: f64 = 18.0;

pub const OUTPUT_INNER_CONTAINER_TOP_PADDING: f64 = 4.0;

/// `cellToolbarLocation: string | { [key: string]: string }`.
#[derive(Clone, Debug, PartialEq)]
pub enum CellToolbarLocation {
    Single(String),
    PerViewType(HashMap<String, String>),
}

/// O que `defaultConfigConstants` e `compactConfigConstants` definem.
#[derive(Clone, Copy, Debug)]
struct ConfigConstants {
    code_cell_left_margin: f64,
    cell_run_gutter: f64,
    markdown_cell_top_margin: f64,
    markdown_cell_bottom_margin: f64,
    markdown_cell_left_margin: f64,
    markdown_cell_gutter: f64,
    focus_indicator_left_margin: f64,
}

const DEFAULT_CONFIG_CONSTANTS: ConfigConstants = ConfigConstants {
    code_cell_left_margin: 28.0,
    cell_run_gutter: 32.0,
    markdown_cell_top_margin: 8.0,
    markdown_cell_bottom_margin: 8.0,
    markdown_cell_left_margin: 0.0,
    markdown_cell_gutter: 32.0,
    focus_indicator_left_margin: 4.0,
};

const COMPACT_CONFIG_CONSTANTS: ConfigConstants = ConfigConstants {
    code_cell_left_margin: 8.0,
    cell_run_gutter: 36.0,
    markdown_cell_top_margin: 6.0,
    markdown_cell_bottom_margin: 6.0,
    markdown_cell_left_margin: 8.0,
    markdown_cell_gutter: 36.0,
    focus_indicator_left_margin: 4.0,
};

/// `NotebookLayoutConfiguration & NotebookDisplayOptions`.
#[derive(Clone, Debug)]
pub struct NotebookLayoutConfiguration {
    // NotebookLayoutConfiguration
    pub cell_right_margin: f64,
    pub cell_run_gutter: f64,
    pub cell_top_margin: f64,
    pub cell_bottom_margin: f64,
    pub cell_output_padding: f64,
    pub code_cell_left_margin: f64,
    pub markdown_cell_left_margin: f64,
    pub markdown_cell_gutter: f64,
    pub markdown_cell_top_margin: f64,
    pub markdown_cell_bottom_margin: f64,
    pub markdown_preview_padding: f64,
    pub markdown_fold_hint_height: f64,
    pub editor_toolbar_height: f64,
    pub editor_top_padding: f64,
    pub editor_bottom_padding: f64,
    pub editor_bottom_padding_without_status_bar: f64,
    pub collapsed_indicator_height: f64,
    pub cell_status_bar_height: f64,
    pub focus_indicator_left_margin: f64,
    pub focus_indicator_gap: f64,
    // NotebookDisplayOptions
    pub show_cell_status_bar: Option<String>,
    pub cell_toolbar_location: CellToolbarLocation,
    pub cell_toolbar_interaction: Option<String>,
    pub compact_view: bool,
    pub focus_indicator: String,
    pub insert_toolbar_position: String,
    pub insert_toolbar_alignment: String,
    pub global_toolbar: bool,
    pub sticky_scroll_enabled: bool,
    pub sticky_scroll_mode: String,
    pub consolidated_output_button: bool,
    pub consolidated_run_button: bool,
    pub show_folding_controls: String,
    pub drag_and_drop_enabled: bool,
    pub interactive_window_collapse_code_cells: Option<InteractiveWindowCollapseCodeCells>,
    pub output_scrolling: bool,
    pub output_word_wrap: bool,
    pub output_line_limit: f64,
    pub output_linkify_file_paths: bool,
    pub output_minimal_error: bool,
    pub font_size: f64,
    pub output_font_size: f64,
    pub output_font_family: String,
    pub output_line_height: f64,
    pub markup_font_size: f64,
    pub markdown_line_height: f64,
    pub editor_options_customizations: Dynamic,
    pub markup_font_family: String,
    pub disable_rulers: Option<bool>,
}

/// `NotebookOptionsChangeEvent`: campos ausentes são falsos; `readonly` é opcional de verdade (`typeof e.readonly ===
/// 'boolean'` distingue "ausente" de "falso").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookOptionsChangeEvent {
    pub cell_status_bar_visibility: bool,
    pub cell_toolbar_location: bool,
    pub cell_toolbar_interaction: bool,
    pub editor_top_padding: bool,
    pub compact_view: bool,
    pub focus_indicator: bool,
    pub insert_toolbar_position: bool,
    pub insert_toolbar_alignment: bool,
    pub global_toolbar: bool,
    pub sticky_scroll_enabled: bool,
    pub sticky_scroll_mode: bool,
    pub show_folding_controls: bool,
    pub consolidated_output_button: bool,
    pub consolidated_run_button: bool,
    pub drag_and_drop_enabled: bool,
    pub font_size: bool,
    pub output_font_size: bool,
    pub markup_font_size: bool,
    pub markdown_line_height: bool,
    pub font_family: bool,
    pub output_font_family: bool,
    pub editor_options_customizations: bool,
    pub interactive_window_collapse_code_cells: bool,
    pub output_line_height: bool,
    pub output_word_wrap: bool,
    pub output_scrolling: bool,
    pub output_linkify_file_paths: bool,
    pub minimal_error: bool,
    pub readonly: Option<bool>,
    pub markup_font_family: bool,
}

/// `{ bottomToolbarGap, bottomToolbarHeight }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BottomToolbarDimensions {
    pub bottom_toolbar_gap: f64,
    pub bottom_toolbar_height: f64,
}

/// `{ top, bottom }` de `computeEditorPadding`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPadding {
    pub top: f64,
    pub bottom: f64,
}

/// `{ bottomIndicatorTop, verticalIndicatorHeight }` de `computeIndicatorPosition`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorPosition {
    pub bottom_indicator_top: f64,
    pub vertical_indicator_height: f64,
}

/// As sobreposições do construtor (`overrides`).
#[derive(Clone, Debug)]
pub struct NotebookOptionsOverrides {
    pub cell_toolbar_interaction: String,
    pub global_toolbar: bool,
    pub sticky_scroll_enabled: bool,
    pub drag_and_drop_enabled: bool,
    pub disable_rulers: bool,
}

fn config_string(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn config_bool(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

/// Número ausente é `NaN`, como `undefined` depois de uma conta.
fn config_number(value: &Dynamic) -> f64 {
    match value {
        Dynamic::Number(number) => *number,
        _ => f64::NAN,
    }
}

/// `NotebookOptions`.
pub struct NotebookOptions {
    base: Rc<Disposable>,
    configuration_service: Rc<dyn IConfigurationService>,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    is_readonly: Cell<bool>,
    overrides: Option<NotebookOptionsOverrides>,
    layout_configuration: RefCell<NotebookLayoutConfiguration>,
    on_did_change_options: Rc<Emitter<NotebookOptionsChangeEvent>>,
    editor_top_padding: Cell<f64>,
}

impl NotebookOptions {
    pub fn new(
        is_readonly: bool,
        overrides: Option<NotebookOptionsOverrides>,
        configuration_service: Rc<dyn IConfigurationService>,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_options = Emitter::<NotebookOptionsChangeEvent>::new();
        base.register(on_did_change_options.clone());

        let get = |key: &str| configuration_service.get_value(Some(key), None);
        let show_cell_status_bar = config_string(&get(NotebookSetting::SHOW_CELL_STATUS_BAR));
        let global_toolbar = overrides.as_ref().map(|o| o.global_toolbar).or_else(|| config_bool(&get(NotebookSetting::GLOBAL_TOOLBAR))).unwrap_or(true);
        let sticky_scroll_enabled =
            overrides.as_ref().map(|o| o.sticky_scroll_enabled).or_else(|| config_bool(&get(NotebookSetting::STICKY_SCROLL_ENABLED))).unwrap_or(false);
        let sticky_scroll_mode = config_string(&get(NotebookSetting::STICKY_SCROLL_MODE)).unwrap_or_else(|| "flat".to_string());
        let consolidated_output_button = config_bool(&get(NotebookSetting::CONSOLIDATED_OUTPUT_BUTTON)).unwrap_or(true);
        let consolidated_run_button = config_bool(&get(NotebookSetting::CONSOLIDATED_RUN_BUTTON)).unwrap_or(false);
        let drag_and_drop_enabled =
            overrides.as_ref().map(|o| o.drag_and_drop_enabled).or_else(|| config_bool(&get(NotebookSetting::DRAG_AND_DROP_ENABLED))).unwrap_or(true);
        let cell_toolbar_location = Self::read_cell_toolbar_location(&get(NotebookSetting::CELL_TOOLBAR_LOCATION));
        let cell_toolbar_interaction =
            overrides.as_ref().map(|o| o.cell_toolbar_interaction.clone()).or_else(|| config_string(&get(NotebookSetting::CELL_TOOLBAR_VISIBILITY)));
        let compact_view = config_bool(&get(NotebookSetting::COMPACT_VIEW)).unwrap_or(true);
        let focus_indicator = config_string(&get(NotebookSetting::FOCUS_INDICATOR)).unwrap_or_else(|| "gutter".to_string());
        let insert_toolbar_position = Self::compute_insert_toolbar_position_option(&configuration_service, is_readonly);
        let insert_toolbar_alignment = config_string(&get(NotebookSetting::EXPERIMENTAL_INSERT_TOOLBAR_ALIGNMENT)).unwrap_or_else(|| "center".to_string());
        let show_folding_controls = config_string(&get(NotebookSetting::SHOW_FOLDING_CONTROLS)).unwrap_or_else(|| "mouseover".to_string());
        let font_size = config_number(&get("editor.fontSize"));
        let markup_font_size = config_number(&get(NotebookSetting::MARKUP_FONT_SIZE));
        let markdown_line_height = config_number(&get(NotebookSetting::MARKDOWN_LINE_HEIGHT));
        let editor_options_customizations = match get(NotebookSetting::CELL_EDITOR_OPTIONS_CUSTOMIZATIONS) {
            value @ Dynamic::Object(_) => value,
            _ => Dynamic::Object(je_base::dynamic::ObjectRef::new()),
        };
        let interactive_window_collapse_code_cells = Self::read_interactive_window_collapse(&get(NotebookSetting::INTERACTIVE_WINDOW_COLLAPSE_CODE_CELLS));

        let output_line_height_setting_value = config_number(&get(NotebookSetting::OUTPUT_LINE_HEIGHT));
        let configured_output_font_size = config_number(&get(NotebookSetting::OUTPUT_FONT_SIZE));
        // `getValue<number>(outputFontSize) || fontSize`: 0 e NaN são falsos.
        let output_font_size = if configured_output_font_size != 0.0 && !configured_output_font_size.is_nan() { configured_output_font_size } else { font_size };
        let output_font_family = config_string(&get(NotebookSetting::OUTPUT_FONT_FAMILY)).unwrap_or_default();
        let output_scrolling = config_bool(&get(NotebookSetting::OUTPUT_SCROLLING)).unwrap_or(false);

        let output_line_height = Self::compute_output_line_height(&configuration_service, output_line_height_setting_value, output_font_size);
        let output_word_wrap = config_bool(&get(NotebookSetting::OUTPUT_WORD_WRAP)).unwrap_or(false);
        let output_line_limit = match get(NotebookSetting::TEXT_OUTPUT_LINE_LIMIT) {
            Dynamic::Number(limit) => limit,
            _ => 30.0,
        };
        let linkify_file_paths = config_bool(&get(NotebookSetting::LINKIFY_OUTPUT_FILE_PATHS)).unwrap_or(true);
        let minimal_errors = config_bool(&get(NotebookSetting::MINIMAL_ERROR_RENDERING)).unwrap_or(false);
        let markup_font_family = config_string(&get(NotebookSetting::MARKUP_FONT_FAMILY)).unwrap_or_default();

        let constants = if compact_view { COMPACT_CONFIG_CONSTANTS } else { DEFAULT_CONFIG_CONSTANTS };
        let layout_configuration = NotebookLayoutConfiguration {
            cell_right_margin: 16.0,
            cell_run_gutter: constants.cell_run_gutter,
            cell_top_margin: 6.0,
            cell_bottom_margin: 6.0,
            cell_output_padding: 8.0,
            code_cell_left_margin: constants.code_cell_left_margin,
            markdown_cell_left_margin: constants.markdown_cell_left_margin,
            markdown_cell_gutter: constants.markdown_cell_gutter,
            markdown_cell_top_margin: constants.markdown_cell_top_margin,
            markdown_cell_bottom_margin: constants.markdown_cell_bottom_margin,
            markdown_preview_padding: 8.0,
            markdown_fold_hint_height: 22.0,
            editor_toolbar_height: 0.0,
            editor_top_padding: 12.0,
            editor_bottom_padding: 4.0,
            editor_bottom_padding_without_status_bar: 12.0,
            collapsed_indicator_height: 28.0,
            cell_status_bar_height: 22.0,
            focus_indicator_left_margin: constants.focus_indicator_left_margin,
            focus_indicator_gap: 3.0,
            show_cell_status_bar,
            cell_toolbar_location,
            cell_toolbar_interaction,
            compact_view,
            focus_indicator,
            insert_toolbar_position,
            insert_toolbar_alignment,
            global_toolbar,
            sticky_scroll_enabled,
            sticky_scroll_mode,
            consolidated_output_button,
            consolidated_run_button,
            show_folding_controls,
            drag_and_drop_enabled,
            interactive_window_collapse_code_cells,
            output_scrolling,
            output_word_wrap,
            output_line_limit,
            output_linkify_file_paths: linkify_file_paths,
            output_minimal_error: minimal_errors,
            font_size,
            output_font_size,
            output_font_family,
            output_line_height,
            markup_font_size,
            markdown_line_height,
            editor_options_customizations,
            markup_font_family,
            disable_rulers: overrides.as_ref().map(|o| o.disable_rulers),
        };

        let this = Rc::new(Self {
            base,
            configuration_service: configuration_service.clone(),
            notebook_execution_state_service,
            is_readonly: Cell::new(is_readonly),
            overrides,
            layout_configuration: RefCell::new(layout_configuration),
            on_did_change_options,
            editor_top_padding: Cell::new(12.0),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(configuration_service.on_did_change_configuration().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.update_configuration(&|key: &str| event.affects_configuration(key, None));
            }
        }));
        this
    }

    pub fn on_did_change_options(&self) -> Event<NotebookOptionsChangeEvent> {
        self.on_did_change_options.event()
    }

    /// `updateOptions(isReadonly)`.
    pub fn update_options(&self, is_readonly: bool) {
        if self.is_readonly.get() != is_readonly {
            self.is_readonly.set(is_readonly);
            self.update_configuration_for_keys(&[NotebookSetting::INSERT_TOOLBAR_LOCATION]);
        }
    }

    /// O `_updateConfiguration` com um evento sintético cujas chaves afetadas são exatamente `keys` (o `affectsConfiguration`
    /// do evento de `updateOptions` do original só responde `true` para `notebook.insertToolbarLocation`).
    pub fn update_configuration_for_keys(&self, keys: &[&str]) {
        let keys: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
        self.update_configuration(&|key: &str| keys.iter().any(|k| k == key));
    }

    fn read_cell_toolbar_location(value: &Dynamic) -> CellToolbarLocation {
        match value {
            Dynamic::String(text) => CellToolbarLocation::Single(text.to_string_lossy()),
            Dynamic::Object(object) => CellToolbarLocation::PerViewType(
                object.entries().into_iter().filter_map(|(key, value)| config_string(&value).map(|value| (key.to_string_lossy(), value))).collect(),
            ),
            _ => CellToolbarLocation::PerViewType(HashMap::from([("default".to_string(), "right".to_string())])),
        }
    }

    fn read_interactive_window_collapse(value: &Dynamic) -> Option<InteractiveWindowCollapseCodeCells> {
        match config_string(value).as_deref() {
            Some("always") => Some(InteractiveWindowCollapseCodeCells::Always),
            Some("never") => Some(InteractiveWindowCollapseCodeCells::Never),
            Some("fromEditor") => Some(InteractiveWindowCollapseCodeCells::FromEditor),
            _ => None,
        }
    }

    fn compute_insert_toolbar_position_option(configuration_service: &Rc<dyn IConfigurationService>, is_read_only: bool) -> String {
        if is_read_only {
            "hidden".to_string()
        } else {
            config_string(&configuration_service.get_value(Some(NotebookSetting::INSERT_TOOLBAR_LOCATION), None)).unwrap_or_else(|| "both".to_string())
        }
    }

    fn compute_output_line_height(configuration_service: &Rc<dyn IConfigurationService>, line_height: f64, output_font_size: f64) -> f64 {
        const MINIMUM_LINE_HEIGHT: f64 = 9.0;
        let mut line_height = line_height;

        if line_height == 0.0 {
            // use editor line height
            let editor_font_size = match configuration_service.get_value(Some("editor.fontSize"), None) {
                Dynamic::Number(size) => size,
                _ => 14.0,
            };
            line_height = (editor_font_size * 1.35).round();
        } else if line_height < MINIMUM_LINE_HEIGHT {
            // Values too small to be line heights in pixels are in ems.
            let mut font_size = output_font_size;
            if font_size == 0.0 {
                font_size = config_number(&configuration_service.get_value(Some("editor.fontSize"), None));
            }

            line_height *= font_size;
        }

        // Enforce integer, minimum constraints
        line_height = line_height.round();
        if line_height < MINIMUM_LINE_HEIGHT {
            line_height = MINIMUM_LINE_HEIGHT;
        }

        line_height
    }

    fn update_configuration(&self, affects: &dyn Fn(&str) -> bool) {
        let cell_status_bar_visibility = affects(NotebookSetting::SHOW_CELL_STATUS_BAR);
        let cell_toolbar_location = affects(NotebookSetting::CELL_TOOLBAR_LOCATION);
        let cell_toolbar_interaction = affects(NotebookSetting::CELL_TOOLBAR_VISIBILITY);
        let compact_view = affects(NotebookSetting::COMPACT_VIEW);
        let focus_indicator = affects(NotebookSetting::FOCUS_INDICATOR);
        let insert_toolbar_position = affects(NotebookSetting::INSERT_TOOLBAR_LOCATION);
        let insert_toolbar_alignment = affects(NotebookSetting::EXPERIMENTAL_INSERT_TOOLBAR_ALIGNMENT);
        let global_toolbar = affects(NotebookSetting::GLOBAL_TOOLBAR);
        let sticky_scroll_enabled = affects(NotebookSetting::STICKY_SCROLL_ENABLED);
        let sticky_scroll_mode = affects(NotebookSetting::STICKY_SCROLL_MODE);
        let consolidated_output_button = affects(NotebookSetting::CONSOLIDATED_OUTPUT_BUTTON);
        let consolidated_run_button = affects(NotebookSetting::CONSOLIDATED_RUN_BUTTON);
        let show_folding_controls = affects(NotebookSetting::SHOW_FOLDING_CONTROLS);
        let drag_and_drop_enabled = affects(NotebookSetting::DRAG_AND_DROP_ENABLED);
        let font_size = affects("editor.fontSize");
        let output_font_size = affects(NotebookSetting::OUTPUT_FONT_SIZE);
        let markup_font_size = affects(NotebookSetting::MARKUP_FONT_SIZE);
        let markdown_line_height = affects(NotebookSetting::MARKDOWN_LINE_HEIGHT);
        let font_family = affects("editor.fontFamily");
        let output_font_family = affects(NotebookSetting::OUTPUT_FONT_FAMILY);
        let editor_options_customizations = affects(NotebookSetting::CELL_EDITOR_OPTIONS_CUSTOMIZATIONS);
        let interactive_window_collapse_code_cells = affects(NotebookSetting::INTERACTIVE_WINDOW_COLLAPSE_CODE_CELLS);
        let output_line_height = affects(NotebookSetting::OUTPUT_LINE_HEIGHT);
        let output_scrolling = affects(NotebookSetting::OUTPUT_SCROLLING);
        let output_word_wrap = affects(NotebookSetting::OUTPUT_WORD_WRAP);
        let output_linkify_file_paths = affects(NotebookSetting::LINKIFY_OUTPUT_FILE_PATHS);
        let minimal_error = affects(NotebookSetting::MINIMAL_ERROR_RENDERING);
        let markup_font_family = affects(NotebookSetting::MARKUP_FONT_FAMILY);

        if !(cell_status_bar_visibility
            || cell_toolbar_location
            || cell_toolbar_interaction
            || compact_view
            || focus_indicator
            || insert_toolbar_position
            || insert_toolbar_alignment
            || global_toolbar
            || sticky_scroll_enabled
            || sticky_scroll_mode
            || consolidated_output_button
            || consolidated_run_button
            || show_folding_controls
            || drag_and_drop_enabled
            || font_size
            || output_font_size
            || markup_font_size
            || markdown_line_height
            || font_family
            || output_font_family
            || editor_options_customizations
            || interactive_window_collapse_code_cells
            || output_line_height
            || output_scrolling
            || output_word_wrap
            || output_linkify_file_paths
            || minimal_error
            || markup_font_family)
        {
            return;
        }

        let service = &self.configuration_service;
        let get = |key: &str| service.get_value(Some(key), None);
        let mut configuration = self.layout_configuration.borrow().clone();

        if cell_status_bar_visibility {
            configuration.show_cell_status_bar = config_string(&get(NotebookSetting::SHOW_CELL_STATUS_BAR));
        }

        if cell_toolbar_location {
            configuration.cell_toolbar_location = Self::read_cell_toolbar_location(&get(NotebookSetting::CELL_TOOLBAR_LOCATION));
        }

        if cell_toolbar_interaction && self.overrides.is_none() {
            configuration.cell_toolbar_interaction = config_string(&get(NotebookSetting::CELL_TOOLBAR_VISIBILITY));
        }

        if focus_indicator {
            configuration.focus_indicator = config_string(&get(NotebookSetting::FOCUS_INDICATOR)).unwrap_or_else(|| "gutter".to_string());
        }

        if compact_view {
            let compact_view_value = config_bool(&get(NotebookSetting::COMPACT_VIEW)).unwrap_or(true);
            let constants = if compact_view_value { COMPACT_CONFIG_CONSTANTS } else { DEFAULT_CONFIG_CONSTANTS };
            configuration.code_cell_left_margin = constants.code_cell_left_margin;
            configuration.cell_run_gutter = constants.cell_run_gutter;
            configuration.markdown_cell_top_margin = constants.markdown_cell_top_margin;
            configuration.markdown_cell_bottom_margin = constants.markdown_cell_bottom_margin;
            configuration.markdown_cell_left_margin = constants.markdown_cell_left_margin;
            configuration.markdown_cell_gutter = constants.markdown_cell_gutter;
            configuration.focus_indicator_left_margin = constants.focus_indicator_left_margin;
            configuration.compact_view = compact_view_value;
        }

        if insert_toolbar_alignment {
            configuration.insert_toolbar_alignment = config_string(&get(NotebookSetting::EXPERIMENTAL_INSERT_TOOLBAR_ALIGNMENT)).unwrap_or_else(|| "center".to_string());
        }

        if insert_toolbar_position {
            configuration.insert_toolbar_position = Self::compute_insert_toolbar_position_option(service, self.is_readonly.get());
        }

        if global_toolbar && self.overrides.is_none() {
            configuration.global_toolbar = config_bool(&get(NotebookSetting::GLOBAL_TOOLBAR)).unwrap_or(true);
        }

        if sticky_scroll_enabled && self.overrides.is_none() {
            configuration.sticky_scroll_enabled = config_bool(&get(NotebookSetting::STICKY_SCROLL_ENABLED)).unwrap_or(false);
        }

        if sticky_scroll_mode {
            configuration.sticky_scroll_mode = config_string(&get(NotebookSetting::STICKY_SCROLL_MODE)).unwrap_or_else(|| "flat".to_string());
        }

        if consolidated_output_button {
            configuration.consolidated_output_button = config_bool(&get(NotebookSetting::CONSOLIDATED_OUTPUT_BUTTON)).unwrap_or(true);
        }

        if consolidated_run_button {
            configuration.consolidated_run_button = config_bool(&get(NotebookSetting::CONSOLIDATED_RUN_BUTTON)).unwrap_or(true);
        }

        if show_folding_controls {
            configuration.show_folding_controls = config_string(&get(NotebookSetting::SHOW_FOLDING_CONTROLS)).unwrap_or_else(|| "mouseover".to_string());
        }

        if drag_and_drop_enabled {
            configuration.drag_and_drop_enabled = config_bool(&get(NotebookSetting::DRAG_AND_DROP_ENABLED)).unwrap_or(true);
        }

        if font_size {
            configuration.font_size = config_number(&get("editor.fontSize"));
        }

        if output_font_size || font_size {
            let configured = config_number(&get(NotebookSetting::OUTPUT_FONT_SIZE));
            configuration.output_font_size = if configured != 0.0 && !configured.is_nan() { configured } else { configuration.font_size };
        }

        if markup_font_size {
            configuration.markup_font_size = config_number(&get(NotebookSetting::MARKUP_FONT_SIZE));
        }

        if markdown_line_height {
            configuration.markdown_line_height = config_number(&get(NotebookSetting::MARKDOWN_LINE_HEIGHT));
        }

        if output_font_family {
            configuration.output_font_family = config_string(&get(NotebookSetting::OUTPUT_FONT_FAMILY)).unwrap_or_default();
        }

        if editor_options_customizations {
            configuration.editor_options_customizations = get(NotebookSetting::CELL_EDITOR_OPTIONS_CUSTOMIZATIONS);
        }

        if interactive_window_collapse_code_cells {
            configuration.interactive_window_collapse_code_cells = Self::read_interactive_window_collapse(&get(NotebookSetting::INTERACTIVE_WINDOW_COLLAPSE_CODE_CELLS));
        }

        if output_line_height || font_size || output_font_size {
            let line_height = config_number(&get(NotebookSetting::OUTPUT_LINE_HEIGHT));
            configuration.output_line_height = Self::compute_output_line_height(service, line_height, configuration.output_font_size);
        }

        if output_word_wrap {
            configuration.output_word_wrap = config_bool(&get(NotebookSetting::OUTPUT_WORD_WRAP)).unwrap_or(false);
        }

        if output_scrolling {
            configuration.output_scrolling = config_bool(&get(NotebookSetting::OUTPUT_SCROLLING)).unwrap_or(false);
        }

        if output_linkify_file_paths {
            configuration.output_linkify_file_paths = config_bool(&get(NotebookSetting::LINKIFY_OUTPUT_FILE_PATHS)).unwrap_or(false);
        }

        if minimal_error {
            configuration.output_minimal_error = config_bool(&get(NotebookSetting::MINIMAL_ERROR_RENDERING)).unwrap_or(false);
        }

        if markup_font_family {
            configuration.markup_font_family = config_string(&get(NotebookSetting::MARKUP_FONT_FAMILY)).unwrap_or_default();
        }

        *self.layout_configuration.borrow_mut() = configuration;

        // trigger event
        self.on_did_change_options.fire(NotebookOptionsChangeEvent {
            cell_status_bar_visibility,
            cell_toolbar_location,
            cell_toolbar_interaction,
            editor_top_padding: false,
            compact_view,
            focus_indicator,
            insert_toolbar_position,
            insert_toolbar_alignment,
            global_toolbar,
            sticky_scroll_enabled,
            sticky_scroll_mode,
            show_folding_controls,
            consolidated_output_button,
            consolidated_run_button,
            drag_and_drop_enabled,
            font_size,
            output_font_size,
            markup_font_size,
            markdown_line_height,
            font_family,
            output_font_family,
            editor_options_customizations,
            interactive_window_collapse_code_cells,
            output_line_height,
            output_word_wrap,
            output_scrolling,
            output_linkify_file_paths,
            minimal_error,
            readonly: None,
            markup_font_family,
        });
    }

    pub fn get_cell_collapse_default(&self) -> NotebookCellDefaultCollapseConfig {
        let never = self.layout_configuration.borrow().interactive_window_collapse_code_cells == Some(InteractiveWindowCollapseCodeCells::Never);
        NotebookCellDefaultCollapseConfig {
            code_cell: Some(NotebookCellCollapseState { input_collapsed: Some(!never), output_collapsed: None }),
            markup_cell: None,
        }
    }

    /// `getLayoutConfiguration()` (e `getDisplayOptions()`): uma cópia do estado atual (o original devolve o objeto congelado).
    pub fn get_layout_configuration(&self) -> NotebookLayoutConfiguration {
        self.layout_configuration.borrow().clone()
    }

    pub fn get_display_options(&self) -> NotebookLayoutConfiguration {
        self.get_layout_configuration()
    }

    pub fn get_cell_editor_container_left_margin(&self) -> f64 {
        let configuration = self.layout_configuration.borrow();
        configuration.code_cell_left_margin + configuration.cell_run_gutter
    }

    pub fn compute_collapsed_markdown_cell_height(&self, view_type: &str) -> f64 {
        let BottomToolbarDimensions { bottom_toolbar_gap, .. } = self.compute_bottom_toolbar_dimensions(Some(view_type));
        let configuration = self.layout_configuration.borrow();
        configuration.markdown_cell_top_margin + configuration.collapsed_indicator_height + bottom_toolbar_gap + configuration.markdown_cell_bottom_margin
    }

    pub fn compute_bottom_toolbar_offset(&self, total_height: f64, view_type: &str) -> f64 {
        let BottomToolbarDimensions { bottom_toolbar_gap, bottom_toolbar_height } = self.compute_bottom_toolbar_dimensions(Some(view_type));
        total_height - bottom_toolbar_gap - bottom_toolbar_height / 2.0
    }

    pub fn compute_code_cell_editor_width(&self, outer_width: f64) -> f64 {
        let configuration = self.layout_configuration.borrow();
        outer_width - (configuration.code_cell_left_margin + configuration.cell_run_gutter + configuration.cell_right_margin)
    }

    pub fn compute_markdown_cell_editor_width(&self, outer_width: f64) -> f64 {
        let configuration = self.layout_configuration.borrow();
        outer_width - configuration.markdown_cell_gutter - configuration.markdown_cell_left_margin - configuration.cell_right_margin
    }

    pub fn compute_status_bar_height(&self) -> f64 {
        self.layout_configuration.borrow().cell_status_bar_height
    }

    fn compute_bottom_toolbar_dimensions_for(compact_view: bool, insert_toolbar_position: &str, insert_toolbar_alignment: &str, cell_toolbar: &str) -> BottomToolbarDimensions {
        if insert_toolbar_alignment == "left" || cell_toolbar != "hidden" {
            return BottomToolbarDimensions { bottom_toolbar_gap: 18.0, bottom_toolbar_height: 18.0 };
        }

        if insert_toolbar_position == "betweenCells" || insert_toolbar_position == "both" {
            if compact_view {
                BottomToolbarDimensions { bottom_toolbar_gap: 12.0, bottom_toolbar_height: 20.0 }
            } else {
                BottomToolbarDimensions { bottom_toolbar_gap: 20.0, bottom_toolbar_height: 20.0 }
            }
        } else {
            BottomToolbarDimensions { bottom_toolbar_gap: 0.0, bottom_toolbar_height: 0.0 }
        }
    }

    pub fn compute_bottom_toolbar_dimensions(&self, view_type: Option<&str>) -> BottomToolbarDimensions {
        let cell_toolbar_position = self.compute_cell_toolbar_location(view_type);
        let configuration = self.layout_configuration.borrow();
        Self::compute_bottom_toolbar_dimensions_for(
            configuration.compact_view,
            &configuration.insert_toolbar_position,
            &configuration.insert_toolbar_alignment,
            cell_toolbar_position,
        )
    }

    /// `'right' | 'left' | 'hidden'`.
    pub fn compute_cell_toolbar_location(&self, view_type: Option<&str>) -> &'static str {
        let configuration = self.layout_configuration.borrow();
        match &configuration.cell_toolbar_location {
            CellToolbarLocation::Single(location) => match location.as_str() {
                "left" => "left",
                "right" => "right",
                "hidden" => "hidden",
                _ => "right",
            },
            CellToolbarLocation::PerViewType(map) => {
                if let Some(view_type) = view_type {
                    let notebook_specific_setting = map.get(view_type).or_else(|| map.get("default"));
                    match notebook_specific_setting.map(String::as_str) {
                        Some("left") => "left",
                        Some("right") => "right",
                        Some("hidden") => "hidden",
                        _ => "right",
                    }
                } else {
                    "right"
                }
            }
        }
    }

    pub fn compute_top_insert_toolbar_height(&self, view_type: Option<&str>) -> f64 {
        {
            let configuration = self.layout_configuration.borrow();
            if configuration.insert_toolbar_position == "betweenCells" || configuration.insert_toolbar_position == "both" {
                return SCROLLABLE_ELEMENT_PADDING_TOP;
            }
        }

        let cell_toolbar_location = self.compute_cell_toolbar_location(view_type);
        if cell_toolbar_location == "left" || cell_toolbar_location == "right" {
            return SCROLLABLE_ELEMENT_PADDING_TOP;
        }

        0.0
    }

    pub fn compute_editor_padding(&self, internal_metadata: &NotebookCellInternalMetadata, cell_uri: &URI) -> EditorPadding {
        let configuration = self.layout_configuration.borrow();
        EditorPadding {
            top: self.editor_top_padding.get(),
            bottom: if self.status_bar_is_visible(internal_metadata, cell_uri) { configuration.editor_bottom_padding } else { configuration.editor_bottom_padding_without_status_bar },
        }
    }

    pub fn compute_editor_statusbar_height(&self, internal_metadata: &NotebookCellInternalMetadata, cell_uri: &URI) -> f64 {
        if self.status_bar_is_visible(internal_metadata, cell_uri) {
            self.compute_status_bar_height()
        } else {
            0.0
        }
    }

    fn status_bar_is_visible(&self, internal_metadata: &NotebookCellInternalMetadata, cell_uri: &URI) -> bool {
        let exe = self.notebook_execution_state_service.get_cell_execution(cell_uri);
        match self.layout_configuration.borrow().show_cell_status_bar.as_deref() {
            Some("visible") => true,
            Some("visibleAfterExecute") => internal_metadata.last_run_success.is_some() || exe.is_some(),
            _ => false,
        }
    }

    pub fn compute_indicator_position(&self, total_height: f64, fold_hint_height: f64, view_type: Option<&str>) -> IndicatorPosition {
        let BottomToolbarDimensions { bottom_toolbar_gap, .. } = self.compute_bottom_toolbar_dimensions(view_type);
        IndicatorPosition {
            bottom_indicator_top: total_height - bottom_toolbar_gap - self.layout_configuration.borrow().cell_bottom_margin - fold_hint_height,
            vertical_indicator_height: total_height - bottom_toolbar_gap - fold_hint_height,
        }
    }
}

impl IDisposable for NotebookOptions {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookOptions"
    }
}
