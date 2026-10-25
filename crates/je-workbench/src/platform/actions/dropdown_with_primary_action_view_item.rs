//! Porte de `src/vs/platform/actions/browser/dropdownWithPrimaryActionViewItem.ts`: [`DropdownWithPrimaryActionViewItem`], o botão
//! dividido de uma ação principal (um [`MenuEntryActionViewItem`]) com a seta de um menu de ações secundárias ao lado. A barra de
//! execução da célula do notebook (`codeCellRunToolbar.ts`) o usa.
//!
//! Divergências:
//! - **Serviços injetados chegam como argumentos.** `IContextMenuService` é o provedor de menu de contexto; `IKeybindingService` e
//!   `IContextKeyService` são o [`MenuEntryServices`] do item principal. `INotificationService`, `IThemeService` e
//!   `IAccessibilityService` só serviam ao `MenuEntryActionViewItem` e não existem na árvore nativa (ver o módulo dele).
//! - **`onDidChangeDropdownVisibility`** é um [`Emitter`] próprio que repassa a visibilidade do menu atual, porque `update(...)`
//!   troca o `DropdownMenuActionViewItem` e o ouvinte do chamador precisa sobreviver à troca.
//! - **Tamanho.** Não há motor de layout: `size` soma os tamanhos naturais dos dois itens e `layout` põe o contêiner da ação
//!   principal (`.action-container`) e o do menu (`.dropdown-action-container`) lado a lado, a altura inteira.
//! - **`tabIndex` do elemento principal** é [`IActionViewItem::set_focusable`] do item principal, e `element.focus()` é `focus`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_ui::{EventKind, NodeId, Role, Ui};
use je_widgets::action_view_items::{BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};
use je_widgets::context_view_api::{IContextMenuProvider, IContextViewCloseAnimation};
use je_widgets::dom::{StandardKeyboardEvent, create_el};
use je_widgets::dropdown_action_view_item::{DropdownMenuActionViewItem, IDropdownMenuActionViewItemOptions, IKeybindingProvider, MenuActions};
use je_widgets::hover_api::IHoverDelegate;
use je_platform::keybinding::keybinding::IKeybindingService;

use super::menu_entry_action_view_item::{IMenuEntryActionViewItemOptions, MenuEntryActionViewItem, MenuEntryServices};

/// `IDropdownWithPrimaryActionViewItemOptions`.
#[derive(Clone, Default)]
pub struct IDropdownWithPrimaryActionViewItemOptions {
    pub action_runner: Option<Rc<dyn IActionRunner>>,
    pub get_key_binding: Option<IKeybindingProvider>,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    pub menu_class_name: Option<String>,
    pub close_animation: Option<IContextViewCloseAnimation>,
    pub get_anchor: Option<Rc<dyn Fn() -> NodeId>>,
    pub menu_as_child: Option<bool>,
    pub skip_telemetry: bool,
}

/// `DropdownWithPrimaryActionViewItem`.
pub struct DropdownWithPrimaryActionViewItem {
    base: BaseActionViewItem,
    weak_self: Weak<DropdownWithPrimaryActionViewItem>,
    primary_action: Rc<MenuEntryActionViewItem>,
    dropdown: RefCell<Rc<DropdownMenuActionViewItem>>,
    options: IDropdownWithPrimaryActionViewItemOptions,
    context_menu_provider: Rc<dyn IContextMenuProvider>,
    keybinding_service: Rc<dyn IKeybindingService>,
    container: Cell<Option<NodeId>>,
    primary_container: Cell<Option<NodeId>>,
    dropdown_container: Cell<Option<NodeId>>,
    on_did_change_dropdown_visibility: Rc<Emitter<bool>>,
}

impl DropdownWithPrimaryActionViewItem {
    /// `new DropdownWithPrimaryActionViewItem(primaryAction, dropdownAction, dropdownMenuActions, className, options, ...)`. A
    /// `primary_action` é a de um `MenuItemAction`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        primary_action: ActionRef,
        dropdown_action: ActionRef,
        dropdown_menu_actions: MenuActions,
        class_name: &str,
        options: Option<IDropdownWithPrimaryActionViewItemOptions>,
        context_menu_provider: Rc<dyn IContextMenuProvider>,
        services: MenuEntryServices,
    ) -> Rc<Self> {
        let options = options.unwrap_or_default();
        let keybinding_service = services.keybinding_service.clone();
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let base = BaseActionViewItem::new_derived(
                ui,
                None,
                primary_action.clone(),
                IActionViewItemOptions { hover_delegate: options.hover_delegate.clone(), ..IActionViewItemOptions::default() },
                weak.clone() as Weak<dyn IActionViewItem>,
            );
            let primary = MenuEntryActionViewItem::new(
                ui,
                primary_action,
                Some(IMenuEntryActionViewItemOptions { hover_delegate: options.hover_delegate.clone(), ..IMenuEntryActionViewItemOptions::default() }),
                services,
            );
            if let Some(action_runner) = options.action_runner.clone() {
                primary.set_action_runner(action_runner);
            }

            let class_names = if class_name.is_empty() { vec!["codicon".to_owned(), "codicon-chevron-down".to_owned()] } else { vec!["codicon".to_owned(), "codicon-chevron-down".to_owned(), class_name.to_owned()] };
            let dropdown = Self::create_dropdown(ui, dropdown_action, dropdown_menu_actions, class_names, &options, &context_menu_provider, &keybinding_service, true);
            let on_did_change_dropdown_visibility = Emitter::new();
            base.register(on_did_change_dropdown_visibility.clone());
            Self::forward_visibility(&base, &dropdown, &on_did_change_dropdown_visibility);

            Self {
                base,
                weak_self: weak.clone(),
                primary_action: primary,
                dropdown: RefCell::new(dropdown),
                options,
                context_menu_provider,
                keybinding_service,
                container: Cell::new(None),
                primary_container: Cell::new(None),
                dropdown_container: Cell::new(None),
                on_did_change_dropdown_visibility,
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn create_dropdown(
        ui: &Ui,
        dropdown_action: ActionRef,
        dropdown_menu_actions: MenuActions,
        class_names: Vec<String>,
        options: &IDropdownWithPrimaryActionViewItemOptions,
        context_menu_provider: &Rc<dyn IContextMenuProvider>,
        keybinding_service: &Rc<dyn IKeybindingService>,
        with_default_keybinding_provider: bool,
    ) -> Rc<DropdownMenuActionViewItem> {
        let keybinding_provider = options.get_key_binding.clone().or_else(|| {
            with_default_keybinding_provider.then(|| {
                let keybinding_service = keybinding_service.clone();
                Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false)) as IKeybindingProvider
            })
        });
        DropdownMenuActionViewItem::new(
            ui,
            dropdown_action,
            dropdown_menu_actions,
            context_menu_provider.clone(),
            IDropdownMenuActionViewItemOptions {
                base: IActionViewItemOptions { hover_delegate: options.hover_delegate.clone(), ..IActionViewItemOptions::default() },
                menu_as_child: options.menu_as_child.unwrap_or(true),
                class_names,
                menu_class_name: options.menu_class_name.clone(),
                close_animation: options.close_animation.clone(),
                get_anchor: options.get_anchor.clone(),
                action_runner: options.action_runner.clone(),
                keybinding_provider,
                skip_telemetry: options.skip_telemetry,
                ..IDropdownMenuActionViewItemOptions::default()
            },
        )
    }

    fn forward_visibility(base: &BaseActionViewItem, dropdown: &Rc<DropdownMenuActionViewItem>, fire: &Rc<Emitter<bool>>) {
        let fire = fire.clone();
        base.register(dropdown.on_did_change_visibility().listen(move |visible| fire.fire(*visible)));
    }

    /// `get onDidChangeDropdownVisibility`.
    pub fn on_did_change_dropdown_visibility(&self) -> BaseEvent<bool> {
        self.on_did_change_dropdown_visibility.event()
    }

    fn dropdown(&self) -> Rc<DropdownMenuActionViewItem> {
        self.dropdown.borrow().clone()
    }

    /// `update(dropdownAction, dropdownMenuActions, dropdownIcon?)`.
    pub fn update(&self, dropdown_action: ActionRef, dropdown_menu_actions: Vec<ActionRef>, dropdown_icon: Option<&str>) {
        self.dropdown().dispose();
        let icon = dropdown_icon.filter(|icon| !icon.is_empty()).unwrap_or("codicon-chevron-down");
        // O `update` do original não passa `skipTelemetry` nem o provedor de atalhos padrão.
        let options = IDropdownWithPrimaryActionViewItemOptions { skip_telemetry: false, ..self.options.clone() };
        let dropdown = Self::create_dropdown(
            self.base.ui(),
            dropdown_action,
            MenuActions::Actions(dropdown_menu_actions),
            vec!["codicon".to_owned(), icon.to_owned()],
            &options,
            &self.context_menu_provider,
            &self.keybinding_service,
            false,
        );
        Self::forward_visibility(&self.base, &dropdown, &self.on_did_change_dropdown_visibility);
        *self.dropdown.borrow_mut() = dropdown.clone();
        if let Some(dropdown_container) = self.dropdown_container.get() {
            dropdown.render(dropdown_container);
        }
    }

    /// `showDropdown()`.
    pub fn show_dropdown(&self) {
        self.dropdown().show();
    }

    fn on_key_down(&self, e: &mut je_ui::Event, from_primary: bool) {
        if !self.base.action().enabled() {
            return;
        }
        let Some(event) = StandardKeyboardEvent::from_event(e) else { return };
        if from_primary && event.equals(KeyCode::RightArrow.0 as u32) {
            self.primary_action.set_focusable(false);
            self.dropdown().focus(false);
            e.stop(true);
        } else if !from_primary && event.equals(KeyCode::LeftArrow.0 as u32) {
            let dropdown = self.dropdown();
            self.primary_action.set_focusable(true);
            dropdown.set_focusable(false);
            self.primary_action.focus(false);
            e.stop(true);
        }
    }
}

impl IActionViewItem for DropdownWithPrimaryActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        self.base.set_action_runner(action_runner.clone());
        self.primary_action.set_action_runner(action_runner.clone());
        self.dropdown().set_action_runner(action_runner);
    }

    fn set_action_context(&self, context: Option<Dynamic>) {
        self.base.set_action_context(context.clone());
        self.primary_action.set_action_context(context.clone());
        self.dropdown().set_action_context(context);
    }

    fn render(&self, container: NodeId) {
        self.container.set(Some(container));
        self.base.base_render(container);
        let ui = self.base.ui().clone();
        let (primary_container, dropdown_container) = {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "monaco-dropdown-with-primary");
            let primary_container = create_el(&mut tree, container, Role::Button, "action-container");
            let dropdown_container = create_el(&mut tree, container, Role::Generic, "dropdown-action-container");
            (primary_container, dropdown_container)
        };
        self.primary_container.set(Some(primary_container));
        self.dropdown_container.set(Some(dropdown_container));
        self.primary_action.render(primary_container);
        self.dropdown().render(dropdown_container);

        for (node, from_primary) in [(primary_container, true), (dropdown_container, false)] {
            let weak = self.weak_self.clone();
            self.base.register(ui.listen(node, EventKind::KeyDown, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_key_down(e, from_primary);
                }
            }));
        }

        self.update_enabled();
        self.base.request_layout();
    }

    fn focus(&self, from_right: bool) {
        if from_right {
            self.dropdown().focus(false);
        } else {
            self.primary_action.set_focusable(true);
            self.primary_action.focus(false);
        }
    }

    fn blur(&self) {
        self.primary_action.set_focusable(false);
        self.dropdown().blur();
        self.base.base_blur();
    }

    fn set_focusable(&self, focusable: bool) {
        self.primary_action.set_focusable(focusable);
        if !focusable {
            self.dropdown().set_focusable(false);
        }
    }

    fn tab_stops(&self) -> usize {
        self.primary_action.tab_stops() + self.dropdown().tab_stops()
    }

    fn update_enabled(&self) {
        let disabled = !self.base.action().enabled();
        if let Some(element) = self.base.element() {
            self.base.ui().tree_mut().toggle_class(element, "disabled", disabled);
        }
    }

    fn size(&self, vertical: bool) -> ItemSize {
        let (primary_width, primary_height) = self.primary_action.size(vertical);
        let (dropdown_width, dropdown_height) = self.dropdown().size(vertical);
        (primary_width + dropdown_width, primary_height.max(dropdown_height))
    }

    fn layout(&self, vertical: bool, _width: f64, height: f64) {
        let (Some(primary_container), Some(dropdown_container)) = (self.primary_container.get(), self.dropdown_container.get()) else { return };
        let (primary_width, _) = self.primary_action.size(vertical);
        let (dropdown_width, _) = self.dropdown().size(vertical);
        {
            let mut tree = self.base.ui().tree_mut();
            tree.set_bounds(primary_container, je_render::kurbo::Rect::new(0.0, 0.0, primary_width, height));
            tree.set_bounds(dropdown_container, je_render::kurbo::Rect::new(primary_width, 0.0, primary_width + dropdown_width, height));
        }
        self.primary_action.layout(vertical, primary_width, height);
        self.dropdown().layout(vertical, dropdown_width, height);
    }
}

impl IDisposable for DropdownWithPrimaryActionViewItem {
    fn dispose(&self) {
        self.primary_action.dispose();
        self.dropdown().dispose();
        self.base.base_dispose();
    }
}
