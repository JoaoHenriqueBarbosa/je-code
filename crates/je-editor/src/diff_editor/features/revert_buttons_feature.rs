//! Porte de `src/vs/editor/browser/widget/diffEditor/features/revertButtonsFeature.ts`: as setas que revertem uma mudança (ou a seleção)
//! do editor modificado para o texto do original, na margem de glifos do editor modificado.
//!
//! Divergências:
//! - **DOM vira dado.** O `div.revertButton` com o ícone `arrowRight` é o [`RevertButton`]: o título ([`RevertButton::title`]) e o ícone
//!   ([`RevertButton::icon`]) são o que o pintor lê. Os ouvintes de `mousedown`, `mouseup` e `click` do `domNode` são os métodos
//!   [`RevertButton::on_mouse_down`], [`RevertButton::on_mouse_up`] e [`RevertButton::on_click`], que o dono da janela chama quando o
//!   evento nativo chega no botão; cada um devolve se o botão consumiu o evento (o `stopPropagation()` e o `preventDefault()` do
//!   original), e depois do descarte nenhum deles faz nada (o `removeEventListener`).
//! - **`this._widget: DiffEditorWidget`.** É o `Weak` do widget que o [`DiffEditorFeatureContext`] traz (a feature nasce dentro do
//!   construtor do widget). Um botão cujo widget já foi descartado não reverte nada.
//! - **`RangeMapping[] | LineRangeMapping`** é o enum [`RevertDiffs`]; o `instanceof LineRangeMapping` do clique é o `match`.
//! - **`selectedDiffs`.** O `Set<DetailedLineRangeMapping>` por identidade é o conjunto dos índices do mapeamento em `diff.mappings`
//!   (cada [`SelectedDiff`] guarda o índice), e `emptyArr` (a constante que não dispara mudança) é um `Rc` criado uma vez e devolvido
//!   sempre que não há diferença selecionada (a igualdade estrita do derivado é a identidade do `Rc`).

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::observables::derived::derived_owned;
use je_base::observable::{DebugOwner, Observable, autorun_with_store};
use je_base::themables::ThemeIcon;

use crate::diff::range_mapping::{DetailedLineRangeMapping, RangeMapping};
use crate::diff_editor::diff_editor_widget::DiffEditorWidget;
use crate::diff_editor::feature_contracts::DiffEditorFeatureContext;
use crate::editor_browser::{GlyphMarginWidgetPosition, IGlyphMarginWidget};
use crate::line_range::{LineRange, LineRangeSet};
use crate::model::GlyphMarginLane;
use crate::range::Range;

/// Uma diferença tocada pela seleção: o índice dela em `diff.mappings` e os mapeamentos internos que a seleção intersecta.
pub struct SelectedDiff {
    pub mapping_index: usize,
    pub range_mappings: Vec<RangeMapping>,
}

/// `RevertButtonsFeature`.
pub struct RevertButtonsFeature {
    base: Rc<Disposable>,
}

impl RevertButtonsFeature {
    pub fn new(context: &DiffEditorFeatureContext) -> Rc<Self> {
        let owner = DebugOwner::new("RevertButtonsFeature");
        let base = Disposable::new();
        let editors = context.editors.clone();
        let diff_model = context.diff_model.clone();
        let options = context.options.clone();
        let widget = context.diff_editor.clone();

        // `emptyArr`: a constante que não dispara mudança (`[]` é sempre um array novo).
        let empty_arr: Rc<Vec<SelectedDiff>> = Rc::default();
        let selected_diffs = {
            let (editors, diff_model) = (editors.clone(), diff_model.clone());
            derived_owned(owner, move |reader| {
                // selectedDiffs
                let diff = diff_model.read(reader).and_then(|model| model.diff.read(reader));
                // Return `emptyArr` because it is a constant. [] is always a new array and would trigger a change.
                let Some(diff) = diff else { return empty_arr.clone() };

                let selections = editors.modified_selections.read(reader);
                if selections.iter().all(|s| s.is_empty()) {
                    return empty_arr.clone();
                }

                let selected_line_numbers = LineRangeSet::new(selections.iter().map(|s| LineRange::from_range_inclusive(s)).collect());

                let result: Vec<SelectedDiff> = diff
                    .mappings
                    .iter()
                    .enumerate()
                    .filter_map(|(mapping_index, m)| {
                        let inner_changes = m.line_range_mapping.inner_changes.as_ref()?;
                        if !selected_line_numbers.intersects(&m.line_range_mapping.modified) {
                            return None;
                        }
                        Some(SelectedDiff {
                            mapping_index,
                            range_mappings: inner_changes
                                .iter()
                                .filter(|c| selections.iter().any(|s| Range::are_intersecting(&c.modified_range, s)))
                                .copied()
                                .collect(),
                        })
                    })
                    .collect();
                if result.is_empty() || result.iter().all(|r| r.range_mappings.is_empty()) {
                    return empty_arr.clone();
                }
                Rc::new(result)
            })
        };

        base.register(autorun_with_store(move |reader, store| {
            if !options.should_render_old_revert_arrows.read(reader) {
                return;
            }
            let model = diff_model.read(reader);
            let diff = model.as_ref().and_then(|model| model.diff.read(reader));
            let (Some(model), Some(diff)) = (model, diff) else { return };
            if model.moved_text_to_compare.read(reader).is_some() {
                return;
            }

            let mut glyph_widgets_modified: Vec<Rc<dyn IGlyphMarginWidget>> = Vec::new();

            let selected_diffs = selected_diffs.read(reader);
            let selected_diffs_set: HashSet<usize> = selected_diffs.iter().map(|d| d.mapping_index).collect();

            if !selected_diffs.is_empty() {
                // The button to revert the selection
                let selections = editors.modified_selections.read(reader);

                let btn = RevertButton::new(
                    selections.last().expect("há seleção quando há diferença selecionada").position_line_number,
                    widget.clone(),
                    RevertDiffs::Ranges(selected_diffs.iter().flat_map(|d| d.range_mappings.iter().copied()).collect()),
                    true,
                );
                store.add(btn.clone());
                editors.modified.add_glyph_margin_widget(btn.clone());
                glyph_widgets_modified.push(btn);
            }

            for (index, m) in diff.mappings.iter().enumerate() {
                if selected_diffs_set.contains(&index) {
                    continue;
                }
                if !m.line_range_mapping.modified.is_empty() && m.line_range_mapping.inner_changes.is_some() {
                    let btn = RevertButton::new(
                        m.line_range_mapping.modified.start_line_number,
                        widget.clone(),
                        RevertDiffs::Lines(m.line_range_mapping.clone()),
                        false,
                    );
                    store.add(btn.clone());
                    editors.modified.add_glyph_margin_widget(btn.clone());
                    glyph_widgets_modified.push(btn);
                }
            }

            let modified = editors.modified.clone();
            store.add(to_disposable(move || {
                for w in &glyph_widgets_modified {
                    modified.remove_glyph_margin_widget(w);
                }
            }));
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for RevertButtonsFeature {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que um [`RevertButton`] reverte: `RangeMapping[] | LineRangeMapping`.
pub enum RevertDiffs {
    Ranges(Vec<RangeMapping>),
    Lines(DetailedLineRangeMapping),
}

thread_local! {
    /// `RevertButton.counter`.
    static REVERT_BUTTON_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// `RevertButton`.
pub struct RevertButton {
    id: String,
    line_number: i32,
    widget: Weak<DiffEditorWidget>,
    diffs: RevertDiffs,
    revert_selection: bool,
    disposed: Cell<bool>,
}

impl RevertButton {
    pub fn new(line_number: i32, widget: Weak<DiffEditorWidget>, diffs: RevertDiffs, revert_selection: bool) -> Rc<Self> {
        let counter = REVERT_BUTTON_COUNTER.with(|counter| {
            let current = counter.get();
            counter.set(current + 1);
            current
        });
        Rc::new(Self { id: format!("revertButton{counter}"), line_number, widget, diffs, revert_selection, disposed: Cell::new(false) })
    }

    /// O `title` do `domNode`.
    pub fn title(&self) -> String {
        if self.revert_selection {
            localize("revertSelectedChanges", "Revert Selected Changes", &[])
        } else {
            localize("revertChange", "Revert Change", &[])
        }
    }

    /// O ícone do botão (`Codicon.arrowRight`).
    pub fn icon(&self) -> ThemeIcon {
        ThemeIcon::from_id("arrow-right")
    }

    /// O `mousedown` do `domNode`: devolve se o botão consumiu o evento.
    pub fn on_mouse_down(&self, button: u8) -> bool {
        // don't prevent context menu from showing up
        !self.disposed.get() && button != 2
    }

    /// O `mouseup` do `domNode`: devolve se o botão consumiu o evento.
    pub fn on_mouse_up(&self) -> bool {
        !self.disposed.get()
    }

    /// O `click` do `domNode`: reverte e devolve se o botão consumiu o evento.
    pub fn on_click(&self) -> bool {
        if self.disposed.get() {
            return false;
        }
        if let Some(widget) = self.widget.upgrade() {
            match &self.diffs {
                RevertDiffs::Lines(diffs) => widget.revert(diffs),
                RevertDiffs::Ranges(diffs) => widget.revert_range_mappings(diffs),
            }
        }
        true
    }
}

impl IGlyphMarginWidget for RevertButton {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    /// Get the placement of the glyph widget.
    fn get_position(&self) -> GlyphMarginWidgetPosition {
        GlyphMarginWidgetPosition {
            lane: GlyphMarginLane::Right,
            range: Range::new(self.line_number, 1, self.line_number, 1),
            z_index: 10001,
        }
    }
}

impl IDisposable for RevertButton {
    fn dispose(&self) {
        self.disposed.set(true);
    }
}
