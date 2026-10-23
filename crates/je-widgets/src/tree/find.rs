//! Porte da busca de `src/vs/base/browser/ui/tree/abstractTree.ts`: o filtro por digitação
//! ([`FindFilter`], com o `contiguousFuzzyScore`), os botões ([`FindToggles`]) e o controlador
//! ([`FindController`]) que liga o filtro à árvore (modo destaque ou filtro, fuzzy ou contíguo).
//!
//! Do TypeScript para o Rust:
//! - O `FindWidget` do original (a caixa de busca com `FindInput`, `ActionBar`, `Toggle` e
//!   `InputBox`) não existe: esses widgets de `vs/base/browser/ui` ainda não foram portados. O controlador
//!   conversa com o widget por [`IFindWidget`] e o cria por [`IFindWidgetFactory`] (o `contextViewProvider`
//!   do original), `// TODO(integração)` até o `FindInput` existir. O controlador, o filtro e o estado
//!   (padrão, modo, tipo de casamento, histórico, mensagem de "sem resultados") são o do original.
//! - Uma árvore com busca tem `FilterData = FindFilterData` (o `FuzzyScore | LabelFuzzyScore` do original,
//!   que o TypeScript apaga com um cast). Quem monta a árvore chama [`with_find_filter`] antes de criar o
//!   modelo (o `FindFilter` troca o filtro das opções, como no construtor da `AbstractTree`) e
//!   [`attach_find_controller`] depois.
//! - `FuzzyScore.Default` e a pontuação do [`contiguous_fuzzy_score`] usam `i32` (o `je-base` guarda a
//!   pontuação em `Vec<i32>`): o `Number.MAX_SAFE_INTEGER` do original é `i32::MAX`.
//! - `alert()` (anúncio para leitor de tela) não existe no `je-ui`: `// TODO(integração)`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::filters::{FUZZY_SCORE_DEFAULT, FuzzyScore, FuzzyScoreOptions, fuzzy_score, is_default_fuzzy_score};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::themables::ThemeIcon;
use je_ui::NodeId;

use crate::common::store_emitter;
use crate::list::list_types::IKeyboardNavigationLabelProvider;
use crate::tree::abstract_tree::{AbstractTree, AbstractTreeOptions, AbstractTreeOptionsUpdate, TreeFindMatchType, TreeFindMode};
use crate::tree::index_tree_model::{get_visible_state, is_filter_result};
use crate::tree::tree_types::{ITreeFilter, ITreeModel, TreeFilterResult, TreeNodeApi, TreeVisibility, VisibilityValue};

/// `FuzzyScore | LabelFuzzyScore`.
#[derive(Clone, Debug, PartialEq)]
pub enum FindFilterData {
    Score(FuzzyScore),
    /// `LabelFuzzyScore`: o rótulo que casou, quando o elemento tem mais de um.
    Label { label: String, score: FuzzyScore },
}

impl FindFilterData {
    /// `FuzzyScore.Default`.
    pub fn default_score() -> Self {
        Self::Score(FUZZY_SCORE_DEFAULT.to_vec())
    }

    /// `FuzzyScore.isDefault(node.filterData)`: um `LabelFuzzyScore` nunca é o padrão.
    pub fn is_default(data: Option<&FindFilterData>) -> bool {
        match data {
            None => true,
            Some(Self::Score(score)) => is_default_fuzzy_score(Some(score)),
            Some(Self::Label { .. }) => false,
        }
    }
}

/// `contiguousFuzzyScore`.
pub fn contiguous_fuzzy_score(pattern_lower: &str, word_lower: &str) -> Option<FuzzyScore> {
    let pattern: Vec<u16> = pattern_lower.encode_utf16().collect();
    let word: Vec<u16> = word_lower.to_lowercase().encode_utf16().collect();

    let index = if pattern.is_empty() { Some(0) } else { word.windows(pattern.len()).position(|w| w == pattern.as_slice()) }?;

    let mut score = vec![i32::MAX, 0];

    for i in (1..=pattern.len()).rev() {
        score.push((index + i - 1) as i32);
    }

    Some(score)
}

/// `TreeVisibility | ((node: T) => TreeVisibility)`.
pub type DefaultFindVisibility<E> = Rc<dyn Fn(&E) -> TreeVisibility>;

/// `FindFilter<T>`.
pub struct FindFilter<E: 'static> {
    keyboard_navigation_label_provider: Rc<dyn IKeyboardNavigationLabelProvider<E>>,
    filter: Option<Rc<dyn ITreeFilter<E, FindFilterData>>>,
    default_find_visibility: Option<DefaultFindVisibility<E>>,
    total_count: Cell<usize>,
    match_count: Cell<usize>,
    find_match_type: Cell<TreeFindMatchType>,
    find_mode: Cell<TreeFindMode>,
    pattern: RefCell<String>,
    lowercase_pattern: RefCell<String>,
}

impl<E: 'static> FindFilter<E> {
    pub fn new(
        keyboard_navigation_label_provider: Rc<dyn IKeyboardNavigationLabelProvider<E>>,
        filter: Option<Rc<dyn ITreeFilter<E, FindFilterData>>>,
        default_find_visibility: Option<DefaultFindVisibility<E>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            keyboard_navigation_label_provider,
            filter,
            default_find_visibility,
            total_count: Cell::new(0),
            match_count: Cell::new(0),
            find_match_type: Cell::new(TreeFindMatchType::Fuzzy),
            find_mode: Cell::new(TreeFindMode::Highlight),
            pattern: RefCell::new(String::new()),
            lowercase_pattern: RefCell::new(String::new()),
        })
    }

    pub fn total_count(&self) -> usize {
        self.total_count.get()
    }

    pub fn match_count(&self) -> usize {
        self.match_count.get()
    }

    pub fn find_match_type(&self) -> TreeFindMatchType {
        self.find_match_type.get()
    }

    pub fn set_find_match_type(&self, find_match_type: TreeFindMatchType) {
        self.find_match_type.set(find_match_type);
    }

    pub fn find_mode(&self) -> TreeFindMode {
        self.find_mode.get()
    }

    pub fn set_find_mode(&self, find_mode: TreeFindMode) {
        self.find_mode.set(find_mode);
    }

    pub fn pattern(&self) -> String {
        self.pattern.borrow().clone()
    }

    pub fn set_pattern(&self, pattern: &str) {
        *self.pattern.borrow_mut() = pattern.to_owned();
        *self.lowercase_pattern.borrow_mut() = pattern.to_lowercase();
    }

    pub fn reset(&self) {
        self.total_count.set(0);
        self.match_count.set(0);
    }
}

impl<E: 'static> ITreeFilter<E, FindFilterData> for FindFilter<E> {
    fn filter(&self, element: &E, parent_visibility: TreeVisibility) -> TreeFilterResult<FindFilterData> {
        let mut visibility = TreeVisibility::Visible;

        if let Some(filter) = &self.filter {
            let result = filter.filter(element, parent_visibility);

            visibility = match &result {
                TreeFilterResult::Bool(true) => TreeVisibility::Visible,
                TreeFilterResult::Bool(false) => TreeVisibility::Hidden,
                TreeFilterResult::Data { visibility, .. } if is_filter_result(&result) => get_visible_state(*visibility),
                TreeFilterResult::Data { .. } => unreachable!(),
                TreeFilterResult::Visibility(v) => *v,
            };

            if visibility == TreeVisibility::Hidden {
                return TreeFilterResult::Bool(false);
            }
        }

        self.total_count.set(self.total_count.get() + 1);

        let pattern = self.pattern.borrow().clone();

        if pattern.is_empty() {
            self.match_count.set(self.match_count.get() + 1);
            return TreeFilterResult::Data { data: FindFilterData::default_score(), visibility: VisibilityValue::Visibility(visibility) };
        }

        let labels = self.keyboard_navigation_label_provider.get_keyboard_navigation_label(element);
        let lowercase_pattern = self.lowercase_pattern.borrow().clone();

        for label in &labels {
            let Some(label_str) = label else {
                return TreeFilterResult::Data { data: FindFilterData::default_score(), visibility: VisibilityValue::Visibility(visibility) };
            };

            let score = if self.find_match_type.get() == TreeFindMatchType::Contiguous {
                contiguous_fuzzy_score(&lowercase_pattern, &label_str.to_lowercase())
            } else {
                let pattern: Vec<u16> = pattern.encode_utf16().collect();
                let pattern_low: Vec<u16> = lowercase_pattern.encode_utf16().collect();
                let word: Vec<u16> = label_str.encode_utf16().collect();
                let word_low: Vec<u16> = label_str.to_lowercase().encode_utf16().collect();
                fuzzy_score(&pattern, &pattern_low, 0, &word, &word_low, 0, Some(&FuzzyScoreOptions { first_match_can_be_weak: true, boost_full_match: true }))
            };

            if let Some(score) = score {
                self.match_count.set(self.match_count.get() + 1);

                let data = if labels.len() == 1 { FindFilterData::Score(score) } else { FindFilterData::Label { label: label_str.clone(), score } };
                return TreeFilterResult::Data { data, visibility: VisibilityValue::Visibility(visibility) };
            }
        }

        if self.find_mode.get() == TreeFindMode::Filter {
            match &self.default_find_visibility {
                Some(default_find_visibility) => TreeFilterResult::Visibility(default_find_visibility(element)),
                None => TreeFilterResult::Visibility(TreeVisibility::Recurse),
            }
        } else {
            TreeFilterResult::Data { data: FindFilterData::default_score(), visibility: VisibilityValue::Visibility(visibility) }
        }
    }
}

/// Troca o filtro das opções pelo [`FindFilter`], como o construtor da `AbstractTree` faz quando há
/// `keyboardNavigationLabelProvider` e a busca não está desligada. Devolve o filtro de busca.
pub fn with_find_filter<M: ITreeModel<FilterData = FindFilterData>>(options: &mut AbstractTreeOptions<M>) -> Option<Rc<FindFilter<M::Element>>> {
    let provider = options.keyboard_navigation_label_provider.clone()?;

    if options.find_widget_enabled == Some(false) {
        return None;
    }

    let find_filter = FindFilter::new(provider, options.filter.take(), options.default_find_visibility.clone());
    options.filter = Some(find_filter.clone());
    Some(find_filter)
}

/// `ITreeFindToggleContribution`.
#[derive(Clone, Debug)]
pub struct TreeFindToggleContribution {
    pub id: String,
    pub title: String,
    pub icon: ThemeIcon,
    pub is_checked: bool,
}

/// `FindToggles`.
pub struct FindToggles {
    state_map: RefCell<Vec<TreeFindToggleContribution>>,
}

impl FindToggles {
    pub fn new(start_states: Vec<TreeFindToggleContribution>) -> Self {
        Self { state_map: RefCell::new(start_states) }
    }

    pub fn states(&self) -> Vec<TreeFindToggleContribution> {
        self.state_map.borrow().clone()
    }

    pub fn get(&self, id: &str) -> bool {
        self.state_map.borrow().iter().find(|s| s.id == id).map(|s| s.is_checked).unwrap_or_else(|| crate::common::throw(&format!("No state found for toggle id {id}")))
    }

    /// `set`: devolve se o valor mudou.
    pub fn set(&self, id: &str, value: bool) -> bool {
        let mut states = self.state_map.borrow_mut();
        let Some(state) = states.iter_mut().find(|s| s.id == id) else {
            crate::common::throw(&format!("No state found for toggle id {id}"));
        };

        if state.is_checked == value {
            return false;
        }

        state.is_checked = value;
        true
    }
}

/// `ITreeFindToggleChangeEvent`.
#[derive(Clone, Debug)]
pub struct TreeFindToggleChangeEvent {
    pub id: String,
    pub is_checked: bool,
}

/// O que o `FindWidget` expõe ao controlador (o `findInput` e o `actionbar` do original).
pub trait IFindWidget {
    fn set_value(&self, value: &str);
    fn set_toggle_state(&self, id: &str, checked: bool);
    fn set_place_holder(&self, place_holder: &str);
    fn get_history(&self) -> Vec<String>;
    fn focus(&self);
    fn select(&self);
    /// `showMessage({ type: WARNING, content })`; `None` mostra só o aviso, sem texto.
    fn show_warning(&self, content: Option<&str>);
    fn clear_message(&self);
    /// Fecha o widget (o `dispose` do original, que espera 300 ms pela animação).
    fn dispose(&self);
}

/// Os eventos do `FindWidget` que o controlador ouve.
#[derive(Clone)]
pub struct FindWidgetCallbacks {
    pub on_did_change_value: Rc<dyn Fn(&str)>,
    pub on_did_disable: Rc<dyn Fn()>,
    pub on_did_toggle_change: Rc<dyn Fn(&TreeFindToggleChangeEvent)>,
}

/// Cria o `FindWidget` (o `contextViewProvider` do original). `// TODO(integração)`: o `FindInput` ainda não existe.
pub trait IFindWidgetFactory {
    fn create(&self, container: NodeId, placeholder: &str, toggles: Vec<TreeFindToggleContribution>, history: Option<Vec<String>>, callbacks: FindWidgetCallbacks) -> Rc<dyn IFindWidget>;
}

const MODE_TOGGLE: &str = "mode";
const MATCH_TYPE_TOGGLE: &str = "matchType";

/// `IFindControllerOptions`.
pub struct FindControllerOptions {
    pub placeholder: Option<String>,
    pub show_not_found_message: Option<bool>,
    pub find_widget_container: Option<NodeId>,
    pub default_find_mode: Option<TreeFindMode>,
    pub default_find_match_type: Option<TreeFindMatchType>,
}

/// O que a `AbstractTree` usa do controlador de busca.
pub trait FindHandle<M: ITreeModel> {
    fn open(&self);
    fn close(&self);
    fn mode(&self) -> TreeFindMode;
    fn set_mode(&self, mode: TreeFindMode);
    fn match_type(&self) -> TreeFindMatchType;
    fn set_match_type(&self, match_type: TreeFindMatchType);
    fn should_allow_focus(&self, node: &Rc<M::Node>) -> bool;
    fn update_options(&self, update: &AbstractTreeOptionsUpdate<M::Element>);
}

/// `FindController<T, TFilterData>` (com o `AbstractFindController`).
pub struct FindController<M: ITreeModel<FilterData = FindFilterData>>
where
    M::Element: Eq + std::hash::Hash,
{
    weak_self: Weak<FindController<M>>,
    tree: Weak<AbstractTree<M>>,
    filter: Rc<FindFilter<M::Element>>,
    widget_factory: Rc<dyn IFindWidgetFactory>,
    options: FindControllerOptions,
    toggles: FindToggles,
    placeholder: RefCell<String>,
    history: RefCell<Option<Vec<String>>>,
    pattern: RefCell<String>,
    previous_pattern: RefCell<String>,
    widget: RefCell<Option<Rc<dyn IFindWidget>>>,
    on_did_change_pattern: Rc<Emitter<String>>,
    on_did_change_open_state: Rc<Emitter<bool>>,
    on_did_change_mode: Rc<Emitter<TreeFindMode>>,
    on_did_change_match_type: Rc<Emitter<TreeFindMatchType>>,
    disposables: Rc<DisposableStore>,
}

impl<M: ITreeModel<FilterData = FindFilterData>> FindController<M>
where
    M::Element: Eq + std::hash::Hash,
{
    pub fn new(tree: &Rc<AbstractTree<M>>, filter: Rc<FindFilter<M::Element>>, widget_factory: Rc<dyn IFindWidgetFactory>, options: FindControllerOptions) -> Rc<Self> {
        let default_find_mode = options.default_find_mode.unwrap_or(TreeFindMode::Highlight);
        let default_find_match_type = options.default_find_match_type.unwrap_or(TreeFindMatchType::Fuzzy);

        let toggles = FindToggles::new(vec![
            TreeFindToggleContribution { id: MODE_TOGGLE.to_owned(), icon: ThemeIcon::from_id("list-filter"), title: "Filter".to_owned(), is_checked: default_find_mode == TreeFindMode::Filter },
            TreeFindToggleContribution { id: MATCH_TYPE_TOGGLE.to_owned(), icon: ThemeIcon::from_id("search-fuzzy"), title: "Fuzzy Match".to_owned(), is_checked: default_find_match_type == TreeFindMatchType::Fuzzy },
        ]);

        filter.set_find_match_type(default_find_match_type);
        filter.set_find_mode(default_find_mode);

        let disposables = DisposableStore::new();
        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            tree: Rc::downgrade(tree),
            filter,
            widget_factory,
            placeholder: RefCell::new(options.placeholder.clone().unwrap_or_else(|| "Type to search".to_owned())),
            options,
            toggles,
            history: RefCell::new(None),
            pattern: RefCell::new(String::new()),
            previous_pattern: RefCell::new(String::new()),
            widget: RefCell::new(None),
            on_did_change_pattern: store_emitter(&disposables),
            on_did_change_open_state: store_emitter(&disposables),
            on_did_change_mode: store_emitter(&disposables),
            on_did_change_match_type: store_emitter(&disposables),
            disposables: disposables.clone(),
        });

        let weak = this.weak_self.clone();
        disposables.add(tree.on_did_change_model().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };

            if !this.is_opened() {
                return;
            }

            if !this.pattern.borrow().is_empty()
                && let Some(tree) = this.tree.upgrade()
            {
                tree.refilter();
            }

            this.render();
        }));

        let weak = this.weak_self.clone();
        disposables.add(tree.on_will_refilter().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.filter.reset();
            }
        }));

        this
    }

    pub fn pattern(&self) -> String {
        self.pattern.borrow().clone()
    }

    pub fn is_opened(&self) -> bool {
        self.widget.borrow().is_some()
    }

    pub fn on_did_change_pattern(&self) -> BaseEvent<String> {
        self.on_did_change_pattern.event()
    }

    pub fn on_did_change_open_state(&self) -> BaseEvent<bool> {
        self.on_did_change_open_state.event()
    }

    pub fn on_did_change_mode(&self) -> BaseEvent<TreeFindMode> {
        self.on_did_change_mode.event()
    }

    pub fn on_did_change_match_type(&self) -> BaseEvent<TreeFindMatchType> {
        self.on_did_change_match_type.event()
    }

    fn set_placeholder(&self, value: &str) {
        *self.placeholder.borrow_mut() = value.to_owned();

        if let Some(widget) = &*self.widget.borrow() {
            widget.set_place_holder(value);
        }
    }

    fn update_toggle_state(&self, id: &str, checked: bool) {
        self.toggles.set(id, checked);

        if let Some(widget) = &*self.widget.borrow() {
            widget.set_toggle_state(id, checked);
        }
    }

    fn render_message(&self, show_not_found: bool) {
        let Some(widget) = self.widget.borrow().clone() else { return };

        if show_not_found {
            let show = self.tree.upgrade().is_none_or(|t| t.options().update.show_not_found_message.unwrap_or(true));
            widget.show_warning(if show { Some("No results found.") } else { None });
        } else {
            widget.clear_message();
        }
    }

    fn render(&self) {
        let no_matches = self.filter.match_count() == 0 && self.filter.total_count() > 0;
        let show_not_found = no_matches && !self.pattern.borrow().is_empty();
        self.render_message(show_not_found);

        // TODO(integração): `alertResults` (anúncio para leitor de tela).
    }

    fn on_did_change_value(&self, pattern: &str) {
        *self.pattern.borrow_mut() = pattern.to_owned();
        self.on_did_change_pattern.fire(pattern.to_owned());
        self.filter.set_pattern(pattern);
        self.apply_pattern(pattern);
    }

    /// `applyPattern`.
    fn apply_pattern(&self, pattern: &str) {
        let Some(tree) = self.tree.upgrade() else { return };

        tree.refilter();

        if !pattern.is_empty() {
            tree.focus_next(0, true, None, Some(Rc::new(|node: &Rc<M::Node>| !FindFilterData::is_default(node.filter_data().as_ref()))));
        }

        let focus = tree.get_focus();

        if let Some(element) = focus.first() {
            // `tree.getRelativeTop(element) === null`: o foco não está inteiro na janela, então o revela.
            if let Some(location) = self.location_of(&tree, element)
                && tree.get_relative_top(&location).is_none()
            {
                tree.reveal(&location, Some(0.5));
            }
        }

        self.render();
    }

    /// A referência do modelo de um elemento: a do nó que está em foco com esse elemento.
    fn location_of(&self, tree: &Rc<AbstractTree<M>>, element: &M::Element) -> Option<M::Ref> {
        tree.view.get_focused_elements().into_iter().find(|n| &n.element() == element).map(|n| tree.model.get_node_location(&n))
    }

    fn on_did_toggle_change(&self, e: &TreeFindToggleChangeEvent) {
        match e.id.as_str() {
            MODE_TOGGLE => self.set_mode_impl(if e.is_checked { TreeFindMode::Filter } else { TreeFindMode::Highlight }),
            MATCH_TYPE_TOGGLE => self.set_match_type_impl(if e.is_checked { TreeFindMatchType::Fuzzy } else { TreeFindMatchType::Contiguous }),
            _ => {}
        }
    }

    fn set_mode_impl(&self, mode: TreeFindMode) {
        if mode == self.mode() {
            return;
        }

        let is_filter_mode = mode == TreeFindMode::Filter;
        self.update_toggle_state(MODE_TOGGLE, is_filter_mode);
        self.set_placeholder(if is_filter_mode { "Type to filter" } else { "Type to search" });
        self.filter.set_find_mode(mode);

        if let Some(tree) = self.tree.upgrade() {
            tree.refilter();
        }

        self.render();
        self.on_did_change_mode.fire(mode);
    }

    fn set_match_type_impl(&self, match_type: TreeFindMatchType) {
        if match_type == self.match_type() {
            return;
        }

        self.update_toggle_state(MATCH_TYPE_TOGGLE, match_type == TreeFindMatchType::Fuzzy);
        self.filter.set_find_match_type(match_type);

        if let Some(tree) = self.tree.upgrade() {
            tree.refilter();
        }

        self.render();
        self.on_did_change_match_type.fire(match_type);
    }
}

impl<M: ITreeModel<FilterData = FindFilterData>> FindHandle<M> for FindController<M>
where
    M::Element: Eq + std::hash::Hash,
{
    fn open(&self) {
        let existing = self.widget.borrow().clone();

        if let Some(widget) = existing {
            widget.focus();
            widget.select();
            return;
        }

        let Some(tree) = self.tree.upgrade() else { return };
        let widget_container = self.options.find_widget_container.unwrap_or_else(|| tree.get_html_element());

        if self.options.find_widget_container.is_none() {
            tree.update_options(&AbstractTreeOptionsUpdate { padding_top: Some(30.0), ..Default::default() });
        }

        let weak = self.weak_self.clone();
        let on_did_change_value: Rc<dyn Fn(&str)> = {
            let weak = weak.clone();
            Rc::new(move |value| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_value(value);
                }
            })
        };
        let on_did_disable: Rc<dyn Fn()> = {
            let weak = weak.clone();
            Rc::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.close();
                }
            })
        };
        let on_did_toggle_change: Rc<dyn Fn(&TreeFindToggleChangeEvent)> = Rc::new(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_toggle_change(e);
            }
        });

        let widget = self.widget_factory.create(
            widget_container,
            &self.placeholder.borrow(),
            self.toggles.states(),
            self.history.borrow().clone(),
            FindWidgetCallbacks { on_did_change_value, on_did_disable, on_did_toggle_change },
        );
        *self.widget.borrow_mut() = Some(widget.clone());

        widget.focus();
        widget.set_value(&self.previous_pattern.borrow());
        widget.select();
        self.on_did_change_open_state.fire(true);
    }

    fn close(&self) {
        let Some(widget) = self.widget.borrow_mut().take() else { return };

        if self.options.find_widget_container.is_none()
            && let Some(tree) = self.tree.upgrade()
        {
            tree.update_options(&AbstractTreeOptionsUpdate { padding_top: Some(0.0), ..Default::default() });
        }

        *self.history.borrow_mut() = Some(widget.get_history());
        widget.dispose();
        *self.previous_pattern.borrow_mut() = self.pattern.borrow().clone();
        self.on_did_change_value("");

        if let Some(tree) = self.tree.upgrade() {
            tree.dom_focus();
        }

        self.on_did_change_open_state.fire(false);
    }

    fn mode(&self) -> TreeFindMode {
        if self.toggles.get(MODE_TOGGLE) { TreeFindMode::Filter } else { TreeFindMode::Highlight }
    }

    fn set_mode(&self, mode: TreeFindMode) {
        self.set_mode_impl(mode);
    }

    fn match_type(&self) -> TreeFindMatchType {
        if self.toggles.get(MATCH_TYPE_TOGGLE) { TreeFindMatchType::Fuzzy } else { TreeFindMatchType::Contiguous }
    }

    fn set_match_type(&self, match_type: TreeFindMatchType) {
        self.set_match_type_impl(match_type);
    }

    /// `shouldAllowFocus`.
    fn should_allow_focus(&self, node: &Rc<M::Node>) -> bool {
        if !self.is_opened() || self.pattern.borrow().is_empty() {
            return true;
        }

        if self.filter.total_count() > 0 && self.filter.match_count() <= 1 {
            return true;
        }

        !FindFilterData::is_default(node.filter_data().as_ref())
    }

    fn update_options(&self, update: &AbstractTreeOptionsUpdate<M::Element>) {
        if let Some(mode) = update.default_find_mode {
            self.set_mode_impl(mode);
        }

        if let Some(match_type) = update.default_find_match_type {
            self.set_match_type_impl(match_type);
        }
    }
}

impl<M: ITreeModel<FilterData = FindFilterData>> IDisposable for FindController<M>
where
    M::Element: Eq + std::hash::Hash,
{
    fn dispose(&self) {
        *self.history.borrow_mut() = None;

        if let Some(widget) = self.widget.borrow_mut().take() {
            widget.dispose();
        }

        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FindController"
    }
}

/// Cria o [`FindController`] e o liga à árvore (o `if (findWidgetEnabled && keyboardNavigationLabelProvider &&
/// contextViewProvider)` do construtor da `AbstractTree`). Devolve o controlador para quem quiser descartá-lo.
pub fn attach_find_controller<M: ITreeModel<FilterData = FindFilterData>>(
    tree: &Rc<AbstractTree<M>>,
    find_filter: Rc<FindFilter<M::Element>>,
    widget_factory: Rc<dyn IFindWidgetFactory>,
    options: FindControllerOptions,
) -> Rc<FindController<M>>
where
    M::Element: Eq + std::hash::Hash,
{
    let controller = FindController::new(tree, find_filter, widget_factory, options);
    tree.set_find_handle(Some(controller.clone()));
    controller
}
