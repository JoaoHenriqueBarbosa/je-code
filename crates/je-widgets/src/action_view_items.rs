//! Porte de `src/vs/base/browser/ui/actionbar/actionViewItems.ts` (e do `actionbar.css`): o item de uma
//! [`ActionBar`](crate::action_bar::ActionBar), isto é, o botão (ícone e/ou rótulo) de uma ação.
//!
//! # API para quem consome (toolbar, dropdown, menu, dialog, inputbox, findinput)
//!
//! - [`IActionViewItem`] é a interface do original (`action`, `actionRunner`, `setActionContext`, `render`,
//!   `isEnabled`, `focus`, `blur`, `showHover`) mais o que a `ActionBar` pede dos itens que herdam de
//!   `BaseActionViewItem` (`setFocusable`, `isFocused`, `trapsArrowNavigation`, `getActionToRun`) e os ganchos
//!   protegidos que as subclasses sobrescrevem (`updateEnabled`, `updateLabel`, `updateClass`,
//!   `updateChecked`, `getClass`, `getTooltip`, `getHoverContents`, `getHoverOptions`, `updateAriaLabel`).
//!   Tudo tem implementação padrão em cima de [`BaseActionViewItem`], então um item novo só implementa
//!   [`IActionViewItem::base`] (devolve o `BaseActionViewItem` embutido), `IDisposable` (ver
//!   [`impl_action_view_item_dispose`]) e sobrescreve o que a subclasse do TypeScript sobrescreveria.
//! - **Herança vira composição.** Uma subclasse guarda um [`BaseActionViewItem`] e o constrói com
//!   [`BaseActionViewItem::new_derived`], passando o `Weak` de si mesma (`Rc::new_cyclic`): é por ele que o
//!   base chama os ganchos virtuais da subclasse. O que a subclasse chamaria com `super.x()` é
//!   `self.base.base_x()` (`base_render`, `base_focus`, `base_blur`, `base_set_focusable`, `base_is_focused`,
//!   `update_tooltip`, `base_dispose`). Todo item do Rust é "um `BaseActionViewItem`": o `instanceof
//!   BaseActionViewItem` da `ActionBar` é sempre verdadeiro (um item que não herda do base no TypeScript
//!   embute um base que nunca recebe `render`).
//! - [`IActionViewItemOptions`] reúne `IBaseActionViewItemOptions` e `IActionViewItemOptions` (o segundo
//!   estende o primeiro) numa estrutura só; [`IBaseActionViewItemOptions`] é um alias dela. Os estilos (as
//!   regras de CSS que o original deixa na folha de estilo) entram por `styles` ([`ActionBarStyles`]).
//! - **Contexto** (`unknown`): `Option<Dynamic>`, `None` é `null`/`undefined`. O `context || this` do construtor
//!   do original (o próprio item como contexto) não existe: sem contexto o item roda a ação com o contexto
//!   padrão do clique (`{ preserveFocus }`, ou o evento se `use_event_as_context`).
//! - **Tamanho.** Não há motor de layout: a `ActionBar` pergunta o tamanho natural de cada item
//!   ([`IActionViewItem::size`], o que o CSS (`display: flex`, `padding: 3px`, codicon de 16px) daria) e
//!   posiciona o `li`. O item avisa a barra de que o tamanho mudou com [`BaseActionViewItem::set_size`].
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - `li.action-item` é o nó do `render(container)`; dentro dele, `a.action-label` (nó com a classe
//!   `action-label`, papel `Button`) e, com teclado e rótulo, `span.keybinding`. As classes (`disabled`,
//!   `checked`, `active`, `focused`, `codicon`, `separator`, as classes da ação) são as do DOM, lidas com
//!   `tree.has_class`.
//! - `:hover`, `:active`, `:focus` e `.checked` (das regras de `actionbar.css` e do `style.css` do workbench)
//!   viram [`ActionViewItem::refresh_style`], chamada nos ouvintes de ponteiro e foco e nas trocas de estado.
//!   O `outline` é uma borda de 1px por dentro (`outline-offset: -1px`); o `outline: 1px dashed` do hover é
//!   sólido (o `je-ui` não tem borda tracejada). Texto de ícone e opacidade: `.action-label.icon` desabilitado
//!   vira opacidade 0,6; o codicon desabilitado muda de cor.
//! - `tabIndex` é o campo [`ActionViewItem::label_tab_index`] (o `je-ui` não tem ordem de tabulação); o
//!   foco é o do `Ui`.
//!
//! Fica de fora: `Gesture`/`Tap` (toque), `draggable` (arrastar e soltar), o `contextmenu` com Ctrl do macOS
//! (o `je-ui` não tem evento de menu de contexto) e o `aria-pressed`/`aria-selected`/`aria-disabled` (o `je-ui`
//! só tem `role` e `label`; o `role` do rótulo é o do original).
//!
//! # `SelectActionViewItem`
//!
//! [`SelectActionViewItem`] guarda um [`SelectBox`](crate::select_box::SelectBox) e roda a ação com o texto da
//! opção escolhida (ou o que [`SelectActionViewItem::set_action_context_factory`] devolver, o `getActionContext`
//! que as subclasses sobrescrevem). Como no original o `render` não chama o do base: `element` fica vazio e o
//! item não tem dica nem `aria-label` próprios. O tamanho natural é o do `<select>` (`min-width: 100px` e
//! `padding: 2px 23px 2px 8px` do `selectBox.css`, ver `SelectBoxList::natural_size`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, AltModeModifiers, IActionChangeEvent, IActionRunner, Separator, get_action_for_alt_mode};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::ColorTheme;
use je_ui::{Content, Event, EventKind, NodeId, NodeStyle, PointerButton, Role, Ui};

use crate::context_view_api::IContextViewProvider;
use crate::dom::create_el;
use crate::hover_api::{HoverPlacement, IHoverDelegate, IManagedHover, IManagedHoverOptions, ManagedHoverContent, ManagedHoverContentOrFactory, get_base_layer_hover_delegate, get_default_hover_delegate};
use crate::select_box::{ISelectBoxDelegate, ISelectBoxOptions, ISelectBoxStyles, ISelectOptionItem, SelectBox};
use crate::text::{WORKBENCH_LINE_HEIGHT, measure_text, ui_font};

/// O `--vscode-cornerRadius-medium` (`baseSizes.ts`: 6px em todos os temas).
pub const CORNER_RADIUS_MEDIUM: f64 = 6.0;
/// O `font-size` de `.action-label` e `.keybinding`.
pub const ACTION_LABEL_FONT_SIZE: f32 = 11.0;
/// O `padding` de `.action-label` e `.keybinding`.
pub const ACTION_LABEL_PADDING: f64 = 3.0;
/// A altura do separador de um menu (`menu.ts`): `margin: 5px 0` mais a linha de 1px.
pub const MENU_SEPARATOR_HEIGHT: f64 = 11.0;
/// A largura e a altura do `.codicon` dentro de um item.
pub const ACTION_ICON_SIZE: f64 = 16.0;

/// As cores e medidas que o CSS do action bar lê de variáveis do tema (`actionbar.css`, mais as regras
/// `.monaco-workbench .monaco-action-bar` do `style.css` e `.highlight-toggled` do `actionWidget.css`). Cor
/// `None` é a variável indefinida (a regra não pinta).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionBarStyles {
    /// A cor do texto herdada do container (`foreground`).
    pub foreground: Option<Rgba>,
    /// `--vscode-disabledForeground`: rótulo desabilitado e separador horizontal.
    pub disabled_foreground: Option<Rgba>,
    /// `--vscode-menu-separatorBackground` (o separador vertical, com `disabledForeground` de reserva).
    pub menu_separator_background: Option<Rgba>,
    /// `--vscode-toolbar-hoverBackground`.
    pub toolbar_hover_background: Option<Rgba>,
    /// `--vscode-toolbar-hoverOutline`.
    pub toolbar_hover_outline: Option<Rgba>,
    /// `--vscode-toolbar-activeBackground`.
    pub toolbar_active_background: Option<Rgba>,
    /// `--vscode-focusBorder`.
    pub focus_border: Option<Rgba>,
    /// `--vscode-actionBar-toggledBackground` (só com `highlight_toggled_items`).
    pub toggled_background: Option<Rgba>,
    /// `--vscode-cornerRadius-medium`.
    pub corner_radius: f64,
}

impl Default for ActionBarStyles {
    /// Sem tema: nenhuma cor, o raio padrão.
    fn default() -> Self {
        Self { foreground: None, disabled_foreground: None, menu_separator_background: None, toolbar_hover_background: None, toolbar_hover_outline: None, toolbar_active_background: None, focus_border: None, toggled_background: None, corner_radius: CORNER_RADIUS_MEDIUM }
    }
}

/// Os estilos do action bar com as cores resolvidas no tema.
pub fn default_action_bar_styles(theme: &ColorTheme) -> ActionBarStyles {
    ActionBarStyles {
        foreground: theme.get_color("foreground"),
        disabled_foreground: theme.get_color("disabledForeground"),
        menu_separator_background: theme.get_color("menu.separatorBackground"),
        toolbar_hover_background: theme.get_color("toolbar.hoverBackground"),
        toolbar_hover_outline: theme.get_color("toolbar.hoverOutline"),
        toolbar_active_background: theme.get_color("toolbar.activeBackground"),
        focus_border: theme.get_color("focusBorder"),
        toggled_background: theme.get_color("actionBar.toggledBackground"),
        corner_radius: CORNER_RADIUS_MEDIUM,
    }
}

/// O estado que as regras de CSS do rótulo de ação (`.action-label`) consultam: `:hover`, `:active`, `:focus`,
/// `.disabled`, `.checked` num `.highlight-toggled`, o separador e a barra vertical.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LabelState {
    pub separator: bool,
    pub vertical: bool,
    pub disabled: bool,
    pub hovered: bool,
    pub active: bool,
    pub highlight_toggled_checked: bool,
    pub focused: bool,
    /// O item é de um `Menu` (`isMenu`): o separador vertical dele é o do `menu.css`, não o do `actionbar.css`.
    pub menu: bool,
}

impl ActionBarStyles {
    /// Aplica ao estilo do rótulo o fundo, a borda, o raio, a opacidade, o cursor e a cor do codicon que as
    /// regras de CSS dariam no estado, e devolve a cor do texto. Compartilhada pelo [`ActionViewItem`] e pelo
    /// rótulo de ícone do `DropdownMenuActionViewItem`.
    pub(crate) fn apply_to_label(&self, style: &mut NodeStyle, state: LabelState, has_icon_class: bool) -> Option<Rgba> {
        let mut background = None;
        let mut border = None;
        // O separador de uma barra vertical comum é uma caixa de 2px (`padding-top: 1px` mais `border-bottom: 1px`,
        // altura 0): só a linha de baixo pinta. O do menu é a caixa de 1px com o fundo.
        let ruled_separator = state.separator && state.vertical && !state.menu;
        style.border_widths = None;
        if ruled_separator {
            border = self.menu_separator_background.or(self.disabled_foreground);
            style.border_widths = Some([0.0, 0.0, 1.0, 0.0]);
        } else if state.separator {
            background = if state.vertical { self.menu_separator_background.or(self.disabled_foreground) } else { self.disabled_foreground };
        } else {
            if state.hovered && !state.disabled && !state.vertical {
                background = self.toolbar_hover_background;
                border = self.toolbar_hover_outline;
            }
            if state.active && !state.disabled && !state.vertical {
                background = self.toolbar_active_background;
            }
            if state.highlight_toggled_checked {
                background = self.toggled_background;
            }
            if border.is_none() && state.focused {
                border = self.focus_border;
            }
        }
        let color = if state.disabled { self.disabled_foreground.or(self.foreground) } else { self.foreground };
        style.background = background;
        style.border = border.map(|color| je_ui::Border { width: 1.0, color });
        style.radius = if state.separator { 0.0 } else { self.corner_radius };
        style.opacity = if state.disabled && has_icon_class { 0.6 } else { 1.0 };
        style.cursor = Some(if state.disabled || state.separator { je_ui::Cursor::Default } else { je_ui::Cursor::Pointer });
        if let (Some(color), Content::Codicon { color: icon_color, .. }) = (color, &mut style.content) {
            *icon_color = color;
        }
        color
    }
}

/// `IBaseActionViewItemOptions` e `IActionViewItemOptions` juntos (o segundo estende o primeiro).
#[derive(Clone, Default)]
pub struct IActionViewItemOptions {
    pub draggable: bool,
    pub is_menu: bool,
    pub is_tab_list: bool,
    pub use_event_as_context: bool,
    pub hover_delegate: Option<Rc<dyn IHoverDelegate>>,
    /// `icon`: sem valor, falso.
    pub icon: Option<bool>,
    /// `label`: sem valor, verdadeiro.
    pub label: Option<bool>,
    /// `keybinding`: o rótulo do atalho (`string | null`).
    pub keybinding: Option<String>,
    pub keybinding_not_rendered_with_label: bool,
    /// As cores do CSS (não está no original). Sem valor, [`ActionBarStyles::default`].
    pub styles: Option<Rc<ActionBarStyles>>,
}

/// `IBaseActionViewItemOptions`.
pub type IBaseActionViewItemOptions = IActionViewItemOptions;

/// O tamanho natural de um item: `(largura, altura)`.
pub type ItemSize = (f64, f64);

/// `IActionViewItem` e a parte de `BaseActionViewItem` que as subclasses estendem. Ver o doc do módulo.
pub trait IActionViewItem: IDisposable {
    /// O `BaseActionViewItem` embutido. Para o próprio [`BaseActionViewItem`], ele mesmo.
    fn base(&self) -> &BaseActionViewItem;

    fn action(&self) -> ActionRef {
        self.base().action()
    }
    fn action_runner(&self) -> Rc<dyn IActionRunner> {
        self.base().action_runner()
    }
    fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        self.base().set_action_runner(action_runner);
    }
    fn set_action_context(&self, context: Option<Dynamic>) {
        self.base().set_action_context(context);
    }
    /// `render(element)`: monta o item dentro do `li` (ou do container que a barra criou).
    fn render(&self, container: NodeId) {
        self.base().base_render(container);
    }
    fn is_enabled(&self) -> bool {
        self.action().enabled()
    }
    /// `focus(fromRight?)`.
    fn focus(&self, _from_right: bool) {
        self.base().base_focus();
    }
    fn blur(&self) {
        self.base().base_blur();
    }
    fn show_hover(&self) {}

    fn is_focused(&self) -> bool {
        self.base().base_is_focused()
    }
    fn set_focusable(&self, focusable: bool) {
        self.base().base_set_focusable(focusable);
    }
    fn traps_arrow_navigation(&self) -> bool {
        false
    }
    /// Quantos nós do item são parada de Tab (`tabIndex === 0`): o nó do item (`BaseActionViewItem`), o
    /// rótulo (`ActionViewItem`), ou a soma dos filhos de um item composto. O `querySelectorAll('[tabindex="0"]')`
    /// do original, que o `je-ui` não tem (o `tabIndex` vive no item).
    fn tab_stops(&self) -> usize {
        usize::from(self.base().tab_index() == 0)
    }
    /// `descendant.tabIndex = 0; descendant.focus()` para um nó focável de um item composto (o botão de menu
    /// de um `ActionWithDropdownActionViewItem`). Devolve falso se o nó não é de um filho do item.
    fn focus_descendant(&self, _node: NodeId) -> bool {
        false
    }
    /// `getActionToRun(event)`: a ação alternativa com Alt (ou Shift fora do macOS) nos menus.
    fn get_action_to_run(&self, modifiers: AltModeModifiers) -> ActionRef {
        let base = self.base();
        if base.options.is_menu { get_action_for_alt_mode(&base.action(), modifiers, &host_platform()) } else { base.action() }
    }

    /// O tamanho natural do item (o que o `li` mediria no CSS) na orientação da barra. Sem sobrescrever, o
    /// último tamanho dado a [`BaseActionViewItem::set_size`].
    fn size(&self, _vertical: bool) -> ItemSize {
        self.base().size.get()
    }
    /// A barra posicionou o `li` com `width` x `height`: o item posiciona o que tem dentro.
    fn layout(&self, _vertical: bool, _width: f64, _height: f64) {}

    // Ganchos protegidos (`protected` no original).
    fn update_enabled(&self) {}
    fn update_label(&self) {}
    fn update_class(&self) {}
    fn update_checked(&self) {}
    fn get_class(&self) -> Option<String> {
        self.base().action().class()
    }
    fn get_tooltip(&self) -> Option<String> {
        Some(self.base().action().tooltip())
    }
    fn get_hover_contents(&self) -> Option<ManagedHoverContent> {
        self.get_tooltip().map(ManagedHoverContent::Text)
    }
    fn get_hover_options(&self) -> Option<IManagedHoverOptions> {
        None
    }
    fn update_aria_label(&self) {
        let base = self.base();
        if let Some(element) = base.element() {
            base.ui.tree_mut().set_label(element, self.get_tooltip().unwrap_or_default());
        }
    }
}

/// Implementa `IDisposable` para um item que guarda o base no campo `$field`: o `dispose()` do base mais o
/// que `$extra` fizer antes (`|this| ...`).
#[macro_export]
macro_rules! impl_action_view_item_dispose {
    ($type:ty, $field:ident) => {
        impl je_base::lifecycle::IDisposable for $type {
            fn dispose(&self) {
                self.$field.base_dispose();
            }
        }
    };
}

thread_local! {
    static HOST_PLATFORM: Platform = Platform::detect();
}

/// A plataforma que o `getActionForAltMode` consulta (`isWindows`, `isLinux`).
pub(crate) fn host_platform() -> Platform {
    HOST_PLATFORM.with(Clone::clone)
}

/// O contexto de um evento de ponteiro como objeto (`{ shiftKey, ctrlKey, altKey, metaKey }`: o
/// `IContextMenuEvent`), o que o original passa quando usa o evento como contexto da ação.
pub fn event_as_context(event: &Event) -> Dynamic {
    let m = event.pointer.map(|p| p.modifiers).or_else(|| event.key.map(|k| k.modifiers)).unwrap_or_default();
    Dynamic::Object(ObjectRef::from_entries([("shiftKey", Dynamic::Bool(m.shift)), ("ctrlKey", Dynamic::Bool(m.ctrl)), ("altKey", Dynamic::Bool(m.alt)), ("metaKey", Dynamic::Bool(m.meta))]))
}

/// `BaseActionViewItem`: o estado e o comportamento comuns de todo item (ação, contexto, runner, eventos de
/// clique, foco, dica). Usável sozinho (é um [`IActionViewItem`]) e embutido pelas subclasses.
pub struct BaseActionViewItem {
    base: Rc<Disposable>,
    pub(crate) ui: Ui,
    owner: Weak<dyn IActionViewItem>,
    element: Cell<Option<NodeId>>,
    context: RefCell<Option<Dynamic>>,
    action: ActionRef,
    action_runner: RefCell<Option<Rc<dyn IActionRunner>>>,
    custom_hover: RefCell<Option<Rc<dyn IManagedHover>>>,
    /// As opções (`protected readonly options`).
    pub options: IActionViewItemOptions,
    size: Cell<ItemSize>,
    layout_requester: RefCell<Option<Rc<dyn Fn()>>>,
    tab_index: Cell<i32>,
    disposed: Cell<bool>,
}

impl BaseActionViewItem {
    /// `new BaseActionViewItem(context, action, options)`.
    pub fn new(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionViewItemOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self::new_derived(ui, context, action, options, weak.clone() as Weak<dyn IActionViewItem>))
    }

    /// O construtor para subclasses: `owner` é o `Weak` do item completo (`Rc::new_cyclic`).
    pub fn new_derived(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionViewItemOptions, owner: Weak<dyn IActionViewItem>) -> Self {
        let this = Self {
            base: Disposable::new(),
            ui: ui.clone(),
            owner,
            element: Cell::new(None),
            context: RefCell::new(context),
            action,
            action_runner: RefCell::new(None),
            custom_hover: RefCell::new(None),
            options,
            size: Cell::new((0.0, 0.0)),
            layout_requester: RefCell::new(None),
            tab_index: Cell::new(-1),
            disposed: Cell::new(false),
        };
        if let Some(action) = this.action.as_base_action() {
            let owner = this.owner.clone();
            this.base.register(action.on_did_change().listen(move |event| {
                if let Some(owner) = owner.upgrade() {
                    // Ainda não renderizado: não há UI para atualizar.
                    if owner.base().element().is_some() {
                        Self::handle_action_change_event(&*owner, event);
                    }
                }
            }));
        }
        this
    }

    fn handle_action_change_event(owner: &dyn IActionViewItem, event: &IActionChangeEvent) {
        if event.enabled.is_some() {
            owner.update_enabled();
        }
        if event.checked.is_some() {
            owner.update_checked();
        }
        if event.class.is_some() {
            owner.update_class();
        }
        if event.label.is_some() {
            owner.update_label();
            owner.base().update_tooltip();
        }
        if event.tooltip.is_some() {
            owner.base().update_tooltip();
        }
    }

    /// O `register(...)` do `Disposable` do base (para a subclasse registrar o que ela cria).
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.base.register(disposable)
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// O item completo (a subclasse), para os ganchos virtuais.
    fn owner(&self) -> Option<Rc<dyn IActionViewItem>> {
        self.owner.upgrade()
    }

    /// `element`: o container recebido em `render` (o `li`).
    pub fn element(&self) -> Option<NodeId> {
        self.element.get()
    }

    /// `this.element = ...` de uma subclasse que não chama o `render` do base e guarda ali outro nó (o
    /// `DropdownMenuActionViewItem` guarda o rótulo `a.action-label`, não o `li`).
    pub fn set_element(&self, element: NodeId) {
        self.element.set(Some(element));
    }

    pub fn action(&self) -> ActionRef {
        self.action.clone()
    }

    /// `_context`.
    pub fn context(&self) -> Option<Dynamic> {
        self.context.borrow().clone()
    }

    pub fn set_action_context(&self, context: Option<Dynamic>) {
        *self.context.borrow_mut() = context;
    }

    /// `get actionRunner`: sem runner dado, cria um `ActionRunner` e o descarta junto com o item.
    pub fn action_runner(&self) -> Rc<dyn IActionRunner> {
        if let Some(runner) = self.action_runner.borrow().clone() {
            return runner;
        }
        let runner = ActionRunner::new();
        self.base.register(runner.clone());
        let runner: Rc<dyn IActionRunner> = runner;
        *self.action_runner.borrow_mut() = Some(runner.clone());
        runner
    }

    pub fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        *self.action_runner.borrow_mut() = Some(action_runner);
    }

    /// `tabIndex` do elemento (0 depois de `focus`/`setFocusable(true)`, -1 antes).
    pub fn tab_index(&self) -> i32 {
        self.tab_index.get()
    }

    /// Define o tamanho natural do item e pede à barra para posicioná-lo de novo.
    pub fn set_size(&self, width: f64, height: f64) {
        if self.size.get() != (width, height) {
            self.size.set((width, height));
            self.request_layout();
        }
    }

    /// A barra registra aqui o que refaz o layout dela.
    pub fn set_layout_requester(&self, requester: Option<Rc<dyn Fn()>>) {
        *self.layout_requester.borrow_mut() = requester;
    }

    /// Pede à barra que dona do item posicione os itens de novo.
    pub fn request_layout(&self) {
        let requester = self.layout_requester.borrow().clone();
        if let Some(requester) = requester {
            requester();
        }
    }

    /// `render(container)` do base: os ouvintes de ponteiro do `li`.
    pub fn base_render(&self, container: NodeId) {
        self.element.set(Some(container));
        let enable_dragging = self.options.draggable;
        let ui = &self.ui;

        let action = self.action.clone();
        self.base.register(ui.listen(container, EventKind::PointerDown, false, {
            let ui = ui.clone();
            move |e| {
                if !enable_dragging {
                    e.stop(true); // não roda ao arrastar, porque isso desligaria o arraste
                }
                if action.enabled() && e.pointer.is_some_and(|p| p.button == PointerButton::Primary) {
                    ui.tree_mut().add_class(container, "active");
                }
            }
        }));

        let owner = self.owner.clone();
        self.base.register(ui.listen(container, EventKind::Click, false, move |e| {
            e.stop(true);
            // os menus não usam o clique
            if let Some(owner) = owner.upgrade()
                && !owner.base().options.is_menu
            {
                owner.base().on_click(e, false);
            }
        }));
        self.base.register(ui.listen(container, EventKind::DoubleClick, false, |e| e.stop(true)));
        for kind in [EventKind::PointerUp, EventKind::PointerLeave] {
            let ui = ui.clone();
            self.base.register(ui.clone().listen(container, kind, false, move |e| {
                e.stop(false);
                ui.tree_mut().remove_class(container, "active");
            }));
        }
    }

    /// `onClick(event, preserveFocus = false)`: roda a ação (a alternativa, num menu com Alt) com o contexto.
    pub fn on_click(&self, event: &mut Event, preserve_focus: bool) {
        event.stop(true);
        let context = self.context().unwrap_or_else(|| {
            if self.options.use_event_as_context { event_as_context(event) } else { Dynamic::Object(ObjectRef::from_entries([("preserveFocus", Dynamic::Bool(preserve_focus))])) }
        });
        let modifiers = event.pointer.map(|p| AltModeModifiers { alt_key: p.modifiers.alt, shift_key: p.modifiers.shift }).unwrap_or_default();
        let action = self.owner().map_or_else(|| self.action(), |owner| owner.get_action_to_run(modifiers));
        // O `run` do runner devolve a promessa e o original a ignora.
        let _ = self.action_runner().run(&action, Some(context));
    }

    /// `focus()` do base: o elemento ganha `tabIndex` 0, o foco e a classe `focused`.
    pub fn base_focus(&self) {
        if let Some(element) = self.element() {
            self.tab_index.set(0);
            self.ui.focus(element);
            self.ui.tree_mut().add_class(element, "focused");
        }
    }

    /// `isFocused()` do base.
    pub fn base_is_focused(&self) -> bool {
        self.element().is_some_and(|e| self.ui.tree().has_class(e, "focused"))
    }

    /// `blur()` do base.
    pub fn base_blur(&self) {
        if let Some(element) = self.element() {
            if self.ui.focused() == Some(element) {
                self.ui.blur();
            }
            self.tab_index.set(-1);
            self.ui.tree_mut().remove_class(element, "focused");
        }
    }

    /// `setFocusable(focusable)` do base.
    pub fn base_set_focusable(&self, focusable: bool) {
        if self.element().is_some() {
            self.tab_index.set(if focusable { 0 } else { -1 });
        }
    }

    /// `updateTooltip()`: o texto de acessibilidade e a dica gerenciada (criada na primeira dica não vazia).
    pub fn update_tooltip(&self) {
        let Some(element) = self.element() else { return };
        let Some(owner) = self.owner() else { return };
        let title = owner.get_hover_contents().unwrap_or_else(|| ManagedHoverContent::Text(String::new()));
        owner.update_aria_label();
        let is_empty = matches!(&title, ManagedHoverContent::Text(text) if text.is_empty());
        let existing = self.custom_hover.borrow().clone();
        match existing {
            None if !is_empty => {
                let delegate = self.options.hover_delegate.clone().unwrap_or_else(|| get_default_hover_delegate(HoverPlacement::Element));
                let hover = get_base_layer_hover_delegate().setup_managed_hover(delegate, element, ManagedHoverContentOrFactory::Content(title), owner.get_hover_options());
                let disposable: DisposableRef = hover.clone();
                self.base.register(disposable);
                *self.custom_hover.borrow_mut() = Some(hover);
            }
            Some(hover) => hover.update(ManagedHoverContentOrFactory::Content(title), owner.get_hover_options()),
            None => {}
        }
    }

    /// `dispose()` do base: tira o elemento da árvore e solta o contexto.
    pub fn base_dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some(element) = self.element.take() {
            self.ui.remove(element);
        }
        *self.context.borrow_mut() = None;
        self.base.dispose();
    }
}

impl IActionViewItem for BaseActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self
    }
}

impl IDisposable for BaseActionViewItem {
    fn dispose(&self) {
        self.base_dispose();
    }
}

/// `ActionViewItem`: o item padrão, um rótulo (`a.action-label`) com ícone codicon e/ou texto.
pub struct ActionViewItem {
    base: BaseActionViewItem,
    weak_self: Weak<ActionViewItem>,
    icon: bool,
    label_enabled: bool,
    label: Cell<Option<NodeId>>,
    text: Cell<Option<NodeId>>,
    keybinding_node: Cell<Option<NodeId>>,
    keybinding_text: Cell<Option<NodeId>>,
    css_class: RefCell<String>,
    label_tab_index: Cell<i32>,
    hovered: Cell<bool>,
    vertical: Cell<bool>,
    styles: Rc<ActionBarStyles>,
    /// Tamanho natural horizontal: `(rótulo, atalho)`.
    metrics: Cell<LabelMetrics>,
}

#[derive(Clone, Copy, Default)]
struct LabelMetrics {
    label: ItemSize,
    keybinding: ItemSize,
}

impl ActionViewItem {
    /// `new ActionViewItem(context, action, options)`.
    pub fn new(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionViewItemOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self::construct(ui, context, action, options, weak, weak.clone() as Weak<dyn IActionViewItem>))
    }

    /// O construtor para subclasses por composição (`ActionWithDropdownActionViewItem`): `owner` é o `Weak` do
    /// item completo, a quem o base chama os ganchos virtuais (`update_enabled`, `get_tooltip`...).
    pub fn new_with_owner(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionViewItemOptions, owner: Weak<dyn IActionViewItem>) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self::construct(ui, context, action, options, weak, owner))
    }

    fn construct(ui: &Ui, context: Option<Dynamic>, action: ActionRef, options: IActionViewItemOptions, weak: &Weak<Self>, owner: Weak<dyn IActionViewItem>) -> Self {
        let (icon, label_enabled) = (options.icon.unwrap_or(false), options.label.unwrap_or(true));
        let styles = options.styles.clone().unwrap_or_default();
        Self {
            base: BaseActionViewItem::new_derived(ui, context, action, options, owner),
            weak_self: weak.clone(),
            icon,
            label_enabled,
            label: Cell::new(None),
            text: Cell::new(None),
            keybinding_node: Cell::new(None),
            keybinding_text: Cell::new(None),
            css_class: RefCell::new(String::new()),
            label_tab_index: Cell::new(-1),
            hovered: Cell::new(false),
            vertical: Cell::new(false),
            styles,
            metrics: Cell::new(LabelMetrics::default()),
        }
    }

    /// O nó `a.action-label`.
    pub fn label(&self) -> Option<NodeId> {
        self.label.get()
    }

    /// O nó `span.keybinding`.
    pub fn keybinding_node(&self) -> Option<NodeId> {
        self.keybinding_node.get()
    }

    /// `label.tabIndex`.
    pub fn label_tab_index(&self) -> i32 {
        self.label_tab_index.get()
    }

    fn is_separator(&self) -> bool {
        self.base.action.id() == Separator::ID
    }

    fn default_aria_role(&self) -> Role {
        if self.is_separator() {
            Role::Presentation
        } else if self.base.options.is_menu {
            Role::MenuItem
        } else if self.base.options.is_tab_list {
            Role::Tab
        } else {
            Role::Button
        }
    }

    fn is_disabled(&self) -> bool {
        !self.base.action.enabled()
    }

    /// O id do codicon da classe da ação (`codicon codicon-add` vira `add`).
    pub fn codicon_id(class: &str) -> Option<String> {
        class.split_whitespace().find_map(|c| c.strip_prefix("codicon-")).filter(|id| !id.starts_with("modifier-")).map(str::to_owned)
    }

    /// Recalcula o tamanho natural (o que o CSS mediria) e avisa a barra.
    fn relayout(&self) {
        let font = ui_font(ACTION_LABEL_FONT_SIZE, 400);
        let line_height = WORKBENCH_LINE_HEIGHT;
        let pad = 2.0 * ACTION_LABEL_PADDING;
        let label = if self.is_separator() {
            (1.0 + 8.0, ACTION_ICON_SIZE + 10.0)
        } else if self.icon {
            (ACTION_ICON_SIZE + pad, ACTION_ICON_SIZE + pad)
        } else if self.label_enabled {
            (measure_text(&self.base.action.label(), &font) + pad, line_height + pad)
        } else {
            (pad, line_height + pad)
        };
        let keybinding = match (&self.base.options.keybinding, self.keybinding_node.get()) {
            (Some(text), Some(_)) => (measure_text(text, &font) + pad, line_height + pad),
            _ => (0.0, 0.0),
        };
        self.metrics.set(LabelMetrics { label, keybinding });
        self.base.set_size(label.0.max(keybinding.0), label.1 + keybinding.1);
    }

    /// As regras de CSS de `:hover`, `:active`, `:focus`, `.checked`, `.disabled` e as cores do texto, no
    /// rótulo e no atalho. Chamada nos ouvintes e nas trocas de estado.
    pub fn refresh_style(&self) {
        let (Some(label), Some(element)) = (self.label.get(), self.base.element()) else { return };
        let styles = &self.styles;
        let disabled = self.is_disabled();
        let vertical = self.vertical.get();
        let separator = self.is_separator();
        let action = &self.base.action;
        let focused = self.base.ui.focused() == Some(label);
        let mut tree = self.base.ui.tree_mut();
        let active = tree.has_class(element, "active");
        let highlight_toggled = tree.parent(element).is_some_and(|list| tree.has_class(list, "highlight-toggled"));
        let checked = action.checked() == Some(true);

        let has_icon_class = tree.has_class(label, "icon");
        let state = LabelState { separator, vertical, disabled, hovered: self.hovered.get(), active, highlight_toggled_checked: highlight_toggled && checked, focused, menu: self.base.options.is_menu };
        let color = styles.apply_to_label(tree.style_mut(label), state, has_icon_class);
        if let (Some(text), Some(color)) = (self.text.get(), color)
            && let Content::Text { color: text_color, .. } = &mut tree.style_mut(text).content
        {
            *text_color = color;
        }
        if let (Some(text), Some(color)) = (self.keybinding_text.get(), styles.foreground)
            && let Content::Text { color: text_color, .. } = &mut tree.style_mut(text).content
        {
            *text_color = color;
        }
        if let Some(node) = self.keybinding_node.get() {
            tree.style_mut(node).radius = styles.corner_radius;
        }
        tree.style_mut(element).cursor = Some(if disabled { je_ui::Cursor::Default } else { je_ui::Cursor::Pointer });
    }

    /// Põe o texto (ou o codicon) do rótulo no nó, na cor atual.
    fn render_label_content(&self) {
        let Some(label) = self.label.get() else { return };
        let styles = &self.styles;
        let font = ui_font(ACTION_LABEL_FONT_SIZE, 400);
        let color = styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255));
        let line_height = WORKBENCH_LINE_HEIGHT;
        let mut tree = self.base.ui.tree_mut();
        let class = self.css_class.borrow().clone();
        let icon_id = if self.icon && !self.is_separator() { Self::codicon_id(&class) } else { None };
        tree.style_mut(label).content = match icon_id {
            Some(id) => Content::Codicon { id, size: ACTION_ICON_SIZE as f32, color },
            None => Content::None,
        };
        let text_node = if let Some(text) = self.text.get() {
            text
        } else {
            let text = create_el(&mut tree, label, Role::Generic, "action-label-text");
            self.text.set(Some(text));
            text
        };
        let wants_text = self.label_enabled && !self.icon && !self.is_separator();
        tree.style_mut(text_node).visible = wants_text;
        if wants_text {
            let text = self.base.action.label();
            let width = crate::text::set_node_text(&mut tree, text_node, &text, &font, color, line_height);
            tree.set_bounds(text_node, Rect::new(ACTION_LABEL_PADDING, ACTION_LABEL_PADDING, ACTION_LABEL_PADDING + width, ACTION_LABEL_PADDING + line_height));
        }
    }
}

impl IActionViewItem for ActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    fn render(&self, container: NodeId) {
        self.base.base_render(container);
        let ui = self.base.ui.clone();
        let label = {
            let mut tree = ui.tree_mut();
            let label = create_el(&mut tree, container, self.default_aria_role(), "action-label");
            if let (true, Some(keybinding), false) = (self.label_enabled, self.base.options.keybinding.clone(), self.base.options.keybinding_not_rendered_with_label) {
                let node = create_el(&mut tree, container, Role::Generic, "keybinding");
                let font = ui_font(ACTION_LABEL_FONT_SIZE, 400);
                let line_height = WORKBENCH_LINE_HEIGHT;
                let text = create_el(&mut tree, node, Role::Generic, "keybinding-text");
                let width = crate::text::set_node_text(&mut tree, text, &keybinding, &font, self.styles.foreground.unwrap_or(Rgba::new(204, 204, 204, 255)), line_height);
                tree.set_bounds(text, Rect::new(ACTION_LABEL_PADDING, ACTION_LABEL_PADDING, ACTION_LABEL_PADDING + width, ACTION_LABEL_PADDING + line_height));
                self.keybinding_node.set(Some(node));
                self.keybinding_text.set(Some(text));
            }
            label
        };
        self.label.set(Some(label));

        for kind in [EventKind::PointerEnter, EventKind::PointerLeave] {
            let weak = self.weak_self.clone();
            self.base.register(ui.listen(label, kind, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    this.hovered.set(e.kind == EventKind::PointerEnter);
                    this.refresh_style();
                }
            }));
        }
        for (kind, capture) in [(EventKind::Focus, true), (EventKind::Blur, true), (EventKind::PointerDown, false), (EventKind::PointerUp, false)] {
            let weak = self.weak_self.clone();
            self.base.register(ui.listen(container, kind, capture, move |_| {
                if let Some(this) = weak.upgrade() {
                    this.refresh_style();
                }
            }));
        }

        self.update_class();
        self.update_label();
        self.base.update_tooltip();
        self.update_enabled();
        self.update_checked();
    }

    fn focus(&self, _from_right: bool) {
        if let Some(label) = self.label.get() {
            self.label_tab_index.set(0);
            self.base.ui.focus(label);
            self.refresh_style();
        }
    }

    fn is_focused(&self) -> bool {
        self.label.get().is_some() && self.label_tab_index.get() == 0
    }

    fn tab_stops(&self) -> usize {
        usize::from(self.label.get().is_some() && self.label_tab_index.get() == 0)
    }

    fn blur(&self) {
        if self.label.get().is_some() {
            self.label_tab_index.set(-1);
        }
    }

    fn set_focusable(&self, focusable: bool) {
        if self.label.get().is_some() {
            self.label_tab_index.set(if focusable { 0 } else { -1 });
        }
    }

    fn update_label(&self) {
        self.render_label_content();
        self.relayout();
    }

    fn get_tooltip(&self) -> Option<String> {
        let action = &self.base.action;
        let mut title = None;
        if !action.tooltip().is_empty() {
            title = Some(action.tooltip());
        } else if !action.label().is_empty() {
            let label = action.label();
            title = Some(match &self.base.options.keybinding {
                Some(keybinding) if !keybinding.is_empty() => localize("titleLabel", "{0} ({1})", &[&label, keybinding]),
                _ => label,
            });
        }
        title
    }

    fn update_class(&self) {
        let Some(label) = self.label.get() else { return };
        let action = &self.base.action;
        {
            let mut tree = self.base.ui.tree_mut();
            for class in self.css_class.borrow().split_whitespace() {
                tree.remove_class(label, class);
            }
        }
        if self.is_separator() && action.class().is_some_and(|c| !c.is_empty()) {
            if let Some(class) = action.class() {
                self.base.ui.tree_mut().add_class(label, &class);
            }
        } else if self.icon {
            let class = self.get_class().unwrap_or_default();
            {
                let mut tree = self.base.ui.tree_mut();
                tree.add_class(label, "codicon");
                for c in class.split_whitespace() {
                    tree.add_class(label, c);
                }
            }
            *self.css_class.borrow_mut() = class;
            self.update_enabled();
        } else {
            self.base.ui.tree_mut().remove_class(label, "codicon");
        }
        self.render_label_content();
        self.relayout();
    }

    fn update_enabled(&self) {
        let enabled = self.base.action.enabled();
        {
            let mut tree = self.base.ui.tree_mut();
            if let Some(label) = self.label.get() {
                tree.toggle_class(label, "disabled", !enabled);
            }
            if let Some(element) = self.base.element() {
                tree.toggle_class(element, "disabled", !enabled);
            }
        }
        self.refresh_style();
    }

    fn update_aria_label(&self) {
        if let Some(label) = self.label.get() {
            self.base.ui.tree_mut().set_label(label, self.get_tooltip().unwrap_or_default());
        }
    }

    fn update_checked(&self) {
        let Some(label) = self.label.get() else { return };
        let checked = self.base.action.checked();
        let mut tree = self.base.ui.tree_mut();
        match checked {
            Some(checked) => {
                tree.toggle_class(label, "checked", checked);
                if let Some(element) = self.base.element() {
                    tree.toggle_class(element, "checked", checked);
                }
                // No original: `aria-selected` numa lista de abas, `aria-pressed` e `role="button"` nas demais.
                let role = if self.base.options.is_tab_list { self.default_aria_role() } else { Role::Button };
                tree.set_role(label, role);
            }
            None => {
                tree.remove_class(label, "checked");
                if let Some(element) = self.base.element() {
                    tree.remove_class(element, "checked");
                }
                let role = self.default_aria_role();
                tree.set_role(label, role);
            }
        }
        drop(tree);
        self.refresh_style();
    }

    fn size(&self, vertical: bool) -> ItemSize {
        let metrics = self.metrics.get();
        if self.is_separator() && vertical {
            // `margin: 5px 4px !important` (a do `.separator` genérico vence a de `.8em`), `padding-top: 1px`,
            // `border-bottom: 1px`: 12px de altura, a largura é a da barra.
            // Num menu (`menu.ts`): `margin: 5px 0`, `border-bottom: 1px` e altura 0: 11px.
            (0.0, if self.base.options.is_menu { MENU_SEPARATOR_HEIGHT } else { 12.0 })
        } else {
            (metrics.label.0.max(metrics.keybinding.0), metrics.label.1 + metrics.keybinding.1)
        }
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.vertical.set(vertical);
        let (Some(label), metrics) = (self.label.get(), self.metrics.get()) else { return };
        let mut tree = self.base.ui.tree_mut();
        if self.is_separator() {
            if vertical {
                // Barra comum: `margin: 5px 4px`, `width: 100%` (de caixa de conteúdo: passa 4px do `li`) e a caixa de
                // 2px (1px de `padding-top` e a linha de 1px). Num menu, `margin: 5px 0` e a linha de 1px.
                if self.base.options.is_menu {
                    tree.set_bounds(label, Rect::new(0.0, 5.0, width, 6.0));
                } else {
                    tree.set_bounds(label, Rect::new(4.0, 5.0, 4.0 + width, 7.0));
                }
            } else {
                // `width: 1px; height: 16px; margin: 5px 4px`.
                tree.set_bounds(label, Rect::new(4.0, 5.0, 5.0, 5.0 + ACTION_ICON_SIZE));
            }
        } else {
            // Em barra vertical o rótulo (`display: flex`, nível de bloco) ocupa a largura toda do `li`.
            let label_width = if vertical { width.max(metrics.label.0) } else { metrics.label.0 };
            tree.set_bounds(label, Rect::new(0.0, 0.0, label_width, metrics.label.1));
            if let Some(node) = self.keybinding_node.get() {
                let kb_width = if vertical { width.max(metrics.keybinding.0) } else { metrics.keybinding.0 };
                tree.set_bounds(node, Rect::new(0.0, metrics.label.1, kb_width, metrics.label.1 + metrics.keybinding.1));
            }
        }
        let _ = height;
        drop(tree);
        self.refresh_style();
    }
}

impl_action_view_item_dispose!(ActionViewItem, base);

/// O que o `getActionContext(option, index)` devolve.
pub type ActionContextFactory = Rc<dyn Fn(&str, usize) -> Dynamic>;

/// `SelectActionViewItem`: um item cuja ação é escolher uma opção numa caixa de seleção.
pub struct SelectActionViewItem {
    base: BaseActionViewItem,
    /// `protected selectBox`.
    select_box: Rc<SelectBox>,
    action_context_factory: RefCell<Option<ActionContextFactory>>,
}

impl SelectActionViewItem {
    /// `new SelectActionViewItem(ctx, action, options, selected, contextViewProvider, styles, selectBoxOptions)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &Ui,
        context: Option<Dynamic>,
        action: ActionRef,
        options: Vec<ISelectOptionItem>,
        selected: usize,
        context_view_provider: Rc<dyn IContextViewProvider>,
        styles: ISelectBoxStyles,
        select_box_options: Option<ISelectBoxOptions>,
    ) -> Rc<Self> {
        let select_box = SelectBox::new(ui, options, selected, context_view_provider, styles, select_box_options);
        select_box.set_focusable(false);
        let this = Rc::new_cyclic(|weak: &Weak<Self>| Self {
            base: BaseActionViewItem::new_derived(ui, context, action, IActionViewItemOptions::default(), weak.clone() as Weak<dyn IActionViewItem>),
            select_box: select_box.clone(),
            action_context_factory: RefCell::new(None),
        });
        this.base.register(select_box.clone());
        let weak = Rc::downgrade(&this);
        this.base.register(select_box.on_did_select().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.run_action(&e.selected, e.index);
            }
        }));
        this
    }

    pub fn set_options(&self, options: Vec<ISelectOptionItem>, selected: Option<usize>) {
        self.select_box.set_options(options, selected);
    }

    pub fn select(&self, index: usize) {
        self.select_box.select(index);
    }

    /// A caixa de seleção (`this.selectBox`).
    pub fn select_box(&self) -> &Rc<SelectBox> {
        &self.select_box
    }

    /// Troca o `getActionContext`: sem fábrica o contexto é o texto da opção.
    pub fn set_action_context_factory(&self, factory: Option<ActionContextFactory>) {
        *self.action_context_factory.borrow_mut() = factory;
    }

    fn run_action(&self, option: &str, index: usize) {
        let factory = self.action_context_factory.borrow().clone();
        let context = factory.map_or_else(|| Dynamic::from(option), |factory| factory(option, index));
        // O `run` do runner devolve a promessa e o original a ignora.
        let _ = self.base.action_runner().run(&self.base.action(), Some(context));
    }
}

impl IActionViewItem for SelectActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        &self.base
    }

    fn set_focusable(&self, focusable: bool) {
        self.select_box.set_focusable(focusable);
    }

    fn focus(&self, _from_right: bool) {
        self.select_box.focus();
    }

    fn blur(&self) {
        self.select_box.blur();
    }

    fn render(&self, container: NodeId) {
        self.select_box.render(container);
    }

    fn size(&self, _vertical: bool) -> ItemSize {
        self.select_box.natural_size()
    }

    fn layout(&self, _vertical: bool, _width: f64, _height: f64) {
        self.select_box.layout();
    }
}

impl_action_view_item_dispose!(SelectActionViewItem, base);

