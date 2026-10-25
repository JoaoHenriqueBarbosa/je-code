//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorDecorations.ts`: as decorações de linha e de trecho que o
//! editor de diff põe no original e no modificado a partir do diff (e dos blocos de texto movido).
//!
//! Divergências:
//! - O construtor do original recebe também o `widget: DiffEditorWidget`, que nunca usa; o porte não o recebe.
//! - Os campos `_editors`, `_diffModel` e `_options` do original só alimentam o derivado; aqui o derivado os captura e a struct
//!   guarda só o `base` que descarta as duas aplicações de decorações.
//! - `{ originalDecorations, modifiedDecorations } | null` é `Option<Rc<DiffDecorations>>`, com as listas em `Rc` (o `===` do
//!   original compara a identidade do array); `d?.originalDecorations || []` é o `Rc` da lista, ou uma lista vazia nova.
//! - `m === activeMovedText` (identidade) é a igualdade de valor do `MovedText`: o `activeMovedText` do view model é um `Rc`
//!   novo a cada sincronização do diff, que não é o elemento de `diff.movedTexts`.
//! - `before: { content, inlineClassName }` do texto apagado leva o conteúdo como `String` (a conversão do texto UTF-16 troca
//!   uma unidade solta por U+FFFD).
//! - As opções dos blocos movidos e do texto apagado são objetos literais no original (`createDynamic`, não registrados).

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, IReader, Observable, derived};

use crate::diff_editor::MOVED_CODE_BLOCK_PADDING;
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::registrations_contribution::diff_decoration_options;
use crate::diff_editor_options::DiffEditorOptions;
use crate::diff_editor_utils::apply_observable_decorations;
use crate::diff_editor_view_model::DiffEditorViewModel;
use crate::diff_editor::components::diff_editor_view_zones::allows_true_inline_diff_rendering;
use crate::line_range::LineRange;
use crate::model::{InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration};
use crate::range::Range;
use crate::text_model::ModelDecorationOptions;

/// `{ originalDecorations, modifiedDecorations }`.
struct DiffDecorations {
    original_decorations: Rc<Vec<ModelDeltaDecoration>>,
    modified_decorations: Rc<Vec<ModelDeltaDecoration>>,
}

/// `{ range, options }`.
fn decoration(range: Range, options: &Rc<ModelDecorationOptions>) -> ModelDeltaDecoration {
    ModelDeltaDecoration { range, options: Rc::clone(options) }
}

/// `lineRange.toInclusiveRange()!`.
fn inclusive_range(line_range: &LineRange) -> Range {
    line_range.to_inclusive_range().expect("o intervalo de linhas não está vazio")
}

/// A lista de decorações de um lado, ou uma lista vazia sem diff (`d?.<lado>Decorations || []`).
fn side_decorations(
    decorations: &IObservable<Option<Rc<DiffDecorations>>>,
    side: fn(&DiffDecorations) -> &Rc<Vec<ModelDeltaDecoration>>,
) -> IObservable<Rc<Vec<ModelDeltaDecoration>>> {
    let decorations = decorations.clone();
    derived(move |reader| decorations.read(reader).map(|d| Rc::clone(side(&d))).unwrap_or_default())
}

/// O corpo do derivado `_decorations`.
fn compute_decorations(
    diff_model: &IObservable<Option<Rc<DiffEditorViewModel>>>,
    options: &DiffEditorOptions,
    reader: &dyn IReader,
) -> Option<Rc<DiffDecorations>> {
    let diff_model = diff_model.read(reader)?;
    let diff = diff_model.diff.read(reader)?;

    let moved_text_to_compare = diff_model.moved_text_to_compare.read(reader);
    let render_indicators = options.render_indicators.read(reader);
    let show_empty_decorations = options.show_empty_decorations.read(reader);

    let decorations = diff_decoration_options();
    let (line_add_background, line_delete_background) = if render_indicators {
        (&decorations.diff_line_add_decoration_background_with_indicator, &decorations.diff_line_delete_decoration_background_with_indicator)
    } else {
        (&decorations.diff_line_add_decoration_background, &decorations.diff_line_delete_decoration_background)
    };

    let mut original_decorations: Vec<ModelDeltaDecoration> = Vec::new();
    let mut modified_decorations: Vec<ModelDeltaDecoration> = Vec::new();
    if moved_text_to_compare.is_none() {
        for m in &diff.mappings {
            let mapping = &m.line_range_mapping;
            if !mapping.original.is_empty() {
                original_decorations.push(decoration(inclusive_range(&mapping.original), line_delete_background));
            }
            if !mapping.modified.is_empty() {
                modified_decorations.push(decoration(inclusive_range(&mapping.modified), line_add_background));
            }

            if mapping.modified.is_empty() || mapping.original.is_empty() {
                if !mapping.original.is_empty() {
                    original_decorations.push(decoration(inclusive_range(&mapping.original), &decorations.diff_whole_line_delete_decoration));
                }
                if !mapping.modified.is_empty() {
                    modified_decorations.push(decoration(inclusive_range(&mapping.modified), &decorations.diff_whole_line_add_decoration));
                }
            } else {
                let use_inline_diff = options.use_true_inline_diff_rendering.read(reader) && allows_true_inline_diff_rendering(mapping);
                for i in mapping.inner_changes.as_deref().unwrap_or(&[]) {
                    // Don't show empty markers outside the line range
                    if mapping.original.contains(i.original_range.start_line_number) {
                        let options = if i.original_range.is_empty() && show_empty_decorations {
                            &decorations.diff_delete_decoration_empty
                        } else {
                            &decorations.diff_delete_decoration
                        };
                        original_decorations.push(decoration(i.original_range, options));
                    }
                    if mapping.modified.contains(i.modified_range.start_line_number) {
                        let options = if i.modified_range.is_empty() && show_empty_decorations && !use_inline_diff {
                            &decorations.diff_add_decoration_empty
                        } else {
                            &decorations.diff_add_decoration
                        };
                        modified_decorations.push(decoration(i.modified_range, options));
                    }
                    if use_inline_diff {
                        let deleted_text = diff_model.model.original.get_value_in_range(&i.original_range).to_string_lossy();
                        modified_decorations.push(decoration(
                            i.modified_range,
                            &ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                                description: "deleted-text".to_owned(),
                                before: Some(InjectedTextOptions {
                                    content: deleted_text,
                                    inline_class_name: Some("inline-deleted-text".to_owned()),
                                    ..InjectedTextOptions::default()
                                }),
                                z_index: 100000,
                                show_if_collapsed: true,
                                ..ModelDecorationOptionsInit::default()
                            }),
                        ));
                    }
                }
            }
        }
    }

    if let Some(moved_text_to_compare) = &moved_text_to_compare {
        for m in &moved_text_to_compare.changes {
            if let Some(full_range_original) = m.original.to_inclusive_range() {
                original_decorations.push(decoration(full_range_original, line_delete_background));
            }
            if let Some(full_range_modified) = m.modified.to_inclusive_range() {
                modified_decorations.push(decoration(full_range_modified, line_add_background));
            }

            for i in m.inner_changes.as_deref().unwrap_or(&[]) {
                original_decorations.push(decoration(i.original_range, &decorations.diff_delete_decoration));
                modified_decorations.push(decoration(i.modified_range, &decorations.diff_add_decoration));
            }
        }
    }
    let active_moved_text = diff_model.active_moved_text.read(reader);

    for m in &diff.moved_texts {
        let current_move = if active_moved_text.as_deref() == Some(m) { " currentMove" } else { "" };
        original_decorations.push(decoration(
            inclusive_range(&m.line_range_mapping.original),
            &ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                description: "moved".to_owned(),
                block_class_name: Some(format!("movedOriginal{current_move}")),
                block_padding: Some([MOVED_CODE_BLOCK_PADDING, 0, MOVED_CODE_BLOCK_PADDING, MOVED_CODE_BLOCK_PADDING]),
                ..ModelDecorationOptionsInit::default()
            }),
        ));

        modified_decorations.push(decoration(
            inclusive_range(&m.line_range_mapping.modified),
            &ModelDecorationOptions::create_dynamic(&ModelDecorationOptionsInit {
                description: "moved".to_owned(),
                block_class_name: Some(format!("movedModified{current_move}")),
                block_padding: Some([4, 0, 4, 4]),
                ..ModelDecorationOptionsInit::default()
            }),
        ));
    }

    Some(Rc::new(DiffDecorations { original_decorations: Rc::new(original_decorations), modified_decorations: Rc::new(modified_decorations) }))
}

/// `DiffEditorDecorations`.
pub struct DiffEditorDecorations {
    base: Rc<Disposable>,
}

impl DiffEditorDecorations {
    pub fn new(editors: &DiffEditorEditors, diff_model: IObservable<Option<Rc<DiffEditorViewModel>>>, options: Rc<DiffEditorOptions>) -> Rc<Self> {
        // `_decorations`, o campo que o original inicializa antes do corpo do construtor.
        let decorations = derived(move |reader| compute_decorations(&diff_model, &options, reader));

        let base = Disposable::new();
        base.register(apply_observable_decorations(&editors.original, side_decorations(&decorations, |d| &d.original_decorations)));
        base.register(apply_observable_decorations(&editors.modified, side_decorations(&decorations, |d| &d.modified_decorations)));
        Rc::new(Self { base })
    }
}

impl IDisposable for DiffEditorDecorations {
    fn dispose(&self) {
        self.base.dispose();
    }
}
