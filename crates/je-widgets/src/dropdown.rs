//! Porte de `src/vs/base/browser/ui/dropdown/dropdown.ts` (e do `dropdown.css`): o [`BaseDropdown`], um rótulo
//! que abre e fecha ao clicar (ou com Enter e Space), e o [`DropdownMenu`], que o liga a um menu de contexto.
//!
//! # API para quem consome (toolbar, dropdownActionViewItem, workbench)
//!
//! - [`BaseDropdown::new`]`(ui, container, options)` cria `div.monaco-dropdown > div.dropdown-label` dentro do
//!   contêiner. O rótulo é o texto de `options.label` ou o que o [`ILabelRenderer`] montar; o `show`/`hide`
//!   avisam por [`BaseDropdown::on_did_change_visibility`].
//! - Herança vira composição, como nos itens de ação: uma subclasse guarda um [`BaseDropdown`], o constrói
//!   com [`BaseDropdown::new_derived`] (passando o `Weak` de si mesma) e implementa [`IDropdown`]. O `show()`
//!   virtual do original é [`IDropdown::show`]; o clique no rótulo e as teclas chamam por ele. O `hide()` que o
//!   [`DropdownMenu`] sobrescreve só chama o `super.hide()`, então não é virtual aqui.
//! - [`DropdownMenu`] guarda as ações (ou um [`IActionProvider`]) e, em `show`, entrega ao
//!   [`IContextMenuProvider`] um [`IContextMenuDelegate`] com o que o original monta (âncora, ações, contexto,
//!   fábrica de itens, atalhos, classe do menu, animação de fechamento, `onHide`, runner, alinhamento).
//!
//! Como o DOM e o CSS viram [`je_ui`]: `.monaco-dropdown` e `.dropdown-label` têm `height: 100%`
//! ([`je_ui::Tree::set_fill`]); `.dropdown-label` é um `display: flex` com `align-items: center` e
//! `justify-content: center`, que o [`BaseDropdown::layout`] refaz (o `je-ui` não tem motor de layout): os
//! filhos do rótulo formam uma fileira centrada. O `.monaco-dropdown.active` (menu aberto) é uma classe do nó.
//!
//! Fica de fora: o toque (`Gesture.addTarget`, `EventType.Tap`; o `je-ui` não tem toque), o `box-shadow` e o
//! `border-radius` de `.dropdown-menu` (o menu é de outro módulo) e `isActionProvider` (a checagem de tipo em
//! tempo de execução do original é o próprio tipo [`IActionProvider`] aqui). O `BaseDropdown` do original
//! herda de `ActionRunner`; aqui ele guarda um e o oferece em [`BaseDropdown::action_runner`] (não é um
//! `IActionRunner` porque repassar o trait inteiro seria função de repasse). `Dropdown`, que a árvore do
//! VS Code 1.143.1 já não tem em `dropdown.ts`, não existe. O `IMenuOptions` é do módulo `menu` (outro
//! agente): [`DropdownMenuOptions`] guarda só os campos que o dropdown lê.
//! TODO(integração): trocar [`DropdownMenuOptions`] por `menu::IMenuOptions` quando o `Menu` existir.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::keybindings::ResolvedKeybinding;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_ui::{EventKind, NodeId, PointerButton, Role, Ui};

use crate::action_bar::IActionViewItemProvider;
use crate::action_view_items::{IActionViewItem, IActionViewItemOptions};
use crate::common::registered_emitter;
use crate::context_view_api::{Anchor, CONTEXT_VIEW_MENU_MOTION_CLASS, IContextMenuDelegate, IContextMenuProvider, IContextViewCloseAnimation, context_view_menu_close_animation};
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::dropdown_action_view_item::IKeybindingProvider;
use crate::hover_api::{HoverPlacement, IManagedHover, ManagedHoverContent, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::text::{measure_text, natural_line_height, set_node_text, ui_font};

/// `ILabelRenderer`: monta o rótulo no contêiner e devolve o que o desmonta.
pub type ILabelRenderer = Rc<dyn Fn(NodeId) -> Option<DisposableRef>>;

/// O tamanho e o peso da fonte do texto padrão do rótulo (`--vscode-font-size`).
const LABEL_FONT_SIZE: f32 = 13.0;

/// `IBaseDropdownOptions`.
#[derive(Clone, Default)]
pub struct IBaseDropdownOptions {
    pub label: Option<String>,
    pub label_renderer: Option<ILabelRenderer>,
    pub is_enabled: Option<Rc<dyn Fn() -> bool>>,
    /// A cor do texto herdada do contêiner (não está no original). Sem valor, o texto padrão do rótulo usa um
    /// cinza claro.
    pub foreground: Option<Rgba>,
}

impl IBaseDropdownOptions {
    /// `baseOptions.isEnabled?.() === false`.
    fn is_disabled(&self) -> bool {
        self.is_enabled.as_ref().is_some_and(|is_enabled| !is_enabled())
    }
}

/// A parte virtual do [`BaseDropdown`]: o que uma subclasse sobrescreve.
pub trait IDropdown: IDisposable {
    /// O [`BaseDropdown`] embutido.
    fn base(&self) -> &BaseDropdown;

    /// `show()`: o clique e as teclas do rótulo chamam por aqui.
    fn show(&self) {
        self.base().base_show();
    }
}

/// `BaseDropdown`.
pub struct BaseDropdown {
    base: Rc<Disposable>,
    ui: Ui,
    options: IBaseDropdownOptions,
    element: NodeId,
    label: Cell<Option<NodeId>>,
    visible: Cell<bool>,
    on_did_change_visibility: Rc<Emitter<bool>>,
    hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    action_runner: Rc<ActionRunner>,
}

impl BaseDropdown {
    /// `new BaseDropdown(container, options)`.
    pub fn new(ui: &Ui, container: NodeId, options: IBaseDropdownOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self::new_derived(ui, container, options, weak.clone() as Weak<dyn IDropdown>))
    }

    /// O construtor para subclasses: `owner` é o `Weak` do dropdown completo (`Rc::new_cyclic`), a quem o
    /// clique e as teclas pedem o `show()` virtual.
    pub fn new_derived(ui: &Ui, container: NodeId, options: IBaseDropdownOptions, owner: Weak<dyn IDropdown>) -> Self {
        let base = Disposable::new();
        let action_runner = ActionRunner::new();
        base.register(action_runner.clone());
        let on_did_change_visibility = registered_emitter::<bool>(&base);
        let (element, label) = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Generic, "monaco-dropdown");
            tree.set_fill(element, false, true);
            let label = create_el(&mut tree, element, Role::Generic, "dropdown-label");
            tree.set_fill(label, true, true);
            tree.style_mut(label).cursor = Some(je_ui::Cursor::Pointer);
            (element, label)
        };
        let this = Self { base, ui: ui.clone(), options, element, label: Cell::new(Some(label)), visible: Cell::new(false), on_did_change_visibility, hover: RefCell::new(None), action_runner };

        // Impede o comportamento padrão do clique de disparar
        for kind in [EventKind::Click, EventKind::PointerDown] {
            this.base.register(ui.listen(element, kind, false, |e| e.stop(true)));
        }

        let toggle = {
            let (owner, options) = (owner, this.options.clone());
            move || {
                if options.is_disabled() {
                    return;
                }
                if let Some(owner) = owner.upgrade() {
                    if owner.base().is_visible() { owner.base().hide() } else { owner.show() }
                }
            }
        };
        this.base.register(ui.listen(label, EventKind::PointerDown, false, {
            let toggle = toggle.clone();
            move |e| {
                // O clique direito não abre, para deixar o menu de contexto separado (#151064)
                if e.pointer.is_some_and(|p| p.button != PointerButton::Primary) {
                    return;
                }
                toggle();
            }
        }));
        this.base.register(ui.listen(label, EventKind::KeyDown, false, move |e| {
            let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
            if event.equals(KeyCode::Enter.0 as u32) || event.equals(KeyCode::Space.0 as u32) {
                e.stop(true); // #57997
                toggle();
            }
        }));

        let label_renderer = this.options.label_renderer.clone().unwrap_or_else(|| this.default_label_renderer());
        if let Some(cleanup) = label_renderer(label) {
            this.base.register(cleanup);
        }
        this
    }

    /// O `labelRenderer` padrão: `container.textContent = baseOptions.label || ''`.
    fn default_label_renderer(&self) -> ILabelRenderer {
        let (ui, text, color) = (self.ui.clone(), self.options.label.clone().unwrap_or_default(), self.options.foreground.unwrap_or(Rgba::new(204, 204, 204, 255)));
        Rc::new(move |container: NodeId| {
            let mut tree = ui.tree_mut();
            crate::dom::clear_node(&mut tree, container);
            if !text.is_empty() {
                let font = ui_font(LABEL_FONT_SIZE, 400);
                let line_height = natural_line_height(&font);
                let node = create_el(&mut tree, container, Role::Generic, "dropdown-label-text");
                set_node_text(&mut tree, node, &text, &font, color, line_height);
                tree.set_bounds(node, Rect::new(0.0, 0.0, measure_text(&text, &font), line_height));
            }
            None
        })
    }

    /// `element`: o `div.monaco-dropdown`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// `label`: o `div.dropdown-label`, ou nada depois do `dispose`.
    pub fn label(&self) -> Option<NodeId> {
        self.label.get()
    }

    /// O `ActionRunner` de que o `BaseDropdown` herda no original.
    pub fn action_runner(&self) -> &Rc<ActionRunner> {
        &self.action_runner
    }

    pub fn on_did_change_visibility(&self) -> BaseEvent<bool> {
        self.on_did_change_visibility.event()
    }

    /// `set tooltip(tooltip)`: a dica gerenciada nasce no primeiro valor não vazio.
    pub fn set_tooltip(&self, tooltip: &str) {
        let Some(label) = self.label.get() else { return };
        let existing = self.hover.borrow().clone();
        match existing {
            None if !tooltip.is_empty() => {
                let hover = get_base_layer_hover_delegate().setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), label, tooltip.into(), None);
                let disposable: DisposableRef = hover.clone();
                self.base.register(disposable);
                *self.hover.borrow_mut() = Some(hover);
            }
            Some(hover) => hover.update(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(tooltip.to_owned())), None),
            None => {}
        }
    }

    /// `show()` do base.
    pub fn base_show(&self) {
        if !self.visible.get() && !self.options.is_disabled() {
            self.visible.set(true);
            self.on_did_change_visibility.fire(true);
        }
    }

    /// `hide()`.
    pub fn hide(&self) {
        if self.visible.get() {
            self.visible.set(false);
            self.on_did_change_visibility.fire(false);
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible.get()
    }

    /// O `justify-content: center` e o `align-items: center` do `.dropdown-label`: os filhos do rótulo, nos
    /// tamanhos que têm, formam uma fileira centrada no retângulo dele.
    pub fn layout(&self) {
        let Some(label) = self.label.get() else { return };
        let mut tree = self.ui.tree_mut();
        let size = tree.bounds(label);
        let children = tree.children(label).to_vec();
        let total: f64 = children.iter().map(|c| tree.bounds(*c).width()).sum();
        let mut x = (size.width() - total) / 2.0;
        for child in children {
            let bounds = tree.bounds(child);
            let y = (size.height() - bounds.height()) / 2.0;
            tree.set_bounds(child, Rect::new(x, y, x + bounds.width(), y + bounds.height()));
            x += bounds.width();
        }
    }

    /// `dispose()` do base: descarta o que registrou, esconde e tira o rótulo da árvore.
    pub fn base_dispose(&self) {
        self.base.dispose();
        self.hide();
        if let Some(label) = self.label.take() {
            self.ui.remove(label);
        }
    }
}

impl IDropdown for BaseDropdown {
    fn base(&self) -> &BaseDropdown {
        self
    }
}

impl IDisposable for BaseDropdown {
    fn dispose(&self) {
        self.base_dispose();
    }
}

/// `IActionProvider`: as ações do menu, pedidas a cada abertura.
pub trait IActionProvider {
    fn get_actions(&self) -> Vec<ActionRef>;
}

impl<F: Fn() -> Vec<ActionRef>> IActionProvider for F {
    fn get_actions(&self) -> Vec<ActionRef> {
        self()
    }
}

/// O que o dropdown lê de `IMenuOptions` (`menu.ts`): contexto, fábrica de itens, runner, atalhos e o
/// alinhamento (um `getter` no original, por isso uma função).
#[derive(Clone, Default)]
pub struct DropdownMenuOptions {
    pub context: Option<Dynamic>,
    pub action_view_item_provider: Option<IActionViewItemProvider>,
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub get_key_binding: Option<IKeybindingProvider>,
    pub anchor_alignment: Option<Rc<dyn Fn() -> AnchorAlignment>>,
}

/// `IDropdownMenuOptions`.
#[derive(Clone)]
pub struct IDropdownMenuOptions {
    pub base: IBaseDropdownOptions,
    pub context_menu_provider: Rc<dyn IContextMenuProvider>,
    pub actions: Option<Vec<ActionRef>>,
    pub action_provider: Option<Rc<dyn IActionProvider>>,
    pub menu_class_name: Option<String>,
    pub close_animation: Option<IContextViewCloseAnimation>,
    pub get_anchor: Option<Rc<dyn Fn() -> NodeId>>,
    /// `menuAsChild`: escopo reduzido de #99448.
    pub menu_as_child: bool,
    pub skip_telemetry: bool,
}

impl IDropdownMenuOptions {
    pub fn new(context_menu_provider: Rc<dyn IContextMenuProvider>) -> Self {
        Self { base: IBaseDropdownOptions::default(), context_menu_provider, actions: None, action_provider: None, menu_class_name: None, close_animation: None, get_anchor: None, menu_as_child: false, skip_telemetry: false }
    }
}

/// `DropdownMenu`.
pub struct DropdownMenu {
    base: BaseDropdown,
    weak_self: Weak<DropdownMenu>,
    options: IDropdownMenuOptions,
    menu_options: RefCell<Option<DropdownMenuOptions>>,
    actions: RefCell<Vec<ActionRef>>,
}

impl DropdownMenu {
    /// `new DropdownMenu(container, options)`.
    pub fn new(ui: &Ui, container: NodeId, options: IDropdownMenuOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            base: BaseDropdown::new_derived(ui, container, options.base.clone(), weak.clone() as Weak<dyn IDropdown>),
            weak_self: weak.clone(),
            actions: RefCell::new(options.actions.clone().unwrap_or_default()),
            menu_options: RefCell::new(None),
            options,
        })
    }

    /// `set menuOptions`.
    pub fn set_menu_options(&self, options: Option<DropdownMenuOptions>) {
        *self.menu_options.borrow_mut() = options;
    }

    /// `get menuOptions`.
    pub fn menu_options(&self) -> Option<DropdownMenuOptions> {
        self.menu_options.borrow().clone()
    }

    /// `menuOptions.context = context`, ou `menuOptions = { context }` se ainda não há opções.
    pub fn set_menu_context(&self, context: Option<Dynamic>) {
        self.menu_options.borrow_mut().get_or_insert_with(DropdownMenuOptions::default).context = context;
    }

    /// `get actions`: as do provedor, se há, senão as guardadas.
    fn actions(&self) -> Vec<ActionRef> {
        match &self.options.action_provider {
            Some(provider) => provider.get_actions(),
            None => self.actions.borrow().clone(),
        }
    }

    fn get_menu_class_name(&self) -> String {
        let mut class_names: Vec<&str> = self.options.menu_class_name.as_deref().map(|names| names.split_whitespace().collect()).unwrap_or_default();
        if !class_names.contains(&CONTEXT_VIEW_MENU_MOTION_CLASS) {
            class_names.push(CONTEXT_VIEW_MENU_MOTION_CLASS);
        }
        class_names.join(" ")
    }

    /// `onHide()`: o menu fechou.
    fn on_hide(&self) {
        self.hide();
        self.base.ui.tree_mut().remove_class(self.base.element, "active");
    }
}

impl std::ops::Deref for DropdownMenu {
    type Target = BaseDropdown;

    fn deref(&self) -> &BaseDropdown {
        &self.base
    }
}

impl IDropdown for DropdownMenu {
    fn base(&self) -> &BaseDropdown {
        &self.base
    }

    fn show(&self) {
        if self.options.base.is_disabled() {
            return;
        }
        self.base.base_show();
        self.base.ui.tree_mut().add_class(self.base.element, "active");
        self.options.context_menu_provider.show_context_menu(Rc::new(DropdownMenuDelegate { dropdown: self.weak_self.clone() }));
    }
}

impl IDisposable for DropdownMenu {
    fn dispose(&self) {
        self.base.base_dispose();
    }
}

/// O `IContextMenuDelegate` literal de `DropdownMenu.show`. Guarda o dropdown por `Weak`: o provedor de menus
/// o segura até o menu fechar, e o dropdown pode ter sido descartado nesse meio tempo.
struct DropdownMenuDelegate {
    dropdown: Weak<DropdownMenu>,
}

impl DropdownMenuDelegate {
    /// O que o delegado lê do `menuOptions`, ou o padrão se o dropdown já se foi ou não tem opções.
    fn menu_options<T>(&self, read: impl FnOnce(DropdownMenuOptions) -> Option<T>) -> Option<T> {
        self.dropdown.upgrade()?.menu_options().and_then(read)
    }
}

impl IContextMenuDelegate for DropdownMenuDelegate {
    fn get_anchor(&self) -> Anchor {
        // Sem o dropdown (já descartado) não há âncora: o menu sai no canto da janela.
        self.dropdown.upgrade().map_or(Anchor::Mouse { x: 0.0, y: 0.0 }, |d| Anchor::Node(d.options.get_anchor.as_ref().map_or(d.base.element, |get_anchor| get_anchor())))
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.dropdown.upgrade().map(|d| d.actions()).unwrap_or_default()
    }

    fn get_actions_context(&self, _event: Option<&crate::context_view_api::IContextMenuEvent>) -> Option<Dynamic> {
        self.menu_options(|options| options.context)
    }

    fn get_action_view_item(&self, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        self.menu_options(|menu| menu.action_view_item_provider).and_then(|provider| provider(action, options))
    }

    fn get_key_binding(&self, action: &ActionRef) -> Option<Rc<dyn ResolvedKeybinding>> {
        self.menu_options(|menu| menu.get_key_binding).and_then(|provider| provider(action))
    }

    fn get_menu_class_name(&self) -> Option<String> {
        self.dropdown.upgrade().map(|d| d.get_menu_class_name())
    }

    fn on_hide(&self, _did_cancel: bool) {
        if let Some(dropdown) = self.dropdown.upgrade() {
            dropdown.on_hide();
        }
    }

    fn close_animation(&self) -> Option<IContextViewCloseAnimation> {
        let dropdown = self.dropdown.upgrade()?;
        Some(dropdown.options.close_animation.clone().unwrap_or_else(context_view_menu_close_animation))
    }

    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.menu_options(|menu| menu.action_runner)
    }

    fn skip_telemetry(&self) -> bool {
        self.dropdown.upgrade().is_some_and(|d| d.options.skip_telemetry)
    }

    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        let dropdown = self.dropdown.upgrade()?;
        Some(dropdown.menu_options().and_then(|menu| menu.anchor_alignment).map_or(AnchorAlignment::Left, |alignment| alignment()))
    }

    fn dom_for_shadow_root(&self) -> Option<NodeId> {
        self.dropdown.upgrade().filter(|d| d.options.menu_as_child).map(|d| d.base.element)
    }
}
