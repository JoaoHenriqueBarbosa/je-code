//! Porte de `src/vs/editor/browser/widget/diffEditor/diffEditorOptions.ts`: as opções de um editor de diff como observáveis
//! (`DiffEditorOptions`), a validação delas (`validateDiffEditorOptions`) e o `DiffEditorVariant` (que vive em
//! [`crate::multi_diff_editor_options`]).
//!
//! Divergências:
//! - **`IDiffEditorOptions` é um [`Dynamic`].** `IDiffEditorOptions extends IEditorOptions, IDiffEditorBaseOptions` mistura as
//!   opções do editor de código (cujo conjunto completo ainda não tem um tipo só) com as do diff; como o `validate*` do
//!   original e das demais opções do porte (`editor_options.rs`), a entrada é um objeto [`Dynamic`]. O estado guardado
//!   ([`DiffEditorOptionsState`]) separa o `IEditorOptions` (o objeto recebido, com as mudanças de `updateOptions` espalhadas
//!   por cima, como o `{ ...currentOptions, ...changedOptions }`) do `Required<IDiffEditorBaseOptions>` já validado
//!   ([`ValidDiffEditorBaseOptions`]); os campos do diff que o objeto cru ainda traga são ignorados, porque quem os lê é o validado.
//! - `readOnly` (`IEditorOptions.readOnly`, `boolean | undefined`) é lido só como condição (`if (readOnly)`), então o observável
//!   [`DiffEditorOptions::read_only`] é o `bool` da veracidade do valor.
//! - `ISettableObservable<..., { changedOptions }>` é o `ObservableValue` com o [`DiffEditorOptionsChange`] como tipo de mudança
//!   (o observável é sempre notificado, como o `set(newOptions, tx, { changedOptions })` do original, que passa uma mudança).
//! - `IAccessibilityService` é o de `je_platform::accessibility`, injetado por [`Injectable`]; o `options` é o argumento estático.
//! - `derivedConstOnceDefined` de `shouldRenderInlineViewInSmartMode` é o `derived_observable_with_cache` com `lastValue ?? fn(reader)`
//!   (o `derived_const_once_defined` do porte não recomputa um valor `undefined` do `fn`, ao contrário do original).
//! - `setModel` guarda um `Rc<DiffEditorViewModel>`, e o view model guarda estas opções: o ciclo que o coletor de lixo do original
//!   desfaz é desfeito por quem chama `set_model(None)` ao descartar o widget.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::observables::observable_from_event::observable_from_event_owned;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    DebugOwner, FlattenExt, IObservable, ITransaction, Observable, ObservableExt, Settable, SettableExt, StrictEquals, derived_observable_with_cache, transaction,
};
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_instantiation::Injectable;
use je_platform::accessibility::IAccessibilityService;

use crate::config::diff_editor::{
    DIFF_EDITOR_DEFAULT_OPTIONS, DiffWordWrap, ValidDiffEditorBaseOptions, ValidExperimentalDiffEditorOptions, ValidHideUnchangedRegionsOptions,
    diff_algorithm_name_as_str, diff_algorithm_name_from_str,
};
use crate::config::editor_options::{boolean, clamped_float, clamped_int, string_set};
use crate::diff::range_mapping::LineRangeMapping;
use crate::diff_editor_view_model::{DiffEditorViewModel, DiffState};
use crate::diff_editor_view_zones::allows_true_inline_diff_rendering;
pub use crate::multi_diff_editor_options::DiffEditorVariant;

/// O estado das opções: o `IEditorOptions & Required<IDiffEditorBaseOptions>` do original.
#[derive(Clone)]
pub struct DiffEditorOptionsState {
    /// A parte `IEditorOptions` (o objeto recebido mais as mudanças de `updateOptions`).
    pub editor_options: Dynamic,
    /// A parte `Required<IDiffEditorBaseOptions>`.
    pub valid: ValidDiffEditorBaseOptions,
}

/// A mudança `{ changedOptions: IDiffEditorOptions }` que `updateOptions` passa ao observável.
#[derive(Clone)]
pub struct DiffEditorOptionsChange {
    pub changed_options: Dynamic,
}

/// `'auto' | 'inline'` de `_widthBasedLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WidthBasedLayout {
    Auto,
    Inline,
}

impl StrictEquals for WidthBasedLayout {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

type OptionsValue = Rc<ObservableValue<Rc<DiffEditorOptionsState>, DiffEditorOptionsChange>>;

/// `derived(this, reader => this._options.read(reader).<campo>)`: o observável de um campo das opções validadas.
fn option_field<T: Clone + StrictEquals + 'static>(
    owner: &DebugOwner,
    options: &OptionsValue,
    field: impl Fn(&ValidDiffEditorBaseOptions) -> T + 'static,
) -> IObservable<T> {
    let options = options.clone();
    derived_owned(owner.clone(), move |reader| field(&options.read(reader).valid))
}

/// `DiffEditorOptions`.
pub struct DiffEditorOptions {
    options: OptionsValue,
    diff_editor_width: Rc<ObservableValue<f64, ()>>,
    width_based_layout: Rc<ObservableValue<WidthBasedLayout, ()>>,
    screen_reader_mode: IObservable<bool>,
    model: Rc<ObservableValue<Option<Rc<DiffEditorViewModel>>, ()>>,

    pub could_show_inline_view_because_of_size: IObservable<bool>,
    pub render_overview_ruler: IObservable<bool>,
    pub render_side_by_side_in_automatic_mode: IObservable<bool>,
    pub render_side_by_side: IObservable<bool>,
    pub temporary_inline_mode: IObservable<bool>,
    pub read_only: IObservable<bool>,
    pub should_render_old_revert_arrows: IObservable<bool>,
    pub should_render_gutter_menu: IObservable<bool>,
    pub render_indicators: IObservable<bool>,
    pub enable_split_view_resizing: IObservable<bool>,
    pub split_view_default_ratio: IObservable<f64>,
    pub ignore_trim_whitespace: IObservable<bool>,
    pub max_computation_time_ms: IObservable<f64>,
    pub show_moves: IObservable<bool>,
    pub is_in_embedded_editor: IObservable<bool>,
    pub diff_word_wrap: IObservable<DiffWordWrap>,
    pub original_editable: IObservable<bool>,
    pub diff_code_lens: IObservable<bool>,
    pub accessibility_verbose: IObservable<bool>,
    pub diff_algorithm: IObservable<crate::editor_worker::DiffAlgorithmName>,
    pub show_empty_decorations: IObservable<bool>,
    pub only_show_accessible_diff_viewer: IObservable<bool>,
    pub compact_mode: IObservable<bool>,
    /// `trueInlineDiffRenderingEnabled` (privado no original, lido só pelos derivados daqui; público para não ficar sem leitor).
    pub true_inline_diff_rendering_enabled: IObservable<bool>,
    pub use_true_inline_diff_rendering: IObservable<bool>,
    pub hide_unchanged_regions: IObservable<bool>,
    pub hide_unchanged_regions_reveal_line_count: IObservable<i32>,
    pub hide_unchanged_regions_context_line_count: IObservable<i32>,
    pub hide_unchanged_regions_minimum_line_count: IObservable<i32>,
    should_render_inline_view_in_smart_mode: IObservable<bool>,
    pub inline_view_hide_original_line_numbers: IObservable<bool>,
}

impl Injectable for DiffEditorOptions {
    /// `options: Readonly<IDiffEditorOptions>`.
    type Args = Dynamic;
    type Deps = (Rc<dyn IAccessibilityService>,);

    fn create(options: Dynamic, (accessibility_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self::new(&options, accessibility_service))
    }
}

impl DiffEditorOptions {
    pub fn new(options: &Dynamic, accessibility_service: Rc<dyn IAccessibilityService>) -> Self {
        let owner = DebugOwner::new("DiffEditorOptions");

        let diff_editor_width = observable_value_owned::<f64, ()>(owner.clone(), 0.0);
        let width_based_layout = observable_value_owned::<WidthBasedLayout, ()>(owner.clone(), WidthBasedLayout::Auto);
        let screen_reader_mode = {
            let service = accessibility_service.clone();
            observable_from_event_owned(owner.clone(), accessibility_service.on_did_change_screen_reader_optimized(), move |_| service.is_screen_reader_optimized())
        };
        let initial = Rc::new(DiffEditorOptionsState { editor_options: options.clone(), valid: validate_diff_editor_options(options, &DIFF_EDITOR_DEFAULT_OPTIONS) });
        let options_value: OptionsValue = observable_value_owned(owner.clone(), initial);
        let model = observable_value_owned::<Option<Rc<DiffEditorViewModel>>, ()>(owner.clone(), None);

        let field = |f: fn(&ValidDiffEditorBaseOptions) -> bool| option_field(&owner, &options_value, f);

        let compact_mode = field(|o| o.compact_mode);
        let render_gutter_menu = field(|o| o.render_gutter_menu);
        let true_inline_diff_rendering_enabled = field(|o| o.experimental.use_true_inline_view);

        // `shouldRenderInlineViewInSmartMode`.
        let should_render_inline_view_in_smart_mode = {
            let (owner_for_cache, true_inline) = (owner.clone(), true_inline_diff_rendering_enabled.clone());
            let nested: IObservable<IObservable<Option<bool>>> = model.map(move |model, _| {
                let true_inline = true_inline.clone();
                derived_observable_with_cache::<Option<bool>>(Some(owner_for_cache.clone()), move |reader, last_value| {
                    // `lastValue ?? fn(reader)`
                    if let Some(Some(value)) = last_value {
                        return Some(*value);
                    }
                    let diffs = model.as_ref().and_then(|model| model.diff.read(reader));
                    diffs.map(|diffs| is_simple_diff(&diffs, true_inline.read(reader)))
                })
            });
            nested.flatten().map(|value, _| value.unwrap_or(false))
        };

        let could_show_inline_view_because_of_size = {
            let (options_value, width) = (options_value.clone(), diff_editor_width.clone());
            derived_owned(owner.clone(), move |reader| {
                let valid = options_value.read(reader).valid;
                valid.render_side_by_side && width.read(reader) <= f64::from(valid.render_side_by_side_inline_breakpoint)
            })
        };
        let render_side_by_side_in_automatic_mode = {
            let (options_value, width, compact_mode, smart, screen_reader) =
                (options_value.clone(), diff_editor_width.clone(), compact_mode.clone(), should_render_inline_view_in_smart_mode.clone(), screen_reader_mode.clone());
            derived_owned(owner.clone(), move |reader| {
                if compact_mode.read(reader) && smart.read(reader) {
                    return false;
                }
                width.read(reader) > f64::from(options_value.read(reader).valid.render_side_by_side_inline_breakpoint) || screen_reader.read(reader)
            })
        };
        let render_side_by_side = {
            let (options_value, layout, compact_mode, smart, screen_reader, could_show) = (
                options_value.clone(),
                width_based_layout.clone(),
                compact_mode.clone(),
                should_render_inline_view_in_smart_mode.clone(),
                screen_reader_mode.clone(),
                could_show_inline_view_because_of_size.clone(),
            );
            derived_owned(owner.clone(), move |reader| {
                if compact_mode.read(reader) && smart.read(reader) {
                    return false;
                }
                options_value.read(reader).valid.render_side_by_side
                    && layout.read(reader) == WidthBasedLayout::Auto
                    && !(options_value.read(reader).valid.use_inline_view_when_space_is_limited && could_show.read(reader) && !screen_reader.read(reader))
            })
        };
        let read_only = {
            let options_value = options_value.clone();
            derived_owned(owner.clone(), move |reader| options_value.read(reader).editor_options.get_str("readOnly").is_truthy())
        };
        let should_render_old_revert_arrows = {
            let (options_value, render_side_by_side, read_only, render_gutter_menu) =
                (options_value.clone(), render_side_by_side.clone(), read_only.clone(), render_gutter_menu.clone());
            derived_owned(owner.clone(), move |reader| {
                if !options_value.read(reader).valid.render_margin_revert_icon {
                    return false;
                }
                if !render_side_by_side.read(reader) {
                    return false;
                }
                if read_only.read(reader) {
                    return false;
                }
                if render_gutter_menu.read(reader) {
                    return false;
                }
                true
            })
        };
        let show_moves = {
            let (options_value, render_side_by_side) = (options_value.clone(), render_side_by_side.clone());
            derived_owned(owner.clone(), move |reader| options_value.read(reader).valid.experimental.show_moves && render_side_by_side.read(reader))
        };
        let use_true_inline_diff_rendering = {
            let (render_side_by_side, enabled) = (render_side_by_side.clone(), true_inline_diff_rendering_enabled.clone());
            derived_owned(owner.clone(), move |reader| !render_side_by_side.read(reader) && enabled.read(reader))
        };
        let inline_view_hide_original_line_numbers = {
            let (options_value, compact_mode) = (options_value.clone(), compact_mode.clone());
            derived_owned(owner.clone(), move |reader| options_value.read(reader).valid.hide_original_line_numbers || compact_mode.read(reader))
        };
        let temporary_inline_mode = width_based_layout.map(|layout, _| layout == WidthBasedLayout::Inline);

        Self {
            diff_editor_width,
            screen_reader_mode,
            model,
            could_show_inline_view_because_of_size,
            render_overview_ruler: field(|o| o.render_overview_ruler),
            render_side_by_side_in_automatic_mode,
            render_side_by_side,
            temporary_inline_mode,
            read_only,
            should_render_old_revert_arrows,
            should_render_gutter_menu: render_gutter_menu,
            render_indicators: field(|o| o.render_indicators),
            enable_split_view_resizing: field(|o| o.enable_split_view_resizing),
            split_view_default_ratio: option_field(&owner, &options_value, |o| o.split_view_default_ratio),
            ignore_trim_whitespace: field(|o| o.ignore_trim_whitespace),
            max_computation_time_ms: option_field(&owner, &options_value, |o| o.max_computation_time),
            show_moves,
            is_in_embedded_editor: field(|o| o.is_in_embedded_editor),
            diff_word_wrap: option_field(&owner, &options_value, |o| o.diff_word_wrap),
            original_editable: field(|o| o.original_editable),
            diff_code_lens: field(|o| o.diff_code_lens),
            accessibility_verbose: field(|o| o.accessibility_verbose),
            diff_algorithm: option_field(&owner, &options_value, |o| o.diff_algorithm),
            show_empty_decorations: field(|o| o.experimental.show_empty_decorations),
            only_show_accessible_diff_viewer: field(|o| o.only_show_accessible_diff_viewer),
            compact_mode,
            true_inline_diff_rendering_enabled,
            use_true_inline_diff_rendering,
            hide_unchanged_regions: field(|o| o.hide_unchanged_regions.enabled),
            hide_unchanged_regions_reveal_line_count: option_field(&owner, &options_value, |o| o.hide_unchanged_regions.reveal_line_count),
            hide_unchanged_regions_context_line_count: option_field(&owner, &options_value, |o| o.hide_unchanged_regions.context_line_count),
            hide_unchanged_regions_minimum_line_count: option_field(&owner, &options_value, |o| o.hide_unchanged_regions.minimum_line_count),
            should_render_inline_view_in_smart_mode,
            inline_view_hide_original_line_numbers,
            options: options_value,
            width_based_layout,
        }
    }

    /// `get editorOptions()`: o estado completo das opções; os observadores com delta recebem o [`DiffEditorOptionsChange`].
    pub fn editor_options(&self) -> IObservable<Rc<DiffEditorOptionsState>> {
        self.options.clone()
    }

    pub fn update_options(&self, changed_options: &Dynamic) {
        let current_options = self.options.get();
        let new_diff_editor_options = validate_diff_editor_options(changed_options, &current_options.valid);
        let new_options = Rc::new(DiffEditorOptionsState {
            editor_options: spread_options(&current_options.editor_options, changed_options),
            valid: new_diff_editor_options,
        });
        let current = current_options.valid;
        transaction(|tx| {
            if current.render_side_by_side != new_diff_editor_options.render_side_by_side
                || current.use_inline_view_when_space_is_limited != new_diff_editor_options.use_inline_view_when_space_is_limited
                || current.render_side_by_side_inline_breakpoint != new_diff_editor_options.render_side_by_side_inline_breakpoint
            {
                self.width_based_layout.set(WidthBasedLayout::Auto, Some(tx));
            }
            self.options.set_with_change(new_options, Some(tx), DiffEditorOptionsChange { changed_options: changed_options.clone() });
        });
    }

    pub fn set_width(&self, width: f64, smooth_resize_start_width: Option<f64>) {
        let options = self.options.get().valid;
        let breakpoint = f64::from(options.render_side_by_side_inline_breakpoint);
        transaction(|tx| {
            self.diff_editor_width.set(width, Some(tx));
            if width <= breakpoint {
                self.width_based_layout.set(WidthBasedLayout::Auto, Some(tx));
            } else if let Some(start_width) = smooth_resize_start_width
                && self.width_based_layout.get() == WidthBasedLayout::Auto
                && options.render_side_by_side
                && options.use_inline_view_when_space_is_limited
                && start_width <= breakpoint
                && width > breakpoint
                && !self.screen_reader_mode.get()
                && !(self.compact_mode.get() && self.should_render_inline_view_in_smart_mode.get())
            {
                self.width_based_layout.set(WidthBasedLayout::Inline, Some(tx));
            }
        });
    }

    pub fn reset_width_based_layout(&self, tx: Option<&dyn ITransaction>) {
        self.width_based_layout.set(WidthBasedLayout::Auto, tx);
    }

    pub fn set_model(&self, model: Option<Rc<DiffEditorViewModel>>) {
        self.model.set(model, None);
    }
}

fn is_simple_diff(diff: &DiffState, supports_true_diff_rendering: bool) -> bool {
    diff.mappings.iter().all(|m| {
        is_insertion(&m.line_range_mapping)
            || is_deletion(&m.line_range_mapping)
            || (supports_true_diff_rendering && allows_true_inline_diff_rendering(&m.line_range_mapping))
    })
}

fn is_insertion(mapping: &LineRangeMapping) -> bool {
    mapping.original.length() == 0
}

fn is_deletion(mapping: &LineRangeMapping) -> bool {
    mapping.modified.length() == 0
}

/// `{ ...base, ...overrides }`: as propriedades de `overrides` entram por cima, mantendo a posição das que já existiam.
pub fn spread_options(base: &Dynamic, overrides: &Dynamic) -> Dynamic {
    let result = ObjectRef::from_entries(base.own_entries());
    for (key, value) in overrides.own_entries() {
        result.set(&key, value);
    }
    Dynamic::Object(result)
}

/// `validateDiffEditorOptions(options, defaults)`.
pub fn validate_diff_editor_options(options: &Dynamic, defaults: &ValidDiffEditorBaseOptions) -> ValidDiffEditorBaseOptions {
    let max = f64::from(MAX_SAFE_SMALL_INTEGER);
    let experimental = options.get_str("experimental");
    let hide_unchanged_regions = options.get_str("hideUnchangedRegions");
    let enabled = {
        let enabled = hide_unchanged_regions.optional_get_str("enabled");
        // `options.hideUnchangedRegions?.enabled ?? options.experimental?.collapseUnchangedRegions`
        if enabled.is_nullish() { experimental.optional_get_str("collapseUnchangedRegions") } else { enabled }
    };
    let diff_algorithm = string_set(
        &options.get_str("diffAlgorithm"),
        diff_algorithm_name_as_str(defaults.diff_algorithm),
        &["legacy", "advanced", "advanced-external", "advanced-wasm"],
        Some(&[("smart", "legacy"), ("experimental", "advanced")]),
    );
    let diff_word_wrap = string_set(&options.get_str("diffWordWrap"), defaults.diff_word_wrap.as_str(), &["off", "on", "inherit"], None);

    ValidDiffEditorBaseOptions {
        enable_split_view_resizing: boolean(&options.get_str("enableSplitViewResizing"), defaults.enable_split_view_resizing),
        split_view_default_ratio: clamped_float(&options.get_str("splitViewDefaultRatio"), 0.5, 0.1, 0.9),
        render_side_by_side: boolean(&options.get_str("renderSideBySide"), defaults.render_side_by_side),
        render_margin_revert_icon: boolean(&options.get_str("renderMarginRevertIcon"), defaults.render_margin_revert_icon),
        max_computation_time: clamped_int(&options.get_str("maxComputationTime"), defaults.max_computation_time, 0.0, max),
        max_file_size: clamped_int(&options.get_str("maxFileSize"), defaults.max_file_size, 0.0, max),
        ignore_trim_whitespace: boolean(&options.get_str("ignoreTrimWhitespace"), defaults.ignore_trim_whitespace),
        render_indicators: boolean(&options.get_str("renderIndicators"), defaults.render_indicators),
        original_editable: boolean(&options.get_str("originalEditable"), defaults.original_editable),
        diff_code_lens: boolean(&options.get_str("diffCodeLens"), defaults.diff_code_lens),
        render_overview_ruler: boolean(&options.get_str("renderOverviewRuler"), defaults.render_overview_ruler),
        diff_word_wrap: DiffWordWrap::from_text(&diff_word_wrap).unwrap_or(defaults.diff_word_wrap),
        diff_algorithm: diff_algorithm_name_from_str(&diff_algorithm).unwrap_or(defaults.diff_algorithm),
        accessibility_verbose: boolean(&options.get_str("accessibilityVerbose"), defaults.accessibility_verbose),
        experimental: ValidExperimentalDiffEditorOptions {
            show_moves: boolean(&experimental.optional_get_str("showMoves"), defaults.experimental.show_moves),
            show_empty_decorations: boolean(&experimental.optional_get_str("showEmptyDecorations"), defaults.experimental.show_empty_decorations),
            use_true_inline_view: boolean(&experimental.optional_get_str("useTrueInlineView"), defaults.experimental.use_true_inline_view),
        },
        hide_unchanged_regions: ValidHideUnchangedRegionsOptions {
            enabled: boolean(&enabled, defaults.hide_unchanged_regions.enabled),
            context_line_count: clamped_int(
                &hide_unchanged_regions.optional_get_str("contextLineCount"),
                f64::from(defaults.hide_unchanged_regions.context_line_count),
                0.0,
                max,
            ) as i32,
            minimum_line_count: clamped_int(
                &hide_unchanged_regions.optional_get_str("minimumLineCount"),
                f64::from(defaults.hide_unchanged_regions.minimum_line_count),
                0.0,
                max,
            ) as i32,
            reveal_line_count: clamped_int(
                &hide_unchanged_regions.optional_get_str("revealLineCount"),
                f64::from(defaults.hide_unchanged_regions.reveal_line_count),
                0.0,
                max,
            ) as i32,
        },
        is_in_embedded_editor: boolean(&options.get_str("isInEmbeddedEditor"), defaults.is_in_embedded_editor),
        only_show_accessible_diff_viewer: boolean(&options.get_str("onlyShowAccessibleDiffViewer"), defaults.only_show_accessible_diff_viewer),
        render_side_by_side_inline_breakpoint: clamped_int(
            &options.get_str("renderSideBySideInlineBreakpoint"),
            f64::from(defaults.render_side_by_side_inline_breakpoint),
            0.0,
            max,
        ) as i32,
        use_inline_view_when_space_is_limited: boolean(&options.get_str("useInlineViewWhenSpaceIsLimited"), defaults.use_inline_view_when_space_is_limited),
        render_gutter_menu: boolean(&options.get_str("renderGutterMenu"), defaults.render_gutter_menu),
        compact_mode: boolean(&options.get_str("compactMode"), defaults.compact_mode),
        hide_original_line_numbers: boolean(&options.get_str("hideOriginalLineNumbers"), defaults.hide_original_line_numbers),
    }
}
