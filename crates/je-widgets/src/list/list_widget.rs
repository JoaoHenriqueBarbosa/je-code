//! Porte de `src/vs/base/browser/ui/list/listWidget.ts`: a [`List`], com seleção, foco, âncora, teclado
//! (setas, página, Enter, Esc, Ctrl+A) e mouse (clique, Ctrl e Shift para vários, duplo clique).
//!
//! Do TypeScript para o Rust:
//! - As classes de CSS dos traits (`focused`, `selected`, `anchor`) são classes do nó da linha, como no
//!   original. A [`Trait`] usa o `TraitRenderer` do original em forma direta: em vez de um renderer por
//!   trait no pipeline, a lista aplica as classes às linhas renderizadas com [`List::refresh_traits`] em
//!   cada render e mudança (mesmo efeito observável, uma passada só).
//! - `focusNextPage` e `focusPreviousPage` são assíncronos no original (esperam um `timeout(0)` para o
//!   scroll chegar); aqui a parte síncrona roda na chamada e a continuação é agendada com `spawn_local`,
//!   e as funções não devolvem a promessa (quem precisar esperar usa um `timeout(0)` depois, como o teste
//!   do VS Code já faz).
//! - O `DefaultStyleController` gera CSS de texto e não existe: [`List::style`] guarda os [`IListStyles`] e
//!   pinta as linhas (fundo, texto, outline de foco, raio) com a cascata de [`IListStyles::row_paint`], a cada
//!   mudança de `focused`/`selected`, de hover (`PointerEnter`/`PointerLeave` na linha) e de foco da lista.
//!   TypeNavigationController (digitar para achar) é o [`TypeNavigationController`], criado quando há
//!   `keyboardNavigationLabelProvider`. A arrastar e soltar (`dnd`) é a do original: o `ListViewDragAndDrop` entrega os elementos
//!   da seleção à [`ListView`], e as linhas com `drop-target*` são pintadas por [`List::paint_row`] (fundo de soltar
//!   e a linha de 1 px entre linhas).
//! - **A cor do texto.** O `color` do CSS herda; o `je-ui` o modela em [`Foreground`]. A lista dá à linha a cor dela
//!   (`Foreground::Color`) e ao `.monaco-list` a cor do tema ([`List::style`]); o texto que herda a cor a toma pela
//!   resolução do `je-ui`. Os renderers que ainda fixam a cor no texto (`Foreground::Content`) recebem a cor da linha
//!   copiada em cada nó, com a exceção do texto sob um nó que declara a própria cor. A classe [`ACTIVE_SELECTION_CLASS`]
//!   na linha é o `.monaco-list:focus .selected` das regras de descendente.
//! - `aria-*` não existem no `je-ui` (só `role` e `label`); o `getActiveDescendantId` fica por [`List::get_element_id`]. O
//!   `accessibilityProvider` ([`IListAccessibilityProvider`]) dá o papel e o rótulo da lista e de cada linha.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent, EventBufferer};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::runtime;
use je_render::Rgba;
use je_ui::{Border, Content, EventKind, Foreground, KeyData, NodeId, PointerButton, Role, Ui};

use crate::common::{registered_emitter, throw};
use crate::dnd::{DragEvent, IDragAndDropData};
use crate::drag_image::DragImageStyle;
use crate::sash::is_macintosh;
use crate::list::list_dnd::{IListDragAndDrop, IListViewDragAndDrop, ListDragOverEffectPosition, ListDragOverResult, ListViewTargetSector};
use crate::list::list_types::{
    BrowserEvent, ElementFilter, GroupId, IIdentityProvider, IKeyboardNavigationDelegate, IKeyboardNavigationEventFilter, IKeyboardNavigationLabelProvider, IListAccessibilityProvider, IListRenderer,
    IListVirtualDelegate, ListEvent, ListMouseEvent, list_error,
};
use crate::list::list_styles::{IListStyles, RowState};
use crate::list::list_view::{ListView, ListViewOptions};
use crate::list::type_navigation::{DefaultKeyboardNavigationDelegate, TypeNavigationController, TypeNavigationMode};

/// A classe da linha selecionada de uma lista com foco (`.monaco-list:focus .selected`), que a lista mantém em [`List::paint_row`]
/// para as regras de descendente (`Foreground::ColorUnlessUnder`).
pub const ACTIVE_SELECTION_CLASS: &str = "active-selection";

/// A largura da borda de cima: a linha do `.drop-target-before::before`.
const DROP_LINE_BEFORE: [f64; 4] = [1.0, 0.0, 0.0, 0.0];
/// A largura da borda de baixo: a linha do `.drop-target-after::after`.
const DROP_LINE_AFTER: [f64; 4] = [0.0, 0.0, 1.0, 0.0];

/// `getContiguousRangeContaining`.
fn get_contiguous_range_containing(range: &[usize], value: usize) -> Vec<usize> {
    let Some(index) = range.iter().position(|v| *v == value) else {
        return Vec::new();
    };
    let mut result = vec![value];
    let mut i = index as i64 - 1;
    let mut j = index + 1;
    // para a esquerda, enquanto consecutivos
    while i >= 0 && range[i as usize] == range[(i + 1) as usize] - 1 {
        result.insert(0, range[i as usize]);
        i -= 1;
    }
    while j < range.len() && range[j] == range[j - 1] + 1 {
        result.push(range[j]);
        j += 1;
    }
    result
}

/// `disjunction`: a diferença simétrica de dois conjuntos ordenados.
fn disjunction(one: &[usize], other: &[usize]) -> Vec<usize> {
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < one.len() || j < other.len() {
        if i >= one.len() {
            result.push(other[j]);
            j += 1;
        } else if j >= other.len() {
            result.push(one[i]);
            i += 1;
        } else if one[i] == other[j] {
            result.push(one[i]);
            i += 1;
            j += 1;
        } else if one[i] < other[j] {
            result.push(one[i]);
            i += 1;
        } else {
            result.push(other[j]);
            j += 1;
        }
    }

    result
}

/// `relativeComplement`: o que está em `one` e não em `other` (ambos ordenados).
fn relative_complement(one: &[usize], other: &[usize]) -> Vec<usize> {
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < one.len() || j < other.len() {
        if i >= one.len() {
            break;
        } else if j >= other.len() {
            result.push(one[i]);
            i += 1;
        } else if one[i] == other[j] {
            i += 1;
            j += 1;
        } else if one[i] < other[j] {
            result.push(one[i]);
            i += 1;
        } else {
            j += 1;
        }
    }

    result
}

/// O aviso dos índices que mudaram de estado no trait.
type IndexesCallback = Rc<dyn Fn(&[usize])>;

/// `ITraitChangeEvent`.
#[derive(Clone, Debug)]
pub struct TraitChangeEvent {
    pub indexes: Vec<usize>,
    pub browser_event: Option<BrowserEvent>,
}

/// `Trait<T>`: um conjunto de índices (foco, seleção, âncora) que acompanha o `splice` da lista.
pub struct Trait {
    name: &'static str,
    indexes: RefCell<Vec<usize>>,
    sorted_indexes: RefCell<Vec<usize>>,
    on_change: Rc<Emitter<TraitChangeEvent>>,
    /// Os índices que mudaram desde a última aplicação às linhas.
    on_changed_indexes: RefCell<Option<IndexesCallback>>,
}

impl Trait {
    fn new(name: &'static str) -> Self {
        Self { name, indexes: RefCell::default(), sorted_indexes: RefCell::default(), on_change: Emitter::new(), on_changed_indexes: RefCell::new(None) }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn on_change(&self) -> BaseEvent<TraitChangeEvent> {
        self.on_change.event()
    }

    /// `splice(start, deleteCount, elements)`: `elements[i]` diz se o novo elemento tem o trait.
    pub fn splice(&self, start: usize, delete_count: usize, elements: &[bool]) {
        let diff = elements.len() as i64 - delete_count as i64;
        let end = start + delete_count;
        let mut sorted_indexes = Vec::new();
        let old = self.sorted_indexes.borrow().clone();
        let mut i = 0;

        while i < old.len() && old[i] < start {
            sorted_indexes.push(old[i]);
            i += 1;
        }

        for (j, has) in elements.iter().enumerate() {
            if *has {
                sorted_indexes.push(j + start);
            }
        }

        while i < old.len() && old[i] >= end {
            sorted_indexes.push((old[i] as i64 + diff) as usize);
            i += 1;
        }

        self.do_set(sorted_indexes.clone(), sorted_indexes, None);
    }

    pub fn set(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) -> Vec<usize> {
        let mut sorted = indexes.clone();
        sorted.sort_unstable();
        self.do_set(indexes, sorted, browser_event)
    }

    fn do_set(&self, indexes: Vec<usize>, sorted_indexes: Vec<usize>, browser_event: Option<BrowserEvent>) -> Vec<usize> {
        let result = std::mem::replace(&mut *self.indexes.borrow_mut(), indexes.clone());
        let sorted_result = std::mem::replace(&mut *self.sorted_indexes.borrow_mut(), sorted_indexes);

        let to_render = disjunction(&sorted_result, &self.sorted_indexes.borrow().clone());
        if let Some(f) = self.on_changed_indexes.borrow().clone() {
            f(&to_render);
        }

        self.on_change.fire(TraitChangeEvent { indexes, browser_event });
        result
    }

    pub fn get(&self) -> Vec<usize> {
        self.indexes.borrow().clone()
    }

    pub fn contains(&self, index: usize) -> bool {
        self.sorted_indexes.borrow().binary_search(&index).is_ok()
    }
}

impl IDisposable for Trait {
    fn dispose(&self) {
        self.on_change.dispose();
    }
}

/// `TraitSpliceable<T>`: repassa o `splice` ao trait, preservando por identidade quando há provedor.
fn splice_trait<T: Clone + 'static>(trait_: &Trait, view: &ListView<T>, identity_provider: Option<&Rc<dyn IIdentityProvider<T>>>, start: usize, delete_count: usize, elements: &[T]) {
    let Some(identity_provider) = identity_provider else {
        return trait_.splice(start, delete_count, &vec![false; elements.len()]);
    };

    let past: Vec<String> = trait_.get().into_iter().map(|i| identity_provider.get_id(&view.element(i))).collect();

    if past.is_empty() {
        return trait_.splice(start, delete_count, &vec![false; elements.len()]);
    }

    let past: HashSet<String> = past.into_iter().collect();
    let with_trait: Vec<bool> = elements.iter().map(|e| past.contains(&identity_provider.get_id(e))).collect();
    trait_.splice(start, delete_count, &with_trait);
}

/// Os métodos que uma subclasse da `List` sobrescreve no original (a `TreeNodeList` e o
/// `TreeNodeListMouseController` da árvore). Cada `after_*` roda depois da versão base; os de mouse
/// substituem o comportamento padrão e chamam `list.default_*` quando o original chama `super`.
pub trait ListOverrides<T: Clone + 'static> {
    fn after_splice(&self, _list: &List<T>, _start: usize, _elements: &[T]) {}
    fn after_set_focus(&self, _list: &List<T>, _indexes: &[usize], _browser_event: Option<BrowserEvent>) {}
    fn after_set_selection(&self, _list: &List<T>, _indexes: &[usize], _browser_event: Option<BrowserEvent>) {}
    fn after_set_anchor(&self, _list: &List<T>, _index: Option<usize>) {}
    /// `MouseController.onViewPointer`.
    fn view_pointer(&self, list: &List<T>, e: &ListMouseEvent<T>) {
        list.default_view_pointer(e);
    }
    /// `MouseController.onDoubleClick`.
    fn double_click(&self, list: &List<T>, e: &ListMouseEvent<T>) {
        list.default_double_click(e);
    }
    /// `MouseController.onMouseDown`.
    fn mouse_down(&self, list: &List<T>, e: &ListMouseEvent<T>) {
        list.default_mouse_down(e);
    }
}

/// `IListOptions<T>` (os campos que a lista usa; os de estilo não existem).
#[derive(Clone)]
pub struct ListOptions<T> {
    pub view: ListViewOptions,
    pub identity_provider: Option<Rc<dyn IIdentityProvider<T>>>,
    pub multiple_selection_support: Option<bool>,
    pub keyboard_support: Option<bool>,
    /// A subclasse da lista, quando há (ver [`ListOverrides`]).
    pub overrides: Option<Rc<dyn ListOverrides<T>>>,
    /// `accessibilityProvider`: o papel e o rótulo da lista e das linhas.
    pub accessibility_provider: Option<Rc<dyn IListAccessibilityProvider<T>>>,
    /// `dnd`: "Once passed to a list, that list takes up the responsibility of disposing it".
    pub dnd: Option<Rc<dyn IListDragAndDrop<T>>>,
    /// `keyboardNavigationLabelProvider`: com ele a lista navega digitando (ver [`TypeNavigationController`]).
    pub keyboard_navigation_label_provider: Option<Rc<dyn IKeyboardNavigationLabelProvider<T>>>,
    /// `keyboardNavigationDelegate`; sem ele vale o [`DefaultKeyboardNavigationDelegate`].
    pub keyboard_navigation_delegate: Option<Rc<dyn IKeyboardNavigationDelegate>>,
    /// `keyboardNavigationEventFilter`; sem ele toda tecla pode começar a navegação.
    pub keyboard_navigation_event_filter: Option<IKeyboardNavigationEventFilter>,
    /// `typeNavigationEnabled`: ligado casa também por trechos difusos; ausente, só por prefixo.
    pub type_navigation_enabled: Option<bool>,
    /// `typeNavigationMode` (padrão: automático).
    pub type_navigation_mode: Option<TypeNavigationMode>,
}

impl<T> Default for ListOptions<T> {
    fn default() -> Self {
        Self {
            view: ListViewOptions::default(),
            identity_provider: None,
            multiple_selection_support: None,
            keyboard_support: None,
            overrides: None,
            accessibility_provider: None,
            dnd: None,
            keyboard_navigation_label_provider: None,
            keyboard_navigation_delegate: None,
            keyboard_navigation_event_filter: None,
            type_navigation_enabled: None,
            type_navigation_mode: None,
        }
    }
}

/// `List<T>`.
pub struct List<T: Clone + 'static> {
    base: Rc<Disposable>,
    weak_self: Weak<List<T>>,
    ui: Ui,
    user: String,
    options: ListOptions<T>,
    focus: Rc<Trait>,
    selection: Rc<Trait>,
    anchor: Rc<Trait>,
    event_bufferer: Rc<EventBufferer>,
    pub view: Rc<ListView<T>>,
    on_did_change_focus: Rc<Emitter<ListEvent<T>>>,
    on_did_change_selection: Rc<Emitter<ListEvent<T>>>,
    on_pointer: Rc<Emitter<ListMouseEvent<T>>>,
    on_did_dispose: Rc<Emitter<()>>,
    disposed: Cell<bool>,
    /// Os estilos de [`List::style`]; sem eles as linhas não são pintadas.
    styles: RefCell<Option<IListStyles>>,
    /// O `:focus` do `.monaco-list`.
    list_focused: Cell<bool>,
    /// As linhas sob o ponteiro.
    hovered: RefCell<HashSet<NodeId>>,
    /// Os nós de texto e de codicon que a cor da linha tomou: (cor original, cor aplicada).
    overridden_colors: RefCell<HashMap<NodeId, (Rgba, Rgba)>>,
    /// O `typeNavigationController`, quando há `keyboardNavigationLabelProvider`.
    type_navigation: RefCell<Option<Rc<TypeNavigationController<T>>>>,
}

/// `ListViewDragAndDrop<T>`: o `IListDragAndDrop` do usuário como o `IListViewDragAndDrop` da `ListView`, com os
/// elementos arrastados vindos da seleção da lista.
struct ListViewDragAndDrop<T: Clone + 'static> {
    list: Weak<List<T>>,
    dnd: Rc<dyn IListDragAndDrop<T>>,
}

impl<T: Clone + 'static> IListDragAndDrop<T> for ListViewDragAndDrop<T> {
    fn get_drag_uri(&self, element: &T) -> Option<String> {
        self.dnd.get_drag_uri(element)
    }

    fn get_drag_label(&self, elements: &[T], original_event: &DragEvent) -> Option<String> {
        self.dnd.get_drag_label(elements, original_event)
    }

    fn on_drag_start(&self, data: &Rc<dyn IDragAndDropData>, original_event: &DragEvent) {
        self.dnd.on_drag_start(data, original_event);
    }

    fn on_drag_over(&self, data: &Rc<dyn IDragAndDropData>, target_element: Option<&T>, target_index: Option<usize>, target_sector: Option<ListViewTargetSector>, original_event: &DragEvent) -> ListDragOverResult {
        self.dnd.on_drag_over(data, target_element, target_index, target_sector, original_event)
    }

    fn on_drag_leave(&self, data: &Rc<dyn IDragAndDropData>, target_element: Option<&T>, target_index: Option<usize>, original_event: &DragEvent) {
        self.dnd.on_drag_leave(data, target_element, target_index, original_event);
    }

    fn on_drag_end(&self, original_event: &DragEvent) {
        self.dnd.on_drag_end(original_event);
    }

    fn drop(&self, data: &Rc<dyn IDragAndDropData>, target_element: Option<&T>, target_index: Option<usize>, target_sector: Option<ListViewTargetSector>, original_event: &DragEvent) {
        (*self.dnd).drop(data, target_element, target_index, target_sector, original_event);
    }

    fn dispose(&self) {
        self.dnd.dispose();
    }
}

impl<T: Clone + 'static> IListViewDragAndDrop<T> for ListViewDragAndDrop<T> {
    fn get_drag_elements(&self, element: &T, index: usize) -> Vec<T> {
        match self.list.upgrade() {
            Some(list) if list.get_selection().contains(&index) => list.get_selected_elements(),
            _ => vec![element.clone()],
        }
    }
}

/// O renderer que aplica as classes dos traits (`focused`, `selected`, `anchor`) à linha e guarda o
/// índice renderizado de cada nó para reaplicar quando o trait muda.
struct TraitsRenderer<T: Clone + 'static> {
    inner: Rc<dyn IListRenderer<T>>,
    list: RefCell<Weak<List<T>>>,
    ui: Ui,
}

impl<T: Clone + 'static> IListRenderer<T> for TraitsRenderer<T> {
    fn template_id(&self) -> String {
        self.inner.template_id()
    }

    fn render_template(&self, container: NodeId) -> crate::list::list_types::TemplateData {
        let template_data = self.inner.render_template(container);
        if let Some(list) = self.list.borrow().upgrade() {
            list.listen_row_hover(container);
        }
        template_data
    }

    fn render_element(&self, element: &T, index: usize, template_data: &crate::list::list_types::TemplateData, details: Option<&crate::list::list_types::ListElementRenderDetails>) {
        if let Some(list) = self.list.borrow().upgrade()
            && let Some(node) = list.view.dom_element(index)
        {
            list.apply_traits(node, index);
            list.apply_accessibility(node, element);
        }
        self.inner.render_element(element, index, template_data, details);
        if let Some(list) = self.list.borrow().upgrade()
            && let Some(node) = list.view.dom_element(index)
        {
            list.paint_row(node);
        }
    }

    fn dispose_element(&self, element: &T, index: i64, template_data: &crate::list::list_types::TemplateData, details: Option<&crate::list::list_types::ListElementRenderDetails>) {
        self.inner.dispose_element(element, index, template_data, details);
    }

    fn dispose_template(&self, template_data: &crate::list::list_types::TemplateData) {
        let _ = &self.ui;
        self.inner.dispose_template(template_data);
    }
}

impl<T: Clone + 'static> List<T> {
    pub fn new(user: &str, ui: &Ui, container: NodeId, virtual_delegate: Rc<dyn IListVirtualDelegate<T>>, renderers: Vec<Rc<dyn IListRenderer<T>>>, options: ListOptions<T>) -> Rc<Self> {
        let base = Disposable::new();
        let wrapped: Vec<Rc<TraitsRenderer<T>>> = renderers.into_iter().map(|inner| Rc::new(TraitsRenderer { inner, list: RefCell::new(Weak::new()), ui: ui.clone() })).collect();
        let view = ListView::new(ui, container, virtual_delegate, wrapped.iter().map(|r| r.clone() as Rc<dyn IListRenderer<T>>).collect(), options.view.clone());
        base.register(view.clone());

        let this = Rc::new_cyclic(|weak| Self {
            base: base.clone(),
            weak_self: weak.clone(),
            ui: ui.clone(),
            user: user.to_owned(),
            options: options.clone(),
            focus: Rc::new(Trait::new("focused")),
            selection: Rc::new(Trait::new("selected")),
            anchor: Rc::new(Trait::new("anchor")),
            event_bufferer: EventBufferer::new(),
            view,
            on_did_change_focus: registered_emitter(&base),
            on_did_change_selection: registered_emitter(&base),
            on_pointer: registered_emitter(&base),
            on_did_dispose: registered_emitter(&base),
            disposed: Cell::new(false),
            styles: RefCell::new(None),
            list_focused: Cell::new(false),
            hovered: RefCell::new(HashSet::new()),
            overridden_colors: RefCell::new(HashMap::new()),
            type_navigation: RefCell::new(None),
        });
        for renderer in &wrapped {
            *renderer.list.borrow_mut() = Rc::downgrade(&this);
        }
        if let Some(dnd) = &options.dnd {
            // `dnd: options.dnd && new ListViewDragAndDrop(this, options.dnd)`
            this.view.set_dnd(Rc::new(ListViewDragAndDrop { list: this.weak_self.clone(), dnd: dnd.clone() }));
            let dnd = dnd.clone();
            base.register(to_disposable(move || dnd.dispose()));
        }
        let weak = this.weak_self.clone();
        base.register(this.view.on_did_change_drag_feedback().listen(move |_| {
            if let Some(list) = weak.upgrade() {
                list.repaint_rows();
            }
        }));
        if let Some(provider) = &options.accessibility_provider {
            // `setAttribute('role', ...)` e `setAttribute('aria-label', ...)` do `domNode` da lista.
            let (role, label) = (provider.get_widget_role().unwrap_or(Role::List), provider.get_widget_aria_label());
            let mut tree = ui.tree_mut();
            tree.set_role(this.view.dom_node, role);
            tree.set_label(this.view.dom_node, label);
        }
        for t in [&this.focus, &this.selection, &this.anchor] {
            base.register(t.clone());
            let weak = this.weak_self.clone();
            let name = t.name();
            *t.on_changed_indexes.borrow_mut() = Some(Rc::new(move |indexes: &[usize]| {
                if let Some(list) = weak.upgrade() {
                    list.refresh_traits(name, indexes);
                }
            }));
        }

        // eventos com buffer de `splice`
        for (trait_, emitter) in [(&this.focus, &this.on_did_change_focus), (&this.selection, &this.on_did_change_selection)] {
            let weak = this.weak_self.clone();
            let emitter = emitter.clone();
            base.register(this.event_bufferer.wrap_event(&trait_.on_change()).listen(move |e| {
                if let Some(list) = weak.upgrade() {
                    emitter.fire(list.to_list_event(e));
                }
            }));
        }
        let weak = this.weak_self.clone();
        base.register(this.focus.on_change().listen(move |_| {
            if let Some(list) = weak.upgrade() {
                let focused = !list.focus.get().is_empty();
                list.ui.tree_mut().toggle_class(list.view.dom_node, "element-focused", focused);
            }
        }));
        let weak = this.weak_self.clone();
        base.register(this.selection.on_change().listen(move |_| {
            if let Some(list) = weak.upgrade() {
                let n = list.selection.get().len();
                let mut tree = list.ui.tree_mut();
                tree.toggle_class(list.view.dom_node, "selection-none", n == 0);
                tree.toggle_class(list.view.dom_node, "selection-single", n == 1);
                tree.toggle_class(list.view.dom_node, "selection-multiple", n > 1);
            }
        }));

        if options.keyboard_support.unwrap_or(true) {
            this.install_keyboard_controller();
        }
        if let Some(provider) = &options.keyboard_navigation_label_provider {
            let delegate = options.keyboard_navigation_delegate.clone().unwrap_or_else(|| Rc::new(DefaultKeyboardNavigationDelegate) as Rc<dyn IKeyboardNavigationDelegate>);
            let event_filter = options.keyboard_navigation_event_filter.clone().unwrap_or_else(|| Rc::new(|_: &KeyData| true) as IKeyboardNavigationEventFilter);
            let controller = TypeNavigationController::new(
                this.weak_self.clone(),
                ui,
                this.view.dom_node,
                provider.clone(),
                event_filter,
                delegate,
                options.type_navigation_enabled,
                options.type_navigation_mode,
            );
            base.register(controller.clone());
            *this.type_navigation.borrow_mut() = Some(controller);
        }
        this.install_mouse_controller();
        for kind in [EventKind::Focus, EventKind::Blur] {
            let weak = this.weak_self.clone();
            base.register(ui.listen(this.view.dom_node, kind, false, move |e| {
                if let Some(list) = weak.upgrade() {
                    list.list_focused.set(e.kind == EventKind::Focus);
                    list.repaint_rows();
                }
            }));
        }
        this
    }

    /// O `role` e o `aria-label` da linha que o `ListView.insertItemInDOM` escreve com o `accessibilityProvider`, antes do
    /// `renderElement`: sem papel do provedor a linha é `listitem`, e um rótulo `null` ou vazio tira o rótulo.
    fn apply_accessibility(&self, row: NodeId, element: &T) {
        let Some(provider) = &self.options.accessibility_provider else { return };
        let role = provider.get_role(element).unwrap_or(Role::ListItem);
        let label = provider.get_aria_label(element).filter(|label| !label.is_empty());
        let mut tree = self.ui.tree_mut();
        tree.set_role(row, role);
        match label {
            Some(label) => tree.set_label(row, label),
            None => tree.clear_label(row),
        }
    }

    /// `style(styles)` do `DefaultStyleController`: guarda os estilos e repinta as linhas renderizadas.
    pub fn style(&self, styles: &IListStyles) {
        *self.styles.borrow_mut() = Some(styles.clone());
        // O `color` do `.monaco-list` (o `--vscode-foreground` do workbench, o `.ref-tree { color }` do peek): o texto das
        // linhas que herda a cor (`Foreground::Inherit`) a lê daqui quando a linha não declara a própria.
        self.ui.tree_mut().style_mut(self.view.dom_node).foreground = styles.foreground.map_or(Foreground::Content, Foreground::Color);
        self.view.set_drag_image_style(DragImageStyle {
            background: styles.list_active_selection_background,
            foreground: styles.list_active_selection_foreground,
            outline: styles.list_focus_outline,
        });
        self.repaint_rows();
    }

    fn repaint_rows(&self) {
        self.paint_rows_container();
        for row in self.view.rendered_dom_nodes() {
            self.paint_row(row);
        }
    }

    /// O fundo do `.monaco-list-rows`: o da lista, ou o de soltar quando a lista inteira é o alvo
    /// (`.monaco-list .monaco-list-rows.drop-target { background-color: listDropOverBackground }`).
    fn paint_rows_container(&self) {
        let styles = self.styles.borrow();
        let Some(styles) = styles.as_ref() else { return };
        let rows = self.view.rows_container;
        let mut tree = self.ui.tree_mut();
        let drop_target = tree.has_class(rows, ListDragOverEffectPosition::Over.class_name());
        tree.style_mut(rows).background = if drop_target { styles.list_drop_over_background.or(styles.list_background) } else { styles.list_background };
    }

    /// Acompanha o ponteiro sobre a linha (o `:hover` do CSS).
    fn listen_row_hover(&self, row: NodeId) {
        for kind in [EventKind::PointerEnter, EventKind::PointerLeave] {
            let weak = self.weak_self.clone();
            self.base.register(self.ui.listen(row, kind, false, move |e| {
                if let Some(list) = weak.upgrade() {
                    if e.kind == EventKind::PointerEnter {
                        list.hovered.borrow_mut().insert(row);
                    } else {
                        list.hovered.borrow_mut().remove(&row);
                    }
                    list.paint_row(row);
                }
            }));
        }
    }

    /// Pinta a linha com a cascata dos estilos: fundo, outline (borda de 1 px por dentro), raio e a cor do
    /// texto e dos codicons de todos os descendentes (o `color` herdado do CSS). O estado vem das classes
    /// `focused` e `selected` da linha, que a lista mantém. A cor original de um descendente volta quando a
    /// linha perde o estado; se o renderer escreveu outra cor no meio, a dele vale.
    fn paint_row(&self, row: NodeId) {
        let styles = self.styles.borrow();
        let Some(styles) = styles.as_ref() else { return };
        let mut tree = self.ui.tree_mut();
        if !tree.is_alive(row) {
            return;
        }

        let state = RowState { list_focused: self.list_focused.get(), focused: tree.has_class(row, "focused"), selected: tree.has_class(row, "selected"), hovered: self.hovered.borrow().contains(&row) };
        let mut paint = styles.row_paint(state);

        // `.monaco-list-row.drop-target { background-color: listDropOverBackground !important; color: inherit !important }`
        if let Some(background) = styles.list_drop_over_background
            && tree.has_class(row, ListDragOverEffectPosition::Over.class_name())
        {
            paint.background = Some(background);
            paint.foreground = None;
            paint.icon_foreground = None;
        }

        // `.drop-target-before::before` e `.drop-target-after::after`: a linha de 1 px de `listDropBetweenBackground` no
        // topo ou no pé da linha (a da lista inteira vale para a primeira e a última linha). O `::before` do original
        // fica por cima do outline; aqui a linha ocupa o lugar da borda.
        let rows_container = self.view.rows_container;
        let (is_first, is_last) = (tree.children(rows_container).first() == Some(&row), tree.children(rows_container).last() == Some(&row));
        let drop_line = styles.list_drop_between_background.and_then(|color| {
            if tree.has_class(row, ListDragOverEffectPosition::Before.class_name()) || (is_first && tree.has_class(rows_container, ListDragOverEffectPosition::Before.class_name())) {
                Some((color, DROP_LINE_BEFORE))
            } else if tree.has_class(row, ListDragOverEffectPosition::After.class_name()) || (is_last && tree.has_class(rows_container, ListDragOverEffectPosition::After.class_name())) {
                Some((color, DROP_LINE_AFTER))
            } else {
                None
            }
        });

        // `.monaco-list:focus .selected`: a classe que as regras de descendente das linhas (`Foreground::ColorUnlessUnder`,
        // como o `.reference-file` do peek) leem, já que o `:focus` da lista não é um antepassado no `je-ui`.
        tree.toggle_class(row, ACTIVE_SELECTION_CLASS, state.list_focused && state.selected);

        let row_style = tree.style_mut(row);
        row_style.background = paint.background;
        // A cor da linha é o `color` do CSS dela: o texto que herda a cor (`Foreground::Inherit`) a toma.
        row_style.foreground = paint.foreground.map_or(Foreground::Content, Foreground::Color);
        match drop_line {
            Some((color, widths)) => {
                row_style.border = Some(Border { width: 1.0, color });
                row_style.border_widths = Some(widths);
            }
            None => {
                row_style.border = paint.outline.map(|color| Border { width: 1.0, color });
                if row_style.border_widths == Some(DROP_LINE_BEFORE) || row_style.border_widths == Some(DROP_LINE_AFTER) {
                    row_style.border_widths = None;
                }
            }
        }
        row_style.radius = styles.row_radius;

        let mut overridden = self.overridden_colors.borrow_mut();
        for node in tree.subtree(row).into_iter().skip(1) {
            // Um `Role::Image` com texto é um glifo com cor própria (o `::before` do ícone de arquivo, que o CSS
            // pinta com `color` explícito e por isso não herda a cor da linha).
            if tree.role(node) == Role::Image {
                continue;
            }
            // O texto que herda a cor (`Foreground::Inherit`) ou que está sob um nó que declara a própria (o `.reference-file`,
            // o `.monaco-count-badge`) já tem a cor certa pela resolução do `color`: a da linha não o alcança, como no CSS.
            if tree.style(node).foreground != Foreground::Content || tree.declares_foreground_below(node, row) {
                continue;
            }
            let (color, target) = match &mut tree.style_mut(node).content {
                Content::Text { color, .. } => (color, paint.foreground),
                Content::Codicon { color, .. } => (color, paint.icon_foreground.or(paint.foreground)),
                Content::None | Content::Host(_) | Content::Paragraph(_) => continue,
            };
            match (target, overridden.get(&node).copied()) {
                (Some(target), previous) => {
                    let original = match previous {
                        Some((original, applied)) if *color == applied => original,
                        _ => *color,
                    };
                    overridden.insert(node, (original, target));
                    *color = target;
                }
                (None, Some((original, applied))) => {
                    if *color == applied {
                        *color = original;
                    }
                    overridden.remove(&node);
                }
                (None, None) => {}
            }
        }
    }

    /// Aplica as classes dos três traits à linha renderizada do índice.
    fn apply_traits(&self, node: NodeId, index: usize) {
        let mut tree = self.ui.tree_mut();
        for t in [&self.focus, &self.selection, &self.anchor] {
            tree.toggle_class(node, t.name(), t.contains(index));
        }
    }

    fn refresh_traits(&self, name: &str, indexes: &[usize]) {
        let trait_ = match name {
            "focused" => &self.focus,
            "selected" => &self.selection,
            _ => &self.anchor,
        };
        for &index in indexes {
            if index < self.view.length()
                && let Some(node) = self.view.dom_element(index)
            {
                self.ui.tree_mut().toggle_class(node, name, trait_.contains(index));
                self.paint_row(node);
            }
        }
    }

    pub fn on_did_change_focus(&self) -> BaseEvent<ListEvent<T>> {
        self.on_did_change_focus.event()
    }

    pub fn on_did_change_selection(&self) -> BaseEvent<ListEvent<T>> {
        self.on_did_change_selection.event()
    }

    pub fn on_pointer(&self) -> BaseEvent<ListMouseEvent<T>> {
        self.on_pointer.event()
    }

    pub fn on_did_dispose(&self) -> BaseEvent<()> {
        self.on_did_dispose.event()
    }

    pub fn dom_id(&self) -> &str {
        &self.view.dom_id
    }

    pub fn options(&self) -> &ListOptions<T> {
        &self.options
    }

    /// `triggerTypeNavigation()`: no modo [`TypeNavigationMode::Trigger`], liga (ou desliga) a navegação por tipo para a
    /// próxima palavra.
    pub fn trigger_type_navigation(&self) {
        if let Some(controller) = &*self.type_navigation.borrow() {
            controller.trigger();
        }
    }

    /// O trecho do `updateOptions` que o `TypeNavigationController` lê (`typeNavigationEnabled` e `typeNavigationMode`); o
    /// que está ausente fica como estava.
    pub fn update_type_navigation_options(&self, type_navigation_enabled: Option<bool>, type_navigation_mode: Option<TypeNavigationMode>) {
        if let Some(controller) = &*self.type_navigation.borrow() {
            controller.update_options(type_navigation_enabled, type_navigation_mode);
        }
    }

    /// `splice(start, deleteCount, elements)`.
    pub fn splice(&self, start: usize, delete_count: usize, elements: &[T]) {
        if start > self.view.length() {
            throw(&list_error(&self.user, &format!("Invalid start index: {start}")).to_string());
        }

        if delete_count == 0 && elements.is_empty() {
            return;
        }

        self.event_bufferer.buffer_events(|| {
            let identity = self.options.identity_provider.as_ref();
            splice_trait(&self.focus, &self.view, identity, start, delete_count, elements);
            splice_trait(&self.selection, &self.view, identity, start, delete_count, elements);
            splice_trait(&self.anchor, &self.view, identity, start, delete_count, elements);
            self.view.splice(start, delete_count, elements);
        });

        if let Some(overrides) = &self.options.overrides {
            overrides.after_splice(self, start, elements);
        }
    }

    pub fn update_element_height(&self, index: usize, size: Option<f64>) {
        self.view.update_element_height(index, size, None);
    }

    pub fn rerender(&self) {
        self.view.rerender();
    }

    pub fn element(&self, index: usize) -> T {
        self.view.element(index)
    }

    pub fn index_at(&self, position: f64) -> i64 {
        self.view.index_at(position)
    }

    pub fn length(&self) -> usize {
        self.view.length()
    }

    pub fn content_height(&self) -> f64 {
        self.view.content_height()
    }

    pub fn scroll_top(&self) -> f64 {
        self.view.get_scroll_top()
    }

    pub fn set_scroll_top(&self, scroll_top: f64) {
        self.view.set_scroll_top(scroll_top, false);
    }

    pub fn scroll_height(&self) -> f64 {
        self.view.scroll_height()
    }

    pub fn render_height(&self) -> f64 {
        self.view.render_height()
    }

    pub fn first_visible_index(&self) -> i64 {
        self.view.first_visible_index()
    }

    pub fn last_visible_index(&self) -> i64 {
        self.view.last_visible_index()
    }

    pub fn dom_focus(&self) {
        self.ui.focus(self.view.dom_node);
    }

    pub fn layout(&self, height: Option<f64>, width: Option<f64>) {
        self.view.layout(height, width);
    }

    pub fn set_selection(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        self.set_selection_base(indexes.clone(), browser_event);

        if let Some(overrides) = &self.options.overrides {
            overrides.after_set_selection(self, &indexes, browser_event);
        }
    }

    /// O `super.setSelection` da `TreeNodeList` (e o `fromAPI = true` dela).
    pub fn set_selection_base(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        for index in &indexes {
            if *index >= self.length() {
                throw(&list_error(&self.user, &format!("Invalid index {index}")).to_string());
            }
        }

        let indexes = indexes.into_iter().filter(|i| self.get_element_group_id(*i) != Some(GroupId::NotSelectable)).collect();
        self.selection.set(indexes, browser_event);
    }

    pub fn get_selection(&self) -> Vec<usize> {
        self.selection.get()
    }

    pub fn get_selected_elements(&self) -> Vec<T> {
        self.get_selection().into_iter().map(|i| self.view.element(i)).collect()
    }

    pub fn set_anchor(&self, index: Option<usize>) {
        self.set_anchor_base(index);

        if let Some(overrides) = &self.options.overrides {
            overrides.after_set_anchor(self, index);
        }
    }

    /// O `super.setAnchor` da `TreeNodeList` (e o `fromAPI = true` dela).
    pub fn set_anchor_base(&self, index: Option<usize>) {
        let Some(index) = index else {
            self.anchor.set(Vec::new(), None);
            return;
        };

        if index >= self.length() {
            throw(&list_error(&self.user, &format!("Invalid index {index}")).to_string());
        }

        self.anchor.set(vec![index], None);
    }

    pub fn get_anchor(&self) -> Option<usize> {
        self.anchor.get().first().copied()
    }

    pub fn get_anchor_element(&self) -> Option<T> {
        self.get_anchor().map(|a| self.element(a))
    }

    pub fn get_element_group_id(&self, index: usize) -> Option<GroupId> {
        let identity_provider = self.options.identity_provider.as_ref()?;
        identity_provider.get_group_id(&self.element(index))
    }

    pub fn filter_indices_by_group(&self, indices: Vec<usize>, reference_group_id: GroupId) -> Vec<usize> {
        let Some(identity_provider) = self.options.identity_provider.as_ref() else {
            return indices;
        };

        if reference_group_id == GroupId::NotSelectable {
            return Vec::new();
        }

        indices.into_iter().filter(|index| identity_provider.get_group_id(&self.element(*index)) == Some(reference_group_id)).collect()
    }

    pub fn set_focus(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        self.set_focus_base(indexes.clone(), browser_event);

        if let Some(overrides) = &self.options.overrides {
            overrides.after_set_focus(self, &indexes, browser_event);
        }
    }

    /// O `super.setFocus` da `TreeNodeList` (e o `fromAPI = true` dela).
    pub fn set_focus_base(&self, indexes: Vec<usize>, browser_event: Option<BrowserEvent>) {
        for index in &indexes {
            if *index >= self.length() {
                throw(&list_error(&self.user, &format!("Invalid index {index}")).to_string());
            }
        }

        self.focus.set(indexes, browser_event);
    }

    pub fn focus_next(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>, filter: Option<&dyn Fn(&T) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let focus = self.focus.get();
        let index = self.find_next_index(focus.first().map_or(0, |f| f + n) as i64, do_loop, filter);

        if index > -1 {
            self.set_focus(vec![index as usize], browser_event);
        }
    }

    pub fn focus_previous(&self, n: usize, do_loop: bool, browser_event: Option<BrowserEvent>, filter: Option<&dyn Fn(&T) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let focus = self.focus.get();
        let index = self.find_previous_index(focus.first().map_or(0, |f| *f as i64 - n as i64), do_loop, filter);

        if index > -1 {
            self.set_focus(vec![index as usize], browser_event);
        }
    }

    /// `focusNextPage`: a parte síncrona roda aqui; a continuação do scroll, num `timeout(0)`.
    pub fn focus_next_page(&self, browser_event: Option<BrowserEvent>, filter: Option<ElementFilter<T>>) {
        let mut last_page_index = self.view.index_at(self.view.get_scroll_top() + self.view.render_height());
        last_page_index = if last_page_index == 0 { 0 } else { last_page_index - 1 };
        let currently_focused = self.get_focus().first().map(|i| *i as i64);

        if currently_focused != Some(last_page_index) && currently_focused.is_none_or(|c| last_page_index > c) {
            let last_good_page_index = self.find_previous_index(last_page_index, false, filter.as_deref());

            if last_good_page_index > -1 && currently_focused != Some(last_good_page_index) {
                self.set_focus(vec![last_good_page_index as usize], browser_event);
            } else {
                self.set_focus(vec![last_page_index as usize], browser_event);
            }
        } else {
            let previous_scroll_top = self.view.get_scroll_top();
            let mut nextpage_scroll_top = previous_scroll_top + self.view.render_height();

            if currently_focused.is_some_and(|c| last_page_index > c) {
                nextpage_scroll_top -= self.view.element_height(last_page_index as usize);
            }

            self.view.set_scroll_top(nextpage_scroll_top, false);

            if self.view.get_scroll_top() != previous_scroll_top {
                self.set_focus(Vec::new(), None);

                // Dá tempo para o scroll renderizar os elementos novos.
                let weak = self.weak_self.clone();
                runtime::spawn_local(async move {
                    let _ = je_base::async_::timeout(0).await;
                    if let Some(list) = weak.upgrade() {
                        list.focus_next_page(browser_event, filter);
                    }
                });
            }
        }
    }

    /// `focusPreviousPage`: ver [`List::focus_next_page`].
    pub fn focus_previous_page(&self, browser_event: Option<BrowserEvent>, filter: Option<ElementFilter<T>>, get_padding_top: Rc<dyn Fn() -> f64>) {
        let padding_top = get_padding_top();
        let scroll_top = self.view.get_scroll_top() + padding_top;

        let first_page_index = if scroll_top == 0.0 { self.view.index_at(scroll_top) } else { self.view.index_after(scroll_top - 1.0) };

        let currently_focused = self.get_focus().first().map(|i| *i as i64);

        if currently_focused != Some(first_page_index) && currently_focused.is_none_or(|c| c >= first_page_index) {
            let first_good_page_index = self.find_next_index(first_page_index, false, filter.as_deref());

            if first_good_page_index > -1 && currently_focused != Some(first_good_page_index) {
                self.set_focus(vec![first_good_page_index as usize], browser_event);
            } else {
                self.set_focus(vec![first_page_index as usize], browser_event);
            }
        } else {
            let previous_scroll_top = scroll_top;
            self.view.set_scroll_top(scroll_top - self.view.render_height() - padding_top, false);

            if self.view.get_scroll_top() + get_padding_top() != previous_scroll_top {
                self.set_focus(Vec::new(), None);

                let weak = self.weak_self.clone();
                runtime::spawn_local(async move {
                    let _ = je_base::async_::timeout(0).await;
                    if let Some(list) = weak.upgrade() {
                        list.focus_previous_page(browser_event, filter, get_padding_top);
                    }
                });
            }
        }
    }

    pub fn focus_last(&self, browser_event: Option<BrowserEvent>, filter: Option<&dyn Fn(&T) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let index = self.find_previous_index(self.length() as i64 - 1, false, filter);

        if index > -1 {
            self.set_focus(vec![index as usize], browser_event);
        }
    }

    pub fn focus_first(&self, browser_event: Option<BrowserEvent>, filter: Option<&dyn Fn(&T) -> bool>) {
        self.focus_nth(0, browser_event, filter);
    }

    pub fn focus_nth(&self, n: usize, browser_event: Option<BrowserEvent>, filter: Option<&dyn Fn(&T) -> bool>) {
        if self.length() == 0 {
            return;
        }

        let index = self.find_next_index(n as i64, false, filter);

        if index > -1 {
            self.set_focus(vec![index as usize], browser_event);
        }
    }

    fn find_next_index(&self, mut index: i64, do_loop: bool, filter: Option<&dyn Fn(&T) -> bool>) -> i64 {
        let length = self.length() as i64;
        for _ in 0..length {
            if index >= length && !do_loop {
                return -1;
            }

            index %= length;

            if filter.is_none_or(|f| f(&self.element(index as usize))) {
                return index;
            }

            index += 1;
        }

        -1
    }

    fn find_previous_index(&self, mut index: i64, do_loop: bool, filter: Option<&dyn Fn(&T) -> bool>) -> i64 {
        let length = self.length() as i64;
        for _ in 0..length {
            if index < 0 && !do_loop {
                return -1;
            }

            // `%` do JavaScript mantém o sinal; o `(length + ...) % length` reduz ao intervalo
            index = (length + (index % length)) % length;

            if filter.is_none_or(|f| f(&self.element(index as usize))) {
                return index;
            }

            index -= 1;
        }

        -1
    }

    pub fn get_focus(&self) -> Vec<usize> {
        self.focus.get()
    }

    pub fn get_focused_elements(&self) -> Vec<T> {
        self.get_focus().into_iter().map(|i| self.view.element(i)).collect()
    }

    /// `reveal(index, relativeTop?, paddingTop = 0)`.
    pub fn reveal(&self, index: usize, relative_top: Option<f64>, padding_top: f64) {
        if index >= self.length() {
            throw(&list_error(&self.user, &format!("Invalid index {index}")).to_string());
        }

        let scroll_top = self.view.get_scroll_top();
        let element_top = self.view.element_top(index);
        let element_height = self.view.element_height(index);

        if let Some(relative_top) = relative_top {
            // y = mx + b
            let m = element_height - self.view.render_height() + padding_top;
            self.view.set_scroll_top(m * relative_top.clamp(0.0, 1.0) + element_top - padding_top, false);
        } else {
            let view_item_bottom = element_top + element_height;
            let scroll_bottom = scroll_top + self.view.render_height();

            if element_top < scroll_top + padding_top && view_item_bottom >= scroll_bottom {
                // o elemento já cobre a janela: nada a fazer
            } else if element_top < scroll_top + padding_top || (view_item_bottom >= scroll_bottom && element_height >= self.view.render_height()) {
                self.view.set_scroll_top(element_top - padding_top, false);
            } else if view_item_bottom >= scroll_bottom {
                self.view.set_scroll_top(view_item_bottom - self.view.render_height(), false);
            }
        }
    }

    /// `getRelativeTop`: a posição relativa do elemento na janela, ou nada se está fora dela.
    pub fn get_relative_top(&self, index: usize, padding_top: f64) -> Option<f64> {
        if index >= self.length() {
            throw(&list_error(&self.user, &format!("Invalid index {index}")).to_string());
        }

        let scroll_top = self.view.get_scroll_top();
        let element_top = self.view.element_top(index);
        let element_height = self.view.element_height(index);

        if element_top < scroll_top + padding_top || element_top + element_height > scroll_top + self.view.render_height() {
            return None;
        }

        let m = element_height - self.view.render_height() + padding_top;
        Some(((scroll_top + padding_top - element_top) / m).abs())
    }

    pub fn is_dom_focused(&self) -> bool {
        self.ui.focused() == Some(self.view.dom_node)
    }

    pub fn get_html_element(&self) -> NodeId {
        self.view.dom_node
    }

    pub fn get_element_id(&self, index: usize) -> String {
        self.view.get_element_dom_id(index)
    }

    pub fn get_element_top(&self, index: usize) -> f64 {
        self.view.element_top(index)
    }

    pub fn get_element_height(&self, index: usize) -> f64 {
        self.view.element_height(index)
    }

    fn to_list_event(&self, e: &TraitChangeEvent) -> ListEvent<T> {
        ListEvent { indexes: e.indexes.clone(), elements: e.indexes.iter().map(|i| self.view.element(*i)).collect(), browser_event: e.browser_event }
    }

    // -------------------- KeyboardController --------------------

    fn install_keyboard_controller(&self) {
        let weak = self.weak_self.clone();
        let multiple = self.options.multiple_selection_support;
        self.base.register(self.ui.listen(self.view.dom_node, EventKind::KeyDown, false, move |e| {
            let (Some(list), Some(KeyData { key_code, modifiers })) = (weak.upgrade(), e.key) else { return };
            let browser_event = Some(BrowserEvent { modifiers, button: PointerButton::Primary, target: Some(e.target), click_count: 0, is_keyboard: true, force_event: false });
            match key_code {
                KeyCode::Enter => {
                    e.stop(true);
                    list.set_selection(list.get_focus(), browser_event);
                }
                KeyCode::UpArrow => {
                    e.stop(true);
                    list.focus_previous(1, false, browser_event, None);
                    list.reveal_focus_and_anchor();
                }
                KeyCode::DownArrow => {
                    e.stop(true);
                    list.focus_next(1, false, browser_event, None);
                    list.reveal_focus_and_anchor();
                }
                KeyCode::PageUp => {
                    e.stop(true);
                    list.focus_previous_page(browser_event, None, Rc::new(|| 0.0));
                    list.reveal_focus_and_anchor();
                }
                KeyCode::PageDown => {
                    e.stop(true);
                    list.focus_next_page(browser_event, None);
                    list.reveal_focus_and_anchor();
                }
                KeyCode::Escape => {
                    if !list.get_selection().is_empty() {
                        e.stop(true);
                        list.set_selection(Vec::new(), browser_event);
                    }
                    list.dom_focus();
                }
                KeyCode::KeyA if multiple.unwrap_or(false) && (if is_macintosh() { modifiers.meta } else { modifiers.ctrl }) => {
                    e.stop(true);
                    let mut selection: Vec<usize> = (0..list.length()).collect();
                    let focused = list.get_focus();
                    let reference_group_id = focused.first().and_then(|f| list.get_element_group_id(*f));

                    if let Some(group) = reference_group_id {
                        selection = list.filter_indices_by_group(selection, group);
                    }

                    list.set_selection(selection, browser_event);
                    list.set_anchor(None);
                    list.dom_focus();
                }
                _ => {}
            }
        }));
    }

    fn reveal_focus_and_anchor(&self) {
        if let Some(el) = self.get_focus().first().copied() {
            self.set_anchor(Some(el));
            self.reveal(el, None, 0.0);
        } else {
            self.set_anchor(None);
        }
        self.dom_focus();
    }

    // -------------------- MouseController --------------------

    fn install_mouse_controller(&self) {
        if !self.options.view.mouse_support {
            return;
        }

        let weak = self.weak_self.clone();
        self.base.register(self.view.on_mouse_down().listen(move |e| {
            let Some(list) = weak.upgrade() else { return };
            match &list.options.overrides {
                Some(overrides) => overrides.mouse_down(&list, e),
                None => list.default_mouse_down(e),
            }
        }));

        let weak = self.weak_self.clone();
        self.base.register(self.view.on_mouse_click().listen(move |e| {
            let Some(list) = weak.upgrade() else { return };
            match &list.options.overrides {
                Some(overrides) => overrides.view_pointer(&list, e),
                None => list.default_view_pointer(e),
            }
        }));

        let weak = self.weak_self.clone();
        self.base.register(self.view.on_mouse_dbl_click().listen(move |e| {
            let Some(list) = weak.upgrade() else { return };
            match &list.options.overrides {
                Some(overrides) => overrides.double_click(&list, e),
                None => list.default_double_click(e),
            }
        }));
    }

    fn multiple_selection(&self) -> bool {
        self.options.multiple_selection_support != Some(false)
    }

    /// `MouseController.onMouseDown`.
    pub fn default_mouse_down(&self, _e: &ListMouseEvent<T>) {
        if !self.is_dom_focused() {
            self.dom_focus();
        }
    }

    /// `MouseController.onDoubleClick`.
    pub fn default_double_click(&self, e: &ListMouseEvent<T>) {
        if self.multiple_selection() && (Self::is_selection_single_change_event(e) || Self::is_selection_range_change_event(e)) {
            return;
        }
        if e.is_handled_by_list.get() {
            return;
        }
        e.is_handled_by_list.set(true);
        let focus = self.get_focus();
        self.set_selection(focus, Some(e.browser_event));
    }

    /// `MouseController.isSelectionRangeChangeEvent`.
    pub fn is_selection_range_change_event(event: &ListMouseEvent<T>) -> bool {
        event.browser_event.modifiers.shift
    }

    /// `MouseController.isSelectionSingleChangeEvent`.
    pub fn is_selection_single_change_event(event: &ListMouseEvent<T>) -> bool {
        if is_macintosh() { event.browser_event.modifiers.meta } else { event.browser_event.modifiers.ctrl }
    }

    /// `MouseController.onViewPointer`.
    pub fn default_view_pointer(&self, e: &ListMouseEvent<T>) {
        let multiple = self.multiple_selection();
        if e.is_handled_by_list.get() {
            return;
        }
        e.is_handled_by_list.set(true);

        let Some(focus) = e.index else {
            self.set_focus(Vec::new(), Some(e.browser_event));
            self.set_selection(Vec::new(), Some(e.browser_event));
            self.set_anchor(None);
            return;
        };

        if multiple && (Self::is_selection_single_change_event(e) || Self::is_selection_range_change_event(e)) {
            return self.change_selection(e, focus);
        }

        self.set_focus(vec![focus], Some(e.browser_event));
        self.set_anchor(Some(focus));

        if e.browser_event.button != PointerButton::Secondary {
            // `isMouseRightClick`
            let focus_group_id = self.get_element_group_id(focus);

            if focus_group_id != Some(GroupId::NotSelectable) {
                self.set_selection(vec![focus], Some(e.browser_event));
            }
        }

        self.on_pointer.fire(e.clone());
    }

    fn change_selection(&self, e: &ListMouseEvent<T>, focus: usize) {
        let mut anchor = self.get_anchor();

        if e.browser_event.modifiers.shift {
            let anchor = *anchor.get_or_insert_with(|| {
                let current_focus = self.get_focus().first().copied();
                let a = current_focus.unwrap_or(focus);
                self.set_anchor(Some(a));
                a
            });

            let (min, max) = (anchor.min(focus), anchor.max(focus));
            let mut range_selection: Vec<usize> = (min..=max).collect();
            let selected_element = self.get_selection().first().copied();

            if let Some(selected) = selected_element
                && let Some(reference_group_id) = self.get_element_group_id(selected)
            {
                range_selection = self.filter_indices_by_group(range_selection, reference_group_id);
            }

            let selection = self.get_selection();
            let contiguous_range = get_contiguous_range_containing(&disjunction(&selection, &[anchor]), anchor);

            if contiguous_range.is_empty() {
                return;
            }

            let new_selection = disjunction(&range_selection, &relative_complement(&selection, &contiguous_range));
            self.set_selection(new_selection, Some(e.browser_event));
            self.set_focus(vec![focus], Some(e.browser_event));
        } else if Self::is_selection_single_change_event(e) {
            let selection = self.get_selection();
            let new_selection: Vec<usize> = selection.iter().copied().filter(|i| *i != focus).collect();

            self.set_focus(vec![focus], None);
            self.set_anchor(Some(focus));

            let focus_group_id = self.get_element_group_id(focus);

            if focus_group_id == Some(GroupId::NotSelectable) {
                return; // não dá para selecionar este elemento
            }

            if selection.len() == new_selection.len() {
                let mut with_focus = new_selection;
                with_focus.push(focus);
                let items_to_be_selected = match focus_group_id {
                    Some(group) => self.filter_indices_by_group(with_focus, group),
                    None => with_focus,
                };
                self.set_selection(items_to_be_selected, Some(e.browser_event));
            } else {
                self.set_selection(new_selection, Some(e.browser_event));
            }
        }
    }
}

impl<T: Clone + 'static> IDisposable for List<T> {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.on_did_dispose.fire(());
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "List"
    }
}
