//! Porte parcial de `src/vs/platform/actions/browser/menuEntryActionViewItem.ts`: `MenuEntryActionViewItem` e
//! `SubmenuEntryActionViewItem`, as classes-base que `cellActionView.ts` (as partes de célula do notebook) estende. A lógica de
//! modelo do arquivo (`getActionBarActions` e companhia) está em `je_platform::actions::menu_entry_action_view_item`.
//!
//! Herança vira composição, como em `je-widgets`: [`MenuEntryActionViewItem`] guarda um [`ActionViewItem`] e uma subclasse guarda um
//! [`MenuEntryActionViewItem`] (construído com [`MenuEntryActionViewItem::new_with_owner`], passando o `Weak` de si mesma para o base
//! chamar os ganchos que ela sobrescreve). A macro [`delegate_action_view_item`] gera o que uma subclasse não sobrescreve.
//!
//! Divergências:
//! - **Troca pela ação alternativa (`alt`).** O `ModifierKeyEmitter` que alterna rótulo, dica e ícone para o `alt` do item não é
//!   honrado: o [`ActionViewItem`] desenha o rótulo e o ícone da própria ação e não expõe um gancho para trocá-los. A ação rodada no
//!   clique e a dica são sempre as da ação principal. As barras das células do notebook não usam `alt`.
//! - **`onClickAnimation` e `_notificationService.error`.** A animação de clique não existe numa UI de nós nativos e o `run` do
//!   `ActionRunner` já reporta o erro da ação, então `IMenuEntryActionViewItemOptions` não tem `onClickAnimation` nem
//!   `onDidTriggerClickAnimation`.
//! - **Ícone por URI.** O `backgroundImage` do ícone `{ dark, light }` não tem equivalente na árvore: o item só ganha a classe
//!   `icon`, e o desenho do ícone fica com o codicon da classe da ação.
//! - **Serviços injetados chegam como argumentos** ([`MenuEntryServices`]).

use std::rc::Rc;

use je_base::actions::{ActionRef, IAction};
use je_base::themables::ThemeIcon;
use je_platform::action::Icon;
use je_platform::actions::{MenuItemAction, SubmenuItemAction};
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_ui::{NodeId, Ui};
use je_widgets::action_view_items::{ActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};
use je_widgets::context_view_api::IContextMenuProvider;
use je_widgets::dropdown_action_view_item::{DropdownMenuActionViewItem, IDropdownMenuActionViewItemOptions, MenuActions};
use je_widgets::hover_api::IHoverDelegate;

/// Gera o `impl IActionViewItem` e o `impl IDisposable` de uma subclasse que guarda o item pai (um `Rc` de um [`IActionViewItem`]) no
/// campo `$field`. A macro delega tudo que uma subclasse de `MenuEntryActionViewItem` não costuma sobrescrever; o bloco `$custom`
/// traz o que ela sobrescreve: `render`, `update_label`, `update_class`, `get_tooltip`, `size` e `layout`.
macro_rules! delegate_action_view_item {
    ($type:ty, $field:ident, { $($custom:tt)* }) => {
        impl je_widgets::action_view_items::IActionViewItem for $type {
            fn base(&self) -> &je_widgets::action_view_items::BaseActionViewItem {
                je_widgets::action_view_items::IActionViewItem::base(&*self.$field)
            }

            fn focus(&self, from_right: bool) {
                je_widgets::action_view_items::IActionViewItem::focus(&*self.$field, from_right)
            }

            fn is_focused(&self) -> bool {
                je_widgets::action_view_items::IActionViewItem::is_focused(&*self.$field)
            }

            fn blur(&self) {
                je_widgets::action_view_items::IActionViewItem::blur(&*self.$field)
            }

            fn set_focusable(&self, focusable: bool) {
                je_widgets::action_view_items::IActionViewItem::set_focusable(&*self.$field, focusable)
            }

            fn tab_stops(&self) -> usize {
                je_widgets::action_view_items::IActionViewItem::tab_stops(&*self.$field)
            }

            fn update_enabled(&self) {
                je_widgets::action_view_items::IActionViewItem::update_enabled(&*self.$field)
            }

            fn update_checked(&self) {
                je_widgets::action_view_items::IActionViewItem::update_checked(&*self.$field)
            }

            fn update_aria_label(&self) {
                je_widgets::action_view_items::IActionViewItem::update_aria_label(&*self.$field)
            }

            $($custom)*
        }

        impl je_base::lifecycle::IDisposable for $type {
            fn dispose(&self) {
                je_base::lifecycle::IDisposable::dispose(&*self.$field)
            }
        }
    };
}

pub(crate) use delegate_action_view_item;

/// `IMenuEntryActionViewItemOptions`, sem `onClickAnimation` (ver as divergências do módulo).
#[derive(Clone, Default)]
pub struct IMenuEntryActionViewItemOptions {
    pub draggable: bool,
    pub keybinding: Option<String>,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    pub keybinding_not_rendered_with_label: bool,
}

/// Os serviços que o construtor do original recebe por injeção.
#[derive(Clone)]
pub struct MenuEntryServices {
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
}

/// `MenuEntryActionViewItem`.
pub struct MenuEntryActionViewItem {
    inner: Rc<ActionViewItem>,
    action: ActionRef,
    services: MenuEntryServices,
    /// `options.icon`: o item desenha o ícone da ação; sem ele (`options.label`) desenha o rótulo.
    has_icon: bool,
}

impl MenuEntryActionViewItem {
    /// `new MenuEntryActionViewItem(action, options, ...)`. A `action` é a de um `MenuItemAction`.
    pub fn new(ui: &Ui, action: ActionRef, options: Option<IMenuEntryActionViewItemOptions>, services: MenuEntryServices) -> Rc<Self> {
        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| Self::construct(ui, action, options, services, weak.clone() as std::rc::Weak<dyn IActionViewItem>))
    }

    /// O construtor para subclasses: `owner` é o `Weak` da subclasse, a quem o base chama os ganchos que ela sobrescreve.
    pub fn new_with_owner(ui: &Ui, action: ActionRef, options: Option<IMenuEntryActionViewItemOptions>, services: MenuEntryServices, owner: std::rc::Weak<dyn IActionViewItem>) -> Rc<Self> {
        Rc::new(Self::construct(ui, action, options, services, owner))
    }

    fn construct(ui: &Ui, action: ActionRef, options: Option<IMenuEntryActionViewItemOptions>, services: MenuEntryServices, owner: std::rc::Weak<dyn IActionViewItem>) -> Self {
        let options = options.unwrap_or_default();
        let menu_item_action = action.as_any().downcast_ref::<MenuItemAction>().expect("MenuEntryActionViewItem exige um MenuItemAction");
        let has_icon = action.class().is_some_and(|class| !class.is_empty()) || menu_item_action.item.icon.is_some();
        let base_options = IActionViewItemOptions {
            icon: Some(has_icon),
            label: Some(!has_icon),
            draggable: options.draggable,
            keybinding: options.keybinding,
            hover_delegate: options.hover_delegate,
            keybinding_not_rendered_with_label: options.keybinding_not_rendered_with_label,
            ..IActionViewItemOptions::default()
        };
        Self { inner: ActionViewItem::new_with_owner(ui, None, action.clone(), base_options, owner), action, services, has_icon }
    }

    /// `this.options.label`: o item desenha o rótulo (a ação não tem ícone nem classe).
    pub fn is_label_mode(&self) -> bool {
        !self.has_icon
    }

    /// `_menuItemAction`.
    pub fn menu_item_action(&self) -> &MenuItemAction {
        self.action.as_any().downcast_ref::<MenuItemAction>().expect("MenuEntryActionViewItem exige um MenuItemAction")
    }

    /// `_commandAction`: sem a troca pela ação alternativa (ver as divergências do módulo), é a própria ação.
    pub fn command_action(&self) -> &MenuItemAction {
        self.menu_item_action()
    }

    /// O [`ActionViewItem`] pai, para a subclasse que precisa do nó do rótulo.
    pub fn inner(&self) -> &Rc<ActionViewItem> {
        &self.inner
    }

    /// `getTooltip()`.
    fn tooltip_with_keybinding(&self) -> String {
        let command_action = self.command_action();
        let tooltip = if command_action.tooltip().is_empty() { command_action.label() } else { command_action.tooltip() };
        self.services.keybinding_service.append_keybinding(&tooltip, Some(&command_action.id()), Some(&*self.services.context_key_service), false)
    }
}

delegate_action_view_item!(MenuEntryActionViewItem, inner, {
    fn render(&self, container: NodeId) {
        self.inner.render(container);
        self.inner.base().ui().tree_mut().add_class(container, "menu-entry");
    }

    fn update_label(&self) {
        self.inner.update_label();
    }

    fn update_class(&self) {
        self.inner.update_class();
    }

    fn get_tooltip(&self) -> Option<String> {
        Some(self.tooltip_with_keybinding())
    }

    fn size(&self, vertical: bool) -> ItemSize {
        self.inner.size(vertical)
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.inner.layout(vertical, width, height);
    }
});

/// `SubmenuEntryActionViewItem`.
pub struct SubmenuEntryActionViewItem {
    inner: Rc<DropdownMenuActionViewItem>,
    action: Rc<SubmenuItemAction>,
}

impl SubmenuEntryActionViewItem {
    /// `new SubmenuEntryActionViewItem(action, options, keybindingService, contextMenuService, themeService)`: o `contextMenuService`
    /// chega como o provedor de menu de contexto.
    pub fn new(
        ui: &Ui,
        action: Rc<SubmenuItemAction>,
        options: Option<IDropdownMenuActionViewItemOptions>,
        keybinding_service: Rc<dyn IKeybindingService>,
        context_menu_provider: Rc<dyn IContextMenuProvider>,
    ) -> Rc<Self> {
        let mut dropdown_options = options.unwrap_or_default();
        if dropdown_options.class_names.is_empty()
            && let Some(Icon::Theme(icon)) = &action.item.icon
        {
            dropdown_options.class_names = ThemeIcon::as_class_name(icon).split_whitespace().map(str::to_owned).collect();
        }
        if dropdown_options.keybinding_provider.is_none() {
            dropdown_options.keybinding_provider = Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false)));
        }
        let submenu = action.clone();
        let menu_actions = MenuActions::Provider(Rc::new(move || submenu.actions().to_vec()));
        Rc::new(Self { inner: DropdownMenuActionViewItem::new(ui, action.as_action(), menu_actions, context_menu_provider, dropdown_options), action })
    }

    /// O [`DropdownMenuActionViewItem`] pai.
    pub fn inner(&self) -> &Rc<DropdownMenuActionViewItem> {
        &self.inner
    }

    /// A `SubmenuItemAction` do item (`this._action`).
    pub fn submenu_item_action(&self) -> &Rc<SubmenuItemAction> {
        &self.action
    }
}

impl IActionViewItem for SubmenuEntryActionViewItem {
    fn base(&self) -> &je_widgets::action_view_items::BaseActionViewItem {
        self.inner.base()
    }

    fn render(&self, container: NodeId) {
        self.inner.render(container);
        let ui = self.inner.base().ui().clone();
        let mut tree = ui.tree_mut();
        tree.add_class(container, "menu-entry");
        if matches!(self.action.item.icon, Some(Icon::Uris { .. }))
            && let Some(element) = self.inner.element()
        {
            tree.add_class(element, "icon");
        }
    }

    fn focus(&self, from_right: bool) {
        self.inner.focus(from_right);
    }

    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }

    fn blur(&self) {
        self.inner.blur();
    }

    fn set_focusable(&self, focusable: bool) {
        self.inner.set_focusable(focusable);
    }

    fn tab_stops(&self) -> usize {
        self.inner.tab_stops()
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn set_action_context(&self, context: Option<je_base::dynamic::Dynamic>) {
        self.inner.set_action_context(context);
    }

    fn update_enabled(&self) {
        self.inner.update_enabled();
    }

    fn size(&self, vertical: bool) -> ItemSize {
        self.inner.size(vertical)
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.inner.layout(vertical, width, height);
    }
}

impl je_base::lifecycle::IDisposable for SubmenuEntryActionViewItem {
    fn dispose(&self) {
        je_base::lifecycle::IDisposable::dispose(&*self.inner);
    }
}
