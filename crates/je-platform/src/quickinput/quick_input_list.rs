//! Porte de `src/vs/platform/quickinput/browser/quickInputList.ts`: a lista do `QuickPick` (`QuickInputList`), com os elementos
//! de item e de separador, o filtro (fuzzy e contíguo, ciente de ícones), a ordenação por rótulo, o foco, a seleção, os
//! itens marcados e a rolagem.
//!
//! Notas de tradução:
//! - **Árvore e renderizadores.** O original usa uma `WorkbenchObjectTree` com renderizadores de DOM. Aqui a árvore é o modelo
//!   achatado das linhas visíveis ([`Row`]): itens e separadores com botões (os pais dos itens que vêm depois deles), com as
//!   mesmas alturas do `QuickInputItemDelegate` (30 do separador, 44 do item com detalhe, 22 do resto). O desenho nativo lê as
//!   linhas por [`QuickInputList::rows`]. Os atributos de cada linha (`data-quick-input-id`, `not-pickable`) saem de
//!   [`QuickInputListRow`].
//! - **Rolagem.** `scrollTop`, `scrollHeight` e a altura visível seguem o `ListView`: a altura visível é a menor entre o
//!   conteúdo e o `max-height` de `layout()`, e `reveal` rola o mínimo para mostrar a linha (ou a põe no topo com
//!   `relativeTop = 0`).
//! - **Observáveis.** `visibleCount`, `allVisibleChecked`, `checkedCount` e `checkedElements` são `observableValue`: o evento
//!   só dispara quando o valor muda.
//! - **Foco assíncrono de página.** O `focusNextPage` do `ListWidget` espera um `timeout(0)` entre rolar e focar; aqui a
//!   segunda volta roda na hora (o desenho nativo não tem a pintura intermediária que o motivou).
//! - **Dicas (`showHover`, `toggleHover`).** O `WorkbenchHoverDelegate` (`crate::hover::workbench_hover_delegate`) mostra a dica na
//!   hora do desenho; a lista só guarda se há tooltip no elemento focado ([`QuickInputList::focused_tooltip`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::comparers::compare_anything;
use je_base::event::{Emitter, Event};
use je_base::filters::Match;
use je_base::icon_labels::{ParsedLabelWithIcons, get_codicon_aria_label, matches_fuzzy_icon_aware, parse_label_with_icons};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::strings::ltrim;

use super::picker_quick_access::same_item;
use super::quick_input::{
    IQuickPickItem, IQuickPickItemButtonEvent, IQuickPickSeparator, IQuickPickSeparatorButtonEvent, MatchOnLabelMode, QuickInputButtonRef, QuickPickFocus,
    QuickPickInput, QuickPickTooltip,
};
use super::quick_input_box::{QuickInputFocus, QuickInputFocusTarget};
use super::quick_input_browser::{IQuickInputListUi, QuickPickSelectionEvent};
use crate::contextview::view_element::ViewElement;

const SEPARATOR_HEIGHT: f64 = 30.0;
const ITEM_HEIGHT: f64 = 22.0;
const ITEM_WITH_DETAIL_HEIGHT: f64 = 44.0;

/// Uma linha visível da árvore achatada: o índice é o do elemento (item ou separador) dentro do modelo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Item(usize),
    Separator(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Top {
    Item(usize),
    Separator(usize),
}

struct ItemElement {
    index: usize,
    child_index: usize,
    has_checkbox: bool,
    item: IQuickPickItem,
    separator: Option<IQuickPickSeparator>,
    parent: Option<usize>,
    hidden: bool,
    checked: bool,
    sane_label: JsString,
    sane_sort_label: JsString,
    sane_description: Option<JsString>,
    sane_detail: Option<String>,
    label_highlights: Option<Vec<Match>>,
    description_highlights: Option<Vec<Match>>,
    detail_highlights: Option<Vec<Match>>,
}

struct SeparatorElement {
    index: usize,
    separator: IQuickPickSeparator,
    children: Vec<usize>,
    hidden: bool,
}

/// O que o desenho nativo precisa de uma linha (o que os renderizadores de item e de separador escrevem no DOM).
#[derive(Clone, Debug, PartialEq)]
pub struct QuickInputListRow {
    pub row: Row,
    pub label: JsString,
    pub description: Option<JsString>,
    pub detail: Option<String>,
    /// `data-quick-input-id`: ausente quando o item não tem `id` em texto.
    pub data_quick_input_id: Option<String>,
    pub not_pickable: bool,
    pub has_checkbox: bool,
    pub checked: bool,
    pub focused: bool,
    pub selected: bool,
    /// O texto do separador embutido (`.quick-input-list-separator`).
    pub separator_label: Option<String>,
    pub separator_border: bool,
    pub label_highlights: Vec<Match>,
    pub description_highlights: Vec<Match>,
    pub detail_highlights: Vec<Match>,
    pub aria_label: String,
    pub top: f64,
    pub height: f64,
}

struct State {
    input_elements: Vec<QuickPickInput>,
    element_tree: Vec<Top>,
    current: Vec<Top>,
    items: Vec<ItemElement>,
    separators: Vec<SeparatorElement>,
    rows: Vec<Row>,
    focused: Option<usize>,
    selected: Vec<usize>,
    scroll_top: f64,
    max_height: Option<f64>,
    last_query: Option<String>,
    has_checkboxes: bool,
    visible_count: usize,
    all_visible_checked: bool,
    checked_count: usize,
    checked_items: Vec<IQuickPickItem>,
    displayed: bool,
    enabled: bool,
    aria_label: Option<String>,
    match_on_description: bool,
    match_on_detail: bool,
    match_on_label: bool,
    match_on_label_mode: MatchOnLabelMode,
    sort_by_label: bool,
    should_loop: bool,
}

fn items_equal(a: &[IQuickPickItem], b: &[IQuickPickItem]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_item(x, y))
}

fn matches_contiguous(word: &str, word_to_match_against: &JsString) -> Option<Vec<Match>> {
    let haystack = word_to_match_against.to_string().to_lowercase();
    let needle = word.to_lowercase();
    let byte_index = haystack.find(&needle)?;
    let start = haystack[..byte_index].encode_utf16().count();
    Some(vec![Match { start, end: start + word.encode_utf16().count() }])
}

fn matches_contiguous_icon_aware(query: &str, target: &ParsedLabelWithIcons) -> Option<Vec<Match>> {
    let ParsedLabelWithIcons { text, icon_offsets } = target;
    // Return early if there are no icon markers in the word to match against
    let Some(icon_offsets) = icon_offsets.as_ref().filter(|offsets| !offsets.is_empty()) else {
        return matches_contiguous(query, text);
    };
    // Trim the word to match against because it could have leading whitespace now if the word started with an icon
    let trimmed = JsString::from(ltrim(text.units(), &[u16::from(b' ')]));
    let leading_whitespace_offset = text.len() - trimmed.len();
    let mut matches = matches_contiguous(query, &trimmed);
    // Map matches back to offsets with icon and trimming
    if let Some(matches) = &mut matches {
        for found in matches {
            let icon_offset = icon_offsets[found.start + leading_whitespace_offset] + leading_whitespace_offset;
            found.start += icon_offset;
            found.end += icon_offset;
        }
    }
    matches
}

fn compare_entries(a: &ItemElement, b: &ItemElement, look_for: &[u16]) -> i32 {
    let a_has = a.label_highlights.as_ref().is_some_and(|h| !h.is_empty());
    let b_has = b.label_highlights.as_ref().is_some_and(|h| !h.is_empty());
    match (a_has, b_has) {
        (true, false) => -1,
        (false, true) => 1,
        (false, false) => 0,
        (true, true) => compare_anything(a.sane_sort_label.units(), b.sane_sort_label.units(), look_for),
    }
}

impl State {
    fn element_height(&self, row: Row) -> f64 {
        match row {
            Row::Separator(_) => SEPARATOR_HEIGHT,
            Row::Item(index) => {
                if self.items[index].sane_detail.as_deref().is_some_and(|detail| !detail.is_empty()) {
                    ITEM_WITH_DETAIL_HEIGHT
                } else {
                    ITEM_HEIGHT
                }
            }
        }
    }

    fn total_height(&self) -> f64 {
        self.rows.iter().map(|row| self.element_height(*row)).sum()
    }

    fn view_height(&self) -> f64 {
        let total = self.total_height();
        self.max_height.map_or(total, |max| total.min(max))
    }

    fn row_top(&self, position: usize) -> f64 {
        self.rows[..position].iter().map(|row| self.element_height(*row)).sum()
    }

    fn clamp_scroll(&self, scroll_top: f64) -> f64 {
        let max = (self.total_height() - self.view_height()).max(0.0);
        scroll_top.clamp(0.0, max)
    }

    fn set_scroll_top(&mut self, scroll_top: f64) {
        self.scroll_top = self.clamp_scroll(scroll_top);
    }

    fn row_position(&self, row: Row) -> Option<usize> {
        self.rows.iter().position(|candidate| *candidate == row)
    }

    /// `index_at(y)`: a linha que cobre a posição vertical (a última se passar do fim).
    fn index_at(&self, y: f64) -> usize {
        let mut top = 0.0;
        for (position, row) in self.rows.iter().enumerate() {
            top += self.element_height(*row);
            if y < top {
                return position;
            }
        }
        self.rows.len()
    }

    /// `reveal(location, relativeTop)`.
    fn reveal(&mut self, row: Row, relative_top: Option<f64>) {
        let Some(position) = self.row_position(row) else { return };
        let top = self.row_top(position);
        let height = self.element_height(row);
        let view = self.view_height();
        match relative_top {
            Some(relative) => {
                let target = top - relative * (view - height);
                self.set_scroll_top(target);
            }
            None => {
                if top < self.scroll_top {
                    self.set_scroll_top(top);
                } else if top + height > self.scroll_top + view {
                    self.set_scroll_top(top + height - view);
                }
            }
        }
    }

    fn parent_of(&self, item: usize) -> Option<usize> {
        self.items[item].parent
    }

    fn item_visible(&self, item: usize) -> bool {
        self.row_position(Row::Item(item)).is_some()
    }

    /// Reconstrói as linhas visíveis a partir dos elementos da árvore (`TreeVisibility`).
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for top in &self.current {
            match *top {
                Top::Item(index) => {
                    if !self.items[index].hidden {
                        rows.push(Row::Item(index));
                    }
                }
                Top::Separator(index) => {
                    let separator = &self.separators[index];
                    if separator.hidden {
                        continue;
                    }
                    let children: Vec<usize> = separator.children.iter().copied().filter(|child| !self.items[*child].hidden).collect();
                    if children.is_empty() {
                        continue;
                    }
                    rows.push(Row::Separator(index));
                    rows.extend(children.into_iter().map(Row::Item));
                }
            }
        }
        self.rows = rows;
    }

    fn visible_items(&self) -> Vec<usize> {
        self.rows.iter().filter_map(|row| if let Row::Item(index) = row { Some(*index) } else { None }).collect()
    }

    fn all_visible_checked(&self, elements: &[usize], mut when_none_visible: bool) -> bool {
        for index in elements {
            let element = &self.items[*index];
            if !element.hidden && element.item.pickable != Some(false) {
                if !element.checked {
                    return false;
                }
                when_none_visible = true;
            }
        }
        when_none_visible
    }

    fn checked_elements(&self) -> Vec<IQuickPickItem> {
        self.items.iter().filter(|element| element.checked).map(|element| element.item.clone()).collect()
    }

    fn aria_label_of(&self, item: &ItemElement) -> String {
        if let Some(aria_label) = item.item.base.aria_label.as_deref().filter(|label| !label.is_empty()) {
            return aria_label.to_owned();
        }
        let parts = [
            Some(item.sane_label.clone()),
            item.sane_description.clone(),
            item.sane_detail.as_deref().map(JsString::from),
        ];
        parts
            .into_iter()
            .flatten()
            .map(|part| get_codicon_aria_label(Some(part.units())).to_string())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// O que `[ ... ]` de um passo de mutação pede para disparar depois que o estado foi solto.
#[derive(Default)]
struct Pending {
    focus: Option<Vec<IQuickPickItem>>,
    selection: Option<Vec<IQuickPickItem>>,
    visible_count: Option<usize>,
    all_visible_checked: Option<bool>,
    checked_count: Option<usize>,
    checked_elements: Option<Vec<IQuickPickItem>>,
    leave: bool,
}

/// `QuickInputList`.
pub struct QuickInputList {
    base: Rc<Disposable>,
    parent: Rc<ViewElement>,
    focus_state: Rc<QuickInputFocus>,
    id: String,
    state: RefCell<State>,
    active_descendant: RefCell<Option<String>>,
    scroll_laid_out: Cell<u32>,
    on_leave: Rc<Emitter<()>>,
    on_did_change_focus: Rc<Emitter<Vec<IQuickPickItem>>>,
    on_did_change_selection: Rc<Emitter<QuickPickSelectionEvent>>,
    on_changed_visible_count: Rc<Emitter<usize>>,
    on_changed_all_visible_checked: Rc<Emitter<bool>>,
    on_changed_checked_count: Rc<Emitter<usize>>,
    on_changed_checked_elements: Rc<Emitter<Vec<IQuickPickItem>>>,
    on_button_triggered: Rc<Emitter<IQuickPickItemButtonEvent>>,
    on_separator_button_triggered: Rc<Emitter<IQuickPickSeparatorButtonEvent>>,
}

impl QuickInputList {
    pub fn new(parent: Rc<ViewElement>, focus_state: Rc<QuickInputFocus>, id: &str) -> Rc<Self> {
        let base = Disposable::new();
        let on_leave = Emitter::new();
        let on_did_change_focus = Emitter::new();
        let on_did_change_selection = Emitter::new();
        let on_changed_visible_count = Emitter::new();
        let on_changed_all_visible_checked = Emitter::new();
        let on_changed_checked_count = Emitter::new();
        let on_changed_checked_elements = Emitter::new();
        let on_button_triggered = Emitter::new();
        let on_separator_button_triggered = Emitter::new();
        base.register(on_leave.clone());
        base.register(on_did_change_focus.clone());
        base.register(on_did_change_selection.clone());
        base.register(on_changed_visible_count.clone());
        base.register(on_changed_all_visible_checked.clone());
        base.register(on_changed_checked_count.clone());
        base.register(on_changed_checked_elements.clone());
        base.register(on_button_triggered.clone());
        base.register(on_separator_button_triggered.clone());
        Rc::new(Self {
            base,
            parent,
            focus_state,
            id: id.to_owned(),
            state: RefCell::new(State {
                input_elements: Vec::new(),
                element_tree: Vec::new(),
                current: Vec::new(),
                items: Vec::new(),
                separators: Vec::new(),
                rows: Vec::new(),
                focused: None,
                selected: Vec::new(),
                scroll_top: 0.0,
                max_height: None,
                last_query: None,
                has_checkboxes: false,
                visible_count: 0,
                all_visible_checked: false,
                checked_count: 0,
                checked_items: Vec::new(),
                displayed: true,
                enabled: true,
                aria_label: None,
                match_on_description: false,
                match_on_detail: false,
                match_on_label: true,
                match_on_label_mode: MatchOnLabelMode::Fuzzy,
                sort_by_label: true,
                should_loop: true,
            }),
            active_descendant: RefCell::new(None),
            scroll_laid_out: Cell::new(0),
            on_leave,
            on_did_change_focus,
            on_did_change_selection,
            on_changed_visible_count,
            on_changed_all_visible_checked,
            on_changed_checked_count,
            on_changed_checked_elements,
            on_button_triggered,
            on_separator_button_triggered,
        })
    }

    // -----------------------------------------------------------------------------------------
    // Eventos e propriedades.
    // -----------------------------------------------------------------------------------------

    pub fn on_leave(&self) -> Event<()> {
        self.on_leave.event()
    }

    pub fn on_changed_visible_count(&self) -> Event<usize> {
        self.on_changed_visible_count.event()
    }

    pub fn on_changed_all_visible_checked(&self) -> Event<bool> {
        self.on_changed_all_visible_checked.event()
    }

    pub fn on_changed_checked_count(&self) -> Event<usize> {
        self.on_changed_checked_count.event()
    }

    /// O `id` do elemento da árvore (`listId`).
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn displayed(&self) -> bool {
        self.state.borrow().displayed
    }

    pub fn set_displayed(&self, displayed: bool) {
        self.state.borrow_mut().displayed = displayed;
    }

    pub fn enabled(&self) -> bool {
        self.state.borrow().enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.borrow_mut().enabled = enabled;
    }

    /// `getActiveDescendant()`: o `aria-activedescendant` da árvore.
    pub fn get_active_descendant(&self) -> Option<String> {
        self.active_descendant.borrow().clone()
    }

    /// `scrollHeight`.
    pub fn scroll_height(&self) -> f64 {
        self.state.borrow().total_height()
    }

    /// A altura visível da lista (`renderHeight`).
    pub fn render_height(&self) -> f64 {
        self.state.borrow().view_height()
    }

    /// O `max-height` que `layout()` pôs (`undefined` é sem limite).
    pub fn max_height(&self) -> Option<f64> {
        self.state.borrow().max_height
    }

    /// Quantas vezes `layout()` refez a árvore.
    pub fn layout_count(&self) -> u32 {
        self.scroll_laid_out.get()
    }

    /// `layout(maxHeight)`.
    pub fn layout(&self, max_height: Option<f64>) {
        let mut state = self.state.borrow_mut();
        // Make sure height aligns with list item heights, and add some extra height so that it's clear there's more to scroll.
        state.max_height = max_height.filter(|height| *height != 0.0).map(|height| (height / 44.0).floor() * 44.0 + 6.0);
        let scroll_top = state.scroll_top;
        state.set_scroll_top(scroll_top);
        self.scroll_laid_out.set(self.scroll_laid_out.get() + 1);
    }

    /// As linhas visíveis, com o que o desenho precisa de cada uma.
    pub fn rows(&self) -> Vec<QuickInputListRow> {
        let state = self.state.borrow();
        let mut top = 0.0;
        state
            .rows
            .iter()
            .map(|row| {
                let height = state.element_height(*row);
                let this_top = top;
                top += height;
                match *row {
                    Row::Item(index) => {
                        let element = &state.items[index];
                        QuickInputListRow {
                            row: *row,
                            label: element.sane_label.clone(),
                            description: element.sane_description.clone(),
                            detail: element.sane_detail.clone(),
                            data_quick_input_id: element.item.base.id.clone(),
                            not_pickable: element.item.pickable == Some(false),
                            has_checkbox: element.has_checkbox,
                            checked: element.checked,
                            focused: state.focused == Some(index),
                            selected: state.selected.contains(&index),
                            separator_label: element.separator.as_ref().and_then(|separator| separator.label.clone()).filter(|label| !label.is_empty()),
                            separator_border: element.separator.is_some() && element.child_index != 0,
                            label_highlights: element.label_highlights.clone().unwrap_or_default(),
                            description_highlights: element.description_highlights.clone().unwrap_or_default(),
                            detail_highlights: element.detail_highlights.clone().unwrap_or_default(),
                            aria_label: state.aria_label_of(element),
                            top: this_top,
                            height,
                        }
                    }
                    Row::Separator(index) => {
                        let separator = &state.separators[index];
                        QuickInputListRow {
                            row: *row,
                            label: JsString::from(separator.separator.label.clone().unwrap_or_default()),
                            description: separator.separator.description.clone().map(JsString::from),
                            detail: None,
                            data_quick_input_id: separator.separator.id.clone(),
                            not_pickable: false,
                            has_checkbox: false,
                            checked: false,
                            focused: false,
                            selected: false,
                            separator_label: None,
                            separator_border: false,
                            label_highlights: Vec::new(),
                            description_highlights: Vec::new(),
                            detail_highlights: Vec::new(),
                            aria_label: separator.separator.aria_label.clone().unwrap_or_default(),
                            top: this_top,
                            height,
                        }
                    }
                }
            })
            .collect()
    }

    /// O tooltip do elemento focado (`toggleHover` só age se houver um).
    pub fn focused_tooltip(&self) -> Option<QuickPickTooltip> {
        let state = self.state.borrow();
        state.focused.and_then(|index| state.items[index].item.tooltip.clone())
    }

    /// Dispara o botão de um item (o clique na barra de ações da linha).
    pub fn trigger_item_button(&self, item: &IQuickPickItem, button: QuickInputButtonRef) {
        self.on_button_triggered.fire(IQuickPickItemButtonEvent { button, item: item.clone() });
    }

    /// Dispara o botão de um separador.
    pub fn trigger_separator_button(&self, separator: &IQuickPickSeparator, button: QuickInputButtonRef) {
        self.on_separator_button_triggered.fire(IQuickPickSeparatorButtonEvent { button, separator: separator.clone() });
    }

    /// O clique no contêiner da lista com o mouse (`CLICK` com `x || y`), ou o botão do meio: a lista deixa o foco.
    pub fn leave(&self) {
        self.on_leave.fire(());
    }

    /// O usuário escolheu os itens com o mouse ou o teclado (`onDidChangeSelection` da árvore).
    pub fn select_elements(&self, items: &[IQuickPickItem], middle_click: bool) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            let selected: Vec<usize> = items.iter().filter_map(|item| state.items.iter().position(|element| same_item(&element.item, item))).collect();
            self.apply_selection(&mut state, selected, &mut pending);
        }
        self.fire_pending(pending, middle_click);
    }

    /// `toggleCheckbox()`.
    pub fn toggle_checkbox(&self) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            let elements: Vec<usize> = state.focused.into_iter().collect();
            let all_checked = state.all_visible_checked(&elements, true);
            for index in elements {
                if state.items[index].item.base.disabled != Some(true) {
                    state.items[index].checked = !all_checked;
                }
            }
            self.update_checked_observables(&mut state, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    /// `setAllVisibleChecked(checked)`.
    pub fn set_all_visible_checked(&self, checked: bool) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            for element in state.items.iter_mut() {
                if !element.hidden && element.item.base.disabled != Some(true) && element.item.pickable != Some(false) {
                    element.checked = checked;
                }
            }
            self.update_checked_observables(&mut state, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    /// `toggleHover()`: sem o `WorkbenchHoverDelegate`, só diz se o elemento focado tem o que mostrar.
    pub fn toggle_hover(&self) -> bool {
        self.focused_tooltip().is_some()
    }

    // -----------------------------------------------------------------------------------------
    // Núcleo.
    // -----------------------------------------------------------------------------------------

    fn fire_pending(&self, pending: Pending, middle_click: bool) {
        if let Some(count) = pending.visible_count {
            self.on_changed_visible_count.fire(count);
        }
        if let Some(checked) = pending.all_visible_checked {
            self.on_changed_all_visible_checked.fire(checked);
        }
        if let Some(count) = pending.checked_count {
            self.on_changed_checked_count.fire(count);
        }
        if let Some(items) = pending.checked_elements {
            self.on_changed_checked_elements.fire(items);
        }
        if let Some(items) = pending.focus {
            self.on_did_change_focus.fire(items);
        }
        if let Some(items) = pending.selection {
            self.on_did_change_selection.fire(QuickPickSelectionEvent { items, middle_click });
        }
        if pending.leave {
            self.on_leave.fire(());
        }
    }

    fn update_checked_observables(&self, state: &mut State, pending: &mut Pending) {
        let all: Vec<usize> = (0..state.items.len()).collect();
        let all_visible_checked = state.all_visible_checked(&all, false);
        if all_visible_checked != state.all_visible_checked {
            state.all_visible_checked = all_visible_checked;
            pending.all_visible_checked = Some(all_visible_checked);
        }
        let checked_count = state.items.iter().filter(|element| element.checked).count();
        if checked_count != state.checked_count {
            state.checked_count = checked_count;
            pending.checked_count = Some(checked_count);
        }
        let checked_items = state.checked_elements();
        if !items_equal(&checked_items, &state.checked_items) {
            state.checked_items = checked_items.clone();
            pending.checked_elements = Some(checked_items);
        }
    }

    /// `TreeModel.onDidSpliceModel`: o que mudou de visível no modelo (contagem, marcados, foco e seleção perdidos).
    fn on_model_changed(&self, state: &mut State, pending: &mut Pending) {
        let visible_count = state.items.iter().filter(|element| !element.hidden).count();
        if visible_count != state.visible_count {
            state.visible_count = visible_count;
            pending.visible_count = Some(visible_count);
        }
        if state.has_checkboxes {
            self.update_checked_observables(state, pending);
        }
    }

    fn set_focus_to(&self, state: &mut State, focused: Option<usize>, pending: &mut Pending) {
        state.focused = focused;
        let items = focused.map(|index| vec![state.items[index].item.clone()]).unwrap_or_default();
        *self.active_descendant.borrow_mut() = focused.and_then(|index| state.row_position(Row::Item(index))).map(|position| format!("{}_{position}", self.id));
        pending.focus = Some(items);
    }

    fn apply_selection(&self, state: &mut State, selected: Vec<usize>, pending: &mut Pending) {
        state.selected = selected.clone();
        let items: Vec<IQuickPickItem> = selected.iter().map(|index| state.items[*index].item.clone()).collect();
        pending.selection = Some(items);
    }

    /// `_setElementsToTree(elements)`.
    fn set_elements_to_tree(&self, state: &mut State, tops: Vec<Top>, pending: &mut Pending) {
        state.current = tops;
        state.rebuild_rows();
        // Foco e seleção que saíram da árvore.
        if let Some(focused) = state.focused {
            if !state.item_visible(focused) {
                self.set_focus_to(state, None, pending);
            }
        }
        let selected_before = state.selected.len();
        let still: Vec<usize> = state.selected.iter().copied().filter(|index| state.item_visible(*index)).collect();
        if still.len() != selected_before {
            self.apply_selection(state, still, pending);
        }
        let scroll_top = state.scroll_top;
        state.set_scroll_top(scroll_top);
        self.on_model_changed(state, pending);
    }

    fn item_index_of(&self, state: &State, item: &IQuickPickItem) -> Option<usize> {
        state.items.iter().position(|element| same_item(&element.item, item))
    }

    // -----------------------------------------------------------------------------------------
    // Foco.
    // -----------------------------------------------------------------------------------------

    fn find_next_row(&self, state: &State, start: i64, loop_around: bool, filter: &dyn Fn(&State, Row) -> bool) -> Option<usize> {
        let length = state.rows.len() as i64;
        if length == 0 {
            return None;
        }
        let mut index = start;
        for _ in 0..length {
            if index >= length && !loop_around {
                return None;
            }
            index = index.rem_euclid(length);
            if filter(state, state.rows[index as usize]) {
                return Some(index as usize);
            }
            index += 1;
        }
        None
    }

    fn find_previous_row(&self, state: &State, start: i64, loop_around: bool, filter: &dyn Fn(&State, Row) -> bool) -> Option<usize> {
        let length = state.rows.len() as i64;
        if length == 0 {
            return None;
        }
        let mut index = start;
        for _ in 0..length {
            if index < 0 && !loop_around {
                return None;
            }
            index = (length + (index % length)) % length;
            if filter(state, state.rows[index as usize]) {
                return Some(index as usize);
            }
            index -= 1;
        }
        None
    }

    fn focused_position(&self, state: &State) -> Option<usize> {
        state.focused.and_then(|index| state.row_position(Row::Item(index)))
    }

    fn focus_row(&self, state: &mut State, position: usize, pending: &mut Pending) {
        match state.rows[position] {
            Row::Item(index) => self.set_focus_to(state, Some(index), pending),
            Row::Separator(_) => {}
        }
    }

    fn first_child_of(state: &State, separator: usize) -> Option<usize> {
        state.separators[separator].children.first().copied()
    }

    fn reveal_for_previous(state: &mut State, item: usize) {
        match state.parent_of(item) {
            Some(parent) if state.separators[parent].children.first() == Some(&item) => state.reveal(Row::Separator(parent), None),
            _ => state.reveal(Row::Item(item), None),
        }
    }

    fn is_item_row(_: &State, row: Row) -> bool {
        matches!(row, Row::Item(_))
    }

    fn focus_impl(&self, what: QuickPickFocus, state: &mut State, pending: &mut Pending) {
        if state.items.is_empty() || state.rows.is_empty() {
            return;
        }
        let what =if what == QuickPickFocus::Second && state.items.len() < 2 { QuickPickFocus::First } else { what };
        match what {
            QuickPickFocus::First => {
                state.set_scroll_top(0.0);
                if let Some(position) = self.find_next_row(state, 0, false, &Self::is_item_row) {
                    self.focus_row(state, position, pending);
                }
            }
            QuickPickFocus::Second => {
                state.set_scroll_top(0.0);
                let seen_first = Cell::new(false);
                let filter = |_: &State, row: Row| {
                    if !matches!(row, Row::Item(_)) {
                        return false;
                    }
                    if seen_first.get() {
                        return true;
                    }
                    seen_first.set(true);
                    false
                };
                if let Some(position) = self.find_next_row(state, 0, false, &filter) {
                    self.focus_row(state, position, pending);
                }
            }
            QuickPickFocus::Last => {
                let total = state.total_height();
                state.set_scroll_top(total);
                let last = state.rows.len() as i64 - 1;
                if let Some(position) = self.find_previous_row(state, last, false, &Self::is_item_row) {
                    self.focus_row(state, position, pending);
                }
            }
            QuickPickFocus::Next => {
                let previous = state.focused;
                let start = self.focused_position(state).map_or(0, |position| position as i64 + 1);
                if let Some(position) = self.find_next_row(state, start, state.should_loop, &Self::is_item_row) {
                    if let Row::Item(index) = state.rows[position] {
                        state.reveal(Row::Item(index), None);
                    }
                    self.focus_row(state, position, pending);
                }
                if previous.is_some() && previous == state.focused {
                    pending.leave = true;
                }
            }
            QuickPickFocus::Previous => {
                let previous = state.focused;
                let start = self.focused_position(state).map_or(0, |position| position as i64 - 1);
                if let Some(position) = self.find_previous_row(state, start, state.should_loop, &Self::is_item_row) {
                    if let Row::Item(index) = state.rows[position] {
                        Self::reveal_for_previous(state, index);
                    }
                    self.focus_row(state, position, pending);
                }
                if previous.is_some() && previous == state.focused {
                    pending.leave = true;
                }
            }
            QuickPickFocus::NextPage => self.focus_next_page(state, pending),
            QuickPickFocus::PreviousPage => self.focus_previous_page(state, pending),
            QuickPickFocus::NextSeparator => self.focus_next_separator(state, pending),
            QuickPickFocus::PreviousSeparator => self.focus_previous_separator(state, pending),
        }
    }

    fn focus_next_page(&self, state: &mut State, pending: &mut Pending) {
        loop {
            let last_page = state.index_at(state.scroll_top + state.view_height());
            let last_page = (if last_page == 0 { 0 } else { last_page - 1 }).min(state.rows.len().saturating_sub(1));
            let current = self.focused_position(state);
            if current != Some(last_page) && current.is_none_or(|position| last_page > position) {
                let last_good = self.find_previous_row(state, last_page as i64, false, &Self::is_item_row);
                let target = match last_good {
                    Some(good) if current != Some(good) => good,
                    _ => last_page,
                };
                if let Some(Row::Item(index)) = state.rows.get(target).copied() {
                    state.reveal(Row::Item(index), None);
                }
                self.focus_row(state, target, pending);
                return;
            }
            let previous_scroll_top = state.scroll_top;
            let mut next_scroll_top = previous_scroll_top + state.view_height();
            if current.is_some_and(|position| last_page > position) {
                next_scroll_top -= state.element_height(state.rows[last_page]);
            }
            state.set_scroll_top(next_scroll_top);
            if state.scroll_top == previous_scroll_top {
                return;
            }
            pending.focus = Some(Vec::new());
            state.focused = None;
        }
    }

    fn focus_previous_page(&self, state: &mut State, pending: &mut Pending) {
        loop {
            let first_page = state.index_at(state.scroll_top);
            let first_page = first_page.min(state.rows.len().saturating_sub(1));
            let first_page_top = state.row_top(first_page);
            // Se a linha da frente está cortada, a página começa na seguinte.
            let first_page = if first_page_top < state.scroll_top && first_page + 1 < state.rows.len() { first_page + 1 } else { first_page };
            let current = self.focused_position(state);
            if current != Some(first_page) && current.is_none_or(|position| first_page < position) {
                let first_good = self.find_next_row(state, first_page as i64, false, &Self::is_item_row);
                let target = match first_good {
                    Some(good) if current != Some(good) => good,
                    _ => first_page,
                };
                if let Some(Row::Item(index)) = state.rows.get(target).copied() {
                    Self::reveal_for_previous(state, index);
                }
                self.focus_row(state, target, pending);
                return;
            }
            let previous_scroll_top = state.scroll_top;
            let mut next_scroll_top = previous_scroll_top - state.view_height();
            if current.is_some_and(|position| first_page < position) {
                next_scroll_top += state.element_height(state.rows[first_page]);
            }
            state.set_scroll_top(next_scroll_top);
            if state.scroll_top == previous_scroll_top {
                return;
            }
            pending.focus = Some(Vec::new());
            state.focused = None;
        }
    }

    fn separator_visible(&self, state: &State, separator: usize) -> bool {
        let Some(position) = state.row_position(Row::Separator(separator)) else { return false };
        let top = state.row_top(position);
        let height = state.element_height(Row::Separator(separator));
        top >= state.scroll_top && top + height <= state.scroll_top + state.view_height()
    }

    fn item_with_separator_visible(&self, state: &State, item: usize) -> bool {
        let Some(position) = state.row_position(Row::Item(item)) else { return false };
        let top = state.row_top(position);
        let height = state.element_height(Row::Item(item));
        top >= state.scroll_top && top + height <= state.scroll_top + state.view_height()
    }

    fn first_top_item(state: &State) -> Option<usize> {
        match state.element_tree.first() {
            Some(Top::Item(index)) => Some(*index),
            _ => None,
        }
    }

    fn focus_next_separator(&self, state: &mut State, pending: &mut Pending) {
        let before = state.focused;
        let mut found_separator_as_item = false;
        let start = self.focused_position(state).map_or(0, |position| position as i64 + 1);
        let length = state.rows.len() as i64;
        let mut chosen = None;
        let mut index = start;
        for _ in 0..length {
            index = index.rem_euclid(length.max(1));
            let row = state.rows[index as usize];
            let accept = if found_separator_as_item {
                true
            } else {
                match row {
                    Row::Separator(separator) => {
                        found_separator_as_item = true;
                        if self.separator_visible(state, separator) {
                            if let Some(child) = Self::first_child_of(state, separator) {
                                state.reveal(Row::Item(child), None);
                            }
                        } else {
                            state.reveal(Row::Separator(separator), Some(0.0));
                        }
                        false
                    }
                    Row::Item(item) => {
                        if state.items[item].separator.is_some() {
                            if self.item_with_separator_visible(state, item) {
                                state.reveal(Row::Item(item), None);
                            } else {
                                state.reveal(Row::Item(item), Some(0.0));
                            }
                            true
                        } else if Self::first_top_item(state) == Some(item) {
                            // We should stop at the first item in the list if it's a regular item.
                            state.reveal(Row::Item(item), Some(0.0));
                            true
                        } else {
                            false
                        }
                    }
                }
            };
            if accept {
                chosen = Some(index as usize);
                break;
            }
            index += 1;
        }
        if let Some(position) = chosen {
            self.focus_row(state, position, pending);
        }
        if before == state.focused {
            // If we didn't move, then we should just move to the end of the list.
            let total = state.total_height();
            state.set_scroll_top(total);
            let last = state.rows.len() as i64 - 1;
            if let Some(position) = self.find_previous_row(state, last, false, &Self::is_item_row) {
                self.focus_row(state, position, pending);
            }
        }
    }

    fn focus_previous_separator(&self, state: &mut State, pending: &mut Pending) {
        let mut focus_element: Option<usize> = None;
        // If we are already sitting on an inline separator, then we have already found the current separator.
        let mut found_separator = state.focused.is_some_and(|index| state.items[index].separator.is_some());
        let start = self.focused_position(state).map_or(0, |position| position as i64 - 1);
        let length = state.rows.len() as i64;
        let mut index = start;
        for _ in 0..length {
            index = (length + (index % length.max(1))) % length.max(1);
            let row = state.rows[index as usize];
            let mut stop = false;
            match row {
                Row::Separator(separator) => {
                    if found_separator {
                        if focus_element.is_none() {
                            if self.separator_visible(state, separator) {
                                state.reveal(Row::Separator(separator), None);
                            } else {
                                state.reveal(Row::Separator(separator), Some(0.0));
                            }
                            focus_element = Self::first_child_of(state, separator);
                        }
                    } else {
                        found_separator = true;
                    }
                }
                Row::Item(item) => {
                    if focus_element.is_none() {
                        if state.items[item].separator.is_some() {
                            if self.item_with_separator_visible(state, item) {
                                state.reveal(Row::Item(item), None);
                            } else {
                                state.reveal(Row::Item(item), Some(0.0));
                            }
                            focus_element = Some(item);
                        } else if Self::first_top_item(state) == Some(item) {
                            // We should stop at the first item in the list if it's a regular item.
                            state.reveal(Row::Item(item), Some(0.0));
                            stop = true;
                        }
                    }
                }
            }
            if stop {
                break;
            }
            index -= 1;
        }
        if let Some(item) = focus_element {
            self.set_focus_to(state, Some(item), pending);
        }
    }
}

impl IQuickInputListUi for QuickInputList {
    /// `filter(query)`.
    fn filter(&self, query: &str) -> bool {
        let mut pending = Pending::default();
        let did_filter;
        {
            let mut state = self.state.borrow_mut();
            state.last_query = Some(query.to_owned());
            if !(state.sort_by_label || state.match_on_label || state.match_on_description || state.match_on_detail) {
                return false;
            }
            let query_with_whitespace = query;
            let query = query.trim();
            let (match_on_label, match_on_description, match_on_detail) = (state.match_on_label, state.match_on_description, state.match_on_detail);
            let mode = state.match_on_label_mode;
            let sort_by_label = state.sort_by_label;

            // Reset filtering
            if query.is_empty() || !(match_on_label || match_on_description || match_on_detail) {
                let inputs = state.input_elements.clone();
                for element in state.items.iter_mut() {
                    element.label_highlights = None;
                    element.description_highlights = None;
                    element.detail_highlights = None;
                    element.hidden = false;
                    let previous = if element.index > 0 { inputs.get(element.index - 1) } else { None };
                    element.separator = match previous {
                        Some(QuickPickInput::Separator(separator)) if separator.buttons.is_none() => Some(separator.clone()),
                        _ => None,
                    };
                }
            }
            // Filter by value (since we support icons in labels, use $(..) aware fuzzy matching)
            else {
                let inputs = state.input_elements.clone();
                let query_units: Vec<u16> = query.encode_utf16().collect();
                let mut current_separator: Option<IQuickPickSeparator> = None;
                for element in state.items.iter_mut() {
                    let label_highlights = if match_on_label {
                        let parsed = parse_label_with_icons(element.sane_label.units());
                        match mode {
                            MatchOnLabelMode::Fuzzy => matches_fuzzy_icon_aware(&query_units, &parsed, false),
                            MatchOnLabelMode::Contiguous => matches_contiguous_icon_aware(query_with_whitespace, &parsed),
                        }
                    } else {
                        None
                    };
                    let description_highlights = if match_on_description {
                        matches_fuzzy_icon_aware(&query_units, &parse_label_with_icons(element.sane_description.clone().unwrap_or_default().units()), false)
                    } else {
                        None
                    };
                    let detail_highlights = if match_on_detail {
                        let detail = JsString::from(element.sane_detail.clone().unwrap_or_default());
                        matches_fuzzy_icon_aware(&query_units, &parse_label_with_icons(detail.units()), false)
                    } else {
                        None
                    };
                    if label_highlights.is_some() || description_highlights.is_some() || detail_highlights.is_some() {
                        element.label_highlights = label_highlights;
                        element.description_highlights = description_highlights;
                        element.detail_highlights = detail_highlights;
                        element.hidden = false;
                    } else {
                        element.label_highlights = None;
                        element.description_highlights = None;
                        element.detail_highlights = None;
                        element.hidden = element.item.always_show != Some(true);
                    }
                    // Ensure separators are filtered out first before deciding if we need to bring them back
                    element.separator = None;
                    // we can show the separator unless the list gets sorted by match
                    if !sort_by_label {
                        let previous = if element.index > 0 { inputs.get(element.index - 1) } else { None };
                        if let Some(QuickPickInput::Separator(separator)) = previous {
                            if separator.buttons.is_none() {
                                current_separator = Some(separator.clone());
                            }
                        }
                        if current_separator.is_some() && !element.hidden {
                            element.separator = current_separator.take();
                        }
                    }
                }
            }

            let flat = state.sort_by_label && !query.is_empty();
            let tops: Vec<Top> = if flat {
                // We don't render any separators if we're sorting so just render the elements
                let look_for: Vec<u16> = state.last_query.clone().unwrap_or_default().to_lowercase().encode_utf16().collect();
                let mut order: Vec<usize> = (0..state.items.len()).collect();
                let items = &state.items;
                order.sort_by(|a, b| compare_entries(&items[*a], &items[*b], &look_for).cmp(&0));
                order.into_iter().map(Top::Item).collect()
            } else {
                // Render the full tree
                state.element_tree.clone()
            };
            self.set_elements_to_tree(&mut state, tops, &mut pending);
            did_filter = true;
        }
        self.fire_pending(pending, false);
        did_filter
    }

    fn focus(&self, focus: QuickPickFocus) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            self.focus_impl(focus, &mut state, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    fn clear_focus(&self) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            self.set_focus_to(&mut state, None, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    fn dom_focus(&self) {
        self.focus_state.set(QuickInputFocusTarget::List);
    }

    fn scroll_top(&self) -> f64 {
        self.state.borrow().scroll_top
    }

    fn set_scroll_top(&self, scroll_top: f64) {
        self.state.borrow_mut().set_scroll_top(scroll_top);
    }

    /// `setElements(inputElements)`.
    fn set_elements(&self, input_elements: &[QuickPickInput]) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            state.last_query = None;
            state.input_elements = input_elements.to_vec();
            state.has_checkboxes = self.parent.class_list_contains("show-checkboxes");
            // O foco, a seleção e os marcados dos elementos antigos vão embora com eles.
            if state.focused.is_some() {
                self.set_focus_to(&mut state, None, &mut pending);
            }
            if !state.selected.is_empty() {
                self.apply_selection(&mut state, Vec::new(), &mut pending);
            }
            let (mut items, mut separators, mut tree) = (Vec::<ItemElement>::new(), Vec::<SeparatorElement>::new(), Vec::<Top>::new());
            let mut current_separator: Option<usize> = None;
            for (index, input) in input_elements.iter().enumerate() {
                match input {
                    QuickPickInput::Separator(separator) => {
                        if separator.buttons.is_none() {
                            // This separator will be rendered as a part of the list item
                            continue;
                        }
                        separators.push(SeparatorElement { index, separator: separator.clone(), children: Vec::new(), hidden: false });
                        current_separator = Some(separators.len() - 1);
                        tree.push(Top::Separator(separators.len() - 1));
                    }
                    QuickPickInput::Item(item) => {
                        let previous = if index > 0 { input_elements.get(index - 1) } else { None };
                        let separator = match previous {
                            Some(QuickPickInput::Separator(previous)) if previous.buttons.is_none() => Some(previous.clone()),
                            _ => None,
                        };
                        let sane_label = item.base.label.clone();
                        let sane_sort_label = JsString::from(je_base::js_string::trim(parse_label_with_icons(sane_label.units()).text.units()));
                        let child_index = current_separator.map_or(index, |separator| separators[separator].children.len());
                        let element = ItemElement {
                            index,
                            child_index,
                            has_checkbox: state.has_checkboxes && item.pickable != Some(false),
                            item: item.clone(),
                            separator,
                            parent: current_separator,
                            hidden: false,
                            checked: false,
                            sane_label,
                            sane_sort_label,
                            sane_description: item.base.description.clone(),
                            sane_detail: item.detail.clone(),
                            label_highlights: item.highlights.as_ref().and_then(|highlights| highlights.label.clone()),
                            description_highlights: item.highlights.as_ref().and_then(|highlights| highlights.description.clone()),
                            detail_highlights: item.highlights.as_ref().and_then(|highlights| highlights.detail.clone()),
                        };
                        items.push(element);
                        let item_index = items.len() - 1;
                        match current_separator {
                            Some(separator) => separators[separator].children.push(item_index),
                            None => tree.push(Top::Item(item_index)),
                        }
                    }
                }
            }
            state.items = items;
            state.separators = separators;
            state.element_tree = tree.clone();
            self.set_elements_to_tree(&mut state, tree, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    fn get_checked_elements(&self) -> Vec<IQuickPickItem> {
        self.state.borrow().checked_elements()
    }

    fn set_checked_elements(&self, items: &[IQuickPickItem]) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            for element in state.items.iter_mut() {
                element.checked = items.iter().any(|item| same_item(item, &element.item));
            }
            self.update_checked_observables(&mut state, &mut pending);
        }
        self.fire_pending(pending, false);
    }

    fn set_selected_elements(&self, items: &[IQuickPickItem]) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            let selected: Vec<usize> = items.iter().filter_map(|item| self.item_index_of(&state, item)).collect();
            self.apply_selection(&mut state, selected, &mut pending);
            // When the user selects a separator... (só itens chegam por aqui; os separadores têm a rota de `select_elements`).
        }
        self.fire_pending(pending, false);
    }

    fn set_focused_elements(&self, items: &[IQuickPickItem]) {
        let mut pending = Pending::default();
        {
            let mut state = self.state.borrow_mut();
            let elements: Vec<usize> = items.iter().filter_map(|item| self.item_index_of(&state, item)).filter(|index| !state.items[*index].hidden).collect();
            // A árvore guarda um foco só no teclado; vários elementos viram o primeiro.
            self.set_focus_to(&mut state, elements.first().copied(), &mut pending);
            if !items.is_empty() {
                if let Some(focused) = state.focused {
                    state.reveal(Row::Item(focused), None);
                }
            }
        }
        self.fire_pending(pending, false);
    }

    fn set_should_loop(&self, should_loop: bool) {
        self.state.borrow_mut().should_loop = should_loop;
    }

    fn set_match_on_description(&self, value: bool) {
        self.state.borrow_mut().match_on_description = value;
    }

    fn set_match_on_detail(&self, value: bool) {
        self.state.borrow_mut().match_on_detail = value;
    }

    fn set_match_on_label(&self, value: bool) {
        self.state.borrow_mut().match_on_label = value;
    }

    fn set_match_on_label_mode(&self, value: MatchOnLabelMode) {
        self.state.borrow_mut().match_on_label_mode = value;
    }

    fn set_sort_by_label(&self, value: bool) {
        self.state.borrow_mut().sort_by_label = value;
    }

    fn aria_label(&self) -> Option<String> {
        self.state.borrow().aria_label.clone()
    }

    fn set_aria_label(&self, aria_label: Option<&str>) {
        self.state.borrow_mut().aria_label = Some(aria_label.unwrap_or_default().to_owned());
    }

    fn on_did_change_focus(&self) -> Event<Vec<IQuickPickItem>> {
        self.on_did_change_focus.event()
    }

    fn on_did_change_selection(&self) -> Event<QuickPickSelectionEvent> {
        self.on_did_change_selection.event()
    }

    fn on_changed_checked_elements(&self) -> Event<Vec<IQuickPickItem>> {
        self.on_changed_checked_elements.event()
    }

    fn on_button_triggered(&self) -> Event<IQuickPickItemButtonEvent> {
        self.on_button_triggered.event()
    }

    fn on_separator_button_triggered(&self) -> Event<IQuickPickSeparatorButtonEvent> {
        self.on_separator_button_triggered.event()
    }
}

impl IDisposable for QuickInputList {
    fn dispose(&self) {
        self.base.dispose();
    }
}
