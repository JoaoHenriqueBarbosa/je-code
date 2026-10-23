//! Porte de `src/vs/base/browser/ui/menu/menubar.ts` (e do `menubar.css`): a [`MenuBar`], a barra de menus
//! ("Arquivo", "Editar", ...) com mnemônicos (`Alt`+letra), foco entre os menus pelas setas, abertura ao
//! pairar com um menu aberto, o botão de estouro (`...`) quando os menus não cabem e o modo compacto (um
//! botão só, o menu do aplicativo).
//!
//! # API para quem consome (a barra de título, a barra de atividades)
//!
//! - [`MenuBar::new`]`(ui, container, options, menu_style)`; [`MenuBar::push`] acrescenta menus (`label` com
//!   `&` de mnemônico, `actions`), [`MenuBar::update_menu`], [`MenuBar::update`], [`MenuBar::toggle_focus`],
//!   [`MenuBar::blur`], [`MenuBar::get_width`]/[`MenuBar::get_height`], `on_visibility_change` e
//!   `on_focus_state_change`.
//! - Quem posiciona a barra dá o tamanho do `container` (o CSS é `height: 100%`) e chama [`MenuBar::update`]
//!   quando ele muda de largura (o estouro é calculado ali, no próximo quadro de animação).
//! - As cores que a folha do workbench (`menubarControl.css`) lê de variáveis ficam em [`MenuBarStyles`]
//!   (entram por [`IMenuBarOptions::styles`]; não estão no original).
//!
//! # Como o DOM e o CSS viram `je-ui`
//!
//! Cada menu é um `div.menubar-menu-button` (papel `menuitem`, `aria-label` é o rótulo do nó) filho do
//! `container`, com um `div.menubar-menu-title` e dentro dele os trechos de texto; o elemento `<mnemonic>` do
//! original é um nó com a classe `mnemonic` (o `je-ui` não tem tags). O botão de estouro vem por último.
//! Sem motor de layout, [`MenuBar::layout_buttons`] põe os botões em fila (`display: flex`, título de
//! `padding: 0 8px`, 22px de altura centrado) e o `offsetWidth` de um botão é a largura do nó. `visibility:
//! hidden` é o nó invisível (a posição fica reservada pelo cálculo, não pelo nó). O recorte (`overflow:
//! hidden`) não existe: o menu é filho do botão e o recorte cortaria o menu aberto (no DOM ele é `position:
//! fixed`); quem esconde o que não cabe é o próprio cálculo de estouro.
//!
//! `focusin`/`focusout` com `relatedTarget`: o `je-ui` não dá o alvo relacionado. O elemento que perdeu o
//! foco é lembrado pelo ouvinte de captura da raiz, e a perda de foco é conferida numa microtarefa (quando o
//! foco novo, se há, já chegou).
//!
//! Fica de fora: `Gesture`/toque (`Tap`); `browser.isFullscreen` (entra como [`IMenuBarOptions::is_fullscreen`]);
//! o `zoom` (`getDomNodeZoomLevel`); `ScanCode.AltLeft` (o evento só distingue `Alt`); e o contorno tracejado do
//! `:hover` (`outline: dashed`), que é sólido.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, Separator, SubmenuAction};
use je_base::async_::RunOnceScheduler;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::{KeyCode, KeyCodeUtils, KeyMod};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::runtime;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, Event, EventKind, NodeId, PointerButton, Role, Ui};

use crate::common::{registered_emitter, registered_store};
use crate::dom::{StandardKeyboardEvent, create_el, window_size};
use crate::menu::mnemonics::{LabelPieces, clean_mnemonic, escape_html, exec_menu_mnemonic, split_escaped_label, unescape_double_ampersands};
use crate::menu::{GetKeyBinding, HorizontalDirection, IMenuDirection, IMenuOptions, IMenuStyles, Menu, VerticalDirection};
use crate::modifier_key_emitter::{ModifierKey, ModifierKeyEmitter, ModifierKeyStatus};
use crate::sash::is_macintosh;
use crate::text::ui_font;

/// O `padding: 0px 8px` do `.menubar-menu-title`.
const TITLE_PADDING: f64 = 8.0;
/// O `line-height: 22px` da barra de título, que dá a altura do título.
const TITLE_HEIGHT: f64 = 22.0;
/// O `border-radius: 5px` do título.
const TITLE_RADIUS: f64 = 5.0;
/// O tamanho do botão de estouro (`width: 22px` mais o `padding: 0 8px`).
const TOGGLE_MORE_WIDTH: f64 = 22.0 + 2.0 * TITLE_PADDING;
/// A altura de uma linha de menu que o `compactMode` vertical `Above` assume (o `30` fixo do original).
const COMPACT_MENU_ROW_HEIGHT: f64 = 30.0;
/// O z-index do `.menubar-menu-items-holder`.
const HOLDER_Z_INDEX: i32 = 2000;

/// As cores do `menubarControl.css` e do `titlebarpart.css` que a barra de menus usa.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBarStyles {
    /// `--vscode-titleBar-activeForeground`: o texto de um menu.
    pub foreground: Option<Rgba>,
    /// `--vscode-menubar-selectionForeground`: o texto do menu aberto, com foco ou sob o ponteiro.
    pub selection_foreground: Option<Rgba>,
    /// `--vscode-menubar-selectionBackground`: o fundo do título nesses estados.
    pub selection_background: Option<Rgba>,
    /// `--vscode-menubar-selectionBorder`: o contorno do título.
    pub selection_border: Option<Rgba>,
}

/// As cores da barra de menus resolvidas no tema.
pub fn default_menu_bar_styles(theme: &ColorTheme) -> MenuBarStyles {
    MenuBarStyles {
        foreground: theme.get_color("titleBar.activeForeground"),
        selection_foreground: theme.get_color("menubar.selectionForeground"),
        selection_background: theme.get_color("menubar.selectionBackground"),
        selection_border: theme.get_color("menubar.selectionBorder"),
    }
}

/// `IMenuBarOptions`.
#[derive(Clone, Default)]
pub struct IMenuBarOptions {
    pub enable_mnemonics: bool,
    pub disable_alt_focus: bool,
    /// `visible`, `toggle`, `hidden`, `classic` ou `compact`.
    pub visibility: Option<String>,
    pub get_keybinding: Option<GetKeyBinding>,
    pub always_on_mnemonics: bool,
    pub compact_mode: Option<IMenuDirection>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub get_compact_menu_actions: Option<Rc<dyn Fn() -> Vec<ActionRef>>>,
    /// As cores (não está no original).
    pub styles: Option<Rc<MenuBarStyles>>,
    /// `browser.isFullscreen(mainWindow)` (não está no original; sem valor, falso).
    pub is_fullscreen: Option<Rc<dyn Fn() -> bool>>,
}

/// `MenuBarMenu`.
#[derive(Clone)]
pub struct MenuBarMenu {
    pub actions: Vec<ActionRef>,
    pub label: String,
}

struct MenuEntry {
    label: String,
    actions: Vec<ActionRef>,
    title_element: Option<NodeId>,
    button_element: Option<NodeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MenubarState {
    Hidden,
    Visible,
    Focused,
    Open,
}

/// O botão de estouro, que o original guarda como um `MenuBarMenuWithElements`.
struct OverflowMenu {
    title_element: NodeId,
    button_element: NodeId,
    actions: RefCell<Vec<ActionRef>>,
}

#[derive(Clone)]
struct FocusedMenu {
    index: i32,
    holder: Option<NodeId>,
    widget: Option<Rc<Menu>>,
}

pub struct MenuBar {
    base: Rc<Disposable>,
    weak_self: Weak<MenuBar>,
    ui: Ui,
    container: NodeId,
    options: RefCell<IMenuBarOptions>,
    menu_style: Rc<IMenuStyles>,
    menus: RefCell<Vec<MenuEntry>>,
    overflow_menu: OverflowMenu,
    focused_menu: RefCell<Option<FocusedMenu>>,
    focus_to_return: Cell<Option<NodeId>>,
    menu_updater: Rc<RunOnceScheduler>,
    mnemonics_in_use: Cell<bool>,
    opened_via_keyboard: Cell<bool>,
    awaiting_alt_release: Cell<bool>,
    ignore_next_mouse_up: Cell<bool>,
    mnemonics: RefCell<HashMap<String, usize>>,
    update_pending: Cell<bool>,
    focus_state: Cell<MenubarState>,
    action_runner: Rc<dyn IActionRunner>,
    on_visibility_change: Rc<Emitter<bool>>,
    on_focus_state_change: Rc<Emitter<bool>>,
    num_menus_shown: Cell<usize>,
    overflow_layout_scheduled: RefCell<Option<DisposableRef>>,
    menu_disposables: Rc<DisposableStore>,
    hovered_button: Cell<Option<NodeId>>,
    last_blurred: Cell<Option<(NodeId, u64)>>,
    related_target: Cell<Option<NodeId>>,
    blur_stamp: Cell<u64>,
    shown: Cell<bool>,
}

impl MenuBar {
    pub const OVERFLOW_INDEX: i32 = -1;

    /// `new MenuBar(container, options, menuStyle)`.
    pub fn new(ui: &Ui, container: NodeId, options: IMenuBarOptions, menu_style: IMenuStyles) -> Rc<Self> {
        let base = Disposable::new();
        let is_compact = options.compact_mode.is_some();
        {
            let mut tree = ui.tree_mut();
            tree.set_role(container, Role::MenuBar);
            if is_compact {
                tree.add_class(container, "compact");
            }
        }
        let on_visibility_change = registered_emitter::<bool>(&base);
        let on_focus_state_change = registered_emitter::<bool>(&base);
        let menu_disposables = registered_store(&base);
        let action_runner: Rc<dyn IActionRunner> = match options.action_runner.clone() {
            Some(runner) => runner,
            None => {
                let runner = ActionRunner::new();
                base.register(runner.clone());
                runner
            }
        };
        let overflow_menu = Self::create_overflow_button(ui, container, is_compact);

        let this = Rc::new_cyclic(|weak: &Weak<MenuBar>| {
            let menu_updater = RunOnceScheduler::new(
                {
                    let weak = weak.clone();
                    move || {
                        if let Some(this) = weak.upgrade() {
                            this.update(None);
                        }
                    }
                },
                200,
            );
            base.register(menu_updater.clone());
            MenuBar {
                base: base.clone(),
                weak_self: weak.clone(),
                ui: ui.clone(),
                container,
                options: RefCell::new(options),
                menu_style: Rc::new(menu_style),
                menus: RefCell::new(Vec::new()),
                overflow_menu,
                focused_menu: RefCell::new(None),
                focus_to_return: Cell::new(None),
                menu_updater,
                mnemonics_in_use: Cell::new(false),
                opened_via_keyboard: Cell::new(false),
                awaiting_alt_release: Cell::new(false),
                ignore_next_mouse_up: Cell::new(false),
                mnemonics: RefCell::new(HashMap::new()),
                update_pending: Cell::new(false),
                focus_state: Cell::new(MenubarState::Visible),
                action_runner,
                on_visibility_change,
                on_focus_state_change,
                num_menus_shown: Cell::new(0),
                overflow_layout_scheduled: RefCell::new(None),
                menu_disposables,
                hovered_button: Cell::new(None),
                last_blurred: Cell::new(None),
                related_target: Cell::new(None),
                blur_stamp: Cell::new(0),
                shown: Cell::new(true),
            }
        });
        this.install_overflow_listeners();
        this.install_listeners();
        this.set_unfocused_state();
        this
    }

    fn styles(&self) -> Rc<MenuBarStyles> {
        self.options.borrow().styles.clone().unwrap_or_default()
    }

    fn is_compact(&self) -> bool {
        self.options.borrow().compact_mode.is_some()
    }

    fn visibility(&self) -> Option<String> {
        self.options.borrow().visibility.clone()
    }

    /// Os ouvintes do `constructor`.
    fn install_listeners(self: &Rc<Self>) {
        let (ui, container) = (&self.ui, self.container);
        let root = ui.root();
        let weak = self.weak_self.clone();

        self.base.register(self.action_runner.on_will_run().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.set_unfocused_state();
                }
            }
        }));
        self.base.register(ModifierKeyEmitter::get_instance(ui).event().listen({
            let weak = weak.clone();
            move |status| {
                if let Some(this) = weak.upgrade() {
                    this.on_modifier_key_toggled(status);
                }
            }
        }));

        self.base.register(ui.listen(container, EventKind::KeyDown, false, {
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_container_key_down(e);
                }
            }
        }));

        // `mousedown` fora da barra conta como perda de foco (os botões param a propagação).
        self.base.register(ui.listen(root, EventKind::PointerDown, false, {
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade()
                    && this.is_focused()
                {
                    this.set_unfocused_state();
                }
            }
        }));

        // `focusin`/`focusout` e o `relatedTarget`: ver a documentação do módulo.
        self.base.register(ui.listen(root, EventKind::Blur, true, {
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else { return };
                let stamp = this.blur_stamp.get() + 1;
                this.blur_stamp.set(stamp);
                this.last_blurred.set(Some((e.target, stamp)));
                let weak = Rc::downgrade(&this);
                runtime::queue_microtask(move || {
                    if let Some(this) = weak.upgrade()
                        && this.last_blurred.get().is_some_and(|(_, s)| s == stamp)
                    {
                        this.last_blurred.set(None);
                    }
                });
            }
        }));
        self.base.register(ui.listen(root, EventKind::Focus, true, {
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.related_target.set(this.last_blurred.take().map(|(node, _)| node));
                }
            }
        }));
        self.base.register(ui.listen(container, EventKind::Focus, true, {
            let weak = weak.clone();
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                if let Some(related) = this.related_target.take()
                    && !this.ui.tree().is_ancestor(this.container, related)
                {
                    this.focus_to_return.set(Some(related));
                }
            }
        }));
        self.base.register(ui.listen(container, EventKind::Blur, true, {
            let weak = weak.clone();
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                let weak = Rc::downgrade(&this);
                // Quando o foco novo (se há) já chegou.
                runtime::queue_microtask(move || {
                    let Some(this) = weak.upgrade() else { return };
                    match this.ui.focused() {
                        // Perdeu o foco sem alvo (por exemplo, uma webview).
                        None => this.set_unfocused_state(),
                        Some(active) if !this.ui.tree().is_ancestor(this.container, active) => {
                            this.focus_to_return.set(None);
                            this.set_unfocused_state();
                        }
                        Some(_) => {}
                    }
                });
            }
        }));

        // `keydown` na janela: Alt mais a letra de um mnemônico.
        self.base.register(ui.listen(root, EventKind::KeyDown, false, {
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else { return };
                let Some(key) = e.key else { return };
                if !this.options.borrow().enable_mnemonics || !key.modifiers.alt || key.modifiers.ctrl || e.default_prevented() {
                    return;
                }
                let Some(letter) = key_letter(key.key_code) else { return };
                let Some(menu_index) = this.mnemonics.borrow().get(&letter).copied() else { return };
                this.mnemonics_in_use.set(true);
                this.update_mnemonic_visibility(true);
                this.on_menu_triggered(menu_index as i32, false);
            }
        }));
    }

    fn on_container_key_down(&self, e: &mut Event) {
        let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
        let mut event_handled = true;
        let letter = key_letter(event.key_code).unwrap_or_default();
        let tab_nav = is_macintosh() && !self.is_compact();
        let (left, right) = (KeyCode::LeftArrow.0 as u32, KeyCode::RightArrow.0 as u32);
        let (tab, shift_tab) = (KeyCode::Tab.0 as u32, KeyCode::Tab.0 as u32 | KeyMod::SHIFT);
        if event.equals(left) || (tab_nav && event.equals(shift_tab)) {
            self.focus_previous();
        } else if event.equals(right) || (tab_nav && event.equals(tab)) {
            self.focus_next();
        } else if event.equals(KeyCode::Escape.0 as u32) && self.is_focused() && !self.is_open() {
            self.set_unfocused_state();
        } else if !self.is_open() && !event.ctrl_key && self.options.borrow().enable_mnemonics && self.mnemonics_in_use.get() && self.mnemonics.borrow().contains_key(&letter) {
            let menu_index = self.mnemonics.borrow()[&letter];
            self.on_menu_triggered(menu_index as i32, false);
        } else {
            event_handled = false;
        }
        // Nunca deixa o Tab andar quando não é compacto.
        if !self.is_compact() && (event.equals(shift_tab) || event.equals(tab)) {
            e.prevent_default();
        }
        if event_handled {
            e.stop(true);
        }
    }

    /// Os ouvintes que cada botão de menu (e o de estouro) tem: teclado, ponteiro e `mouseenter`.
    fn install_button_listeners(self: &Rc<Self>, button: NodeId, menu_index: i32) {
        let ui = &self.ui;
        let weak = self.weak_self.clone();
        self.base.register(ui.listen(button, EventKind::KeyUp, false, {
            let weak = weak.clone();
            move |e| {
                let (Some(this), Some(event)) = (weak.upgrade(), StandardKeyboardEvent::from_event(e)) else { return };
                let is_overflow = menu_index == Self::OVERFLOW_INDEX;
                let compact = this.is_compact();
                // O botão de um menu abre com `Down` ou `Enter`; o de estouro, ver `createOverflowMenu`.
                let mut triggers = vec![KeyCode::Enter];
                if !is_overflow || !compact {
                    triggers.push(KeyCode::DownArrow);
                } else {
                    triggers.push(KeyCode::Space);
                    match this.options.borrow().compact_mode.map(|d| d.horizontal) {
                        Some(HorizontalDirection::Right) => triggers.push(KeyCode::RightArrow),
                        Some(HorizontalDirection::Left) => triggers.push(KeyCode::LeftArrow),
                        None => {}
                    }
                }
                if triggers.iter().any(|k| event.equals(k.0 as u32)) && !this.is_open() {
                    *this.focused_menu.borrow_mut() = Some(FocusedMenu { index: menu_index, holder: None, widget: None });
                    this.opened_via_keyboard.set(true);
                    this.set_focus_state(MenubarState::Open);
                    e.stop(true);
                }
            }
        }));
        self.base.register(ui.listen(button, EventKind::PointerDown, false, {
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else { return };
                // Ignora o clique que não é do botão esquerdo.
                if e.pointer.is_some_and(|p| p.button != PointerButton::Primary) {
                    e.prevent_default();
                    return;
                }
                if !this.is_open() {
                    // Abre o menu no `mousedown` e ignora o `mouseup` seguinte.
                    this.ignore_next_mouse_up.set(true);
                    this.on_menu_triggered(menu_index, true);
                } else {
                    this.ignore_next_mouse_up.set(false);
                }
                e.stop(true);
            }
        }));
        self.base.register(ui.listen(button, EventKind::PointerUp, false, {
            let weak = weak.clone();
            move |e| {
                let Some(this) = weak.upgrade() else { return };
                if e.default_prevented() {
                    return;
                }
                if !this.ignore_next_mouse_up.get() {
                    if this.is_focused() {
                        this.on_menu_triggered(menu_index, true);
                    }
                } else {
                    this.ignore_next_mouse_up.set(false);
                }
            }
        }));
        self.base.register(ui.listen(button, EventKind::PointerEnter, false, {
            let weak = weak.clone();
            move |_| {
                let Some(this) = weak.upgrade() else { return };
                this.hovered_button.set(Some(button));
                if this.is_open() && !this.is_current_menu(menu_index) {
                    this.ui.focus(button);
                    this.cleanup_custom_menu();
                    this.show_custom_menu(menu_index, false);
                } else if this.is_focused() && !this.is_open() {
                    *this.focused_menu.borrow_mut() = Some(FocusedMenu { index: menu_index, holder: None, widget: None });
                    this.ui.focus(button);
                }
                this.refresh_styles();
            }
        }));
        self.base.register(ui.listen(button, EventKind::PointerLeave, false, {
            move |_| {
                if let Some(this) = weak.upgrade() {
                    if this.hovered_button.get() == Some(button) {
                        this.hovered_button.set(None);
                    }
                    this.refresh_styles();
                }
            }
        }));
        for kind in [EventKind::Focus, EventKind::Blur] {
            let weak = self.weak_self.clone();
            self.base.register(ui.listen(button, kind, false, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.refresh_styles();
                }
            }));
        }
    }

    /// `push(arg)`.
    pub fn push(self: &Rc<Self>, menus: &[MenuBarMenu]) {
        for menu_bar_menu in menus {
            let menu_index = self.menus.borrow().len();
            let clean_menu_label = clean_mnemonic(&menu_bar_menu.label);
            // Registra os mnemônicos.
            if let Some(matches) = exec_menu_mnemonic(&menu_bar_menu.label) {
                self.register_mnemonic(menu_index, matches.letter());
            }
            if self.is_compact() {
                self.menus.borrow_mut().push(MenuEntry { label: menu_bar_menu.label.clone(), actions: menu_bar_menu.actions.clone(), title_element: None, button_element: None });
                continue;
            }
            let (button, title) = {
                let mut tree = self.ui.tree_mut();
                let button = tree.create_detached(Role::MenuItem);
                tree.set_class_name(button, "menubar-menu-button");
                tree.set_label(button, clean_menu_label);
                let title = create_el(&mut tree, button, Role::Presentation, "menubar-menu-title");
                tree.insert_before(self.container, button, Some(self.overflow_menu.button_element));
                (button, title)
            };
            self.update_labels(title, button, &menu_bar_menu.label);
            self.install_button_listeners(button, menu_index as i32);
            self.menus.borrow_mut().push(MenuEntry { label: menu_bar_menu.label.clone(), actions: menu_bar_menu.actions.clone(), title_element: Some(title), button_element: Some(button) });
        }
        self.layout_buttons();
    }

    fn create_overflow_button(ui: &Ui, container: NodeId, is_compact: bool) -> OverflowMenu {
        let mut tree = ui.tree_mut();
        let button = create_el(&mut tree, container, Role::MenuItem, "menubar-menu-button");
        let label = if is_compact { localize("mAppMenu", "Application Menu", &[]) } else { localize("mMore", "More", &[]) };
        tree.set_label(button, label);
        let title = create_el(&mut tree, button, Role::Presentation, "menubar-menu-title toolbar-toggle-more codicon codicon-menubar-more");
        tree.style_mut(title).content = Content::Codicon { id: "menubar-more".to_owned(), size: 16.0, color: Rgba::new(204, 204, 204, 255) };
        // `buttonElement.style.visibility = 'hidden'`
        tree.style_mut(button).visible = false;
        OverflowMenu { title_element: title, button_element: button, actions: RefCell::new(Vec::new()) }
    }

    fn install_overflow_listeners(self: &Rc<Self>) {
        self.install_button_listeners(self.overflow_menu.button_element, Self::OVERFLOW_INDEX);
    }

    /// `updateMenu(menu)`.
    pub fn update_menu(&self, menu: &MenuBarMenu) {
        if let Some(entry) = self.menus.borrow_mut().iter_mut().find(|m| m.label == menu.label) {
            entry.actions = menu.actions.clone();
        }
    }

    /// `blur()`.
    pub fn blur(&self) {
        self.set_unfocused_state();
    }

    /// `getWidth()`.
    pub fn get_width(&self) -> f64 {
        if self.is_compact() {
            return 0.0;
        }
        let menus = self.menus.borrow();
        let (Some(first), Some(last)) = (menus.first().and_then(|m| m.button_element), menus.last().and_then(|m| m.button_element)) else { return 0.0 };
        let tree = self.ui.tree();
        let left = tree.absolute_bounds(first).x0;
        let right_button = if self.has_overflow() { self.overflow_menu.button_element } else { last };
        tree.absolute_bounds(right_button).x1 - left
    }

    /// `getHeight()`: o `clientHeight` do contêiner.
    pub fn get_height(&self) -> f64 {
        self.ui.tree().bounds(self.container).height()
    }

    /// `toggleFocus()`.
    pub fn toggle_focus(&self) {
        if !self.is_focused() && self.visibility().as_deref() != Some("hidden") {
            self.mnemonics_in_use.set(true);
            let index = if self.num_menus_shown.get() > 0 { 0 } else { Self::OVERFLOW_INDEX };
            *self.focused_menu.borrow_mut() = Some(FocusedMenu { index, holder: None, widget: None });
            self.set_focus_state(MenubarState::Focused);
        } else if !self.is_open() {
            self.set_unfocused_state();
        }
    }

    fn button_of(&self, index: i32) -> Option<NodeId> {
        if index == Self::OVERFLOW_INDEX { Some(self.overflow_menu.button_element) } else { self.menus.borrow().get(usize::try_from(index).ok()?).and_then(|m| m.button_element) }
    }

    fn focus_button(&self, index: i32) {
        if let Some(button) = self.button_of(index) {
            self.ui.focus(button);
        }
    }

    fn blur_button(&self, index: i32) {
        if let Some(button) = self.button_of(index)
            && self.ui.focused() == Some(button)
        {
            self.ui.blur();
        }
    }

    /// A largura natural do botão (`offsetWidth`): o título com o `padding: 0 8px`.
    fn button_width(&self, button: NodeId) -> f64 {
        let tree = self.ui.tree();
        if button == self.overflow_menu.button_element {
            return if self.is_compact() { tree.bounds(self.container).width() } else { TOGGLE_MORE_WIDTH };
        }
        let title = tree.children(button).first().copied();
        title.map_or(0.0, |t| 2.0 * TITLE_PADDING + tree.children(t).iter().map(|c| tree.bounds(*c).width()).sum::<f64>())
    }

    /// Põe os botões em fila (`display: flex`), com o título centrado na altura do contêiner.
    pub fn layout_buttons(&self) {
        let height = self.ui.tree().bounds(self.container).height();
        let children = self.ui.tree().children(self.container).to_vec();
        let compact = self.is_compact();
        let mut x = 0.0;
        for button in children {
            let width = self.button_width(button);
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(button, Rect::new(x, 0.0, x + width, height));
            if let Some(title) = tree.children(button).first().copied() {
                let title_height = TITLE_HEIGHT;
                let y = ((height - title_height) / 2.0).max(0.0);
                if button == self.overflow_menu.button_element {
                    // `width: 22px; height: 22px; padding: 0 8px` (ou 100% no compacto).
                    let w = if compact { width } else { 22.0 + 2.0 * TITLE_PADDING };
                    tree.set_bounds(title, Rect::new(0.0, y, w, y + title_height));
                } else {
                    tree.set_bounds(title, Rect::new(0.0, y, width, y + title_height));
                    let mut piece_x = TITLE_PADDING;
                    for piece in tree.children(title).to_vec() {
                        let w = tree.bounds(piece).width();
                        tree.set_bounds(piece, Rect::new(piece_x, 0.0, piece_x + w, TITLE_HEIGHT));
                        piece_x += w;
                    }
                }
            }
            x += width;
        }
        self.refresh_styles();
    }

    fn visible_menus(&self) -> Vec<(NodeId, NodeId)> {
        self.menus.borrow().iter().filter_map(|m| Some((m.button_element?, m.title_element?))).collect()
    }

    /// `updateOverflowAction()`.
    fn update_overflow_action(&self) {
        if self.menus.borrow().is_empty() {
            return;
        }
        let overflow_menu_only_class = "overflow-menu-only";
        // Tira a restrição de só o estouro para dar o máximo de espaço.
        self.ui.tree_mut().toggle_class(self.container, overflow_menu_only_class, false);
        self.layout_buttons();

        let size_available = self.ui.tree().bounds(self.container).width();
        let mut current_size = 0.0;
        let compact = self.is_compact();
        let mut full = compact;
        let prev_num_menus_shown = self.num_menus_shown.get();
        let mut num_menus_shown = 0usize;
        let showable = self.visible_menus();
        let set_visible = |button: NodeId, visible: bool| self.ui.tree_mut().style_mut(button).visible = visible;
        for (button, _) in &showable {
            if !full {
                let size = self.button_width(*button);
                if current_size + size > size_available {
                    full = true;
                } else {
                    current_size += size;
                    num_menus_shown += 1;
                    if num_menus_shown > prev_num_menus_shown {
                        set_visible(*button, true);
                    }
                }
            }
            if full {
                set_visible(*button, false);
            }
        }

        // Abaixo do mínimo de menus, só o menu de hambúrguer.
        if (num_menus_shown as f64 - 1.0) <= showable.len() as f64 / 4.0 {
            for (button, _) in &showable {
                set_visible(*button, false);
            }
            full = true;
            num_menus_shown = 0;
            current_size = 0.0;
        }

        let overflow_button = self.overflow_menu.button_element;
        let menus_snapshot: Vec<(String, Vec<ActionRef>)> = self.menus.borrow().iter().map(|m| (m.label.clone(), m.actions.clone())).collect();
        if compact {
            let mut actions: Vec<ActionRef> = Vec::new();
            for (label, menu_actions) in menus_snapshot.iter().skip(num_menus_shown) {
                actions.push(SubmenuAction::new(&format!("menubar.submenu.{label}"), label, menu_actions.clone(), None));
            }
            let compact_actions = self.options.borrow().get_compact_menu_actions.as_ref().map(|f| f());
            if let Some(compact_actions) = compact_actions.filter(|a| !a.is_empty()) {
                actions.push(Separator::new());
                actions.extend(compact_actions);
            }
            *self.overflow_menu.actions.borrow_mut() = actions;
            set_visible(overflow_button, true);
        } else if full {
            // Não cabe nem o botão de estouro: tira mais menus.
            let overflow_width = self.button_width(overflow_button);
            while current_size + overflow_width > size_available && num_menus_shown > 0 {
                num_menus_shown -= 1;
                let size = self.button_width(showable[num_menus_shown].0);
                set_visible(showable[num_menus_shown].0, false);
                current_size -= size;
            }
            let mut actions: Vec<ActionRef> = Vec::new();
            for (button, _) in showable.iter().skip(num_menus_shown) {
                if let Some((label, menu_actions)) = self.menus.borrow().iter().find(|m| m.button_element == Some(*button)).map(|m| (m.label.clone(), m.actions.clone())) {
                    actions.push(SubmenuAction::new(&format!("menubar.submenu.{label}"), &label, menu_actions, None));
                }
            }
            *self.overflow_menu.actions.borrow_mut() = actions;
            let next_sibling = {
                let tree = self.ui.tree();
                let children = tree.children(self.container);
                children.iter().position(|c| *c == overflow_button).and_then(|i| children.get(i + 1)).copied()
            };
            if let Some((first_hidden, _)) = showable.get(num_menus_shown)
                && next_sibling != Some(*first_hidden)
            {
                self.ui.tree_mut().insert_before(self.container, overflow_button, Some(*first_hidden));
            }
            set_visible(overflow_button, true);
        } else {
            self.ui.tree_mut().append_child(self.container, overflow_button);
            set_visible(overflow_button, false);
        }
        self.num_menus_shown.set(num_menus_shown);
        // Só o estouro: a classe evita que ele ocupe espaço.
        self.ui.tree_mut().toggle_class(self.container, overflow_menu_only_class, num_menus_shown == 0);
        self.layout_buttons();
    }

    /// `updateLabels(titleElement, buttonElement, label)`.
    fn update_labels(&self, title: NodeId, button: NodeId, label: &str) {
        let clean_menu_label = clean_mnemonic(label);
        let enable_mnemonics = self.options.borrow().enable_mnemonics;
        let pieces = if enable_mnemonics { split_escaped_label(&escape_html(label)) } else { LabelPieces::Plain(unescape_double_ampersands(&clean_menu_label)) };
        self.rebuild_title(title, &pieces);
        // `aria-keyshortcuts` (`Alt+<letra>`) não existe no `je-ui`.
        let _ = button;
    }

    /// Refaz os nós de texto do título.
    fn rebuild_title(&self, title: NodeId, pieces: &LabelPieces) {
        let font: TextStyle = ui_font(13.0, 400);
        let foreground = self.styles().foreground.unwrap_or(Rgba::new(204, 204, 204, 255));
        let mut tree = self.ui.tree_mut();
        for child in tree.children(title).to_vec() {
            tree.remove(child);
        }
        let parts: Vec<(&str, bool)> = match pieces {
            LabelPieces::Plain(text) => vec![(text.as_str(), false)],
            LabelPieces::Mnemonic { before, letter, after } => vec![(before.as_str(), false), (letter.as_str(), true), (after.as_str(), false)],
        };
        for (text, is_mnemonic) in parts {
            if text.is_empty() {
                continue;
            }
            let node = create_el(&mut tree, title, Role::Generic, if is_mnemonic { "mnemonic" } else { "menubar-text" });
            let width = crate::text::set_node_text(&mut tree, node, text, &font, foreground, TITLE_HEIGHT);
            tree.set_bounds(node, Rect::new(0.0, 0.0, width, TITLE_HEIGHT));
        }
    }

    /// `update(options?)`.
    pub fn update(&self, options: Option<IMenuBarOptions>) {
        if let Some(options) = options {
            *self.options.borrow_mut() = options;
        }
        // Não atualiza enquanto o menu está em uso.
        if self.is_focused() {
            self.update_pending.set(true);
            return;
        }
        let labels: Vec<(NodeId, NodeId, String)> = self.menus.borrow().iter().filter_map(|m| Some((m.title_element?, m.button_element?, m.label.clone()))).collect();
        for (title, button, label) in labels {
            self.update_labels(title, button, &label);
        }
        self.layout_buttons();
        if self.overflow_layout_scheduled.borrow().is_none() {
            let weak = self.weak_self.clone();
            let scheduled = self.ui.schedule_at_next_animation_frame(0, move || {
                if let Some(this) = weak.upgrade() {
                    this.update_overflow_action();
                    *this.overflow_layout_scheduled.borrow_mut() = None;
                }
            });
            *self.overflow_layout_scheduled.borrow_mut() = Some(scheduled);
        }
        self.set_unfocused_state();
    }

    fn register_mnemonic(&self, menu_index: usize, mnemonic: char) {
        self.mnemonics.borrow_mut().insert(mnemonic.to_lowercase().collect(), menu_index);
    }

    fn hide_menubar(&self) {
        if self.shown.replace(false) {
            self.ui.tree_mut().style_mut(self.container).visible = false;
            self.on_visibility_change.fire(false);
        }
    }

    fn show_menubar(&self) {
        if !self.shown.replace(true) {
            self.ui.tree_mut().style_mut(self.container).visible = true;
            self.on_visibility_change.fire(true);
            self.update_overflow_action();
        }
    }

    fn focus_state(&self) -> MenubarState {
        self.focus_state.get()
    }

    /// `set focusState(value)`.
    fn set_focus_state(&self, value: MenubarState) {
        if self.focus_state() >= MenubarState::Focused && value < MenubarState::Focused {
            // Perdeu o foco: atualiza o menu se for preciso.
            if self.update_pending.get() {
                self.menu_updater.schedule(None);
                self.update_pending.set(false);
            }
        }
        if value == self.focus_state() {
            return;
        }
        let (is_visible, is_open, is_focused) = (self.is_visible(), self.is_open(), self.is_focused());
        self.focus_state.set(value);
        let focused_index = self.focused_menu.borrow().as_ref().map(|f| f.index);
        match value {
            MenubarState::Hidden => {
                if is_visible {
                    self.hide_menubar();
                }
                if is_open {
                    self.cleanup_custom_menu();
                }
                if is_focused {
                    *self.focused_menu.borrow_mut() = None;
                    self.return_focus();
                }
            }
            MenubarState::Visible => {
                if !is_visible {
                    self.show_menubar();
                }
                if is_open {
                    self.cleanup_custom_menu();
                }
                if is_focused {
                    if let Some(index) = focused_index {
                        self.blur_button(index);
                    }
                    *self.focused_menu.borrow_mut() = None;
                    self.return_focus();
                }
            }
            MenubarState::Focused => {
                if !is_visible {
                    self.show_menubar();
                }
                if is_open {
                    self.cleanup_custom_menu();
                }
                if let Some(index) = focused_index {
                    // Ligada, a barra pode estar compacta e tentando focar o primeiro menu: foca o estouro.
                    let index = if index == 0 && self.num_menus_shown.get() == 0 {
                        if let Some(focused) = self.focused_menu.borrow_mut().as_mut() {
                            focused.index = Self::OVERFLOW_INDEX;
                        }
                        Self::OVERFLOW_INDEX
                    } else {
                        index
                    };
                    self.focus_button(index);
                }
            }
            MenubarState::Open => {
                if !is_visible {
                    self.show_menubar();
                }
                if let Some(index) = focused_index {
                    self.cleanup_custom_menu();
                    self.show_custom_menu(index, self.opened_via_keyboard.get());
                }
            }
        }
        self.focus_state.set(value);
        self.on_focus_state_change.fire(self.focus_state() >= MenubarState::Focused);
        self.refresh_styles();
    }

    /// `focusToReturn.focus()`.
    fn return_focus(&self) {
        if let Some(node) = self.focus_to_return.take()
            && self.ui.tree().is_alive(node)
        {
            self.ui.focus(node);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.focus_state() >= MenubarState::Visible
    }

    fn is_focused(&self) -> bool {
        self.focus_state() >= MenubarState::Focused
    }

    fn is_open(&self) -> bool {
        self.focus_state() >= MenubarState::Open
    }

    fn has_overflow(&self) -> bool {
        self.is_compact() || self.num_menus_shown.get() < self.menus.borrow().len()
    }

    fn set_unfocused_state(&self) {
        let visibility = self.visibility();
        let fullscreen = self.options.borrow().is_fullscreen.as_ref().is_some_and(|f| f());
        if matches!(visibility.as_deref(), Some("toggle" | "hidden")) || (visibility.as_deref() == Some("classic") && fullscreen) {
            self.set_focus_state(MenubarState::Hidden);
        } else {
            self.set_focus_state(MenubarState::Visible);
        }
        self.ignore_next_mouse_up.set(false);
        self.mnemonics_in_use.set(false);
        self.update_mnemonic_visibility(false);
    }

    /// `focusPrevious()` e `focusNext()`: o índice vizinho entre os menus mostrados (e o estouro).
    fn move_focus(&self, forward: bool) {
        let shown = self.num_menus_shown.get() as i32;
        let current = self.focused_menu.borrow().as_ref().map(|f| f.index);
        let Some(current) = current else { return };
        if shown == 0 {
            return;
        }
        let new_index = if forward {
            if current == Self::OVERFLOW_INDEX {
                0
            } else if current == shown - 1 {
                Self::OVERFLOW_INDEX
            } else {
                (current + 1) % shown
            }
        } else if current == Self::OVERFLOW_INDEX {
            shown - 1
        } else if current == 0 && self.has_overflow() {
            Self::OVERFLOW_INDEX
        } else {
            (current - 1 + shown) % shown
        };
        if new_index == current {
            return;
        }
        if self.is_open() {
            self.cleanup_custom_menu();
            self.show_custom_menu(new_index, true);
        } else if self.is_focused() {
            if let Some(focused) = self.focused_menu.borrow_mut().as_mut() {
                focused.index = new_index;
            }
            self.focus_button(new_index);
        }
    }

    fn focus_previous(&self) {
        self.move_focus(false);
    }

    fn focus_next(&self) {
        self.move_focus(true);
    }

    /// `updateMnemonicVisibility(visible)`: sublinha a letra do mnemônico de cada título.
    fn update_mnemonic_visibility(&self, visible: bool) {
        let underline = self.options.borrow().always_on_mnemonics || visible;
        let color = self.styles().foreground.unwrap_or(Rgba::new(204, 204, 204, 255));
        let titles: Vec<NodeId> = self.menus.borrow().iter().filter_map(|m| m.title_element).collect();
        let mut tree = self.ui.tree_mut();
        for title in titles {
            // `titleElement.children.item(0)`: o primeiro elemento filho, a letra.
            let Some(letter) = tree.children(title).iter().copied().find(|c| tree.has_class(*c, "mnemonic")) else { continue };
            let existing = tree.children_with_class(letter, "underline").first().copied();
            match (underline, existing) {
                (true, None) => {
                    let width = tree.bounds(letter).width();
                    let node = create_el(&mut tree, letter, Role::Generic, "underline");
                    tree.set_bounds(node, Rect::new(0.0, TITLE_HEIGHT / 2.0 + 7.0, width, TITLE_HEIGHT / 2.0 + 8.0));
                    tree.style_mut(node).background = Some(color);
                }
                (false, Some(node)) => tree.remove(node),
                _ => {}
            }
        }
    }

    fn should_alt_key_focus(&self) -> bool {
        if is_macintosh() {
            return false;
        }
        let options = self.options.borrow();
        !options.disable_alt_focus || options.visibility.as_deref() == Some("toggle")
    }

    pub fn on_visibility_change(&self) -> BaseEvent<bool> {
        self.on_visibility_change.event()
    }

    pub fn on_focus_state_change(&self) -> BaseEvent<bool> {
        self.on_focus_state_change.event()
    }

    fn on_menu_triggered(&self, menu_index: i32, clicked: bool) {
        if self.is_open() {
            if self.is_current_menu(menu_index) {
                self.set_unfocused_state();
            } else {
                self.cleanup_custom_menu();
                self.show_custom_menu(menu_index, self.opened_via_keyboard.get());
            }
        } else {
            *self.focused_menu.borrow_mut() = Some(FocusedMenu { index: menu_index, holder: None, widget: None });
            self.opened_via_keyboard.set(!clicked);
            self.set_focus_state(MenubarState::Open);
        }
    }

    fn on_modifier_key_toggled(&self, status: &ModifierKeyStatus) {
        let all_modifiers_released = !status.alt_key && !status.ctrl_key && !status.shift_key && !status.meta_key;
        if self.visibility().as_deref() == Some("hidden") {
            return;
        }
        // Impede o padrão do Alt se a barra não está escondida e o Alt foca.
        if let Some(event) = &status.event
            && self.should_alt_key_focus()
            && event.key_code == KeyCode::Alt
        {
            event.prevent_default();
        }
        // Alt apertado com a barra focada devolve o foco.
        if self.is_focused() && status.last_key_pressed == Some(ModifierKey::Alt) && status.alt_key {
            self.set_unfocused_state();
            self.mnemonics_in_use.set(false);
            self.awaiting_alt_release.set(true);
        }
        // Alt apertado e solto sozinho.
        if all_modifiers_released && status.last_key_pressed == Some(ModifierKey::Alt) && status.last_key_released == Some(ModifierKey::Alt) && !self.awaiting_alt_release.get() {
            if !self.is_focused() && self.should_alt_key_focus() {
                self.mnemonics_in_use.set(true);
                let index = if self.num_menus_shown.get() > 0 { 0 } else { Self::OVERFLOW_INDEX };
                *self.focused_menu.borrow_mut() = Some(FocusedMenu { index, holder: None, widget: None });
                self.set_focus_state(MenubarState::Focused);
            } else if !self.is_open() {
                self.set_unfocused_state();
            }
        }
        // Alt solto.
        if !status.alt_key && status.last_key_released == Some(ModifierKey::Alt) {
            self.awaiting_alt_release.set(false);
        }
        if self.options.borrow().enable_mnemonics && !self.menus.borrow().is_empty() && !self.is_open() {
            self.update_mnemonic_visibility((!self.awaiting_alt_release.get() && status.alt_key) || self.mnemonics_in_use.get());
        }
    }

    fn is_current_menu(&self, menu_index: i32) -> bool {
        self.focused_menu.borrow().as_ref().is_some_and(|f| f.index == menu_index)
    }

    fn cleanup_custom_menu(&self) {
        let focused = self.focused_menu.borrow().clone();
        if let Some(focused) = focused {
            // Tira o foco dos menus primeiro.
            self.focus_button(focused.index);
            if let Some(holder) = focused.holder {
                let parent = self.ui.tree().parent(holder);
                if let Some(parent) = parent {
                    self.ui.tree_mut().remove_class(parent, "open");
                }
                self.ui.remove(holder);
            }
            if let Some(widget) = &focused.widget {
                widget.dispose();
            }
            *self.focused_menu.borrow_mut() = Some(FocusedMenu { index: focused.index, holder: None, widget: None });
        }
        self.menu_disposables.clear();
        self.refresh_styles();
    }

    /// `showCustomMenu(menuIndex, selectFirst)`.
    fn show_custom_menu(&self, menu_index: i32, select_first: bool) {
        let num_shown = self.num_menus_shown.get() as i32;
        let actual_menu_index = if menu_index >= num_shown { Self::OVERFLOW_INDEX } else { menu_index };
        let (actions, button, title) = if actual_menu_index == Self::OVERFLOW_INDEX {
            (self.overflow_menu.actions.borrow().clone(), self.overflow_menu.button_element, self.overflow_menu.title_element)
        } else {
            let menus = self.menus.borrow();
            let Some(menu) = usize::try_from(actual_menu_index).ok().and_then(|i| menus.get(i)) else { return };
            let (Some(button), Some(title)) = (menu.button_element, menu.title_element) else { return };
            (menu.actions.clone(), button, title)
        };
        let options = self.options.borrow().clone();
        let compact_mode = options.compact_mode;
        let holder = {
            let mut tree = self.ui.tree_mut();
            let holder = create_el(&mut tree, button, Role::Generic, "menubar-menu-items-holder");
            tree.style_mut(holder).z_index = HOLDER_Z_INDEX;
            tree.add_class(button, "open");
            holder
        };
        // `position: fixed`: a posição é da janela; o nó é filho do botão, então vira relativa a ele.
        let (title_rect, button_origin) = {
            let tree = self.ui.tree();
            (tree.absolute_bounds(title), tree.absolute_bounds(button))
        };
        let container_size = {
            let b = self.ui.tree().bounds(self.container);
            (b.width(), b.height())
        };
        let window_width = window_size(&self.ui).0;
        let mut right_anchor: Option<f64> = None;
        let left = match compact_mode.map(|d| d.horizontal) {
            Some(HorizontalDirection::Right) => title_rect.x0 + container_size.0,
            Some(HorizontalDirection::Left) => {
                right_anchor = Some(window_width - title_rect.x0);
                0.0
            }
            None => title_rect.x0,
        };
        let top = match compact_mode.map(|d| d.vertical) {
            Some(VerticalDirection::Above) => title_rect.y0 - self.menus.borrow().len() as f64 * COMPACT_MENU_ROW_HEIGHT + container_size.1,
            Some(VerticalDirection::Below) => title_rect.y0,
            None => title_rect.y1,
        };
        self.ui.tree_mut().set_bounds(holder, Rect::new(left - button_origin.x0, top - button_origin.y0, left - button_origin.x0, top - button_origin.y0));

        let menu_options = IMenuOptions {
            get_key_binding: options.get_keybinding.clone(),
            action_runner: Some(self.action_runner.clone()),
            enable_mnemonics: options.always_on_mnemonics || (self.mnemonics_in_use.get() && options.enable_mnemonics),
            aria_label: self.ui.tree().label(button).map(str::to_owned),
            expand_direction: Some(if self.is_compact() { compact_mode.expect("compacto tem direção") } else { IMenuDirection { horizontal: HorizontalDirection::Right, vertical: VerticalDirection::Below } }),
            use_event_as_context: true,
            ..Default::default()
        };
        let menu_widget = self.menu_disposables.add_rc(Menu::new(&self.ui, holder, &actions, menu_options, self.menu_style.clone()));
        if let Some(right) = right_anchor {
            // `right: windowWidth - titleLeft` com `left: auto`: ancora a borda direita.
            let width = menu_widget.menu_size().0;
            let mut tree = self.ui.tree_mut();
            let b = tree.bounds(holder);
            let x = window_width - right - width - button_origin.x0;
            tree.set_bounds(holder, Rect::new(x, b.y0, x + width, b.y1));
        }
        let weak = self.weak_self.clone();
        self.menu_disposables.add(menu_widget.on_did_cancel().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.set_focus_state(MenubarState::Focused);
            }
        }));
        if actual_menu_index != menu_index {
            menu_widget.trigger(usize::try_from(menu_index - num_shown).unwrap_or(0));
        } else {
            menu_widget.focus(select_first);
        }
        *self.focused_menu.borrow_mut() = Some(FocusedMenu { index: actual_menu_index, holder: Some(holder), widget: Some(menu_widget) });
        self.refresh_styles();
    }

    /// As regras de `menubarControl.css` e `menubar.css` para `:hover`, `:focus`, `.open` e `:focus-within`.
    pub fn refresh_styles(&self) {
        let styles = self.styles();
        let compact = self.is_compact();
        let focused = self.ui.focused();
        let focus_within = focused.is_some_and(|f| self.ui.tree().is_ancestor(self.container, f));
        let buttons: Vec<NodeId> = self.ui.tree().children(self.container).to_vec();
        let mut tree = self.ui.tree_mut();
        for button in buttons {
            let Some(title) = tree.children(button).first().copied() else { continue };
            let open = tree.has_class(button, "open");
            let focus = focused == Some(button);
            let hover = self.hovered_button.get() == Some(button);
            let selected = !compact && (open || focus || (!focus_within && hover));
            let color = if selected { styles.selection_foreground.or(styles.foreground) } else { styles.foreground };
            let border = if !compact && (open || focus || hover) { styles.selection_border.map(|color| Border { width: 1.0, color }) } else { None };
            let style = tree.style_mut(title);
            style.background = if selected { styles.selection_background } else { None };
            style.radius = TITLE_RADIUS;
            style.border = border;
            style.cursor = Some(Cursor::Default);
            if let (Some(color), Content::Codicon { color: icon_color, .. }) = (color, &mut style.content) {
                *icon_color = color;
            }
            if let Some(color) = color {
                for piece in tree.children(title).to_vec() {
                    if let Content::Text { color: text_color, .. } = &mut tree.style_mut(piece).content {
                        *text_color = color;
                    }
                    for underline in tree.children_with_class(piece, "underline") {
                        tree.style_mut(underline).background = Some(color);
                    }
                }
            }
        }
    }
}

/// A tecla como o `e.key.toLocaleLowerCase()` do original a daria, para as teclas de um caractere.
fn key_letter(key_code: KeyCode) -> Option<String> {
    KeyCodeUtils::to_string(key_code).filter(|s| s.chars().count() == 1).map(str::to_lowercase)
}

impl IDisposable for MenuBar {
    fn dispose(&self) {
        self.base.dispose();
        let menus = std::mem::take(&mut *self.menus.borrow_mut());
        for menu in menus {
            if let Some(title) = menu.title_element {
                self.ui.remove(title);
            }
            if let Some(button) = menu.button_element {
                self.ui.remove(button);
            }
        }
        self.ui.remove(self.overflow_menu.title_element);
        self.ui.remove(self.overflow_menu.button_element);
        if let Some(scheduled) = self.overflow_layout_scheduled.borrow_mut().take() {
            scheduled.dispose();
        }
    }
}
