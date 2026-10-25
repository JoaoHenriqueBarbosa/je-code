//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/ghostText/ghostTextView.ts`: `GhostTextView`,
//! `GhostTextWidgetWarning`, `IGhostTextWidgetData`, as decorações inline do texto fantasma, o
//! `AdditionalLinesWidget` (a view zone das linhas adicionais), o `computeGhostTextViewData` e o realce de sintaxe.
//!
//! Notas de tradução:
//! - `GhostText | GhostTextReplacement` é o [`GhostTextOrReplacement`]; `IGhostTextWidgetData` é o trait
//!   [`IGhostTextWidgetData`] (o `handleInlineCompletionShown` é um método).
//! - O `@ILanguageService` do construtor só serve ao `languageIdCodec`; o porte recebe o [`ILanguageIdCodec`] direto.
//! - O `attachedData` das decorações de texto injetado é o [`InjectedTextAttachedData`] de `model`: o
//!   `GhostTextAttachedData` leva o dono da view como uma ficha (`Rc`) comparada por identidade, como o `owner === this`
//!   do original. O hover e o clique no texto injetado leem esse dado do alvo do mouse.
//! - `_isDisposed` é um observável; `dispose` o marca antes de descartar o resto, na mesma ordem de registro do
//!   original (primeiro o widget das linhas, depois o marcador, depois as decorações).
//! - `derivedOpts({ equalsFn: equalsIfDefinedC(thisEqualsC()) })` do dado do widget é o `equals_fn` de
//!   [`AdditionalLinesData::equals`]. Os valores que o original devolve como objetos novos a cada cálculo são
//!   `Rc` (a igualdade estrita compara a identidade, como o `===`).
//! - `StringBuilder` + `innerHTML` de `renderLines`, `ttPolicy` e `applyFontInfo` não existem: a UI é nativa. Quem
//!   desenha a view zone recebe as [`LineData`] prontas (tokens e decorações de classe) em [`GhostTextViewNode`] e
//!   aplica nelas o `renderViewLine` com as opções do editor (`renderWhitespace` sempre `none`, para não confundir
//!   com o texto real); o `editorOptionsChanged` do original continua recriando a zona quando uma dessas opções muda,
//!   para o desenhista ler as novas. O `tabSize` do modelo viaja no nó.
//! - `editor.changeViewZones` e `editor.onMouseUp` são os do [`CodeEditorWidget`], como no original. A view zone é o
//!   [`GhostTextViewZone`] (`IViewZone`), e o `domNode` que `renderLines` monta é o [`GhostTextViewNode`], que o
//!   pintor nativo recupera por `downcast` de [`IViewZone::dom_node`]. O `mousedown` com `preventDefault` (para a zona
//!   não roubar o foco do editor) e o filtro `isTargetGhostText` do `click` são de quem pinta o nó: ele só chama
//!   `on_click` quando o clique cai num elemento de texto fantasma, e o nó não tira o foco do editor. O hover do
//!   widget (`isTargetGhostText(p.target.element)`) vira "o alvo é a view zone do widget" (`view_zone_id`).
//! - O `_viewZoneListener` (um `MutableDisposable` com os ouvintes DOM) não tem análogo: os ouvintes são o `on_click`
//!   da zona, que sai junto com ela.
//!
//! Divergências (fora do escopo, por nome de membro do original):
//! - `getWarningWidgetContext` e o `ghostTextViewWarningWidgetData` do nó DOM: o aviso é desenhado por quem tem o
//!   widget de conteúdo, que lê o intervalo e o ícone de [`GhostTextWarningWidget`]. Com `USE_SQUIGGLES_FOR_WARNING`
//!   verdadeiro (a constante do original) esse widget nunca é criado, como no original.
//! - `RangeSingleLine` (`ranges/rangeSingleLine.ts`) ainda não tem porte próprio: é o tipo local [`RangeSingleLine`].
//! - O conteúdo do texto injetado vira `String` (`InjectedTextOptions.content`): uma unidade UTF-16 solta (par
//!   substituto partido) é substituída por U+FFFD na decoração; o texto das linhas adicionais continua [`JsString`].

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::js_string;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::observable::base::{IObservable, ISettableObservable};
use je_base::observable::debug_name::{DebugNameOptions, DebugNameSource};
use je_base::observable::deps::EqualityComparer;
use je_base::observable::observables::const_observable::const_observable;
use je_base::observable::observables::derived::{DerivedOptions, derived, derived_opts};
use je_base::observable::observables::observable_signal_from_event::observable_signal_from_event;
use je_base::observable::observables::observable_value::{ObservableValueOptions, observable_value_opts};
use je_base::observable::reactions::autorun::{autorun, autorun_with_store};
use je_base::themables::ThemeIcon;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::column_range::ColumnRange;
use crate::config::editor_options::EditorOption;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::cursor_common::CursorSimpleModel;
use crate::edits::edit::BaseEdit;
use crate::edits::string_edit::{BaseStringEdit, StringEdit, StringReplacement};
use crate::editor_browser::{
    ContentWidgetPosition, ContentWidgetPositionPreference, IViewZone, IViewZoneChangeAccessor, MouseEvent, MouseTarget,
};
use crate::inline_completions::ghost_text::{GhostTextLine, GhostTextOrReplacement};
use crate::inline_completions::inline_completion_view_data::InlineCompletionViewData;
use crate::inline_decorations::InlineDecorationType;
use crate::languages::{ILanguageIdCodec, InlineCompletionWarning};
use crate::line_decorations::LineDecoration;
use crate::line_tokens::{LineTokens, TokenArray};
use crate::model::{
    InjectedTextAttachedData, InjectedTextCursorStops, InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration,
    PositionAffinity,
};
use crate::observable_code_editor::{IObservableContentWidget, observable_code_editor};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};

/// `IGhostTextWidgetData`.
pub trait IGhostTextWidgetData {
    /// `ghostText`.
    fn ghost_text(&self) -> &GhostTextOrReplacement;

    /// `warning`.
    fn warning(&self) -> Option<&GhostTextWidgetWarning>;

    /// `handleInlineCompletionShown(viewData)`.
    fn handle_inline_completion_shown(&self, view_data: &InlineCompletionViewData);
}

/// `GhostTextWidgetWarning`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhostTextWidgetWarning {
    pub icon: ThemeIcon,
}

impl GhostTextWidgetWarning {
    /// `GhostTextWidgetWarning.from(warning)`.
    pub fn from(warning: Option<&InlineCompletionWarning>) -> Option<GhostTextWidgetWarning> {
        let warning = warning?;
        Some(GhostTextWidgetWarning::new(warning.icon.clone()))
    }

    /// O construtor: o ícone padrão é `Codicon.warning`.
    pub fn new(icon: Option<ThemeIcon>) -> Self {
        Self { icon: icon.unwrap_or_else(|| ThemeIcon { id: "warning".to_owned(), color: None }) }
    }
}

const USE_SQUIGGLES_FOR_WARNING: bool = true;

/// `GHOST_TEXT_CLASS_NAME`.
pub const GHOST_TEXT_CLASS_NAME: &str = "ghost-text";

/// `ranges/rangeSingleLine.ts`: um intervalo de colunas de uma linha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeSingleLine {
    pub line_number: i32,
    pub column_range: ColumnRange,
}

/// O `GhostTextAttachedData`: o dono da view é a ficha, comparada por identidade.
struct GhostTextAttachedData {
    owner: Rc<OwnerToken>,
}

/// Identidade de um `GhostTextView` (o `this` do `owner === this`).
struct OwnerToken;

impl GhostTextAttachedData {
    /// `a instanceof GhostTextAttachedData && a.owner === this`.
    fn is_owned_by(data: &InjectedTextAttachedData, owner: &Rc<OwnerToken>) -> bool {
        data.downcast::<GhostTextAttachedData>().is_some_and(|attached| Rc::ptr_eq(&attached.owner, owner))
    }
}

/// `{ column, text, preview, lineDecorations }` de `inlineTexts` em `computeGhostTextViewData`.
#[derive(Clone, Debug)]
struct InlineText {
    column: i32,
    text: JsString,
    preview: bool,
    line_decorations: Vec<LineDecoration>,
}

/// `{ content, decorations }` de `additionalLines` em `computeGhostTextViewData`.
#[derive(Clone, Debug)]
struct AdditionalLine {
    content: JsString,
    decorations: Vec<LineDecoration>,
}

/// O resultado de `computeGhostTextViewData`.
struct GhostTextViewData {
    inline_texts: Vec<InlineText>,
    additional_lines: Vec<AdditionalLine>,
    hidden_range: Option<ColumnRange>,
    additional_lines_original_suffix: Option<RangeSingleLine>,
}

/// `addToAdditionalLines(ghLines, className)`.
fn add_to_additional_lines(additional_lines: &mut Vec<AdditionalLine>, mut gh_lines: &[GhostTextLine], class_name: Option<&str>) {
    if let Some(last_line) = additional_lines.last_mut() {
        let first = &gh_lines[0];
        if let Some(class_name) = class_name {
            let start = last_line.content.len() as i32 + 1;
            last_line.decorations.push(LineDecoration::new(
                start,
                start + first.line.len() as i32,
                class_name,
                InlineDecorationType::Regular,
            ));
        }
        last_line.content.push_units(first.line.units());

        gh_lines = &gh_lines[1..];
    }
    for gh_line in gh_lines {
        let mut decorations = Vec::new();
        if let Some(class_name) = class_name {
            decorations.push(LineDecoration::new(1, gh_line.line.len() as i32 + 1, class_name, InlineDecorationType::Regular));
        }
        decorations.extend(gh_line.line_decorations.iter().cloned());
        additional_lines.push(AdditionalLine { content: gh_line.line.clone(), decorations });
    }
}

/// `computeGhostTextViewData(ghostText, textModel, ghostTextClassName)`.
fn compute_ghost_text_view_data(ghost_text: &GhostTextOrReplacement, text_model: &TextModel, ghost_text_class_name: &str) -> GhostTextViewData {
    let mut inline_texts: Vec<InlineText> = Vec::new();
    let mut additional_lines: Vec<AdditionalLine> = Vec::new();

    let line_number = ghost_text.line_number();
    let text_buffer_line = CursorSimpleModel::get_line_content(text_model, line_number);

    let mut hidden_text_start_column: Option<i32> = None;
    let mut last_idx: i32 = 0;
    for part in ghost_text.parts() {
        let mut gh_lines: &[GhostTextLine] = &part.lines;
        if hidden_text_start_column.is_none() {
            inline_texts.push(InlineText {
                column: part.column,
                text: gh_lines[0].line.clone(),
                preview: part.preview,
                line_decorations: gh_lines[0].line_decorations.clone(),
            });
            gh_lines = &gh_lines[1..];
        } else {
            let between = js_string::substring(text_buffer_line.units(), i64::from(last_idx), i64::from(part.column - 1));
            let between = GhostTextLine { line: JsString::from(between), line_decorations: Vec::new() };
            add_to_additional_lines(&mut additional_lines, std::slice::from_ref(&between), None);
        }

        if !gh_lines.is_empty() {
            add_to_additional_lines(&mut additional_lines, gh_lines, Some(ghost_text_class_name));
            if hidden_text_start_column.is_none() && part.column <= text_buffer_line.len() as i32 {
                hidden_text_start_column = Some(part.column);
            }
        }

        last_idx = part.column - 1;
    }
    let line_end_column = text_buffer_line.len() as i32 + 1;
    let additional_lines_original_suffix = hidden_text_start_column
        .map(|_| RangeSingleLine { line_number, column_range: ColumnRange::new(last_idx + 1, line_end_column) });

    let hidden_range = hidden_text_start_column.map(|start| ColumnRange::new(start, line_end_column));

    GhostTextViewData { inline_texts, additional_lines, hidden_range, additional_lines_original_suffix }
}

/// `LineData`.
#[derive(Clone)]
pub struct LineData {
    /// Must not contain a linebreak!
    pub content: LineTokens,
    pub decorations: Vec<LineDecoration>,
}

impl LineData {
    pub fn new(content: LineTokens, decorations: Vec<LineDecoration>) -> Self {
        Self { content, decorations }
    }

    /// `equals(other)`.
    pub fn equals(&self, other: &LineData) -> bool {
        self.content.equals_line_tokens(&other.content) && self.decorations == other.decorations
    }
}

/// `AdditionalLinesData`.
pub struct AdditionalLinesData {
    pub line_number: i32,
    pub additional_lines: Rc<Vec<LineData>>,
    pub min_reserved_line_count: i32,
}

impl AdditionalLinesData {
    /// `equals(other)`.
    pub fn equals(&self, other: &AdditionalLinesData) -> bool {
        self.line_number == other.line_number
            && self.min_reserved_line_count == other.min_reserved_line_count
            && self.additional_lines.len() == other.additional_lines.len()
            && self.additional_lines.iter().zip(other.additional_lines.iter()).all(|(a, b)| a.equals(b))
    }
}

/// O `domNode` da view zone do texto fantasma (o que `renderLines` monta): as linhas prontas para quem desenha.
pub struct GhostTextViewNode {
    /// As linhas a desenhar (`suggest-preview-text`), uma por linha da zona, na ordem.
    pub lines: Rc<Vec<LineData>>,
    /// `tabSize` do modelo, usado no `RenderLineInput` de cada linha.
    pub tab_size: i32,
    /// A classe `clickable` do contêiner das linhas.
    pub is_clickable: bool,
    /// O `click` do `domNode`, só quando a zona é clicável. Quem pinta só o chama quando o alvo é texto fantasma.
    pub on_click: Option<Rc<dyn Fn(&MouseEvent)>>,
}

/// A view zone do texto fantasma: o `IViewZone` que o `addZone` do original recebe.
pub struct GhostTextViewZone {
    pub after_line_number: i32,
    pub height_in_lines: i32,
    /// O `domNode`.
    pub node: Rc<GhostTextViewNode>,
    /// `onComputedHeight`: a altura final da zona em pixels.
    pub on_computed_height: Rc<dyn Fn(f64)>,
}

impl IViewZone for GhostTextViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number
    }

    /// `afterColumnAffinity: PositionAffinity.Right`.
    fn after_column_affinity(&self) -> Option<PositionAffinity> {
        Some(PositionAffinity::Right)
    }

    fn height_in_lines(&self) -> Option<f64> {
        Some(f64::from(self.height_in_lines))
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        Rc::clone(&self.node) as Rc<dyn Any>
    }

    fn on_computed_height(&self, height: f64) {
        (self.on_computed_height)(height);
    }
}

/// `{ viewZoneId, heightInLines, lineNumber }` de `_viewZoneInfo`.
struct ViewZoneInfo {
    view_zone_id: String,
    height_in_lines: i32,
    line_number: i32,
}

/// O estado de `AdditionalLinesWidget` que o autorun, os ouvintes e o hover compartilham.
struct AdditionalLinesInner {
    editor: Rc<CodeEditorWidget>,
    view_zone_info: RefCell<Option<ViewZoneInfo>>,
    view_zone_height: ISettableObservable<Option<f64>>,
    on_did_click: Rc<Emitter<MouseEvent>>,
    has_been_accepted: Cell<bool>,
    should_keep_cursor_stable: bool,
    is_clickable: bool,
}

impl AdditionalLinesInner {
    /// `clear()`.
    fn clear(&self) {
        self.editor.change_view_zones(&mut |accessor| {
            self.remove_active_view_zone(accessor);
        });
    }

    /// `updateLines(lineNumber, additionalLines, minReservedLineCount)`.
    fn update_lines(&self, line_number: i32, additional_lines: &Rc<Vec<LineData>>, min_reserved_line_count: i32) {
        let Some(text_model) = self.editor.get_model() else {
            return;
        };

        let tab_size = text_model.get_options().tab_size;

        observable_code_editor(&self.editor).transaction(|_| {
            self.editor.change_view_zones(&mut |accessor| {
                self.remove_active_view_zone(accessor);

                let height_in_lines = (additional_lines.len() as i32).max(min_reserved_line_count);
                if height_in_lines > 0 {
                    let on_click: Option<Rc<dyn Fn(&MouseEvent)>> = self.is_clickable.then(|| {
                        let on_did_click = Rc::clone(&self.on_did_click);
                        Rc::new(move |e: &MouseEvent| on_did_click.fire(e.clone())) as Rc<dyn Fn(&MouseEvent)>
                    });
                    let view_zone_height = Rc::clone(&self.view_zone_height);
                    let id = accessor.add_zone(Rc::new(GhostTextViewZone {
                        after_line_number: line_number,
                        height_in_lines,
                        node: Rc::new(GhostTextViewNode {
                            lines: Rc::clone(additional_lines),
                            tab_size,
                            is_clickable: self.is_clickable,
                            on_click,
                        }),
                        // Nota do original: seria possível usar uma transação para evitar cintilação?
                        on_computed_height: Rc::new(move |height| view_zone_height.set(Some(height), None)),
                    }));

                    self.keep_cursor_stable(line_number, height_in_lines);

                    *self.view_zone_info.borrow_mut() = Some(ViewZoneInfo { view_zone_id: id, height_in_lines, line_number });
                }
            });
        });
    }

    /// `removeActiveViewZone(changeAccessor)`.
    fn remove_active_view_zone(&self, accessor: &mut dyn IViewZoneChangeAccessor) {
        let info = self.view_zone_info.borrow_mut().take();
        if let Some(info) = info {
            accessor.remove_zone(&info.view_zone_id);

            if !self.has_been_accepted.get() {
                self.keep_cursor_stable(info.line_number, -info.height_in_lines);
            }

            self.view_zone_height.set(None, None);
        }
    }

    /// `keepCursorStable(lineNumber, heightInLines)`.
    fn keep_cursor_stable(&self, line_number: i32, height_in_lines: i32) {
        if !self.should_keep_cursor_stable {
            return;
        }

        let cursor_line_number = self.editor.get_selection().map(|selection| selection.start_line_number);
        if let Some(cursor_line_number) = cursor_line_number
            && line_number < cursor_line_number
        {
            let line_height = *self.editor.get_option(LINE_HEIGHT);
            self.editor.set_scroll_top(self.editor.get_scroll_top() + f64::from(height_in_lines) * line_height, None);
        }
    }
}

/// `AdditionalLinesWidget`: a view zone com as linhas adicionais do texto fantasma.
pub struct AdditionalLinesWidget {
    base: Rc<Disposable>,
    inner: Rc<AdditionalLinesInner>,
    on_did_click: Rc<Emitter<MouseEvent>>,
    pub is_hovered: IObservable<bool>,
}

impl AdditionalLinesWidget {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        lines: IObservable<Option<Rc<AdditionalLinesData>>>,
        should_keep_cursor_stable: bool,
        is_clickable: bool,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let view_zone_height = observable_value_opts::<Option<f64>, ()>(
            ObservableValueOptions { debug: DebugNameOptions::named("viewZoneHeight"), equals_fn: None, lazy: false },
            None,
        );
        let watched_options = [
            EditorOption::DisableMonospaceOptimizations,
            EditorOption::StopRenderingLineAfter,
            EditorOption::RenderWhitespace,
            EditorOption::RenderControlCharacters,
            EditorOption::FontLigatures,
            EditorOption::FontInfo,
            EditorOption::LineHeight,
            EditorOption::EffectiveFullwidthCharacterWidth,
        ];
        let editor_options_changed = observable_signal_from_event(
            "editorOptionChanged",
            editor.on_did_change_configuration().filter(move |e| watched_options.iter().any(|option| e.has_changed(*option)), None),
        );
        let on_did_click = Emitter::<MouseEvent>::new();
        let inner = Rc::new(AdditionalLinesInner {
            editor: Rc::clone(&editor),
            view_zone_info: RefCell::new(None),
            view_zone_height,
            on_did_click: Rc::clone(&on_did_click),
            has_been_accepted: Cell::new(false),
            should_keep_cursor_stable,
            is_clickable,
        });

        let hover_store = DisposableStore::new();
        base.register(Rc::clone(&hover_store));
        let is_hovered = observable_code_editor(&editor).is_target_hovered(
            {
                let inner = Rc::clone(&inner);
                move |p| is_target_ghost_text(&inner, &p.target)
            },
            &hover_store,
        );

        if should_keep_cursor_stable {
            let inner = Rc::clone(&inner);
            base.register(editor.on_before_execute_edit().listen(move |e| {
                inner.has_been_accepted.set(e.source.as_deref() == Some("inlineCompletionAccept"));
            }));
        }

        let autorun_inner = Rc::clone(&inner);
        base.register(autorun(move |reader| {
            // @description update view zone
            let lines = lines.read(reader);
            editor_options_changed.read(reader);

            if let Some(lines) = lines {
                autorun_inner.has_been_accepted.set(false);
                autorun_inner.update_lines(lines.line_number, &lines.additional_lines, lines.min_reserved_line_count);
            } else {
                autorun_inner.clear();
            }
        }));

        Rc::new(Self { base, inner, on_did_click, is_hovered })
    }

    /// `viewZoneId`.
    pub fn view_zone_id(&self) -> Option<String> {
        self.inner.view_zone_info.borrow().as_ref().map(|info| info.view_zone_id.clone())
    }

    /// `viewZoneHeight`.
    pub fn view_zone_height(&self) -> IObservable<Option<f64>> {
        self.inner.view_zone_height.to_observable()
    }

    /// `onDidClick`.
    pub fn on_did_click(&self) -> Event<MouseEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for AdditionalLinesWidget {
    fn dispose(&self) {
        self.base.dispose();
        self.inner.clear();
    }
}

/// `isTargetGhostText(target)` para o alvo do mouse: a view zone das linhas adicionais do widget.
fn is_target_ghost_text(inner: &AdditionalLinesInner, target: &MouseTarget) -> bool {
    match target {
        MouseTarget::ViewZone(zone) => {
            inner.view_zone_info.borrow().as_ref().is_some_and(|info| info.view_zone_id == zone.detail.view_zone_id)
        }
        _ => false,
    }
}

/// O que o `GhostTextView` aceita como opções do construtor.
#[derive(Default)]
pub struct GhostTextViewOptions {
    pub extra_classes: Vec<String>,
    pub is_clickable: bool,
    pub should_keep_cursor_stable: bool,
    pub min_reserved_line_count: Option<IObservable<i32>>,
    pub use_syntax_highlighting: Option<IObservable<bool>>,
    pub highlight_short_suggestions: bool,
}

/// O `{ lineNumber, position, icon }` de `_warningState`.
struct WarningState {
    line_number: i32,
    position: Position,
    icon: ThemeIcon,
}

/// O widget de conteúdo do aviso (só existe com `USE_SQUIGGLES_FOR_WARNING` falso): posiciona o ícone no fim da
/// linha; quem desenha lê `range` (o `ghostTextViewWarningWidgetData`) e `icon`.
pub struct GhostTextWarningWidget {
    position: IObservable<Option<ContentWidgetPosition>>,
    pub range: Range,
    pub icon: ThemeIcon,
}

impl IObservableContentWidget for GhostTextWarningWidget {
    fn position(&self) -> IObservable<Option<ContentWidgetPosition>> {
        Rc::clone(&self.position)
    }

    fn allow_editor_overflow(&self) -> bool {
        false
    }
}

/// `{ replacedRange, inlineTexts, additionalLines, hiddenRange, lineNumber, ... }` de `_state`.
struct GhostTextViewState {
    replaced_range: Option<ColumnRange>,
    inline_texts: Vec<InlineTextWithTokens>,
    additional_lines: Rc<Vec<LineData>>,
    hidden_range: Option<ColumnRange>,
    line_number: i32,
    additional_reserved_line_count: i32,
    telemetry_view_data: InlineCompletionViewData,
    data: Rc<dyn IGhostTextWidgetData>,
}

/// Um `inlineTexts` com os tokens do trecho, quando há realce de sintaxe.
struct InlineTextWithTokens {
    text: InlineText,
    tokens: Option<TokenArray>,
}

/// `allText.replace(/\s/g, '')`: o `\s` do JavaScript.
fn is_js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

/// `equalsFn: equalsIfDefinedC(thisEqualsC())` do dado do widget das linhas adicionais.
fn optional_lines_data_equals() -> EqualityComparer<Option<Rc<AdditionalLinesData>>> {
    Rc::new(|a, b| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.equals(b),
        _ => false,
    })
}

/// `{ owner: this, debugName }` sem dono.
fn named_options<T>(name: &str, equals_fn: Option<EqualityComparer<T>>) -> DerivedOptions<T> {
    DerivedOptions { owner: None, debug_name: Some(DebugNameSource::from(name)), equals_fn, on_last_observer_removed: None }
}

/// `GhostTextView`.
pub struct GhostTextView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<MouseEvent>>,
    additional_lines_widget: Rc<AdditionalLinesWidget>,
    pub is_hovered: IObservable<bool>,
    pub height: IObservable<f64>,
}

impl GhostTextView {
    pub fn new(
        editor: Rc<CodeEditorWidget>,
        data: IObservable<Option<Rc<dyn IGhostTextWidgetData>>>,
        options: GhostTextViewOptions,
        language_id_codec: Rc<dyn ILanguageIdCodec>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let owner = Rc::new(OwnerToken);
        let on_did_click = Emitter::<MouseEvent>::new();

        let is_disposed = observable_value_opts::<bool, ()>(
            ObservableValueOptions { debug: DebugNameOptions::named("isDisposed"), equals_fn: None, lazy: false },
            false,
        );

        let extra_classes = options.extra_classes;
        let is_clickable = options.is_clickable;
        let should_keep_cursor_stable = options.should_keep_cursor_stable;
        let min_reserved_line_count: IObservable<i32> = options.min_reserved_line_count.unwrap_or_else(|| const_observable(0));
        let use_syntax_highlighting: IObservable<bool> = options.use_syntax_highlighting.unwrap_or_else(|| const_observable(true));
        let highlight_short_text = options.highlight_short_suggestions;

        let editor_obs = observable_code_editor(&editor);

        let warning_state: IObservable<Option<Rc<WarningState>>> = {
            let data = Rc::clone(&data);
            derived(move |reader| {
                let model = data.read(reader)?;
                let warning = model.warning()?;
                let gt = model.ghost_text();
                let column = gt.parts().first().map_or(1, |part| part.column);
                Some(Rc::new(WarningState {
                    line_number: gt.line_number(),
                    position: Position::new(gt.line_number(), column),
                    icon: warning.icon.clone(),
                }))
            })
        };

        let non_whitespace_count: IObservable<Option<i32>> = {
            let data = Rc::clone(&data);
            derived(move |reader| {
                let data = data.read(reader)?;
                let count = data
                    .ghost_text()
                    .parts()
                    .iter()
                    .flat_map(|part| part.lines.iter())
                    .flat_map(|line| line.line.units().iter().copied())
                    .filter(|unit| !is_js_whitespace(*unit))
                    .count();
                Some(count as i32)
            })
        };

        let extra_class_names: IObservable<String> = {
            let (warning_state, non_whitespace_count, use_syntax_highlighting) =
                (Rc::clone(&warning_state), Rc::clone(&non_whitespace_count), Rc::clone(&use_syntax_highlighting));
            derived(move |reader| {
                let mut extra_classes = extra_classes.clone();
                if USE_SQUIGGLES_FOR_WARNING && warning_state.read(reader).is_some() {
                    extra_classes.push("warning".to_owned());
                }
                let non_whitespace_count = non_whitespace_count.read(reader);
                if highlight_short_text && non_whitespace_count.is_some_and(|count| count != 0 && count < 3) {
                    extra_classes.push("short-text".to_owned());
                } else if use_syntax_highlighting.read(reader) {
                    extra_classes.push("syntax-highlighted".to_owned());
                }
                extra_classes.iter().map(|class| format!(" {class}")).collect::<String>()
            })
        };

        let state: IObservable<Option<Rc<GhostTextViewState>>> = {
            let (is_disposed, data, editor_obs, use_syntax_highlighting, extra_class_names, min_reserved_line_count, editor) = (
                Rc::clone(&is_disposed),
                Rc::clone(&data),
                Rc::clone(&editor_obs),
                Rc::clone(&use_syntax_highlighting),
                Rc::clone(&extra_class_names),
                Rc::clone(&min_reserved_line_count),
                Rc::clone(&editor),
            );
            derived(move |reader| {
                if is_disposed.read(reader) {
                    return None;
                }

                let props = data.read(reader)?;
                let text_model = editor_obs.model.read(reader)?;

                let ghost_text = props.ghost_text();
                let replaced_range = match ghost_text {
                    GhostTextOrReplacement::Replacement(replacement) => Some(replacement.column_range),
                    GhostTextOrReplacement::GhostText(_) => None,
                };

                let syntax_highlighting_enabled = use_syntax_highlighting.read(reader);
                let extra_class_names = extra_class_names.read(reader);
                let GhostTextViewData { inline_texts, additional_lines, hidden_range, additional_lines_original_suffix } =
                    compute_ghost_text_view_data(ghost_text, &text_model, &format!("{GHOST_TEXT_CLASS_NAME}{extra_class_names}"));

                let line_number = ghost_text.line_number();
                let current_line = CursorSimpleModel::get_line_content(&*text_model, line_number);
                let edit = StringEdit::new(inline_texts.iter().map(|t| StringReplacement::insert(t.column - 1, t.text.clone())).collect());
                let tokens: Option<Vec<LineTokens>> = syntax_highlighting_enabled.then(|| {
                    let mut lines = vec![edit.apply(current_line.units())];
                    lines.extend(additional_lines.iter().map(|l| l.content.clone()));
                    text_model.tokenization().tokenize_lines_at(line_number, &lines)
                })
                .flatten();
                let new_ranges = edit.get_new_ranges();
                let inline_texts_with_tokens: Vec<InlineTextWithTokens> = inline_texts
                    .into_iter()
                    .enumerate()
                    .map(|(idx, text)| {
                        let tokens = tokens.as_ref().and_then(|tokens| tokens.first()).map(|line| line.get_tokens_in_range(new_ranges[idx]));
                        InlineTextWithTokens { text, tokens }
                    })
                    .collect();

                let last_additional_line = additional_lines.len().checked_sub(1);
                let tokenized_additional_lines: Vec<LineData> = additional_lines
                    .iter()
                    .enumerate()
                    .map(|(idx, line)| {
                        let mut content = tokens
                            .as_ref()
                            .and_then(|tokens| tokens.get(idx + 1))
                            .cloned()
                            .unwrap_or_else(|| LineTokens::create_empty(line.content.clone(), Rc::clone(&language_id_codec)));
                        if Some(idx) == last_additional_line
                            && let Some(suffix) = &additional_lines_original_suffix
                        {
                            let existing = text_model.tokenization().get_line_tokens(suffix.line_number);
                            let range = suffix.column_range.to_zero_based_offset_range();
                            let existing_tokens = TokenArray::from_line_tokens(&existing).slice(range);
                            let existing_text = existing.get_line_content_ref().slice(range.start as usize, range.end_exclusive as usize);
                            let mut text = content.get_line_content_ref().clone();
                            text.push_units(existing_text.units());
                            content = TokenArray::from_line_tokens(&content).append(&existing_tokens).to_line_tokens(&text, Rc::clone(&language_id_codec));
                        }
                        LineData::new(content, line.decorations.clone())
                    })
                    .collect();

                let cursor_column = editor.get_selection().map_or(0, |selection| selection.start_column);
                let disjoint_inline_texts: Vec<&InlineTextWithTokens> =
                    inline_texts_with_tokens.iter().filter(|inline| !inline.text.text.is_empty()).collect();
                let has_insertion_on_current_line = !disjoint_inline_texts.is_empty();
                let telemetry_view_data = InlineCompletionViewData::new(
                    (if has_insertion_on_current_line { disjoint_inline_texts[0].text.column } else { 1 }) - cursor_column,
                    if has_insertion_on_current_line {
                        0
                    } else {
                        additional_lines.iter().position(|line| !line.content.is_empty()).map_or(0, |index| index as i32 + 1)
                    },
                    i32::from(has_insertion_on_current_line),
                    additional_lines.len() as i32 + i32::from(has_insertion_on_current_line),
                    0,
                    disjoint_inline_texts.iter().map(|inline| inline.text.text.len() as i32).sum::<i32>()
                        + tokenized_additional_lines.iter().map(|line| line.content.get_text_length() as i32).sum::<i32>(),
                    disjoint_inline_texts.len() as i32 + i32::from(!additional_lines.is_empty()),
                    (disjoint_inline_texts.len() > 1 && tokenized_additional_lines.is_empty())
                        .then(|| disjoint_inline_texts.iter().all(|inline| inline.text.text == disjoint_inline_texts[0].text.text)),
                );
                drop(disjoint_inline_texts);

                Some(Rc::new(GhostTextViewState {
                    replaced_range,
                    inline_texts: inline_texts_with_tokens,
                    additional_lines: Rc::new(tokenized_additional_lines),
                    hidden_range,
                    line_number,
                    additional_reserved_line_count: min_reserved_line_count.read(reader),
                    telemetry_view_data,
                    data: props,
                }))
            })
        };

        let additional_lines_widget = {
            let state = Rc::clone(&state);
            let lines = derived_opts(named_options("lines", Some(optional_lines_data_equals())), move |reader| {
                // @description lines
                state.read(reader).map(|ui_state| {
                    Rc::new(AdditionalLinesData {
                        line_number: ui_state.line_number,
                        additional_lines: Rc::clone(&ui_state.additional_lines),
                        min_reserved_line_count: ui_state.additional_reserved_line_count,
                    })
                })
            });
            AdditionalLinesWidget::new(Rc::clone(&editor), lines, should_keep_cursor_stable, is_clickable)
        };
        base.register(Rc::clone(&additional_lines_widget) as DisposableRef);

        let hover_store = DisposableStore::new();
        base.register(Rc::clone(&hover_store));
        let is_inline_text_hovered = {
            let owner = Rc::clone(&owner);
            editor_obs.is_target_hovered(
                move |p| match &p.target {
                    MouseTarget::ContentText(target) => target
                        .detail
                        .injected_text
                        .as_ref()
                        .is_some_and(|injected| GhostTextAttachedData::is_owned_by(&injected.options.attached_data, &owner)),
                    _ => false,
                },
                &hover_store,
            )
        };

        base.register(to_disposable({
            let is_disposed = Rc::clone(&is_disposed);
            move || is_disposed.set(true, None)
        }));

        let decorations: IObservable<Rc<Vec<ModelDeltaDecoration>>> = {
            let (state, extra_class_names, owner) = (Rc::clone(&state), Rc::clone(&extra_class_names), Rc::clone(&owner));
            derived(move |reader| {
                let Some(ui_state) = state.read(reader) else {
                    return Rc::new(Vec::new());
                };

                let mut decorations: Vec<ModelDeltaDecoration> = Vec::new();

                let extra_class_names = extra_class_names.read(reader);

                if let Some(replaced_range) = ui_state.replaced_range {
                    decorations.push(ModelDeltaDecoration {
                        range: replaced_range.to_range(ui_state.line_number),
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            inline_class_name: Some(format!("inline-completion-text-to-replace{extra_class_names}")),
                            description: "GhostTextReplacement".to_owned(),
                            ..Default::default()
                        }),
                    });
                }

                if let Some(hidden_range) = ui_state.hidden_range {
                    decorations.push(ModelDeltaDecoration {
                        range: hidden_range.to_range(ui_state.line_number),
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            inline_class_name: Some("ghost-text-hidden".to_owned()),
                            description: "ghost-text-hidden".to_owned(),
                            ..Default::default()
                        }),
                    });
                }

                for p in &ui_state.inline_texts {
                    let mut inline_extra_class_names = String::new();
                    if highlight_short_text && p.text.text.len() < 5 {
                        inline_extra_class_names.push_str(" short-text");
                    }
                    // Como no original, os intervalos das decorações de linha não são considerados.
                    let line_decoration_classes = p
                        .text
                        .line_decorations
                        .iter()
                        .map(|d| format!(" {}", d.class_name.to_string_lossy()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let inline_class_name = format!(
                        "{}{}{}{}{}",
                        if p.text.preview { "ghost-text-decoration-preview" } else { "ghost-text-decoration" },
                        if is_clickable { " clickable" } else { "" },
                        extra_class_names,
                        inline_extra_class_names,
                        line_decoration_classes,
                    );
                    let position = Position::new(ui_state.line_number, p.text.column);
                    decorations.push(ModelDeltaDecoration {
                        range: Range::from_positions(position, position),
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            description: "ghost-text-decoration".to_owned(),
                            after: Some(InjectedTextOptions {
                                content: p.text.text.to_string_lossy(),
                                tokens: p.tokens.clone(),
                                inline_class_name: Some(inline_class_name),
                                cursor_stops: Some(InjectedTextCursorStops::Left),
                                attached_data: InjectedTextAttachedData::new(Rc::new(GhostTextAttachedData { owner: Rc::clone(&owner) })),
                                ..InjectedTextOptions::default()
                            }),
                            show_if_collapsed: true,
                            ..Default::default()
                        }),
                    });
                }

                Rc::new(decorations)
            })
        };
        base.register(editor_obs.set_decorations(&decorations));

        if is_clickable {
            let on_did_click_from_lines = Rc::clone(&on_did_click);
            base.register(additional_lines_widget.on_did_click().listen(move |e| on_did_click_from_lines.fire(e.clone())));

            let (on_did_click_from_text, owner) = (Rc::clone(&on_did_click), Rc::clone(&owner));
            base.register(editor.on_mouse_up().listen(move |e| {
                let MouseTarget::ContentText(target) = &e.target else {
                    return;
                };
                if target
                    .detail
                    .injected_text
                    .as_ref()
                    .is_some_and(|injected| GhostTextAttachedData::is_owned_by(&injected.options.attached_data, &owner))
                {
                    on_did_click_from_text.fire(e.event.clone());
                }
            }));
        }

        base.register({
            let state = Rc::clone(&state);
            autorun(move |reader| {
                if let Some(state) = state.read(reader) {
                    state.data.handle_inline_completion_shown(&state.telemetry_view_data);
                }
            })
        });

        base.register({
            let (warning_state, editor_obs) = (Rc::clone(&warning_state), Rc::clone(&editor_obs));
            autorun_with_store(move |reader, store| {
                if USE_SQUIGGLES_FOR_WARNING {
                    return;
                }

                let Some(state) = warning_state.read(reader) else {
                    return;
                };

                let position = ContentWidgetPosition {
                    position: Some(Position::new(state.line_number, i32::MAX)),
                    secondary_position: None,
                    preference: vec![ContentWidgetPositionPreference::Exact],
                    position_affinity: Some(PositionAffinity::Right),
                };
                store.add(editor_obs.create_content_widget(Rc::new(GhostTextWarningWidget {
                    position: const_observable(Some(position)),
                    range: Range::from_positions(state.position, state.position),
                    icon: state.icon.clone(),
                })));
            })
        });

        let is_hovered: IObservable<bool> = {
            let (is_disposed, additional_lines_widget) = (Rc::clone(&is_disposed), Rc::clone(&additional_lines_widget));
            derived(move |reader| {
                if is_disposed.read(reader) {
                    return false;
                }
                is_inline_text_hovered.read(reader) || additional_lines_widget.is_hovered.read(reader)
            })
        };

        let height: IObservable<f64> = {
            let (additional_lines_widget, editor_obs) = (Rc::clone(&additional_lines_widget), Rc::clone(&editor_obs));
            derived(move |reader| {
                let line_height = *editor_obs.get_option(LINE_HEIGHT).read(reader);
                line_height + additional_lines_widget.view_zone_height().read(reader).unwrap_or(0.0)
            })
        };

        Rc::new(Self { base, on_did_click, additional_lines_widget, is_hovered, height })
    }

    /// `onDidClick`.
    pub fn on_did_click(&self) -> Event<MouseEvent> {
        self.on_did_click.event()
    }

    /// `ownsViewZone(viewZoneId)`.
    pub fn owns_view_zone(&self, view_zone_id: &str) -> bool {
        self.additional_lines_widget.view_zone_id().as_deref() == Some(view_zone_id)
    }
}

impl IDisposable for GhostTextView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
