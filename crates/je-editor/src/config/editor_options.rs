//! Porte da infraestrutura de `src/vs/editor/common/config/editorOptions.ts` (linhas 1050 a 1520 e o
//! enum `EditorOption` das linhas 5846 a 6028).
//!
//! Desenho (ver `docs/design.md`, "Opções do editor: chaves tipadas"):
//!
//! - [`Opt<T>`] é a chave tipada: carrega o id da opção e o tipo do valor calculado, e
//!   [`ComputedEditorOptions::get`] devolve `&T` por `downcast_ref` (sem `unsafe`).
//! - As classes `BaseEditorOption`, `SimpleEditorOption` e `ComputedEditorOption` do original viram o
//!   trait [`EditorOptionImpl`] (com `compute`, `applyUpdate` e `schema` já com o corpo padrão) mais as
//!   structs concretas deste arquivo. [`ComputedEditorOption`] é a struct que fixa `name = "_never_"`,
//!   `schema = None` e `validate = defaultValue`.
//! - [`ErasedEditorOption`] é a versão sem tipo associado, para a tabela de registro.
//!
//! Divergências e pendências:
//!
//! - `unknown` do TypeScript é [`Dynamic`], que carrega objetos e arrays, então as opções-objeto
//!   validam campo a campo.
//! - `IConfigurationPropertySchema` e `{ [path: string]: IConfigurationPropertySchema }` são objetos
//!   [`Dynamic`] (a convenção de `je-base/src/json_schema.rs`): os construtores escrevem neles `type`,
//!   `default`, `minimum`, `maximum` e `enum` na mesma ordem de chaves do original.
//! - Os valores de texto das opções são `String`, então um surrogate isolado de uma `string` do
//!   JavaScript vira U+FFFD ao entrar.
//! - `applyUpdate` do original faz mesclagem profunda em objetos. Aqui o padrão compara por
//!   igualdade (`PartialEq`), que é o caso de primitivos e arrays; opção cujo valor é struct com campos
//!   parciais sobrescreve [`EditorOptionImpl::apply_update`].
//! - `value in renamedValues` de `stringSet` também enxerga chaves do protótipo de `Object` no JS
//!   (`toString`...); aqui só valem as chaves da tabela.

use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};

use super::font_info::FontInfo;

// ---------------------------------------------------------------------------------------------
// enum EditorOption

macro_rules! editor_options {
    ($($variant:ident => $js:literal),* $(,)?) => {
        /// `EditorOption`: o id de cada opção, com os mesmos valores numéricos do original.
        #[repr(usize)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum EditorOption {
            $($variant),*
        }

        impl EditorOption {
            /// Quantidade de ids (o tamanho de `ComputedEditorOptions`).
            pub const COUNT: usize = [$($js),*].len();
            /// Todos os ids, na ordem do enum.
            pub const ALL: [EditorOption; EditorOption::COUNT] = [$(EditorOption::$variant),*];

            /// Nome JS (camelCase) da opção.
            pub const fn name(self) -> &'static str {
                EDITOR_OPTION_NAMES[self as usize]
            }
        }

        /// Nome JS camelCase de cada id, indexado por `EditorOption as usize`.
        pub const EDITOR_OPTION_NAMES: [&str; EditorOption::COUNT] = [$($js),*];
    };
}

editor_options! {
    AcceptSuggestionOnCommitCharacter => "acceptSuggestionOnCommitCharacter",
    AcceptSuggestionOnEnter => "acceptSuggestionOnEnter",
    AccessibilitySupport => "accessibilitySupport",
    AccessibilityPageSize => "accessibilityPageSize",
    AllowOverflow => "allowOverflow",
    AllowVariableLineHeights => "allowVariableLineHeights",
    AllowVariableFonts => "allowVariableFonts",
    AllowVariableFontsInAccessibilityMode => "allowVariableFontsInAccessibilityMode",
    AriaLabel => "ariaLabel",
    AriaRequired => "ariaRequired",
    AutoClosingBrackets => "autoClosingBrackets",
    AutoClosingComments => "autoClosingComments",
    ScreenReaderAnnounceInlineSuggestion => "screenReaderAnnounceInlineSuggestion",
    AutoClosingDelete => "autoClosingDelete",
    AutoClosingOvertype => "autoClosingOvertype",
    AutoClosingQuotes => "autoClosingQuotes",
    AutoIndent => "autoIndent",
    AutoIndentOnPaste => "autoIndentOnPaste",
    AutoIndentOnPasteWithinString => "autoIndentOnPasteWithinString",
    AutomaticLayout => "automaticLayout",
    AutoSurround => "autoSurround",
    BracketPairColorization => "bracketPairColorization",
    Guides => "guides",
    CodeLens => "codeLens",
    CodeLensFontFamily => "codeLensFontFamily",
    CodeLensFontSize => "codeLensFontSize",
    ColorDecorators => "colorDecorators",
    ColorDecoratorsLimit => "colorDecoratorsLimit",
    ColumnSelection => "columnSelection",
    Comments => "comments",
    Contextmenu => "contextmenu",
    CopyWithSyntaxHighlighting => "copyWithSyntaxHighlighting",
    CursorBlinking => "cursorBlinking",
    CursorSmoothCaretAnimation => "cursorSmoothCaretAnimation",
    CursorStyle => "cursorStyle",
    CursorSurroundingLines => "cursorSurroundingLines",
    CursorSurroundingLinesStyle => "cursorSurroundingLinesStyle",
    CursorWidth => "cursorWidth",
    CursorHeight => "cursorHeight",
    DisableLayerHinting => "disableLayerHinting",
    DisableMonospaceOptimizations => "disableMonospaceOptimizations",
    DomReadOnly => "domReadOnly",
    DragAndDrop => "dragAndDrop",
    DropIntoEditor => "dropIntoEditor",
    EditContext => "editContext",
    EmptySelectionClipboard => "emptySelectionClipboard",
    ExperimentalGpuAcceleration => "experimentalGpuAcceleration",
    ExperimentalWhitespaceRendering => "experimentalWhitespaceRendering",
    ExtraEditorClassName => "extraEditorClassName",
    FastScrollSensitivity => "fastScrollSensitivity",
    Find => "find",
    FixedOverflowWidgets => "fixedOverflowWidgets",
    Folding => "folding",
    FoldingStrategy => "foldingStrategy",
    FoldingHighlight => "foldingHighlight",
    FoldingImportsByDefault => "foldingImportsByDefault",
    FoldingMaximumRegions => "foldingMaximumRegions",
    UnfoldOnClickAfterEndOfLine => "unfoldOnClickAfterEndOfLine",
    FontFamily => "fontFamily",
    FontInfo => "fontInfo",
    FontLigatures => "fontLigatures",
    FontSize => "fontSize",
    FontWeight => "fontWeight",
    FontVariations => "fontVariations",
    FormatOnPaste => "formatOnPaste",
    FormatOnType => "formatOnType",
    GlyphMargin => "glyphMargin",
    GotoLocation => "gotoLocation",
    HideCursorInOverviewRuler => "hideCursorInOverviewRuler",
    Hover => "hover",
    InDiffEditor => "inDiffEditor",
    InlineSuggest => "inlineSuggest",
    LetterSpacing => "letterSpacing",
    Lightbulb => "lightbulb",
    LineDecorationsWidth => "lineDecorationsWidth",
    LineHeight => "lineHeight",
    LineNumbers => "lineNumbers",
    LineNumbersMinChars => "lineNumbersMinChars",
    LinkedEditing => "linkedEditing",
    Links => "links",
    MatchBrackets => "matchBrackets",
    Minimap => "minimap",
    MouseStyle => "mouseStyle",
    MouseWheelScrollSensitivity => "mouseWheelScrollSensitivity",
    MouseWheelZoom => "mouseWheelZoom",
    MultiCursorMergeOverlapping => "multiCursorMergeOverlapping",
    MultiCursorModifier => "multiCursorModifier",
    MouseMiddleClickAction => "mouseMiddleClickAction",
    MultiCursorPaste => "multiCursorPaste",
    MultiCursorLimit => "multiCursorLimit",
    OccurrencesHighlight => "occurrencesHighlight",
    OccurrencesHighlightDelay => "occurrencesHighlightDelay",
    OccurrencesHighlightFromAllProviders => "occurrencesHighlightFromAllProviders",
    OvertypeCursorStyle => "overtypeCursorStyle",
    OvertypeOnPaste => "overtypeOnPaste",
    OverviewRulerBorder => "overviewRulerBorder",
    OverviewRulerLanes => "overviewRulerLanes",
    Padding => "padding",
    PasteAs => "pasteAs",
    ParameterHints => "parameterHints",
    PeekWidgetDefaultFocus => "peekWidgetDefaultFocus",
    Placeholder => "placeholder",
    DefinitionLinkOpensInPeek => "definitionLinkOpensInPeek",
    QuickSuggestions => "quickSuggestions",
    QuickSuggestionsDelay => "quickSuggestionsDelay",
    ReadOnly => "readOnly",
    ReadOnlyMessage => "readOnlyMessage",
    RenameOnType => "renameOnType",
    RenderRichScreenReaderContent => "renderRichScreenReaderContent",
    RenderControlCharacters => "renderControlCharacters",
    RenderFinalNewline => "renderFinalNewline",
    RenderLineHighlight => "renderLineHighlight",
    RenderLineHighlightOnlyWhenFocus => "renderLineHighlightOnlyWhenFocus",
    RenderValidationDecorations => "renderValidationDecorations",
    RenderWhitespace => "renderWhitespace",
    RevealHorizontalRightPadding => "revealHorizontalRightPadding",
    RoundedSelection => "roundedSelection",
    Rulers => "rulers",
    Scrollbar => "scrollbar",
    ScrollBeyondLastColumn => "scrollBeyondLastColumn",
    ScrollBeyondLastLine => "scrollBeyondLastLine",
    ScrollPredominantAxis => "scrollPredominantAxis",
    SelectionClipboard => "selectionClipboard",
    SelectionHighlight => "selectionHighlight",
    SelectionHighlightMaxLength => "selectionHighlightMaxLength",
    SelectionHighlightMultiline => "selectionHighlightMultiline",
    SelectedTextMatchMode => "selectedTextMatchMode",
    SelectOnLineNumbers => "selectOnLineNumbers",
    ShowFoldingControls => "showFoldingControls",
    ShowUnused => "showUnused",
    SnippetSuggestions => "snippetSuggestions",
    SmartSelect => "smartSelect",
    SmoothScrolling => "smoothScrolling",
    StickyScroll => "stickyScroll",
    StickyTabStops => "stickyTabStops",
    StopRenderingLineAfter => "stopRenderingLineAfter",
    Suggest => "suggest",
    SuggestFontSize => "suggestFontSize",
    SuggestLineHeight => "suggestLineHeight",
    SuggestOnTriggerCharacters => "suggestOnTriggerCharacters",
    SuggestSelection => "suggestSelection",
    TabCompletion => "tabCompletion",
    TabIndex => "tabIndex",
    TrimWhitespaceOnDelete => "trimWhitespaceOnDelete",
    UnicodeHighlighting => "unicodeHighlighting",
    UnusualLineTerminators => "unusualLineTerminators",
    UseShadowDom => "useShadowDOM",
    UseTabStops => "useTabStops",
    WordBreak => "wordBreak",
    WordSegmenterLocales => "wordSegmenterLocales",
    WordSeparators => "wordSeparators",
    WordWrap => "wordWrap",
    WordWrapBreakAfterCharacters => "wordWrapBreakAfterCharacters",
    WordWrapBreakBeforeCharacters => "wordWrapBreakBeforeCharacters",
    WordWrapColumn => "wordWrapColumn",
    WordWrapOverride1 => "wordWrapOverride1",
    WordWrapOverride2 => "wordWrapOverride2",
    WrappingIndent => "wrappingIndent",
    WrappingStrategy => "wrappingStrategy",
    ShowDeprecated => "showDeprecated",
    InertialScroll => "inertialScroll",
    InlayHints => "inlayHints",
    WrapOnEscapedLineFeeds => "wrapOnEscapedLineFeeds",
    WordWrapIndicator => "wordWrapIndicator",
    // Ficam no fim porque têm dependências.
    EffectiveCursorStyle => "effectiveCursorStyle",
    EditorClassName => "editorClassName",
    PixelRatio => "pixelRatio",
    TabFocusMode => "tabFocusMode",
    LayoutInfo => "layoutInfo",
    WrappingInfo => "wrappingInfo",
    DefaultColorDecorators => "defaultColorDecorators",
    ColorDecoratorsActivatedOn => "colorDecoratorsActivatedOn",
    InlineCompletionsAccessibilityVerbose => "inlineCompletionsAccessibilityVerbose",
    EffectiveEditContext => "effectiveEditContext",
    ScrollOnMiddleClick => "scrollOnMiddleClick",
    EffectiveAllowVariableFonts => "effectiveAllowVariableFonts",
    DoubleClickSelectsBlock => "doubleClickSelectsBlock",
    FullwidthCharacterWidth => "fullwidthCharacterWidth",
    // Vem depois de `fullwidthCharacterWidth`, de quem é calculada.
    EffectiveFullwidthCharacterWidth => "effectiveFullwidthCharacterWidth",
}

// ---------------------------------------------------------------------------------------------
// Ambiente, memória, evento, valores calculados

/// `AccessibilitySupport` (`src/vs/platform/accessibility/common/accessibility.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilitySupport {
    /// Não se sabe se há leitor de tela (a plataforma ainda não respondeu).
    Unknown = 0,
    Disabled = 1,
    Enabled = 2,
}

/// `inputMode: 'insert' | 'overtype'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Overtype,
}

/// `ComputeOptionsMemory`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComputeOptionsMemory {
    pub stable_minimap_layout_input: Option<layout_info_computer::MinimapLayoutInput>,
    pub stable_fit_max_minimap_scale: f64,
    pub stable_fit_remaining_width: f64,
}

impl ComputeOptionsMemory {
    pub fn new() -> Self {
        Self::default()
    }
}

/// `IEnvironmentalOptions`.
#[derive(Clone, Debug)]
pub struct EnvironmentalOptions {
    pub memory: Option<Rc<RefCell<ComputeOptionsMemory>>>,
    pub outer_width: f64,
    pub outer_height: f64,
    pub font_info: FontInfo,
    pub extra_editor_class_name: String,
    pub is_dominated_by_long_lines: bool,
    pub view_line_count: f64,
    pub line_numbers_digit_count: f64,
    pub empty_selection_clipboard: bool,
    pub pixel_ratio: f64,
    pub tab_focus_mode: bool,
    pub input_mode: InputMode,
    pub accessibility_support: AccessibilitySupport,
    pub glyph_margin_decoration_lane_count: f64,
    pub edit_context_supported: bool,
}

/// `ConfigurationChangedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationChangedEvent {
    values: Vec<bool>,
}

impl ConfigurationChangedEvent {
    pub fn new(values: Vec<bool>) -> Self {
        Self { values }
    }

    pub fn has_changed(&self, id: EditorOption) -> bool {
        // No JavaScript, passar do fim do array dá `undefined`, que é falso.
        self.values.get(id as usize).copied().unwrap_or(false)
    }
}

/// Chave tipada de uma opção: o id e o tipo do valor calculado.
pub struct Opt<T> {
    id: EditorOption,
    _value: PhantomData<fn() -> T>,
}

impl<T> Opt<T> {
    pub const fn new(id: EditorOption) -> Self {
        Self { id, _value: PhantomData }
    }

    pub const fn id(&self) -> EditorOption {
        self.id
    }
}

impl<T> Clone for Opt<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Opt<T> {}

impl<T> std::fmt::Debug for Opt<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Opt({:?})", self.id)
    }
}

/// `IComputedEditorOptions`: todos os valores calculados, indexados por `EditorOption`.
#[derive(Clone, Debug)]
pub struct ComputedEditorOptions {
    values: Vec<Rc<dyn Any>>,
}

impl ComputedEditorOptions {
    /// Os valores precisam ter exatamente `EditorOption::COUNT` itens, na ordem do enum.
    pub fn new(values: Vec<Rc<dyn Any>>) -> Self {
        assert_eq!(values.len(), EditorOption::COUNT, "um valor por EditorOption");
        Self { values }
    }

    /// Valor calculado da opção. Entra em pânico se o tipo guardado difere do de `key`, o que só
    /// acontece com um erro de programação na tabela de registro.
    pub fn get<T: 'static>(&self, key: Opt<T>) -> &T {
        let any: &dyn Any = &*self.values[key.id as usize];
        any.downcast_ref::<T>()
            .unwrap_or_else(|| panic!("tipo errado para a opção {}", key.id.name()))
    }

    /// Acesso sem tipo, para o registro e para comparar duas configurações.
    pub fn get_any(&self, id: EditorOption) -> &Rc<dyn Any> {
        &self.values[id as usize]
    }

    /// `new ComputedEditorOptions()` do `EditorConfiguration`: todos os valores ainda por escrever. Ler um
    /// valor antes de [`ComputedEditorOptions::write`] (o "Cannot read uninitialized value" do original) dá
    /// pânico em [`ComputedEditorOptions::get`], porque o marcador é `()`.
    pub fn uninitialized() -> Self {
        let placeholder: Rc<dyn Any> = Rc::new(());
        Self { values: vec![placeholder; EditorOption::COUNT] }
    }

    /// `_write(id, value)`.
    pub fn write(&mut self, id: EditorOption, value: Rc<dyn Any>) {
        self.values[id as usize] = value;
    }
}

// ---------------------------------------------------------------------------------------------
// applyUpdate

/// `ApplyUpdateResult<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApplyUpdateResult<T> {
    pub new_value: T,
    pub did_change: bool,
}

/// `applyUpdate`, no caso de valor que se compara por igualdade (primitivos e arrays):
/// `new ApplyUpdateResult(update, value !== update)`. A mesclagem profunda de objetos fica com a
/// opção que tem um struct como valor (ver [`EditorOptionImpl::apply_update`]).
pub fn apply_update<T: PartialEq>(value: Option<&T>, update: T) -> ApplyUpdateResult<T> {
    let did_change = value != Some(&update);
    ApplyUpdateResult { new_value: update, did_change }
}

// ---------------------------------------------------------------------------------------------
// Traits de opção

/// `IEditorOption<K, V>`, com `V` como tipo associado.
pub trait EditorOptionImpl {
    type Value: Clone + PartialEq + 'static;

    fn id(&self) -> EditorOption;
    fn name(&self) -> &str;
    fn default_value(&self) -> Self::Value;
    fn validate(&self, input: &Dynamic) -> Self::Value;

    fn schema(&self) -> Option<&Dynamic> {
        None
    }

    fn compute(&self, _env: &EnvironmentalOptions, _options: &ComputedEditorOptions, value: &Self::Value) -> Self::Value {
        value.clone()
    }

    fn apply_update(&self, value: Option<&Self::Value>, update: Self::Value) -> ApplyUpdateResult<Self::Value> {
        apply_update(value, update)
    }
}

/// [`EditorOptionImpl`] sem o tipo associado, para a tabela de registro (`editorOptionsRegistry`).
pub trait ErasedEditorOption {
    fn id(&self) -> EditorOption;
    fn name(&self) -> &str;
    fn schema(&self) -> Option<&Dynamic>;
    fn default_value(&self) -> Rc<dyn Any>;
    fn validate(&self, input: &Dynamic) -> Rc<dyn Any>;
    fn compute(&self, env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &dyn Any) -> Rc<dyn Any>;
    /// Devolve o novo valor e se mudou.
    fn apply_update(&self, value: Option<&dyn Any>, update: &dyn Any) -> (Rc<dyn Any>, bool);
    /// A igualdade estrutural de `EditorOptionsUtil._deepEquals` sobre dois valores calculados desta opção.
    fn values_equal(&self, a: &dyn Any, b: &dyn Any) -> bool;
}

fn downcast<T: 'static>(value: &dyn Any) -> &T {
    value.downcast_ref::<T>().expect("valor do tipo da opção")
}

impl<O: EditorOptionImpl> ErasedEditorOption for O {
    fn id(&self) -> EditorOption {
        EditorOptionImpl::id(self)
    }

    fn name(&self) -> &str {
        EditorOptionImpl::name(self)
    }

    fn schema(&self) -> Option<&Dynamic> {
        EditorOptionImpl::schema(self)
    }

    fn default_value(&self) -> Rc<dyn Any> {
        Rc::new(EditorOptionImpl::default_value(self))
    }

    fn validate(&self, input: &Dynamic) -> Rc<dyn Any> {
        Rc::new(EditorOptionImpl::validate(self, input))
    }

    fn compute(&self, env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &dyn Any) -> Rc<dyn Any> {
        Rc::new(EditorOptionImpl::compute(self, env, options, downcast::<O::Value>(value)))
    }

    fn apply_update(&self, value: Option<&dyn Any>, update: &dyn Any) -> (Rc<dyn Any>, bool) {
        let result = EditorOptionImpl::apply_update(self, value.map(downcast::<O::Value>), downcast::<O::Value>(update).clone());
        (Rc::new(result.new_value), result.did_change)
    }

    fn values_equal(&self, a: &dyn Any, b: &dyn Any) -> bool {
        downcast::<O::Value>(a) == downcast::<O::Value>(b)
    }
}

// ---------------------------------------------------------------------------------------------
// Conversões do JavaScript

/// `parseInt(value, 10)` (`NaN` quando não há dígito).
fn js_parse_int(text: &str) -> f64 {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let n: f64 = digits.parse().unwrap_or(f64::INFINITY);
    if negative { -n } else { n }
}

/// `parseFloat(value)` (`NaN` quando não há número no começo).
fn js_parse_float(text: &str) -> f64 {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let sign = |n: f64| if negative { -n } else { n };
    if rest.starts_with("Infinity") {
        return sign(f64::INFINITY);
    }
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i;
    let mut frac_digits = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        frac_digits = j - i - 1;
        if int_digits > 0 || frac_digits > 0 {
            i = j;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    // Expoente só vale com pelo menos um dígito depois do `e`.
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > start {
            i = j;
        }
    }
    sign(rest[..i].parse::<f64>().unwrap_or(f64::NAN))
}

/// `value | 0` do JavaScript (`ToInt32`).
fn to_int32(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let modulo = value.trunc().rem_euclid(4_294_967_296.0);
    if modulo >= 2_147_483_648.0 { modulo - 4_294_967_296.0 } else { modulo }
}

/// `boolean(value, defaultValue)`.
pub fn boolean(value: &Dynamic, default_value: bool) -> bool {
    if value.is_undefined() {
        return default_value;
    }
    if matches!(value, Dynamic::String(s) if *s == "false") {
        // A string 'false' conta como falso.
        return false;
    }
    value.is_truthy()
}

/// `clampedInt<T>(value, defaultValue, minimum, maximum)`: `Err(default)` é o caso `number | T` em que
/// vale o valor padrão.
pub fn clamped_int_or<T>(value: &Dynamic, default_value: T, minimum: f64, maximum: f64) -> Result<f64, T> {
    let number = match value {
        Dynamic::String(s) => js_parse_int(&s.to_string_lossy()),
        Dynamic::Number(n) => *n,
        _ => return Err(default_value),
    };
    if number.is_nan() {
        return Err(default_value);
    }
    // `Math.max`/`Math.min` com NaN dão NaN, mas aqui `number` já não é NaN.
    let mut r = number;
    r = if minimum.is_nan() || r.is_nan() { f64::NAN } else { r.max(minimum) };
    r = if maximum.is_nan() || r.is_nan() { f64::NAN } else { r.min(maximum) };
    Ok(to_int32(r))
}

/// `clampedInt` com padrão numérico.
pub fn clamped_int(value: &Dynamic, default_value: f64, minimum: f64, maximum: f64) -> f64 {
    clamped_int_or(value, default_value, minimum, maximum).unwrap_or_else(|d| d)
}

/// `EditorFloatOption.float`.
pub fn float(value: &Dynamic, default_value: f64) -> f64 {
    let number = match value {
        Dynamic::String(s) => js_parse_float(&s.to_string_lossy()),
        Dynamic::Number(n) => *n,
        _ => return default_value,
    };
    if number.is_nan() { default_value } else { number }
}

/// `EditorFloatOption.clamp`.
pub fn clamp_float(n: f64, min: f64, max: f64) -> f64 {
    if n < min {
        return min;
    }
    if n > max {
        return max;
    }
    n
}

/// `clampedFloat(value, defaultValue, minimum, maximum)`.
pub fn clamped_float(value: &Dynamic, default_value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_undefined() {
        return default_value;
    }
    clamp_float(float(value, default_value), minimum, maximum)
}

/// `EditorStringOption.string`.
pub fn string(value: &Dynamic, default_value: &str) -> String {
    match value {
        Dynamic::String(s) => s.to_string_lossy(),
        _ => default_value.to_string(),
    }
}

/// `stringSet(value, defaultValue, allowedValues, renamedValues)`.
pub fn string_set(value: &Dynamic, default_value: &str, allowed_values: &[&str], renamed_values: Option<&[(&str, &str)]>) -> String {
    let Dynamic::String(value) = value else {
        return default_value.to_string();
    };
    let value = value.to_string_lossy();
    if let Some(renamed) = renamed_values {
        if let Some((_, to)) = renamed.iter().find(|(from, _)| *from == value) {
            return (*to).to_string();
        }
    }
    if !allowed_values.contains(&value.as_str()) {
        return default_value.to_string();
    }
    value
}

/// Peças comuns aos arquivos de cada opção (`config/editor_options/*.rs`).
#[macro_use]
pub mod option_support;

// ---------------------------------------------------------------------------------------------
// Opções concretas

/// `ComputedEditorOption`: opção que só existe calculada (`name = "_never_"`, sem esquema, `validate`
/// devolve o padrão).
pub struct ComputedEditorOption<V> {
    id: EditorOption,
    default_value: V,
    compute_fn: fn(&EnvironmentalOptions, &ComputedEditorOptions, &V) -> V,
}

impl<V> ComputedEditorOption<V> {
    pub fn new(id: EditorOption, default_value: V, compute_fn: fn(&EnvironmentalOptions, &ComputedEditorOptions, &V) -> V) -> Self {
        Self { id, default_value, compute_fn }
    }
}

impl<V: Clone + PartialEq + 'static> EditorOptionImpl for ComputedEditorOption<V> {
    type Value = V;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        "_never_"
    }

    fn default_value(&self) -> V {
        self.default_value.clone()
    }

    fn validate(&self, _input: &Dynamic) -> V {
        self.default_value.clone()
    }

    fn compute(&self, env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &V) -> V {
        (self.compute_fn)(env, options, value)
    }
}

/// `schema.<key> = value` do original, na ordem de chamada. Um esquema que não é objeto (o
/// original só passa objetos) não recebe nada.
fn write_schema(schema: &Option<Dynamic>, entries: Vec<(&str, Dynamic)>) {
    if let Some(Dynamic::Object(object)) = schema {
        for (key, value) in entries {
            object.set(&key.into(), value);
        }
    }
}

fn strings_array(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::string(value)))
}

/// `EditorBooleanOption`.
pub struct EditorBooleanOption {
    id: EditorOption,
    name: &'static str,
    default_value: bool,
    schema: Option<Dynamic>,
}

impl EditorBooleanOption {
    pub fn new(id: EditorOption, name: &'static str, default_value: bool, schema: Option<Dynamic>) -> Self {
        write_schema(&schema, vec![("type", Dynamic::string("boolean")), ("default", default_value.into())]);
        Self { id, name, default_value, schema }
    }
}

impl EditorOptionImpl for EditorBooleanOption {
    type Value = bool;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> bool {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> bool {
        boolean(input, self.default_value)
    }
}

/// `EditorIntOption`. O valor é `f64` porque o `number` do JavaScript é.
pub struct EditorIntOption {
    id: EditorOption,
    name: &'static str,
    default_value: f64,
    pub minimum: f64,
    pub maximum: f64,
    schema: Option<Dynamic>,
}

impl EditorIntOption {
    pub fn new(id: EditorOption, name: &'static str, default_value: f64, minimum: f64, maximum: f64, schema: Option<Dynamic>) -> Self {
        write_schema(
            &schema,
            vec![
                ("type", Dynamic::string("integer")),
                ("default", default_value.into()),
                ("minimum", minimum.into()),
                ("maximum", maximum.into()),
            ],
        );
        Self { id, name, default_value, minimum, maximum, schema }
    }
}

impl EditorOptionImpl for EditorIntOption {
    type Value = f64;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> f64 {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> f64 {
        clamped_int(input, self.default_value, self.minimum, self.maximum)
    }
}

/// `EditorFloatOption`.
pub struct EditorFloatOption {
    id: EditorOption,
    name: &'static str,
    default_value: f64,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub validation_fn: fn(f64) -> f64,
    schema: Option<Dynamic>,
}

impl EditorFloatOption {
    pub fn new(
        id: EditorOption,
        name: &'static str,
        default_value: f64,
        validation_fn: fn(f64) -> f64,
        schema: Option<Dynamic>,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> Self {
        // O original escreve `minimum`/`maximum` mesmo quando `undefined` (a chave passa a existir).
        let optional = |value: Option<f64>| value.map_or(Dynamic::Undefined, Dynamic::Number);
        write_schema(
            &schema,
            vec![
                ("type", Dynamic::string("number")),
                ("default", default_value.into()),
                ("minimum", optional(minimum)),
                ("maximum", optional(maximum)),
            ],
        );
        Self { id, name, default_value, minimum, maximum, validation_fn, schema }
    }
}

impl EditorOptionImpl for EditorFloatOption {
    type Value = f64;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> f64 {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> f64 {
        (self.validation_fn)(float(input, self.default_value))
    }
}

/// `EditorStringOption`.
pub struct EditorStringOption {
    id: EditorOption,
    name: &'static str,
    default_value: String,
    schema: Option<Dynamic>,
}

impl EditorStringOption {
    pub fn new(id: EditorOption, name: &'static str, default_value: &str, schema: Option<Dynamic>) -> Self {
        write_schema(&schema, vec![("type", Dynamic::string("string")), ("default", Dynamic::string(default_value))]);
        Self { id, name, default_value: default_value.to_string(), schema }
    }
}

impl EditorOptionImpl for EditorStringOption {
    type Value = String;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> String {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> String {
        string(input, &self.default_value)
    }
}

/// `EditorStringEnumOption`.
pub struct EditorStringEnumOption {
    id: EditorOption,
    name: &'static str,
    default_value: String,
    allowed_values: Vec<&'static str>,
    schema: Option<Dynamic>,
}

impl EditorStringEnumOption {
    pub fn new(
        id: EditorOption,
        name: &'static str,
        default_value: &str,
        allowed_values: &[&'static str],
        schema: Option<Dynamic>,
    ) -> Self {
        write_schema(
            &schema,
            vec![
                ("type", Dynamic::string("string")),
                ("enum", strings_array(allowed_values)),
                ("default", Dynamic::string(default_value)),
            ],
        );
        Self { id, name, default_value: default_value.to_string(), allowed_values: allowed_values.to_vec(), schema }
    }
}

impl EditorOptionImpl for EditorStringEnumOption {
    type Value = String;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> String {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> String {
        string_set(input, &self.default_value, &self.allowed_values, None)
    }
}

/// `EditorEnumOption`: o valor interno `V` vem de uma das strings permitidas por `convert`.
pub struct EditorEnumOption<V> {
    id: EditorOption,
    name: &'static str,
    default_value: V,
    allowed_values: Vec<&'static str>,
    convert: fn(&str) -> V,
    schema: Option<Dynamic>,
}

impl<V> EditorEnumOption<V> {
    /// `default_string_value` é o `default` do esquema (o valor em texto, diferente de `default_value`).
    pub fn new(
        id: EditorOption,
        name: &'static str,
        default_value: V,
        default_string_value: &str,
        allowed_values: &[&'static str],
        convert: fn(&str) -> V,
        schema: Option<Dynamic>,
    ) -> Self {
        write_schema(
            &schema,
            vec![
                ("type", Dynamic::string("string")),
                ("enum", strings_array(allowed_values)),
                ("default", Dynamic::string(default_string_value)),
            ],
        );
        Self { id, name, default_value, allowed_values: allowed_values.to_vec(), convert, schema }
    }
}

impl<V: Clone + PartialEq + 'static> EditorOptionImpl for EditorEnumOption<V> {
    type Value = V;

    fn id(&self) -> EditorOption {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn default_value(&self) -> V {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        self.schema.as_ref()
    }

    fn validate(&self, input: &Dynamic) -> V {
        let Dynamic::String(input) = input else {
            return self.default_value.clone();
        };
        let input = input.to_string_lossy();
        if !self.allowed_values.contains(&input.as_str()) {
            return self.default_value.clone();
        }
        (self.convert)(&input)
    }
}

// ---------------------------------------------------------------------------------------------
// autoIndent

/// `EditorAutoIndentStrategy`: em ordem crescente, porque o original compara com `>=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EditorAutoIndentStrategy {
    None = 0,
    Keep = 1,
    Brackets = 2,
    Advanced = 3,
    Full = 4,
}

/// `_autoIndentFromString`. O original devolve `undefined` fora das cinco strings, aqui `None`.
pub fn auto_indent_from_string(auto_indent: &str) -> Option<EditorAutoIndentStrategy> {
    match auto_indent {
        "none" => Some(EditorAutoIndentStrategy::None),
        "keep" => Some(EditorAutoIndentStrategy::Keep),
        "brackets" => Some(EditorAutoIndentStrategy::Brackets),
        "advanced" => Some(EditorAutoIndentStrategy::Advanced),
        "full" => Some(EditorAutoIndentStrategy::Full),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::font_info::{BareFontInfoOptions, FontInfoOptions};

    fn num(n: f64) -> Dynamic {
        Dynamic::Number(n)
    }

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn schema_with_description() -> Option<Dynamic> {
        Some(Dynamic::object([("description", text("d"))]))
    }

    fn font_info() -> FontInfo {
        FontInfo::new(
            FontInfoOptions {
                bare: BareFontInfoOptions {
                    pixel_ratio: 1.0,
                    font_family: "monospace".into(),
                    font_weight: "normal".into(),
                    font_size: 14.0,
                    font_feature_settings: "normal".into(),
                    font_variation_settings: "normal".into(),
                    line_height: 19.0,
                    letter_spacing: 0.0,
                },
                is_monospace: true,
                typical_halfwidth_character_width: 8.0,
                typical_fullwidth_character_width: 16.0,
                can_use_halfwidth_rightwards_arrow: true,
                space_width: 8.0,
                middot_width: 8.0,
                wsmiddot_width: 8.0,
                max_digit_width: 8.0,
            },
            true,
        )
    }

    #[test]
    fn enum_ids_follow_the_original_order() {
        assert_eq!(EditorOption::AcceptSuggestionOnCommitCharacter as usize, 0);
        assert_eq!(EditorOption::AccessibilitySupport as usize, 2);
        assert_eq!(EditorOption::ReadOnly as usize, 105);
        assert_eq!(EditorOption::WordWrapIndicator as usize, EditorOption::EffectiveCursorStyle as usize - 1);
        assert_eq!(EditorOption::EffectiveFullwidthCharacterWidth as usize, EditorOption::COUNT - 1);
        assert_eq!(EDITOR_OPTION_NAMES.len(), EditorOption::COUNT);
        assert_eq!(EditorOption::UseShadowDom.name(), "useShadowDOM");
        for (i, id) in EditorOption::ALL.iter().enumerate() {
            assert_eq!(*id as usize, i);
        }
    }

    #[test]
    fn boolean_cases() {
        assert!(boolean(&Dynamic::Undefined, true));
        assert!(!boolean(&Dynamic::Undefined, false));
        assert!(!boolean(&text("false"), true));
        assert!(boolean(&text("true"), false));
        assert!(boolean(&text("x"), false));
        assert!(!boolean(&text(""), true));
        assert!(!boolean(&Dynamic::Null, true));
        assert!(!boolean(&num(0.0), true));
        assert!(!boolean(&num(f64::NAN), true));
        assert!(boolean(&num(1.0), false));
        assert!(boolean(&Dynamic::array([]), false));
        assert!(boolean(&Dynamic::object([("a", Dynamic::Null)]), false));
        assert!(!boolean(&Dynamic::Bool(false), true));
    }

    #[test]
    fn clamped_int_cases() {
        assert_eq!(clamped_int(&text("12"), 5.0, 0.0, 100.0), 12.0);
        assert_eq!(clamped_int(&text("12px"), 5.0, 0.0, 100.0), 12.0);
        assert_eq!(clamped_int(&text("abc"), 5.0, 0.0, 100.0), 5.0);
        assert_eq!(clamped_int(&Dynamic::Undefined, 5.0, 0.0, 100.0), 5.0);
        assert_eq!(clamped_int(&Dynamic::Bool(true), 5.0, 0.0, 100.0), 5.0);
        assert_eq!(clamped_int(&num(f64::NAN), 5.0, 0.0, 100.0), 5.0);
        assert_eq!(clamped_int(&num(5.7), 0.0, 0.0, 100.0), 5.0);
        assert_eq!(clamped_int(&num(-5.7), 0.0, -10.0, 100.0), -5.0);
        assert_eq!(clamped_int(&num(-50.0), 0.0, 1.0, 100.0), 1.0);
        assert_eq!(clamped_int(&num(500.0), 0.0, 1.0, 100.0), 100.0);
        assert_eq!(clamped_int(&text("-3"), 0.0, -10.0, 10.0), -3.0);
        // `r | 0` dá a volta em 32 bits.
        assert_eq!(clamped_int(&num(1e12), 0.0, 0.0, 1e12), -727_379_968.0);
        assert_eq!(clamped_int_or(&text("x"), "padrao", 0.0, 1.0), Err("padrao"));
        assert_eq!(clamped_int_or(&num(1.0), "padrao", 0.0, 1.0), Ok(1.0));
    }

    #[test]
    fn clamped_float_cases() {
        assert_eq!(clamped_float(&Dynamic::Undefined, 1.5, 0.0, 10.0), 1.5);
        assert_eq!(clamped_float(&text("2.5"), 1.5, 0.0, 10.0), 2.5);
        assert_eq!(clamped_float(&text("2.5abc"), 1.5, 0.0, 10.0), 2.5);
        assert_eq!(clamped_float(&text(".5"), 1.0, 0.0, 10.0), 0.5);
        assert_eq!(clamped_float(&text("1e1"), 1.0, 0.0, 100.0), 10.0);
        assert_eq!(clamped_float(&text("abc"), 1.5, 0.0, 10.0), 1.5);
        assert_eq!(clamped_float(&Dynamic::Null, 1.5, 0.0, 10.0), 1.5);
        assert_eq!(clamped_float(&num(-3.0), 1.5, 0.0, 10.0), 0.0);
        assert_eq!(clamped_float(&num(30.0), 1.5, 0.0, 10.0), 10.0);
        assert_eq!(clamped_float(&text("-Infinity"), 1.5, -5.0, 10.0), -5.0);
    }

    #[test]
    fn string_set_cases() {
        let allowed = ["on", "off", "smart"];
        assert_eq!(string_set(&text("off"), "on", &allowed, None), "off");
        assert_eq!(string_set(&text("nope"), "on", &allowed, None), "on");
        assert_eq!(string_set(&num(1.0), "on", &allowed, None), "on");
        assert_eq!(string_set(&Dynamic::Undefined, "on", &allowed, None), "on");
        assert_eq!(string_set(&Dynamic::array([text("on")]), "off", &allowed, None), "off");
        let renamed: &[(&str, &str)] = &[("true", "on"), ("false", "off")];
        assert_eq!(string_set(&text("true"), "smart", &allowed, Some(renamed)), "on");
        assert_eq!(string_set(&text("false"), "smart", &allowed, Some(renamed)), "off");
        // O renomeado vence mesmo se o destino não estiver na lista.
        let odd: &[(&str, &str)] = &[("old", "zzz")];
        assert_eq!(string_set(&text("old"), "on", &allowed, Some(odd)), "zzz");
        assert_eq!(string_set(&text("smart"), "on", &allowed, Some(renamed)), "smart");
    }

    #[test]
    fn apply_update_by_equality() {
        assert_eq!(apply_update(Some(&1.0), 1.0), ApplyUpdateResult { new_value: 1.0, did_change: false });
        assert_eq!(apply_update(Some(&1.0), 2.0), ApplyUpdateResult { new_value: 2.0, did_change: true });
        assert!(apply_update(None, true).did_change);
        assert!(!apply_update(Some(&vec![1, 2]), vec![1, 2]).did_change);
        assert!(apply_update(Some(&vec![1, 2]), vec![1, 3]).did_change);
        assert!(apply_update(Some(&f64::NAN), f64::NAN).did_change);
    }

    #[test]
    fn simple_options_validate_and_fill_schema() {
        let boolean_option = EditorBooleanOption::new(EditorOption::ReadOnly, "readOnly", true, schema_with_description());
        assert!(EditorOptionImpl::validate(&boolean_option, &Dynamic::Undefined));
        assert!(!EditorOptionImpl::validate(&boolean_option, &text("false")));
        let schema = EditorOptionImpl::schema(&boolean_option).expect("sem esquema");
        assert_eq!(schema.get_str("type"), text("boolean"));
        assert_eq!(schema.get_str("default"), Dynamic::Bool(true));
        assert_eq!(schema.get_str("description"), text("d"));
        assert_eq!(schema.own_keys(), vec!["description", "type", "default"]);

        let int_option = EditorIntOption::new(EditorOption::CursorWidth, "cursorWidth", 0.0, 0.0, 10.0, Some(Dynamic::object::<&str>([])));
        assert_eq!(EditorOptionImpl::validate(&int_option, &num(99.0)), 10.0);
        let schema = EditorOptionImpl::schema(&int_option).expect("sem esquema");
        assert_eq!(schema.get_str("type"), text("integer"));
        assert_eq!((schema.get_str("minimum"), schema.get_str("maximum")), (num(0.0), num(10.0)));
        assert_eq!(schema.own_keys(), vec!["type", "default", "minimum", "maximum"]);

        let float_option = EditorFloatOption::new(EditorOption::LetterSpacing, "letterSpacing", 0.0, |n| clamp_float(n, -5.0, 20.0), None, None, None);
        assert_eq!(EditorOptionImpl::validate(&float_option, &text("30")), 20.0);
        assert_eq!(EditorOptionImpl::validate(&float_option, &text("zz")), 0.0);
        assert!(EditorOptionImpl::schema(&float_option).is_none());

        // O original escreve `minimum`/`maximum` mesmo quando `undefined`: as chaves existem.
        let float_with_schema =
            EditorFloatOption::new(EditorOption::LetterSpacing, "letterSpacing", 1.0, |n| n, Some(Dynamic::object::<&str>([])), Some(0.5), None);
        let schema = EditorOptionImpl::schema(&float_with_schema).expect("sem esquema");
        assert_eq!(schema.get_str("type"), text("number"));
        assert_eq!(schema.own_keys(), vec!["type", "default", "minimum", "maximum"]);
        assert_eq!(schema.get_str("minimum"), num(0.5));
        assert_eq!(schema.get_str("maximum"), Dynamic::Undefined);

        let string_option = EditorStringOption::new(EditorOption::AriaLabel, "ariaLabel", "x", Some(Dynamic::object::<&str>([])));
        assert_eq!(EditorOptionImpl::validate(&string_option, &text("y")), "y");
        assert_eq!(EditorOptionImpl::validate(&string_option, &num(1.0)), "x");
        let schema = EditorOptionImpl::schema(&string_option).expect("sem esquema");
        assert_eq!((schema.get_str("type"), schema.get_str("default")), (text("string"), text("x")));

        let enum_option = EditorStringEnumOption::new(
            EditorOption::AcceptSuggestionOnEnter,
            "acceptSuggestionOnEnter",
            "on",
            &["on", "smart", "off"],
            Some(Dynamic::object::<&str>([])),
        );
        assert_eq!(EditorOptionImpl::validate(&enum_option, &text("smart")), "smart");
        assert_eq!(EditorOptionImpl::validate(&enum_option, &text("x")), "on");
        let schema = EditorOptionImpl::schema(&enum_option).expect("sem esquema");
        assert_eq!(schema.get_str("enum").get_str("length"), num(3.0));
        assert_eq!(schema.own_keys(), vec!["type", "enum", "default"]);

        let auto_indent = EditorEnumOption::new(
            EditorOption::AutoIndent,
            "autoIndent",
            EditorAutoIndentStrategy::Full,
            "full",
            &["none", "keep", "brackets", "advanced", "full"],
            |s| auto_indent_from_string(s).unwrap_or(EditorAutoIndentStrategy::Full),
            Some(Dynamic::object::<&str>([])),
        );
        let schema = EditorOptionImpl::schema(&auto_indent).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["type", "enum", "default"]);
        assert_eq!(schema.get_str("default"), text("full"));
        assert_eq!(EditorOptionImpl::validate(&auto_indent, &text("keep")), EditorAutoIndentStrategy::Keep);
        assert_eq!(EditorOptionImpl::validate(&auto_indent, &text("x")), EditorAutoIndentStrategy::Full);
        assert_eq!(EditorOptionImpl::validate(&auto_indent, &num(1.0)), EditorAutoIndentStrategy::Full);
        assert_eq!(auto_indent_from_string("brackets"), Some(EditorAutoIndentStrategy::Brackets));
        assert_eq!(auto_indent_from_string("x"), None);
    }

    #[test]
    fn typed_keys_and_events() {
        const READ_ONLY: Opt<bool> = Opt::new(EditorOption::ReadOnly);
        const FONT_SIZE: Opt<f64> = Opt::new(EditorOption::FontSize);
        let values: Vec<Rc<dyn Any>> = EditorOption::ALL
            .iter()
            .map(|id| match id {
                EditorOption::ReadOnly => Rc::new(true) as Rc<dyn Any>,
                EditorOption::FontSize => Rc::new(14.0_f64) as Rc<dyn Any>,
                _ => Rc::new(()) as Rc<dyn Any>,
            })
            .collect();
        let options = ComputedEditorOptions::new(values);
        assert!(*options.get(READ_ONLY));
        assert_eq!(*options.get(FONT_SIZE), 14.0);

        let mut changed = vec![false; EditorOption::COUNT];
        changed[EditorOption::FontSize as usize] = true;
        let event = ConfigurationChangedEvent::new(changed);
        assert!(event.has_changed(EditorOption::FontSize));
        assert!(!event.has_changed(EditorOption::ReadOnly));
        assert!(!ConfigurationChangedEvent::new(vec![]).has_changed(EditorOption::ReadOnly));
    }

    #[test]
    fn computed_option_and_erased_registry() {
        let computed = ComputedEditorOption::new(EditorOption::PixelRatio, 1.0, |env, _, _| env.pixel_ratio);
        assert_eq!(EditorOptionImpl::name(&computed), "_never_");
        assert_eq!(EditorOptionImpl::validate(&computed, &text("x")), 1.0);
        assert!(EditorOptionImpl::schema(&computed).is_none());

        let env = EnvironmentalOptions {
            memory: None,
            outer_width: 0.0,
            outer_height: 0.0,
            font_info: font_info(),
            extra_editor_class_name: String::new(),
            is_dominated_by_long_lines: false,
            view_line_count: 1.0,
            line_numbers_digit_count: 1.0,
            empty_selection_clipboard: true,
            pixel_ratio: 2.0,
            tab_focus_mode: false,
            input_mode: InputMode::Insert,
            accessibility_support: AccessibilitySupport::Unknown,
            glyph_margin_decoration_lane_count: 1.0,
            edit_context_supported: false,
        };
        let options = ComputedEditorOptions::new(EditorOption::ALL.iter().map(|_| Rc::new(()) as Rc<dyn Any>).collect());
        let erased: &dyn ErasedEditorOption = &computed;
        let value = erased.compute(&env, &options, &1.0_f64);
        assert_eq!(value.downcast_ref::<f64>(), Some(&2.0));
        let (new_value, did_change) = erased.apply_update(Some(&2.0_f64), &3.0_f64);
        assert_eq!(new_value.downcast_ref::<f64>(), Some(&3.0));
        assert!(did_change);
    }
}

// ---------------------------------------------------------------------------------------------
// Classes de opção, uma por arquivo em `config/editor_options/`.

pub mod accessibility_support;
pub mod bracket_pair_colorization;
pub mod comments;
pub mod cursor_style;
pub mod drop_into_editor;
pub mod editor_class_name;
pub mod editor_font_info;
pub mod effective_options;
pub mod empty_selection_clipboard;
pub mod find;
pub mod font_ligatures;
pub mod font_size;
pub mod font_variations;
pub mod font_weight;
pub mod goto_location;
pub mod guides;
pub mod hover;
pub mod inlay_hints;
pub mod inline_suggest;
pub mod layout_info;
pub mod layout_info_computer;
pub mod lightbulb;
pub mod line_decorations_width;
pub mod line_height;
pub mod line_numbers;
pub mod minimap;
pub mod padding;
pub mod parameter_hints;
pub mod paste_as;
pub mod pixel_ratio;
pub mod placeholder;
pub mod quick_suggestions;
pub mod registry;
pub mod rulers;
pub mod scrollbar;
pub mod simple_keys;
pub mod smart_select;
pub mod sticky_scroll;
pub mod suggest;
pub mod unicode_highlight;
pub mod word_segmenter_locales;
pub mod wrapping_indent;
pub mod wrapping_info;
pub mod wrapping_strategy;
