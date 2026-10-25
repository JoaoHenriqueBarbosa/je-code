//! Porte de `.../inlineEditsViews/longDistanceHint/longDistancePreviewEditor.ts`: `LongDistancePreviewEditor`, o editor de
//! leitura embutido que mostra a linha de destino de uma edição de longa distância (as linhas originais ou as novas, com o
//! diff pintado e o indicador da margem), e `ILongDistancePreviewProps`.
//!
//! Notas de tradução:
//! - o `n.div({ class: 'preview', ref })` (o `element` que hospeda o editor) é o próprio editor embutido
//!   ([`LongDistancePreviewEditor::preview_editor`]): o contêiner nativo é de quem o pinta, e a fábrica
//!   ([`IEmbeddedCodeEditorFactory`]) cria o editor com as opções cruas, como na view lado a lado. O
//!   `@IInstantiationService` do construtor é a fábrica e os serviços do [`LongDistanceHintServices`];
//! - `ModelDecorationOptions.register({...})` (opções registradas, compartilhadas) são as opções criadas a cada uso com
//!   `ModelDecorationOptions::create_dynamic`, com os mesmos campos;
//! - `TextModelValueReference.dangerouslyGetUnderlyingModel()` é `dangerously_get_underlying_model`; o
//!   `setModel(tm.dangerouslyGetUnderlyingModel())` roda só quando há modelo, para evitar a troca `tm -> null -> tm`;
//! - `Math.min`, `Math.max` e `clamp` de `_getHorizontalContentRangeInPreviewEditorToShow` são os de `f64`, e o `OffsetRange`
//!   do crate é inteiro: a faixa a revelar (`preferredRangeToReveal`) arredonda as bordas para o pixel inteiro mais próximo;
//! - `showJumpToDecoration` é a constante `false` do original: o ramo que criava a `JumpToView` nunca roda e não aparece;
//! - `previewEditor.setPosition(pos, 'longDistanceHintPreview')` leva a fonte da mudança como texto;
//! - `lineNumbersMinChars` é atualizado pelo `updateOptions` do editor embutido, que mistura a opção nas que valem por
//!   cima das do editor pai.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{FlattenExt, IObservable, ObservableExt, autorun, const_observable, derived, observable_value};

use crate::code_editor_widget::{CodeEditorWidget, CodeEditorWidgetOptions, ICodeEditor};
use crate::config::editor_options::inline_suggest::INLINE_SUGGEST;
use crate::core_2d::dimension::Dimension;
use crate::cursor_common::CursorSimpleModel;
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::embedded_code_editor_widget::EmbeddedCodeEditorWidget;
use crate::inline_completions::controller::inline_completion_context_keys::inline_completion_context_keys;
use crate::inline_completions::text_model_value_reference::TextModelValueReference;
use crate::line_range::LineRange;
use crate::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::observable_code_editor::{ValueOrObservable, observable_code_editor};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};

use super::super::super::components::gutter_indicator_view::{
    GutterIndicatorServices, InlineEditsGutterIndicator, InlineEditsGutterIndicatorData, InlineSuggestionGutterMenuData, SimpleInlineSuggestModel,
};
use super::super::super::inline_edits_view_interface::InlineEditTabAction;
use super::super::super::utils::utils::{class_names, max_content_width_in_range};
use super::LongDistanceHintServices;

/// `ILongDistancePreviewProps`.
pub struct LongDistancePreviewProps {
    /// assert: nextCursorPosition !== null  xor  diff.length > 0
    pub next_cursor_position: Option<Position>,
    pub diff: Vec<DetailedLineRangeMapping>,
    pub model: Rc<SimpleInlineSuggestModel>,
    pub inline_suggest_info: Rc<InlineSuggestionGutterMenuData>,
    /// The URI of the file the edit targets. When it is the editor's model, the edit targets the current file.
    pub target: Rc<TextModelValueReference>,
}

/// `'original' | 'modified'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewMode {
    Original,
    Modified,
}

/// O `_state` do original.
pub struct PreviewState {
    pub mode: PreviewMode,
    pub target_line_number: i32,
    pub visible_line_range: LineRange,
    pub text_model: Rc<TextModelValueReference>,
    pub diff: Vec<DetailedLineRangeMapping>,
}

/// O resultado de `_getHorizontalContentRangeInPreviewEditorToShow`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizontalContentRange {
    pub indentation_end: f64,
    pub preferred_range_to_reveal: OffsetRange,
    pub max_editor_width: f64,
    pub content_width: f64,
    /// Width of area that is not content.
    pub non_content_width: f64,
}

/// O `_editorDecorations` do original: as decorações do lado original e do lado modificado.
struct EditorDecorations {
    original: Rc<Vec<ModelDeltaDecoration>>,
    modified: Rc<Vec<ModelDeltaDecoration>>,
}

/// `hideEmptyInnerDecorations` (`diff.mode === 'lineReplacement'` está comentado no original).
const HIDE_EMPTY_INNER_DECORATIONS: bool = true;

/// Widens the previewed range around `targetLineNumber` by `contextLineCount` lines on each side,
/// clamped to the model bounds. With `contextLineCount === 0` this returns just the target line.
fn expand_line_range_with_context(target_line_number: i32, context_line_count: i32, line_count: i32) -> LineRange {
    let clamped_target = i32::max(1, line_count.min(target_line_number));
    let start_line_number = i32::max(1, clamped_target - context_line_count);
    let end_line_number_exclusive = (line_count + 1).min(clamped_target + context_line_count + 1);
    LineRange::new(start_line_number, end_line_number_exclusive)
}

/// As opções cruas do editor de prévia (o terceiro argumento do `EmbeddedCodeEditorWidget`).
fn preview_editor_options() -> Dynamic {
    Dynamic::object([
        ("glyphMargin", Dynamic::from(false)),
        ("lineNumbers", Dynamic::from("on")),
        ("minimap", Dynamic::object([("enabled", Dynamic::from(false))])),
        (
            "guides",
            Dynamic::object([
                ("indentation", Dynamic::from(false)),
                ("bracketPairs", Dynamic::from(false)),
                ("bracketPairsHorizontal", Dynamic::from(false)),
                ("highlightActiveIndentation", Dynamic::from(false)),
            ]),
        ),
        ("editContext", Dynamic::from(false)), // is a bit faster
        ("rulers", Dynamic::array([])),
        ("padding", Dynamic::object([("top", Dynamic::from(0)), ("bottom", Dynamic::from(0))])),
        ("selectOnLineNumbers", Dynamic::from(false)),
        ("selectionHighlight", Dynamic::from(false)),
        ("columnSelection", Dynamic::from(false)),
        ("overviewRulerBorder", Dynamic::from(false)),
        ("overviewRulerLanes", Dynamic::from(0)),
        ("revealHorizontalRightPadding", Dynamic::from(0)),
        (
            "bracketPairColorization",
            Dynamic::object([("enabled", Dynamic::from(true)), ("independentColorPoolPerBracketType", Dynamic::from(false))]),
        ),
        ("scrollBeyondLastLine", Dynamic::from(false)),
        (
            "scrollbar",
            Dynamic::object([
                ("vertical", Dynamic::from("hidden")),
                ("horizontal", Dynamic::from("hidden")),
                ("handleMouseWheel", Dynamic::from(false)),
            ]),
        ),
        ("readOnly", Dynamic::from(true)),
        ("wordWrap", Dynamic::from("off")),
        ("wordWrapOverride1", Dynamic::from("off")),
        ("wordWrapOverride2", Dynamic::from("off")),
    ])
}

/// `(/[a-zA-Z0-9_]/).test(char)` de `isVariableNameCharacter`.
fn is_variable_name_character(unit: Option<u16>) -> bool {
    unit.is_some_and(|unit| matches!(unit, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F))
}

/// `char === ' ' || char === '\t'` de `isWhitespace`.
fn is_space_or_tab(unit: Option<u16>) -> bool {
    matches!(unit, Some(0x20 | 0x09))
}

/// Grows the range on each ends until it includes a none-variable-name character
/// or the next character would be a whitespace character
/// or the maxGrow limit is reached.
fn grow_until_variable_boundaries(text_model: &TextModel, range: Range, max_grow: i32) -> Range {
    let start_position = range.get_start_position();
    let end_position = range.get_end_position();
    let line = text_model.get_line_content(start_position.line_number);
    // `line.charAt(col - 1)`: fora da linha é o texto vazio, que nunca casa.
    let unit_at = |column: i32| je_base::js_string::char_code_at(line.units(), i64::from(column) - 1);

    let mut start_column = start_position.column;
    while start_column > 1
        && is_variable_name_character(unit_at(start_column))
        && !is_space_or_tab(unit_at(start_column - 1))
        && start_position.column - start_column < max_grow
    {
        start_column -= 1;
    }

    let mut end_column = end_position.column - 1;
    while end_column <= line.len() as i32
        && is_variable_name_character(unit_at(end_column))
        && !is_space_or_tab(unit_at(end_column + 1))
        && end_column - end_position.column < max_grow
    {
        end_column += 1;
    }

    Range::new(start_position.line_number, start_column, end_position.line_number, end_column + 1)
}

/// `clamp(value, min, max)` de `base/common/numbers`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

/// `LongDistancePreviewEditor`.
pub struct LongDistancePreviewEditor {
    base: Rc<Disposable>,
    preview_editor: Rc<EmbeddedCodeEditorWidget>,
    preview_editor_obs: Rc<crate::observable_code_editor::ObservableCodeEditor>,
    horizontal_content_range_in_preview_editor_to_show: IObservable<Option<Rc<HorizontalContentRange>>>,
    content_height: IObservable<Option<f64>>,
}

impl LongDistancePreviewEditor {
    pub fn new(
        preview_text_model: Rc<TextModel>,
        properties: IObservable<Option<Rc<LongDistancePreviewProps>>>,
        parent_editor: Rc<CodeEditorWidget>,
        tab_action: IObservable<InlineEditTabAction>,
        services: &LongDistanceHintServices,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let parent_editor_obs = observable_code_editor(&parent_editor);

        let state: IObservable<Option<Rc<PreviewState>>> = {
            let (properties, preview_text_model, parent_editor_obs) =
                (Rc::clone(&properties), Rc::clone(&preview_text_model), Rc::clone(&parent_editor_obs));
            let inline_suggest = parent_editor_obs.get_option(INLINE_SUGGEST);
            derived(move |reader| {
                let props = properties.read(reader)?;

                let (mode, target_line_number) = if let Some(next_cursor_position) = props.next_cursor_position {
                    (PreviewMode::Original, next_cursor_position.line_number)
                } else {
                    let first = props.diff.first().expect("sem posição de salto o diff não é vazio");
                    if first.inner_changes.as_ref().is_some_and(|inner| inner.iter().all(|c| c.modified_range.is_empty())) {
                        (PreviewMode::Original, first.original.start_line_number)
                    } else {
                        (PreviewMode::Modified, first.modified.start_line_number)
                    }
                };

                let text_model = match mode {
                    PreviewMode::Modified => TextModelValueReference::snapshot(&preview_text_model),
                    PreviewMode::Original => Rc::clone(&props.target),
                };

                // Optionally widen the previewed range with surrounding context lines (e.g. when the target line is empty).
                let context_line_count = inline_suggest.read(reader).edits.long_distance_hint_context_line_count as i32;
                let display_line_count = match mode {
                    PreviewMode::Modified => CursorSimpleModel::get_line_count(&*preview_text_model),
                    PreviewMode::Original => CursorSimpleModel::get_line_count(&**props.target.dangerously_get_underlying_model()),
                };
                let visible_line_range = expand_line_range_with_context(target_line_number, context_line_count, display_line_count);

                Some(Rc::new(PreviewState { mode, target_line_number, visible_line_range, text_model, diff: props.diff.clone() }))
            })
        };

        let preview_editor = EmbeddedCodeEditorWidget::new(
            preview_editor_options(),
            CodeEditorWidgetOptions { contributions: Some(Vec::new()), telemetry_data: None },
            vec![(inline_completion_context_keys().in_inline_edits_preview_editor.key().to_owned(), Dynamic::from(true))],
            Rc::clone(&parent_editor),
            &*services.embedded_code_editor_factory,
        );
        base.register(preview_editor.clone());

        base.register(autorun({
            let (state, preview_editor) = (Rc::clone(&state), Rc::clone(&preview_editor));
            move |reader| {
                let text_model = state.read(reader).map(|state| Rc::clone(state.text_model.dangerously_get_underlying_model()));

                if let Some(text_model) = text_model {
                    // Avoid transitions from tm -> null -> tm, where tm -> tm would be a no-op.
                    preview_editor.editor().set_model(Some(text_model));
                }
            }
        }));

        let preview_editor_obs = observable_code_editor(preview_editor.editor());

        let editor_decorations: IObservable<Option<Rc<EditorDecorations>>> = {
            let state = Rc::clone(&state);
            derived(move |reader| {
                let state = state.read(reader)?;

                let mut original: Vec<ModelDeltaDecoration> = Vec::new();
                let mut modified: Vec<ModelDeltaDecoration> = Vec::new();

                let whole_line_delete_decoration = || {
                    ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                        class_name: Some("inlineCompletions-char-delete".to_owned()),
                        description: "char-delete".to_owned(),
                        is_whole_line: false,
                        z_index: Some(1), // be on top of diff background decoration
                        ..Default::default()
                    })
                };
                let whole_line_add_decoration = || {
                    ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                        class_name: Some("inlineCompletions-char-insert".to_owned()),
                        description: "char-insert".to_owned(),
                        is_whole_line: true,
                        ..Default::default()
                    })
                };
                let add_decoration = || {
                    ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                        class_name: Some("inlineCompletions-char-insert".to_owned()),
                        description: "char-insert".to_owned(),
                        should_fill_line_on_line_break: Some(true),
                        ..Default::default()
                    })
                };

                for m in &state.diff {
                    if m.modified.is_empty() || m.original.is_empty() {
                        if let Some(range) = m.original.to_inclusive_range() {
                            original.push(ModelDeltaDecoration { range, options: whole_line_delete_decoration() });
                        }
                        if let Some(range) = m.modified.to_inclusive_range() {
                            modified.push(ModelDeltaDecoration { range, options: whole_line_add_decoration() });
                        }
                    } else {
                        for i in m.inner_changes.iter().flatten() {
                            // Don't show empty markers outside the line range
                            if m.original.contains(i.original_range.start_line_number) && !(HIDE_EMPTY_INNER_DECORATIONS && i.original_range.is_empty()) {
                                original.push(ModelDeltaDecoration {
                                    range: i.original_range,
                                    options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                                        description: "char-delete".to_owned(),
                                        should_fill_line_on_line_break: Some(false),
                                        class_name: Some(class_names(&[
                                            Some("inlineCompletions-char-delete"),
                                            i.original_range.is_empty().then_some("empty"),
                                        ])),
                                        z_index: Some(1),
                                        ..Default::default()
                                    }),
                                });
                            }
                            if m.modified.contains(i.modified_range.start_line_number) {
                                modified.push(ModelDeltaDecoration { range: i.modified_range, options: add_decoration() });
                            }
                        }
                    }
                }

                Some(Rc::new(EditorDecorations { original: Rc::new(original), modified: Rc::new(modified) }))
            })
        };

        base.register(preview_editor_obs.set_decorations(&{
            let state = Rc::clone(&state);
            derived(move |reader| -> Rc<Vec<ModelDeltaDecoration>> {
                let state = state.read(reader);
                let decorations = editor_decorations.read(reader);
                match (state, decorations) {
                    (Some(state), Some(decorations)) => {
                        Rc::clone(if state.mode == PreviewMode::Original { &decorations.original } else { &decorations.modified })
                    }
                    _ => Rc::new(Vec::new()),
                }
            })
        }));

        // Mirror the cursor position. Allows the gutter arrow to point in the correct direction.
        base.register(autorun({
            let (properties, parent_editor_obs, preview_editor, preview_text_model) =
                (Rc::clone(&properties), Rc::clone(&parent_editor_obs), Rc::clone(&preview_editor), Rc::clone(&preview_text_model));
            move |reader| {
                if properties.read(reader).is_none() {
                    return;
                }
                let cursor_position = parent_editor_obs.cursor_position.read(reader);
                if let Some(cursor_position) = cursor_position {
                    preview_editor.editor().set_position(preview_text_model.validate_position(cursor_position), Some("longDistanceHintPreview"));
                }
            }
        }));

        base.register(autorun({
            let (state, preview_editor) = (Rc::clone(&state), Rc::clone(&preview_editor));
            move |reader| {
                let Some(state) = state.read(reader) else {
                    return;
                };
                // Ensure there is enough space to the left of the line number for the gutter indicator to fits.
                let line_number_digits = (state.visible_line_range.end_line_number_exclusive - 1).to_string().len() as i32;
                preview_editor.update_options(&Dynamic::object([("lineNumbersMinChars", Dynamic::from(line_number_digits + 1))]));
            }
        }));

        let indicator = InlineEditsGutterIndicator::new(
            Rc::clone(&preview_editor_obs),
            {
                let (state, properties) = (Rc::clone(&state), Rc::clone(&properties));
                derived(move |reader| {
                    let state = state.read(reader)?;
                    let props = properties.read(reader)?;
                    Some(Rc::new(InlineEditsGutterIndicatorData::new(
                        Rc::clone(&props.inline_suggest_info),
                        LineRange::of_length(state.target_line_number, 1),
                        SimpleInlineSuggestModel { accept: Rc::clone(&props.model.accept), jump: Rc::clone(&props.model.jump) },
                        None,
                        None,
                    )))
                })
            },
            tab_action,
            const_observable(0.0),
            const_observable(false),
            observable_value::<bool, ()>("focusIsInMenu", false),
            GutterIndicatorServices {
                theme_service: Rc::clone(&services.theme_service),
                accessibility_service: Rc::clone(&services.accessibility_service),
                host: Rc::clone(&services.gutter_indicator_host),
            },
        );
        base.register(indicator);

        let update_preview_editor_effect: IObservable<()> = {
            let (state, preview_editor_obs, preview_editor, preview_text_model) =
                (Rc::clone(&state), Rc::clone(&preview_editor_obs), Rc::clone(&preview_editor), Rc::clone(&preview_text_model));
            derived(move |reader| {
                preview_editor_obs.model.read(reader); // update when the model is set

                let Some(range) = state.read(reader).map(|state| state.visible_line_range) else {
                    return;
                };
                let line_count = CursorSimpleModel::get_line_count(&*preview_text_model);
                let mut hidden_areas: Vec<Range> = Vec::new();
                if range.start_line_number > 1 {
                    hidden_areas.push(Range::new(1, 1, range.start_line_number - 1, 1));
                }
                if range.end_line_number_exclusive < line_count + 1 {
                    hidden_areas.push(Range::new(range.end_line_number_exclusive, 1, line_count + 1, 1));
                }
                preview_editor.editor().set_hidden_areas(&hidden_areas, None, Some(true));
            })
        };
        update_preview_editor_effect.recompute_initially_and_on_change(base.store(), None);

        let horizontal_content_range_in_preview_editor_to_show: IObservable<Option<Rc<HorizontalContentRange>>> = {
            let (state, properties, preview_editor, preview_editor_obs) =
                (Rc::clone(&state), Rc::clone(&properties), Rc::clone(&preview_editor), Rc::clone(&preview_editor_obs));
            derived(move |reader| {
                let state = state.read(reader)?;

                let diff = &state.diff;
                let jump_to_pos = properties.read(reader).and_then(|props| props.next_cursor_position);

                let visible_range = state.visible_line_range;
                let layout = preview_editor_obs.layout_info.read(reader);
                let true_content_width = max_content_width_in_range(&preview_editor_obs, visible_range, Some(reader));

                let first_character_change: Range = if let Some(jump_to_pos) = jump_to_pos {
                    Range::from_positions(jump_to_pos, jump_to_pos)
                } else if let Some(inner_changes) = diff.first()?.inner_changes.as_ref() {
                    let first = inner_changes.first()?;
                    if state.mode == PreviewMode::Modified { first.modified_range } else { first.original_range }
                } else {
                    return None;
                };

                // find the horizontal range we want to show.
                let text_model = preview_editor.editor().get_model().expect("o editor de prévia tem modelo");
                let preferred_range = grow_until_variable_boundaries(&text_model, first_character_change, 5);
                let left_offset = preview_editor_obs.get_left_of_position(preferred_range.get_start_position(), Some(reader));
                let right_offset = preview_editor_obs.get_left_of_position(preferred_range.get_end_position(), Some(reader));

                let left = clamp(left_offset, 0.0, true_content_width);
                let right = clamp(right_offset, left, true_content_width);

                let indent_col = text_model.get_line_first_non_whitespace_column(preferred_range.start_line_number);
                let indentation_end =
                    preview_editor_obs.get_left_of_position(Position::new(preferred_range.start_line_number, indent_col), Some(reader));

                let preferred_range_to_reveal = OffsetRange::new(left.round() as i32, right.round() as i32);

                Some(Rc::new(HorizontalContentRange {
                    indentation_end,
                    preferred_range_to_reveal,
                    max_editor_width: true_content_width + layout.content_left,
                    content_width: true_content_width,
                    non_content_width: layout.content_left, // Width of area that is not content
                }))
            })
        };

        let content_height: IObservable<Option<f64>> = {
            let (state, preview_editor_obs) = (Rc::clone(&state), Rc::clone(&preview_editor_obs));
            derived(move |reader| -> IObservable<Option<f64>> {
                let Some(view_state) = state.read(reader) else {
                    return const_observable(None);
                };

                preview_editor_obs
                    .observe_line_heights_for_line_range(ValueOrObservable::Value(view_state.visible_line_range))
                    .map(|heights, _| Some(heights.iter().sum::<f64>()))
            })
            .flatten()
        };

        Rc::new(Self { base, preview_editor, preview_editor_obs, horizontal_content_range_in_preview_editor_to_show, content_height })
    }

    /// `previewEditor`: o editor embutido da prévia.
    pub fn preview_editor(&self) -> &Rc<EmbeddedCodeEditorWidget> {
        &self.preview_editor
    }

    /// `horizontalContentRangeInPreviewEditorToShow`.
    pub fn horizontal_content_range_in_preview_editor_to_show(&self) -> IObservable<Option<Rc<HorizontalContentRange>>> {
        Rc::clone(&self.horizontal_content_range_in_preview_editor_to_show)
    }

    /// `contentHeight`: a soma das alturas das linhas visíveis (`None` sem estado).
    pub fn content_height(&self) -> IObservable<Option<f64>> {
        Rc::clone(&self.content_height)
    }

    /// `layout(dimension, desiredPreviewEditorScrollLeft)`.
    pub fn layout(&self, dimension: Dimension, desired_preview_editor_scroll_left: f64) {
        self.preview_editor.editor().layout(Some(dimension));
        self.preview_editor_obs.editor.set_scroll_left(desired_preview_editor_scroll_left, None);
    }
}

impl IDisposable for LongDistancePreviewEditor {
    fn dispose(&self) {
        self.base.dispose();
    }
}
