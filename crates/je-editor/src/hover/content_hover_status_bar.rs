//! Porte de `src/vs/editor/contrib/hover/browser/contentHoverStatusBar.ts`: o [`EditorHoverStatusBar`], a linha de ações no
//! pé do hover (`div.hover-row.status-bar` com um `div.actions` dentro).
//!
//! Como o DOM vira [`je_ui`]:
//! - `hoverElement` é um nó solto com as classes `hover-row` e `status-bar` e `tabindex` 0, e `actionsElement` é o filho de
//!   classe `actions`, com o fundo `editorHoverWidget.statusBarBackground` ([`HoverStyles::status_bar_background`]), como a
//!   barra de status do `WorkbenchHoverWidget`. Quem monta o hover dimensiona a linha e a põe na árvore.
//! - `HoverAction.render(actionsElement, actionOptions, keybindingLabel)` é o [`HoverAction::render`] do `je-widgets`, que
//!   posiciona cada ação à direita das que já estão em `actionsElement`; o elemento de `append` posiciona-se a si mesmo.
//! - `@IKeybindingService` é o `Rc<dyn IKeybindingService>` e `@IHoverService` é o `Rc<dyn IHoverDelegate2>` do `je-widgets`
//!   (o serviço de dicas sobre nós da árvore), e o `getDefaultHoverDelegate('element')` é
//!   `get_default_hover_delegate(HoverPlacement::Element)`.
//! - As cores (`HoverStyles`) chegam pelo construtor, lidas do tema por quem monta o hover.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::keybinding::IKeybindingService;
use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::{create_detached_el, create_el};
use je_widgets::hover_api::{HoverPlacement, IHoverAction, IHoverDelegate2, ManagedHoverContentOrFactory, get_default_hover_delegate};
use je_widgets::hover_widget::{HoverAction, HoverStyles};

use crate::hover::hover_types::{IEditorHoverAction, IEditorHoverStatusBar};

/// `EditorHoverStatusBar`.
pub struct EditorHoverStatusBar {
    base: Rc<Disposable>,
    ui: Ui,
    styles: HoverStyles,
    keybinding_service: Rc<dyn IKeybindingService>,
    hover_service: Rc<dyn IHoverDelegate2>,
    hover_element: NodeId,
    actions_element: NodeId,
    actions: RefCell<Vec<Rc<HoverAction>>>,
    has_content: Cell<bool>,
}

impl EditorHoverStatusBar {
    pub fn new(ui: &Ui, styles: HoverStyles, keybinding_service: Rc<dyn IKeybindingService>, hover_service: Rc<dyn IHoverDelegate2>) -> Rc<Self> {
        let (hover_element, actions_element) = {
            let mut tree = ui.tree_mut();
            let hover_element = create_detached_el(&mut tree, Role::Generic, "hover-row status-bar");
            tree.set_attribute(hover_element, "tabindex", "0");
            let actions_element = create_el(&mut tree, hover_element, Role::Generic, "actions");
            tree.style_mut(actions_element).background = styles.status_bar_background;
            (hover_element, actions_element)
        };
        Rc::new(Self {
            base: Disposable::new(),
            ui: ui.clone(),
            styles,
            keybinding_service,
            hover_service,
            hover_element,
            actions_element,
            actions: RefCell::new(Vec::new()),
            has_content: Cell::new(false),
        })
    }

    /// `hoverElement`.
    pub fn hover_element(&self) -> NodeId {
        self.hover_element
    }

    /// `actions`.
    pub fn actions(&self) -> Vec<Rc<HoverAction>> {
        self.actions.borrow().clone()
    }

    /// `hasContent`.
    pub fn has_content(&self) -> bool {
        self.has_content.get()
    }
}

impl IEditorHoverStatusBar for EditorHoverStatusBar {
    fn add_action(&self, action_options: IHoverAction) -> Rc<dyn IEditorHoverAction> {
        let keybinding = self.keybinding_service.lookup_keybinding(&action_options.command_id, None, false);
        let keybinding_label = keybinding.and_then(|keybinding| keybinding.get_label());
        self.has_content.set(true);
        let action = HoverAction::render(&self.ui, self.actions_element, &action_options, keybinding_label.as_deref(), &self.styles);
        self.base.register(action.clone());
        let managed_hover = self.hover_service.setup_managed_hover(
            get_default_hover_delegate(HoverPlacement::Element),
            action.action_container,
            ManagedHoverContentOrFactory::from(action.action_rendered_label.as_str()),
            None,
        );
        self.base.register(managed_hover);
        self.actions.borrow_mut().push(Rc::clone(&action));
        action
    }

    fn append(&self, element: NodeId) -> NodeId {
        self.ui.tree_mut().append_child(self.actions_element, element);
        self.has_content.set(true);
        element
    }
}

impl IDisposable for EditorHoverStatusBar {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "EditorHoverStatusBar"
    }
}
