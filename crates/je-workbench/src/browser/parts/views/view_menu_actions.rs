//! Porte de `src/vs/workbench/browser/parts/views/viewMenuActions.ts`: as ações de menu do título de uma view
//! ([`ViewMenuActions`]) e do título de um contêiner de views ([`ViewContainerMenuActions`]).
//!
//! Divergências:
//! - **Injeção.** O `@IContextKeyService` e o `@IMenuService` do construtor são argumentos explícitos.
//! - **`ViewContainerMenuActions` e a herança.** O original herda de `ViewMenuActions` e passa o serviço escopado ao
//!   `super`; aqui o tipo guarda um [`ViewMenuActions`] e o expõe por `Deref`, e o serviço escopado vira filho dele.
//! - **`element: HTMLElement`.** O alvo do serviço escopado é o [`IContextKeyServiceTarget`] do nó da view.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::actions::menu_entry_action_view_item::{PrimaryAndSecondaryActions, get_action_bar_actions};
use je_platform::actions::menu_ids;
use je_platform::actions::{IMenu, IMenuService, MenuActionOptions, MenuCreateOptions, MenuId};
use je_platform::contextkey::contextkey::{IContextKeyService, IContextKeyServiceTarget};

use crate::common::views::{IViewDescriptorService, ViewContainerRc, view_container_location_to_string};

/// `IViewMenuActionsOptions`.
#[derive(Clone, Default)]
pub struct IViewMenuActionsOptions {
    pub primary_action_groups: Option<Vec<String>>,
}

/// `ViewMenuActions`.
pub struct ViewMenuActions {
    base: Rc<Disposable>,
    pub menu_id: MenuId,
    context_menu_id: Option<MenuId>,
    options: Option<MenuActionOptions>,
    menu_actions_options: Option<IViewMenuActionsOptions>,
    context_key_service: Rc<dyn IContextKeyService>,
    menu_service: Rc<dyn IMenuService>,
    menu: Rc<dyn IMenu>,
    on_did_change: Rc<Emitter<()>>,
    actions: RefCell<Option<Rc<PrimaryAndSecondaryActions>>>,
}

impl ViewMenuActions {
    pub fn new(
        menu_id: MenuId,
        context_menu_id: Option<MenuId>,
        options: Option<MenuActionOptions>,
        menu_actions_options: Option<IViewMenuActionsOptions>,
        context_key_service: Rc<dyn IContextKeyService>,
        menu_service: Rc<dyn IMenuService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let menu = menu_service.create_menu(&menu_id, context_key_service.clone(), Some(MenuCreateOptions { emit_events_for_submenu_changes: true, event_debounce_delay: None }));
        base.register(menu.clone());

        let this = Rc::new(Self { base, menu_id, context_menu_id, options, menu_actions_options, context_key_service, menu_service, menu, on_did_change, actions: RefCell::new(None) });

        let weak = Rc::downgrade(&this);
        this.base.register(this.menu.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.actions.borrow_mut() = None;
                this.on_did_change.fire(());
            }
        }));

        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn get_actions(&self) -> Rc<PrimaryAndSecondaryActions> {
        if let Some(actions) = self.actions.borrow().clone() {
            return actions;
        }
        let groups = self.menu.get_actions(self.options.as_ref());
        let actions = Rc::new(get_action_bar_actions(&groups, &|group| self.is_primary_action_group(group), true));
        *self.actions.borrow_mut() = Some(actions.clone());
        actions
    }

    fn is_primary_action_group(&self, group: &str) -> bool {
        if group == "navigation" {
            return true;
        }

        if let Some(primary_action_groups) = self.menu_actions_options.as_ref().and_then(|options| options.primary_action_groups.as_ref()) {
            return primary_action_groups.iter().any(|primary| primary == group);
        }

        false
    }

    /// `getPrimaryActions()`.
    pub fn get_primary_actions(&self) -> Vec<ActionRef> {
        self.get_actions().primary.clone()
    }

    /// `getSecondaryActions()`.
    pub fn get_secondary_actions(&self) -> Vec<ActionRef> {
        self.get_actions().secondary.clone()
    }

    /// `getContextMenuActions()`.
    pub fn get_context_menu_actions(&self) -> Vec<ActionRef> {
        if let Some(context_menu_id) = &self.context_menu_id {
            let menu = self.menu_service.get_menu_actions(context_menu_id, self.context_key_service.as_ref(), self.options.as_ref());
            return get_action_bar_actions(&menu, &|group| group == "navigation", false).secondary;
        }
        Vec::new()
    }
}

impl IDisposable for ViewMenuActions {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `ViewContainerMenuActions`.
pub struct ViewContainerMenuActions {
    inner: Rc<ViewMenuActions>,
}

impl ViewContainerMenuActions {
    pub fn new(
        element: Rc<dyn IContextKeyServiceTarget>,
        view_container: ViewContainerRc,
        menu_actions_options: Option<IViewMenuActionsOptions>,
        view_descriptor_service: Rc<dyn IViewDescriptorService>,
        context_key_service: Rc<dyn IContextKeyService>,
        menu_service: Rc<dyn IMenuService>,
    ) -> Rc<Self> {
        let scoped_context_key_service = context_key_service.create_scoped(element);
        scoped_context_key_service.create_key("viewContainer", Dynamic::from(view_container.id.as_str()));
        let location_name = |service: &dyn IViewDescriptorService, container: &ViewContainerRc| {
            Dynamic::from(view_container_location_to_string(service.get_view_container_location(container).expect("o contêiner tem uma localização")))
        };
        let view_container_location_key = scoped_context_key_service.create_key("viewContainerLocation", location_name(view_descriptor_service.as_ref(), &view_container));

        let inner = ViewMenuActions::new(
            menu_ids::view_container_title(),
            Some(menu_ids::view_container_title_context()),
            Some(MenuActionOptions { should_forward_args: true, render_short_title: true, ..Default::default() }),
            menu_actions_options,
            scoped_context_key_service.clone(),
            menu_service,
        );
        inner.base.register(scoped_context_key_service);

        let (service, container) = (view_descriptor_service.clone(), view_container.clone());
        inner.base.register(view_descriptor_service.on_did_change_container_location().listen(move |change| {
            if Rc::ptr_eq(&change.view_container, &container) {
                view_container_location_key.set(location_name(service.as_ref(), &container));
            }
        }));

        Rc::new(Self { inner })
    }
}

impl Deref for ViewContainerMenuActions {
    type Target = ViewMenuActions;

    fn deref(&self) -> &ViewMenuActions {
        &self.inner
    }
}

impl IDisposable for ViewContainerMenuActions {
    fn dispose(&self) {
        self.inner.dispose();
    }
}
