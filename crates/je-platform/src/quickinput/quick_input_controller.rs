//! Porte de `src/vs/platform/quickinput/browser/quickInputController.ts`: o `QuickInputController`, que monta o widget do quick
//! input (título, barras de ação, caixa, contagens, mensagem, lista), cria os controladores (`createQuickPick`,
//! `createInputBox`, `createQuickWidget`), mostra e esconde com a animação de fechamento, posiciona o widget (topo, âncora,
//! arrastar), e oferece `pick` e `input`. Inclui o `QuickInputDragAndDropController` (o estado e o cálculo de encaixe).
//!
//! Notas de tradução:
//! - **DOM.** O widget é um conjunto de [`ViewElement`] (contêiner, barra de título, descrições, contêineres de contagem, botões e
//!   mensagem), com `display` pelos `show`/`hide` e a geometria (`left`, `top`, `right`, `bottom`, `width`) escrita no estilo e
//!   no retângulo numérico do elemento. Os textos, as contagens, as ações, a mensagem e o estado dos botões ficam em
//!   [`QuickInputUi`], que o desenho nativo lê.
//! - **Medição.** `filterContainer.getBoundingClientRect()` (usado só por `alignOverlayInput`) é calculado do retângulo do widget:
//!   o `.quick-input-header` tem `padding: 6px 6px 4px 6px` e a caixa tem a altura de `setHeight` (a de 26 do
//!   `verticalPadding` quando não há). A altura da barra de título não foi medida e entra como [`QuickInputUi::set_title_bar_height`]
//!   (0 por padrão).
//! - **Animação de fechamento.** O CSS dá `animation` ao widget só com `.modern-ui.monaco-enable-motion` num ancestral e fora do
//!   modo `overlay`; `style.animation = 'none'` (propriedade do estilo do widget) a suprime. [`QuickInputCloseAnimation`] é o
//!   `Animation` do Web Animations: `finish()` e `cancel()` disparam os eventos `finish` e `cancel` que o controlador escuta.
//!   `inert` é [`QuickInputController::widget_inert`].
//! - **Foco e teclado.** Os ouvintes de DOM (`focus`, `blur`, `keydown` e `keyup` da janela, cliques na barra) viram os métodos
//!   [`QuickInputController::handle_focus_in`], [`QuickInputController::handle_blur`], [`QuickInputController::update_key_mods`]
//!   e `QuickInputBox::press_key`, que o desenho nativo chama quando recebe o evento. `previousFocusElement` é o fecho que
//!   devolve o foco ao elemento anterior (e diz se ele ainda está visível).
//! - **`backButton.tooltip`.** O botão é compartilhado e imutável aqui; o tooltip que o original escreve nele fica em
//!   [`QuickInputController::back_button_tooltip`].
//! - **`pick` e `input`.** `pick` devolve a lista (`pick_many`) ou o item (`pick`); a `Promise` do original é a de
//!   `je_base::promise`. O `Event.debounce` de 100 ms do `input` é o de `je_base::event`.
//! - **Aux windows.** O `ILayoutService` daqui tem um contêiner só, então `reparentUI` por janela não existe; `reparent_ui` só
//!   troca o contêiner (o `onWillUnregisterWindow` é do chamador).
//! - **Fica de fora** o `QuickTree` e a árvore (`QuickInputTreeController`), que têm porte próprio.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::async_::{Sequencer, disposable_timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{DebounceOptions, Emitter, Event};
use je_base::layout::{AnchorAlignment, AnchorPosition, IAnchor, ILayout2DOptions, IRect, ISize, layout2d};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::severity::Severity;

use super::picker_quick_access::same_item;
use super::quick_input::{
    IInputBox, IInputOptions, IKeyMods, IPickOptions, IQuickInput, IQuickNavigateConfiguration, IQuickPick, IQuickPickItem,
    IQuickPickItemButtonContext, InputValidationResult, PickActiveItem, QuickInputAlignment, QuickInputAlignmentSetting, QuickInputAnchor,
    QuickInputAnchorPosition, QuickInputButtonRef, QuickInputHideReason, QuickInputRef, QuickInputType, QuickPickFocus, QuickPickInput, QuickPicks,
};
use super::quick_input_box::{QuickInputBox, QuickInputBoxStyles, QuickInputFocus, QuickInputFocusTarget};
use super::quick_input_browser::{
    IQuickInputBoxUi, IQuickInputListUi, IQuickInputUi, InputBox, QuickInputKeyUp, QuickInputMessageDecoration, QuickPick,
    QuickWidget, Visibilities, back_button, end_of_quick_input_box_context_key, in_quick_input_context_key, quick_input_alignment_context_key,
    quick_input_type_context_key,
};
use super::quick_input_list::QuickInputList;
use crate::contextkey::contextkey::{IContextKey, IContextKeyService};
use crate::contextview::context_view::{AnchorTarget, get_anchor_rect};
use crate::contextview::view_element::ViewElement;
use crate::layout::layout_service::ILayoutService;
use crate::storage::storage::{StorageService, StorageScope, StorageTarget};

const VIEWSTATE_STORAGE_KEY: &str = "workbench.quickInput.viewState";
const QUICK_INPUT_MOTION_CLOSING_CLASS: &str = "quick-input-widget-closing";
const QUICK_INPUT_OVERLAY_CLASS: &str = "quick-input-widget-overlay";
const QUICK_INPUT_CLOSE_ANIMATION_DURATION: u64 = 150;
const QUICK_INPUT_MOTION_ANCESTOR_CLASSES: [&str; 2] = ["modern-ui", "monaco-enable-motion"];
const MAX_WIDTH: f64 = 600.0;
/// `.quick-input-header { padding: 6px 6px 4px 6px }`.
const HEADER_PADDING_TOP: f64 = 6.0;
const HEADER_PADDING_LEFT: f64 = 6.0;
/// A altura do `.monaco-inputbox` do quick input (o `26` do `verticalPadding`).
const INPUT_BOX_HEIGHT: f64 = 26.0;

// ---------------------------------------------------------------------------------------------
// Opções e estilos.
// ---------------------------------------------------------------------------------------------

/// `IQuickInputStyles.widget`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuickInputWidgetStyles {
    pub quick_input_background: Option<String>,
    pub quick_input_foreground: Option<String>,
    pub quick_input_title_background: Option<String>,
    pub widget_border: Option<String>,
    pub widget_shadow: Option<String>,
}

/// `IQuickInputStyles`, do que o widget nativo usa (botão, toggle, contagem e barra de progresso são do desenho).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IQuickInputStyles {
    pub input_box: QuickInputBoxStyles,
    pub widget: QuickInputWidgetStyles,
    pub picker_group_border: Option<String>,
    pub picker_group_foreground: Option<String>,
}

/// `IQuickInputOptions`.
#[derive(Clone)]
pub struct IQuickInputOptions {
    pub container: Rc<ViewElement>,
    pub id_prefix: String,
    pub ignore_focus_out: Rc<dyn Fn() -> bool>,
    pub return_focus: Rc<dyn Fn()>,
    pub back_keybinding_label: Rc<dyn Fn() -> Option<String>>,
    pub set_context_key: Rc<dyn Fn(Option<&str>)>,
    pub link_opener_delegate: Rc<dyn Fn(&str)>,
    pub styles: IQuickInputStyles,
}

/// `QuickInputViewState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuickInputViewState {
    pub top: Option<f64>,
    pub left: Option<f64>,
}

/// O `{ top?, left?, done }` de `dndViewState`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DndViewState {
    top: Option<f64>,
    left: Option<f64>,
    done: bool,
}

#[derive(Clone, Copy, Debug)]
struct OverlayLayoutCorrection {
    anchor: IRect,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    width: f64,
}

/// `style.left/right/top/bottom/width` do widget (`None` é `'initial'` ou `''`).
#[derive(Clone, Copy, Debug, Default)]
struct Geometry {
    width: f64,
    left: Option<f64>,
    right: Option<f64>,
    top: Option<f64>,
    bottom: Option<f64>,
}

struct AnchorScope {
    anchor: Option<QuickInputAnchor>,
    anchor_position: Option<QuickInputAnchorPosition>,
    consumed: Cell<bool>,
}

/// O resultado de `pick`: os itens escolhidos (um só no modo simples).
type PickResult = Option<Vec<IQuickPickItem>>;

/// Quem o controlador criou, para o `instanceof` e o `currentQuickInput`.
enum Created {
    Pick(Weak<QuickPick>),
    Input(Weak<InputBox>),
}

fn same_object<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::eq(Rc::as_ptr(a) as *const (), Rc::as_ptr(b) as *const ())
}

fn toggle_display(element: &ViewElement, visible: bool) {
    if visible {
        element.show();
    } else {
        element.hide();
    }
}

/// `dom.hasParentWithClass(container, ['modern-ui', 'monaco-enable-motion'])`: algum ancestral tem todas as classes.
fn has_motion_ancestor(element: &Rc<ViewElement>) -> bool {
    let mut candidate = element.parent_element();
    while let Some(current) = candidate {
        if QUICK_INPUT_MOTION_ANCESTOR_CLASSES.iter().all(|name| current.class_list_contains(name)) {
            return true;
        }
        candidate = current.parent_element();
    }
    false
}

// ---------------------------------------------------------------------------------------------
// A animação de fechamento.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimationState {
    Running,
    Finished,
    Cancelled,
}

/// O `Animation` do fechamento do widget (`container.getAnimations()[0]`).
pub struct QuickInputCloseAnimation {
    state: Cell<AnimationState>,
    on_finish: Rc<Emitter<()>>,
    on_cancel: Rc<Emitter<()>>,
}

impl QuickInputCloseAnimation {
    fn new() -> Rc<Self> {
        Rc::new(Self { state: Cell::new(AnimationState::Running), on_finish: Emitter::new(), on_cancel: Emitter::new() })
    }

    /// `animation.finish()`.
    pub fn finish(&self) {
        if self.state.get() == AnimationState::Running {
            self.state.set(AnimationState::Finished);
            self.on_finish.fire(());
        }
    }

    /// `animation.cancel()` (o que o navegador faz quando o modo de movimento reduzido liga).
    pub fn cancel(&self) {
        if self.state.get() == AnimationState::Running {
            self.state.set(AnimationState::Cancelled);
            self.on_cancel.fire(());
        }
    }

    /// Se a animação ainda está em curso (conta em `getAnimations()`).
    pub fn is_running(&self) -> bool {
        self.state.get() == AnimationState::Running
    }

    /// O evento `finish` da animação.
    pub fn on_finish(&self) -> Event<()> {
        self.on_finish.event()
    }

    /// O evento `cancel` da animação.
    pub fn on_cancel(&self) -> Event<()> {
        self.on_cancel.event()
    }

    /// Solta os emissores (a animação acabou de ser descartada pelo controlador).
    fn dispose(&self) {
        self.on_finish.dispose();
        self.on_cancel.dispose();
    }
}

// ---------------------------------------------------------------------------------------------
// O que o controlador entrega aos controladores (`QuickInputUI`).
// ---------------------------------------------------------------------------------------------

/// `QuickInputUI`, nativo: os elementos, os widgets e o estado visível do quick input.
pub struct QuickInputUi {
    controller: Weak<QuickInputController>,
    id_prefix: String,
    container: Rc<ViewElement>,
    title_bar: Rc<ViewElement>,
    title: Rc<ViewElement>,
    description1: Rc<ViewElement>,
    description2: Rc<ViewElement>,
    check_all: Rc<ViewElement>,
    input_container: Rc<ViewElement>,
    filter_container: Rc<ViewElement>,
    visible_count_container: Rc<ViewElement>,
    count_container: Rc<ViewElement>,
    ok_container: Rc<ViewElement>,
    custom_button_container: Rc<ViewElement>,
    message: Rc<ViewElement>,
    progress_bar: Rc<ViewElement>,
    html_widget: Rc<ViewElement>,
    html_widget_content: RefCell<Option<Rc<ViewElement>>>,
    focus: Rc<QuickInputFocus>,
    input_box: Rc<QuickInputBox>,
    list: Rc<QuickInputList>,
    title_text: RefCell<String>,
    description_text: RefCell<String>,
    visible_count: Cell<usize>,
    count: Cell<usize>,
    check_all_checked: Cell<bool>,
    check_all_enabled: Cell<bool>,
    left_actions: RefCell<(Vec<ActionRef>, Vec<ActionRef>)>,
    right_actions: RefCell<(Vec<ActionRef>, Vec<ActionRef>)>,
    inline_actions: RefCell<(Vec<ActionRef>, Vec<ActionRef>)>,
    count_container_right: RefCell<String>,
    rendered_message: RefCell<Option<String>>,
    message_decoration: RefCell<QuickInputMessageDecoration>,
    progress_infinite: Cell<bool>,
    progress_aria_hidden: Cell<bool>,
    ok_enabled: Cell<bool>,
    ok_label: RefCell<String>,
    custom_button_label: RefCell<String>,
    custom_button_hover: RefCell<String>,
    custom_button_secondary: Cell<bool>,
    ignore_focus_out: Cell<bool>,
    key_mods: Cell<IKeyMods>,
    context_key: RefCell<Option<String>>,
    title_bar_height: Cell<f64>,
    inert: Cell<bool>,
    on_did_accept: Rc<Emitter<()>>,
    on_did_custom: Rc<Emitter<()>>,
    on_did_trigger_button: Rc<Emitter<QuickInputButtonRef>>,
    on_container_key_up: Rc<Emitter<QuickInputKeyUp>>,
}

impl QuickInputUi {
    /// O prefixo dos ids do widget (`idPrefix`).
    pub fn id_prefix(&self) -> &str {
        &self.id_prefix
    }
    pub fn title_bar(&self) -> &Rc<ViewElement> {
        &self.title_bar
    }
    pub fn title(&self) -> &Rc<ViewElement> {
        &self.title
    }
    pub fn description1(&self) -> &Rc<ViewElement> {
        &self.description1
    }
    pub fn description2(&self) -> &Rc<ViewElement> {
        &self.description2
    }
    pub fn check_all(&self) -> &Rc<ViewElement> {
        &self.check_all
    }
    pub fn input_container(&self) -> &Rc<ViewElement> {
        &self.input_container
    }
    pub fn filter_container(&self) -> &Rc<ViewElement> {
        &self.filter_container
    }
    pub fn visible_count_container(&self) -> &Rc<ViewElement> {
        &self.visible_count_container
    }
    pub fn count_container(&self) -> &Rc<ViewElement> {
        &self.count_container
    }
    pub fn ok_container(&self) -> &Rc<ViewElement> {
        &self.ok_container
    }
    pub fn custom_button_container(&self) -> &Rc<ViewElement> {
        &self.custom_button_container
    }
    pub fn message_element(&self) -> &Rc<ViewElement> {
        &self.message
    }
    pub fn progress_bar(&self) -> &Rc<ViewElement> {
        &self.progress_bar
    }
    pub fn html_widget(&self) -> &Rc<ViewElement> {
        &self.html_widget
    }
    pub fn input_box_widget(&self) -> &Rc<QuickInputBox> {
        &self.input_box
    }
    pub fn list_widget(&self) -> &Rc<QuickInputList> {
        &self.list
    }
    pub fn focus_state(&self) -> &Rc<QuickInputFocus> {
        &self.focus
    }
    pub fn visible_count(&self) -> usize {
        self.visible_count.get()
    }
    pub fn count(&self) -> usize {
        self.count.get()
    }
    pub fn check_all_checked(&self) -> bool {
        self.check_all_checked.get()
    }
    pub fn check_all_enabled(&self) -> bool {
        self.check_all_enabled.get()
    }
    pub fn left_actions(&self) -> (Vec<ActionRef>, Vec<ActionRef>) {
        self.left_actions.borrow().clone()
    }
    pub fn right_actions(&self) -> (Vec<ActionRef>, Vec<ActionRef>) {
        self.right_actions.borrow().clone()
    }
    pub fn inline_actions(&self) -> (Vec<ActionRef>, Vec<ActionRef>) {
        self.inline_actions.borrow().clone()
    }
    pub fn count_container_right_value(&self) -> String {
        self.count_container_right.borrow().clone()
    }
    pub fn rendered_message(&self) -> Option<String> {
        self.rendered_message.borrow().clone()
    }
    pub fn message_decoration(&self) -> QuickInputMessageDecoration {
        self.message_decoration.borrow().clone()
    }
    pub fn progress_is_infinite(&self) -> bool {
        self.progress_infinite.get()
    }
    pub fn progress_aria_hidden(&self) -> bool {
        self.progress_aria_hidden.get()
    }
    pub fn ok_enabled(&self) -> bool {
        self.ok_enabled.get()
    }
    pub fn ok_label(&self) -> String {
        self.ok_label.borrow().clone()
    }
    pub fn custom_button_label(&self) -> String {
        self.custom_button_label.borrow().clone()
    }
    pub fn custom_button_hover(&self) -> String {
        self.custom_button_hover.borrow().clone()
    }
    pub fn custom_button_secondary(&self) -> bool {
        self.custom_button_secondary.get()
    }
    pub fn ignore_focus_out(&self) -> bool {
        self.ignore_focus_out.get()
    }
    pub fn context_key(&self) -> Option<String> {
        self.context_key.borrow().clone()
    }

    /// A altura da barra de título quando visível (não medida: 0 até o desenho informar).
    pub fn set_title_bar_height(&self, height: f64) {
        self.title_bar_height.set(height);
    }

    pub fn set_key_mods(&self, key_mods: IKeyMods) {
        self.key_mods.set(key_mods);
    }

    /// Os cliques que o desenho nativo entrega: `ok.onDidClick`, `customButton.onDidClick`, o botão de título e o `keyup`.
    pub fn click_ok(&self) {
        if self.ok_enabled.get() {
            self.on_did_accept.fire(());
        }
    }

    pub fn click_custom_button(&self) {
        self.on_did_custom.fire(());
    }

    pub fn trigger_button(&self, button: QuickInputButtonRef) {
        self.on_did_trigger_button.fire(button);
    }

    pub fn key_up(&self, event: QuickInputKeyUp) {
        self.on_container_key_up.fire(event);
    }

    /// `checkAll.onChange`: marca ou desmarca tudo que está visível na lista.
    pub fn set_check_all(&self, checked: bool) {
        self.check_all_checked.set(checked);
        self.list.set_all_visible_checked(checked);
    }

    /// O retângulo da caixa de filtro (`filterContainer.getBoundingClientRect()`), medido do retângulo do widget.
    pub fn filter_container_rect(&self) -> IRect {
        let widget = self.container.rect();
        let title = if self.title_bar.is_hidden() { 0.0 } else { self.title_bar_height.get() };
        IRect {
            top: widget.top + title + HEADER_PADDING_TOP,
            left: widget.left + HEADER_PADDING_LEFT,
            width: widget.width - 2.0 * HEADER_PADDING_LEFT,
            height: self.input_box.height().unwrap_or(INPUT_BOX_HEIGHT),
        }
    }

    fn controller(&self) -> Rc<QuickInputController> {
        self.controller.upgrade().expect("o QuickInputController vive enquanto o widget é usado")
    }

    /// O trecho de `show()` que zera o widget antes de mostrar (de `leftActionBar.setActions([])` a `inputBox.setHeight`).
    fn reset_for_show(&self) {
        *self.left_actions.borrow_mut() = (Vec::new(), Vec::new());
        self.title_text.borrow_mut().clear();
        self.description_text.borrow_mut().clear();
        *self.html_widget_content.borrow_mut() = None;
        *self.right_actions.borrow_mut() = (Vec::new(), Vec::new());
        *self.inline_actions.borrow_mut() = (Vec::new(), Vec::new());
        self.check_all_checked.set(false);
        // ui.inputBox.value = ''; Avoid triggering an event.
        self.input_box.set_placeholder("");
        self.input_box.set_password(false);
        self.input_box.show_decoration(Severity::Ignore);
        self.visible_count.set(0);
        self.count.set(0);
        *self.count_container_right.borrow_mut() = "4px".to_owned();
        *self.rendered_message.borrow_mut() = None;
        self.progress_infinite.set(false);
        self.progress_aria_hidden.set(true);
        self.list.set_elements(&[]);
        self.list.set_match_on_description(false);
        self.list.set_match_on_detail(false);
        self.list.set_match_on_label(true);
        self.list.set_sort_by_label(true);
        self.ignore_focus_out.set(false);
        self.input_box.clear_toggles();
        self.input_box.clear_actions();
        self.input_box.set_height(None);
    }

    fn set_enabled_widgets(&self, enabled: bool) {
        for (primary, secondary) in [self.left_actions.borrow().clone(), self.right_actions.borrow().clone()] {
            for action in primary.iter().chain(secondary.iter()) {
                action.set_enabled(enabled);
            }
        }
        self.check_all_enabled.set(enabled);
        self.input_box.set_enabled(enabled);
        self.ok_enabled.set(enabled);
        self.list.set_enabled(enabled);
    }
}

impl IQuickInputUi for QuickInputUi {
    fn container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }
    fn input_box(&self) -> Rc<dyn IQuickInputBoxUi> {
        self.input_box.clone()
    }
    fn list(&self) -> Rc<dyn IQuickInputListUi> {
        self.list.clone()
    }
    fn title_text(&self) -> String {
        self.title_text.borrow().clone()
    }
    fn set_title_text(&self, text: &str) {
        *self.title_text.borrow_mut() = text.to_owned();
    }
    fn description_text(&self) -> String {
        self.description_text.borrow().clone()
    }
    fn set_description_text(&self, text: &str) {
        *self.description_text.borrow_mut() = text.to_owned();
    }
    fn show_progress_infinite(&self) {
        self.progress_infinite.set(true);
        self.progress_aria_hidden.set(false);
    }
    fn stop_progress(&self) {
        self.progress_infinite.set(false);
        self.progress_aria_hidden.set(true);
    }
    fn set_left_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>) {
        *self.left_actions.borrow_mut() = (primary, secondary);
    }
    fn set_right_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>) {
        *self.right_actions.borrow_mut() = (primary, secondary);
    }
    fn set_inline_actions(&self, primary: Vec<ActionRef>, secondary: Vec<ActionRef>) {
        *self.inline_actions.borrow_mut() = (primary, secondary);
    }
    fn set_count_container_right(&self, right: &str) {
        *self.count_container_right.borrow_mut() = right.to_owned();
    }
    fn reset_message(&self) {
        *self.rendered_message.borrow_mut() = None;
    }
    fn render_message(&self, message: &str, _disposables: &DisposableStore) {
        *self.rendered_message.borrow_mut() = Some(message.to_owned());
    }
    fn set_message_decoration(&self, decoration: &QuickInputMessageDecoration) {
        *self.message_decoration.borrow_mut() = decoration.clone();
    }
    fn set_ok_label(&self, label: &str) {
        *self.ok_label.borrow_mut() = label.to_owned();
    }
    fn set_custom_button_label(&self, label: &str) {
        *self.custom_button_label.borrow_mut() = label.to_owned();
    }
    fn set_custom_button_hover(&self, hover: &str) {
        *self.custom_button_hover.borrow_mut() = hover.to_owned();
    }
    fn set_custom_button_secondary(&self, secondary: bool) {
        self.custom_button_secondary.set(secondary);
    }
    fn reset_widget(&self, widget: Option<Rc<ViewElement>>) {
        if let Some(previous) = self.html_widget_content.borrow_mut().take() {
            previous.set_parent(None);
        }
        if let Some(widget) = &widget {
            widget.set_parent(Some(self.html_widget.clone()));
        }
        *self.html_widget_content.borrow_mut() = widget;
    }
    fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.event()
    }
    fn on_did_custom(&self) -> Event<()> {
        self.on_did_custom.event()
    }
    fn on_did_trigger_button(&self) -> Event<QuickInputButtonRef> {
        self.on_did_trigger_button.event()
    }
    fn on_container_key_up(&self) -> Event<QuickInputKeyUp> {
        self.on_container_key_up.event()
    }
    fn set_ignore_focus_out(&self, ignore_focus_out: bool) {
        self.ignore_focus_out.set(ignore_focus_out);
    }
    fn key_mods(&self) -> IKeyMods {
        self.key_mods.get()
    }
    fn show(&self, controller: Rc<dyn IQuickInput>) {
        self.controller().show(controller);
    }
    fn set_visibilities(&self, visibilities: Visibilities) {
        self.controller().set_visibilities(visibilities);
    }
    fn set_enabled(&self, enabled: bool) {
        self.controller().set_enabled(enabled);
    }
    fn set_context_key(&self, context_key: Option<&str>) {
        *self.context_key.borrow_mut() = context_key.map(str::to_owned);
        (self.controller().options.set_context_key)(context_key);
    }
    fn link_opener_delegate(&self, content: &str) {
        (self.controller().options.link_opener_delegate)(content);
    }
    fn hide(&self) {
        self.controller().hide(None);
    }
}

// ---------------------------------------------------------------------------------------------
// O controlador.
// ---------------------------------------------------------------------------------------------

/// `QuickInputController`.
pub struct QuickInputController {
    this: Weak<QuickInputController>,
    base: Rc<Disposable>,
    options: IQuickInputOptions,
    layout_service: Rc<dyn ILayoutService>,
    storage_service: Rc<dyn StorageService>,
    ui: RefCell<Option<Rc<QuickInputUi>>>,
    dimension: Cell<Option<ISize>>,
    title_bar_offset: Cell<f64>,
    overlay_layout_correction: Cell<Option<OverlayLayoutCorrection>>,
    geometry: Cell<Geometry>,
    widget_height: Cell<f64>,
    enabled: Cell<bool>,
    on_did_accept: Rc<Emitter<()>>,
    on_did_custom: Rc<Emitter<()>>,
    on_did_trigger_button: Rc<Emitter<QuickInputButtonRef>>,
    key_mods: Cell<IKeyMods>,
    controller: RefCell<Option<Rc<dyn IQuickInput>>>,
    anchor_sequencer: Sequencer,
    anchor_scope: RefCell<Option<Rc<AnchorScope>>>,
    container: RefCell<Rc<ViewElement>>,
    styles: RefCell<IQuickInputStyles>,
    on_show: Rc<Emitter<()>>,
    on_hide: Rc<Emitter<()>>,
    previous_focus: RefCell<Option<Rc<dyn Fn() -> bool>>>,
    view_state: Cell<Option<QuickInputViewState>>,
    dnd_state: Cell<Option<DndViewState>>,
    dnd_enabled: Cell<bool>,
    dnd_alignment: Cell<QuickInputAlignment>,
    alignment: Cell<QuickInputAlignment>,
    on_alignment_change: Rc<Emitter<QuickInputAlignment>>,
    close_animation: RefCell<Option<Rc<QuickInputCloseAnimation>>>,
    close_animation_disposables: RefCell<Option<Rc<DisposableStore>>>,
    in_quick_input_context: Rc<dyn IContextKey>,
    quick_input_type_context: Rc<dyn IContextKey>,
    end_of_quick_input_box_context: Rc<dyn IContextKey>,
    alignment_context: Rc<dyn IContextKey>,
    back_button_tooltip: RefCell<String>,
    created: RefCell<Vec<Created>>,
}

impl QuickInputController {
    pub fn new(
        options: IQuickInputOptions,
        layout_service: Rc<dyn ILayoutService>,
        context_key_service: Rc<dyn IContextKeyService>,
        storage_service: Rc<dyn StorageService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let (on_did_accept, on_did_custom, on_did_trigger_button, on_show, on_hide, on_alignment_change) =
            (Emitter::new(), Emitter::new(), Emitter::new(), Emitter::new(), Emitter::new(), Emitter::new());
        base.register(on_did_accept.clone());
        base.register(on_did_custom.clone());
        base.register(on_did_trigger_button.clone());
        base.register(on_show.clone());
        base.register(on_hide.clone());
        base.register(on_alignment_change.clone());
        let in_quick_input_context = in_quick_input_context_key().bind_to(&*context_key_service);
        let quick_input_type_context = quick_input_type_context_key().bind_to(&*context_key_service);
        let end_of_quick_input_box_context = end_of_quick_input_box_context_key().bind_to(&*context_key_service);
        let alignment_context = quick_input_alignment_context_key().bind_to(&*context_key_service);
        let view_state = Self::load_view_state(&*storage_service);
        let container = options.container.clone();
        let styles = options.styles.clone();
        let back_button_tooltip = back_button().tooltip.clone().unwrap_or_default();
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            options,
            layout_service,
            storage_service,
            ui: RefCell::new(None),
            dimension: Cell::new(None),
            title_bar_offset: Cell::new(0.0),
            overlay_layout_correction: Cell::new(None),
            geometry: Cell::new(Geometry::default()),
            widget_height: Cell::new(0.0),
            enabled: Cell::new(true),
            on_did_accept,
            on_did_custom,
            on_did_trigger_button,
            key_mods: Cell::new(IKeyMods::default()),
            controller: RefCell::new(None),
            anchor_sequencer: Sequencer::new(),
            anchor_scope: RefCell::new(None),
            container: RefCell::new(container),
            styles: RefCell::new(styles),
            on_show,
            on_hide,
            previous_focus: RefCell::new(None),
            view_state: Cell::new(view_state),
            dnd_state: Cell::new(None),
            dnd_enabled: Cell::new(true),
            dnd_alignment: Cell::new(QuickInputAlignment::Top),
            alignment: Cell::new(QuickInputAlignment::Top),
            on_alignment_change,
            close_animation: RefCell::new(None),
            close_animation_disposables: RefCell::new(None),
            in_quick_input_context,
            quick_input_type_context,
            end_of_quick_input_box_context,
            alignment_context,
            back_button_tooltip: RefCell::new(back_button_tooltip),
            created: RefCell::new(Vec::new()),
        });
        this
    }

    // -----------------------------------------------------------------------------------------
    // Propriedades.
    // -----------------------------------------------------------------------------------------

    pub fn container(&self) -> Rc<ViewElement> {
        self.container.borrow().clone()
    }

    pub fn on_show(&self) -> Event<()> {
        self.on_show.event()
    }

    pub fn on_hide(&self) -> Event<()> {
        self.on_hide.event()
    }

    /// `alignment` (o observável vira o valor mais o evento de mudança).
    pub fn alignment(&self) -> QuickInputAlignment {
        self.alignment.get()
    }

    pub fn on_did_change_alignment(&self) -> Event<QuickInputAlignment> {
        self.on_alignment_change.event()
    }

    /// `backButton`.
    pub fn back_button(&self) -> QuickInputButtonRef {
        back_button()
    }

    /// O tooltip do botão de voltar (`Back` ou `Back ({0})` com o atalho).
    pub fn back_button_tooltip(&self) -> String {
        self.back_button_tooltip.borrow().clone()
    }

    pub fn is_visible(&self) -> bool {
        self.controller.borrow().is_some()
    }

    /// `currentQuickInput`.
    pub fn current_quick_input(&self) -> Option<QuickInputRef> {
        let current = self.controller.borrow().clone()?;
        let created = self.created.borrow();
        created.iter().find_map(|entry| match entry {
            Created::Pick(weak) => weak.upgrade().filter(|pick| same_object(&current, pick)).map(|pick| QuickInputRef::QuickPick(pick)),
            Created::Input(weak) => weak.upgrade().filter(|input| same_object(&current, input)).map(|input| QuickInputRef::InputBox(input)),
        })
    }

    /// O widget montado (cria na primeira vez, como o `getUI()`).
    pub fn ui(&self) -> Rc<QuickInputUi> {
        self.get_ui()
    }

    /// O widget (`.quick-input-widget`).
    pub fn widget(&self) -> Rc<ViewElement> {
        self.get_ui().container.clone()
    }

    /// `widget.inert`.
    pub fn widget_inert(&self) -> bool {
        self.ui.borrow().as_ref().is_some_and(|ui| ui.inert.get())
    }

    /// A animação de fechamento em curso (`widget.getAnimations()[0]`).
    pub fn closing_animation(&self) -> Option<Rc<QuickInputCloseAnimation>> {
        self.close_animation.borrow().clone()
    }

    /// `getComputedStyle(widget).animationName`.
    pub fn animation_name(&self) -> &'static str {
        let Some(ui) = self.ui.borrow().clone() else { return "none" };
        let motion = has_motion_ancestor(&ui.container) && !ui.container.class_list_contains(QUICK_INPUT_OVERLAY_CLASS);
        if !motion || ui.container.style_get_property("animation").as_deref() == Some("none") {
            "none"
        } else if ui.container.class_list_contains(QUICK_INPUT_MOTION_CLOSING_CLASS) {
            "quick-input-widget-close"
        } else {
            "quick-input-widget-open"
        }
    }

    /// `keyMods` (como a janela os registra).
    pub fn key_mods(&self) -> IKeyMods {
        self.key_mods.get()
    }

    /// Os ouvintes de `keydown`, `keyup` e `mousedown` da janela (`registerKeyModsListeners`).
    pub fn update_key_mods(&self, ctrl_key: bool, meta_key: bool, alt_key: bool, shift_key: bool) {
        let key_mods = IKeyMods { ctrl_cmd: ctrl_key || meta_key, alt: alt_key, shift: shift_key };
        self.key_mods.set(key_mods);
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.set_key_mods(key_mods);
        }
    }

    // -----------------------------------------------------------------------------------------
    // O widget.
    // -----------------------------------------------------------------------------------------

    fn get_ui(&self) -> Rc<QuickInputUi> {
        if let Some(ui) = self.ui.borrow().clone() {
            return ui;
        }
        let container = ViewElement::with_classes(&["quick-input-widget", "show-file-icons"]);
        container.set_parent(Some(self.container()));
        container.hide();

        let child = |classes: &[&str]| {
            let element = ViewElement::with_classes(classes);
            element.set_parent(Some(container.clone()));
            element
        };
        let title_bar = child(&["quick-input-titlebar"]);
        let title = child(&["quick-input-title"]);
        let header = child(&["quick-input-header"]);
        let check_all = child(&["quick-input-check-all"]);
        let description2 = child(&["quick-input-description"]);
        let input_container = child(&["quick-input-and-message"]);
        let filter_container = child(&["quick-input-filter"]);
        let visible_count_container = child(&["quick-input-visible-count"]);
        let count_container = child(&["quick-input-count"]);
        let ok_container = child(&["quick-input-action"]);
        let custom_button_container = child(&["quick-input-action"]);
        let message = child(&["quick-input-message"]);
        let progress_bar = child(&["quick-input-progress"]);
        let html_widget = child(&["quick-input-html-widget"]);
        let description1 = child(&["quick-input-description"]);
        let _ = header;

        let focus = QuickInputFocus::new();
        let styles = self.styles.borrow().clone();
        let input_box = QuickInputBox::new(focus.clone(), styles.input_box.clone());
        input_box.set_attribute("aria-describedby", &format!("{}message", self.options.id_prefix));
        let list_id = format!("{}list", self.options.id_prefix);
        let list = QuickInputList::new(container.clone(), focus.clone(), &list_id);
        input_box.set_attribute("aria-controls", &list_id);

        let ui = Rc::new(QuickInputUi {
            controller: self.this.clone(),
            id_prefix: self.options.id_prefix.clone(),
            container,
            title_bar,
            title,
            description1,
            description2,
            check_all,
            input_container,
            filter_container,
            visible_count_container,
            count_container,
            ok_container,
            custom_button_container,
            message,
            progress_bar,
            html_widget,
            html_widget_content: RefCell::new(None),
            focus,
            input_box,
            list,
            title_text: RefCell::new(String::new()),
            description_text: RefCell::new(String::new()),
            visible_count: Cell::new(0),
            count: Cell::new(0),
            check_all_checked: Cell::new(false),
            check_all_enabled: Cell::new(true),
            left_actions: RefCell::new((Vec::new(), Vec::new())),
            right_actions: RefCell::new((Vec::new(), Vec::new())),
            inline_actions: RefCell::new((Vec::new(), Vec::new())),
            count_container_right: RefCell::new("4px".to_owned()),
            rendered_message: RefCell::new(None),
            message_decoration: RefCell::new(QuickInputMessageDecoration::default()),
            progress_infinite: Cell::new(false),
            progress_aria_hidden: Cell::new(true),
            ok_enabled: Cell::new(true),
            ok_label: RefCell::new(String::new()),
            custom_button_label: RefCell::new(String::new()),
            custom_button_hover: RefCell::new(String::new()),
            custom_button_secondary: Cell::new(false),
            ignore_focus_out: Cell::new(false),
            key_mods: Cell::new(self.key_mods.get()),
            context_key: RefCell::new(None),
            title_bar_height: Cell::new(0.0),
            inert: Cell::new(false),
            on_did_accept: self.on_did_accept.clone(),
            on_did_custom: self.on_did_custom.clone(),
            on_did_trigger_button: self.on_did_trigger_button.clone(),
            on_container_key_up: Emitter::new(),
        });
        self.base.register(ui.on_container_key_up.clone());
        self.base.register(ui.input_box.clone());
        self.base.register(ui.list.clone());
        self.wire_ui(&ui);
        *self.ui.borrow_mut() = Some(ui.clone());
        self.update_styles();
        ui
    }

    /// Os `this._register(...)` do `getUI()` que ligam a lista e a caixa ao resto do widget.
    fn wire_ui(&self, ui: &Rc<QuickInputUi>) {
        let (input_box, list) = (ui.input_box.clone(), ui.list.clone());
        let weak_ui = Rc::downgrade(ui);
        let weak = self.this.clone();

        let (list_for_focus, box_for_focus) = (list.clone(), input_box.clone());
        self.base.register(list.on_did_change_focus().listen(move |_| {
            if box_for_focus.has_focus() {
                match list_for_focus.get_active_descendant() {
                    Some(active_descendant) => {
                        box_for_focus.set_attribute("aria-activedescendant", &active_descendant);
                        box_for_focus.set_list_focus_mode(true);
                    }
                    None => {
                        box_for_focus.remove_attribute("aria-activedescendant");
                        box_for_focus.set_list_focus_mode(false);
                    }
                }
            }
        }));

        let ui_for_all = weak_ui.clone();
        self.base.register(list.on_changed_all_visible_checked().listen(move |checked| {
            // TODO: Support tri-state checkbox when we remove the .indent property that is faking tree structure.
            if let Some(ui) = ui_for_all.upgrade() {
                ui.check_all_checked.set(*checked);
            }
        }));
        let ui_for_visible = weak_ui.clone();
        self.base.register(list.on_changed_visible_count().listen(move |count| {
            if let Some(ui) = ui_for_visible.upgrade() {
                ui.visible_count.set(*count);
            }
        }));
        let ui_for_count = weak_ui.clone();
        let count_store = self.base_store();
        self.base.register(list.on_changed_checked_count().listen(move |count| {
            // Sem o adiamento o leitor de tela não anuncia a contagem final (https://github.com/microsoft/vscode/issues/258617).
            let (ui, count) = (ui_for_count.clone(), *count);
            disposable_timeout(
                move || {
                    if let Some(ui) = ui.upgrade() {
                        ui.count.set(count);
                    }
                },
                0,
                Some(count_store.clone()),
            );
        }));

        let (weak_leave, box_for_leave, list_for_leave) = (weak.clone(), input_box.clone(), list.clone());
        let leave_store = self.base_store();
        self.base.register(list.on_leave().listen(move |()| {
            // Defer to avoid the input field reacting to the triggering key.
            let (weak, input_box, list) = (weak_leave.clone(), box_for_leave.clone(), list_for_leave.clone());
            disposable_timeout(
                move || {
                    let Some(this) = weak.upgrade() else { return };
                    if this.controller.borrow().is_none() {
                        return;
                    }
                    input_box.set_focus();
                    if this.current_can_select_many() {
                        list.clear_focus();
                    }
                },
                0,
                Some(leave_store.clone()),
            );
        }));

        self.base.register(input_box.on_key_down().listen({
            let (weak, input_box) = (weak.clone(), input_box.clone());
            move |event| {
                let Some(this) = weak.upgrade() else { return };
                let value = input_box.is_selection_at_end();
                if this.end_of_quick_input_box_context.get() != Dynamic::from(value) {
                    this.end_of_quick_input_box_context.set(Dynamic::from(value));
                }
                // Don't remove aria-activedescendant when only modifier keys are pressed to prevent screen reader re-announcements.
                if !je_base::key_codes::is_modifier_key(event.key_code) {
                    input_box.remove_attribute("aria-activedescendant");
                    // Reset ARIA popup mode to allow normal text editing with arrow keys
                    input_box.set_list_focus_mode(false);
                }
            }
        }));
    }

    /// Um `DisposableStore` ligado ao descarte do controlador, para os timers dos ouvintes.
    fn base_store(&self) -> Rc<DisposableStore> {
        let store = DisposableStore::new();
        self.base.register(store.clone());
        store
    }

    /// `this.controller instanceof QuickPick && this.controller.canSelectMany`.
    fn current_can_select_many(&self) -> bool {
        match self.current_quick_input() {
            Some(QuickInputRef::QuickPick(pick)) => pick.can_select_many(),
            _ => false,
        }
    }

    /// O foco entrou no widget (o `FOCUS` do contêiner, em captura). `from_inside_input` é o `relatedTarget` dentro do
    /// `inputContainer`; `from_inside_container`, dentro do contêiner; `previous` devolve o foco ao elemento anterior.
    pub fn handle_focus_in(&self, from_inside_input: bool, from_inside_container: bool, previous: Option<Rc<dyn Fn() -> bool>>) {
        let ui = self.get_ui();
        if from_inside_input {
            let value = ui.input_box.is_selection_at_end();
            if self.end_of_quick_input_box_context.get() != Dynamic::from(value) {
                self.end_of_quick_input_box_context.set(Dynamic::from(value));
            }
        }
        // Ignore focus events within container
        if from_inside_container {
            return;
        }
        self.in_quick_input_context.set(Dynamic::from(true));
        *self.previous_focus.borrow_mut() = previous;
        ui.focus.set(QuickInputFocusTarget::Container);
        // `container.addEventListener('focus', () => inputBox.setFocus())`
        ui.input_box.set_focus();
    }

    /// O foco saiu do widget (`focusTracker.onDidBlur`).
    pub fn handle_blur(&self) {
        let ui = self.get_ui();
        ui.focus.set(QuickInputFocusTarget::None);
        if !ui.ignore_focus_out.get() && !(self.options.ignore_focus_out)() {
            self.hide(Some(QuickInputHideReason::Blur));
        }
        self.in_quick_input_context.set(Dynamic::from(false));
        self.end_of_quick_input_box_context.set(Dynamic::from(false));
        *self.previous_focus.borrow_mut() = None;
    }

    // -----------------------------------------------------------------------------------------
    // Criar controladores.
    // -----------------------------------------------------------------------------------------

    /// `createQuickPick(options)`.
    pub fn create_quick_pick(&self) -> Rc<QuickPick> {
        let ui = self.get_ui();
        let pick = QuickPick::new(ui);
        self.created.borrow_mut().push(Created::Pick(Rc::downgrade(&pick)));
        pick
    }

    /// `createInputBox()`.
    pub fn create_input_box(&self) -> Rc<InputBox> {
        let ui = self.get_ui();
        let input = InputBox::new(ui);
        self.created.borrow_mut().push(Created::Input(Rc::downgrade(&input)));
        input
    }

    /// `createQuickWidget()`.
    pub fn create_quick_widget(&self) -> Rc<QuickWidget> {
        let ui = self.get_ui();
        QuickWidget::new(ui)
    }

    /// `withQuickInputAnchor(anchor, anchorPosition, operation)`.
    pub fn with_quick_input_anchor<T: Clone + 'static>(
        &self,
        anchor: Option<QuickInputAnchor>,
        anchor_position: Option<QuickInputAnchorPosition>,
        operation: impl FnOnce() -> Promise<T> + 'static,
    ) -> Promise<T> {
        let weak = self.this.clone();
        self.anchor_sequencer.queue(move || {
            let Some(this) = weak.upgrade() else { return operation() };
            let scope = Rc::new(AnchorScope { anchor, anchor_position, consumed: Cell::new(false) });
            *this.anchor_scope.borrow_mut() = Some(scope.clone());
            let weak = weak.clone();
            let clear = move || {
                if let Some(this) = weak.upgrade() {
                    let same = this.anchor_scope.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &scope));
                    if same {
                        *this.anchor_scope.borrow_mut() = None;
                    }
                }
            };
            operation().finally(clear)
        })
    }

    // -----------------------------------------------------------------------------------------
    // pick e input.
    // -----------------------------------------------------------------------------------------

    /// `pick(picks, options, token)` no modo de escolha simples.
    pub fn pick(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<IQuickPickItem>> {
        self.pick_impl(picks, options.cloned().unwrap_or_default(), token.cloned(), false).then(|result| result.and_then(|items| items.into_iter().next()))
    }

    /// `pick(picks, { canPickMany: true })`.
    pub fn pick_many(&self, picks: QuickPicks, options: Option<&IPickOptions>, token: Option<&CancellationToken>) -> Promise<Option<Vec<IQuickPickItem>>> {
        self.pick_impl(picks, options.cloned().unwrap_or_default(), token.cloned(), true)
    }

    fn pick_impl(&self, picks: QuickPicks, options: IPickOptions, token: Option<CancellationToken>, can_pick_many: bool) -> Promise<PickResult> {
        let this = self.this.clone();
        Promise::with_executor(move |resolver| {
            let Some(controller) = this.upgrade() else {
                resolver.resolve(None);
                return;
            };
            if token.as_ref().is_some_and(CancellationToken::is_cancellation_requested) {
                resolver.resolve(None);
                return;
            }
            let input = controller.create_quick_pick();
            let on_key_mods = options.on_key_mods.clone();
            let resolved = Rc::new(Cell::new(false));
            let resolve = {
                let (resolver, resolved, weak_input) = (resolver.clone(), resolved.clone(), Rc::downgrade(&input));
                Rc::new(move |result: PickResult| {
                    if !resolved.replace(true) {
                        if let (Some(on_key_mods), Some(input)) = (&on_key_mods, weak_input.upgrade()) {
                            on_key_mods(input.key_mods());
                        }
                        resolver.resolve(result);
                    }
                })
            };
            let active_item: Rc<RefCell<Option<IQuickPickItem>>> = Rc::new(RefCell::new(None));
            let disposables = DisposableStore::new();
            disposables.add(input.clone());

            let weak_input = Rc::downgrade(&input);
            disposables.add(input.on_did_accept().listen({
                let (resolve, weak_input) = (resolve.clone(), weak_input.clone());
                move |_| {
                    let Some(input) = weak_input.upgrade() else { return };
                    if input.can_select_many() {
                        resolve(Some(input.selected_items()));
                        input.hide();
                    } else if let Some(result) = input.active_items().into_iter().next() {
                        resolve(Some(vec![result]));
                        input.hide();
                    }
                }
            }));
            disposables.add(input.on_did_change_active().listen({
                let on_did_focus = options.on_did_focus.clone();
                move |items| {
                    if let (Some(focused), Some(on_did_focus)) = (items.first(), &on_did_focus) {
                        on_did_focus(focused);
                    }
                }
            }));
            disposables.add(input.on_did_change_selection().listen({
                let (resolve, weak_input) = (resolve.clone(), weak_input.clone());
                move |items| {
                    let Some(input) = weak_input.upgrade() else { return };
                    if !input.can_select_many() {
                        if let Some(result) = items.first() {
                            resolve(Some(vec![result.clone()]));
                            input.hide();
                        }
                    }
                }
            }));
            disposables.add(input.on_did_trigger_item_button().listen({
                let (on_trigger, weak_input) = (options.on_did_trigger_item_button.clone(), weak_input.clone());
                move |event| {
                    let Some(on_trigger) = &on_trigger else { return };
                    let (weak_input, item) = (weak_input.clone(), event.item.clone());
                    let remove_item: Rc<dyn Fn()> = Rc::new(move || {
                        let Some(input) = weak_input.upgrade() else { return };
                        let mut items = input.items();
                        let Some(index) = items.iter().position(|entry| matches!(entry, QuickPickInput::Item(candidate) if same_item(candidate, &item))) else { return };
                        let removed = items.remove(index);
                        let active_items: Vec<IQuickPickItem> = input
                            .active_items()
                            .into_iter()
                            .filter(|active| !matches!(&removed, QuickPickInput::Item(removed) if same_item(active, removed)))
                            .collect();
                        let keep_scroll_position_before = input.keep_scroll_position();
                        input.set_keep_scroll_position(true);
                        input.set_items(items);
                        input.set_active_items(active_items);
                        input.set_keep_scroll_position(keep_scroll_position_before);
                    });
                    on_trigger(IQuickPickItemButtonContext { event: event.clone(), remove_item });
                }
            }));
            disposables.add(input.on_did_trigger_separator_button().listen({
                let on_trigger = options.on_did_trigger_separator_button.clone();
                move |event| {
                    if let Some(on_trigger) = &on_trigger {
                        on_trigger(event.clone());
                    }
                }
            }));
            disposables.add(input.on_did_change_value().listen({
                let (active_item, weak_input) = (active_item.clone(), weak_input.clone());
                move |value| {
                    let Some(input) = weak_input.upgrade() else { return };
                    let active = active_item.borrow().clone();
                    if let Some(active) = active {
                        let current = input.active_items();
                        if value.is_empty() && (current.len() != 1 || !same_item(&current[0], &active)) {
                            input.set_active_items(vec![active]);
                        }
                    }
                }
            }));
            if let Some(token) = &token {
                disposables.add(token.on_cancellation_requested().listen({
                    let weak_input = weak_input.clone();
                    move |()| {
                        if let Some(input) = weak_input.upgrade() {
                            input.hide();
                        }
                    }
                }));
            }
            disposables.add(input.on_did_hide().listen({
                let (resolve, disposables) = (resolve.clone(), disposables.clone());
                move |_| {
                    disposables.dispose();
                    resolve(None);
                }
            }));

            input.set_title(options.title.clone());
            if let Some(value) = options.value.clone().filter(|value| !value.is_empty()) {
                input.set_value(value);
            }
            input.set_can_select_many(can_pick_many);
            input.set_placeholder(options.place_holder.clone());
            input.set_prompt(options.prompt.clone());
            input.set_ignore_focus_out(options.ignore_focus_lost == Some(true));
            input.set_match_on_description(options.match_on_description == Some(true));
            input.set_match_on_detail(options.match_on_detail == Some(true));
            if let Some(sort_by_label) = options.sort_by_label {
                input.set_sort_by_label(sort_by_label);
            }
            input.set_match_on_label(options.match_on_label.unwrap_or(true)); // default to true
            input.set_quick_navigate(options.quick_navigate.clone());
            input.set_hide_input(options.hide_input == Some(true));
            input.set_context_key(options.context_key.clone());
            input.set_anchor(options.anchor);
            input.set_anchor_position(options.anchor_position);
            input.set_busy(true);

            let active_promise: Promise<Option<IQuickPickItem>> = match options.active_item.clone() {
                None => Promise::resolved(None),
                Some(PickActiveItem::Item(item)) => Promise::resolved(Some(item)),
                Some(PickActiveItem::Promised(promise)) => promise.then(Some),
            };
            let picks_promise = picks.into_promise();
            let loaded = {
                let (weak_input, active_item) = (weak_input.clone(), active_item.clone());
                picks_promise.then_promise({
                    let active_promise = active_promise.clone();
                    move |items| active_promise.then(move |active| (items, active))
                })
                .then(move |(items, active)| {
                    *active_item.borrow_mut() = active.clone();
                    let Some(input) = weak_input.upgrade() else { return };
                    input.set_busy(false);
                    input.set_items(items.clone());
                    if input.can_select_many() {
                        let picked = items
                            .iter()
                            .filter_map(|item| item.as_item())
                            .filter(|item| item.picked == Some(true))
                            .cloned()
                            .collect();
                        input.set_selected_items(picked);
                    }
                    if let Some(active) = active {
                        input.set_active_items(vec![active]);
                    }
                })
            };
            let _ = loaded;
            input.show();
            let reject_input = weak_input.clone();
            picks_promise.then_result(move |result| {
                if let Err(error) = result {
                    resolver.reject(error);
                    if let Some(input) = reject_input.upgrade() {
                        input.hide();
                    }
                }
                Ok(())
            });
        })
    }

    fn set_validation_on_input(input: &dyn IInputBox, validation_result: &InputValidationResult) {
        match validation_result {
            InputValidationResult::Message(message) if !message.is_empty() => {
                input.set_severity(Severity::Error);
                input.set_validation_message(Some(message.clone()));
            }
            InputValidationResult::Content { content, severity } => {
                input.set_severity(*severity);
                input.set_validation_message(Some(content.clone()));
            }
            _ => {
                input.set_severity(Severity::Ignore);
                input.set_validation_message(None);
            }
        }
    }

    /// `input(options, token)`.
    pub fn input(&self, options: Option<&IInputOptions>, token: Option<&CancellationToken>) -> Promise<Option<String>> {
        let (options, token, this) = (options.cloned().unwrap_or_default(), token.cloned(), self.this.clone());
        Promise::with_executor(move |resolver| {
            let Some(controller) = this.upgrade() else {
                resolver.resolve(None);
                return;
            };
            if token.as_ref().is_some_and(CancellationToken::is_cancellation_requested) {
                resolver.resolve(None);
                return;
            }
            let input = controller.create_input_box();
            let validate_input: Rc<dyn Fn(&str) -> Promise<InputValidationResult>> =
                options.validate_input.clone().unwrap_or_else(|| Rc::new(|_| Promise::resolved(InputValidationResult::None)));
            let initial_value = options.value.clone().unwrap_or_default();
            let validation_value = Rc::new(RefCell::new(initial_value.clone()));
            let validation = Rc::new(RefCell::new(validate_input(&initial_value)));
            let disposables = DisposableStore::new();
            disposables.add(input.clone());
            let weak_input = Rc::downgrade(&input);

            let on_did_value_change = input.on_did_change_value().debounce(|_last: Option<String>, current: &String| current.clone(), DebounceOptions::default(), Some(&*disposables));
            disposables.add(on_did_value_change.listen({
                let (validate_input, validation_value, validation, weak_input) = (validate_input.clone(), validation_value.clone(), validation.clone(), weak_input.clone());
                move |value| {
                    if *value != *validation_value.borrow() {
                        *validation.borrow_mut() = validate_input(value);
                        *validation_value.borrow_mut() = value.clone();
                    }
                    let (value, validation_value, weak_input) = (value.clone(), validation_value.clone(), weak_input.clone());
                    let current = validation.borrow().clone();
                    current.then(move |result| {
                        if value == *validation_value.borrow() {
                            if let Some(input) = weak_input.upgrade() {
                                Self::set_validation_on_input(&*input, &result);
                            }
                        }
                    });
                }
            }));
            disposables.add(input.on_did_accept().listen({
                let (validate_input, validation_value, validation, weak_input, resolver) =
                    (validate_input.clone(), validation_value.clone(), validation.clone(), weak_input.clone(), resolver.clone());
                move |()| {
                    let Some(input) = weak_input.upgrade() else { return };
                    let value = input.value();
                    if value != *validation_value.borrow() {
                        *validation.borrow_mut() = validate_input(&value);
                        *validation_value.borrow_mut() = value.clone();
                    }
                    let (validation_value, weak_input, resolver) = (validation_value.clone(), weak_input.clone(), resolver.clone());
                    let current = validation.borrow().clone();
                    current.then(move |result| {
                        let valid = match &result {
                            InputValidationResult::None => true,
                            InputValidationResult::Message(message) => message.is_empty(),
                            InputValidationResult::Content { severity, .. } => *severity != Severity::Error,
                        };
                        let Some(input) = weak_input.upgrade() else { return };
                        if valid {
                            resolver.resolve(Some(value));
                            input.hide();
                        } else if value == *validation_value.borrow() {
                            Self::set_validation_on_input(&*input, &result);
                        }
                    });
                }
            }));
            if let Some(token) = &token {
                disposables.add(token.on_cancellation_requested().listen({
                    let weak_input = weak_input.clone();
                    move |()| {
                        if let Some(input) = weak_input.upgrade() {
                            input.hide();
                        }
                    }
                }));
            }
            disposables.add(input.on_did_hide().listen({
                let (disposables, resolver) = (disposables.clone(), resolver.clone());
                move |_| {
                    disposables.dispose();
                    resolver.resolve(None);
                }
            }));

            input.set_title(options.title.clone());
            input.set_value(initial_value);
            input.set_value_selection(options.value_selection);
            input.set_prompt(options.prompt.clone());
            input.set_placeholder(options.place_holder.clone());
            input.set_password(options.password == Some(true));
            input.set_ignore_focus_out(options.ignore_focus_lost == Some(true));
            input.set_anchor(options.anchor);
            input.set_anchor_position(options.anchor_position);
            input.show();
        })
    }

    // -----------------------------------------------------------------------------------------
    // Mostrar, esconder, habilitar.
    // -----------------------------------------------------------------------------------------

    fn show(&self, controller: Rc<dyn IQuickInput>) {
        self.complete_close_animation();
        let anchor_scope = self.anchor_scope.borrow().clone();
        if controller.quick_input_type() == QuickInputType::QuickPick && controller.anchor().is_none() {
            if let Some(scope) = anchor_scope.filter(|scope| !scope.consumed.get()) {
                scope.consumed.set(true);
                controller.set_anchor(scope.anchor);
                controller.set_anchor_position(scope.anchor_position);
            }
        }
        let ui = self.get_ui();
        let old_controller = self.controller.replace(Some(controller.clone()));
        if let Some(old_controller) = old_controller {
            old_controller.did_hide(None);
        }

        self.set_enabled(true);
        ui.reset_for_show();

        let label = (self.options.back_keybinding_label)();
        *self.back_button_tooltip.borrow_mut() = match label {
            Some(label) => je_base::nls::localize("quickInput.backWithKeybinding", "Back ({0})", &[&label]),
            None => je_base::nls::localize("quickInput.back", "Back", &[]),
        };

        self.overlay_layout_correction.set(None);
        if controller.anchor_position() == Some(QuickInputAnchorPosition::Overlay) {
            ui.container.class_list_add(QUICK_INPUT_OVERLAY_CLASS);
        } else {
            ui.container.class_list_remove(QUICK_INPUT_OVERLAY_CLASS);
        }
        ui.container.show();
        self.update_layout();
        self.dnd_set_enabled(controller.anchor().is_none());
        self.dnd_layout_container();
        if controller.anchor().is_some() {
            // Anchored quick inputs are positioned near a specific element, not at the default top location.
            self.set_alignment_value(QuickInputAlignment::Custom);
        } else {
            // Re-sync from DnD in case a previous anchored input left us stale.
            self.set_alignment_value(self.dnd_alignment.get());
        }
        self.on_show.fire(());
        ui.input_box.set_focus();
        self.quick_input_type_context.set(Dynamic::string(controller.quick_input_type().as_str()));
    }

    fn set_alignment_value(&self, alignment: QuickInputAlignment) {
        if self.alignment.get() != alignment {
            self.alignment.set(alignment);
            self.on_alignment_change.fire(alignment);
        }
    }

    fn set_visibilities(&self, visibilities: Visibilities) {
        let ui = self.get_ui();
        let flag = |value: Option<bool>| value == Some(true);
        let input_or_check_all = flag(visibilities.input_box) || flag(visibilities.check_all);
        toggle_display(&ui.title_bar, flag(visibilities.title));
        toggle_display(&ui.title, flag(visibilities.title));
        toggle_display(&ui.description1, flag(visibilities.description) && input_or_check_all);
        toggle_display(&ui.description2, flag(visibilities.description) && !input_or_check_all);
        toggle_display(&ui.check_all, flag(visibilities.check_all));
        toggle_display(&ui.input_container, flag(visibilities.input_box));
        toggle_display(&ui.filter_container, flag(visibilities.input_box));
        toggle_display(&ui.visible_count_container, flag(visibilities.visible_count));
        toggle_display(&ui.count_container, flag(visibilities.count));
        toggle_display(&ui.ok_container, flag(visibilities.ok));
        toggle_display(&ui.custom_button_container, flag(visibilities.custom_button));
        toggle_display(&ui.message, flag(visibilities.message));
        toggle_display(&ui.progress_bar, flag(visibilities.progress_bar));
        ui.list.set_displayed(flag(visibilities.list));
        for (class_name, on) in [("show-checkboxes", flag(visibilities.check_box)), ("hidden-input", !flag(visibilities.input_box) && !flag(visibilities.description))] {
            if on {
                ui.container.class_list_add(class_name);
            } else {
                ui.container.class_list_remove(class_name);
            }
        }
        self.overlay_layout_correction.set(None);
        self.update_layout(); // TODO
    }

    fn set_enabled(&self, enabled: bool) {
        if enabled != self.enabled.get() {
            self.enabled.set(enabled);
            self.get_ui().set_enabled_widgets(enabled);
        }
    }

    /// `hide(reason)`.
    pub fn hide(&self, reason: Option<QuickInputHideReason>) {
        let Some(controller) = self.controller.borrow().clone() else { return };
        controller.will_hide(reason);

        let ui = self.ui.borrow().clone();
        let focus_changed = ui.as_ref().is_some_and(|ui| !ui.focus.is_inside());
        *self.controller.borrow_mut() = None;
        self.on_hide.fire(());
        if let Some(ui) = &ui {
            let container = &ui.container;
            if !container.class_list_contains(QUICK_INPUT_OVERLAY_CLASS) && has_motion_ancestor(container) {
                ui.inert.set(true);
                container.class_list_add(QUICK_INPUT_MOTION_CLOSING_CLASS);
                let animation_disposables = DisposableStore::new();
                *self.close_animation_disposables.borrow_mut() = Some(animation_disposables.clone());
                // CSS can suppress motion independently of the workbench classes.
                if container.style_get_property("animation").as_deref() != Some("none") {
                    let animation = QuickInputCloseAnimation::new();
                    *self.close_animation.borrow_mut() = Some(animation.clone());
                    for event in [animation.on_finish(), animation.on_cancel()] {
                        let weak = self.this.clone();
                        animation_disposables.add(event.listen(move |()| {
                            if let Some(this) = weak.upgrade() {
                                this.complete_close_animation();
                            }
                        }));
                    }
                    let weak = self.this.clone();
                    animation_disposables.add(disposable_timeout(
                        move || {
                            if let Some(this) = weak.upgrade() {
                                this.complete_close_animation();
                            }
                        },
                        QUICK_INPUT_CLOSE_ANIMATION_DURATION,
                        None,
                    ));
                } else {
                    self.complete_close_animation();
                }
            } else {
                container.hide();
            }
        }
        if !focus_changed {
            let previous = self.previous_focus.borrow().clone();
            if previous.is_some_and(|restore| restore()) {
                *self.previous_focus.borrow_mut() = None;
            } else {
                (self.options.return_focus)();
            }
        }
        controller.did_hide(reason);
    }

    fn complete_close_animation(&self) {
        let Some(disposables) = self.close_animation_disposables.borrow_mut().take() else { return };
        disposables.dispose();
        let animation = self.close_animation.borrow_mut().take();
        if let Some(animation) = animation {
            animation.dispose();
        }
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.inert.set(false);
            ui.container.class_list_remove(QUICK_INPUT_MOTION_CLOSING_CLASS);
            ui.container.hide();
        }
    }

    /// `focus()`.
    pub fn focus(&self) {
        if self.is_visible() {
            let ui = self.get_ui();
            if ui.input_box.enabled() {
                ui.input_box.set_focus();
            } else {
                ui.list.dom_focus();
            }
        }
    }

    /// `toggle()`.
    pub fn toggle(&self) {
        if self.is_visible() && self.current_can_select_many() {
            self.get_ui().list.toggle_checkbox();
        }
    }

    /// `toggleHover()`: sem o `WorkbenchHoverDelegate`, só diz se há dica a mostrar na linha focada.
    pub fn toggle_hover(&self) -> bool {
        if self.is_visible() && matches!(self.current_quick_input(), Some(QuickInputRef::QuickPick(_))) {
            return self.get_ui().list.toggle_hover();
        }
        false
    }

    /// `navigate(next, quickNavigate)`.
    pub fn navigate(&self, next: bool, quick_navigate: Option<&IQuickNavigateConfiguration>) {
        if self.is_visible() && self.get_ui().list.displayed() {
            self.get_ui().list.focus(if next { QuickPickFocus::Next } else { QuickPickFocus::Previous });
            if let (Some(quick_navigate), Some(QuickInputRef::QuickPick(pick))) = (quick_navigate, self.current_quick_input()) {
                pick.set_quick_navigate(Some(quick_navigate.clone()));
            }
        }
    }

    /// `accept(keyMods)`.
    pub fn accept(&self, key_mods: Option<IKeyMods>) -> Promise<()> {
        // When accepting the item programmatically, `keyMods` come from the provided set or are unset.
        let key_mods = key_mods.unwrap_or_default();
        self.key_mods.set(key_mods);
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.set_key_mods(key_mods);
        }
        self.on_did_accept.fire(());
        Promise::resolved(())
    }

    /// `back()`.
    pub fn back(&self) -> Promise<()> {
        self.on_did_trigger_button.fire(back_button());
        Promise::resolved(())
    }

    /// `cancel(reason)`.
    pub fn cancel(&self, reason: Option<QuickInputHideReason>) -> Promise<()> {
        self.hide(reason);
        Promise::resolved(())
    }

    /// `setAlignment(alignment)`.
    pub fn set_alignment(&self, alignment: QuickInputAlignmentSetting) {
        if self.controller.borrow().as_ref().is_some_and(|controller| controller.anchor().is_some()) {
            return; // anchored inputs own their own positioning
        }
        self.dnd_set_alignment(alignment, true);
    }

    // -----------------------------------------------------------------------------------------
    // Layout.
    // -----------------------------------------------------------------------------------------

    /// `layout(dimension, titleBarOffset)`.
    pub fn layout(&self, dimension: ISize, title_bar_offset: f64) {
        self.dimension.set(Some(dimension));
        self.title_bar_offset.set(title_bar_offset);
        self.overlay_layout_correction.set(None);
        self.update_layout();
    }

    /// `reparentUI(container)`.
    pub fn reparent_ui(&self, container: Rc<ViewElement>) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.container.set_parent(Some(container.clone()));
            *self.container.borrow_mut() = container;
        }
    }

    fn write_geometry(&self, geometry: Geometry) {
        self.geometry.set(geometry);
        let Some(ui) = self.ui.borrow().clone() else { return };
        let container = self.layout_service.get_container().rect();
        let style = |name: &str, value: Option<f64>, initial: bool| match value {
            Some(value) => ui.container.style_set_property(name, &format!("{value}px")),
            None if initial => ui.container.style_set_property(name, "initial"),
            None => ui.container.style_remove_property(name),
        };
        let anchored = self.controller.borrow().as_ref().is_some_and(|controller| controller.anchor().is_some());
        style("left", geometry.left, anchored);
        style("right", geometry.right, anchored);
        style("top", geometry.top, anchored);
        style("bottom", geometry.bottom, anchored);
        ui.container.style_set_property("width", &format!("{}px", geometry.width));
        let height = self.widget_height.get();
        let left = geometry.left.unwrap_or_else(|| container.width - geometry.right.unwrap_or(0.0) - geometry.width);
        let top = geometry.top.unwrap_or_else(|| container.height - geometry.bottom.unwrap_or(0.0) - height);
        ui.container.set_position(top, left);
        ui.container.set_size(geometry.width, height);
    }

    fn update_layout(&self) {
        let Some(ui) = self.ui.borrow().clone() else { return };
        let Some(controller) = self.controller.borrow().clone() else { return };
        let dimension = self.dimension.get().unwrap_or(ISize { width: 0.0, height: 0.0 });
        let mut width = (dimension.width * 0.62 /* golden cut */).min(MAX_WIDTH);
        let mut list_height = dimension.height * 0.4;
        let mut overlay_anchor: Option<IRect> = None;
        let mut geometry = Geometry { width, ..Geometry::default() };

        // Position
        if let Some(anchor_value) = controller.anchor() {
            let container = self.layout_service.get_container().rect();
            let vertical_padding = 6.0 + 26.0 + 16.0; // Accounts for input box and padding
            let target = AnchorTarget::Anchor(IAnchor { x: anchor_value.x, y: anchor_value.y, width: anchor_value.width, height: anchor_value.height });
            let mut anchor = get_anchor_rect(&target);
            let mut list_height_ratio = 0.2;
            let mut max_list_height = 200.0;
            let preferred_anchor_position;
            let position = controller.anchor_position();
            if position == Some(QuickInputAnchorPosition::Overlay) {
                overlay_anchor = Some(anchor);
                ui.input_box.set_height(Some(anchor.height));
                width = anchor.width;
                list_height_ratio = 0.4;
                anchor = IRect { height: 0.0, ..anchor };
                max_list_height = 400.0_f64.min(container.top + container.height - anchor.top - vertical_padding);
                preferred_anchor_position = AnchorPosition::Below;
            } else {
                width = 380.0;
                preferred_anchor_position = if position == Some(QuickInputAnchorPosition::Below) { AnchorPosition::Below } else { AnchorPosition::Above };
            }

            list_height = (dimension.height * list_height_ratio).min(max_list_height);

            // Beware: we need to add some extra pixels to the height to account for the input and padding.
            let anchor_gap = if position == Some(QuickInputAnchorPosition::Overlay) { 0.0 } else { 4.0 };
            let container_height = list_height.floor() + vertical_padding + anchor_gap;
            let placed = layout2d(
                container,
                ISize { width, height: container_height },
                anchor,
                Some(ILayout2DOptions { anchor_position: Some(preferred_anchor_position), ..Default::default() }),
            );
            if placed.anchor_alignment == AnchorAlignment::Right {
                geometry.right = Some(placed.right);
                geometry.left = None;
            } else {
                geometry.left = Some(placed.left);
                geometry.right = None;
            }
            if placed.anchor_position == AnchorPosition::Above {
                geometry.bottom = Some(placed.bottom + anchor_gap);
                geometry.top = None;
            } else {
                geometry.top = Some(placed.top + anchor_gap);
                geometry.bottom = None;
            }
            geometry.width = width;
            self.widget_height.set(container_height);
        } else {
            let view_state = self.view_state.get();
            geometry.top = Some(match view_state.and_then(|state| state.top) {
                Some(top) => (dimension.height * top).round(),
                None => self.title_bar_offset.get(),
            });
            geometry.left = Some((dimension.width * view_state.and_then(|state| state.left).unwrap_or(0.5 /* center */) - width / 2.0).round());
            geometry.width = width;
        }
        self.write_geometry(geometry);

        if let Some(anchor) = overlay_anchor {
            self.align_overlay_input(&ui, anchor);
        }
        ui.input_box.layout();
        ui.list.layout(Some(list_height));
    }

    fn align_overlay_input(&self, ui: &Rc<QuickInputUi>, anchor: IRect) {
        let mut correction = self.overlay_layout_correction.get();
        let stale = correction.is_none_or(|current| {
            current.anchor.left != anchor.left || current.anchor.top != anchor.top || current.anchor.width != anchor.width || current.anchor.height != anchor.height
        });
        if stale {
            ui.input_box.layout();
            let input = ui.filter_container_rect();
            correction = Some(OverlayLayoutCorrection {
                anchor,
                left: anchor.left - input.left,
                right: input.left + input.width - (anchor.left + anchor.width),
                top: anchor.top - input.top,
                bottom: input.top + input.height - (anchor.top + anchor.height),
                width: anchor.width - input.width,
            });
            self.overlay_layout_correction.set(correction);
        }
        let Some(correction) = correction else { return };
        let mut geometry = self.geometry.get();
        geometry.width += correction.width;
        match geometry.left {
            Some(left) => geometry.left = Some(left + correction.left),
            None => geometry.right = geometry.right.map(|right| right + correction.right),
        }
        match geometry.top {
            Some(top) => geometry.top = Some(top + correction.top),
            None => geometry.bottom = geometry.bottom.map(|bottom| bottom + correction.bottom),
        }
        self.write_geometry(geometry);
    }

    // -----------------------------------------------------------------------------------------
    // Estilos.
    // -----------------------------------------------------------------------------------------

    /// `applyStyles(styles)`.
    pub fn apply_styles(&self, styles: IQuickInputStyles) {
        *self.styles.borrow_mut() = styles.clone();
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.input_box.apply_styles(styles.input_box);
        }
        self.update_styles();
    }

    fn update_styles(&self) {
        let Some(ui) = self.ui.borrow().clone() else { return };
        let styles = self.styles.borrow().widget.clone();
        let set = |element: &ViewElement, name: &str, value: Option<String>| match value {
            Some(value) => element.style_set_property(name, &value),
            None => element.style_remove_property(name),
        };
        set(&ui.title_bar, "background-color", styles.quick_input_title_background);
        set(&ui.container, "background-color", styles.quick_input_background);
        set(&ui.container, "color", styles.quick_input_foreground);
        set(&ui.container, "border", styles.widget_border.map(|border| format!("1px solid {border}")));
    }

    // -----------------------------------------------------------------------------------------
    // Estado da posição (`viewState`) e o arrastar.
    // -----------------------------------------------------------------------------------------

    /// Lê o `{"top":n,"left":n}` que [`Self::save_view_state`] escreve (só esse formato, só números).
    fn load_view_state(storage: &dyn StorageService) -> Option<QuickInputViewState> {
        let data = storage.get(VIEWSTATE_STORAGE_KEY, StorageScope::Application, Some("{}"))?;
        let number = |key: &str| -> Option<f64> {
            let marker = format!("\"{key}\":");
            let start = data.find(&marker)? + marker.len();
            let rest = data[start..].trim_start();
            let end = rest.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))).unwrap_or(rest.len());
            rest[..end].parse::<f64>().ok()
        };
        let (top, left) = (number("top"), number("left"));
        (top.is_some() || left.is_some()).then_some(QuickInputViewState { top, left })
    }

    fn save_view_state(&self, view_state: Option<QuickInputViewState>) {
        let is_main_window = Rc::ptr_eq(&self.layout_service.active_container(), &self.layout_service.main_container());
        if !is_main_window {
            return;
        }
        match view_state {
            Some(state) => {
                let fields: Vec<String> = [("top", state.top), ("left", state.left)].into_iter().filter_map(|(key, value)| value.map(|value| format!("\"{key}\":{value}"))).collect();
                let json = format!("{{{}}}", fields.join(","));
                self.storage_service.store(VIEWSTATE_STORAGE_KEY, json.as_str().into(), StorageScope::Application, StorageTarget::Machine);
            }
            None => self.storage_service.remove(VIEWSTATE_STORAGE_KEY, StorageScope::Application),
        }
    }

    /// `QuickInputDragAndDropController.dndViewState`: o `autorun` do controlador roda aqui, na hora.
    fn dnd_set_state(&self, state: DndViewState) {
        self.dnd_state.set(Some(state));
        if let (Some(top), Some(left)) = (state.top, state.left) {
            self.view_state.set(Some(QuickInputViewState { top: Some(top), left: Some(left) }));
        } else {
            // Reset position/size
            self.view_state.set(None);
        }
        self.update_layout();
        // Save position
        if state.done {
            self.save_view_state(self.view_state.get());
        }
    }

    fn set_dnd_alignment_state(&self, value: Option<QuickInputAlignment>) {
        self.alignment_context.set(match value {
            Some(QuickInputAlignment::Top) => Dynamic::string("top"),
            Some(QuickInputAlignment::Center) => Dynamic::string("center"),
            _ => Dynamic::Undefined,
        });
        let alignment = value.unwrap_or(QuickInputAlignment::Custom);
        self.dnd_alignment.set(alignment);
        // Mirror DnD alignment into the stable observable, salvo se um quick input ancorado o possui.
        if self.controller.borrow().as_ref().is_none_or(|controller| controller.anchor().is_none()) {
            self.set_alignment_value(alignment);
        }
    }

    fn dnd_set_enabled(&self, enabled: bool) {
        self.dnd_enabled.set(enabled);
        if let Some(ui) = self.ui.borrow().as_ref() {
            if enabled {
                ui.container.class_list_remove("no-drag");
            } else {
                ui.container.class_list_add("no-drag");
            }
        }
    }

    fn container_size(&self) -> ISize {
        self.container().size()
    }

    fn widget_size(&self) -> ISize {
        self.ui.borrow().as_ref().map_or(ISize { width: 0.0, height: 0.0 }, |ui| ui.container.size())
    }

    fn top_snap_value(&self) -> f64 {
        self.title_bar_offset.get()
    }

    fn center_y_snap_value(&self) -> f64 {
        (self.container_size().height * 0.25).round()
    }

    fn center_x_snap_value(&self) -> f64 {
        (self.container_size().width / 2.0).round() - (self.widget_size().width / 2.0).round()
    }

    /// `layoutContainer(dimension)`.
    pub fn dnd_layout_container(&self) {
        if !self.dnd_enabled.get() {
            return;
        }
        if let Some(DndViewState { top: Some(top), left: Some(left), .. }) = self.dnd_state.get() {
            let dimension = self.container_size();
            let a = (left * 1e2).round() / 1e2;
            let d = a * dimension.width - self.widget_size().width / 2.0;
            self.dnd_layout(top * dimension.height, d);
        }
    }

    /// `setAlignment(alignment, done)`.
    pub fn dnd_set_alignment(&self, alignment: QuickInputAlignmentSetting, done: bool) {
        let container = self.container_size();
        let widget = self.widget_size();
        match alignment {
            QuickInputAlignmentSetting::Top => {
                self.dnd_set_state(DndViewState {
                    top: Some(self.top_snap_value() / container.height),
                    left: Some((self.center_x_snap_value() + widget.width / 2.0) / container.width),
                    done,
                });
                self.set_dnd_alignment_state(Some(QuickInputAlignment::Top));
            }
            QuickInputAlignmentSetting::Center => {
                self.dnd_set_state(DndViewState {
                    top: Some(self.center_y_snap_value() / container.height),
                    left: Some((self.center_x_snap_value() + widget.width / 2.0) / container.width),
                    done,
                });
                self.set_dnd_alignment_state(Some(QuickInputAlignment::Center));
            }
            QuickInputAlignmentSetting::Custom { top, left } => {
                self.dnd_set_state(DndViewState { top: Some(top), left: Some(left), done });
                self.set_dnd_alignment_state(None);
            }
        }
    }

    /// O clique duplo na área de arrastar: volta ao topo.
    pub fn dnd_reset(&self) {
        if !self.dnd_enabled.get() {
            return;
        }
        self.dnd_set_state(DndViewState { top: None, left: None, done: true });
        self.set_dnd_alignment_state(Some(QuickInputAlignment::Top));
    }

    /// O mouse arrastou o widget para `(top, left)` em coordenadas do contêiner (`mousemove` com o botão apertado).
    pub fn dnd_drag_to(&self, top: f64, left: f64) {
        if self.dnd_enabled.get() {
            self.dnd_layout(top, left);
        }
    }

    /// O botão do mouse foi solto depois de arrastar: guarda a posição.
    pub fn dnd_drop(&self) {
        if self.dnd_enabled.get() {
            let state = self.dnd_state.get();
            self.dnd_set_state(DndViewState { top: state.and_then(|state| state.top), left: state.and_then(|state| state.left), done: true });
        }
    }

    fn dnd_layout(&self, top_coordinate: f64, left_coordinate: f64) {
        const SNAP_THRESHOLD: f64 = 20.0;
        let container = self.container_size();
        let widget = self.widget_size();
        let snap_y_top = self.top_snap_value();
        let snap_y = self.center_y_snap_value();
        let snap_x = self.center_x_snap_value();
        // Make sure the quick input is not moved outside the container
        let mut top_coordinate = top_coordinate.min(container.height - widget.height).max(0.0);

        let snapping_to_top = (top_coordinate - snap_y_top).abs() < SNAP_THRESHOLD;
        if snapping_to_top {
            top_coordinate = snap_y_top;
        }
        let snapping_to_center = (top_coordinate - snap_y).abs() < SNAP_THRESHOLD;
        if snapping_to_center {
            top_coordinate = snap_y;
        }
        let top = top_coordinate / container.height;

        // Make sure the quick input is not moved outside the container
        let mut left_coordinate = left_coordinate.min(container.width - widget.width).max(0.0);
        let snapping_to_center_x = (left_coordinate - snap_x).abs() < SNAP_THRESHOLD;
        if snapping_to_center_x {
            left_coordinate = snap_x;
        }
        let left = (left_coordinate + widget.width / 2.0) / container.width;

        self.dnd_set_state(DndViewState { top: Some(top), left: Some(left), done: false });
        if snapping_to_center_x {
            if snapping_to_top {
                self.set_dnd_alignment_state(Some(QuickInputAlignment::Top));
                return;
            } else if snapping_to_center {
                self.set_dnd_alignment_state(Some(QuickInputAlignment::Center));
                return;
            }
        }
        self.set_dnd_alignment_state(None);
    }
}

impl IDisposable for QuickInputController {
    /// `dispose()`.
    fn dispose(&self) {
        self.complete_close_animation();
        self.base.dispose();
    }
}
