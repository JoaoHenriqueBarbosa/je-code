//! Porte de `src/vs/workbench/contrib/markers/browser/markersViewActions.ts` (e de `markersViewActions.css`): os filtros
//! do painel de problemas (`MarkersFilters`), a ação de correção rápida (`QuickFixAction`) e o item que a desenha
//! (`QuickFixActionViewItem`).
//!
//! Notas de tradução:
//! - **`IMarkersFiltersChangeEvent`.** Os campos `excludedFiles?: boolean` e companheiros (que o original só põe em
//!   `true`) são `bool`, falsos quando o campo falta.
//! - **`MarkersFilters`.** Cada filtro é uma chave de contexto (`MarkersContextKeys`) ligada ao serviço recebido; o getter
//!   é o `!!key.get()` e o setter só grava e avisa quando o valor muda (a comparação `!==` do original, estrita sobre o
//!   booleano). O `filterHistory` é um campo mutável (`RefCell`).
//! - **`QuickFixAction extends Action`** é composição: guarda uma [`Action`] e repassa o contrato de [`IAction`]. O
//!   `as_any` devolve o próprio `QuickFixAction` (o `<QuickFixAction>action` do item de ação é o `downcast_ref`), e
//!   `as_base_action` devolve a [`Action`] de dentro, para o item ouvir as mudanças dela.
//! - **`QuickFixActionViewItem extends ActionViewItem`** é composição ([`ActionViewItem::new_with_owner`]), como o
//!   `ActionWithDropdownActionViewItem`. O `@IContextMenuService` do construtor é o parâmetro `context_menu_service`.
//!   Divergência: o `onClick` do `BaseActionViewItem` do porte não é virtual, então o clique roda a ação pelo executor de
//!   ações (o caminho do teclado do original), e `QuickFixAction.run` dispara `onShowQuickFixes`, que o desenhista do
//!   marcador liga a [`QuickFixActionViewItem::show_quick_fixes`]; o resultado visível (o menu de contexto abaixo do
//!   item) é o mesmo. `getDomNodePagePosition` é o `absolute_bounds` do nó.
//! - **Classes de ícone.** O `ThemeIcon.asClassName(Codicon.lightBulb)` usa o codicon `light-bulb` direto.
//! - **`./markersViewActions.css`.** Só tem a regra do botão de filtro marcado (`.markers-filters.checked`), que é o
//!   estilo do botão de filtros da barra de ações da visão (a próxima fatia, `markersView.ts`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::{Action, ActionRef, IAction};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::layout::IAnchor;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};
use je_ui::{NodeId, Ui};
use je_widgets::action_view_items::{ActionViewItem, BaseActionViewItem, IActionViewItem, IActionViewItemOptions, ItemSize};

use super::markers_common::{
    show_active_file_filter_context_key, show_errors_filter_context_key, show_excluded_files_filter_context_key, show_info_filter_context_key, show_warnings_filter_context_key,
};
use super::markers_model::Marker;
use super::messages::Messages;

/// `IMarkersFiltersChangeEvent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkersFiltersChangeEvent {
    pub excluded_files: bool,
    pub show_warnings: bool,
    pub show_errors: bool,
    pub show_infos: bool,
    pub active_file: bool,
}

/// `IMarkersFiltersOptions`.
#[derive(Clone, Debug, Default)]
pub struct MarkersFiltersOptions {
    pub filter_history: Vec<String>,
    pub show_errors: bool,
    pub show_warnings: bool,
    pub show_infos: bool,
    pub excluded_files: bool,
    pub active_file: bool,
}

/// `MarkersFilters`.
pub struct MarkersFilters {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<MarkersFiltersChangeEvent>>,
    excluded_files: Rc<dyn IContextKey>,
    active_file: Rc<dyn IContextKey>,
    show_warnings: Rc<dyn IContextKey>,
    show_infos: Rc<dyn IContextKey>,
    show_errors: Rc<dyn IContextKey>,
    filter_history: RefCell<Vec<String>>,
}

impl MarkersFilters {
    pub fn new(options: &MarkersFiltersOptions, context_key_service: &dyn IContextKeyService) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        let bind = |key: je_platform::contextkey::RawContextKey, value: bool| {
            let bound = key.bind_to(context_key_service);
            bound.set(Dynamic::Bool(value));
            bound
        };
        Rc::new(Self {
            base,
            on_did_change,
            excluded_files: bind(show_excluded_files_filter_context_key(), options.excluded_files),
            active_file: bind(show_active_file_filter_context_key(), options.active_file),
            show_warnings: bind(show_warnings_filter_context_key(), options.show_warnings),
            show_infos: bind(show_info_filter_context_key(), options.show_infos),
            show_errors: bind(show_errors_filter_context_key(), options.show_errors),
            filter_history: RefCell::new(options.filter_history.clone()),
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<MarkersFiltersChangeEvent> {
        self.on_did_change.event()
    }

    /// `filterHistory`.
    pub fn filter_history(&self) -> Vec<String> {
        self.filter_history.borrow().clone()
    }

    pub fn set_filter_history(&self, filter_history: Vec<String>) {
        *self.filter_history.borrow_mut() = filter_history;
    }

    /// O setter comum: grava e avisa só quando o valor muda.
    fn update(&self, key: &Rc<dyn IContextKey>, value: bool, event: MarkersFiltersChangeEvent) {
        if !matches!(key.get(), Dynamic::Bool(current) if current == value) {
            key.set(Dynamic::Bool(value));
            self.on_did_change.fire(event);
        }
    }

    pub fn excluded_files(&self) -> bool {
        self.excluded_files.get().is_truthy()
    }

    pub fn set_excluded_files(&self, files_exclude: bool) {
        self.update(&self.excluded_files, files_exclude, MarkersFiltersChangeEvent { excluded_files: true, ..Default::default() });
    }

    pub fn active_file(&self) -> bool {
        self.active_file.get().is_truthy()
    }

    pub fn set_active_file(&self, active_file: bool) {
        self.update(&self.active_file, active_file, MarkersFiltersChangeEvent { active_file: true, ..Default::default() });
    }

    pub fn show_warnings(&self) -> bool {
        self.show_warnings.get().is_truthy()
    }

    pub fn set_show_warnings(&self, show_warnings: bool) {
        self.update(&self.show_warnings, show_warnings, MarkersFiltersChangeEvent { show_warnings: true, ..Default::default() });
    }

    pub fn show_errors(&self) -> bool {
        self.show_errors.get().is_truthy()
    }

    pub fn set_show_errors(&self, show_errors: bool) {
        self.update(&self.show_errors, show_errors, MarkersFiltersChangeEvent { show_errors: true, ..Default::default() });
    }

    pub fn show_infos(&self) -> bool {
        self.show_infos.get().is_truthy()
    }

    pub fn set_show_infos(&self, show_infos: bool) {
        self.update(&self.show_infos, show_infos, MarkersFiltersChangeEvent { show_infos: true, ..Default::default() });
    }
}

impl IDisposable for MarkersFilters {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "MarkersFilters"
    }
}

/// `QuickFixAction`.
pub struct QuickFixAction {
    base: Rc<Disposable>,
    action: Rc<Action>,
    on_show_quick_fixes: Rc<Emitter<()>>,
    quick_fixes: RefCell<Vec<ActionRef>>,
    /// `readonly marker`.
    pub marker: Rc<Marker>,
}

impl QuickFixAction {
    pub const ID: &'static str = "workbench.actions.problems.quickfix";

    /// `private static readonly CLASS`.
    fn class() -> String {
        format!("markers-panel-action-quickfix {}", ThemeIcon::from_id("light-bulb").as_class_name())
    }

    /// `private static readonly AUTO_FIX_CLASS`.
    fn auto_fix_class() -> String {
        format!("{} autofixable", Self::class())
    }

    pub fn new(marker: Rc<Marker>) -> Rc<Self> {
        let base = Disposable::new();
        let on_show_quick_fixes = Emitter::new();
        base.register(on_show_quick_fixes.clone());
        let action = Action::new(Self::ID, &Messages::markers_panel_action_tooltip_quickfix(), &Self::class(), false, None);
        base.register(action.clone());
        Rc::new(Self { base, action, on_show_quick_fixes, quick_fixes: RefCell::new(Vec::new()), marker })
    }

    /// `onShowQuickFixes`.
    pub fn on_show_quick_fixes(&self) -> Event<()> {
        self.on_show_quick_fixes.event()
    }

    /// `onDidChange` da [`Action`] de dentro.
    pub fn on_did_change(&self) -> Event<je_base::actions::IActionChangeEvent> {
        self.action.on_did_change()
    }

    /// `get quickFixes()`.
    pub fn quick_fixes(&self) -> Vec<ActionRef> {
        self.quick_fixes.borrow().clone()
    }

    /// `set quickFixes(quickFixes)`: habilita a ação quando há correções.
    pub fn set_quick_fixes(&self, quick_fixes: Vec<ActionRef>) {
        let has_fixes = !quick_fixes.is_empty();
        *self.quick_fixes.borrow_mut() = quick_fixes;
        self.action.set_enabled(has_fixes);
    }

    /// `autoFixable(autofixable)`.
    pub fn auto_fixable(&self, autofixable: bool) {
        self.action.set_class(Some(if autofixable { Self::auto_fix_class() } else { Self::class() }));
    }
}

impl IAction for QuickFixAction {
    fn id(&self) -> String {
        self.action.id()
    }
    fn label(&self) -> String {
        self.action.label()
    }
    fn set_label(&self, value: String) {
        self.action.set_label(value);
    }
    fn tooltip(&self) -> String {
        self.action.tooltip()
    }
    fn set_tooltip(&self, value: String) {
        self.action.set_tooltip(value);
    }
    fn class(&self) -> Option<String> {
        self.action.class()
    }
    fn set_class(&self, value: Option<String>) {
        self.action.set_class(value);
    }
    fn enabled(&self) -> bool {
        self.action.enabled()
    }
    fn set_enabled(&self, value: bool) {
        self.action.set_enabled(value);
    }
    fn checked(&self) -> Option<bool> {
        self.action.checked()
    }
    fn set_checked(&self, value: Option<bool>) {
        self.action.set_checked(value);
    }

    /// `override run()`: pede para mostrar as correções.
    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        self.on_show_quick_fixes.fire(());
        Promise::resolved(Dynamic::Undefined)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_base_action(&self) -> Option<&Action> {
        Some(&*self.action)
    }
}

impl IDisposable for QuickFixAction {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "QuickFixAction"
    }
}

/// O delegado do menu de contexto das correções: ancora no ponto dado e lista as ações.
struct QuickFixesMenuDelegate {
    anchor: IAnchor,
    actions: Vec<ActionRef>,
}

impl IContextMenuDelegate for QuickFixesMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Anchor(self.anchor)
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }
}

/// `QuickFixActionViewItem`.
pub struct QuickFixActionViewItem {
    inner: Rc<ActionViewItem>,
    ui: Ui,
    context_menu_service: Rc<dyn IContextMenuService>,
    disposed: Cell<bool>,
}

impl QuickFixActionViewItem {
    /// `new QuickFixActionViewItem(action, options)` com o `@IContextMenuService` do construtor.
    pub fn new(ui: &Ui, action: ActionRef, options: IActionViewItemOptions, context_menu_service: Rc<dyn IContextMenuService>) -> Rc<Self> {
        let options = IActionViewItemOptions { icon: Some(true), label: Some(false), ..options };
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            inner: ActionViewItem::new_with_owner(ui, None, action, options, weak.clone() as Weak<dyn IActionViewItem>),
            ui: ui.clone(),
            context_menu_service,
            disposed: Cell::new(false),
        })
    }

    /// `showQuickFixes()`: o menu de contexto com as correções, logo abaixo do item.
    pub fn show_quick_fixes(&self) {
        let Some(element) = self.inner.base().element() else { return };
        if !self.is_enabled() {
            return;
        }
        let position = self.ui.tree().absolute_bounds(element);
        let quick_fixes = self.inner.base().action().as_any().downcast_ref::<QuickFixAction>().map(QuickFixAction::quick_fixes).unwrap_or_default();
        if !quick_fixes.is_empty() {
            let anchor = IAnchor { x: position.x0 + 10.0, y: position.y0 + position.height() + 4.0, width: None, height: None };
            self.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(QuickFixesMenuDelegate { anchor, actions: quick_fixes })));
        }
    }
}

impl IActionViewItem for QuickFixActionViewItem {
    fn base(&self) -> &BaseActionViewItem {
        self.inner.base()
    }

    fn render(&self, container: NodeId) {
        self.inner.render(container);
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

    fn update_enabled(&self) {
        self.inner.update_enabled();
    }

    fn update_label(&self) {
        self.inner.update_label();
    }

    fn update_class(&self) {
        self.inner.update_class();
    }

    fn update_checked(&self) {
        self.inner.update_checked();
    }

    fn get_tooltip(&self) -> Option<String> {
        self.inner.get_tooltip()
    }

    fn update_aria_label(&self) {
        self.inner.update_aria_label();
    }

    fn size(&self, vertical: bool) -> ItemSize {
        self.inner.size(vertical)
    }

    fn layout(&self, vertical: bool, width: f64, height: f64) {
        self.inner.layout(vertical, width, height);
    }
}

impl IDisposable for QuickFixActionViewItem {
    fn dispose(&self) {
        if !self.disposed.replace(true) {
            self.inner.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "QuickFixActionViewItem"
    }
}
