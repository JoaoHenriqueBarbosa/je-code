//! Porte de `src/vs/editor/browser/widget/diffEditor/components/accessibleDiffViewer.ts`: o visualizador acessível do editor de diff,
//! que mostra uma diferença por vez (com as linhas de contexto ao redor) como uma lista navegável pelo teclado.
//!
//! Divergências:
//! - **DOM vira dado.** A visibilidade do `parentNode` (`style.visibility`) é [`AccessibleDiffViewer::parent_visible`]. O `View` produz o
//!   [`AccessibleDiffViewerViewState`]: o tamanho (`applyStyle`), a ação de fechar do `ActionBar`, e a tabela (`div.diff-review-table`)
//!   com o rótulo de acessibilidade, a fonte do editor modificado (`applyFontInfo`) e uma [`AccessibleDiffViewerRow`] por linha, com
//!   o `tabIndex` e o foco. Os campos de cada linha (números de linha, largura, ícone do espaçador, rótulo de acessibilidade) são os
//!   atributos e filhos do `div.diff-review-row`; a classe da linha (`line-insert`, `line-delete`) e a dos números (`char-insert`,
//!   `char-delete`) saem do [`AccessibleDiffViewerLineType`].
//! - **HTML da linha.** O `_getLineHtml` devolve o `html` do `renderViewLine2` (sem o `TrustedTypes` do `_ttPolicy`, que só existe no
//!   navegador): esse HTML é a especificação das partes que o pintor desenha, como diz `view_line_renderer`, e nenhum HTML é
//!   interpretado em tempo de execução. É o campo [`AccessibleDiffViewerLineRow::line_html`].
//! - **Eventos viram métodos.** O `keydown` do `domNode` é [`View::on_key_down`] (com o [`AccessibleDiffViewerKeyEvent`] já resolvido
//!   para o `StandardKeyboardEvent.equals`: o `ctrl_cmd` é Ctrl no Windows e no Linux e Cmd no Mac), o `focus` de cada linha é
//!   [`View::on_row_focus`] e o `run` da ação de fechar é [`View::on_close_click`]. Os que o original termina com `preventDefault()`
//!   devolvem se consumiram o evento.
//! - **`DomScrollableElement`.** A barra de rolagem do conteúdo (`scanDomNode` quando muda o tamanho e depois de renderizar) é do
//!   pintor; o dado é só o tamanho.
//! - **Ícones.** `registerIcon('diff-review-insert', Codicon.add, ...)` e os outros dois são os `ThemeIcon` com esses ids (o registro
//!   de ícones guarda só a descrição e o ícone padrão).
//! - **`IAccessbilitySignalOptions.source`** não existe no `playSignal` do porte; só o sinal é tocado.
//! - **`ViewElementGroup.range`.** O original monta o `LineRangeMapping` com o intervalo do modificado no lugar do original e vice-versa
//!   (`new LineRangeMapping(modifiedRange, originalRange)`); o porte copia a ordem dos argumentos, que `groups.findIndex` e o cabeçalho
//!   leem como o original lê.
//! - **Identidade de `ViewElement`.** `currentElement === viewItem` e `lines.indexOf(line)` comparam objetos; os elementos são `Rc` e a
//!   comparação é a identidade do `Rc`. O `ViewElement` do TypeScript (a união de quatro classes) é o enum [`ViewElement`].
//! - **`ILanguageService`** vem do serviço de instanciação; só o `languageIdCodec` é usado.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::arrays::{for_each_adjacent, group_adjacent_by};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value_owned};
use je_base::observable::{
    DebugOwner, IObservable, IReader, IReaderWithStore, ITransaction, Observable, ObservableExt, SettableExt, autorun, autorun_with_store, derived,
    recompute_initially_and_on_change, subtransaction, transaction,
};
use je_base::themables::ThemeIcon;
use je_instantiation::InstantiationService;

use crate::code_action::platform::{AccessibilitySignal, I_ACCESSIBILITY_SIGNAL_SERVICE, IAccessibilitySignalService};
use crate::code_editor_widget::{ICodeEditor, SelectionOrRange};
use crate::config::editor_options::ComputedEditorOptions;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::font_ligatures::{EditorFontLigatures, FONT_LIGATURES};
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::config::font_info::FontInfo;
use crate::cursor_common::CursorSimpleModel;
use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping};
use crate::diff_editor::components::diff_editor_editors::DiffEditorEditors;
use crate::diff_editor::feature_contracts::{IAccessibleDiffViewer, SetAccessibleDiffViewerVisible};
use crate::inline_completions::utils::dispose_handle;
use crate::inline_completions::view::inline_edits::inline_edits_views::inline_edits_line_replacement_view::{
    DISABLE_MONOSPACE_OPTIMIZATIONS, RENDER_CONTROL_CHARACTERS, RENDER_WHITESPACE, STOP_RENDERING_LINE_AFTER,
};
use crate::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use crate::languages::ILanguageIdCodec;
use crate::line_range::LineRange;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::view_line_renderer::{RenderLineInput, RenderLineInputOptions, render_view_line2};
use crate::view_model_types::ViewLineRenderingData;

/// `accessibleDiffViewerInsertIcon` (`diff-review-insert`).
fn accessible_diff_viewer_insert_icon() -> ThemeIcon {
    ThemeIcon::from_id("diff-review-insert")
}

/// `accessibleDiffViewerRemoveIcon` (`diff-review-remove`).
fn accessible_diff_viewer_remove_icon() -> ThemeIcon {
    ThemeIcon::from_id("diff-review-remove")
}

/// `accessibleDiffViewerCloseIcon` (`diff-review-close`).
fn accessible_diff_viewer_close_icon() -> ThemeIcon {
    ThemeIcon::from_id("diff-review-close")
}

/// `IAccessibleDiffViewerModel`.
pub trait IAccessibleDiffViewerModel {
    fn get_original_model(&self) -> Rc<TextModel>;
    fn get_original_options(&self) -> &ComputedEditorOptions;

    /// Should do: `setSelection`, `revealLine` and `focus`
    fn original_reveal(&self, range: Range);

    fn get_modified_model(&self) -> Rc<TextModel>;
    fn get_modified_options(&self) -> &ComputedEditorOptions;
    /// Should do: `setSelection`, `revealLine` and `focus`
    fn modified_reveal(&self, range: Option<Range>);
    fn modified_set_selection(&self, range: Range);
    fn modified_focus(&self);

    fn get_modified_position(&self) -> Option<Position>;
}

/// `AccessibleDiffViewerModelFromEditors`.
pub struct AccessibleDiffViewerModelFromEditors {
    editors: Rc<DiffEditorEditors>,
}

impl AccessibleDiffViewerModelFromEditors {
    pub fn new(editors: Rc<DiffEditorEditors>) -> Self {
        Self { editors }
    }
}

impl IAccessibleDiffViewerModel for AccessibleDiffViewerModelFromEditors {
    fn get_original_model(&self) -> Rc<TextModel> {
        self.editors.original.get_model().expect("o editor original tem modelo")
    }

    fn get_original_options(&self) -> &ComputedEditorOptions {
        self.editors.original.get_options()
    }

    fn original_reveal(&self, range: Range) {
        self.editors.original.reveal_range(range, None, None, None);
        self.editors.original.set_selection(SelectionOrRange::from(range), None);
        self.editors.original.focus();
    }

    fn get_modified_model(&self) -> Rc<TextModel> {
        self.editors.modified.get_model().expect("o editor modificado tem modelo")
    }

    fn get_modified_options(&self) -> &ComputedEditorOptions {
        self.editors.modified.get_options()
    }

    fn modified_reveal(&self, range: Option<Range>) {
        if let Some(range) = range {
            self.editors.modified.reveal_range(range, None, None, None);
            self.editors.modified.set_selection(SelectionOrRange::from(range), None);
        }
        self.editors.modified.focus();
    }

    fn modified_set_selection(&self, range: Range) {
        self.editors.modified.set_selection(SelectionOrRange::from(range), None);
    }

    fn modified_focus(&self) {
        self.editors.modified.focus();
    }

    fn get_modified_position(&self) -> Option<Position> {
        self.editors.modified.get_position()
    }
}

/// O que o `_state` derivado guarda: o modelo e a visão.
struct AccessibleDiffViewerState {
    model: Rc<ViewModel>,
    view: Rc<View>,
}

/// `AccessibleDiffViewer`.
pub struct AccessibleDiffViewer {
    base: Rc<Disposable>,
    visible: IObservable<bool>,
    set_visible: SetAccessibleDiffViewerVisible,
    state: IObservable<Option<Rc<AccessibleDiffViewerState>>>,
    parent_visible: Rc<Cell<bool>>,
}

impl AccessibleDiffViewer {
    /// `new AccessibleDiffViewer(parentNode, visible, setVisible, canClose, width, height, diffs, models, instantiationService)`. O
    /// `parentNode` é o estado de visibilidade [`AccessibleDiffViewer::parent_visible`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        visible: IObservable<bool>,
        set_visible: SetAccessibleDiffViewerVisible,
        can_close: IObservable<bool>,
        width: IObservable<f64>,
        height: IObservable<f64>,
        diffs: IObservable<Option<Rc<Vec<DetailedLineRangeMapping>>>>,
        models: Rc<dyn IAccessibleDiffViewerModel>,
        instantiation_service: &Rc<InstantiationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let (signal_service, language_service) =
            instantiation_service.invoke_function(|accessor| (accessor.get(I_ACCESSIBILITY_SIGNAL_SERVICE), accessor.get(I_LANGUAGE_SERVICE)));
        let parent_visible = Rc::new(Cell::new(false));

        let state: IObservable<Option<Rc<AccessibleDiffViewerState>>> = {
            let (visible, set_visible, parent_visible) = (visible.clone(), set_visible.clone(), parent_visible.clone());
            derived(move |reader| {
                let visible = visible.read(reader);
                parent_visible.set(visible);
                if !visible {
                    return None;
                }
                let model = ViewModel::new(diffs.clone(), models.clone(), set_visible.clone(), can_close.clone(), signal_service.clone());
                reader.store().add(dispose_handle(&model));
                let view = View::new(model.clone(), width.clone(), height.clone(), models.clone(), language_service.clone());
                reader.store().add(dispose_handle(&view));
                Some(Rc::new(AccessibleDiffViewerState { model, view }))
            })
        };
        base.register(recompute_initially_and_on_change(&state, None));

        Rc::new(Self { base, visible, set_visible, state, parent_visible })
    }

    /// `parentNode.style.visibility`: `visible` ou `hidden`.
    pub fn parent_visible(&self) -> bool {
        self.parent_visible.get()
    }

    /// A visão atual, quando o visualizador está visível.
    pub fn view(&self) -> Option<Rc<View>> {
        self.state.get().map(|state| state.view.clone())
    }

    /// O modelo atual, quando o visualizador está visível.
    pub fn model(&self) -> Option<Rc<ViewModel>> {
        self.state.get().map(|state| state.model.clone())
    }
}

impl IAccessibleDiffViewer for AccessibleDiffViewer {
    fn next(&self) {
        transaction(|tx| {
            let is_visible = self.visible.get();
            (self.set_visible)(true, Some(tx));
            if is_visible {
                self.state.get().expect("o visualizador visível tem estado").model.next_group(Some(tx));
            }
        });
    }

    fn prev(&self) {
        transaction(|tx| {
            (self.set_visible)(true, Some(tx));
            self.state.get().expect("o visualizador visível tem estado").model.previous_group(Some(tx));
        });
    }

    fn close(&self) {
        transaction(|tx| {
            (self.set_visible)(false, Some(tx));
        });
    }
}

impl IDisposable for AccessibleDiffViewer {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ViewModel`.
pub struct ViewModel {
    base: Rc<Disposable>,
    groups: Rc<ObservableValue<Rc<Vec<Rc<ViewElementGroup>>>, ()>>,
    current_group_idx: Rc<ObservableValue<i32, ()>>,
    current_element_idx: Rc<ObservableValue<i32, ()>>,
    pub current_group: IObservable<Option<Rc<ViewElementGroup>>>,
    pub current_element: IObservable<Option<Rc<ViewElement>>>,
    pub can_close: IObservable<bool>,
    models: Rc<dyn IAccessibleDiffViewerModel>,
    set_visible: SetAccessibleDiffViewerVisible,
}

impl ViewModel {
    pub fn new(
        diffs: IObservable<Option<Rc<Vec<DetailedLineRangeMapping>>>>,
        models: Rc<dyn IAccessibleDiffViewerModel>,
        set_visible: SetAccessibleDiffViewerVisible,
        can_close: IObservable<bool>,
        accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    ) -> Rc<Self> {
        let owner = DebugOwner::new("ViewModel");
        let base = Disposable::new();
        let groups = observable_value_owned::<Rc<Vec<Rc<ViewElementGroup>>>, ()>(owner.clone(), Rc::new(Vec::new()));
        let current_group_idx = observable_value_owned::<i32, ()>(owner.clone(), 0);
        let current_element_idx = observable_value_owned::<i32, ()>(owner, 0);

        let current_group: IObservable<Option<Rc<ViewElementGroup>>> = {
            let groups = groups.clone();
            current_group_idx.map(move |idx, r| groups.read(r).get(idx as usize).cloned())
        };
        let current_element: IObservable<Option<Rc<ViewElement>>> = {
            let current_group = current_group.clone();
            current_element_idx.map(move |idx, r| current_group.read(r).and_then(|group| group.lines.get(idx as usize).cloned()))
        };

        {
            // update groups
            let (diffs, models, groups, current_group_idx) = (diffs, models.clone(), groups.clone(), current_group_idx.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let Some(diffs) = diffs.read(reader) else {
                    groups.set(Rc::new(Vec::new()), None);
                    return;
                };

                let new_groups = compute_view_element_groups(&diffs, models.get_original_model().get_line_count(), models.get_modified_model().get_line_count());

                transaction(|tx| {
                    let p = models.get_modified_position();
                    if let Some(p) = p {
                        let next_group = new_groups.iter().position(|g| p.line_number < g.range.modified.end_line_number_exclusive);
                        if let Some(next_group) = next_group {
                            current_group_idx.set(next_group as i32, Some(tx));
                        }
                    }
                    groups.set(Rc::new(new_groups), Some(tx));
                });
            }));
        }

        {
            // play audio-cue for diff
            let current_element = current_element.clone();
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                match current_element.read(reader).as_deref() {
                    Some(ViewElement::Deleted { .. }) => accessibility_signal_service.play_signal(AccessibilitySignal::DiffLineDeleted),
                    Some(ViewElement::Added { .. }) => accessibility_signal_service.play_signal(AccessibilitySignal::DiffLineInserted),
                    _ => {}
                }
            }));
        }

        {
            // select lines in editor
            // This ensures editor commands (like revert/stage) work
            let (current_element, models) = (current_element.clone(), models.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let Some(current_view_item) = current_element.read(reader) else { return };
                let line_number = match &*current_view_item {
                    ViewElement::Header => return,
                    ViewElement::Unchanged { modified_line_number, .. } | ViewElement::Added { modified_line_number, .. } => *modified_line_number,
                    ViewElement::Deleted { diff, .. } => diff.modified.start_line_number,
                };
                let position = Position::new(line_number, 1);
                models.modified_set_selection(Range::from_positions(position, position));
            }));
        }

        Rc::new(Self { base, groups, current_group_idx, current_element_idx, current_group, current_element, can_close, models, set_visible })
    }

    /// `groups`.
    pub fn groups(&self) -> IObservable<Rc<Vec<Rc<ViewElementGroup>>>> {
        self.groups.to_observable()
    }

    /// `currentGroupIndex`.
    pub fn current_group_index(&self) -> IObservable<i32> {
        self.current_group_idx.to_observable()
    }

    /// `_goToGroupDelta(delta, tx?)`.
    fn go_to_group_delta(&self, delta: i32, tx: Option<&dyn ITransaction>) {
        let groups = self.groups.get();
        if groups.len() <= 1 {
            return;
        }
        subtransaction(tx, |tx| {
            self.current_group_idx.set(OffsetRange::of_length(groups.len() as i32).clip_cyclic(self.current_group_idx.get() + delta), Some(tx));
            self.current_element_idx.set(0, Some(tx));
        });
    }

    pub fn next_group(&self, tx: Option<&dyn ITransaction>) {
        self.go_to_group_delta(1, tx);
    }

    pub fn previous_group(&self, tx: Option<&dyn ITransaction>) {
        self.go_to_group_delta(-1, tx);
    }

    /// `_goToLineDelta(delta)`.
    fn go_to_line_delta(&self, delta: i32) {
        let Some(group) = self.current_group.get() else { return };
        if group.lines.len() <= 1 {
            return;
        }
        transaction(|tx| {
            self.current_element_idx.set(OffsetRange::of_length(group.lines.len() as i32).clip(self.current_element_idx.get() + delta), Some(tx));
        });
    }

    pub fn go_to_next_line(&self) {
        self.go_to_line_delta(1);
    }

    pub fn go_to_previous_line(&self) {
        self.go_to_line_delta(-1);
    }

    pub fn go_to_line(&self, line: &Rc<ViewElement>) {
        let Some(group) = self.current_group.get() else { return };
        let Some(idx) = group.lines.iter().position(|l| Rc::ptr_eq(l, line)) else { return };
        transaction(|tx| {
            self.current_element_idx.set(idx as i32, Some(tx));
        });
    }

    pub fn reveal_current_element_in_editor(&self) {
        if !self.can_close.get() {
            return;
        }
        (self.set_visible)(false, None);

        let Some(cur_elem) = self.current_element.get() else { return };
        match &*cur_elem {
            ViewElement::Deleted { original_line_number, .. } => {
                let position = Position::new(*original_line_number, 1);
                self.models.original_reveal(Range::from_positions(position, position));
            }
            ViewElement::Header => self.models.modified_reveal(None),
            ViewElement::Unchanged { modified_line_number, .. } | ViewElement::Added { modified_line_number, .. } => {
                let position = Position::new(*modified_line_number, 1);
                self.models.modified_reveal(Some(Range::from_positions(position, position)));
            }
        }
    }

    pub fn close(&self) {
        if !self.can_close.get() {
            return;
        }
        (self.set_visible)(false, None);
        self.models.modified_focus();
    }
}

impl IDisposable for ViewModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `viewElementGroupLineMargin`.
const VIEW_ELEMENT_GROUP_LINE_MARGIN: i32 = 3;

/// `computeViewElementGroups(diffs, originalLineCount, modifiedLineCount)`.
fn compute_view_element_groups(diffs: &[DetailedLineRangeMapping], original_line_count: i32, modified_line_count: i32) -> Vec<Rc<ViewElementGroup>> {
    let mut result: Vec<Rc<ViewElementGroup>> = Vec::new();

    for g in group_adjacent_by(diffs.iter().cloned(), |a, b| b.modified.start_line_number - a.modified.end_line_number_exclusive < 2 * VIEW_ELEMENT_GROUP_LINE_MARGIN) {
        let mut view_elements: Vec<Rc<ViewElement>> = vec![Rc::new(ViewElement::Header)];
        let (first, last) = (&g[0], &g[g.len() - 1]);

        let orig_full_range = LineRange::new(
            (first.original.start_line_number - VIEW_ELEMENT_GROUP_LINE_MARGIN).max(1),
            (last.original.end_line_number_exclusive + VIEW_ELEMENT_GROUP_LINE_MARGIN).min(original_line_count + 1),
        );
        let modified_full_range = LineRange::new(
            (first.modified.start_line_number - VIEW_ELEMENT_GROUP_LINE_MARGIN).max(1),
            (last.modified.end_line_number_exclusive + VIEW_ELEMENT_GROUP_LINE_MARGIN).min(modified_line_count + 1),
        );

        for_each_adjacent(&g, |a, b| {
            let orig_range = LineRange::new(
                a.map_or(orig_full_range.start_line_number, |a| a.original.end_line_number_exclusive),
                b.map_or(orig_full_range.end_line_number_exclusive, |b| b.original.start_line_number),
            );
            let modified_range = LineRange::new(
                a.map_or(modified_full_range.start_line_number, |a| a.modified.end_line_number_exclusive),
                b.map_or(modified_full_range.end_line_number_exclusive, |b| b.modified.start_line_number),
            );

            orig_range.for_each(|orig_line_number| {
                view_elements.push(Rc::new(ViewElement::Unchanged {
                    original_line_number: orig_line_number,
                    modified_line_number: modified_range.start_line_number + (orig_line_number - orig_range.start_line_number),
                }));
            });

            if let Some(b) = b {
                b.original.for_each(|orig_line_number| {
                    view_elements.push(Rc::new(ViewElement::Deleted { diff: b.clone(), original_line_number: orig_line_number }));
                });
                b.modified.for_each(|modified_line_number| {
                    view_elements.push(Rc::new(ViewElement::Added { diff: b.clone(), modified_line_number }));
                });
            }
        });

        let modified_range = first.modified.join(&last.modified);
        let original_range = first.original.join(&last.original);

        result.push(Rc::new(ViewElementGroup { range: LineRangeMapping::new(modified_range, original_range), lines: view_elements }));
    }
    result
}

/// `LineType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineType {
    Header,
    Unchanged,
    Deleted,
    Added,
}

/// `ViewElementGroup`.
pub struct ViewElementGroup {
    pub range: LineRangeMapping,
    pub lines: Vec<Rc<ViewElement>>,
}

/// `ViewElement = HeaderViewElement | UnchangedLineViewElement | DeletedLineViewElement | AddedLineViewElement`.
pub enum ViewElement {
    Header,
    Unchanged { original_line_number: i32, modified_line_number: i32 },
    /// `modifiedLineNumber` é `undefined`.
    Deleted { diff: DetailedLineRangeMapping, original_line_number: i32 },
    /// `originalLineNumber` é `undefined`.
    Added { diff: DetailedLineRangeMapping, modified_line_number: i32 },
}

impl ViewElement {
    /// `type`.
    pub fn line_type(&self) -> LineType {
        match self {
            ViewElement::Header => LineType::Header,
            ViewElement::Unchanged { .. } => LineType::Unchanged,
            ViewElement::Deleted { .. } => LineType::Deleted,
            ViewElement::Added { .. } => LineType::Added,
        }
    }

    /// `originalLineNumber`.
    pub fn original_line_number(&self) -> Option<i32> {
        match self {
            ViewElement::Unchanged { original_line_number, .. } | ViewElement::Deleted { original_line_number, .. } => Some(*original_line_number),
            ViewElement::Header | ViewElement::Added { .. } => None,
        }
    }

    /// `modifiedLineNumber`.
    pub fn modified_line_number(&self) -> Option<i32> {
        match self {
            ViewElement::Unchanged { modified_line_number, .. } | ViewElement::Added { modified_line_number, .. } => Some(*modified_line_number),
            ViewElement::Header | ViewElement::Deleted { .. } => None,
        }
    }
}

/// O tipo de uma linha da tabela (o `LineType` sem o cabeçalho).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibleDiffViewerLineType {
    Unchanged,
    Deleted,
    Added,
}

/// O `div.diff-review-row` de uma linha de código.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibleDiffViewerLineRow {
    pub line_type: AccessibleDiffViewerLineType,
    /// `row.style.minWidth`.
    pub min_width: f64,
    /// `cell.style.height`.
    pub cell_height: f64,
    /// O número de linha do original; `None` mostra ` `.
    pub original_line_number: Option<i32>,
    /// `style.width` e `style.minWidth` do número do original.
    pub original_line_numbers_width: f64,
    /// O número de linha do modificado; `None` mostra ` `.
    pub modified_line_number: Option<i32>,
    /// `style.width` e `style.minWidth` do número do modificado.
    pub modified_line_numbers_width: f64,
    /// `style.paddingRight` do número do modificado.
    pub modified_padding_right: f64,
    /// O `span.codicon` do `div.diff-review-spacer` (`None` mostra só `  `).
    pub spacer_icon: Option<ThemeIcon>,
    /// O `html` do `renderViewLine2` da linha.
    pub line_html: String,
    /// `aria-label`.
    pub aria_label: String,
}

/// O cabeçalho (`div.diff-review-summary`) de uma diferença.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibleDiffViewerHeaderRow {
    /// `aria-label`.
    pub aria_label: String,
    /// O texto do `diff-review-cell`: `1/10: @@ -504,7 +517,7 @@`.
    pub summary: String,
}

/// O conteúdo de uma linha da tabela.
#[derive(Clone, Debug, PartialEq)]
pub enum AccessibleDiffViewerRowContent {
    Header(AccessibleDiffViewerHeaderRow),
    Line(AccessibleDiffViewerLineRow),
}

/// Uma linha da tabela, com o `tabIndex` e o foco que o autorun de seleção grava.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibleDiffViewerRow {
    pub content: AccessibleDiffViewerRowContent,
    /// `row.tabIndex`: 0 na linha selecionada, -1 nas outras.
    pub tab_index: i32,
    /// A linha selecionada recebe o foco (`row.focus()`).
    pub focused: bool,
}

/// A ação do `ActionBar` (a de fechar).
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibleDiffViewerAction {
    pub id: &'static str,
    pub label: String,
    /// `class`: `close-diff-review` mais a classe do ícone.
    pub class_name: String,
    pub enabled: bool,
}

/// O `div.diff-review-table`.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibleDiffViewerTable {
    /// `aria-label`.
    pub aria_label: String,
    /// `applyFontInfo(container, modifiedOptions.get(EditorOption.fontInfo))`.
    pub font_info: FontInfo,
    pub rows: Vec<AccessibleDiffViewerRow>,
}

/// O que o pintor lê do `View`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessibleDiffViewerViewState {
    /// `applyStyle(domNode | content, { width, height })`.
    pub width: f64,
    pub height: f64,
    /// As ações do `ActionBar`, só com ícone e sem rótulo.
    pub actions: Vec<AccessibleDiffViewerAction>,
    pub table: Option<AccessibleDiffViewerTable>,
}

/// O `StandardKeyboardEvent` do `keydown`, com os modificadores já resolvidos para `equals`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibleDiffViewerKeyEvent {
    pub key_code: KeyCode,
    /// `KeyMod.CtrlCmd`: Ctrl no Windows e no Linux, Cmd no Mac.
    pub ctrl_cmd: bool,
    pub shift: bool,
    pub alt: bool,
    /// `KeyMod.WinCtrl`.
    pub win_ctrl: bool,
}

impl AccessibleDiffViewerKeyEvent {
    /// `e.equals(keybinding)`: a tecla e exatamente os modificadores dados.
    fn equals(&self, key_code: KeyCode, ctrl_cmd: bool, shift: bool, alt: bool) -> bool {
        self.key_code == key_code && self.ctrl_cmd == ctrl_cmd && self.shift == shift && self.alt == alt && !self.win_ctrl
    }
}

/// `View`.
pub struct View {
    base: Rc<Disposable>,
    model: Rc<ViewModel>,
    width: IObservable<f64>,
    models: Rc<dyn IAccessibleDiffViewerModel>,
    language_service: Rc<dyn ILanguageService>,
    state: Rc<RefCell<AccessibleDiffViewerViewState>>,
}

impl View {
    pub fn new(
        model: Rc<ViewModel>,
        width: IObservable<f64>,
        height: IObservable<f64>,
        models: Rc<dyn IAccessibleDiffViewerModel>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let state: Rc<RefCell<AccessibleDiffViewerViewState>> = Rc::default();

        {
            // update actions
            let (state, can_close) = (state.clone(), model.can_close.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let actions = if can_close.read(reader) {
                    vec![AccessibleDiffViewerAction {
                        id: "diffreview.close",
                        label: localize("label.close", "Close", &[]),
                        class_name: format!("close-diff-review {}", accessible_diff_viewer_close_icon().as_class_name()),
                        enabled: true,
                    }]
                } else {
                    Vec::new()
                };
                state.borrow_mut().actions = actions;
            }));
        }

        {
            // applyStyle(this.domNode, { width, height }) e applyStyle(this._content, { width, height })
            let (state, width, height) = (state.clone(), width.clone(), height.clone());
            base.register(autorun(move |reader| {
                let reader: &dyn IReader = reader;
                let (width, height) = (width.read(reader), height.read(reader));
                let mut state = state.borrow_mut();
                state.width = width;
                state.height = height;
            }));
        }

        let view = Rc::new(Self { base, model, width, models, language_service, state });

        {
            // render
            let weak = Rc::downgrade(&view);
            view.base.register(autorun_with_store(move |reader, store| {
                let Some(view) = weak.upgrade() else { return };
                view.model.current_group.read(reader);
                view.render(store);
            }));
        }

        view
    }

    /// O que o pintor lê.
    pub fn state(&self) -> AccessibleDiffViewerViewState {
        self.state.borrow().clone()
    }

    /// O `keydown` do `domNode`: devolve se a tecla foi consumida (`preventDefault()`).
    pub fn on_key_down(&self, e: &AccessibleDiffViewerKeyEvent) -> bool {
        let mut consumed = false;

        if e.equals(KeyCode::DownArrow, false, false, false) || e.equals(KeyCode::DownArrow, true, false, false) || e.equals(KeyCode::DownArrow, false, false, true) {
            consumed = true;
            self.model.go_to_next_line();
        }

        if e.equals(KeyCode::UpArrow, false, false, false) || e.equals(KeyCode::UpArrow, true, false, false) || e.equals(KeyCode::UpArrow, false, false, true) {
            consumed = true;
            self.model.go_to_previous_line();
        }

        if e.equals(KeyCode::Escape, false, false, false)
            || e.equals(KeyCode::Escape, true, false, false)
            || e.equals(KeyCode::Escape, false, false, true)
            || e.equals(KeyCode::Escape, false, true, false)
        {
            consumed = true;
            self.model.close();
        }

        if e.equals(KeyCode::Space, false, false, false) || e.equals(KeyCode::Enter, false, false, false) {
            consumed = true;
            self.model.reveal_current_element_in_editor();
        }

        consumed
    }

    /// O `focus` da linha `index` da tabela: a linha vira o elemento atual.
    pub fn on_row_focus(&self, index: usize) {
        let line = self.model.current_group.get().and_then(|group| group.lines.get(index).cloned());
        if let Some(line) = line {
            self.model.go_to_line(&line);
        }
    }

    /// O `run` da ação `diffreview.close`.
    pub fn on_close_click(&self) {
        self.model.close();
    }

    /// `_render(store)`.
    fn render(&self, store: &DisposableStore) {
        let original_options = self.models.get_original_options();
        let modified_options = self.models.get_modified_options();

        let original_model = self.models.get_original_model();
        let modified_model = self.models.get_modified_model();
        let original_model_opts = original_model.get_options();
        let modified_model_opts = modified_model.get_options();

        let line_height = *modified_options.get(LINE_HEIGHT);
        let group = self.model.current_group.get();
        let mut rows: Vec<AccessibleDiffViewerRow> = Vec::new();
        for view_item in group.as_ref().map_or(&[][..], |group| group.lines.as_slice()) {
            let Some(group) = &group else { break };

            let content = if view_item.line_type() == LineType::Header {
                let r = &group.range;
                let diff_index = self.model.current_group_index().get();
                let diffs_length = self.model.groups().get().len();
                let get_aria_lines = |lines: i32| match lines {
                    0 => localize("no_lines_changed", "no lines changed", &[]),
                    1 => localize("one_line_changed", "1 line changed", &[]),
                    _ => localize("more_lines_changed", "{0} lines changed", &[lines.to_string().as_str()]),
                };

                let original_changed_lines_cnt_aria = get_aria_lines(r.original.length());
                let modified_changed_lines_cnt_aria = get_aria_lines(r.modified.length());
                let aria_label = localize(
                    "header",
                    "Difference {0} of {1}: original line {2}, {3}, modified line {4}, {5}",
                    &[
                        (diff_index + 1).to_string().as_str(),
                        diffs_length.to_string().as_str(),
                        r.original.start_line_number.to_string().as_str(),
                        original_changed_lines_cnt_aria.as_str(),
                        r.modified.start_line_number.to_string().as_str(),
                        modified_changed_lines_cnt_aria.as_str(),
                    ],
                );

                // e.g.: `1/10: @@ -504,7 +517,7 @@`
                let summary = format!(
                    "{}/{}: @@ -{},{} +{},{} @@",
                    diff_index + 1,
                    diffs_length,
                    r.original.start_line_number,
                    r.original.length(),
                    r.modified.start_line_number,
                    r.modified.length()
                );
                AccessibleDiffViewerRowContent::Header(AccessibleDiffViewerHeaderRow { aria_label, summary })
            } else {
                AccessibleDiffViewerRowContent::Line(self.create_row(
                    view_item,
                    line_height,
                    self.width.get(),
                    original_options,
                    &original_model,
                    original_model_opts.tab_size as i32,
                    modified_options,
                    &modified_model,
                    modified_model_opts.tab_size as i32,
                ))
            };

            rows.push(AccessibleDiffViewerRow { content, tab_index: -1, focused: false });
        }

        self.state.borrow_mut().table = Some(AccessibleDiffViewerTable {
            aria_label: localize("ariaLabel", "Accessible Diff Viewer. Use arrow up and down to navigate.", &[]),
            font_info: modified_options.get(FONT_INFO).clone(),
            rows,
        });

        if let Some(group) = &group {
            for (index, view_item) in group.lines.iter().enumerate() {
                let is_selected_obs: IObservable<bool> = {
                    let (model, view_item) = (self.model.clone(), view_item.clone());
                    derived(move |reader| {
                        // isSelected
                        model.current_element.read(reader).is_some_and(|current| Rc::ptr_eq(&current, &view_item))
                    })
                };

                let state = self.state.clone();
                store.add(autorun(move |reader| {
                    // update tab index
                    let reader: &dyn IReader = reader;
                    let is_selected = is_selected_obs.read(reader);
                    if let Some(row) = state.borrow_mut().table.as_mut().and_then(|table| table.rows.get_mut(index)) {
                        row.tab_index = if is_selected { 0 } else { -1 };
                        row.focused = is_selected;
                    }
                }));
            }
        }
    }

    /// `_createRow(item, lineHeight, width, originalOptions, originalModel, originalModelOpts, modifiedOptions, modifiedModel,
    /// modifiedModelOpts)`, com `originalModelOpts.tabSize` e `modifiedModelOpts.tabSize` (a única parte das opções do modelo que a
    /// linha usa).
    #[allow(clippy::too_many_arguments)]
    fn create_row(
        &self,
        item: &ViewElement,
        line_height: f64,
        width: f64,
        original_options: &ComputedEditorOptions,
        original_model: &Rc<TextModel>,
        original_tab_size: i32,
        modified_options: &ComputedEditorOptions,
        modified_model: &Rc<TextModel>,
        modified_tab_size: i32,
    ) -> AccessibleDiffViewerLineRow {
        let original_layout_info = original_options.get(LAYOUT_INFO);
        let original_line_numbers_width = original_layout_info.glyph_margin_width + original_layout_info.line_numbers_width;

        let modified_layout_info = modified_options.get(LAYOUT_INFO);
        let modified_line_numbers_width = 10.0 + modified_layout_info.glyph_margin_width + modified_layout_info.line_numbers_width;

        let (line_type, spacer_icon) = match item {
            ViewElement::Added { .. } => (AccessibleDiffViewerLineType::Added, Some(accessible_diff_viewer_insert_icon())),
            ViewElement::Deleted { .. } => (AccessibleDiffViewerLineType::Deleted, Some(accessible_diff_viewer_remove_icon())),
            ViewElement::Unchanged { .. } | ViewElement::Header => (AccessibleDiffViewerLineType::Unchanged, None),
        };

        let (original_line_number, modified_line_number) = (item.original_line_number(), item.modified_line_number());

        let mut line_content;
        let line_html;
        if let Some(modified_line_number) = modified_line_number {
            line_html = self.get_line_html(modified_model, modified_options, modified_tab_size, modified_line_number);
            line_content = String::from(modified_model.get_line_content(modified_line_number).to_string_lossy());
        } else {
            let original_line_number = original_line_number.expect("uma linha sem número no modificado tem número no original");
            line_html = self.get_line_html(original_model, original_options, original_tab_size, original_line_number);
            line_content = String::from(original_model.get_line_content(original_line_number).to_string_lossy());
        }

        if line_content.is_empty() {
            line_content = localize("blankLine", "blank", &[]);
        }

        let (original_text, modified_text) = (
            original_line_number.map(|n| n.to_string()).unwrap_or_default(),
            modified_line_number.map(|n| n.to_string()).unwrap_or_default(),
        );
        let aria_label = match item {
            ViewElement::Unchanged { original_line_number, modified_line_number } => {
                if original_line_number == modified_line_number {
                    localize("unchangedLine", "{0} unchanged line {1}", &[line_content.as_str(), original_text.as_str()])
                } else {
                    localize("equalLine", "{0} original line {1} modified line {2}", &[line_content.as_str(), original_text.as_str(), modified_text.as_str()])
                }
            }
            ViewElement::Added { .. } => localize("insertLine", "+ {0} modified line {1}", &[line_content.as_str(), modified_text.as_str()]),
            ViewElement::Deleted { .. } => localize("deleteLine", "- {0} original line {1}", &[line_content.as_str(), original_text.as_str()]),
            ViewElement::Header => String::new(),
        };

        AccessibleDiffViewerLineRow {
            line_type,
            min_width: width,
            cell_height: line_height,
            original_line_number,
            original_line_numbers_width,
            modified_line_number,
            modified_line_numbers_width,
            modified_padding_right: 10.0,
            spacer_icon,
            line_html,
            aria_label,
        }
    }

    /// `_getLineHtml(model, options, tabSize, lineNumber, languageIdCodec)`.
    fn get_line_html(&self, model: &Rc<TextModel>, options: &ComputedEditorOptions, tab_size: i32, line_number: i32) -> String {
        let language_id_codec: Rc<dyn ILanguageIdCodec> = self.language_service.language_id_codec();
        let line_content = model.get_line_content(line_number);
        let font_info = options.get(FONT_INFO);
        let vertical_scrollbar_size = options.get(SCROLLBAR).vertical_scrollbar_size;
        let line_tokens: Rc<dyn IViewLineTokens> = Rc::new(LineTokens::create_empty(line_content.clone(), language_id_codec));
        let is_basic_ascii = ViewLineRenderingData::is_basic_ascii(line_content.units(), model.might_contain_non_basic_ascii());
        let contains_rtl = ViewLineRenderingData::contains_rtl(line_content.units(), is_basic_ascii, model.might_contain_rtl());
        let r = render_view_line2(&RenderLineInput::new(RenderLineInputOptions {
            use_monospace_optimizations: font_info.is_monospace && !*options.get(DISABLE_MONOSPACE_OPTIMIZATIONS),
            can_use_halfwidth_rightwards_arrow: font_info.can_use_halfwidth_rightwards_arrow,
            line_content,
            continues_with_wrapped_line: false,
            is_basic_ascii,
            contains_rtl,
            faux_indent_length: 0,
            line_tokens,
            line_decorations: Vec::new(),
            tab_size,
            start_visible_column: 0,
            space_width: font_info.space_width,
            middot_width: font_info.middot_width,
            wsmiddot_width: font_info.wsmiddot_width,
            stop_rendering_line_after: *options.get(STOP_RENDERING_LINE_AFTER) as i32,
            render_whitespace: options.get(RENDER_WHITESPACE).clone(),
            render_control_characters: *options.get(RENDER_CONTROL_CHARACTERS),
            font_ligatures: options.get(FONT_LIGATURES).as_str() != EditorFontLigatures::OFF,
            selections_on_line: None,
            text_direction: None,
            vertical_scrollbar_size,
            render_new_line_when_empty: false,
            use_two_cell_fullwidth_characters: false,
        }));

        String::from(r.html.to_string_lossy())
    }
}

impl IDisposable for View {
    fn dispose(&self) {
        // reset(this.domNode)
        self.state.borrow_mut().table = None;
        self.base.dispose();
    }
}
