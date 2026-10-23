//! Porte de `src/vs/base/browser/ui/menu/menu.ts`: o [`Menu`], um menu vertical de ações com teclado
//! completo (setas, `Home`/`End`, `Enter`, `Escape`), mnemônicos (`&&`, `Alt`+letra), submenus, ações
//! alternativas (`Alt` e `Shift`), marca de `checked`, atalho, separadores e rolagem quando passa da
//! altura da janela.
//!
//! # API para quem consome (barra de menus, menu de contexto, dropdown)
//!
//! - [`Menu::new`]`(ui, container, actions, options, styles)` monta o menu dentro de `container` (o nó que o
//!   chamador dá: um [`ContextView`](crate::context_view::ContextView), o `menubar-menu-items-holder`...). O
//!   `Menu` "herda" a [`ActionBar`](crate::action_bar::ActionBar): [`Deref`] para ela, então `focus`,
//!   `view_items`, `on_did_cancel`, `is_item_focused`, `run` e o resto são os da barra. O que o `Menu` acrescenta
//!   é `trigger`, `get_container`, `on_scroll`, `scroll_offset`, `menu_items` e `relayout`.
//! - O contêiner passa a ter o tamanho do menu (o CSS dá `position: absolute` shrink-to-fit); quem o posiciona
//!   (a visão de contexto) lê o tamanho depois do `new`.
//! - [`IMenuStyles`] traz as cores resolvidas: [`default_menu_styles`] (o `defaultMenuStyles`) e
//!   [`unthemed_menu_styles`].
//!
//! # Como o DOM e o CSS viram `je-ui`
//!
//! `div.monaco-menu` (borda de 1px, raio 8, `min-width: 160px`) é dentro de um `div.monaco-scrollable-element`
//! (fundo, cor e raio do tema) que o [`AbstractScrollableElement`] cria; a barra de ações vertical tem
//! `padding: 4px 0`. O `DomScrollableElement` não existe (ele lê `scrollTop` de um `HTMLElement`): a barra de
//! ações é deslocada por `-scrollTop` e o recorte só liga quando há rolagem. Sem motor de layout, o menu mede
//! os itens ([`IActionViewItem::size`]) e dá a largura do maior (o shrink-to-fit do CSS) e a altura da soma, até
//! `max(10, alturaDaJanela - topoDoContêiner - 35)`.
//!
//! O `focus()` do DOM rola o item para a vista; o `je-ui` não. [`Menu::reveal_focused_item`] faz isso (centra o
//! item se ele não está inteiro na vista, como o Chromium).
//!
//! # Fica de fora
//!
//! - `getMenuWidgetCSS` (a folha de estilo é lida como especificação e aplicada nos nós), exceto
//!   [`format_rule`]; `Gesture`/toque; a sombra (`box-shadow` não existe no `je-ui`); o vidro fosco, as animações
//!   de abertura e fechamento e as regras de alto contraste (`hc-black`/`hc-light`).
//! - `Menu.globalStyleSheet`/`initializeOrUpdateStyleSheet`: sem CSS não há folha.
//! - O mnemônico vem de [`KeyCodeUtils::to_string`] (`e.key`): teclas de letra e número, não o caractere de
//!   outros alfabetos (o evento do `je-ui` não traz o caractere).
//! - Num menu que rola, o recorte (`overflow`) corta também os submenus dele (no DOM, `position: fixed` escapa
//!   do recorte; o `je-ui` não tem isso).
//! - `getCheckedActionsRepresentation` e o radio do pedido original não existem no menu do VS Code 1.143.1: a
//!   marca é só `checked` (`menuitemcheckbox`).

pub mod item;
pub mod mnemonics;
pub mod styles;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner, Separator, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::event::Event as BaseEvent;
use je_base::key_codes::{KeyCode, KeyCodeUtils};
use je_base::keybindings::ResolvedKeybinding;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, ScrollEvent};
use je_ui::{Border, EventKind, NodeId, Role, Ui};

use crate::action_bar::{ActionBar, ActionTrigger, ActionsOrientation, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use crate::action_view_items::{ActionBarStyles, ActionViewItem, IActionViewItem, IActionViewItemOptions};
use crate::dom::{StandardKeyboardEvent, create_detached_el, window_size};
use crate::modifier_key_emitter::ModifierKeyEmitter;
use crate::sash::is_macintosh;
use crate::scrollbar::scrollable_element::AbstractScrollableElement;
use crate::scrollbar::scrollable_element_options::ScrollableElementCreationOptions;

pub use item::{IMenuItemOptions, MenuActionViewItem};
pub use mnemonics::{MnemonicMatch, clean_mnemonic, exec_menu_mnemonic};
pub use styles::{IMenuStyles, default_menu_styles, format_rule, unthemed_menu_styles};

use item::{MenuItemContext, SubmenuInit};
use styles::{
    MENU_BORDER_WIDTH, MENU_CORNER_RADIUS_LARGE, MENU_MIN_WIDTH, MENU_SCROLLBAR_SIZE, MENU_VERTICAL_PADDING,
};

/// `HorizontalDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalDirection {
    Right,
    Left,
}

/// `VerticalDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalDirection {
    Above,
    Below,
}

/// `IMenuDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IMenuDirection {
    pub horizontal: HorizontalDirection,
    pub vertical: VerticalDirection,
}

/// `getKeyBinding`: o atalho de uma ação.
pub type GetKeyBinding = Rc<dyn Fn(&ActionRef) -> Option<Rc<dyn ResolvedKeybinding>>>;

/// `IMenuOptions`.
#[derive(Clone, Default)]
pub struct IMenuOptions {
    pub context: Option<Dynamic>,
    /// No original o campo existe mas o `Menu` não o usa (`doGetActionViewItem` cria os itens).
    pub action_view_item_provider: Option<IActionViewItemProvider>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub get_key_binding: Option<GetKeyBinding>,
    pub aria_label: Option<String>,
    pub enable_mnemonics: bool,
    pub anchor_alignment: Option<AnchorAlignment>,
    pub expand_direction: Option<IMenuDirection>,
    pub use_event_as_context: bool,
    pub submenu_ids: Option<HashSet<String>>,
}

/// `ISubMenuData`: o menu pai e o submenu aberto a partir dele (compartilhado por todos os itens do pai).
pub struct ParentData {
    pub parent: Weak<Menu>,
    pub submenu: RefCell<Option<Rc<Menu>>>,
}

/// O `movementX`/`movementY` de um `mousemove`: o `je-ui` só dá a posição, então o menu guarda a anterior. O
/// primeiro evento nunca conta como movimento.
#[derive(Default)]
pub struct PointerMotion {
    last: Cell<Option<(f64, f64)>>,
    /// O evento em despacho moveu o ponteiro (`movementX !== 0 || movementY !== 0`).
    pub moved: Cell<bool>,
}

/// Um item da barra do menu, com o tipo que o `instanceof` do original distingue.
#[derive(Clone)]
pub enum MenuItem {
    /// `BaseMenuActionViewItem` ou `SubmenuMenuActionViewItem` (`is_submenu`).
    Action(Rc<MenuActionViewItem>),
    /// `MenuSeparatorActionViewItem`.
    Separator(Rc<ActionViewItem>),
}

/// As medidas do último `relayout`.
#[derive(Clone, Copy, Debug, Default)]
struct MenuSize {
    /// A largura do conteúdo (sem a borda).
    content_width: f64,
    /// A altura do conteúdo da barra (itens mais o `padding: 4px 0`).
    content_height: f64,
    /// A altura visível, sem a borda.
    inner_height: f64,
}

pub struct Menu {
    bar: Rc<ActionBar>,
    weak_self: Weak<Menu>,
    ui: Ui,
    base: Rc<Disposable>,
    container: NodeId,
    menu_element: NodeId,
    scrollable: Rc<AbstractScrollableElement>,
    options: IMenuOptions,
    menu_styles: Rc<IMenuStyles>,
    mnemonics: RefCell<HashMap<String, Vec<Rc<MenuActionViewItem>>>>,
    items: RefCell<Vec<MenuItem>>,
    max_height: f64,
    motion: Rc<PointerMotion>,
    focus_visible: Rc<Cell<bool>>,
    size: Cell<MenuSize>,
}

impl Deref for Menu {
    type Target = ActionBar;

    fn deref(&self) -> &ActionBar {
        &self.bar
    }
}

impl Menu {
    /// `new Menu(container, actions, options, menuStyles)`.
    pub fn new(ui: &Ui, container: NodeId, actions: &[ActionRef], options: IMenuOptions, menu_styles: Rc<IMenuStyles>) -> Rc<Self> {
        Self::new_sharing_modality(ui, container, actions, options, menu_styles, Rc::new(Cell::new(false)))
    }

    /// `new Menu(...)` de um submenu: `:focus-visible` é do navegador todo (a última interação foi o teclado),
    /// então o menu pai e os submenus dele dividem o `focus_visible`.
    pub(crate) fn new_sharing_modality(ui: &Ui, container: NodeId, actions: &[ActionRef], options: IMenuOptions, menu_styles: Rc<IMenuStyles>, focus_visible: Rc<Cell<bool>>) -> Rc<Self> {
        let menu_element = {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "monaco-menu-container");
            tree.set_role(container, Role::Presentation);
            create_detached_el(&mut tree, Role::Presentation, "monaco-menu")
        };
        let motion = Rc::new(PointerMotion::default());
        let base = Disposable::new();

        let this = Rc::new_cyclic(|weak: &Weak<Menu>| {
            let parent_data = Rc::new(ParentData { parent: weak.clone(), submenu: RefCell::new(None) });
            let provider: IActionViewItemProvider = {
                let weak = weak.clone();
                Rc::new(move |action: &ActionRef, _: &IActionViewItemOptions| {
                    let menu = weak.upgrade()?;
                    Some(menu.do_get_action_view_item(action, &parent_data))
                })
            };
            let bar = ActionBar::new(
                ui,
                menu_element,
                IActionBarOptions {
                    orientation: Some(ActionsOrientation::Vertical),
                    context: options.context.clone(),
                    action_view_item_provider: Some(provider),
                    action_runner: options.action_runner.clone(),
                    aria_label: options.aria_label.clone(),
                    aria_role: Some("menu".to_owned()),
                    focus_only_enabled_items: true,
                    trigger_keys: Some(ActionTrigger {
                        keys: Some(if is_macintosh() || crate::action_view_items::host_platform().is_linux { vec![KeyCode::Enter, KeyCode::Space] } else { vec![KeyCode::Enter] }),
                        key_down: true,
                    }),
                    ..Default::default()
                },
            );
            // Rolagem: `alwaysConsumeMouseWheel`, só a barra vertical, `useShadows`.
            let scrollable = AbstractScrollableElement::scrollable_element(
                ui,
                menu_element,
                ScrollableElementCreationOptions {
                    always_consume_mouse_wheel: Some(true),
                    horizontal: Some(je_base::scrollable::ScrollbarVisibility::Hidden),
                    vertical: Some(je_base::scrollable::ScrollbarVisibility::Visible),
                    vertical_scrollbar_size: Some(MENU_SCROLLBAR_SIZE),
                    handle_mouse_wheel: Some(true),
                    use_shadows: Some(true),
                    ..Default::default()
                },
            );
            let max_height = {
                let tree = ui.tree();
                let top = tree.absolute_bounds(container).y0;
                (window_size(ui).1 - top - 35.0).max(10.0)
            };
            Menu {
                bar,
                weak_self: weak.clone(),
                ui: ui.clone(),
                base: base.clone(),
                container,
                menu_element,
                scrollable,
                options: options.clone(),
                menu_styles,
                mnemonics: RefCell::new(HashMap::new()),
                items: RefCell::new(Vec::new()),
                max_height,
                motion: motion.clone(),
                focus_visible: focus_visible.clone(),
                size: Cell::new(MenuSize::default()),
            }
        });
        this.base.register(this.scrollable.clone());
        this.setup(actions);
        this
    }

    fn setup(self: &Rc<Self>, actions: &[ActionRef]) {
        let ui = &self.ui;
        let (dom_node, actions_list) = (self.bar.dom_node(), self.bar.actions_list());
        {
            let mut tree = ui.tree_mut();
            // As caixas são posicionadas por `relayout`, não preenchem o pai.
            tree.set_fill(dom_node, false, false);
            tree.set_fill(actions_list, false, false);
            let scroll_dom = self.scrollable.get_dom_node();
            // Sem recorte: o `position: fixed` do submenu escapa do `overflow` no DOM. Liga só com rolagem.
            tree.style_mut(scroll_dom).clip = false;
            tree.style_mut(self.menu_element).clip = false;
        }
        self.style_scroll_element();

        // `focus-visible`: o teclado liga, o ponteiro desliga.
        let focus_visible = self.focus_visible.clone();
        self.base.register(ui.listen(self.menu_element, EventKind::KeyDown, true, move |_| focus_visible.set(true)));
        let motion = self.motion.clone();
        let focus_visible = self.focus_visible.clone();
        self.base.register(ui.listen(self.menu_element, EventKind::PointerMove, true, move |e| {
            let Some(pointer) = e.pointer else { return };
            let moved = motion.last.replace(Some((pointer.x, pointer.y))).is_some_and(|last| last != (pointer.x, pointer.y));
            motion.moved.set(moved);
            if moved {
                focus_visible.set(false);
            }
        }));
        let focus_visible = self.focus_visible.clone();
        self.base.register(ui.listen(self.menu_element, EventKind::PointerDown, true, move |_| focus_visible.set(false)));

        let weak = self.weak_self.clone();
        // Impede a navegação do Tab nos menus.
        self.base.register(ui.listen(self.menu_element, EventKind::KeyDown, false, |e| {
            if StandardKeyboardEvent::from_event(e).is_some_and(|event| event.equals(KeyCode::Tab.0 as u32)) {
                e.prevent_default();
            }
        }));

        if self.options.enable_mnemonics {
            let weak = weak.clone();
            self.base.register(ui.listen(self.menu_element, EventKind::KeyDown, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_mnemonic_key_down(e);
                }
            }));
        }

        if crate::action_view_items::host_platform().is_linux {
            let weak = weak.clone();
            self.base.register(ui.listen(self.menu_element, EventKind::KeyDown, false, move |e| {
                let (Some(this), Some(event)) = (weak.upgrade(), StandardKeyboardEvent::from_event(e)) else { return };
                let len = this.bar.length();
                if event.equals(KeyCode::Home.0 as u32) || event.equals(KeyCode::PageUp.0 as u32) {
                    this.bar.set_focused_item(len.checked_sub(1));
                    this.bar.focus_next(false, false);
                    e.stop(true);
                } else if event.equals(KeyCode::End.0 as u32) || event.equals(KeyCode::PageDown.0 as u32) {
                    this.bar.set_focused_item(Some(0));
                    this.bar.focus_previous(false);
                    e.stop(true);
                }
            }));
        }

        // `mouseout` que sai da barra: tira o foco do item. (`mouseleave` não dispara entre filhos.)
        self.base.register(ui.listen(dom_node, EventKind::PointerLeave, false, {
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.bar.set_focused_item(None);
                    this.update_focus(false);
                }
            }
        }));

        self.base.register(ui.listen(actions_list, EventKind::PointerMove, false, {
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else { return };
                if !this.motion.moved.get() {
                    return;
                }
                let Some(target) = ui_tree_item_of(&this.ui, this.bar.actions_list(), e.target) else { return };
                if !this.ui.tree().has_class(target, "action-item") {
                    return;
                }
                let last_focused_item = this.bar.focused_item();
                // Mover dentro do item em foco é o caso comum.
                let children = this.ui.tree().children(this.bar.actions_list()).to_vec();
                if last_focused_item.is_some_and(|i| children.get(i) == Some(&target)) {
                    return;
                }
                this.set_focused_item_from_node(target);
                if last_focused_item != this.bar.focused_item() {
                    this.update_focus(false);
                }
            }
        }));

        // Absorve cliques no espaço morto do menu (#63575), no elemento rolável para a barra não fechá-lo.
        self.base.register(ui.listen(self.scrollable.get_dom_node(), EventKind::PointerUp, false, |e| e.prevent_default()));

        self.base.register(self.scrollable.on_scroll().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.apply_scroll();
                }
            }
        }));

        let weak_for_focus = weak.clone();
        self.bar.set_update_focus_override(Some(Rc::new(move |from_right| {
            if let Some(this) = weak_for_focus.upgrade() {
                this.update_focus(from_right);
            }
        })));

        // Filtra separadores consecutivos ou inúteis e ciclos de submenu.
        let filtered: Vec<ActionRef> = actions
            .iter()
            .enumerate()
            .filter(|(idx, a)| {
                if self.options.submenu_ids.as_ref().is_some_and(|ids| ids.contains(&a.id())) {
                    eprintln!("Found submenu cycle: {}", a.id());
                    return false;
                }
                if a.id() == Separator::ID {
                    if *idx == actions.len() - 1 || *idx == 0 {
                        return false;
                    }
                    if actions[idx - 1].id() == Separator::ID {
                        return false;
                    }
                }
                true
            })
            .map(|(_, a)| a.clone())
            .collect();

        self.bar.push(&filtered, IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(true), is_menu: true, ..Default::default() }, index: None });

        ui.tree_mut().append_child(self.container, self.scrollable.get_dom_node());
        self.relayout();

        let items = self.items.borrow().clone();
        let actionable: Vec<&Rc<MenuActionViewItem>> = items.iter().filter_map(|i| if let MenuItem::Action(a) = i { Some(a) } else { None }).collect();
        for (index, item) in actionable.iter().enumerate() {
            item.update_position_in_set(index + 1, actionable.len());
        }

        let weak_for_modifiers = weak;
        self.base.register(ModifierKeyEmitter::get_instance(ui).event().listen(move |_| {
            let Some(this) = weak_for_modifiers.upgrade() else { return };
            this.mnemonics.borrow_mut().clear();
            let items = this.items.borrow().clone();
            for item in items {
                if let MenuItem::Action(item) = item {
                    item.update_alternative_action();
                    if this.options.enable_mnemonics {
                        this.register_mnemonic(&item);
                    }
                }
            }
            this.relayout();
        }));
    }

    /// `doGetActionViewItem(action, options, parentData)`.
    fn do_get_action_view_item(&self, action: &ActionRef, parent_data: &Rc<ParentData>) -> Rc<dyn IActionViewItem> {
        let options = &self.options;
        let ctx = || MenuItemContext { ui: self.ui.clone(), menu_style: self.menu_styles.clone(), focus_visible: self.focus_visible.clone(), motion: self.motion.clone() };
        if action.id() == Separator::ID {
            let separator_styles = ActionBarStyles { menu_separator_background: self.menu_styles.separator_color, disabled_foreground: self.menu_styles.disabled_foreground, ..Default::default() };
            let item = ActionViewItem::new(
                &self.ui,
                options.context.clone(),
                action.clone(),
                // O original passa `{ icon: true }`; `is_menu` dá ao separador a geometria do CSS do menu.
                IActionViewItemOptions { icon: Some(true), is_menu: true, styles: Some(Rc::new(separator_styles)), ..Default::default() },
            );
            self.items.borrow_mut().push(MenuItem::Separator(item.clone()));
            return item;
        }
        let item_options = IMenuItemOptions {
            base: IActionViewItemOptions { use_event_as_context: options.use_event_as_context, ..Default::default() },
            enable_mnemonics: options.enable_mnemonics,
            get_key_binding: options.get_key_binding.clone(),
        };
        let item = if let Some(submenu) = action.as_any().downcast_ref::<SubmenuAction>() {
            let mut submenu_ids = options.submenu_ids.clone().unwrap_or_default();
            submenu_ids.insert(action.id());
            MenuActionViewItem::new_submenu(
                ctx(),
                action.clone(),
                item_options,
                SubmenuInit { submenu_actions: submenu.actions().to_vec(), parent_data: parent_data.clone(), submenu_options: IMenuOptions { submenu_ids: Some(submenu_ids), ..options.clone() } },
            )
        } else {
            MenuActionViewItem::new_action(ctx(), options.context.clone(), action.clone(), item_options)
        };
        let weak = self.weak_self.clone();
        item.set_on_size_changed(Some(Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.relayout();
            }
        })));
        if options.enable_mnemonics {
            self.register_mnemonic(&item);
        }
        self.items.borrow_mut().push(MenuItem::Action(item.clone()));
        item
    }

    /// `_registerMnemonic(item)`.
    fn register_mnemonic(&self, item: &Rc<MenuActionViewItem>) {
        let Some(mnemonic) = item.get_mnemonic() else { return };
        if item.is_enabled() {
            self.mnemonics.borrow_mut().entry(mnemonic).or_default().push(item.clone());
        }
    }

    /// O `keydown` de mnemônicos.
    fn on_mnemonic_key_down(&self, e: &mut je_ui::Event) {
        let Some(key) = e.key.and_then(|k| KeyCodeUtils::to_string(k.key_code)).filter(|s| s.chars().count() == 1) else { return };
        let key = key.to_lowercase();
        let Some(mut actions) = self.mnemonics.borrow().get(&key).cloned() else { return };
        e.stop(true);
        if actions.len() == 1 {
            if actions[0].is_submenu()
                && let Some(container) = actions[0].container()
            {
                self.focus_item_by_element(container);
            }
            actions[0].on_click(e);
        }
        if actions.len() > 1 {
            let action = actions.remove(0);
            if let Some(container) = action.container() {
                self.focus_item_by_element(container);
                actions.push(action);
            }
            self.mnemonics.borrow_mut().insert(key, actions);
        }
    }

    fn focus_item_by_element(&self, element: NodeId) {
        let last_focused_item = self.bar.focused_item();
        self.set_focused_item_from_node(element);
        if last_focused_item != self.bar.focused_item() {
            self.update_focus(false);
        }
    }

    /// `setFocusedItem(element)`: `focusedItem` vira o índice do `li`.
    fn set_focused_item_from_node(&self, element: NodeId) {
        let index = self.ui.tree().children(self.bar.actions_list()).iter().position(|c| *c == element);
        if let Some(index) = index {
            self.bar.set_focused_item(Some(index));
        }
    }

    /// `updateFocus(fromRight)`: o da barra (sempre forçando o foco) e a rolagem do item para a vista.
    fn update_focus(&self, from_right: bool) {
        self.bar.update_focus_base(from_right, true);
        if let Some(index) = self.bar.focused_item() {
            self.reveal_focused_item(index);
        }
    }

    /// O que o `focus()` do navegador faz: rola o item para a vista (centrado, se não estava inteiro nela).
    pub fn reveal_focused_item(&self, index: usize) {
        let size = self.size.get();
        let Some(li) = self.ui.tree().children(self.bar.actions_list()).get(index).copied() else { return };
        let bounds = self.ui.tree().bounds(li);
        let (top, bottom) = (MENU_VERTICAL_PADDING + bounds.y0, MENU_VERTICAL_PADDING + bounds.y1);
        let scroll_top = self.scrollable.get_scroll_position().scroll_top;
        if top >= scroll_top && bottom <= scroll_top + size.inner_height {
            return;
        }
        let target = (top + bottom) / 2.0 - size.inner_height / 2.0;
        self.scrollable.set_scroll_position(&INewScrollPosition { scroll_top: Some(target.clamp(0.0, (size.content_height - size.inner_height).max(0.0))), scroll_left: None });
    }

    /// `styleScrollElement`: raio, cor e fundo do elemento rolável.
    fn style_scroll_element(&self) {
        let style = &self.menu_styles;
        let mut tree = self.ui.tree_mut();
        let scroll_dom = self.scrollable.get_dom_node();
        let s = tree.style_mut(scroll_dom);
        s.radius = MENU_CORNER_RADIUS_LARGE;
        s.background = style.background_color;
        let menu = tree.style_mut(self.menu_element);
        menu.radius = MENU_CORNER_RADIUS_LARGE;
        menu.border = style.border_color.map(|color| Border { width: MENU_BORDER_WIDTH, color });
        tree.style_mut(self.container).radius = MENU_CORNER_RADIUS_LARGE;
    }

    /// `scanDomNode()` mais o shrink-to-fit do CSS: mede os itens, dá o tamanho ao menu e aos nós.
    pub fn relayout(&self) {
        let items = self.bar.view_items();
        let sizes: Vec<(f64, f64)> = items.iter().map(|item| item.size(true)).collect();
        let items_width = sizes.iter().map(|s| s.0).fold(0.0, f64::max);
        let items_height: f64 = sizes.iter().map(|s| s.1).sum();
        let content_width = items_width.max(MENU_MIN_WIDTH);
        let content_height = items_height + 2.0 * MENU_VERTICAL_PADDING;
        let inner_height = content_height.min(self.max_height);
        let size = MenuSize { content_width, content_height, inner_height };
        self.size.set(size);
        let (outer_width, outer_height) = (content_width + 2.0 * MENU_BORDER_WIDTH, inner_height + 2.0 * MENU_BORDER_WIDTH);
        {
            let mut tree = self.ui.tree_mut();
            let scroll_dom = self.scrollable.get_dom_node();
            let bounds = je_render::kurbo::Rect::new(0.0, 0.0, outer_width, outer_height);
            tree.set_bounds(scroll_dom, bounds);
            tree.set_bounds(self.menu_element, bounds);
            tree.set_bounds(self.bar.actions_list(), je_render::kurbo::Rect::new(0.0, MENU_VERTICAL_PADDING, content_width, MENU_VERTICAL_PADDING + items_height));
            // O contêiner dá a largura e a altura do menu (shrink-to-fit), mantendo a posição.
            let container = tree.bounds(self.container);
            tree.set_bounds(self.container, je_render::kurbo::Rect::new(container.x0, container.y0, container.x0 + outer_width, container.y0 + outer_height));
            // O recorte só liga com rolagem (ver a documentação do módulo).
            let scrolls = content_height > inner_height;
            tree.style_mut(self.menu_element).clip = scrolls;
            tree.style_mut(scroll_dom).clip = scrolls;
        }
        self.apply_scroll();
        self.bar.layout();
        self.scrollable.set_scroll_dimensions(&INewScrollDimensions { width: Some(content_width), scroll_width: Some(content_width), height: Some(inner_height), scroll_height: Some(content_height) });
    }

    /// O `scrollTop` do `menuElement`: a barra de ações sobe `scrollTop`.
    fn apply_scroll(&self) {
        let scroll_top = self.scrollable.get_scroll_position().scroll_top;
        let size = self.size.get();
        self.ui.tree_mut().set_bounds(
            self.bar.dom_node(),
            je_render::kurbo::Rect::new(MENU_BORDER_WIDTH, MENU_BORDER_WIDTH - scroll_top, MENU_BORDER_WIDTH + size.content_width, MENU_BORDER_WIDTH - scroll_top + size.content_height),
        );
    }

    /// `getContainer()`: o elemento rolável.
    pub fn get_container(&self) -> NodeId {
        self.scrollable.get_dom_node()
    }

    /// O contêiner que o chamador deu.
    pub fn container(&self) -> NodeId {
        self.container
    }

    /// `get onScroll`.
    pub fn on_scroll(&self) -> BaseEvent<ScrollEvent> {
        self.scrollable.on_scroll()
    }

    /// `get scrollOffset`: o `scrollTop` do `menuElement`.
    pub fn scroll_offset(&self) -> f64 {
        self.scrollable.get_scroll_position().scroll_top
    }

    /// O tamanho do menu, com a borda: `(largura, altura)`.
    pub fn menu_size(&self) -> (f64, f64) {
        let size = self.size.get();
        (size.content_width + 2.0 * MENU_BORDER_WIDTH, size.inner_height + 2.0 * MENU_BORDER_WIDTH)
    }

    /// Os itens na ordem da barra, com o tipo que o `instanceof` do original distingue.
    pub fn menu_items(&self) -> Vec<MenuItem> {
        self.items.borrow().clone()
    }

    /// `trigger(index)`: abre o submenu ou roda a ação do item.
    pub fn trigger(&self, index: usize) {
        let item = self.items.borrow().get(index).cloned();
        match item {
            Some(MenuItem::Action(item)) if item.is_submenu() => {
                self.bar.focus_index(index);
                item.open(true);
            }
            Some(MenuItem::Action(item)) => {
                // `super.run(item._action, item._context)`
                let _ = self.bar.run(&item.base().action(), item.base().context());
            }
            _ => {}
        }
    }
}

/// O filho de `list` (o `li`) que contém `node`, se `node` é um descendente de `list` que não é a própria lista.
fn ui_tree_item_of(ui: &Ui, list: NodeId, node: NodeId) -> Option<NodeId> {
    let tree = ui.tree();
    if node == list || !tree.is_ancestor(list, node) {
        return None;
    }
    let mut current = node;
    while tree.parent(current)? != list {
        current = tree.parent(current)?;
    }
    Some(current)
}

impl IDisposable for Menu {
    fn dispose(&self) {
        self.base.dispose();
        self.bar.dispose();
        self.ui.remove(self.scrollable.get_dom_node());
    }
}
