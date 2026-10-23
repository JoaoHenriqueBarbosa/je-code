//! Porte de `src/vs/base/browser/ui/actionbar/actionbar.ts`: a [`ActionBar`], uma fileira (ou coluna) de
//! botões de ações com navegação por teclado, foco e execução pelo [`ActionRunner`](je_base::actions).
//!
//! # API para quem consome (toolbar, dropdown, menu, dialog)
//!
//! - [`ActionBar::new`]`(ui, container, options)` cria a barra dentro de `container`; os itens são
//!   `Rc<dyn IActionViewItem>` (ver [`crate::action_view_items`]): [`ActionBar::push`] e
//!   [`ActionBar::push_action`] criam o item de cada ação com o `action_view_item_provider` das opções ou, se
//!   ele devolver `None`, com um [`ActionViewItem`].
//! - A barra é um [`IActionRunner`]: `on_did_run`/`on_will_run` repassam os eventos do runner (o das opções ou
//!   o padrão), `run` roda uma ação. `set_action_runner` troca o runner de todos os itens.
//! - Teclado (no `domNode`): seta anterior/próxima (esquerda/direita na horizontal, cima/baixo na vertical),
//!   `Home`/`End`, `Escape` (`on_did_cancel`, só se alguém escuta), `Enter`/`Space` (as `trigger_keys`, no
//!   `keydown` ou no `keyup`), `Tab` num item que captura setas. `focus_only_enabled_items` pula itens
//!   desabilitados, `prevent_loop_navigation` não dá a volta.
//! - Layout: o `je-ui` não tem motor de layout. A barra posiciona os `li` (fila de flex com
//!   `align-items: center` na horizontal, coluna na vertical) em [`ActionBar::layout`], que ela mesma chama
//!   em `push`/`pull`/`clear` e quando um item avisa que o tamanho mudou. Quem redimensiona o container da
//!   barra chama `layout()` de novo (o `height: 100%` do CSS é [`je_ui::Tree::set_fill`]). `get_width` e
//!   `get_height` leem o retângulo do `li`.
//!
//! Como o DOM vira [`je_ui`]: `div.monaco-action-bar` (+ `vertical`) é o `dom_node`; `ul.actions-container`
//! (papel `Toolbar`, ou o `aria_role`, `highlight-toggled` com `highlight_toggled_items`) é o filho; cada
//! item é um `li.action-item` (papel `Presentation`). O `tabIndex` do `actionsList.focus()` não existe: o
//! foco vai ao nó. O `aria-label` é o rótulo do nó.
//!
//! Fica de fora: o `contextmenu` nativo que o original impede nos itens (`allowContextMenu`; o `je-ui` não
//! tem esse evento, então a opção existe mas não muda nada) e o `DisposableMap` de `viewItemDisposables`
//! (o único ouvinte que ele guardava é o do `contextmenu`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, AltModeModifiers, IActionRunner, IRunEvent, Separator};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, EmitterOptions, Event as BaseEvent};
use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, id_of};
use je_base::promise::Promise;
use je_render::kurbo::Rect;
use je_ui::{Event, EventKind, NodeId, Role, Ui};

pub use crate::action_view_items::IActionViewItem;
use crate::action_view_items::{ActionBarStyles, ActionViewItem, IActionViewItemOptions};
use crate::common::{registered_emitter, registered_store};
use crate::dom::{FocusTracker, StandardKeyboardEvent, create_el, is_ancestor};
use crate::hover_api::{IHoverDelegate, create_instant_hover_delegate};

/// `ActionsOrientation`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActionsOrientation {
    #[default]
    Horizontal,
    Vertical,
}

/// `ActionTrigger`: as teclas que rodam a ação do item em foco e se rodam no `keydown` ou no `keyup`.
#[derive(Clone, Debug, Default)]
pub struct ActionTrigger {
    /// Padrão: `Enter` e `Space`.
    pub keys: Option<Vec<KeyCode>>,
    pub key_down: bool,
}

/// O `updateFocus` que uma subclasse sobrescreve: recebe `fromRight`.
pub type UpdateFocusOverride = Rc<dyn Fn(bool)>;

/// `IActionViewItemProvider`: cria o item de uma ação (`None` usa o [`ActionViewItem`] padrão).
pub type IActionViewItemProvider = Rc<dyn Fn(&ActionRef, &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>>>;

/// `IActionBarOptions`.
#[derive(Clone, Default)]
pub struct IActionBarOptions {
    pub orientation: Option<ActionsOrientation>,
    pub context: Option<Dynamic>,
    pub action_view_item_provider: Option<IActionViewItemProvider>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub aria_label: Option<String>,
    /// O `role` do `ul` (`toolbar`, `tablist`, `menu`...). Sem valor, `toolbar`.
    pub aria_role: Option<String>,
    pub trigger_keys: Option<ActionTrigger>,
    pub allow_context_menu: bool,
    pub prevent_loop_navigation: bool,
    pub focus_only_enabled_items: bool,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    /// Itens alternados (`checked`) ganham o fundo `actionBar.toggledBackground`.
    pub highlight_toggled_items: bool,
    /// As cores do CSS (não está no original). Sem valor, [`ActionBarStyles::default`].
    pub styles: Option<Rc<ActionBarStyles>>,
}

/// `IActionOptions`: as opções do item mais a posição de inserção.
#[derive(Clone, Default)]
pub struct IActionOptions {
    pub item: IActionViewItemOptions,
    pub index: Option<i64>,
}

/// O `role` do ARIA como papel do nó.
fn role_of(aria_role: &str) -> Role {
    match aria_role {
        "tablist" => Role::TabList,
        "menu" => Role::Menu,
        "presentation" => Role::Presentation,
        "toolbar" => Role::Toolbar,
        _ => Role::Generic,
    }
}

struct Entry {
    item: Rc<dyn IActionViewItem>,
    li: NodeId,
}

/// A barra de ações.
pub struct ActionBar {
    base: Rc<Disposable>,
    weak_self: Weak<ActionBar>,
    ui: Ui,
    options: IActionBarOptions,
    hover_delegate: Rc<dyn IHoverDelegate>,
    action_runner: RefCell<Rc<dyn IActionRunner>>,
    action_runner_disposables: Rc<DisposableStore>,
    context: RefCell<Option<Dynamic>>,
    orientation: ActionsOrientation,
    trigger_keys: (Vec<KeyCode>, bool),
    entries: RefCell<Vec<Entry>>,
    previously_focused_item: Cell<Option<usize>>,
    focused_item: Cell<Option<usize>>,
    trigger_key_down: Cell<bool>,
    focusable: Cell<bool>,
    cancel_has_listener: Rc<Cell<bool>>,
    /// O `updateFocus` que uma subclasse sobrescreve (o `Menu`): recebe `fromRight`.
    update_focus_override: RefCell<Option<UpdateFocusOverride>>,
    dom_node: NodeId,
    actions_list: NodeId,
    styles: Rc<ActionBarStyles>,
    on_did_blur: Rc<Emitter<()>>,
    on_did_cancel: Rc<Emitter<()>>,
    on_did_run: Rc<Emitter<IRunEvent>>,
    on_will_run: Rc<Emitter<IRunEvent>>,
    /// Os ouvintes dos itens dão a volta nesta barra por `Weak`, mas o foco usa o tracker.
    _focus_tracker: Rc<FocusTracker>,
}

impl ActionBar {
    /// `new ActionBar(container, options)`.
    pub fn new(ui: &Ui, container: NodeId, options: IActionBarOptions) -> Rc<Self> {
        let base = Disposable::new();
        let orientation = options.orientation.unwrap_or_default();
        let trigger_keys = (
            options.trigger_keys.as_ref().and_then(|t| t.keys.clone()).unwrap_or_else(|| vec![KeyCode::Enter, KeyCode::Space]),
            options.trigger_keys.as_ref().is_some_and(|t| t.key_down),
        );
        let hover_delegate: Rc<dyn IHoverDelegate> = match options.hover_delegate.clone() {
            Some(delegate) => delegate,
            None => {
                let scoped = create_instant_hover_delegate();
                base.register(scoped.clone());
                scoped
            }
        };
        let action_runner_disposables = registered_store(&base);
        let action_runner: Rc<dyn IActionRunner> = match options.action_runner.clone() {
            Some(runner) => runner,
            None => {
                let runner = ActionRunner::new();
                action_runner_disposables.add_rc(runner.clone());
                runner
            }
        };
        let cancel_has_listener = Rc::new(Cell::new(false));
        let on_did_blur = registered_emitter::<()>(&base);
        let on_did_cancel = {
            let flag = cancel_has_listener.clone();
            let emitter = Emitter::with_options(EmitterOptions { on_will_add_first_listener: Some(Rc::new(move |_| flag.set(true))), ..Default::default() });
            base.register(emitter.clone());
            emitter
        };
        let on_did_run = registered_emitter::<IRunEvent>(&base);
        let on_will_run = registered_emitter::<IRunEvent>(&base);
        {
            let (run, will_run) = (on_did_run.clone(), on_will_run.clone());
            action_runner_disposables.add(action_runner.on_did_run().listen(move |e| run.fire(e.clone())));
            action_runner_disposables.add(action_runner.on_will_run().listen(move |e| will_run.fire(e.clone())));
        }

        let (dom_node, actions_list) = {
            let mut tree = ui.tree_mut();
            let dom_node = create_el(&mut tree, container, Role::Generic, "monaco-action-bar");
            if orientation == ActionsOrientation::Vertical {
                tree.add_class(dom_node, "vertical");
            }
            tree.set_fill(dom_node, true, true);
            let aria_role = options.aria_role.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "toolbar".to_owned());
            let actions_list = create_el(&mut tree, dom_node, role_of(&aria_role), "actions-container");
            tree.set_fill(actions_list, true, true);
            if options.highlight_toggled_items {
                tree.add_class(actions_list, "highlight-toggled");
            }
            if let Some(label) = options.aria_label.as_ref().filter(|l| !l.is_empty()) {
                tree.set_label(actions_list, label.clone());
            }
            (dom_node, actions_list)
        };
        let focus_tracker = FocusTracker::new(ui, dom_node);
        base.register(focus_tracker.clone());
        let styles = options.styles.clone().unwrap_or_default();

        let this = Rc::new_cyclic(|weak| ActionBar {
            weak_self: weak.clone(),
            ui: ui.clone(),
            hover_delegate,
            action_runner: RefCell::new(action_runner),
            action_runner_disposables,
            context: RefCell::new(options.context.clone()),
            orientation,
            trigger_keys,
            entries: RefCell::new(Vec::new()),
            previously_focused_item: Cell::new(None),
            focused_item: Cell::new(None),
            trigger_key_down: Cell::new(false),
            focusable: Cell::new(true),
            cancel_has_listener,
            update_focus_override: RefCell::new(None),
            dom_node,
            actions_list,
            styles,
            on_did_blur,
            on_did_cancel,
            on_did_run,
            on_will_run,
            _focus_tracker: focus_tracker.clone(),
            options,
            base,
        });

        let weak = this.weak_self.clone();
        this.base.register(ui.listen(dom_node, EventKind::KeyDown, false, {
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_key_down(e);
                }
            }
        }));
        this.base.register(ui.listen(dom_node, EventKind::KeyUp, false, {
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_key_up(e);
                }
            }
        }));
        this.base.register(focus_tracker.on_did_blur().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    let active = this.ui.focused();
                    if active == Some(this.dom_node) || !active.is_some_and(|a| is_ancestor(&this.ui, a, this.dom_node)) {
                        this.on_did_blur.fire(());
                        this.previously_focused_item.set(this.focused_item.get());
                        this.focused_item.set(None);
                        this.trigger_key_down.set(false);
                    }
                }
            }
        }));
        this.base.register(focus_tracker.on_did_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_focused_item();
            }
        }));
        this
    }

    fn on_key_down(&self, e: &mut Event) {
        let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
        let (previous_key, next_key) = match self.orientation {
            ActionsOrientation::Horizontal => (KeyCode::LeftArrow, KeyCode::RightArrow),
            ActionsOrientation::Vertical => (KeyCode::UpArrow, KeyCode::DownArrow),
        };
        let focused_item = self.focused_item.get().and_then(|i| self.item_at(i));
        let mut event_handled = true;
        if event.equals(previous_key.0 as u32) {
            event_handled = self.focus_previous(false);
        } else if event.equals(next_key.0 as u32) {
            event_handled = self.focus_next(false, false);
        } else if event.equals(KeyCode::Escape.0 as u32) && self.cancel_has_listener.get() {
            self.on_did_cancel.fire(());
        } else if event.equals(KeyCode::Home.0 as u32) {
            event_handled = self.focus_first();
        } else if event.equals(KeyCode::End.0 as u32) {
            event_handled = self.focus_last();
        } else if event.equals(KeyCode::Tab.0 as u32) && focused_item.is_some_and(|item| item.traps_arrow_navigation()) {
            // Tab, então força o foco no próximo (#219199)
            event_handled = self.focus_next(false, true);
        } else if self.is_trigger_key_event(&event) {
            // Fica fora do ramo `else` mesmo sem disparar
            if self.trigger_keys.1 {
                self.do_trigger(&event);
            } else {
                self.trigger_key_down.set(true);
            }
        } else {
            event_handled = false;
        }
        if event_handled {
            e.stop(true);
        }
    }

    fn on_key_up(&self, e: &mut Event) {
        let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
        // Roda a ação com Enter/Space
        if self.is_trigger_key_event(&event) {
            if !self.trigger_keys.1 && self.trigger_key_down.get() {
                self.trigger_key_down.set(false);
                self.do_trigger(&event);
            }
            e.stop(true);
        } else if [KeyCode::Tab.0 as u32, KeyMod::SHIFT | KeyCode::Tab.0 as u32, KeyCode::UpArrow.0 as u32, KeyCode::DownArrow.0 as u32, KeyCode::LeftArrow.0 as u32, KeyCode::RightArrow.0 as u32].iter().any(|k| event.equals(*k)) {
            // Recalcula o item em foco
            self.update_focused_item();
        }
    }

    fn item_at(&self, index: usize) -> Option<Rc<dyn IActionViewItem>> {
        self.entries.borrow().get(index).map(|e| e.item.clone())
    }

    /// `viewItems`.
    pub fn view_items(&self) -> Vec<Rc<dyn IActionViewItem>> {
        self.entries.borrow().iter().map(|e| e.item.clone()).collect()
    }

    /// `domNode`.
    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// `actionsList` (o `ul.actions-container`).
    pub fn actions_list(&self) -> NodeId {
        self.actions_list
    }

    /// `getContainer()`.
    pub fn get_container(&self) -> NodeId {
        self.dom_node
    }

    pub fn on_did_blur(&self) -> BaseEvent<()> {
        self.on_did_blur.event()
    }

    /// `onDidCancel`: o `Escape` (só é tratado se alguém escuta).
    pub fn on_did_cancel(&self) -> BaseEvent<()> {
        self.on_did_cancel.event()
    }

    pub fn on_did_run(&self) -> BaseEvent<IRunEvent> {
        self.on_did_run.event()
    }

    pub fn on_will_run(&self) -> BaseEvent<IRunEvent> {
        self.on_will_run.event()
    }

    /// `focusedItem` (índice do item em foco, para as subclasses e os testes).
    pub fn focused_item(&self) -> Option<usize> {
        self.focused_item.get()
    }

    fn refresh_role(&self) {
        let role = if self.length() >= 1 { role_of(&self.options.aria_role.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "toolbar".to_owned())) } else { Role::Presentation };
        self.ui.tree_mut().set_role(self.actions_list, role);
    }

    /// `setAriaLabel(label)`: vazio tira o rótulo.
    pub fn set_aria_label(&self, label: &str) {
        let mut tree = self.ui.tree_mut();
        if label.is_empty() {
            tree.clear_label(self.actions_list);
        } else {
            tree.set_label(self.actions_list, label);
        }
    }

    /// `setFocusable(focusable)`: uma barra que não pode ser focada (e volta a poder) deixa só o primeiro
    /// item habilitado focável.
    pub fn set_focusable(&self, focusable: bool) {
        self.focusable.set(focusable);
        let items = self.view_items();
        if focusable {
            if let Some(first_enabled) = items.iter().find(|item| item.is_enabled()) {
                first_enabled.set_focusable(true);
            }
        } else {
            for item in &items {
                item.set_focusable(false);
            }
        }
    }

    fn is_trigger_key_event(&self, event: &StandardKeyboardEvent) -> bool {
        let item = self.focused_item.get().and_then(|i| self.item_at(i));
        let modifiers = AltModeModifiers { alt_key: event.alt_key, shift_key: event.shift_key };
        let has_alternative = item.is_some_and(|item| id_of(&item.get_action_to_run(modifiers)) != id_of(&item.base().action()));
        self.trigger_keys.0.iter().any(|key| {
            let code = key.0 as u32;
            event.equals(code) || (has_alternative && event.equals(code | if event.alt_key { KeyMod::ALT } else { 0 } | if event.shift_key { KeyMod::SHIFT } else { 0 }))
        })
    }

    fn update_focused_item(&self) {
        let Some(active) = self.ui.focused() else { return };
        let children = self.ui.tree().children(self.actions_list).to_vec();
        for (i, li) in children.into_iter().enumerate() {
            if is_ancestor(&self.ui, active, li) {
                self.focused_item.set(Some(i));
                if let Some(item) = self.item_at(i) {
                    item.show_hover();
                }
                break;
            }
        }
    }

    /// `get context`.
    pub fn context(&self) -> Option<Dynamic> {
        self.context.borrow().clone()
    }

    /// `set context`: repassa aos itens.
    pub fn set_context(&self, context: Option<Dynamic>) {
        *self.context.borrow_mut() = context.clone();
        for item in self.view_items() {
            item.set_action_context(context.clone());
        }
    }

    /// `get actionRunner`.
    pub fn action_runner(&self) -> Rc<dyn IActionRunner> {
        self.action_runner.borrow().clone()
    }

    /// `set actionRunner`: solta os ouvintes do runner antigo e repassa os eventos do novo.
    pub fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        *self.action_runner.borrow_mut() = action_runner.clone();
        self.action_runner_disposables.clear();
        let (run, will_run) = (self.on_did_run.clone(), self.on_will_run.clone());
        self.action_runner_disposables.add(action_runner.on_did_run().listen(move |e| run.fire(e.clone())));
        self.action_runner_disposables.add(action_runner.on_will_run().listen(move |e| will_run.fire(e.clone())));
        for item in self.view_items() {
            item.set_action_runner(action_runner.clone());
        }
    }

    /// `hasAction(action)`: por id.
    pub fn has_action(&self, action: &ActionRef) -> bool {
        let id = action.id();
        self.entries.borrow().iter().any(|e| e.item.action().id() == id)
    }

    /// `getAction(index)`.
    pub fn get_action(&self, index: usize) -> Option<ActionRef> {
        self.item_at(index).map(|item| item.action())
    }

    /// `getAction(element)`: a ação do item que contém o nó.
    pub fn get_action_of_node(&self, node: NodeId) -> Option<ActionRef> {
        let index = {
            let tree = self.ui.tree();
            let mut current = node;
            while tree.parent(current)? != self.actions_list {
                current = tree.parent(current)?;
            }
            tree.children(self.actions_list).iter().position(|c| *c == current)?
        };
        self.get_action(index)
    }

    /// `push(action, options)`.
    pub fn push_action(&self, action: ActionRef, options: IActionOptions) {
        self.push(&[action], options);
    }

    /// `push(actions, options)`: cria o item de cada ação e o insere em `options.index` (no fim por padrão).
    pub fn push(&self, actions: &[ActionRef], options: IActionOptions) {
        let mut index = options.index;
        for action in actions {
            let li = {
                let mut tree = self.ui.tree_mut();
                let li = tree.create_detached(Role::Presentation);
                tree.set_class_name(li, "action-item");
                li
            };

            let mut item_options = options.item.clone();
            item_options.hover_delegate = options.item.hover_delegate.clone().or_else(|| Some(self.hover_delegate.clone()));
            item_options.is_tab_list = self.options.aria_role.as_deref() == Some("tablist");
            item_options.styles = options.item.styles.clone().or_else(|| Some(self.styles.clone()));
            let provided = self.options.action_view_item_provider.as_ref().and_then(|provider| provider(action, &item_options));
            let item = provided.unwrap_or_else(|| ActionViewItem::new(&self.ui, self.context(), action.clone(), item_options.clone()) as Rc<dyn IActionViewItem>);

            item.set_action_runner(self.action_runner());
            item.set_action_context(self.context());
            let weak = self.weak_self.clone();
            item.base().set_layout_requester(Some(Rc::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.layout();
                }
            })));
            {
                let mut tree = self.ui.tree_mut();
                match index {
                    Some(i) if i >= 0 && (i as usize) < tree.children(self.actions_list).len() => {
                        let reference = tree.children(self.actions_list)[i as usize];
                        tree.insert_before(self.actions_list, li, Some(reference));
                    }
                    _ => tree.append_child(self.actions_list, li),
                }
            }
            item.render(li);
            {
                let mut entries = self.entries.borrow_mut();
                match index {
                    Some(i) if i >= 0 && (i as usize) < entries.len() => {
                        entries.insert(i as usize, Entry { item, li });
                        index = Some(i + 1);
                    }
                    _ => entries.push(Entry { item, li }),
                }
            }
        }

        // O primeiro item habilitado precisa poder receber o foco por Tab (#106441)
        if self.focusable.get() {
            let mut did_focus = false;
            for item in self.view_items() {
                // já focou um item, nunca um separador, nunca um desabilitado quando só os habilitados contam
                let focus = !(did_focus || item.action().id() == Separator::ID || (!item.is_enabled() && self.options.focus_only_enabled_items));
                item.set_focusable(focus);
                did_focus = did_focus || focus;
            }
        }

        if let Some(focused) = self.focused_item.get() {
            // Depois de um clear, ações podem ser recolocadas só para alternar algumas: preserva o foco (#97128)
            self.focus_index(focused);
        }
        self.refresh_role();
        self.layout();
    }

    /// Posiciona os `li` como o CSS faria (flex horizontal com `align-items: center`, ou coluna).
    pub fn layout(&self) {
        let vertical = self.orientation == ActionsOrientation::Vertical;
        let entries: Vec<(Rc<dyn IActionViewItem>, NodeId)> = self.entries.borrow().iter().map(|e| (e.item.clone(), e.li)).collect();
        let container = self.ui.tree().bounds(self.actions_list);
        let mut offset = 0.0;
        for (item, li) in entries {
            let (mut width, height) = item.size(vertical);
            if vertical {
                width = container.width();
            }
            let (x, y) = if vertical { (0.0, offset) } else { (offset, (container.height() - height) / 2.0) };
            self.ui.tree_mut().set_bounds(li, Rect::new(x, y, x + width, y + height));
            offset += if vertical { height } else { width };
            item.layout(vertical, width, height);
        }
    }

    /// `getWidth(index)`: o `clientWidth` do `li`.
    pub fn get_width(&self, index: usize) -> f64 {
        self.li_at(index).map_or(0.0, |li| self.ui.tree().bounds(li).width())
    }

    /// `getHeight(index)`.
    pub fn get_height(&self, index: usize) -> f64 {
        self.li_at(index).map_or(0.0, |li| self.ui.tree().bounds(li).height())
    }

    fn li_at(&self, index: usize) -> Option<NodeId> {
        self.entries.borrow().get(index).map(|e| e.li)
    }

    /// `pull(index)`: tira e descarta o item.
    pub fn pull(&self, index: usize) {
        if index >= self.entries.borrow().len() {
            return;
        }
        let entry = self.entries.borrow_mut().remove(index);
        // O `dispose` do item tira o elemento dele da árvore; o `removeChild(li)` do original tira o `li` (que
        // para um item que guarda outro nó em `element`, como o `DropdownMenuActionViewItem`, é outro nó).
        entry.item.dispose();
        self.ui.remove(entry.li);
        self.refresh_role();
        self.layout();
    }

    /// `clear()`.
    pub fn clear(&self) {
        if self.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        for entry in entries {
            entry.item.dispose();
        }
        let children = self.ui.tree().children(self.actions_list).to_vec();
        for child in children {
            self.ui.remove(child);
        }
        self.refresh_role();
    }

    pub fn length(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// `isFocused()`: o foco está na barra.
    pub fn is_focused(&self) -> bool {
        self.ui.focused().is_some_and(|f| is_ancestor(&self.ui, f, self.dom_node))
    }

    /// `isFocused(index)`: o foco está no item.
    pub fn is_item_focused(&self, index: usize) -> bool {
        self.li_at(index).is_some_and(|li| self.ui.focused().is_some_and(|f| is_ancestor(&self.ui, f, li)))
    }

    /// `focus(selectFirst)`: com `true` e nada em foco, foca o primeiro item habilitado; senão refoca o item.
    pub fn focus(&self, select_first: bool) {
        if select_first && self.focused_item.get().is_none() {
            let first_enabled = self.view_items().iter().position(|item| item.is_enabled());
            // Foca o primeiro item habilitado
            self.focused_item.set(first_enabled);
            self.update_focus(false, true);
        } else {
            self.update_focus(false, true);
        }
    }

    /// `focus(index)`.
    pub fn focus_index(&self, index: usize) {
        self.focused_item.set(Some(index));
        self.update_focus(false, true);
    }

    fn focus_first(&self) -> bool {
        self.focused_item.set(self.length().checked_sub(1));
        self.focus_next(true, false)
    }

    fn focus_last(&self) -> bool {
        self.focused_item.set(Some(0));
        self.focus_previous(true)
    }

    /// `focusNext(forceLoop, forceFocus)`.
    pub fn focus_next(&self, force_loop: bool, force_focus: bool) -> bool {
        let len = self.length();
        match self.focused_item.get() {
            None => self.focused_item.set(len.checked_sub(1)),
            Some(_) if len <= 1 => return false,
            Some(_) => {}
        }
        let Some(start_index) = self.focused_item.get() else { return false };
        let mut focused = start_index;
        loop {
            if !force_loop && self.options.prevent_loop_navigation && focused + 1 >= len {
                self.focused_item.set(Some(start_index));
                return false;
            }
            focused = (focused + 1) % len;
            self.focused_item.set(Some(focused));
            let item = self.item_at(focused).expect("índice dentro dos itens");
            if !(focused != start_index && ((self.options.focus_only_enabled_items && !item.is_enabled()) || item.action().id() == Separator::ID)) {
                break;
            }
        }
        self.update_focus(false, force_focus);
        true
    }

    /// `focusPrevious(forceLoop)`.
    pub fn focus_previous(&self, force_loop: bool) -> bool {
        let len = self.length();
        match self.focused_item.get() {
            None => self.focused_item.set(Some(0)),
            Some(_) if len <= 1 => return false,
            Some(_) => {}
        }
        let Some(start_index) = self.focused_item.get() else { return false };
        let mut focused = start_index;
        loop {
            if focused == 0 {
                if !force_loop && self.options.prevent_loop_navigation {
                    self.focused_item.set(Some(start_index));
                    return false;
                }
                focused = len - 1;
            } else {
                focused -= 1;
            }
            self.focused_item.set(Some(focused));
            let item = self.item_at(focused).expect("índice dentro dos itens");
            if !(focused != start_index && ((self.options.focus_only_enabled_items && !item.is_enabled()) || item.action().id() == Separator::ID)) {
                break;
            }
        }
        self.update_focus(true, false);
        true
    }

    /// `focusedItem = index` (o campo `protected` que o `Menu` escreve).
    pub fn set_focused_item(&self, index: Option<usize>) {
        self.focused_item.set(index);
    }

    /// Troca o `updateFocus` por `handler` (o `override` do `Menu`): ele chama [`ActionBar::update_focus_base`]
    /// (o `super.updateFocus`) e acrescenta o que a subclasse faz.
    pub fn set_update_focus_override(&self, handler: Option<UpdateFocusOverride>) {
        *self.update_focus_override.borrow_mut() = handler;
    }

    fn update_focus(&self, from_right: bool, force_focus: bool) {
        let handler = self.update_focus_override.borrow().clone();
        match handler {
            Some(handler) => handler(from_right),
            None => self.update_focus_base(from_right, force_focus),
        }
    }

    /// `updateFocus(fromRight, forceFocus)` da `ActionBar` (o `super.updateFocus` de uma subclasse).
    pub fn update_focus_base(&self, from_right: bool, force_focus: bool) {
        if self.focused_item.get().is_none() {
            self.ui.focus(self.actions_list);
        }
        if let Some(previous) = self.previously_focused_item.get()
            && Some(previous) != self.focused_item.get()
            && let Some(item) = self.item_at(previous)
        {
            item.blur();
        }
        let Some(action_view_item) = self.focused_item.get().and_then(|i| self.item_at(i)) else { return };
        let mut focus_item = true;
        if self.options.focus_only_enabled_items && !action_view_item.is_enabled() {
            focus_item = false;
        }
        if action_view_item.action().id() == Separator::ID {
            focus_item = false;
        }
        if !focus_item {
            self.ui.focus(self.actions_list);
            self.previously_focused_item.set(None);
        } else if force_focus || self.previously_focused_item.get() != self.focused_item.get() {
            action_view_item.focus(from_right);
            self.previously_focused_item.set(self.focused_item.get());
        }
        if focus_item {
            action_view_item.show_hover();
        }
    }

    fn do_trigger(&self, event: &StandardKeyboardEvent) {
        // nada em foco
        let Some(item) = self.focused_item.get().and_then(|i| self.item_at(i)) else { return };
        let context = item.base().context().unwrap_or_else(|| {
            use je_base::dynamic::ObjectRef;
            Dynamic::Object(ObjectRef::from_entries([("shiftKey", Dynamic::Bool(event.shift_key)), ("ctrlKey", Dynamic::Bool(event.ctrl_key)), ("altKey", Dynamic::Bool(event.alt_key)), ("metaKey", Dynamic::Bool(event.meta_key))]))
        });
        let action = item.get_action_to_run(AltModeModifiers { alt_key: event.alt_key, shift_key: event.shift_key });
        // `run` devolve a promessa, ignorada como no original.
        let _ = self.run(&action, Some(context));
    }
}

impl IActionRunner for ActionBar {
    fn on_did_run(&self) -> BaseEvent<IRunEvent> {
        self.on_did_run.event()
    }

    fn on_will_run(&self) -> BaseEvent<IRunEvent> {
        self.on_will_run.event()
    }

    /// `run(action, context)`: o runner da barra roda a ação.
    fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> Promise<()> {
        self.action_runner().run(action, context)
    }
}

impl IDisposable for ActionBar {
    fn dispose(&self) {
        *self.context.borrow_mut() = None;
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        for entry in entries {
            entry.item.dispose();
        }
        self.ui.remove(self.dom_node);
        self.base.dispose();
    }
}

/// `prepareActions(actions)`: tira separadores no começo, no fim e repetidos.
pub fn prepare_actions(mut actions: Vec<ActionRef>) -> Vec<ActionRef> {
    if actions.is_empty() {
        return actions;
    }
    // Separadores no começo
    let Some(first_index_of_action) = actions.iter().position(|a| a.id() != Separator::ID) else { return Vec::new() };
    actions.drain(..first_index_of_action);
    // Separadores no fim
    while actions.last().is_some_and(|a| a.id() == Separator::ID) {
        actions.pop();
    }
    // Separadores repetidos
    let mut found_action = false;
    for k in (0..actions.len()).rev() {
        let is_separator = actions[k].id() == Separator::ID;
        if is_separator && !found_action {
            actions.remove(k);
        } else {
            found_action = !is_separator;
        }
    }
    actions
}
