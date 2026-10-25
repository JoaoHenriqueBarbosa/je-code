//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollController.ts`: o `StickyScrollController`, contribuição
//! do editor que escuta rolagem, layout, tokens, cursor, mouse e foco, calcula as linhas fixas com o
//! [`StickyLineCandidateProvider`] e as entrega ao [`StickyScrollWidget`].
//!
//! Representação:
//! - `StickyScrollController extends Disposable implements IEditorContribution, IStickyScrollController` é a struct
//!   [`StickyScrollController`], criada por [`StickyScrollController::new`] (devolve `Rc<Self>`: os ouvintes, as promessas
//!   e os agendamentos guardam um [`Weak`]). `IStickyScrollController` é o trait [`IStickyScrollController`], cujos métodos
//!   têm aqui a única definição; `StickyScrollController.get(editor)` é [`StickyScrollController::get`].
//! - Os argumentos injetados do construtor (`@IContextMenuService`, `@ILanguageFeaturesService`, `@IInstantiationService`,
//!   `@ILanguageConfigurationService`, `@ILanguageFeatureDebounceService` e `@IContextKeyService`) são os
//!   [`StickyScrollControllerServices`]. O `ILanguageFeatureDebounceService` o original recebe e nunca lê, então não existe
//!   aqui. O `@IInstantiationService` só serve para `invokeFunction(goToDefinitionWithLocation, ...)`, e essa função
//!   (`inlayHintsLocations.ts`, que abre o editor de destino ou o peek por `DefinitionAction`, `ITextModelService` e
//!   `PeekContext`) não tem porte: ela chega como o [`IStickyScrollDefinitionNavigation`] injetado.
//! - O `ClickLinkGesture` é o de [`crate::click_link_gesture`] e o `getDefinitionsAtPosition` é
//!   [`get_location_links_at_position`] sobre o registro `definition_provider`.
//! - **Eventos de DOM como métodos.** O original ouve o DOM do widget (`CONTEXT_MENU`, `MOUSE_DOWN`, `CLICK`, o rastreador de
//!   foco) e da janela (`MOUSE_MOVE`, `KEY_DOWN`, `KEY_UP`). Aqui quem desenha (o pintor nativo) entrega esses eventos:
//!   [`StickyScrollController::on_context_menu`], [`StickyScrollController::on_widget_mouse_down`],
//!   [`StickyScrollController::on_widget_click`], [`StickyScrollController::on_widget_focus`],
//!   [`StickyScrollController::on_widget_blur`], [`StickyScrollController::on_window_mouse_move`],
//!   [`StickyScrollController::on_window_key_down`] e [`StickyScrollController::on_window_key_up`]. O nó DOM do alvo do
//!   mouse é o [`ElementId`] (`this._mouseTarget` só guarda alvos dentro do widget: o pintor passa `None` para os de fora),
//!   e `stickyScrollDomNode.clientHeight === 0` é o widget invisível ([`StickyScrollWidgetLayout::visible`] falso).
//! - `StandardMouseEvent(targetWindow, e)` do menu de contexto é a posição `(posx, posy)`; o `MouseEvent` do clique é o de
//!   `editor_browser`.
//! - `EditorContextKeys.stickyScrollFocused` e `stickyScrollVisible` são as chaves de [`crate::editor_context_keys`].
//! - `Number.MAX_SAFE_INTEGER` de `_maxStickyLines` é `f64`, e `Math.min` propaga o `NaN` como no JavaScript.
//! - As funções `async` (`_renderStickyScroll`, `_updateState`) são [`async_fn`] e rodam de forma síncrona até o primeiro
//!   `await`, como no JavaScript; `_resetState`, que é `async` sem `await`, é o método síncrono
//!   [`StickyScrollController::reset_state`]. O `await` do modelo de dobras (`FoldingController.get(editor)?.getFoldingModel()
//!   ?? undefined`) espera uma rodada de microtarefas mesmo sem controlador de dobras, como o `await undefined`.
//! - `_rebuildFromLine`, que o original chama de "linha" mas o widget lê como índice (`_renderStickyScroll(lineNumber)`), é um
//!   `usize`; o `Math.min` com `Infinity` é o `Option`.
//!
//! Divergências:
//! - `_focusedStickyElementIndex` fora da lista em `goToFocused` (índice `-1` sem linhas fixas): o original revela uma posição
//!   com `lineNumber: undefined`; aqui a ação só descarta o foco e sai.
//! - O ramo `else if (!currentHTMLChild)` do sublinhado do link é inalcançável no original (`currentHTMLChild` é sempre
//!   `undefined` na primeira execução do `then`), então o `if (currentHTMLChild !== childHTML)` vale sempre e não há `else if`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_platform::actions::MenuActionOptions;
use je_platform::actions::menu_ids::sticky_scroll_context;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::{
    AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuMenuDelegate, IContextMenuService,
};

use super::sticky_scroll_element::StickyRange;
use super::sticky_scroll_provider::{IStickyLineCandidateProvider, StickyLineCandidateProvider};
use super::sticky_scroll_widget::{
    StickyScrollHeightChange, StickyScrollWidget, StickyScrollWidgetLayout, StickyScrollWidgetState,
};
use crate::click_link_gesture::{ClickLinkGesture, ClickLinkKeyboardEvent, ClickLinkMouseEvent, IClickLinkGestureOptions};
use crate::code_editor_widget::{
    CodeEditorWidget, ICodeEditor, IEditorContribution, SelectionOrRange,
};
use crate::config::editor_options::line_numbers::{LINE_NUMBERS, RenderLineNumbersType};
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::sticky_scroll::STICKY_SCROLL;
use crate::config::editor_options::EditorOption;
use crate::editor_browser::{ElementId, IOverlayWidget, MouseEvent, MouseTarget, MouseTargetOverlayWidget};
use crate::editor_common::ScrollType;
use crate::editor_context_keys::{sticky_scroll_focused, sticky_scroll_visible};
use crate::folding::folding_controller::FoldingController;
use crate::folding::folding_model::{FoldingModel, toggle_collapse_state};
use crate::goto_symbol::get_location_links_at_position;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_features_service::{LanguageFeaturesService, Location};
use crate::multicursor::listen_weak;
use crate::position::Position;
use crate::range::Range;
use crate::text_model_events::ModelTokensChangedEvent;

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `IStickyScrollController`.
pub trait IStickyScrollController: IDisposable {
    /// `stickyScrollCandidateProvider`.
    fn sticky_scroll_candidate_provider(&self) -> Rc<dyn IStickyLineCandidateProvider>;

    /// `stickyScrollWidgetState`.
    fn sticky_scroll_widget_state(&self) -> StickyScrollWidgetState;

    /// `stickyScrollWidgetHeight`.
    fn sticky_scroll_widget_height(&self) -> f64;

    fn is_focused(&self) -> bool;

    fn focus(&self);

    fn focus_next(&self);

    fn focus_previous(&self);

    fn go_to_focused(&self);

    fn find_scroll_widget_state(&self) -> StickyScrollWidgetState;

    fn select_editor(&self);

    /// `onDidChangeStickyScrollHeight`.
    fn on_did_change_sticky_scroll_height(&self) -> Event<StickyScrollHeightChange>;
}

/// O que o original faz com `goToDefinitionWithLocation(accessor, event, editor, location)` (ver o topo do módulo): abre a
/// definição de `location` (no editor, ao lado ou no peek, conforme o evento e as opções).
pub trait IStickyScrollDefinitionNavigation {
    fn go_to_definition_with_location(&self, event: &ClickLinkMouseEvent, editor: &Rc<CodeEditorWidget>, location: &Location) -> Promise<()>;
}

/// Os serviços injetados no construtor (ver o topo do módulo).
pub struct StickyScrollControllerServices {
    /// `@IContextMenuService`.
    pub context_menu_service: Rc<dyn IContextMenuService>,
    /// `@ILanguageFeaturesService`.
    pub language_features_service: Rc<LanguageFeaturesService>,
    /// `@ILanguageConfigurationService`.
    pub language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `invokeFunction(goToDefinitionWithLocation, ...)` do `@IInstantiationService`.
    pub definition_navigation: Rc<dyn IStickyScrollDefinitionNavigation>,
}

/// A âncora do menu de contexto: a posição do ponteiro (`getAnchor: () => event`).
struct StickyScrollContextMenuDelegate {
    posx: f64,
    posy: f64,
}

impl IContextMenuDelegate for StickyScrollContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Mouse { posx: self.posx, posy: self.posy }
    }

    /// As ações vêm do `menuId` do delegado "de menu".
    fn get_actions(&self) -> Vec<je_base::actions::ActionRef> {
        Vec::new()
    }
}

/// `Math.min(a, b)` do JavaScript: propaga o `NaN`.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `StickyScrollController`.
pub struct StickyScrollController {
    me: Weak<StickyScrollController>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    services: StickyScrollControllerServices,

    sticky_scroll_widget: Rc<StickyScrollWidget>,
    sticky_line_candidate_provider: Rc<dyn IStickyLineCandidateProvider>,
    session_store: Rc<DisposableStore>,
    /// O `sessionStore` de `_registerMouseListeners`: o sublinhado do link e a busca da definição.
    link_session_store: Rc<DisposableStore>,

    widget_state: RefCell<StickyScrollWidgetState>,
    folding_model: RefCell<Option<Rc<FoldingModel>>>,
    max_sticky_lines: Cell<f64>,

    sticky_range_projected_on_editor: Cell<Option<Range>>,
    candidate_definitions_length: Cell<i32>,

    sticky_scroll_focused_context_key: Rc<dyn IContextKey>,
    sticky_scroll_visible_context_key: Rc<dyn IContextKey>,

    focus_disposable_store: RefCell<Option<Rc<DisposableStore>>>,
    focused_sticky_element_index: Cell<i64>,
    enabled: Cell<bool>,
    focused: Cell<bool>,
    position_revealed: Cell<bool>,
    on_mouse_down: Cell<bool>,
    end_line_numbers: RefCell<Vec<i32>>,
    show_end_for_line: Cell<Option<usize>>,
    min_rebuild_from_line: Cell<Option<usize>>,
    mouse_target: Cell<Option<ElementId>>,
    cursor_position_listener: RefCell<Option<DisposableRef>>,
    position_line_number: Cell<Option<i32>>,

    on_did_change_sticky_scroll_height: Rc<Emitter<StickyScrollHeightChange>>,
}

impl StickyScrollController {
    pub const ID: &'static str = "store.contrib.stickyScrollController";

    /// `StickyScrollController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<StickyScrollController>> {
        editor.get_contribution_as::<StickyScrollController>(Self::ID)
    }

    /// `new StickyScrollController(editor, ...services)`.
    pub fn new(editor: &Rc<CodeEditorWidget>, services: StickyScrollControllerServices) -> Rc<Self> {
        let base = Disposable::new();
        let sticky_scroll_widget = StickyScrollWidget::new(editor);
        let sticky_line_candidate_provider = StickyLineCandidateProvider::new(
            editor,
            Rc::clone(&services.language_features_service),
            Rc::clone(&services.language_configuration_service),
        );
        base.register(sticky_scroll_widget.clone());
        base.register(sticky_line_candidate_provider.clone());

        let on_did_change_sticky_scroll_height = Emitter::new();
        base.register(on_did_change_sticky_scroll_height.clone());
        let link_session_store = DisposableStore::new();
        base.register(link_session_store.clone());

        let sticky_scroll_focused_context_key = sticky_scroll_focused().bind_to(&*services.context_key_service);
        let sticky_scroll_visible_context_key = sticky_scroll_visible().bind_to(&*services.context_key_service);

        let this = Rc::new_cyclic(|me: &Weak<StickyScrollController>| {
            base.register(listen_weak(me, editor.on_did_change_line_height(), |this: &StickyScrollController, e| {
                for change in &e.changes {
                    this.render_line_if_sticky(change.line_number);
                }
            }));
            base.register(listen_weak(me, editor.on_did_change_font(), |this: &StickyScrollController, e| {
                for change in &e.changes {
                    this.render_line_if_sticky(change.line_number);
                }
            }));
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &StickyScrollController, e| {
                this.read_configuration_change(e);
            }));
            base.register(listen_weak(me, sticky_scroll_widget.on_did_change_sticky_scroll_height(), |this: &StickyScrollController, e| {
                this.on_did_change_sticky_scroll_height.fire(*e);
            }));

            StickyScrollController {
                me: me.clone(),
                base: base.clone(),
                editor: Rc::downgrade(editor),
                services,
                sticky_scroll_widget: Rc::clone(&sticky_scroll_widget),
                sticky_line_candidate_provider,
                session_store: DisposableStore::new(),
                link_session_store,
                widget_state: RefCell::new(StickyScrollWidgetState::empty()),
                folding_model: RefCell::new(None),
                max_sticky_lines: Cell::new(MAX_SAFE_INTEGER),
                sticky_range_projected_on_editor: Cell::new(None),
                candidate_definitions_length: Cell::new(-1),
                sticky_scroll_focused_context_key,
                sticky_scroll_visible_context_key,
                focus_disposable_store: RefCell::new(None),
                focused_sticky_element_index: Cell::new(-1),
                enabled: Cell::new(false),
                focused: Cell::new(false),
                position_revealed: Cell::new(false),
                on_mouse_down: Cell::new(false),
                end_line_numbers: RefCell::new(Vec::new()),
                show_end_for_line: Cell::new(None),
                min_rebuild_from_line: Cell::new(None),
                mouse_target: Cell::new(None),
                cursor_position_listener: RefCell::new(None),
                position_line_number: Cell::new(None),
                on_did_change_sticky_scroll_height,
            }
        });
        this.register_mouse_listeners(editor);
        this.on_did_resize();
        this.read_configuration();
        this
    }

    /// Os ouvintes de `onDidChangeLineHeight` e `onDidChangeFont`: se a linha é uma das fixas, refaz o widget a partir dela.
    fn render_line_if_sticky(&self, line_number: i32) {
        let is_sticky = self.widget_state.borrow().start_line_numbers.contains(&line_number);
        if is_sticky {
            let _ = self.render_sticky_scroll(Some(line_number as usize));
        }
    }

    fn dispose_focus_sticky_scroll_store(&self) {
        self.sticky_scroll_focused_context_key.set(Dynamic::Bool(false));
        let focus_disposable_store = self.focus_disposable_store.borrow_mut().take();
        if let Some(focus_disposable_store) = focus_disposable_store {
            focus_disposable_store.dispose();
        }
        self.focused.set(false);
        self.position_revealed.set(false);
        self.on_mouse_down.set(false);
    }

    // #region Eventos do widget (o DOM do original)

    /// `CONTEXT_MENU` do nó do widget: mostra o menu `StickyScrollContext` na posição do ponteiro.
    pub fn on_context_menu(&self, posx: f64, posy: f64) {
        self.services.context_menu_service.show_context_menu(ContextMenuRequest::Menu(IContextMenuMenuDelegate {
            menu_id: Some(sticky_scroll_context()),
            menu_action_options: Some(MenuActionOptions { render_short_title: true, ..MenuActionOptions::default() }),
            get_actions_context: None,
            context_key_service: None,
            get_actions: None,
            base: Rc::new(StickyScrollContextMenuDelegate { posx, posy }),
        }));
    }

    /// `focusTracker.onDidBlur`.
    pub fn on_widget_blur(&self) {
        // Suppose that the blurring is caused by scrolling, then keep the focus on the sticky scroll
        // This is determined by the fact that the height of the widget has become zero and there has been no position revealing
        let layout: StickyScrollWidgetLayout = self.sticky_scroll_widget.layout();
        if !self.position_revealed.get() && !layout.visible {
            self.focused_sticky_element_index.set(-1);
            IStickyScrollController::focus(self);
        }
        // In all other casees, dispose the focus on the sticky scroll
        else {
            self.dispose_focus_sticky_scroll_store();
        }
    }

    /// `focusTracker.onDidFocus`.
    pub fn on_widget_focus(&self) {
        IStickyScrollController::focus(self);
    }

    /// `MOUSE_DOWN` do nó do widget: suppose that mouse down on the sticky scroll, then do not focus on the sticky scroll
    /// because this will be followed by the revealing of a position.
    pub fn on_widget_mouse_down(&self) {
        self.on_mouse_down.set(true);
    }

    /// `CLICK` do nó do widget.
    pub fn on_widget_click(&self, mouse_event: &MouseEvent) {
        if mouse_event.ctrl_key || mouse_event.alt_key || mouse_event.meta_key {
            // modifier pressed
            return;
        }
        if !mouse_event.left_button {
            // not left click
            return;
        }
        let target = Some(mouse_event.target);
        if mouse_event.shift_key {
            // shift click
            let Some(line_index) = self.sticky_scroll_widget.get_line_index_from_child_element(target) else {
                return;
            };
            let Some(&end_line_number) = self.end_line_numbers.borrow().get(line_index) else {
                return;
            };
            self.reveal_line_in_center_if_outside_viewport(Position::new(end_line_number, 1));
            return;
        }
        let is_in_folding_icon_dom_node = self.sticky_scroll_widget.is_in_folding_icon_element(target);
        if is_in_folding_icon_dom_node {
            // clicked on folding icon
            let line_number = self.sticky_scroll_widget.get_line_number_from_child_element(target);
            self.toggle_folding_region_for_line(line_number);
            return;
        }
        let is_in_sticky_line = self.sticky_scroll_widget.is_in_sticky_line(target);
        if !is_in_sticky_line {
            return;
        }
        // normal click
        let mut position = self.sticky_scroll_widget.get_editor_position_from_node(target);
        if position.is_none() {
            let Some(line_number) = self.sticky_scroll_widget.get_line_number_from_child_element(target) else {
                // not hovering a sticky scroll line
                return;
            };
            position = Some(Position::new(line_number, 1));
        }
        if let Some(position) = position {
            self.reveal_position(position);
        }
    }

    /// `MOUSE_MOVE` da janela: `target` é o alvo do mouse se ele está dentro do widget, e `None` fora dele (só se guardam
    /// alvos dentro do widget para não reter nós soltos, #146841).
    pub fn on_window_mouse_move(&self, shift_key: bool, target: Option<ElementId>) {
        self.mouse_target.set(target);
        self.on_mouse_move_or_key_down(shift_key);
    }

    /// `KEY_DOWN` da janela.
    pub fn on_window_key_down(&self, shift_key: bool) {
        self.on_mouse_move_or_key_down(shift_key);
    }

    /// `KEY_UP` da janela.
    pub fn on_window_key_up(&self) {
        if self.show_end_for_line.get().is_some() {
            self.show_end_for_line.set(None);
            let _ = self.render_sticky_scroll(None);
        }
    }

    // #endregion

    fn on_mouse_move_or_key_down(&self, shift_key: bool) {
        if !shift_key {
            return;
        }
        let Some(mouse_target) = self.mouse_target.get() else {
            return;
        };
        let current_end_for_line_index = self.sticky_scroll_widget.get_line_index_from_child_element(Some(mouse_target));
        let Some(current_end_for_line_index) = current_end_for_line_index else {
            return;
        };
        if self.show_end_for_line.get() == Some(current_end_for_line_index) {
            return;
        }
        self.show_end_for_line.set(Some(current_end_for_line_index));
        let _ = self.render_sticky_scroll(None);
    }

    fn reveal_position(&self, position: Position) {
        self.reveal_in_editor(position, |editor| editor.reveal_position(position, None));
    }

    fn reveal_line_in_center_if_outside_viewport(&self, position: Position) {
        self.reveal_in_editor(position, |editor| editor.reveal_line_in_center_if_outside_viewport(position.line_number, Some(ScrollType::Smooth)));
    }

    /// `_reveaInEditor(position, revealFunction)`.
    fn reveal_in_editor(&self, position: Position, reveal_function: impl FnOnce(&Rc<CodeEditorWidget>)) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        if self.focused.get() {
            self.dispose_focus_sticky_scroll_store();
        }
        self.position_revealed.set(true);
        reveal_function(&editor);
        editor.set_selection(SelectionOrRange::from(Range::from_positions(position, position)), None);
        editor.focus();
    }

    /// `_registerMouseListeners()`.
    fn register_mouse_listeners(&self, editor: &Rc<CodeEditorWidget>) {
        let sticky_scroll_widget = Rc::clone(&self.sticky_scroll_widget);
        let gesture = ClickLinkGesture::new(
            editor,
            IClickLinkGestureOptions {
                extract_line_number_from_mouse_event: Some(Rc::new(move |e: &ClickLinkMouseEvent| {
                    let position = sticky_scroll_widget.get_editor_position_from_node(e.target.element());
                    position.map_or(0, |position| position.line_number)
                })),
            },
        );
        self.base.register(gesture.clone());

        self.base.register(listen_weak(
            &self.me,
            gesture.on_mouse_move_or_relevant_key_down(),
            |this: &StickyScrollController, (mouse_event, _keyboard_event): &(ClickLinkMouseEvent, Option<ClickLinkKeyboardEvent>)| {
                this.on_link_mouse_move_or_relevant_key_down(mouse_event);
            },
        ));
        self.base.register(listen_weak(&self.me, gesture.on_cancel(), |this: &StickyScrollController, _| {
            this.link_session_store.clear();
        }));
        self.base.register(listen_weak(&self.me, gesture.on_execute(), |this: &StickyScrollController, e: &ClickLinkMouseEvent| {
            this.on_link_execute(e);
        }));
    }

    /// `getMouseEventTarget(mouseEvent)`: o intervalo do texto sob o mouse e o span, se o mouse está sobre um `<span>` de
    /// texto de uma linha fixa.
    fn get_mouse_event_target(&self, mouse_event: &ClickLinkMouseEvent) -> Option<(Range, ElementId)> {
        let editor = self.editor.upgrade()?;
        if !editor.has_model() {
            return None;
        }
        // not hovering over our widget
        let target = self.overlay_widget_target(&mouse_event.target)?;
        let mouse_target_element = target.element?;
        let node = self.sticky_scroll_widget.get_node(mouse_target_element)?;
        if !node.inner_text_is_inner_html {
            // not on a span element rendering text
            return None;
        }
        // not hovering a sticky scroll line
        let position = self.sticky_scroll_widget.get_editor_position_from_node(Some(mouse_target_element))?;
        Some((
            Range::new(position.line_number, position.column, position.line_number, position.column + node.text.len() as i32),
            mouse_target_element,
        ))
    }

    /// O alvo do mouse, se for o widget de overlay do sticky scroll (`type === OVERLAY_WIDGET && detail === getId()`).
    fn overlay_widget_target<'a>(&self, target: &'a MouseTarget) -> Option<&'a MouseTargetOverlayWidget> {
        match target {
            MouseTarget::OverlayWidget(overlay_widget) if overlay_widget.detail == self.sticky_scroll_widget.get_id() => Some(overlay_widget),
            _ => None,
        }
    }

    /// O ouvinte de `gesture.onMouseMoveOrRelevantKeyDown`.
    fn on_link_mouse_move_or_relevant_key_down(&self, mouse_event: &ClickLinkMouseEvent) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let mouse_target = self.get_mouse_event_target(mouse_event);
        let Some((range, text_element)) = mouse_target.filter(|_| mouse_event.has_trigger_modifier && editor.has_model()) else {
            self.link_session_store.clear();
            return;
        };

        if self.sticky_range_projected_on_editor.get() != Some(range) {
            self.sticky_range_projected_on_editor.set(Some(range));
            self.link_session_store.clear();
        } else if self.sticky_scroll_widget.is_text_underlined(text_element) {
            return;
        }

        let cancellation_token = CancellationTokenSource::new(None);
        let to_cancel = cancellation_token.clone();
        self.link_session_store.add(to_disposable(move || to_cancel.dispose_with(true)));

        let Some(model) = editor.get_model() else {
            return;
        };
        let token = cancellation_token.token();
        let position = Position::new(range.start_line_number, range.start_column + 1);
        let (me, sticky_scroll_widget, link_session_store) =
            (self.me.clone(), Rc::clone(&self.sticky_scroll_widget), Rc::clone(&self.link_session_store));
        let _ = get_location_links_at_position(&self.services.language_features_service.definition_provider, &model, position, false, &token)
            .then(move |candidate_definitions| {
                if token.is_cancellation_requested() {
                    return;
                }
                if !candidate_definitions.is_empty() {
                    if let Some(this) = me.upgrade() {
                        this.candidate_definitions_length.set(candidate_definitions.len() as i32);
                    }
                    link_session_store.clear();
                    sticky_scroll_widget.set_text_underlined(text_element, true);
                    let sticky_scroll_widget = Rc::clone(&sticky_scroll_widget);
                    link_session_store.add(to_disposable(move || sticky_scroll_widget.set_text_underlined(text_element, false)));
                } else {
                    link_session_store.clear();
                }
            });
    }

    /// O ouvinte de `gesture.onExecute`.
    fn on_link_execute(&self, e: &ClickLinkMouseEvent) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(target) = self.overlay_widget_target(&e.target) else {
            // not hovering over our widget
            return;
        };
        let Some(position) = self.sticky_scroll_widget.get_editor_position_from_node(target.element) else {
            // not hovering a sticky scroll line
            return;
        };
        let (Some(model), Some(range)) = (editor.get_model(), self.sticky_range_projected_on_editor.get()) else {
            return;
        };
        if self.candidate_definitions_length.get() > 1 {
            if self.focused.get() {
                self.dispose_focus_sticky_scroll_store();
            }
            self.reveal_position(Position::new(position.line_number, 1));
        }
        let _ = self.services.definition_navigation.go_to_definition_with_location(e, &editor, &Location { uri: model.uri(), range });
    }

    /// `_toggleFoldingRegionForLine(line)`.
    fn toggle_folding_region_for_line(&self, line: Option<i32>) {
        let folding_model = self.folding_model.borrow().clone();
        let (Some(folding_model), Some(line)) = (folding_model, line) else {
            return;
        };
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let sticky_line = self.sticky_scroll_widget.get_rendered_sticky_line(line);
        let Some(sticky_line) = sticky_line else {
            return;
        };
        let Some(folding_icon) = sticky_line.folding_icon.as_ref() else {
            return;
        };
        toggle_collapse_state(&folding_model, 1, &[line as u32]);
        folding_icon.is_collapsed.set(!folding_icon.is_collapsed.get());
        let top_for_line_number = if folding_icon.is_collapsed.get() {
            editor.get_top_for_line_number(folding_icon.folding_end_line, None)
        } else {
            editor.get_top_for_line_number(folding_icon.folding_start_line, None)
        };
        let scroll_top = top_for_line_number - *editor.get_option(LINE_HEIGHT) * sticky_line.index as f64 + 1.0;
        editor.set_scroll_top(scroll_top, None);
        let _ = self.render_sticky_scroll(Some(line as usize));
    }

    /// `_readConfiguration()`.
    fn read_configuration(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let options = *editor.get_option(STICKY_SCROLL);
        let overlay_widget: Rc<dyn IOverlayWidget> = self.sticky_scroll_widget.clone();
        if !options.enabled {
            editor.remove_overlay_widget(&overlay_widget);
            self.reset_state();
            self.session_store.clear();
            self.enabled.set(false);
            return;
        } else if !self.enabled.get() {
            // When sticky scroll was just enabled, add the listeners on the sticky scroll
            editor.add_overlay_widget(overlay_widget);
            self.session_store.add(listen_weak(&self.me, editor.on_did_scroll_change(), |this: &StickyScrollController, e| {
                if e.scroll_top_changed {
                    this.show_end_for_line.set(None);
                    let _ = this.render_sticky_scroll(None);
                }
            }));
            self.session_store.add(listen_weak(&self.me, editor.on_did_layout_change(), |this: &StickyScrollController, _| this.on_did_resize()));
            self.session_store.add(listen_weak(&self.me, editor.on_did_change_model_tokens(), |this: &StickyScrollController, e| {
                this.on_tokens_change(e);
            }));
            self.session_store.add(listen_weak(
                &self.me,
                self.sticky_line_candidate_provider.on_did_change_sticky_scroll(),
                |this: &StickyScrollController, _| {
                    this.show_end_for_line.set(None);
                    let _ = this.render_sticky_scroll(None);
                },
            ));
            self.enabled.set(true);
        }

        let line_number_option = editor.get_option(LINE_NUMBERS).render_type;
        if line_number_option == RenderLineNumbersType::Relative {
            if self.cursor_position_listener.borrow().is_none() {
                let listener = listen_weak(&self.me, editor.on_did_change_cursor_position(), |this: &StickyScrollController, e| {
                    if this.position_line_number.get() == Some(e.position.line_number) {
                        return;
                    }
                    this.position_line_number.set(Some(e.position.line_number));
                    this.show_end_for_line.set(None);
                    let _ = this.render_sticky_scroll(Some(0));
                });
                *self.cursor_position_listener.borrow_mut() = Some(listener.clone());
                self.session_store.add(listener);
            }
        } else {
            let listener = self.cursor_position_listener.borrow_mut().take();
            if let Some(listener) = listener {
                self.session_store.delete(&listener);
            }
        }
    }

    /// `_readConfigurationChange(event)`.
    fn read_configuration_change(&self, event: &crate::config::editor_options::ConfigurationChangedEvent) {
        if event.has_changed(EditorOption::StickyScroll)
            || event.has_changed(EditorOption::Minimap)
            || event.has_changed(EditorOption::LineHeight)
            || event.has_changed(EditorOption::ShowFoldingControls)
            || event.has_changed(EditorOption::LineNumbers)
        {
            self.read_configuration();
        }

        if event.has_changed(EditorOption::LineNumbers)
            || event.has_changed(EditorOption::Folding)
            || event.has_changed(EditorOption::ShowFoldingControls)
            || event.has_changed(EditorOption::FontInfo)
            || event.has_changed(EditorOption::EffectiveFullwidthCharacterWidth)
        {
            let _ = self.render_sticky_scroll(Some(0));
        }
    }

    /// `_needsUpdate(event)`.
    fn needs_update(&self, event: &ModelTokensChangedEvent) -> bool {
        let sticky_line_numbers = self.sticky_scroll_widget.line_numbers();
        for sticky_line_number in sticky_line_numbers {
            for range in &event.ranges {
                if sticky_line_number >= range.from_line_number && sticky_line_number <= range.to_line_number {
                    return true;
                }
            }
        }
        false
    }

    /// `_onTokensChange(event)`.
    fn on_tokens_change(&self, event: &ModelTokensChangedEvent) {
        if self.needs_update(event) {
            // Rebuilding the whole widget from line 0
            let _ = self.render_sticky_scroll(Some(0));
        }
    }

    /// `_onDidResize()`.
    fn on_did_resize(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let layout_info = editor.get_layout_info();
        // Make sure sticky scroll doesn't take up more than 25% of the editor
        let theoretical_lines = layout_info.height / *editor.get_option(LINE_HEIGHT);
        self.max_sticky_lines.set((theoretical_lines * 0.25).round());
        let _ = self.render_sticky_scroll(Some(0));
    }

    /// `_renderStickyScroll(rebuildFromLine)`.
    fn render_sticky_scroll(&self, rebuild_from_line: Option<usize>) -> Promise<()> {
        let me = self.me.clone();
        async_fn(async move {
            let Some(this) = me.upgrade() else {
                return Ok(());
            };
            let Some(next_rebuild_from_line) = this.prepare_render(rebuild_from_line) else {
                return Ok(());
            };
            if !this.focused.get() {
                let update = this.update_state(next_rebuild_from_line);
                drop(this);
                update.await?;
                return Ok(());
            }
            // Suppose that previously the sticky scroll widget had height 0, then if there are visible lines, set the last line as focused
            if this.focused_sticky_element_index.get() == -1 {
                let update = this.update_state(next_rebuild_from_line);
                drop(this);
                update.await?;
                let Some(this) = me.upgrade() else {
                    return Ok(());
                };
                this.focused_sticky_element_index.set(this.sticky_scroll_widget.line_number_count() as i64 - 1);
                if this.focused_sticky_element_index.get() != -1 {
                    this.sticky_scroll_widget.focus_line_with_index(this.focused_sticky_element_index.get());
                }
            } else {
                let focused_index = usize::try_from(this.focused_sticky_element_index.get()).ok();
                let focused_sticky_element_line_number =
                    focused_index.and_then(|index| this.sticky_scroll_widget.line_numbers().get(index).copied());
                let update = this.update_state(next_rebuild_from_line);
                drop(this);
                update.await?;
                let Some(this) = me.upgrade() else {
                    return Ok(());
                };
                // Suppose that after setting the state, there are no sticky lines, set the focused index to -1
                if this.sticky_scroll_widget.line_number_count() == 0 {
                    this.focused_sticky_element_index.set(-1);
                } else {
                    let previous_focused_line_number_exists = focused_sticky_element_line_number
                        .is_some_and(|line_number| this.sticky_scroll_widget.line_numbers().contains(&line_number));

                    // If the line number is still there, do not change anything
                    // If the line number is not there, set the new focused line to be the last line
                    if !previous_focused_line_number_exists {
                        this.focused_sticky_element_index.set(this.sticky_scroll_widget.line_number_count() as i64 - 1);
                    }
                    this.sticky_scroll_widget.focus_line_with_index(this.focused_sticky_element_index.get());
                }
            }
            Ok(())
        })
    }

    /// O começo síncrono de `_renderStickyScroll`, até a decisão de atualizar o estado: devolve o `nextRebuildFromLine` se
    /// o estado deve ser atualizado, e `None` se o modelo some ou é grande demais (o estado é zerado) ou se a versão do
    /// modelo de escopos não é a do documento.
    fn prepare_render(&self, rebuild_from_line: Option<usize>) -> Option<Option<usize>> {
        let model = self.editor.upgrade()?.get_model();
        let Some(model) = model.filter(|model| !model.is_too_large_for_tokenization()) else {
            self.reset_state();
            return None;
        };
        let next_rebuild_from_line = self.update_and_get_min_rebuild_from_line(rebuild_from_line);
        let sticky_widget_version = self.sticky_line_candidate_provider.get_version_id();
        let should_update_state = sticky_widget_version.is_none() || sticky_widget_version == Some(model.get_version_id());
        should_update_state.then_some(next_rebuild_from_line)
    }

    /// `_updateAndGetMinRebuildFromLine(rebuildFromLine)`.
    fn update_and_get_min_rebuild_from_line(&self, rebuild_from_line: Option<usize>) -> Option<usize> {
        if let Some(rebuild_from_line) = rebuild_from_line {
            let min_rebuild_from_line_or_infinity = self.min_rebuild_from_line.get().unwrap_or(usize::MAX);
            self.min_rebuild_from_line.set(Some(rebuild_from_line.min(min_rebuild_from_line_or_infinity)));
        }
        self.min_rebuild_from_line.get()
    }

    /// `_updateState(rebuildFromLine)`.
    fn update_state(&self, rebuild_from_line: Option<usize>) -> Promise<()> {
        self.min_rebuild_from_line.set(None);
        let folding_model = self
            .editor
            .upgrade()
            .and_then(|editor| FoldingController::get(&editor))
            .and_then(|folding_controller| folding_controller.get_folding_model());
        let folding_model = folding_model.map_or(MaybePromise::Value(None), MaybePromise::Promise);
        let me = self.me.clone();
        async_fn(async move {
            let folding_model = await_maybe(folding_model).await?;
            let Some(this) = me.upgrade() else {
                return Ok(());
            };
            *this.folding_model.borrow_mut() = folding_model.clone();
            let widget_state = this.find_scroll_widget_state();
            let sticky_widget_has_lines = !widget_state.start_line_numbers.is_empty();
            *this.widget_state.borrow_mut() = widget_state.clone();
            this.sticky_scroll_visible_context_key.set(Dynamic::Bool(sticky_widget_has_lines));
            this.sticky_scroll_widget.set_state(Some(widget_state), folding_model, rebuild_from_line);
            Ok(())
        })
    }

    /// `_resetState()`.
    fn reset_state(&self) {
        self.min_rebuild_from_line.set(None);
        *self.folding_model.borrow_mut() = None;
        *self.widget_state.borrow_mut() = StickyScrollWidgetState::empty();
        self.sticky_scroll_visible_context_key.set(Dynamic::Bool(false));
        self.sticky_scroll_widget.set_state(None, None, None);
    }

    /// `_focusNav(direction)`: `true` é o próximo, `false` o anterior.
    fn focus_nav(&self, direction: bool) {
        let index = self.focused_sticky_element_index.get() + if direction { 1 } else { -1 };
        self.focused_sticky_element_index.set(index);
        self.sticky_scroll_widget.focus_line_with_index(index);
    }
}

impl IStickyScrollController for StickyScrollController {
    fn sticky_scroll_candidate_provider(&self) -> Rc<dyn IStickyLineCandidateProvider> {
        Rc::clone(&self.sticky_line_candidate_provider)
    }

    fn sticky_scroll_widget_state(&self) -> StickyScrollWidgetState {
        self.widget_state.borrow().clone()
    }

    fn sticky_scroll_widget_height(&self) -> f64 {
        self.sticky_scroll_widget.height()
    }

    fn is_focused(&self) -> bool {
        self.focused.get()
    }

    fn focus(&self) {
        // If the mouse is down, do not focus on the sticky scroll
        if self.on_mouse_down.get() {
            self.on_mouse_down.set(false);
            if let Some(editor) = self.editor.upgrade() {
                editor.focus();
            }
            return;
        }
        let focus_state = self.sticky_scroll_focused_context_key.get();
        if matches!(focus_state, Dynamic::Bool(true)) {
            return;
        }
        self.focused.set(true);
        *self.focus_disposable_store.borrow_mut() = Some(DisposableStore::new());
        self.sticky_scroll_focused_context_key.set(Dynamic::Bool(true));
        self.focused_sticky_element_index.set(self.sticky_scroll_widget.line_number_count() as i64 - 1);
        self.sticky_scroll_widget.focus_line_with_index(self.focused_sticky_element_index.get());
    }

    fn focus_next(&self) {
        if self.focused_sticky_element_index.get() < self.sticky_scroll_widget.line_number_count() as i64 - 1 {
            self.focus_nav(true);
        }
    }

    fn focus_previous(&self) {
        if self.focused_sticky_element_index.get() > 0 {
            self.focus_nav(false);
        }
    }

    fn go_to_focused(&self) {
        let line_numbers = self.sticky_scroll_widget.line_numbers();
        self.dispose_focus_sticky_scroll_store();
        let focused_line_number =
            usize::try_from(self.focused_sticky_element_index.get()).ok().and_then(|index| line_numbers.get(index).copied());
        if let Some(line_number) = focused_line_number {
            self.reveal_position(Position::new(line_number, 1));
        }
    }

    fn find_scroll_widget_state(&self) -> StickyScrollWidgetState {
        let Some(editor) = self.editor.upgrade() else {
            return StickyScrollWidgetState::empty();
        };
        let max_number_sticky_lines = js_min(self.max_sticky_lines.get(), editor.get_option(STICKY_SCROLL).max_line_count);
        let scroll_top = editor.get_scroll_top();
        let mut last_line_relative_position = 0.0;
        let mut start_line_numbers: Vec<i32> = Vec::new();
        let mut end_line_numbers: Vec<i32> = Vec::new();
        let array_visible_ranges = editor.get_visible_ranges();
        if let (Some(first), Some(last)) = (array_visible_ranges.first(), array_visible_ranges.last()) {
            let full_visible_range = StickyRange::new(first.start_line_number, last.end_line_number);
            let candidate_ranges = self.sticky_line_candidate_provider.get_candidate_sticky_lines_intersecting(full_visible_range);
            for range in candidate_ranges {
                let start = range.start_line_number;
                let end = range.end_line_number;
                let top_of_element = range.top;
                let bottom_of_element = top_of_element + range.height;
                let top_of_beginning_line = editor.get_top_for_line_number(start, None) - scroll_top;
                let bottom_of_end_line = editor.get_bottom_for_line_number(end, None) - scroll_top;
                if top_of_element > top_of_beginning_line && top_of_element <= bottom_of_end_line {
                    start_line_numbers.push(start);
                    end_line_numbers.push(end + 1);
                    if bottom_of_element > bottom_of_end_line {
                        last_line_relative_position = bottom_of_end_line - bottom_of_element;
                    }
                }
                if start_line_numbers.len() as f64 == max_number_sticky_lines {
                    break;
                }
            }
        }
        *self.end_line_numbers.borrow_mut() = end_line_numbers.clone();
        StickyScrollWidgetState::new(start_line_numbers, end_line_numbers, last_line_relative_position, self.show_end_for_line.get())
    }

    fn select_editor(&self) {
        if let Some(editor) = self.editor.upgrade() {
            editor.focus();
        }
    }

    fn on_did_change_sticky_scroll_height(&self) -> Event<StickyScrollHeightChange> {
        self.on_did_change_sticky_scroll_height.event()
    }
}

impl IDisposable for StickyScrollController {
    fn dispose(&self) {
        self.base.dispose();
        self.session_store.dispose();
    }
}

impl IEditorContribution for StickyScrollController {}
