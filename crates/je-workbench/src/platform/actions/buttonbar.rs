//! Porte de `src/vs/platform/actions/browser/buttonbar.ts` (e do `buttonbar.css`): a [`WorkbenchButtonBar`], a `ButtonBar`
//! de `je-widgets` alimentada por ações (cada ação vira um botão, uma `SubmenuAction` vira um botão com menu, as ações
//! secundárias vão a um botão "mais"), com o `workbenchActionExecuted` por ação, o atalho no título e o spinner de
//! trabalho em andamento, e a [`MenuWorkbenchButtonBar`], alimentada por um menu.
//!
//! Vive no `je-workbench` (e não no `je-platform`) porque estende a `ButtonBar` de `je-widgets`, e a plataforma não
//! depende de widgets.
//!
//! Divergências de tradução:
//! - **Herança.** `WorkbenchButtonBar extends ButtonBar` é composição com `Deref<Target = ButtonBar>` (`buttons`,
//!   `clear`, `add_button`...). O `MenuWorkbenchButtonBar`, que sobrescreve `update` para lançar, não tem `Deref`: expõe só o
//!   que o original deixa usar e um `update` que lança. `Event<this>` do `onDidChange` é `Event<()>`.
//! - **Injeção.** O `IHoverService` é o `IHoverDelegate2` de `je-widgets` (o serviço que o `setupManagedHover` dos widgets
//!   consome); o `IContextMenuService` é o da plataforma, visto como provedor do botão com menu por
//!   [`ButtonContextMenuServiceProvider`].
//! - **Cores.** O `.monaco-button.default-colors` do CSS lê as variáveis `--vscode-button-*`; aqui as cores são
//!   [`IWorkbenchButtonBarOptions::button_styles`] (o `defaultButtonStyles` do tema), como o `styles` do `ActionBar`. Sem
//!   valor, o botão não pinta fundo nem borda.
//! - **Vaga à frente.** `button.prepend(leading)` é [`Button::set_leading`]: o ícone (`span` com as classes da ação) ou o
//!   spinner vira o primeiro filho do botão e o layout reserva a vaga (ver `buttonbar.css`: `16px` mais `4px` até o
//!   rótulo, sem a margem no botão só de ícone). O ícone da ação é o glifo `codicon-*` das classes dela.
//! - **Ícone só de ícone.** As classes `codicon codicon-x` que o original põe no próprio botão são [`Button::set_icon`]
//!   (o botão do `je-widgets` só desenha o glifo pelo `set_icon`); as classes que sobram são acrescentadas como estão.
//! - **`customLabelObs`.** `autorun` e `IObservable` são os de `je_base::observable`; o valor é `Option<ButtonLabel>`.
//! - **`renderAsPlaintext`.** Um rótulo de texto vale por si; um Markdown passa pelo `render_as_plaintext` de `je-base`.
//! - **`async` do clique.** `await this._actionRunner.run(action)` com `finally` é uma `Promise` encadeada em
//!   [`async_fn`](je_base::async_::async_fn).

use std::cell::Cell;
use std::ops::Deref;
use std::rc::Rc;

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, IRunEvent, SubmenuAction};
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::icon_labels::strip_icons;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::markdown_plaintext::render_as_plaintext;
use je_base::nls::localize;
use je_base::observable::base::{IObservable, IReaderWithStore, Observable};
use je_base::observable::reactions::autorun::autorun;
use je_base::themables::ThemeIcon;
use je_platform::action::Icon;
use je_platform::actions::menu_entry_action_view_item::get_action_bar_actions;
use je_platform::actions::{IMenu, IMenuService, MenuActionOptions, MenuId, MenuItemAction};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::telemetry::ITelemetryService;
use je_render::Rgba;
use je_ui::{NodeId, Role, Ui};
use je_widgets::action_view_items::ActionViewItem;
use je_widgets::button::{
    Button, ButtonBar, ButtonDropdownActions, ButtonLabel, ButtonOptions, ButtonStyles, ButtonTitle, ButtonWithDropdown, ButtonWithDropdownOptions, IButton,
    IContextMenuProvider as IButtonContextMenuProvider,
};
use je_widgets::dom::create_detached_el;
use je_widgets::hover_api::{IHoverDelegate, IHoverDelegate2, IManagedHover, ManagedHoverContentOrFactory, create_instant_hover_delegate};
use je_widgets::pixel_spinner::create_pixel_spinner;

use super::toolbar::{IToolBarRenderOptions, PrimaryGroup};
use crate::platform::contextview::ButtonContextMenuServiceProvider;
use crate::services::contextview::native_context_view_service::node_element;

/// A cor do texto que o botão herda sem estilos (o `color: inherit` do original).
const INHERITED_BUTTON_FOREGROUND: Rgba = Rgba::opaque(0xCC, 0xCC, 0xCC);

/// `IButtonConfig`.
#[derive(Clone, Default)]
pub struct IButtonConfig {
    pub show_icon: Option<bool>,
    pub show_label: Option<bool>,
    pub is_secondary: Option<bool>,
    pub custom_label: Option<ButtonLabel>,
    pub custom_label_obs: Option<IObservable<Option<ButtonLabel>>>,
    pub custom_class: Option<String>,
    /// Renders an animated spinner ahead of the label, for a button whose work is currently in flight rather than waiting
    /// to be started.
    ///
    /// The spinner occupies the button's leading slot. Where the button renders an icon at rest (`show_icon` with an
    /// action that carries one) the spinner takes that icon's place rather than adding to it, so the button keeps its
    /// width while its work runs. A button with nothing in that slot at rest grows by the width of the spinner instead.
    pub show_spinner: Option<bool>,
}

/// `IButtonConfigProvider`.
pub type IButtonConfigProvider = Rc<dyn Fn(&ActionRef, usize) -> Option<IButtonConfig>>;

/// `IWorkbenchButtonBarOptions`.
#[derive(Clone, Default)]
pub struct IWorkbenchButtonBarOptions {
    pub telemetry_source: Option<String>,
    pub button_config_provider: Option<IButtonConfigProvider>,
    pub small: bool,
    pub disable_while_running: bool,
    /// `renderSecondaryActions`: sem valor, verdadeiro.
    pub render_secondary_actions: Option<bool>,
    /// As cores dos botões (não está no original; ver o módulo). Sem valor, [`ButtonStyles::default`].
    pub button_styles: Option<ButtonStyles>,
}

/// O botão criado: o simples ou o com menu (o `btn instanceof ButtonWithDropdown` do original).
enum BarButton {
    Plain(Rc<Button>),
    Dropdown(Rc<ButtonWithDropdown>),
}

impl BarButton {
    /// O botão que carrega o rótulo (o `btn.primaryButton` de um botão com menu).
    fn primary(&self) -> &Rc<Button> {
        match self {
            Self::Plain(button) => button,
            Self::Dropdown(button) => &button.primary_button,
        }
    }

    fn button(&self) -> Rc<dyn IButton> {
        match self {
            Self::Plain(button) => button.clone(),
            Self::Dropdown(button) => button.clone(),
        }
    }
}

/// `action.tooltip || action.label`, com o atalho da ação.
fn tooltip_with_keybinding(keybinding_service: &dyn IKeybindingService, action: &ActionRef) -> String {
    let tooltip = Some(action.tooltip()).filter(|tooltip| !tooltip.is_empty()).unwrap_or_else(|| action.label());
    keybinding_service.append_keybinding(&tooltip, Some(&action.id()), None, false)
}

/// `renderActionIcon`: o `span` com as classes da ação, se ela tem classe.
fn render_action_icon(ui: &Ui, action: &ActionRef, foreground: Rgba) -> Option<NodeId> {
    let class = action.class().filter(|class| !class.is_empty())?;
    let mut tree = ui.tree_mut();
    let element = create_detached_el(&mut tree, Role::Generic, &class);
    if let Some(id) = ActionViewItem::codicon_id(&class) {
        tree.style_mut(element).content = je_ui::Content::Codicon { id, size: 16.0, color: foreground };
    }
    Some(element)
}

/// `btn.element.classList.add(...action.class.split(' '))` de um botão só de ícone: o glifo `codicon-*` é o ícone do
/// botão (`set_icon`) e as outras classes entram como estão.
fn add_action_classes(button: &Rc<Button>, class: &str) {
    let mut icon_classes: Vec<String> = Vec::new();
    if let Some(id) = ActionViewItem::codicon_id(class) {
        let icon = ThemeIcon::from_id(id);
        icon_classes = icon.as_class_name_array();
        button.set_icon(&icon);
    }
    let ui = button.ui().clone();
    let mut tree = ui.tree_mut();
    for name in class.split_whitespace().filter(|name| !icon_classes.iter().any(|c| c == name)) {
        tree.add_class(button.element(), name);
    }
}

/// O botão "mais" (e o menu de contexto dele): a âncora é o botão e as ações são as secundárias.
struct MoreActionsMenuDelegate {
    anchor: NodeId,
    actions: Vec<ActionRef>,
    action_runner: Rc<dyn IActionRunner>,
    button: Rc<Button>,
}

impl IContextMenuDelegate for MoreActionsMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Element(node_element(self.anchor))
    }
    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }
    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        Some(self.action_runner.clone())
    }
    fn on_hide(&self, _did_cancel: bool) {
        self.button.set_attribute("aria-expanded", "false");
    }
}

/// O `WorkbenchButtonBar`.
pub struct WorkbenchButtonBar {
    ui: Ui,
    bar: ButtonBar,
    /// `protected readonly _store`.
    store: Rc<DisposableStore>,
    /// `protected readonly _updateStore`.
    update_store: Rc<DisposableStore>,
    action_runner: Rc<ActionRunner>,
    on_did_change: Rc<Emitter<()>>,
    options: Option<IWorkbenchButtonBarOptions>,
    context_menu_service: Rc<dyn IContextMenuService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    hover_service: Rc<dyn IHoverDelegate2>,
    button_context_menu_provider: Rc<dyn IButtonContextMenuProvider>,
    disposed: Cell<bool>,
}

impl WorkbenchButtonBar {
    /// `new WorkbenchButtonBar(container, options, contextMenuService, keybindingService, telemetryService, hoverService)`.
    pub fn new(
        ui: &Ui,
        container: NodeId,
        options: Option<IWorkbenchButtonBarOptions>,
        context_menu_service: Rc<dyn IContextMenuService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        hover_service: Rc<dyn IHoverDelegate2>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let update_store = DisposableStore::new();
        let action_runner = store.add_rc(ActionRunner::new());
        let on_did_change = Emitter::new();
        if let Some(telemetry_source) = options.as_ref().and_then(|o| o.telemetry_source.clone()) {
            store.add(action_runner.on_will_run().listen(move |e: &IRunEvent| {
                telemetry_service.public_log2("workbenchActionExecuted", Some(Dynamic::object([("id", Dynamic::from(e.action.id().as_str())), ("from", Dynamic::from(telemetry_source.as_str()))])));
            }));
        }
        let button_context_menu_provider: Rc<dyn IButtonContextMenuProvider> = ButtonContextMenuServiceProvider::new(context_menu_service.clone());
        Rc::new(Self {
            ui: ui.clone(),
            bar: ButtonBar::new(ui, container, None),
            store,
            update_store,
            action_runner,
            on_did_change,
            options,
            context_menu_service,
            keybinding_service,
            hover_service,
            button_context_menu_provider,
            disposed: Cell::new(false),
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `get onWillRun()`.
    pub fn on_will_run(&self) -> Event<IRunEvent> {
        self.action_runner.on_will_run()
    }

    /// `get onDidRun()`.
    pub fn on_did_run(&self) -> Event<IRunEvent> {
        self.action_runner.on_did_run()
    }

    /// `protected readonly _store`.
    pub fn store(&self) -> &Rc<DisposableStore> {
        &self.store
    }

    fn button_styles(&self) -> ButtonStyles {
        self.options.as_ref().and_then(|o| o.button_styles).unwrap_or_default()
    }

    fn button_options(&self, secondary: bool, aria_label: String) -> ButtonOptions {
        ButtonOptions {
            styles: self.button_styles(),
            title: ButtonTitle::None,
            aria_label: Some(aria_label),
            support_icons: true,
            secondary,
            small: self.options.as_ref().is_some_and(|o| o.small),
            ..ButtonOptions::default()
        }
    }

    /// `update(actions, secondary)`.
    pub fn update(&self, actions: &[ActionRef], secondary: &[ActionRef]) {
        let config_provider: IButtonConfigProvider = self
            .options
            .as_ref()
            .and_then(|o| o.button_config_provider.clone())
            .unwrap_or_else(|| Rc::new(|_: &ActionRef, _: usize| Some(IButtonConfig { show_label: Some(true), ..IButtonConfig::default() })));

        self.update_store.clear();
        self.bar.clear();

        // Support instant hover between buttons
        let hover_delegate: Rc<dyn IHoverDelegate> = {
            let scoped = create_instant_hover_delegate();
            self.update_store.add(scoped.clone());
            scoped
        };

        let action_count = if self.options.as_ref().and_then(|o| o.render_secondary_actions) == Some(false) { actions.len().min(1) } else { actions.len() };
        for (i, action_or_submenu) in actions.iter().take(action_count).enumerate() {
            let secondary_flag = i > 0;
            let submenu = action_or_submenu.as_any().downcast_ref::<SubmenuAction>();
            let action: ActionRef;
            let btn: BarButton;
            let tooltip: String;

            if let Some(submenu) = submenu.filter(|submenu| submenu.actions().len() > 1) {
                let (first, rest) = submenu.actions().split_first().expect("submenu com mais de uma ação");
                action = first.clone();

                tooltip = tooltip_with_keybinding(&*self.keybinding_service, &action);

                let is_secondary = config_provider(&action, i).and_then(|c| c.is_secondary).unwrap_or(secondary_flag);
                btn = BarButton::Dropdown(self.bar.add_button_with_dropdown(ButtonWithDropdownOptions {
                    button: self.button_options(is_secondary, tooltip.clone()),
                    context_menu_provider: self.button_context_menu_provider.clone(),
                    actions: ButtonDropdownActions::Actions(rest.to_vec()),
                    action_runner: Some(self.action_runner.clone()),
                    add_primary_action_to_dropdown: Some(false),
                    dropdown_layer: None,
                }));
            } else {
                action = match submenu.filter(|submenu| submenu.actions().len() == 1) {
                    Some(submenu) => submenu.actions()[0].clone(),
                    None => action_or_submenu.clone(),
                };

                tooltip = tooltip_with_keybinding(&*self.keybinding_service, &action);

                let is_secondary = config_provider(&action, i).and_then(|c| c.is_secondary).unwrap_or(secondary_flag);
                btn = BarButton::Plain(self.bar.add_button(self.button_options(is_secondary, tooltip.clone())));
            }

            let button = btn.button();
            button.set_enabled(action.enabled());
            button.set_checked(action.checked().unwrap_or(false));
            self.ui.tree_mut().add_class(button.element(), "default-colors");

            let config = config_provider(&action, i);
            let show_label = config.as_ref().and_then(|c| c.show_label).unwrap_or(true);
            let show_icon = config.as_ref().and_then(|c| c.show_icon) == Some(true);
            let custom_class = config.as_ref().and_then(|c| c.custom_class.clone());
            let custom_label = config.as_ref().and_then(|c| c.custom_label.clone());
            let custom_label_obs = config.as_ref().and_then(|c| c.custom_label_obs.clone());

            if let Some(custom_class) = custom_class.filter(|class| !class.is_empty()) {
                self.ui.tree_mut().add_class(button.element(), &custom_class);
            }

            // The icon a button shows always comes from the action, which carries it as a CSS class. `MenuItemAction`
            // derives that class from the icon its command declares, including the toggled icon while it is checked,
            // which reading `item.icon` directly would miss.
            //
            // The button's leading slot holds either its icon or, while its work is in flight, the spinner, never both.
            // Both are sized identically (see buttonbar.css), so swapping one for the other leaves the button's width
            // untouched.
            let styles = self.button_styles();
            let foreground = (if is_secondary_style(&btn) { styles.button_secondary_foreground } else { styles.button_foreground }).unwrap_or(INHERITED_BUTTON_FOREGROUND);
            let show_spinner = config.as_ref().and_then(|c| c.show_spinner) == Some(true);
            let leading: Option<NodeId> = if show_spinner {
                let spinner = create_pixel_spinner(&self.ui, None, None);
                spinner.set_color(foreground);
                self.update_store.add_rc(spinner.clone());
                Some(spinner.element())
            } else if show_icon && show_label {
                render_action_icon(&self.ui, &action, foreground)
            } else {
                None
            };
            if let Some(leading) = leading {
                let mut tree = self.ui.tree_mut();
                tree.add_class(leading, "monaco-button-leading-icon");
                if !show_label {
                    // Nothing follows it, so it carries no gap to a label.
                    tree.add_class(leading, "monaco-button-leading-icon-only");
                }
            }

            // Setting the label resets the button's children, so the leading slot is (re-)attached after every label
            // write rather than once up front.
            let apply_leading: Rc<dyn Fn()> = {
                let primary = btn.primary().clone();
                Rc::new(move || {
                    if let Some(leading) = leading {
                        primary.set_leading(Some(leading));
                    }
                })
            };

            let apply_label: Rc<dyn Fn(ButtonLabel)> = {
                let (button, keybinding_service, action_id, apply_leading) = (button.clone(), self.keybinding_service.clone(), action.id(), apply_leading.clone());
                Rc::new(move |label_value: ButtonLabel| {
                    if show_label {
                        button.set_label(label_value.clone());
                    }

                    let plain = match &label_value {
                        ButtonLabel::Text(text) => text.clone(),
                        ButtonLabel::Markdown(markdown) => render_as_plaintext(&markdown.value.to_string_lossy()),
                    };
                    let label_string_value = strip_icons(&plain.encode_utf16().collect::<Vec<_>>()).to_string_lossy();
                    let aria_label_with_keybinding = keybinding_service.append_keybinding(&label_string_value, Some(&action_id), None, false);

                    button.set_title(&aria_label_with_keybinding);
                    button.set_aria_label(&aria_label_with_keybinding);
                    apply_leading();
                })
            };

            if show_label {
                button.set_label(custom_label.clone().unwrap_or_else(|| ButtonLabel::Text(action.label())));
            } else {
                self.ui.tree_mut().add_class(button.element(), "monaco-text-button");
            }
            apply_leading();

            // An icon-only button wears its icon as a codicon class on the button itself, so it is left off while the
            // spinner stands in for it.
            if show_icon && !show_label && !show_spinner {
                let declared_icon = action.as_any().downcast_ref::<MenuItemAction>().and_then(|item| match &item.item.icon {
                    Some(Icon::Theme(icon)) => Some(icon.clone()),
                    _ => None,
                });
                if let Some(icon) = declared_icon {
                    button.set_icon(&icon);
                } else if let Some(class) = action.class().filter(|class| !class.is_empty()) {
                    add_action_classes(btn.primary(), &class);
                }
            }

            if let Some(custom_label_obs) = custom_label_obs {
                let (action, apply_label, custom_label) = (action.clone(), apply_label.clone(), custom_label.clone());
                self.update_store.add(autorun(move |reader: &dyn IReaderWithStore| {
                    let value = custom_label_obs.read(reader);
                    apply_label(value.or_else(|| custom_label.clone()).unwrap_or_else(|| ButtonLabel::Text(action.label())));
                }));
            }

            let hover: Rc<dyn IManagedHover> = self.hover_service.setup_managed_hover(hover_delegate.clone(), button.element(), ManagedHoverContentOrFactory::from(tooltip.as_str()), None);
            let hover: DisposableRef = hover;
            self.update_store.add(hover);

            let (action_runner, disable_while_running) = (self.action_runner.clone(), self.options.as_ref().is_some_and(|o| o.disable_while_running));
            let click_button = button.clone();
            self.update_store.add(button.on_did_click().listen(move |_| {
                if disable_while_running {
                    click_button.set_enabled(false);
                    let (promise, button, action) = (action_runner.run(&action, None), click_button.clone(), action.clone());
                    // `try { await run } finally { btn.enabled = action.enabled; }`
                    let _ = async_fn(async move {
                        let result = promise.await;
                        button.set_enabled(action.enabled());
                        result
                    });
                } else {
                    let _ = action_runner.run(&action, None);
                }
            }));
        }

        if self.options.as_ref().and_then(|o| o.render_secondary_actions) != Some(false) && !secondary.is_empty() {
            let more_actions = localize("moreActions", "More Actions", &[]);
            let btn = self.bar.add_button(self.button_options(true, more_actions.clone()));

            btn.set_icon(&ThemeIcon::from_id("drop-down-button"));
            {
                let mut tree = self.ui.tree_mut();
                tree.add_class(btn.element(), "default-colors");
                tree.add_class(btn.element(), "monaco-text-button");
            }

            btn.set_enabled(true);
            let hover: Rc<dyn IManagedHover> = self.hover_service.setup_managed_hover(hover_delegate.clone(), btn.element(), ManagedHoverContentOrFactory::from(more_actions.as_str()), None);
            let hover: DisposableRef = hover;
            self.update_store.add(hover);
            let (context_menu_service, action_runner, secondary, click_button) = (self.context_menu_service.clone(), self.action_runner.clone(), secondary.to_vec(), btn.clone());
            self.update_store.add(btn.on_did_click().listen(move |_| {
                context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(MoreActionsMenuDelegate {
                    anchor: click_button.element(),
                    actions: secondary.clone(),
                    action_runner: action_runner.clone(),
                    button: click_button.clone(),
                })));
                click_button.set_attribute("aria-expanded", "true");
            }));
        }
        self.on_did_change.fire(());
    }
}

/// O botão foi criado como secundário (`secondary` do `IButtonOptions`)? Lido do `secondary` da classe.
fn is_secondary_style(btn: &BarButton) -> bool {
    let primary = btn.primary();
    primary.ui().tree().has_class(primary.element(), "secondary")
}

impl Deref for WorkbenchButtonBar {
    type Target = ButtonBar;

    fn deref(&self) -> &ButtonBar {
        &self.bar
    }
}

impl IDisposable for WorkbenchButtonBar {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.on_did_change.dispose();
        self.update_store.dispose();
        self.store.dispose();
        self.bar.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "WorkbenchButtonBar"
    }
}

/// `IMenuWorkbenchButtonBarOptions`.
#[derive(Clone, Default)]
pub struct IMenuWorkbenchButtonBarOptions {
    pub bar: IWorkbenchButtonBarOptions,
    pub menu_options: Option<MenuActionOptions>,
    pub toolbar_options: Option<IToolBarRenderOptions>,
}

/// `MenuWorkbenchButtonBar`: uma barra de botões movida por um menu.
pub struct MenuWorkbenchButtonBar {
    inner: Rc<WorkbenchButtonBar>,
}

impl MenuWorkbenchButtonBar {
    /// `new MenuWorkbenchButtonBar(container, menuId, options, menuService, contextKeyService, contextMenuService,
    /// keybindingService, telemetryService, hoverService)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        container: NodeId,
        menu_id: &MenuId,
        options: Option<IMenuWorkbenchButtonBarOptions>,
        menu_service: Rc<dyn IMenuService>,
        context_key_service: Rc<dyn IContextKeyService>,
        context_menu_service: Rc<dyn IContextMenuService>,
        keybinding_service: Rc<dyn IKeybindingService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        hover_service: Rc<dyn IHoverDelegate2>,
    ) -> Rc<Self> {
        let inner = WorkbenchButtonBar::new(ui, container, options.as_ref().map(|o| o.bar.clone()), context_menu_service, keybinding_service, telemetry_service, hover_service);

        let menu: Rc<dyn IMenu> = menu_service.create_menu(menu_id, context_key_service, None);
        inner.store.add(menu.clone());

        let menu_options = options.as_ref().and_then(|o| o.menu_options.clone());
        let toolbar_options = options.and_then(|o| o.toolbar_options);
        let update = {
            let (inner, menu) = (Rc::downgrade(&inner), menu.clone());
            Rc::new(move || {
                let Some(inner) = inner.upgrade() else { return };
                inner.bar.clear();

                let is_primary_action = PrimaryGroup::predicate(toolbar_options.as_ref().and_then(|o| o.primary_group.as_ref()));
                let use_separators = toolbar_options.as_ref().is_some_and(|o| o.use_separators_in_primary_actions);
                let actions = get_action_bar_actions(&menu.get_actions(menu_options.as_ref()), &*is_primary_action, use_separators);

                inner.update(&actions.primary, &actions.secondary);
            })
        };
        {
            let update = update.clone();
            inner.store.add(menu.on_did_change().listen(move |_| update()));
        }
        update();
        Rc::new(Self { inner })
    }

    pub fn on_did_change(&self) -> Event<()> {
        self.inner.on_did_change()
    }

    pub fn on_will_run(&self) -> Event<IRunEvent> {
        self.inner.on_will_run()
    }

    pub fn on_did_run(&self) -> Event<IRunEvent> {
        self.inner.on_did_run()
    }

    pub fn buttons(&self) -> Vec<Rc<dyn IButton>> {
        self.inner.buttons()
    }

    /// `override update(_actions)`: lança, a barra é alimentada pelo menu.
    pub fn update(&self, _actions: &[ActionRef]) {
        je_base::errors::throw(je_base::errors::Error::new("Use Menu or WorkbenchButtonBar"))
    }
}

impl IDisposable for MenuWorkbenchButtonBar {
    fn dispose(&self) {
        self.inner.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MenuWorkbenchButtonBar"
    }
}
