//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/originalEditorInlineDiffView.ts`:
//! `OriginalEditorInlineDiffView`, o diff desenhado por decorações no editor original (e no modificado, quando há).
//!
//! Notas de tradução:
//! - `ICodeEditor` e `ITextModel` são o `Rc<CodeEditorWidget>` e o `Rc<TextModel>`; `AbstractText` é o
//!   `Rc<dyn AbstractText>` e o `observableCodeEditor(editor)` é o [`observable_code_editor`];
//! - `mode: 'insertionInline' | 'sideBySide' | 'deletion' | 'lineReplacement'` é o [`OriginalEditorInlineDiffViewMode`];
//! - `ModelDecorationOptions.register({...})` é o [`ModelDecorationOptions::register`] com o
//!   [`ModelDecorationOptionsInit`], e as opções dinâmicas (`options: {...}` soltas) são o `create_dynamic`. O
//!   `className` de `classNames(...)` usa o [`class_names`] do crate, onde `false` é `None`;
//! - `m.original.toInclusiveRange()!` só roda depois de `!m.original.isEmpty`, e o `LineRange::to_inclusive_range` é
//!   `None` exatamente quando o intervalo é vazio, então o teste e a extração são um `if let`;
//! - o `replacedText === '\n'` só importa no modo `deletion` (lido com `EndOfLinePreference.LF`); fora dele a conta é
//!   pura e o resultado não é usado, então a leitura do modelo só acontece nesse modo;
//! - `InlineEditAttachedData` é um tipo local com a ficha do dono (`Rc` comparada por identidade, como o `owner === this`);
//!   o `attachedData` das decorações é o [`InjectedTextAttachedData`] de `model`, e o alvo do mouse o devolve em
//!   `detail.injected_text`;
//! - `modelTokenizationFinished(model)` é um `observable_from_event` com o contador `i++` do módulo (um `thread_local`);
//! - `lineTokens.getTokensInRange(offsetRange)` é o [`LineTokens::get_tokens_in_range`], e `insertedText.slice(1, -1)` é
//!   a fatia calculada com o comprimento UTF-16 do texto; o `content` do texto injetado vira `String` (uma unidade solta
//!   de par substituto vira U+FFFD, como no ghost text);
//! - `allowsTrueInlineDiffRendering` usa só o `rangeIsSingleLine` (sem a exceção do intervalo `1,1,1,1` do
//!   `diffEditorViewZones`), como este arquivo do original.

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, IReader, Observable, ObservableExt, autorun_with_store, derived, observable_from_event};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::diff_editor_view_zones::range_is_single_line;
use crate::editor_browser::MouseTarget;
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::model::{
    EndOfLinePreference, InjectedTextAttachedData, InjectedTextCursorStops, InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration,
};
use crate::observable_code_editor::observable_code_editor;
use crate::offset_range::OffsetRange;
use crate::range::Range;
use crate::text::abstract_text::AbstractText;
use crate::text_model::{ModelDecorationOptions, TextModel};

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent};
use super::super::utils::utils::class_names;

/// `mode` de `IOriginalEditorInlineDiffViewState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalEditorInlineDiffViewMode {
    InsertionInline,
    SideBySide,
    Deletion,
    LineReplacement,
}

/// `IOriginalEditorInlineDiffViewState`.
pub struct OriginalEditorInlineDiffViewState {
    pub diff: Vec<DetailedLineRangeMapping>,
    pub modified_text: Rc<dyn AbstractText>,
    pub mode: OriginalEditorInlineDiffViewMode,
    pub editor_type: InlineCompletionEditorType,

    pub modified_code_editor: Rc<CodeEditorWidget>,
}

/// O `{ originalDecorations, modifiedDecorations }` do derivado `_decorations`.
struct InlineDiffDecorations {
    original: Rc<Vec<ModelDeltaDecoration>>,
    modified: Rc<Vec<ModelDeltaDecoration>>,
}

/// A ficha de identidade de uma view (o `this` do `owner === this`).
struct OwnerToken;

/// `InlineEditAttachedData`.
struct InlineEditAttachedData {
    owner: Rc<OwnerToken>,
}

impl InlineEditAttachedData {
    /// `a instanceof InlineEditAttachedData && a.owner === this`.
    fn is_owned_by(data: &InjectedTextAttachedData, owner: &Rc<OwnerToken>) -> bool {
        data.downcast::<InlineEditAttachedData>().is_some_and(|attached| Rc::ptr_eq(&attached.owner, owner))
    }
}

/// Um trecho do texto inserido, para o texto injetado ganhar borda no começo e no fim.
struct TextSegment {
    text: JsString,
    extra_classes: &'static [&'static str],
    offset_range: OffsetRange,
}

/// `allowsTrueInlineDiffRendering(mapping)`.
fn allows_true_inline_diff_rendering(mapping: &DetailedLineRangeMapping) -> bool {
    let Some(inner_changes) = &mapping.inner_changes else {
        return false;
    };
    inner_changes.iter().all(|c| range_is_single_line(&c.modified_range) && range_is_single_line(&c.original_range))
}

thread_local! {
    /// O `let i = 0` do módulo.
    static TOKENIZATION_COUNTER: Cell<i64> = const { Cell::new(0) };
}

/// `modelTokenizationFinished(model)`.
fn model_tokenization_finished(model: &Rc<TextModel>) -> IObservable<i64> {
    observable_from_event(model.on_did_change_tokens(), |_| {
        TOKENIZATION_COUNTER.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        })
    })
}

/// Os trechos do texto inserido: com mais de 3 unidades, o primeiro, o miolo e o último; senão o texto inteiro.
fn text_segments(inserted_text: &JsString, modified_range: &Range) -> Vec<TextSegment> {
    let length = inserted_text.len();
    if length > 3 {
        vec![
            TextSegment {
                text: inserted_text.slice(0, 1),
                extra_classes: &["start"],
                offset_range: OffsetRange::new(modified_range.start_column - 1, modified_range.start_column),
            },
            TextSegment {
                text: inserted_text.slice(1, length - 1),
                extra_classes: &[],
                offset_range: OffsetRange::new(modified_range.start_column, modified_range.end_column - 2),
            },
            TextSegment {
                text: inserted_text.slice(length - 1, length),
                extra_classes: &["end"],
                offset_range: OffsetRange::new(modified_range.end_column - 2, modified_range.end_column - 1),
            },
        ]
    } else {
        vec![TextSegment {
            text: inserted_text.clone(),
            extra_classes: &["start", "end"],
            offset_range: OffsetRange::new(modified_range.start_column - 1, modified_range.end_column),
        }]
    }
}

/// O corpo do derivado `_decorations`.
fn compute_decorations(
    original_editor: &Rc<CodeEditorWidget>,
    modified_text_model: &Rc<TextModel>,
    owner: &Rc<OwnerToken>,
    diff: &OriginalEditorInlineDiffViewState,
    tokenization_finished: &IObservable<i64>,
    reader: &dyn IReader,
) -> InlineDiffDecorations {
    let modified = &diff.modified_text;
    let show_inline = diff.mode == OriginalEditorInlineDiffViewMode::InsertionInline;
    let has_one_inner_change = diff.diff.len() == 1 && diff.diff[0].inner_changes.as_ref().is_some_and(|changes| changes.len() == 1);

    let show_empty_decorations = true;

    let mut original_decorations: Vec<ModelDeltaDecoration> = Vec::new();
    let mut modified_decorations: Vec<ModelDeltaDecoration> = Vec::new();

    let diff_line_add_decoration_background = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-line-insert".to_owned()),
        description: "line-insert".to_owned(),
        is_whole_line: true,
        margin_class_name: Some("gutter-insert".to_owned()),
        ..Default::default()
    });

    let diff_line_delete_decoration_background = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-line-delete".to_owned()),
        description: "line-delete".to_owned(),
        is_whole_line: true,
        margin_class_name: Some("gutter-delete".to_owned()),
        ..Default::default()
    });

    let diff_whole_line_delete_decoration = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-char-delete".to_owned()),
        description: "char-delete".to_owned(),
        is_whole_line: false,
        z_index: 1, // be on top of diff background decoration
        ..Default::default()
    });

    let diff_whole_line_add_decoration = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-char-insert".to_owned()),
        description: "char-insert".to_owned(),
        is_whole_line: true,
        ..Default::default()
    });

    let diff_add_decoration = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-char-insert".to_owned()),
        description: "char-insert".to_owned(),
        should_fill_line_on_line_break: Some(true),
        ..Default::default()
    });

    let diff_add_decoration_empty = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-char-insert diff-range-empty".to_owned()),
        description: "char-insert diff-range-empty".to_owned(),
        ..Default::default()
    });

    let nes_original_background = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        class_name: Some("inlineCompletions-original-lines".to_owned()),
        description: "inlineCompletions-original-lines".to_owned(),
        is_whole_line: false,
        should_fill_line_on_line_break: Some(true),
        ..Default::default()
    });

    let show_full_line_decorations = diff.mode != OriginalEditorInlineDiffViewMode::SideBySide
        && diff.mode != OriginalEditorInlineDiffViewMode::Deletion
        && diff.mode != OriginalEditorInlineDiffViewMode::InsertionInline
        && diff.mode != OriginalEditorInlineDiffViewMode::LineReplacement;
    let hide_empty_inner_decorations = diff.mode == OriginalEditorInlineDiffViewMode::LineReplacement;
    for m in &diff.diff {
        if show_full_line_decorations {
            if let Some(range) = m.original.to_inclusive_range() {
                original_decorations.push(ModelDeltaDecoration { range, options: Rc::clone(&diff_line_delete_decoration_background) });
            }
            if let Some(range) = m.modified.to_inclusive_range() {
                modified_decorations.push(ModelDeltaDecoration { range, options: Rc::clone(&diff_line_add_decoration_background) });
            }
        }

        if m.modified.is_empty() || m.original.is_empty() {
            if let Some(range) = m.original.to_inclusive_range() {
                original_decorations.push(ModelDeltaDecoration { range, options: Rc::clone(&diff_whole_line_delete_decoration) });
            }
            if let Some(range) = m.modified.to_inclusive_range() {
                modified_decorations.push(ModelDeltaDecoration { range, options: Rc::clone(&diff_whole_line_add_decoration) });
            }
        } else {
            let use_inline_diff = show_inline && allows_true_inline_diff_rendering(m);
            for i in m.inner_changes.as_deref().unwrap_or(&[]) {
                // Don't show empty markers outside the line range
                if m.original.contains(i.original_range.start_line_number) && !(hide_empty_inner_decorations && i.original_range.is_empty()) {
                    let replaced_is_line_break = diff.mode == OriginalEditorInlineDiffViewMode::Deletion
                        && original_editor
                            .get_model()
                            .is_some_and(|model| matches!(model.get_value_in_range_with(&i.original_range, EndOfLinePreference::Lf).units(), [10]));
                    let class_name = class_names(&[
                        Some("inlineCompletions-char-delete"),
                        (i.original_range.is_single_line() && show_inline).then_some("single-line-inline"),
                        i.original_range.is_empty().then_some("empty"),
                        (((i.original_range.is_empty() && has_one_inner_change) || replaced_is_line_break) && show_empty_decorations && !use_inline_diff)
                            .then_some("diff-range-empty"),
                    ]);
                    original_decorations.push(ModelDeltaDecoration {
                        range: i.original_range,
                        options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                            description: "char-delete".to_owned(),
                            should_fill_line_on_line_break: Some(false),
                            class_name: Some(class_name),
                            inline_class_name: use_inline_diff.then(|| class_names(&[Some("strike-through"), Some("inlineCompletions")])),
                            z_index: 1,
                            ..Default::default()
                        }),
                    });
                }
                if m.modified.contains(i.modified_range.start_line_number) {
                    modified_decorations.push(ModelDeltaDecoration {
                        range: i.modified_range,
                        options: if i.modified_range.is_empty() && show_empty_decorations && !use_inline_diff && has_one_inner_change {
                            Rc::clone(&diff_add_decoration_empty)
                        } else {
                            Rc::clone(&diff_add_decoration)
                        },
                    });
                }
                if use_inline_diff {
                    let inserted_text = modified.get_value_of_range(&i.modified_range);

                    // when the injected text becomes long, the editor will split it into multiple spans
                    // to be able to get the border around the start and end of the text, we need to split it into multiple segments
                    let segments = text_segments(&inserted_text, &i.modified_range);

                    // Tokenization
                    tokenization_finished.read(reader); // reconsider when tokenization is finished
                    let line_tokens = modified_text_model.tokenization().get_line_tokens(i.modified_range.start_line_number);

                    for TextSegment { text, extra_classes, offset_range } in segments {
                        let mut classes = vec![
                            Some("inlineCompletions-char-insert"),
                            (i.modified_range.is_single_line() && show_inline).then_some("single-line-inline"),
                        ];
                        // include extraClasses for additional styling if provided
                        classes.extend(extra_classes.iter().copied().map(Some));
                        original_decorations.push(ModelDeltaDecoration {
                            range: Range::from_positions(i.original_range.get_end_position(), i.original_range.get_end_position()),
                            options: ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                                description: "inserted-text".to_owned(),
                                before: Some(InjectedTextOptions {
                                    content: text.to_string_lossy(),
                                    tokens: Some(line_tokens.get_tokens_in_range(offset_range)),
                                    inline_class_name: Some(class_names(&classes)),
                                    cursor_stops: Some(InjectedTextCursorStops::None),
                                    attached_data: InjectedTextAttachedData::new(Rc::new(InlineEditAttachedData { owner: Rc::clone(owner) })),
                                    ..InjectedTextOptions::default()
                                }),
                                z_index: 2,
                                show_if_collapsed: true,
                                ..Default::default()
                            }),
                        });
                    }
                }
            }
        }
    }

    if diff.editor_type == InlineCompletionEditorType::DiffEditor {
        for m in &diff.diff {
            if !m.original.is_empty() {
                original_decorations.push(ModelDeltaDecoration { range: m.original.to_exclusive_range(), options: Rc::clone(&nes_original_background) });
            }
        }
    }

    InlineDiffDecorations { original: Rc::new(original_decorations), modified: Rc::new(modified_decorations) }
}

/// `OriginalEditorInlineDiffView`.
pub struct OriginalEditorInlineDiffView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    is_hovered: IObservable<bool>,
}

impl OriginalEditorInlineDiffView {
    /// `OriginalEditorInlineDiffView.supportsInlineDiffRendering(mapping)`.
    pub fn supports_inline_diff_rendering(mapping: &DetailedLineRangeMapping) -> bool {
        allows_true_inline_diff_rendering(mapping)
    }

    pub fn new(
        original_editor: Rc<CodeEditorWidget>,
        state: IObservable<Option<Rc<OriginalEditorInlineDiffViewState>>>,
        modified_text_model: Rc<TextModel>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let owner = Rc::new(OwnerToken);
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(Rc::clone(&on_did_click));

        let original_editor_obs = observable_code_editor(&original_editor);

        let is_hovered = {
            let owner = Rc::clone(&owner);
            original_editor_obs.is_target_hovered(
                move |p| match &p.target {
                    MouseTarget::ContentText(target) => target
                        .detail
                        .injected_text
                        .as_ref()
                        .is_some_and(|injected| InlineEditAttachedData::is_owned_by(&injected.options.attached_data, &owner)),
                    _ => false,
                },
                base.store(),
            )
        };
        let tokenization_finished = model_tokenization_finished(&modified_text_model);

        let decorations: IObservable<Option<Rc<InlineDiffDecorations>>> = {
            let (original_editor, owner) = (Rc::clone(&original_editor), Rc::clone(&owner));
            let state = Rc::clone(&state);
            derived(move |reader| {
                let diff = state.read(reader)?;
                Some(Rc::new(compute_decorations(&original_editor, &modified_text_model, &owner, &diff, &tokenization_finished, reader)))
            })
        };

        base.register(original_editor_obs.set_decorations(&decorations.map(|d, _| d.map_or_else(|| Rc::new(Vec::new()), |d| Rc::clone(&d.original)))));

        let modified_code_editor: IObservable<Option<Rc<CodeEditorWidget>>> = state.map(|s, _| s.map(|s| Rc::clone(&s.modified_code_editor)));
        base.register(autorun_with_store({
            let decorations = Rc::clone(&decorations);
            move |reader, store| {
                if let Some(e) = modified_code_editor.read(reader) {
                    store.add(
                        observable_code_editor(&e)
                            .set_decorations(&decorations.map(|d, _| d.map_or_else(|| Rc::new(Vec::new()), |d| Rc::clone(&d.modified)))),
                    );
                }
            }
        }));

        base.register(original_editor.on_mouse_up().listen({
            let (on_did_click, owner) = (Rc::clone(&on_did_click), Rc::clone(&owner));
            move |e| {
                let MouseTarget::ContentText(target) = &e.target else {
                    return;
                };
                let owned = target
                    .detail
                    .injected_text
                    .as_ref()
                    .is_some_and(|injected| InlineEditAttachedData::is_owned_by(&injected.options.attached_data, &owner));
                if owned {
                    on_did_click.fire(InlineEditClickEvent::new(e.event.clone(), false));
                }
            }
        }));

        Rc::new(Self { base, on_did_click, is_hovered })
    }
}

impl IInlineEditsView for OriginalEditorInlineDiffView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for OriginalEditorInlineDiffView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
