//! Porte de `EditorOptions` (`editorOptions.ts`, linhas 6030 a 6989) e de `register` e
//! `editorOptionsRegistry` (linhas 5838 a 5845).
//!
//! O original guarda cada opção em `editorOptionsRegistry[option.id]`; aqui [`editor_options_registry`]
//! devolve a tabela já montada, indexada por `EditorOption as usize`, guardada num `thread_local!`
//! (as opções carregam `Rc` e `Dynamic`, que não são `Sync`).
//!
//! Este arquivo também declara a chave tipada ([`Opt`]) de cada opção simples que não tem classe própria
//! em `config/editor_options/`. As opções-classe têm a sua chave no arquivo da classe, e `glyphMargin`,
//! `folding`, `scrollBeyondLastLine`, `wordWrapIndicator`, `wordWrap`, `wordWrapColumn`,
//! `showFoldingControls`, `lineNumbersMinChars`, `wordWrapOverride1` e `wordWrapOverride2` têm a sua em
//! `simple_keys.rs`. `wordWrap`, `wordWrapOverride1`, `wordWrapOverride2` e `showFoldingControls` guardam o
//! `enum` de `simple_keys.rs` (por isso são `EditorEnumOption`, e não `EditorStringEnumOption`).
//!
//! Divergências:
//!
//! - `nls.localize(chave, texto)` vira o texto em inglês, como nos demais arquivos de opção (a chave de
//!   tradução e os comentários para tradutores não são guardados).
//! - `platform.isChrome` e `platform.isEdge` (que o original lê do `navigator.userAgent`) saem do
//!   `user_agent` da [`Platform`], com as mesmas buscas de `browser.ts` (`Chrome` e `Edg/`).
//! - `IMarkdownString` (`readOnlyMessage`) é um objeto [`Dynamic`], sem validar os campos, como o original.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::platform::Platform;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;

use super::accessibility_support::EditorAccessibilitySupport;
use super::bracket_pair_colorization::BracketPairColorization;
use super::comments::EditorComments;
use super::cursor_style::{
    cursor_blinking_style_from_string, cursor_style_from_string, TextEditorCursorBlinkingStyle, TextEditorCursorStyle,
};
use super::drop_into_editor::EditorDropIntoEditor;
use super::editor_class_name::editor_class_name;
use super::editor_font_info::editor_font_info;
use super::effective_options::{
    effective_allow_variable_fonts, effective_cursor_style, effective_edit_context_enabled,
    effective_fullwidth_character_width,
};
use super::empty_selection_clipboard::EditorEmptySelectionClipboard;
use super::find::EditorFind;
use super::font_ligatures::EditorFontLigatures;
use super::font_size::EditorFontSize;
use super::font_variations::EditorFontVariations;
use super::font_weight::EditorFontWeight;
use super::goto_location::EditorGoToLocation;
use super::guides::GuideOptions;
use super::hover::EditorHover;
use super::inlay_hints::EditorInlayHints;
use super::inline_suggest::InlineEditorSuggest;
use super::layout_info_computer::EditorLayoutInfoComputer;
use super::lightbulb::EditorLightbulb;
use super::line_decorations_width::EditorLineDecorationsWidth;
use super::line_height::EditorLineHeight;
use super::line_numbers::EditorRenderLineNumbersOption;
use super::minimap::EditorMinimap;
use super::padding::EditorPadding;
use super::parameter_hints::EditorParameterHints;
use super::paste_as::EditorPasteAs;
use super::pixel_ratio::editor_pixel_ratio;
use super::placeholder::PlaceholderOption;
use super::quick_suggestions::EditorQuickSuggestions;
use super::rulers::EditorRulers;
use super::scrollbar::EditorScrollbar;
use super::simple_keys::{ShowFoldingControls, WordWrap, WordWrapOverride};
use super::smart_select::SmartSelect;
use super::sticky_scroll::EditorStickyScroll;
use super::suggest::EditorSuggest;
use super::unicode_highlight::UnicodeHighlight;
use super::word_segmenter_locales::WordSegmenterLocales;
use super::wrapping_indent::WrappingIndentOption;
use super::wrapping_info::editor_wrapping_info_computer;
use super::wrapping_strategy::WrappingStrategy;
use super::{
    auto_indent_from_string, clamp_float, EditorAutoIndentStrategy, EditorBooleanOption, EditorEnumOption,
    EditorFloatOption, EditorIntOption, EditorOption, EditorOptionImpl, EditorStringEnumOption, EditorStringOption,
    ErasedEditorOption, Opt,
};
use crate::config::font_info::editor_font_defaults;
use crate::word_helper::USUAL_WORD_SEPARATORS;

/// `Constants.MAX_SAFE_SMALL_INTEGER` como `number`.
const MAX_SMI: f64 = MAX_SAFE_SMALL_INTEGER as f64;

// ---------------------------------------------------------------------------------------------
// Chaves tipadas das opções simples

macro_rules! typed_keys {
    ($($name:ident: $value:ty => $variant:ident),+ $(,)?) => {
        $(
            #[doc = concat!("Chave tipada da opção `", stringify!($variant), "`.")]
            pub const $name: Opt<$value> = Opt::new(EditorOption::$variant);
        )+
    };
}

typed_keys! {
    ACCEPT_SUGGESTION_ON_COMMIT_CHARACTER: bool => AcceptSuggestionOnCommitCharacter,
    ACCEPT_SUGGESTION_ON_ENTER: String => AcceptSuggestionOnEnter,
    ACCESSIBILITY_PAGE_SIZE: f64 => AccessibilityPageSize,
    ALLOW_OVERFLOW: bool => AllowOverflow,
    ALLOW_VARIABLE_LINE_HEIGHTS: bool => AllowVariableLineHeights,
    ALLOW_VARIABLE_FONTS: bool => AllowVariableFonts,
    ALLOW_VARIABLE_FONTS_IN_ACCESSIBILITY_MODE: bool => AllowVariableFontsInAccessibilityMode,
    ARIA_LABEL: String => AriaLabel,
    ARIA_REQUIRED: bool => AriaRequired,
    AUTO_CLOSING_BRACKETS: String => AutoClosingBrackets,
    AUTO_CLOSING_COMMENTS: String => AutoClosingComments,
    SCREEN_READER_ANNOUNCE_INLINE_SUGGESTION: bool => ScreenReaderAnnounceInlineSuggestion,
    AUTO_CLOSING_DELETE: String => AutoClosingDelete,
    AUTO_CLOSING_OVERTYPE: String => AutoClosingOvertype,
    AUTO_CLOSING_QUOTES: String => AutoClosingQuotes,
    AUTO_INDENT: EditorAutoIndentStrategy => AutoIndent,
    AUTO_INDENT_ON_PASTE: bool => AutoIndentOnPaste,
    AUTO_INDENT_ON_PASTE_WITHIN_STRING: bool => AutoIndentOnPasteWithinString,
    AUTOMATIC_LAYOUT: bool => AutomaticLayout,
    AUTO_SURROUND: String => AutoSurround,
    STICKY_TAB_STOPS: bool => StickyTabStops,
    CODE_LENS: bool => CodeLens,
    CODE_LENS_FONT_FAMILY: String => CodeLensFontFamily,
    CODE_LENS_FONT_SIZE: f64 => CodeLensFontSize,
    COLOR_DECORATORS: bool => ColorDecorators,
    COLOR_DECORATORS_ACTIVATED_ON: String => ColorDecoratorsActivatedOn,
    COLOR_DECORATORS_LIMIT: f64 => ColorDecoratorsLimit,
    COLUMN_SELECTION: bool => ColumnSelection,
    CONTEXTMENU: bool => Contextmenu,
    COPY_WITH_SYNTAX_HIGHLIGHTING: bool => CopyWithSyntaxHighlighting,
    CURSOR_BLINKING: TextEditorCursorBlinkingStyle => CursorBlinking,
    CURSOR_SMOOTH_CARET_ANIMATION: String => CursorSmoothCaretAnimation,
    CURSOR_STYLE: TextEditorCursorStyle => CursorStyle,
    OVERTYPE_CURSOR_STYLE: TextEditorCursorStyle => OvertypeCursorStyle,
    CURSOR_SURROUNDING_LINES: f64 => CursorSurroundingLines,
    CURSOR_SURROUNDING_LINES_STYLE: String => CursorSurroundingLinesStyle,
    CURSOR_WIDTH: f64 => CursorWidth,
    CURSOR_HEIGHT: f64 => CursorHeight,
    DISABLE_LAYER_HINTING: bool => DisableLayerHinting,
    DISABLE_MONOSPACE_OPTIMIZATIONS: bool => DisableMonospaceOptimizations,
    DOM_READ_ONLY: bool => DomReadOnly,
    DOUBLE_CLICK_SELECTS_BLOCK: bool => DoubleClickSelectsBlock,
    DRAG_AND_DROP: bool => DragAndDrop,
    EDIT_CONTEXT: bool => EditContext,
    RENDER_RICH_SCREEN_READER_CONTENT: bool => RenderRichScreenReaderContent,
    EXPERIMENTAL_GPU_ACCELERATION: String => ExperimentalGpuAcceleration,
    EXPERIMENTAL_WHITESPACE_RENDERING: String => ExperimentalWhitespaceRendering,
    EXTRA_EDITOR_CLASS_NAME: String => ExtraEditorClassName,
    FAST_SCROLL_SENSITIVITY: f64 => FastScrollSensitivity,
    FIXED_OVERFLOW_WIDGETS: bool => FixedOverflowWidgets,
    FOLDING_STRATEGY: String => FoldingStrategy,
    FOLDING_HIGHLIGHT: bool => FoldingHighlight,
    FOLDING_IMPORTS_BY_DEFAULT: bool => FoldingImportsByDefault,
    FOLDING_MAXIMUM_REGIONS: f64 => FoldingMaximumRegions,
    UNFOLD_ON_CLICK_AFTER_END_OF_LINE: bool => UnfoldOnClickAfterEndOfLine,
    FONT_FAMILY: String => FontFamily,
    FORMAT_ON_PASTE: bool => FormatOnPaste,
    FORMAT_ON_TYPE: bool => FormatOnType,
    FULLWIDTH_CHARACTER_WIDTH: String => FullwidthCharacterWidth,
    HIDE_CURSOR_IN_OVERVIEW_RULER: bool => HideCursorInOverviewRuler,
    IN_DIFF_EDITOR: bool => InDiffEditor,
    INERTIAL_SCROLL: bool => InertialScroll,
    LETTER_SPACING: f64 => LetterSpacing,
    LINKED_EDITING: bool => LinkedEditing,
    LINKS: bool => Links,
    MATCH_BRACKETS: String => MatchBrackets,
    MOUSE_STYLE: String => MouseStyle,
    MOUSE_WHEEL_SCROLL_SENSITIVITY: f64 => MouseWheelScrollSensitivity,
    MOUSE_WHEEL_ZOOM: bool => MouseWheelZoom,
    MULTI_CURSOR_MERGE_OVERLAPPING: bool => MultiCursorMergeOverlapping,
    MULTI_CURSOR_MODIFIER: String => MultiCursorModifier,
    MOUSE_MIDDLE_CLICK_ACTION: String => MouseMiddleClickAction,
    MULTI_CURSOR_PASTE: String => MultiCursorPaste,
    MULTI_CURSOR_LIMIT: f64 => MultiCursorLimit,
    OCCURRENCES_HIGHLIGHT: String => OccurrencesHighlight,
    OCCURRENCES_HIGHLIGHT_DELAY: f64 => OccurrencesHighlightDelay,
    OCCURRENCES_HIGHLIGHT_FROM_ALL_PROVIDERS: bool => OccurrencesHighlightFromAllProviders,
    OVERTYPE_ON_PASTE: bool => OvertypeOnPaste,
    OVERVIEW_RULER_BORDER: bool => OverviewRulerBorder,
    OVERVIEW_RULER_LANES: f64 => OverviewRulerLanes,
    PEEK_WIDGET_DEFAULT_FOCUS: String => PeekWidgetDefaultFocus,
    DEFINITION_LINK_OPENS_IN_PEEK: bool => DefinitionLinkOpensInPeek,
    QUICK_SUGGESTIONS_DELAY: f64 => QuickSuggestionsDelay,
    READ_ONLY: bool => ReadOnly,
    READ_ONLY_MESSAGE: Option<Dynamic> => ReadOnlyMessage,
    RENAME_ON_TYPE: bool => RenameOnType,
    RENDER_CONTROL_CHARACTERS: bool => RenderControlCharacters,
    RENDER_FINAL_NEWLINE: String => RenderFinalNewline,
    RENDER_LINE_HIGHLIGHT: String => RenderLineHighlight,
    RENDER_LINE_HIGHLIGHT_ONLY_WHEN_FOCUS: bool => RenderLineHighlightOnlyWhenFocus,
    RENDER_VALIDATION_DECORATIONS: String => RenderValidationDecorations,
    RENDER_WHITESPACE: String => RenderWhitespace,
    REVEAL_HORIZONTAL_RIGHT_PADDING: f64 => RevealHorizontalRightPadding,
    ROUNDED_SELECTION: bool => RoundedSelection,
    SCROLL_BEYOND_LAST_COLUMN: f64 => ScrollBeyondLastColumn,
    SCROLL_ON_MIDDLE_CLICK: bool => ScrollOnMiddleClick,
    SCROLL_PREDOMINANT_AXIS: bool => ScrollPredominantAxis,
    SELECTION_CLIPBOARD: bool => SelectionClipboard,
    SELECTION_HIGHLIGHT: bool => SelectionHighlight,
    SELECTION_HIGHLIGHT_MAX_LENGTH: f64 => SelectionHighlightMaxLength,
    SELECTION_HIGHLIGHT_MULTILINE: bool => SelectionHighlightMultiline,
    SELECTED_TEXT_MATCH_MODE: String => SelectedTextMatchMode,
    SELECT_ON_LINE_NUMBERS: bool => SelectOnLineNumbers,
    SHOW_UNUSED: bool => ShowUnused,
    SHOW_DEPRECATED: bool => ShowDeprecated,
    SNIPPET_SUGGESTIONS: String => SnippetSuggestions,
    SMOOTH_SCROLLING: bool => SmoothScrolling,
    STOP_RENDERING_LINE_AFTER: f64 => StopRenderingLineAfter,
    INLINE_COMPLETIONS_ACCESSIBILITY_VERBOSE: bool => InlineCompletionsAccessibilityVerbose,
    SUGGEST_FONT_SIZE: f64 => SuggestFontSize,
    SUGGEST_LINE_HEIGHT: f64 => SuggestLineHeight,
    SUGGEST_ON_TRIGGER_CHARACTERS: bool => SuggestOnTriggerCharacters,
    SUGGEST_SELECTION: String => SuggestSelection,
    TAB_COMPLETION: String => TabCompletion,
    TAB_INDEX: f64 => TabIndex,
    TRIM_WHITESPACE_ON_DELETE: bool => TrimWhitespaceOnDelete,
    UNUSUAL_LINE_TERMINATORS: String => UnusualLineTerminators,
    USE_SHADOW_DOM: bool => UseShadowDom,
    USE_TAB_STOPS: bool => UseTabStops,
    WORD_BREAK: String => WordBreak,
    WORD_SEPARATORS: String => WordSeparators,
    WORD_WRAP_BREAK_AFTER_CHARACTERS: String => WordWrapBreakAfterCharacters,
    WORD_WRAP_BREAK_BEFORE_CHARACTERS: String => WordWrapBreakBeforeCharacters,
    WRAP_ON_ESCAPED_LINE_FEEDS: bool => WrapOnEscapedLineFeeds,
    DEFAULT_COLOR_DECORATORS: String => DefaultColorDecorators,
    TAB_FOCUS_MODE: bool => TabFocusMode,
}

// ---------------------------------------------------------------------------------------------
// Peças do esquema

type Entry = (&'static str, Dynamic);

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn schema<const N: usize>(entries: [Entry; N]) -> Option<Dynamic> {
    Some(Dynamic::object(entries))
}

fn description(value: &str) -> Entry {
    ("description", text(value))
}

fn markdown_description(value: &str) -> Entry {
    ("markdownDescription", text(value))
}

fn enum_descriptions(values: &[&str]) -> Entry {
    ("enumDescriptions", super::strings_array(values))
}

fn markdown_enum_descriptions(values: &[&str]) -> Entry {
    ("markdownEnumDescriptions", super::strings_array(values))
}

fn tags(values: &[&str]) -> Entry {
    ("tags", super::strings_array(values))
}

fn included(value: bool) -> Entry {
    ("included", Dynamic::Bool(value))
}

/// `platform.isChrome || platform.isEdge`, com as buscas no `userAgent` de `browser.ts`.
fn is_chrome_or_edge(platform: &Platform) -> bool {
    let user_agent = platform.user_agent.as_deref().unwrap_or("");
    user_agent.contains("Chrome") || user_agent.contains("Edg/")
}

/// `_multiCursorModifierFromString`.
fn multi_cursor_modifier_from_string(multi_cursor_modifier: &str) -> String {
    if multi_cursor_modifier == "ctrlCmd" {
        return if Platform::detect().is_macintosh { "metaKey" } else { "ctrlKey" }.to_string();
    }
    "altKey".to_string()
}

/// `ReadonlyMessage`: `IMarkdownString | undefined`, sem esquema.
struct ReadonlyMessage;

impl EditorOptionImpl for ReadonlyMessage {
    type Value = Option<Dynamic>;

    fn id(&self) -> EditorOption {
        EditorOption::ReadOnlyMessage
    }

    fn name(&self) -> &str {
        "readOnlyMessage"
    }

    fn default_value(&self) -> Option<Dynamic> {
        None
    }

    fn validate(&self, input: &Dynamic) -> Option<Dynamic> {
        if !input.is_truthy() || input.type_of() != "object" {
            return EditorOptionImpl::default_value(self);
        }
        Some(input.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// A tabela

/// `editorOptionsRegistry` em construção: `register` grava a opção na posição do seu id.
struct RegistryBuilder {
    slots: Vec<Option<Rc<dyn ErasedEditorOption>>>,
}

impl RegistryBuilder {
    fn new() -> Self {
        Self { slots: vec![None; EditorOption::COUNT] }
    }

    /// `register`.
    fn register<O: ErasedEditorOption + 'static>(&mut self, option: O) {
        let id = ErasedEditorOption::id(&option) as usize;
        self.slots[id] = Some(Rc::new(option));
    }

    /// A tabela completa; entra em pânico se algum id ficou sem opção (erro de programação).
    fn finish(self) -> Rc<[Rc<dyn ErasedEditorOption>]> {
        self.slots
            .into_iter()
            .enumerate()
            .map(|(id, slot)| slot.unwrap_or_else(|| panic!("opção sem registro: {}", EditorOption::ALL[id].name())))
            .collect()
    }
}

thread_local! {
    static EDITOR_OPTIONS_REGISTRY: Rc<[Rc<dyn ErasedEditorOption>]> = build_registry();
}

/// `editorOptionsRegistry`: uma opção por `EditorOption`, indexada por `EditorOption as usize`.
pub fn editor_options_registry() -> Rc<[Rc<dyn ErasedEditorOption>]> {
    EDITOR_OPTIONS_REGISTRY.with(Rc::clone)
}

/// `EditorOptions`, na ordem do original.
fn build_registry() -> Rc<[Rc<dyn ErasedEditorOption>]> {
    let platform = Platform::detect();
    let mut r = RegistryBuilder::new();

    r.register(EditorBooleanOption::new(
        EditorOption::AcceptSuggestionOnCommitCharacter,
        "acceptSuggestionOnCommitCharacter",
        true,
        schema([markdown_description("Controls whether suggestions should be accepted on commit characters. For example, in JavaScript, the semi-colon (`;`) can be a commit character that accepts a suggestion and types that character.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AcceptSuggestionOnEnter,
        "acceptSuggestionOnEnter",
        "on",
        &["on", "smart", "off"],
        schema([
            markdown_enum_descriptions(&["", "Only accept a suggestion with `Enter` when it makes a textual change.", ""]),
            markdown_description("Controls whether suggestions should be accepted on `Enter`, in addition to `Tab`. Helps to avoid ambiguity between inserting new lines or accepting suggestions."),
        ]),
    ));
    r.register(EditorAccessibilitySupport::new());
    r.register(EditorIntOption::new(
        EditorOption::AccessibilityPageSize,
        "accessibilityPageSize",
        500.0,
        1.0,
        MAX_SMI,
        schema([
            description("Controls the number of lines in the editor that can be read out by a screen reader at once. When we detect a screen reader we automatically set the default to be 500. Warning: this has a performance implication for numbers larger than the default."),
            tags(&["accessibility"]),
        ]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::AllowOverflow, "allowOverflow", true, None));
    r.register(EditorBooleanOption::new(
        EditorOption::AllowVariableLineHeights,
        "allowVariableLineHeights",
        true,
        schema([description("Controls whether to allow using variable line heights in the editor.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::AllowVariableFonts,
        "allowVariableFonts",
        true,
        schema([description("Controls whether to allow using variable fonts in the editor.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::AllowVariableFontsInAccessibilityMode,
        "allowVariableFontsInAccessibilityMode",
        false,
        schema([
            description("Controls whether to allow using variable fonts in the editor in the accessibility mode."),
            tags(&["accessibility"]),
        ]),
    ));
    r.register(EditorStringOption::new(EditorOption::AriaLabel, "ariaLabel", "Editor content", None));
    r.register(EditorBooleanOption::new(EditorOption::AriaRequired, "ariaRequired", false, None));
    r.register(EditorBooleanOption::new(
        EditorOption::ScreenReaderAnnounceInlineSuggestion,
        "screenReaderAnnounceInlineSuggestion",
        true,
        schema([
            description("Control whether inline suggestions are announced by a screen reader."),
            tags(&["accessibility"]),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoClosingBrackets,
        "autoClosingBrackets",
        "languageDefined",
        &["always", "languageDefined", "beforeWhitespace", "never"],
        schema([
            enum_descriptions(&[
                "",
                "Use language configurations to determine when to autoclose brackets.",
                "Autoclose brackets only when the cursor is to the left of whitespace.",
                "",
            ]),
            description("Controls whether the editor should automatically close brackets after the user adds an opening bracket."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoClosingComments,
        "autoClosingComments",
        "languageDefined",
        &["always", "languageDefined", "beforeWhitespace", "never"],
        schema([
            enum_descriptions(&[
                "",
                "Use language configurations to determine when to autoclose comments.",
                "Autoclose comments only when the cursor is to the left of whitespace.",
                "",
            ]),
            description("Controls whether the editor should automatically close comments after the user adds an opening comment."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoClosingDelete,
        "autoClosingDelete",
        "auto",
        &["always", "auto", "never"],
        schema([
            enum_descriptions(&["", "Remove adjacent closing quotes or brackets only if they were automatically inserted.", ""]),
            description("Controls whether the editor should remove adjacent closing quotes or brackets when deleting."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoClosingOvertype,
        "autoClosingOvertype",
        "auto",
        &["always", "auto", "never"],
        schema([
            enum_descriptions(&["", "Type over closing quotes or brackets only if they were automatically inserted.", ""]),
            description("Controls whether the editor should type over closing quotes or brackets."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoClosingQuotes,
        "autoClosingQuotes",
        "languageDefined",
        &["always", "languageDefined", "beforeWhitespace", "never"],
        schema([
            enum_descriptions(&[
                "",
                "Use language configurations to determine when to autoclose quotes.",
                "Autoclose quotes only when the cursor is to the left of whitespace.",
                "",
            ]),
            description("Controls whether the editor should automatically close quotes after the user adds an opening quote."),
        ]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::AutoIndent,
        "autoIndent",
        EditorAutoIndentStrategy::Full,
        "full",
        &["none", "keep", "brackets", "advanced", "full"],
        |value| auto_indent_from_string(value).unwrap_or(EditorAutoIndentStrategy::Full),
        schema([
            enum_descriptions(&[
                "The editor will not insert indentation automatically.",
                "The editor will keep the current line's indentation.",
                "The editor will keep the current line's indentation and honor language defined brackets.",
                "The editor will keep the current line's indentation, honor language defined brackets and invoke special onEnterRules defined by languages.",
                "The editor will keep the current line's indentation, honor language defined brackets, invoke special onEnterRules defined by languages, and honor indentationRules defined by languages.",
            ]),
            description("Controls whether the editor should automatically adjust the indentation when users type, paste, move or indent lines."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::AutoIndentOnPaste,
        "autoIndentOnPaste",
        false,
        schema([description("Controls whether the editor should automatically auto-indent the pasted content.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::AutoIndentOnPasteWithinString,
        "autoIndentOnPasteWithinString",
        true,
        schema([description("Controls whether the editor should automatically auto-indent the pasted content when pasted within a string. This takes effect when autoIndentOnPaste is true.")]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::AutomaticLayout, "automaticLayout", false, None));
    r.register(EditorStringEnumOption::new(
        EditorOption::AutoSurround,
        "autoSurround",
        "languageDefined",
        &["languageDefined", "quotes", "brackets", "never"],
        schema([
            enum_descriptions(&[
                "Use language configurations to determine when to automatically surround selections.",
                "Surround with quotes but not brackets.",
                "Surround with brackets but not quotes.",
                "",
            ]),
            description("Controls whether the editor should automatically surround selections when typing quotes or brackets."),
        ]),
    ));
    r.register(BracketPairColorization::new());
    r.register(GuideOptions::new());
    r.register(EditorBooleanOption::new(
        EditorOption::StickyTabStops,
        "stickyTabStops",
        false,
        schema([description("Emulate selection behavior of tab characters when using spaces for indentation. Selection will stick to tab stops.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::CodeLens,
        "codeLens",
        true,
        schema([description("Controls whether the editor shows CodeLens.")]),
    ));
    r.register(EditorStringOption::new(
        EditorOption::CodeLensFontFamily,
        "codeLensFontFamily",
        "",
        schema([description("Controls the font family for CodeLens.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::CodeLensFontSize,
        "codeLensFontSize",
        0.0,
        0.0,
        100.0,
        schema([
            ("type", text("number")),
            ("default", Dynamic::Number(0.0)),
            ("minimum", Dynamic::Number(0.0)),
            ("maximum", Dynamic::Number(100.0)),
            markdown_description("Controls the font size in pixels for CodeLens. When set to 0, 90% of `#editor.fontSize#` is used."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ColorDecorators,
        "colorDecorators",
        true,
        schema([description("Controls whether the editor should render the inline color decorators and color picker.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::ColorDecoratorsActivatedOn,
        "colorDecoratorsActivatedOn",
        "clickAndHover",
        &["clickAndHover", "hover", "click"],
        schema([
            enum_descriptions(&[
                "Make the color picker appear both on click and hover of the color decorator",
                "Make the color picker appear on hover of the color decorator",
                "Make the color picker appear on click of the color decorator",
            ]),
            description("Controls the condition to make a color picker appear from a color decorator."),
        ]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::ColorDecoratorsLimit,
        "colorDecoratorsLimit",
        500.0,
        1.0,
        1_000_000.0,
        schema([markdown_description("Controls the max number of color decorators that can be rendered in an editor at once.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ColumnSelection,
        "columnSelection",
        false,
        schema([description("Enable that the selection with the mouse and keys is doing column selection.")]),
    ));
    r.register(EditorComments::new());
    r.register(EditorBooleanOption::new(EditorOption::Contextmenu, "contextmenu", true, None));
    r.register(EditorBooleanOption::new(
        EditorOption::CopyWithSyntaxHighlighting,
        "copyWithSyntaxHighlighting",
        true,
        schema([description("Controls whether syntax highlighting should be copied into the clipboard.")]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::CursorBlinking,
        "cursorBlinking",
        TextEditorCursorBlinkingStyle::Blink,
        "blink",
        &["blink", "smooth", "phase", "expand", "solid"],
        |value| cursor_blinking_style_from_string(value).unwrap_or(TextEditorCursorBlinkingStyle::Blink),
        schema([description("Control the cursor animation style.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::CursorSmoothCaretAnimation,
        "cursorSmoothCaretAnimation",
        "off",
        &["off", "explicit", "on"],
        schema([
            enum_descriptions(&[
                "Smooth caret animation is disabled.",
                "Smooth caret animation is enabled only when the user moves the cursor with an explicit gesture.",
                "Smooth caret animation is always enabled.",
            ]),
            description("Controls whether the smooth caret animation should be enabled."),
        ]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::CursorStyle,
        "cursorStyle",
        TextEditorCursorStyle::Line,
        "line",
        &["line", "block", "underline", "line-thin", "block-outline", "underline-thin"],
        |value| cursor_style_from_string(value).unwrap_or(TextEditorCursorStyle::Line),
        schema([description("Controls the cursor style in insert input mode.")]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::OvertypeCursorStyle,
        "overtypeCursorStyle",
        TextEditorCursorStyle::Block,
        "block",
        &["line", "block", "underline", "line-thin", "block-outline", "underline-thin"],
        |value| cursor_style_from_string(value).unwrap_or(TextEditorCursorStyle::Block),
        schema([description("Controls the cursor style in overtype input mode.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::CursorSurroundingLines,
        "cursorSurroundingLines",
        0.0,
        0.0,
        MAX_SMI,
        schema([description("Controls the minimal number of visible leading lines (minimum 0) and trailing lines (minimum 1) surrounding the cursor. Known as 'scrollOff' or 'scrollOffset' in some other editors.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::CursorSurroundingLinesStyle,
        "cursorSurroundingLinesStyle",
        "default",
        &["default", "all"],
        schema([
            enum_descriptions(&[
                "`cursorSurroundingLines` is enforced only when triggered via the keyboard or API.",
                "`cursorSurroundingLines` is enforced always.",
            ]),
            markdown_description("Controls when `#editor.cursorSurroundingLines#` should be enforced."),
        ]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::CursorWidth,
        "cursorWidth",
        0.0,
        0.0,
        MAX_SMI,
        schema([markdown_description("Controls the width of the cursor when `#editor.cursorStyle#` is set to `line`.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::CursorHeight,
        "cursorHeight",
        0.0,
        0.0,
        MAX_SMI,
        schema([markdown_description("Controls the height of the cursor when `#editor.cursorStyle#` is set to `line`. Cursor's max height depends on line height.")]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::DisableLayerHinting, "disableLayerHinting", false, None));
    r.register(EditorBooleanOption::new(
        EditorOption::DisableMonospaceOptimizations,
        "disableMonospaceOptimizations",
        false,
        None,
    ));
    r.register(EditorBooleanOption::new(EditorOption::DomReadOnly, "domReadOnly", false, None));
    r.register(EditorBooleanOption::new(
        EditorOption::DoubleClickSelectsBlock,
        "doubleClickSelectsBlock",
        true,
        schema([description("Controls whether double-clicking next to a bracket or quote selects the content inside.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::DragAndDrop,
        "dragAndDrop",
        true,
        schema([description("Controls whether the editor should allow moving selections via drag and drop.")]),
    ));
    r.register(EditorEmptySelectionClipboard::new());
    r.register(EditorDropIntoEditor::new());
    r.register(EditorBooleanOption::new(
        EditorOption::EditContext,
        "editContext",
        true,
        schema([
            description("Sets whether the EditContext API should be used instead of the text area to power input in the editor."),
            included(is_chrome_or_edge(&platform) || platform.is_native),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::RenderRichScreenReaderContent,
        "renderRichScreenReaderContent",
        false,
        schema([markdown_description("Whether to render rich screen reader content when the `#editor.editContext#` setting is enabled.")]),
    ));
    r.register(EditorStickyScroll::new());
    r.register(EditorStringEnumOption::new(
        EditorOption::ExperimentalGpuAcceleration,
        "experimentalGpuAcceleration",
        "off",
        &["off", "on"],
        schema([
            tags(&["experimental"]),
            enum_descriptions(&["Use regular DOM-based rendering.", "Use GPU acceleration."]),
            description("Controls whether to use the experimental GPU acceleration to render the editor."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::ExperimentalWhitespaceRendering,
        "experimentalWhitespaceRendering",
        "svg",
        &["svg", "font", "off"],
        schema([
            enum_descriptions(&[
                "Use a new rendering method with svgs.",
                "Use a new rendering method with font characters.",
                "Use the stable rendering method.",
            ]),
            description("Controls whether whitespace is rendered with a new, experimental method."),
        ]),
    ));
    r.register(EditorStringOption::new(EditorOption::ExtraEditorClassName, "extraEditorClassName", "", None));
    r.register(EditorFloatOption::new(
        EditorOption::FastScrollSensitivity,
        "fastScrollSensitivity",
        5.0,
        |x| if x <= 0.0 { 5.0 } else { x },
        schema([markdown_description("Scrolling speed multiplier when pressing `Alt`.")]),
        None,
        None,
    ));
    r.register(EditorFind::new());
    r.register(EditorBooleanOption::new(EditorOption::FixedOverflowWidgets, "fixedOverflowWidgets", false, None));
    r.register(EditorBooleanOption::new(
        EditorOption::Folding,
        "folding",
        true,
        schema([description("Controls whether the editor has code folding enabled.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::FoldingStrategy,
        "foldingStrategy",
        "auto",
        &["auto", "indentation"],
        schema([
            enum_descriptions(&[
                "Use a language-specific folding strategy if available, else the indentation-based one.",
                "Use the indentation-based folding strategy.",
            ]),
            description("Controls the strategy for computing folding ranges."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::FoldingHighlight,
        "foldingHighlight",
        true,
        schema([description("Controls whether the editor should highlight folded ranges.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::FoldingImportsByDefault,
        "foldingImportsByDefault",
        false,
        schema([description("Controls whether the editor automatically collapses import ranges.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::FoldingMaximumRegions,
        "foldingMaximumRegions",
        5000.0,
        10.0,
        65000.0, // o limite precisa ser menor que MAX_FOLDING_REGIONS de foldingRanges
        schema([description("The maximum number of foldable regions. Increasing this value may result in the editor becoming less responsive when the current source has a large number of foldable regions.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::UnfoldOnClickAfterEndOfLine,
        "unfoldOnClickAfterEndOfLine",
        false,
        schema([description("Controls whether clicking on the empty content after a folded line will unfold the line.")]),
    ));
    r.register(EditorStringOption::new(
        EditorOption::FontFamily,
        "fontFamily",
        editor_font_defaults().font_family,
        schema([description("Controls the font family.")]),
    ));
    r.register(editor_font_info());
    r.register(EditorFontLigatures::new());
    r.register(EditorFontSize::new());
    r.register(EditorFontWeight::new());
    r.register(EditorFontVariations::new());
    r.register(EditorBooleanOption::new(
        EditorOption::FormatOnPaste,
        "formatOnPaste",
        false,
        schema([description("Controls whether the editor should automatically format the pasted content. A formatter must be available and the formatter should be able to format a range in a document.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::FormatOnType,
        "formatOnType",
        false,
        schema([description("Controls whether the editor should automatically format the line after typing.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::FullwidthCharacterWidth,
        "fullwidthCharacterWidth",
        "font",
        &["font", "twoCells"],
        schema([
            enum_descriptions(&[
                "Render full-width characters using the width defined by the font.",
                "Render full-width characters centered in exactly two character cells. Only applies to monospace fonts when font ligatures are disabled. Does not apply to multi codepoint full-width characters or to GPU rendering. This has a performance impact on line rendering.",
            ]),
            description("Controls the width used to render full-width characters."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::GlyphMargin,
        "glyphMargin",
        true,
        schema([description("Controls whether the editor should render the vertical glyph margin. Glyph margin is mostly used for debugging.")]),
    ));
    r.register(EditorGoToLocation::new());
    r.register(EditorBooleanOption::new(
        EditorOption::HideCursorInOverviewRuler,
        "hideCursorInOverviewRuler",
        false,
        schema([description("Controls whether the cursor should be hidden in the overview ruler.")]),
    ));
    r.register(EditorHover::new());
    r.register(EditorBooleanOption::new(EditorOption::InDiffEditor, "inDiffEditor", false, None));
    r.register(EditorBooleanOption::new(
        EditorOption::InertialScroll,
        "inertialScroll",
        false,
        schema([description("Make scrolling inertial - mostly useful with touchpad on linux.")]),
    ));
    r.register(EditorFloatOption::new(
        EditorOption::LetterSpacing,
        "letterSpacing",
        editor_font_defaults().letter_spacing,
        |x| clamp_float(x, -5.0, 20.0),
        schema([description("Controls the letter spacing in pixels.")]),
        None,
        None,
    ));
    r.register(EditorLightbulb::new());
    r.register(EditorLineDecorationsWidth::new());
    r.register(EditorLineHeight::new());
    r.register(EditorRenderLineNumbersOption::new());
    r.register(EditorIntOption::new(EditorOption::LineNumbersMinChars, "lineNumbersMinChars", 5.0, 1.0, 300.0, None));
    r.register(EditorBooleanOption::new(
        EditorOption::LinkedEditing,
        "linkedEditing",
        false,
        schema([description("Controls whether the editor has linked editing enabled. Depending on the language, related symbols such as HTML tags, are updated while editing.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::Links,
        "links",
        true,
        schema([description("Controls whether the editor should detect links and make them clickable.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::MatchBrackets,
        "matchBrackets",
        "always",
        &["always", "near", "never"],
        schema([description("Highlight matching brackets.")]),
    ));
    r.register(EditorMinimap::new());
    r.register(EditorStringEnumOption::new(
        EditorOption::MouseStyle,
        "mouseStyle",
        "text",
        &["text", "default", "copy"],
        None,
    ));
    r.register(EditorFloatOption::new(
        EditorOption::MouseWheelScrollSensitivity,
        "mouseWheelScrollSensitivity",
        1.0,
        |x| if x == 0.0 { 1.0 } else { x },
        schema([markdown_description("A multiplier to be used on the `deltaX` and `deltaY` of mouse wheel scroll events.")]),
        None,
        None,
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::MouseWheelZoom,
        "mouseWheelZoom",
        false,
        schema([markdown_description(if platform.is_macintosh {
            "Zoom the font of the editor when using mouse wheel and holding `Cmd`."
        } else {
            "Zoom the font of the editor when using mouse wheel and holding `Ctrl`."
        })]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::MultiCursorMergeOverlapping,
        "multiCursorMergeOverlapping",
        true,
        schema([description("Merge multiple cursors when they are overlapping.")]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::MultiCursorModifier,
        "multiCursorModifier",
        "altKey".to_string(),
        "alt",
        &["ctrlCmd", "alt"],
        multi_cursor_modifier_from_string,
        schema([
            markdown_enum_descriptions(&[
                "Maps to `Control` on Windows and Linux and to `Command` on macOS.",
                "Maps to `Alt` on Windows and Linux and to `Option` on macOS.",
            ]),
            markdown_description("The modifier to be used to add multiple cursors with the mouse. The Go to Definition and Open Link mouse gestures will adapt such that they do not conflict with the [multicursor modifier](https://code.visualstudio.com/docs/editor/codebasics#_multicursor-modifier)."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::MouseMiddleClickAction,
        "mouseMiddleClickAction",
        "default",
        &["default", "openLink", "ctrlLeftClick"],
        schema([description("Controls what happens when middle mouse button is clicked in the editor.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::MultiCursorPaste,
        "multiCursorPaste",
        "spread",
        &["spread", "full"],
        schema([
            markdown_enum_descriptions(&[
                "Each cursor pastes a single line of the text. When pasting column selections into a single cursor, each copied line is pasted on a successive destination line and padded with spaces to form a rectangle.",
                "Each cursor pastes the full text.",
            ]),
            markdown_description("Controls pasting when the line count of the pasted text matches the cursor count, or when pasting column selections into a single cursor."),
        ]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::MultiCursorLimit,
        "multiCursorLimit",
        10000.0,
        1.0,
        100000.0,
        schema([markdown_description("Controls the max number of cursors that can be in an active editor at once.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::OccurrencesHighlight,
        "occurrencesHighlight",
        "singleFile",
        &["off", "singleFile", "multiFile"],
        schema([
            markdown_enum_descriptions(&[
                "Does not highlight occurrences.",
                "Highlights occurrences only in the current file.",
                "Experimental: Highlights occurrences across all valid open files.",
            ]),
            markdown_description("Controls whether occurrences should be highlighted across open files."),
        ]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::OccurrencesHighlightDelay,
        "occurrencesHighlightDelay",
        0.0,
        0.0,
        2000.0,
        schema([
            description("Controls the delay in milliseconds after which occurrences are highlighted."),
            tags(&["preview"]),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::OccurrencesHighlightFromAllProviders,
        "occurrencesHighlightFromAllProviders",
        false,
        schema([description("Controls whether single-document highlights from all matching providers are merged. When enabled, basic word-based highlighting is only used if no other provider matches.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::OvertypeOnPaste,
        "overtypeOnPaste",
        true,
        schema([description("Controls whether pasting should overtype.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::OverviewRulerBorder,
        "overviewRulerBorder",
        true,
        schema([description("Controls whether a border should be drawn around the overview ruler.")]),
    ));
    r.register(EditorIntOption::new(EditorOption::OverviewRulerLanes, "overviewRulerLanes", 3.0, 0.0, 3.0, None));
    r.register(EditorPadding::new());
    r.register(EditorPasteAs::new());
    r.register(EditorParameterHints::new());
    r.register(EditorStringEnumOption::new(
        EditorOption::PeekWidgetDefaultFocus,
        "peekWidgetDefaultFocus",
        "tree",
        &["tree", "editor"],
        schema([
            enum_descriptions(&["Focus the tree when opening peek", "Focus the editor when opening peek"]),
            description("Controls whether to focus the inline editor or the tree in the peek widget."),
        ]),
    ));
    r.register(PlaceholderOption::new());
    r.register(EditorBooleanOption::new(
        EditorOption::DefinitionLinkOpensInPeek,
        "definitionLinkOpensInPeek",
        false,
        schema([description("Controls whether the Go to Definition mouse gesture always opens the peek widget.")]),
    ));
    r.register(EditorQuickSuggestions::new());
    r.register(EditorIntOption::new(
        EditorOption::QuickSuggestionsDelay,
        "quickSuggestionsDelay",
        10.0,
        0.0,
        MAX_SMI,
        schema([
            description("Controls the delay in milliseconds after which quick suggestions will show up."),
            ("experiment", Dynamic::object([("mode", text("auto"))])),
        ]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::ReadOnly, "readOnly", false, None));
    r.register(ReadonlyMessage);
    r.register(EditorBooleanOption::new(
        EditorOption::RenameOnType,
        "renameOnType",
        false,
        schema([
            description("Controls whether the editor auto renames on type."),
            ("markdownDeprecationMessage", text("Deprecated, use `#editor.linkedEditing#` instead.")),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::RenderControlCharacters,
        "renderControlCharacters",
        true,
        schema([
            description("Controls whether the editor should render control characters."),
            ("restricted", Dynamic::Bool(true)),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::RenderFinalNewline,
        "renderFinalNewline",
        if platform.is_linux { "dimmed" } else { "on" },
        &["off", "on", "dimmed"],
        schema([description("Render last line number when the file ends with a newline.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::RenderLineHighlight,
        "renderLineHighlight",
        "line",
        &["none", "gutter", "line", "all"],
        schema([
            enum_descriptions(&["", "", "", "Highlights both the gutter and the current line."]),
            description("Controls how the editor should render the current line highlight."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::RenderLineHighlightOnlyWhenFocus,
        "renderLineHighlightOnlyWhenFocus",
        false,
        schema([description("Controls if the editor should render the current line highlight only when the editor is focused.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::RenderValidationDecorations,
        "renderValidationDecorations",
        "editable",
        &["editable", "on", "off"],
        None,
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::RenderWhitespace,
        "renderWhitespace",
        "selection",
        &["none", "boundary", "selection", "trailing", "all"],
        schema([
            enum_descriptions(&[
                "",
                "Render whitespace characters except for single spaces between words.",
                "Render whitespace characters only on selected text.",
                "Render only trailing whitespace characters.",
                "",
            ]),
            description("Controls how the editor should render whitespace characters."),
        ]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::RevealHorizontalRightPadding,
        "revealHorizontalRightPadding",
        15.0,
        0.0,
        1000.0,
        None,
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::RoundedSelection,
        "roundedSelection",
        true,
        schema([description("Controls whether selections should have rounded corners.")]),
    ));
    r.register(EditorRulers::new());
    r.register(EditorScrollbar::new());
    r.register(EditorIntOption::new(
        EditorOption::ScrollBeyondLastColumn,
        "scrollBeyondLastColumn",
        4.0,
        0.0,
        MAX_SMI,
        schema([description("Controls the number of extra characters beyond which the editor will scroll horizontally.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ScrollBeyondLastLine,
        "scrollBeyondLastLine",
        true,
        schema([description("Controls whether the editor will scroll beyond the last line.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ScrollOnMiddleClick,
        "scrollOnMiddleClick",
        false,
        schema([description("Controls whether the editor will scroll when the middle button is pressed.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ScrollPredominantAxis,
        "scrollPredominantAxis",
        true,
        schema([description("Scroll only along the predominant axis when scrolling both vertically and horizontally at the same time. Prevents horizontal drift when scrolling vertically on a trackpad.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::SelectionClipboard,
        "selectionClipboard",
        true,
        schema([
            description("Controls whether the Linux primary clipboard should be supported."),
            included(platform.is_linux),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::SelectionHighlight,
        "selectionHighlight",
        true,
        schema([description("Controls whether the editor should highlight matches similar to the selection.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::SelectionHighlightMaxLength,
        "selectionHighlightMaxLength",
        200.0,
        0.0,
        MAX_SMI,
        schema([description("Controls how many characters can be in the selection before similar matches are not highlighted. Set to zero for unlimited.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::SelectionHighlightMultiline,
        "selectionHighlightMultiline",
        false,
        schema([description("Controls whether the editor should highlight selection matches that span multiple lines.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::SelectedTextMatchMode,
        "selectedTextMatchMode",
        "findOptions",
        &["findOptions", "caseSensitive", "caseInsensitive"],
        schema([
            enum_descriptions(&[
                "Use the Find widget's match case and whole word settings.",
                "Match occurrences case-sensitively and independently of the Find widget.",
                "Match occurrences case-insensitively and independently of the Find widget.",
            ]),
            markdown_description("Controls how occurrences of selected text are matched when selecting the next, previous, or all occurrences and when highlighting selection matches. `caseSensitive` and `caseInsensitive` match substrings independently of the Find widget. An empty selection always starts a whole-word, case-sensitive search."),
        ]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::SelectOnLineNumbers, "selectOnLineNumbers", true, None));
    r.register(EditorEnumOption::new(
        EditorOption::ShowFoldingControls,
        "showFoldingControls",
        ShowFoldingControls::Mouseover,
        "mouseover",
        ShowFoldingControls::ALLOWED,
        |value| ShowFoldingControls::from_text(value).unwrap_or(ShowFoldingControls::Mouseover),
        schema([
            enum_descriptions(&[
                "Always show the folding controls.",
                "Never show the folding controls and reduce the gutter size.",
                "Only show the folding controls when the mouse is over the gutter.",
            ]),
            description("Controls when the folding controls on the gutter are shown."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ShowUnused,
        "showUnused",
        true,
        schema([description("Controls fading out of unused code.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::ShowDeprecated,
        "showDeprecated",
        true,
        schema([description("Controls strikethrough deprecated variables.")]),
    ));
    r.register(EditorInlayHints::new());
    r.register(EditorStringEnumOption::new(
        EditorOption::SnippetSuggestions,
        "snippetSuggestions",
        "inline",
        &["top", "bottom", "inline", "none"],
        schema([
            enum_descriptions(&[
                "Show snippet suggestions on top of other suggestions.",
                "Show snippet suggestions below other suggestions.",
                "Show snippets suggestions with other suggestions.",
                "Do not show snippet suggestions.",
            ]),
            description("Controls whether snippets are shown with other suggestions and how they are sorted."),
        ]),
    ));
    r.register(SmartSelect::new());
    r.register(EditorBooleanOption::new(
        EditorOption::SmoothScrolling,
        "smoothScrolling",
        false,
        schema([description("Controls whether the editor will scroll using an animation.")]),
    ));
    r.register(EditorIntOption::new(EditorOption::StopRenderingLineAfter, "stopRenderingLineAfter", 10000.0, -1.0, MAX_SMI, None));
    r.register(EditorSuggest::new());
    r.register(InlineEditorSuggest::new());
    r.register(EditorBooleanOption::new(
        EditorOption::InlineCompletionsAccessibilityVerbose,
        "inlineCompletionsAccessibilityVerbose",
        false,
        schema([description("Controls whether the accessibility hint should be provided to screen reader users when an inline completion is shown.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::SuggestFontSize,
        "suggestFontSize",
        0.0,
        0.0,
        1000.0,
        schema([markdown_description("Font size for the suggest widget. When set to `0`, the value of `#editor.fontSize#` is used.")]),
    ));
    r.register(EditorIntOption::new(
        EditorOption::SuggestLineHeight,
        "suggestLineHeight",
        0.0,
        0.0,
        1000.0,
        schema([markdown_description("Line height for the suggest widget. When set to `0`, the value of `#editor.lineHeight#` is used. The minimum value is 8.")]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::SuggestOnTriggerCharacters,
        "suggestOnTriggerCharacters",
        true,
        schema([description("Controls whether suggestions should automatically show up when typing trigger characters.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::SuggestSelection,
        "suggestSelection",
        "first",
        &["first", "recentlyUsed", "recentlyUsedByPrefix"],
        schema([
            markdown_enum_descriptions(&[
                "Always select the first suggestion.",
                "Select recent suggestions unless further typing selects one, e.g. `console.| -> console.log` because `log` has been completed recently.",
                "Select suggestions based on previous prefixes that have completed those suggestions, e.g. `co -> console` and `con -> const`.",
            ]),
            description("Controls how suggestions are pre-selected when showing the suggest list."),
        ]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::TabCompletion,
        "tabCompletion",
        "off",
        &["on", "off", "onlySnippets"],
        schema([
            enum_descriptions(&[
                "Tab complete will insert the best matching suggestion when pressing tab.",
                "Disable tab completions.",
                "Tab complete snippets when their prefix match. Works best when 'quickSuggestions' aren't enabled.",
            ]),
            description("Enables tab completions."),
        ]),
    ));
    r.register(EditorIntOption::new(EditorOption::TabIndex, "tabIndex", 0.0, -1.0, MAX_SMI, None));
    r.register(EditorBooleanOption::new(
        EditorOption::TrimWhitespaceOnDelete,
        "trimWhitespaceOnDelete",
        false,
        schema([description("Controls whether the editor will also delete the next line's indentation whitespace when deleting a newline.")]),
    ));
    r.register(UnicodeHighlight::new());
    r.register(EditorStringEnumOption::new(
        EditorOption::UnusualLineTerminators,
        "unusualLineTerminators",
        "prompt",
        &["auto", "off", "prompt"],
        schema([
            enum_descriptions(&[
                "Unusual line terminators are automatically removed.",
                "Unusual line terminators are ignored.",
                "Unusual line terminators prompt to be removed.",
            ]),
            description("Remove unusual line terminators that might cause problems."),
        ]),
    ));
    r.register(EditorBooleanOption::new(EditorOption::UseShadowDom, "useShadowDOM", true, None));
    r.register(EditorBooleanOption::new(
        EditorOption::UseTabStops,
        "useTabStops",
        true,
        schema([description("Spaces and tabs are inserted and deleted in alignment with tab stops.")]),
    ));
    r.register(EditorStringEnumOption::new(
        EditorOption::WordBreak,
        "wordBreak",
        "normal",
        &["normal", "keepAll"],
        schema([
            markdown_enum_descriptions(&[
                "Use the default line break rule.",
                "Word breaks should not be used for Chinese/Japanese/Korean (CJK) text. Non-CJK text behavior is the same as for normal.",
            ]),
            description("Controls the word break rules used for Chinese/Japanese/Korean (CJK) text."),
        ]),
    ));
    r.register(WordSegmenterLocales::new());
    r.register(EditorStringOption::new(
        EditorOption::WordSeparators,
        "wordSeparators",
        USUAL_WORD_SEPARATORS,
        schema([description("Characters that will be used as word separators when doing word related navigations or operations.")]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::WordWrap,
        "wordWrap",
        WordWrap::Off,
        "off",
        WordWrap::ALLOWED,
        |value| WordWrap::from_text(value).unwrap_or(WordWrap::Off),
        schema([
            markdown_enum_descriptions(&[
                "Lines will never wrap.",
                "Lines will wrap at the viewport width.",
                "Lines will wrap at `#editor.wordWrapColumn#`.",
                "Lines will wrap at the minimum of viewport and `#editor.wordWrapColumn#`.",
            ]),
            description("Controls how lines should wrap."),
        ]),
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::WordWrapIndicator,
        "wordWrapIndicator",
        false,
        schema([markdown_description("Controls whether an indicator is rendered at the wrapping column of lines that wrap. Only has an effect when `#editor.wordWrap#` is enabled.")]),
    ));
    r.register(EditorStringOption::new(
        EditorOption::WordWrapBreakAfterCharacters,
        "wordWrapBreakAfterCharacters",
        " \t})]?|/&.,;¢°′″‰℃、。｡､￠，．：；？！％・･ゝゞヽヾーァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖㇰㇱㇲㇳㇴㇵㇶㇷㇸㇹㇺㇻㇼㇽㇾㇿ々〻ｧｨｩｪｫｬｭｮｯｰ”〉》」』】〕）］｝｣",
        None,
    ));
    r.register(EditorStringOption::new(
        EditorOption::WordWrapBreakBeforeCharacters,
        "wordWrapBreakBeforeCharacters",
        "([{‘“〈《「『【〔（［｛｢£¥＄￡￥+＋",
        None,
    ));
    r.register(EditorIntOption::new(
        EditorOption::WordWrapColumn,
        "wordWrapColumn",
        80.0,
        1.0,
        MAX_SMI,
        schema([markdown_description("Controls the wrapping column of the editor when `#editor.wordWrap#` is `wordWrapColumn` or `bounded`.")]),
    ));
    r.register(EditorEnumOption::new(
        EditorOption::WordWrapOverride1,
        "wordWrapOverride1",
        WordWrapOverride::Inherit,
        "inherit",
        WordWrapOverride::ALLOWED,
        |value| WordWrapOverride::from_text(value).unwrap_or(WordWrapOverride::Inherit),
        None,
    ));
    r.register(EditorEnumOption::new(
        EditorOption::WordWrapOverride2,
        "wordWrapOverride2",
        WordWrapOverride::Inherit,
        "inherit",
        WordWrapOverride::ALLOWED,
        |value| WordWrapOverride::from_text(value).unwrap_or(WordWrapOverride::Inherit),
        None,
    ));
    r.register(EditorBooleanOption::new(
        EditorOption::WrapOnEscapedLineFeeds,
        "wrapOnEscapedLineFeeds",
        false,
        schema([markdown_description("Controls whether literal `\\n` shall trigger a wordWrap when `#editor.wordWrap#` is enabled.\n\nFor example:\n```c\nchar* str=\"hello\\nworld\"\n```\nwill be displayed as\n```c\nchar* str=\"hello\\n\n           world\"\n```")]),
    ));

    // Ficam no fim porque têm dependências.
    r.register(effective_cursor_style());
    r.register(editor_class_name());
    r.register(EditorStringEnumOption::new(
        EditorOption::DefaultColorDecorators,
        "defaultColorDecorators",
        "auto",
        &["auto", "always", "never"],
        schema([
            enum_descriptions(&[
                "Show default color decorators only when no extension provides colors decorators.",
                "Always show default color decorators.",
                "Never show default color decorators.",
            ]),
            description("Controls whether inline color decorations should be shown using the default document color provider."),
        ]),
    ));
    r.register(editor_pixel_ratio());
    r.register(EditorBooleanOption::new(
        EditorOption::TabFocusMode,
        "tabFocusMode",
        false,
        schema([markdown_description("Controls whether the editor receives tabs or defers them to the workbench for navigation.")]),
    ));
    r.register(EditorLayoutInfoComputer::new());
    r.register(editor_wrapping_info_computer());
    r.register(WrappingIndentOption::new());
    r.register(WrappingStrategy::new());
    r.register(effective_edit_context_enabled());
    r.register(effective_allow_variable_fonts());
    r.register(effective_fullwidth_character_width());

    r.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::EDITOR_OPTION_NAMES;

    #[test]
    fn registry_has_one_option_per_id_in_enum_order() {
        let registry = editor_options_registry();
        assert_eq!(registry.len(), EditorOption::COUNT);
        for (index, option) in registry.iter().enumerate() {
            assert_eq!(option.id() as usize, index);
            // As opções só calculadas têm `_never_` como nome; as demais usam o nome do enum, menos
            // `unicodeHighlighting`, que o original registra como `unicodeHighlight` (`editorOptions.ts`, linha 4388).
            let expected = match EditorOption::ALL[index] {
                EditorOption::UnicodeHighlighting => "unicodeHighlight",
                _ => EDITOR_OPTION_NAMES[index],
            };
            assert!(
                option.name() == "_never_" || option.name() == expected,
                "nome divergente em {index}: {}",
                option.name()
            );
        }
    }

    #[test]
    fn registry_is_shared_by_the_thread() {
        assert!(Rc::ptr_eq(&editor_options_registry(), &editor_options_registry()));
    }
}
