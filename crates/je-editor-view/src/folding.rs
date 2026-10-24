//! A quebra de linha (`editor.wordWrap`) e a dobra de código da view: o que no VS Code é o `FoldingController`
//! (`editor/contrib/folding/browser/folding.ts`) com o `foldingDecorations.ts`, sobre o `FoldingModel` e o
//! `HiddenRangeModel` portados em `je-editor`. As regiões vêm da indentação e dos marcadores da linguagem
//! (`IndentRangeProvider`, `computeRanges`); as linhas escondidas vão ao `ViewModel` pelo `setHiddenAreas` do
//! widget, como o controlador faz, e o `ViewModel` as tira das linhas da view.
//!
//! As regiões são recalculadas 200 ms depois da última edição (o `Delayer` do `updateScheduler`). O provedor por
//! sintaxe (`SyntaxRangeProvider`) pede os provedores de dobra das extensões, que ainda não existem aqui.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_base::runtime;
use je_editor::code_editor_widget::{ICodeEditor, SelectionOrRange};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::folding::folding_model::{FoldingModel, IDecorationProvider, set_collapse_state_levels_down, set_collapse_state_up};
use je_editor::folding::folding_ranges::FoldingRegion;
use je_editor::folding::hidden_range_model::HiddenRangeModel;
use je_editor::folding::indent_range_provider::compute_ranges;
use je_editor::folding::to_selected_lines;
use je_editor::model::{ModelDecorationOptionsInit, TrackedRangeStickiness};
use je_editor::range::Range;
use je_editor::text_model::{ModelDecorationOptions, ModelDecorationsChangeAccessor, TextModel};
use web_time::{Duration, Instant};

use crate::core::EditorCore;
use crate::view::Shared;

/// `editor.wordWrap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordWrap {
    Off,
    On,
    WordWrapColumn,
    Bounded,
}

impl WordWrap {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::WordWrapColumn => "wordWrapColumn",
            Self::Bounded => "bounded",
        }
    }

    /// `editor.action.toggleWordWrap`: desligada vira `on`, qualquer outra vira `off`.
    pub fn toggled(self) -> Self {
        if self == Self::Off { Self::On } else { Self::Off }
    }
}

/// A demora entre a última edição e o novo cálculo das regiões (`updateDebounceInfo`, mínimo do original).
const UPDATE_DELAY: Duration = Duration::from_millis(200);
/// A fonte das linhas escondidas no `HiddenAreasModel` do `ViewModel` (a identidade do controlador).
const HIDDEN_AREAS_SOURCE: usize = 0x0f01d;

/// As decorações das regiões (`FoldingDecorationProvider`): só a identidade conta aqui, a pintura lê o estado do
/// `FoldingModel`. As três acompanham o texto sem crescer nas bordas, como as do original.
struct DecorationProvider {
    model: Rc<TextModel>,
    collapsed: Rc<ModelDecorationOptions>,
    expanded: Rc<ModelDecorationOptions>,
    hidden: Rc<ModelDecorationOptions>,
}

fn decoration(description: &str, whole_line: bool) -> Rc<ModelDecorationOptions> {
    ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: description.to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        is_whole_line: whole_line,
        ..Default::default()
    })
}

impl IDecorationProvider for DecorationProvider {
    fn get_decoration_option(&self, is_collapsed: bool, is_hidden: bool, _is_manual: bool) -> Rc<ModelDecorationOptions> {
        if is_hidden {
            self.hidden.clone()
        } else if is_collapsed {
            self.collapsed.clone()
        } else {
            self.expanded.clone()
        }
    }

    fn change_decorations(&self, callback: &mut dyn FnMut(&ModelDecorationsChangeAccessor<'_>)) -> Option<()> {
        self.model.change_decorations(|accessor| callback(accessor), 0)
    }

    fn remove_decorations(&self, decoration_ids: &[String]) {
        self.model.change_decorations(|accessor| accessor.delta_decorations(decoration_ids, &[]), 0);
    }
}

pub struct Folding {
    model: Rc<TextModel>,
    folding_model: Rc<FoldingModel>,
    hidden: Rc<HiddenRangeModel>,
    /// A versão do modelo das regiões atuais (`-1`: nunca calculadas).
    computed_version: Cell<i64>,
    last_change: Rc<Cell<Option<Instant>>>,
    /// Um aviso de quadro já está agendado para quando a demora vencer.
    wake_scheduled: Rc<Cell<bool>>,
    shared: Rc<Shared>,
    /// As linhas escondidas novas que o `HiddenRangeModel` avisou e o `ViewModel` ainda não recebeu.
    pending_hidden: Rc<RefCell<Option<Vec<Range>>>>,
    subscriptions: Vec<DisposableRef>,
}

impl Folding {
    pub(crate) fn new(model: &Rc<TextModel>, shared: &Rc<Shared>) -> Self {
        let provider = Rc::new(DecorationProvider {
            model: model.clone(),
            collapsed: decoration("folding-collapsed-visual-decoration", true),
            expanded: decoration("folding-expanded-visual-decoration", false),
            hidden: decoration("folding-hidden-range-decoration", false),
        });
        let folding_model = FoldingModel::new(model.clone(), provider);
        let hidden = HiddenRangeModel::new(folding_model.clone());
        let pending_hidden: Rc<RefCell<Option<Vec<Range>>>> = Rc::default();
        let last_change: Rc<Cell<Option<Instant>>> = Rc::default();
        let mut subscriptions = Vec::new();
        let pending = pending_hidden.clone();
        subscriptions.push(hidden.on_did_change().listen(move |ranges: &Vec<Range>| {
            *pending.borrow_mut() = Some(ranges.clone());
        }));
        let (h, changed) = (hidden.clone(), last_change.clone());
        subscriptions.push(model.on_did_change_content().listen(move |e| {
            h.notify_change_model_content(e);
            changed.set(Some(Instant::now()));
        }));
        Self {
            model: model.clone(),
            folding_model,
            hidden,
            computed_version: Cell::new(-1),
            last_change,
            wake_scheduled: Rc::default(),
            shared: shared.clone(),
            pending_hidden,
            subscriptions,
        }
    }

    /// Recalcula as regiões se o texto mudou e a demora venceu, e entrega ao `ViewModel` as linhas escondidas novas.
    pub(crate) fn update(&self, core: &EditorCore) {
        let version = self.model.get_version_id();
        if version != self.computed_version.get() {
            let due = self.computed_version.get() < 0 || self.last_change.get().is_none_or(|at| at.elapsed() >= UPDATE_DELAY);
            if due {
                self.recompute(core);
            } else if !self.wake_scheduled.replace(true) {
                let (shared, scheduled) = (self.shared.clone(), self.wake_scheduled.clone());
                runtime::set_timeout(
                    move || {
                        scheduled.set(false);
                        shared.dirty.set(true);
                    },
                    UPDATE_DELAY.as_millis() as u64,
                );
            }
        }
        self.apply_hidden(core);
    }

    fn recompute(&self, core: &EditorCore) {
        self.computed_version.set(self.model.get_version_id());
        let configuration = self.model.language_configuration_service().get_language_configuration(&self.model.get_language_id());
        let rules = &configuration.folding_rules;
        let regions = compute_ranges(&self.model, rules.off_side.unwrap_or(false), rules.markers.as_ref(), None);
        let selections = core.selections();
        self.folding_model.update(&regions, Some(&to_selected_lines(Some(&selections))));
    }

    /// `onHiddenRangesChanges`: os cursores saem das linhas escondidas e o `ViewModel` recebe as linhas.
    fn apply_hidden(&self, core: &EditorCore) {
        let Some(ranges) = self.pending_hidden.borrow_mut().take() else { return };
        if !ranges.is_empty() {
            let mut selections = core.selections();
            if self.hidden.adjust_selections(&mut selections) {
                core.widget.set_selections(&selections, Some("api"), None);
            }
        }
        core.widget.set_hidden_areas(&ranges, Some(HIDDEN_AREAS_SOURCE), None);
    }

    /// A região que começa na linha do modelo `line` (índice, dobrada?).
    pub(crate) fn region_starting_at(&self, line: i32) -> Option<(usize, bool)> {
        let regions = self.folding_model.regions();
        let line = u32::try_from(line).ok()?;
        let (mut lo, mut hi) = (0usize, regions.length());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if regions.get_start_line_number(mid) < line { lo = mid + 1 } else { hi = mid }
        }
        (lo < regions.length() && regions.get_start_line_number(lo) == line).then(|| (lo, regions.is_collapsed(lo)))
    }

    /// Há alguma região dobrada?
    pub(crate) fn has_hidden(&self) -> bool {
        self.hidden.has_ranges()
    }

    /// O clique no chevron da linha `line` (`onEditorMouseUp`): dobra ou desdobra a região que começa ali; com
    /// `recursive` (Shift), primeiro as filhas.
    pub(crate) fn toggle_at(&self, core: &EditorCore, line: i32, recursive: bool) -> bool {
        let Some(region) = self.folding_model.get_region_at_line(line as u32) else { return false };
        if region.start_line_number() != line as u32 {
            return false;
        }
        let collapsed = region.is_collapsed();
        let mut to_toggle: Vec<FoldingRegion> = Vec::new();
        if recursive {
            to_toggle.extend(self.folding_model.get_regions_inside(Some(&region), None).into_iter().filter(|r| r.is_collapsed() == collapsed));
        }
        if collapsed || !recursive || to_toggle.is_empty() {
            to_toggle.push(region);
        }
        self.folding_model.toggle_collapse_state(to_toggle);
        self.apply_hidden(core);
        true
    }

    fn selected_lines(core: &EditorCore) -> Vec<u32> {
        core.selections().iter().map(|s| s.start_line_number.max(1) as u32).collect()
    }

    /// `editor.fold` (Ctrl+Shift+[): a região do cursor, ou a mãe se ela já está dobrada.
    pub(crate) fn fold(&self, core: &EditorCore) {
        self.ensure_computed(core);
        set_collapse_state_up(&self.folding_model, true, &Self::selected_lines(core));
        self.apply_hidden(core);
    }

    /// `editor.unfold` (Ctrl+Shift+]): um nível a partir do cursor.
    pub(crate) fn unfold(&self, core: &EditorCore) {
        self.ensure_computed(core);
        set_collapse_state_levels_down(&self.folding_model, false, 1, Some(&Self::selected_lines(core)));
        self.apply_hidden(core);
    }

    /// `editor.foldAll` (Ctrl+K Ctrl+0) e `editor.unfoldAll` (Ctrl+K Ctrl+J).
    pub(crate) fn set_all(&self, core: &EditorCore, collapsed: bool) {
        self.ensure_computed(core);
        set_collapse_state_levels_down(&self.folding_model, collapsed, usize::MAX, None);
        self.apply_hidden(core);
    }

    /// As ações agem sobre as regiões do texto atual (o `getFoldingModel()` espera o cálculo pendente).
    fn ensure_computed(&self, core: &EditorCore) {
        if self.computed_version.get() != self.model.get_version_id() {
            self.recompute(core);
        }
    }

    /// Mostra a linha do cursor se ela ficou escondida (`revealCursor`): desdobra as regiões que a escondem.
    pub(crate) fn reveal_line(&self, core: &EditorCore, line: i32) {
        if !self.hidden.is_hidden(line) {
            return;
        }
        let regions = self.folding_model.get_all_regions_at_line(line as u32, Some(&|r: &FoldingRegion, _| r.is_collapsed() && line as u32 > r.start_line_number()));
        if !regions.is_empty() {
            self.folding_model.toggle_collapse_state(regions);
            self.apply_hidden(core);
        }
    }

    /// Seleciona `range` desdobrando o que o esconde (a busca e o ir para linha passam por aqui).
    pub(crate) fn select_revealing(&self, core: &EditorCore, range: Range) {
        self.reveal_line(core, range.start_line_number);
        core.widget.set_selection(SelectionOrRange::Range(range), Some("keyboard"));
    }
}

impl Drop for Folding {
    fn drop(&mut self) {
        for subscription in self.subscriptions.drain(..) {
            subscription.dispose();
        }
    }
}

/// O chevron da margem (`.cldr`): começa 2 px à direita dos números de linha, numa caixa de 26 px com o codicon de
/// 16 px no centro (medido no oráculo).
const CHEVRON_OFFSET: f64 = 2.0;
const CHEVRON_BOX: f64 = 26.0;
const CHEVRON_SIZE: f32 = 16.0;

impl crate::view::EditorView {
    /// `editor.wordWrap` atual.
    pub fn word_wrap(&self) -> WordWrap {
        self.word_wrap
    }

    /// Troca `editor.wordWrap`: o `ViewModel` recalcula as linhas da view e a rolagem horizontal volta ao começo.
    pub fn set_word_wrap(&mut self, wrap: WordWrap) {
        if self.word_wrap == wrap {
            return;
        }
        self.word_wrap = wrap;
        use je_base::dynamic::Dynamic;
        self.core.widget.update_options(Some(&Dynamic::object([("wordWrap", Dynamic::from(wrap.as_str()))])));
        if wrap != WordWrap::Off {
            let top = self.scroll_top();
            self.scroll_by(-self.scroll_left(), 0.0);
            self.set_scroll_top(top);
        }
        self.shared.invalid_all.set(true);
        self.reveal_cursor();
        self.mark_dirty();
    }

    /// `View: Toggle Word Wrap` (Alt+Z).
    pub fn toggle_word_wrap(&mut self) {
        self.set_word_wrap(self.word_wrap.toggled());
    }

    /// Roda uma ação de dobra e acerta a view: as linhas da view mudaram e o cursor fica à vista.
    fn folding_action(&mut self, action: impl FnOnce(&Folding, &EditorCore)) {
        action(&self.folding, &self.core);
        self.shared.invalid_all.set(true);
        self.reveal_cursor();
        self.reset_blink();
        self.mark_dirty();
    }

    /// `editor.fold` (Ctrl+Shift+[).
    pub fn fold(&mut self) {
        self.folding_action(Folding::fold);
    }

    /// `editor.unfold` (Ctrl+Shift+]).
    pub fn unfold(&mut self) {
        self.folding_action(Folding::unfold);
    }

    /// `editor.foldAll` (Ctrl+K Ctrl+0).
    pub fn fold_all(&mut self) {
        self.folding_action(|f, c| f.set_all(c, true));
    }

    /// `editor.unfoldAll` (Ctrl+K Ctrl+J).
    pub fn unfold_all(&mut self) {
        self.folding_action(|f, c| f.set_all(c, false));
    }

    /// Dobra ou desdobra a região que começa na linha `line` do modelo (o clique no chevron). Devolve se havia região.
    pub fn toggle_fold_at(&mut self, line: i32, recursive: bool) -> bool {
        self.folding.update(&self.core);
        let toggled = self.folding.toggle_at(&self.core, line, recursive);
        if toggled {
            self.shared.invalid_all.set(true);
            self.mark_dirty();
        }
        toggled
    }

    /// O ponteiro passou sobre a view (sem botão apertado): os chevrons de dobra aparecem com ele sobre a margem.
    pub fn mouse_move(&mut self, x: f64, y: f64) {
        let over = x >= 0.0 && y >= 0.0 && y < self.layout.height && x < self.layout.content_left();
        if over != self.hover_margin {
            self.hover_margin = over;
            self.mark_dirty();
        }
    }

    /// O clique na faixa dos chevrons (`GUTTER_LINE_DECORATIONS`): a linha do modelo da região que começa ali.
    pub(crate) fn chevron_line_at(&mut self, x: f64, y: f64) -> Option<i32> {
        let left = self.layout.line_numbers_left() + self.layout.line_numbers_width();
        if x < left || x >= self.layout.content_left() {
            return None;
        }
        self.folding.update(&self.core);
        let view = self.view_position_at(x, y);
        let model = self.to_model(je_editor::position::Position::new(view.line_number, 1));
        // Só a primeira linha da view da linha do modelo tem o chevron.
        if self.to_view(je_editor::position::Position::new(model.line_number, 1)).line_number != view.line_number {
            return None;
        }
        self.folding.region_starting_at(model.line_number).map(|_| model.line_number)
    }

    /// O clique no fim de uma linha dobrada (na reticência) desdobra a região (`unfoldOnClickAfterEndOfLine`).
    pub(crate) fn collapsed_line_end(&self, position: je_editor::position::Position) -> Option<i32> {
        let (_, collapsed) = self.folding.region_starting_at(position.line_number)?;
        (collapsed && position.column == self.model.get_line_max_column(position.line_number)).then_some(position.line_number)
    }

    pub(crate) fn paint_fold_backgrounds(&self, list: &mut je_render::DisplayList, rect: &dyn Fn(f64, f64, f64, f64) -> je_render::kurbo::Rect, laid_out: &[(i32, std::rc::Rc<crate::view::LineLayout>)], scroll_top: f64) {
        if !self.folding.has_hidden() {
            return;
        }
        let (left, right, lh) = (self.layout.content_left(), self.layout.text_right(), self.layout.line_height());
        for (line, layout) in laid_out {
            if layout.first && self.folding.region_starting_at(layout.model_line).is_some_and(|(_, collapsed)| collapsed) {
                // Todas as linhas da view da primeira linha da região (o `isWholeLine` da decoração).
                let top = self.layout.line_top(*line, scroll_top);
                let mut bottom = top + lh;
                let mut next = *line;
                while laid_out.iter().any(|(l, lay)| *l == next && lay.continues) {
                    next += 1;
                    bottom += lh;
                }
                list.fill_rect(rect(left, top, right, bottom), self.colors.fold_background);
            }
        }
    }

    pub(crate) fn paint_fold_ellipsis(&self, list: &mut je_render::DisplayList, origin: (f64, f64), scale: f64, laid_out: &[(i32, std::rc::Rc<crate::view::LineLayout>)], scroll_top: f64, scroll_left: f64) {
        if !self.folding.has_hidden() {
            return;
        }
        let style = self.metrics.style(scale, false, false);
        let lh = self.layout.line_height();
        for (index, (line, layout)) in laid_out.iter().enumerate() {
            // A reticência vai no fim da última linha da view da primeira linha da região dobrada.
            if layout.continues {
                continue;
            }
            let start = laid_out[..=index].iter().rev().find(|(_, l)| l.first).map(|(_, l)| l.model_line).unwrap_or(layout.model_line);
            if !self.folding.region_starting_at(start).is_some_and(|(_, collapsed)| collapsed) {
                continue;
            }
            let text = je_text::with_text_system(|t| t.line("\u{22EF}", &style));
            // `margin-left: 0.2em`.
            let x = self.layout.content_left() + layout.width + 0.2 * f64::from(self.metrics.size) - scroll_left;
            let top = (origin.1 + self.layout.line_top(*line, scroll_top)) * scale;
            let baseline = (top.round() + ((lh * scale - f64::from(text.line_height)) / 2.0).floor() + f64::from(text.baseline)).round();
            text.paint(list, ((origin.0 + x) * scale).round() as f32, (baseline - f64::from(text.baseline)) as f32, self.colors.fold_placeholder);
        }
    }

    pub(crate) fn paint_fold_chevrons(&self, list: &mut je_render::DisplayList, origin: (f64, f64), scale: f64, laid_out: &[(i32, std::rc::Rc<crate::view::LineLayout>)], scroll_top: f64) {
        let left = self.layout.line_numbers_left() + self.layout.line_numbers_width() + CHEVRON_OFFSET;
        let lh = self.layout.line_height();
        let px = (f64::from(CHEVRON_SIZE) * scale) as f32;
        for (line, layout) in laid_out {
            if !layout.first {
                continue;
            }
            let Some((_, collapsed)) = self.folding.region_starting_at(layout.model_line) else { continue };
            // `showFoldingControls: mouseover`: os expandidos só com o ponteiro na margem; os dobrados sempre.
            if !collapsed && !self.hover_margin {
                continue;
            }
            let id = if collapsed { "chevron-right" } else { "chevron-down" };
            let Some(glyph) = je_text::with_text_system(|t| t.codicon(id, px)) else { continue };
            let top = self.layout.line_top(*line, scroll_top);
            let x = (origin.0 + left) * scale + (CHEVRON_BOX * scale - f64::from(glyph.width)) / 2.0;
            let y = (origin.1 + top) * scale + (lh * scale - f64::from(glyph.line_height)) / 2.0;
            glyph.paint(list, x.round() as f32, y.round() as f32, self.colors.folding_control);
        }
    }
}
