//! Porte de `src/vs/workbench/contrib/onboarding/browser/spotlight/spotlightOverlay.ts`: o overlay do spotlight, que
//! escurece a janela, abre um buraco de destaque em volta do alvo e mostra um balão ancorado.
//!
//! Divergências (a UI é nativa, sem DOM):
//! - **A árvore de elementos vira estado.** `.spotlight-overlay`, `.spotlight-blocker`, `.spotlight-hole`,
//!   `.spotlight-callout-pointer`, `.spotlight-callout` e os filhos dele são campos do [`SpotlightOverlay`]: o
//!   `display` da raiz ([`SpotlightOverlay::is_hidden`]), a classe `target-overlay-visible`, o retângulo do buraco
//!   ([`SpotlightOverlay::hole`]), os quatro bloqueadores, o balão, o ponteiro, o título, a descrição e o contador.
//!   O contêiner (`HTMLElement`) é o [`SpotlightContainer`], que guarda os overlays anexados a ele (o
//!   `querySelectorAll` do original).
//! - **Os três `Button`** são [`SpotlightButton`]: o rótulo, o `display`, o `enabled`, o clique e o Escape. Cada um
//!   tem um [`OnboardingElement`] conectado ao documento da janela, que é a identidade de foco e de tabulação.
//! - **O tamanho do balão** (`offsetWidth` e `offsetHeight`) vem de quem desenha: [`SpotlightOverlay::set_callout_size`]
//!   (zero até a primeira medição). O posicionamento usa o `layout2d` do `je-base`.
//! - **`ResizeObserver`, `resize` e `scroll` da janela** não existem: a mudança de tamanho ou de posição do alvo é
//!   vista pelo `animate` por quadro (a posição e o tamanho são comparados a cada quadro), e quem desenha chama
//!   [`SpotlightOverlay::schedule_layout`] quando a janela muda.
//! - **A descrição em markdown** guarda o [`SpotlightDescription`] como veio; quem desenha o markdown informa os links
//!   focáveis por [`SpotlightOverlay::set_description_links`], e a armadilha de foco os põe entre o alvo e os botões.
//!   A subárvore do alvo não existe: o alvo é o único focável dele. O `keydown` do balão é
//!   [`SpotlightOverlay::callout_key_down`].
//! - **`renderMarkdown`** não tem equivalente: o desenho do markdown é de quem pinta o balão.
//! - **`localize`** é o texto em inglês do original.
//! - `extends Disposable` é o `store` do overlay; descartar o overlay tira a raiz do contêiner, depois descarta o
//!   `store` (que restaura o foco por último, como o `_register({ dispose: () => this._restoreFocus() })`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::layout::{AnchorAlignment, AnchorAxisAlignment, AnchorPosition, ILayout2DOptions, IRect, ISize, layout2d};
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::runtime;

use super::onboarding_scenario::OnboardingDismissReason;
use super::onboarding_target::{
    OnboardingElement, OnboardingEvent, OnboardingEventKind, OnboardingFocusTarget, OnboardingKey, OnboardingWindow,
};
use super::spotlight_types::{SpotlightDescription, SpotlightPlacement, SpotlightTargetClickBehavior};

/// How the user advanced to the next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightAdvanceSource {
    Button,
    Target,
}

/// Why a step ended in a skip: the Skip button or the Escape key.
pub type SpotlightSkipReason = OnboardingDismissReason;

/// Default padding (px) added around the target when cutting the highlight hole.
const DEFAULT_HOLE_PADDING: f64 = 6.0;
const POINTER_SIZE: f64 = 10.0;
const POINTER_GAP: f64 = POINTER_SIZE;
const POINTER_EDGE_MARGIN: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// `ISpotlightContent`: o conteúdo do balão de um passo.
#[derive(Debug, Clone)]
pub struct SpotlightContent {
    pub title: String,
    pub description: SpotlightDescription,
    /// Localized primary button label, replacing the default Next or Done.
    pub next_button_label: Option<String>,
    /// Zero-based index of the current step.
    pub step_index: usize,
    /// Total number of steps in the tour. Progress is displayed only for multi-step tours.
    pub step_count: usize,
    /// Whether a "Back" action should be offered.
    pub can_go_back: bool,
    /// Whether this is the final step (the primary button becomes "Done").
    pub is_last_step: bool,
    /// Keeps cancellation available when the final primary button performs an action.
    pub show_end_tour: bool,
}

/// `ISpotlightShowOptions`: como um passo é mostrado.
#[derive(Clone, Default)]
pub struct SpotlightShowOptions {
    pub placement: Option<SpotlightPlacement>,
    pub allow_target_interaction: Option<bool>,
    pub padding: Option<f64>,
    pub hide_next: Option<bool>,
    pub target_overlay_visible: Option<bool>,
    /// Advances on target activation; `advanceOnly` consumes the activation without running its action.
    pub advance_on_target_click: Option<SpotlightTargetClickBehavior>,
    /// Uses the control's activation event instead of DOM clicks when advancing after its action.
    pub on_did_activate_target: Option<Event<()>>,
    pub focus_target: Option<OnboardingFocusTarget>,
}

fn advance_on_click(options: &SpotlightShowOptions) -> bool {
    match options.advance_on_target_click {
        Some(SpotlightTargetClickBehavior::Bool(value)) => value,
        Some(SpotlightTargetClickBehavior::AdvanceOnly) => true,
        None => false,
    }
}

fn advance_only(options: &SpotlightShowOptions) -> bool {
    options.advance_on_target_click == Some(SpotlightTargetClickBehavior::AdvanceOnly)
}

/// Os botões do balão (`Button` do original).
pub struct SpotlightButton {
    element: OnboardingElement,
    label: RefCell<String>,
    title: RefCell<Option<String>>,
    enabled: Cell<bool>,
    on_did_click: Rc<Emitter<()>>,
    on_did_escape: Rc<Emitter<()>>,
    /// O `keydown` que sobe do botão para o balão.
    bubble: Rc<dyn Fn(OnboardingKey, bool, &OnboardingElement)>,
}

impl SpotlightButton {
    fn new(window: &OnboardingWindow, label: &str, bubble: Rc<dyn Fn(OnboardingKey, bool, &OnboardingElement)>) -> Rc<Self> {
        let element = OnboardingElement::new(window);
        element.append_to_body();
        Rc::new(Self {
            element,
            label: RefCell::new(label.to_string()),
            title: RefCell::new(None),
            enabled: Cell::new(true),
            on_did_click: Emitter::new(),
            on_did_escape: Emitter::new(),
            bubble,
        })
    }

    /// O elemento do botão (identidade de foco e de tabulação).
    pub fn element(&self) -> &OnboardingElement {
        &self.element
    }

    /// `textContent`.
    pub fn label(&self) -> String {
        self.label.borrow().clone()
    }

    /// `getTitle()`.
    pub fn title(&self) -> Option<String> {
        self.title.borrow().clone()
    }

    /// Se `style.display` é `'none'`.
    pub fn is_hidden(&self) -> bool {
        self.element.is_display_none()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// `element.click()`: um botão desabilitado engole o clique.
    pub fn click(&self) {
        if self.enabled.get() {
            self.on_did_click.fire(());
        }
    }

    /// `keydown` no botão: o Escape é do botão (`onDidEscape`) e não sobe; as outras teclas sobem para o balão.
    pub fn key_down(&self, key: OnboardingKey, shift_key: bool) {
        if key == OnboardingKey::Escape {
            self.on_did_escape.fire(());
        } else {
            (self.bubble)(key, shift_key, &self.element);
        }
    }

    fn set_label(&self, label: &str) {
        *self.label.borrow_mut() = label.to_string();
    }
}

impl IDisposable for SpotlightButton {
    fn dispose(&self) {
        self.on_did_click.dispose();
        self.on_did_escape.dispose();
        self.element.remove();
    }
}

/// O contêiner que hospeda os overlays (o `HTMLElement` de `layoutService.getContainer`).
#[derive(Default)]
pub struct SpotlightContainer {
    overlays: RefCell<Vec<Rc<SpotlightOverlay>>>,
}

impl SpotlightContainer {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `getElementsByClassName('spotlight-overlay')`.
    pub fn overlays(&self) -> Vec<Rc<SpotlightOverlay>> {
        self.overlays.borrow().clone()
    }

    /// `querySelector('.spotlight-callout')` existe.
    pub fn has_callout(&self) -> bool {
        !self.overlays.borrow().is_empty()
    }

    /// `querySelectorAll('.monaco-button')`: os botões de cada overlay, na ordem Encerrar, Voltar, Próximo.
    pub fn buttons(&self) -> Vec<Rc<SpotlightButton>> {
        self.overlays.borrow().iter().flat_map(|overlay| overlay.buttons()).collect()
    }

    fn append(&self, overlay: Rc<SpotlightOverlay>) {
        self.overlays.borrow_mut().push(overlay);
    }

    fn remove(&self, overlay: &SpotlightOverlay) {
        self.overlays.borrow_mut().retain(|current| !std::ptr::eq(Rc::as_ptr(current), overlay));
    }
}

/// `SpotlightOverlay`.
pub struct SpotlightOverlay {
    weak_self: Weak<SpotlightOverlay>,
    window: OnboardingWindow,
    container: Rc<SpotlightContainer>,
    store: Rc<DisposableStore>,

    root_hidden: Cell<bool>,
    target_overlay_visible: Cell<bool>,
    hole: Cell<Option<IRect>>,
    blockers: RefCell<[Option<IRect>; 4]>,
    callout: Cell<IRect>,
    callout_modal: Cell<bool>,
    callout_size: Cell<ISize>,
    pointer: Cell<Option<(PointerSide, f64, f64)>>,

    title: RefCell<String>,
    description: RefCell<SpotlightDescription>,
    counter: RefCell<String>,
    /// Os links focáveis que o desenho do markdown da descrição criou (o `a[href]` do `querySelectorAll`).
    description_links: RefCell<Vec<OnboardingElement>>,

    back_button: Rc<SpotlightButton>,
    next_button: Rc<SpotlightButton>,
    skip_button: Rc<SpotlightButton>,

    /// Listeners scoped to the currently shown step (re-layout sources).
    step_listeners: Rc<DisposableStore>,

    on_did_click_next: Rc<Emitter<SpotlightAdvanceSource>>,
    on_did_click_previous: Rc<Emitter<()>>,
    on_did_skip: Rc<Emitter<SpotlightSkipReason>>,
    on_did_lose_target: Rc<Emitter<()>>,

    target: RefCell<Option<OnboardingElement>>,
    options: RefCell<SpotlightShowOptions>,
    has_shown: Cell<bool>,
    previous_focus: RefCell<Option<OnboardingElement>>,
    scheduled_layout: RefCell<Option<je_base::lifecycle::DisposableRef>>,
}

impl SpotlightOverlay {
    pub fn new(window: &OnboardingWindow, container: &Rc<SpotlightContainer>) -> Rc<Self> {
        let store = DisposableStore::new();
        let overlay = Rc::new_cyclic(|weak: &Weak<SpotlightOverlay>| {
            let bubble: Rc<dyn Fn(OnboardingKey, bool, &OnboardingElement)> = {
                let weak = weak.clone();
                Rc::new(move |key: OnboardingKey, shift_key: bool, element: &OnboardingElement| {
                    if let Some(overlay) = weak.upgrade() {
                        let event = OnboardingEvent::new(OnboardingEventKind::KeyDown, key, shift_key, element.clone());
                        overlay.on_key_down(&event);
                    }
                })
            };
            Self {
                weak_self: weak.clone(),
                window: window.clone(),
                container: container.clone(),
                store: store.clone(),
                root_hidden: Cell::new(true),
                target_overlay_visible: Cell::new(false),
                hole: Cell::new(None),
                blockers: RefCell::new([None; 4]),
                callout: Cell::new(IRect { top: 0.0, left: 0.0, width: 0.0, height: 0.0 }),
                callout_modal: Cell::new(true),
                callout_size: Cell::new(ISize { width: 0.0, height: 0.0 }),
                pointer: Cell::new(None),
                title: RefCell::new(String::new()),
                description: RefCell::new(SpotlightDescription::default()),
                counter: RefCell::new(String::new()),
                description_links: RefCell::new(Vec::new()),
                skip_button: SpotlightButton::new(window, "End Tour", bubble.clone()),
                back_button: SpotlightButton::new(window, "Back", bubble.clone()),
                next_button: SpotlightButton::new(window, "Next", bubble),
                step_listeners: DisposableStore::new(),
                on_did_click_next: Emitter::new(),
                on_did_click_previous: Emitter::new(),
                on_did_skip: Emitter::new(),
                on_did_lose_target: Emitter::new(),
                target: RefCell::new(None),
                options: RefCell::new(SpotlightShowOptions::default()),
                has_shown: Cell::new(false),
                previous_focus: RefCell::new(None),
                scheduled_layout: RefCell::new(None),
            }
        });
        *overlay.skip_button.title.borrow_mut() = Some("End Tour (Esc)".to_string());

        overlay.store.add_rc(overlay.step_listeners.clone());
        overlay.store.add_rc(overlay.on_did_click_next.clone());
        overlay.store.add_rc(overlay.on_did_click_previous.clone());
        overlay.store.add_rc(overlay.on_did_skip.clone());
        overlay.store.add_rc(overlay.on_did_lose_target.clone());
        for button in [&overlay.skip_button, &overlay.back_button, &overlay.next_button] {
            overlay.store.add_rc(button.clone());
        }

        let weak = overlay.weak_self.clone();
        overlay.store.add(overlay.skip_button.on_did_click.event().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(overlay) = weak.upgrade() {
                    overlay.on_did_skip.fire(OnboardingDismissReason::SkipButton);
                }
            }
        }));
        overlay.store.add(overlay.back_button.on_did_click.event().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(overlay) = weak.upgrade() {
                    overlay.on_did_click_previous.fire(());
                }
            }
        }));
        overlay.store.add(overlay.next_button.on_did_click.event().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(overlay) = weak.upgrade() {
                    overlay.on_did_click_next.fire(SpotlightAdvanceSource::Button);
                }
            }
        }));
        // Buttons swallow Escape internally, so route their escape events to end the tour too.
        for button in [&overlay.skip_button, &overlay.back_button, &overlay.next_button] {
            let weak = weak.clone();
            overlay.store.add(button.on_did_escape.event().listen(move |_| {
                if let Some(overlay) = weak.upgrade() {
                    overlay.on_did_skip.fire(OnboardingDismissReason::EscapeKey);
                }
            }));
        }
        overlay.store.add(to_disposable({
            let weak = weak.clone();
            move || {
                if let Some(overlay) = weak.upgrade() {
                    overlay.restore_focus();
                }
            }
        }));

        container.append(overlay.clone());
        overlay
    }

    pub fn on_did_click_next(&self) -> Event<SpotlightAdvanceSource> {
        self.on_did_click_next.event()
    }

    pub fn on_did_click_previous(&self) -> Event<()> {
        self.on_did_click_previous.event()
    }

    pub fn on_did_skip(&self) -> Event<SpotlightSkipReason> {
        self.on_did_skip.event()
    }

    pub fn on_did_lose_target(&self) -> Event<()> {
        self.on_did_lose_target.event()
    }

    /// Os botões, na ordem do DOM: Encerrar, Voltar, Próximo.
    pub fn buttons(&self) -> Vec<Rc<SpotlightButton>> {
        vec![self.skip_button.clone(), self.back_button.clone(), self.next_button.clone()]
    }

    /// `style.display === 'none'` da raiz.
    pub fn is_hidden(&self) -> bool {
        self.root_hidden.get()
    }

    /// A classe `target-overlay-visible` da raiz.
    pub fn has_target_overlay_visible(&self) -> bool {
        self.target_overlay_visible.get()
    }

    /// O retângulo do `.spotlight-hole` (`left`, `top`, `width`, `height` em px), depois do primeiro layout.
    pub fn hole(&self) -> Option<IRect> {
        self.hole.get()
    }

    /// O `style.top` do `.spotlight-hole`, por exemplo `194px`.
    pub fn hole_top_style(&self) -> Option<String> {
        self.hole.get().map(|hole| format!("{}px", hole.top))
    }

    /// Os quatro bloqueadores (`None` é `display: none`).
    pub fn blockers(&self) -> [Option<IRect>; 4] {
        *self.blockers.borrow()
    }

    /// A posição do balão.
    pub fn callout(&self) -> IRect {
        self.callout.get()
    }

    /// O `aria-modal` do balão.
    pub fn is_callout_modal(&self) -> bool {
        self.callout_modal.get()
    }

    /// O lado, o `left` e o `top` do ponteiro.
    pub fn pointer(&self) -> Option<(PointerSide, f64, f64)> {
        self.pointer.get()
    }

    /// `textContent` do `.spotlight-callout-title`.
    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    /// O conteúdo do `.spotlight-callout-description`.
    pub fn description(&self) -> SpotlightDescription {
        self.description.borrow().clone()
    }

    /// `textContent` do `.spotlight-callout-counter`.
    pub fn counter(&self) -> String {
        self.counter.borrow().clone()
    }

    /// O `offsetWidth` e o `offsetHeight` do balão, medidos por quem desenha.
    pub fn set_callout_size(&self, width: f64, height: f64) {
        self.callout_size.set(ISize { width, height });
    }

    /// Informa os controles focáveis que quem desenha o markdown criou na descrição, em ordem de documento. Valem até o
    /// próximo `show` (o original refaz a subárvore da descrição a cada passo).
    pub fn set_description_links(&self, links: Vec<OnboardingElement>) {
        *self.description_links.borrow_mut() = links;
    }

    /// O `keydown` que chega ao próprio balão (o listener de `this._callout`): Escape encerra e Tab prende o foco.
    pub fn callout_key_down(&self, key: OnboardingKey, shift_key: bool) {
        let event = OnboardingEvent::new(OnboardingEventKind::KeyDown, key, shift_key, OnboardingElement::new(&self.window));
        self.on_key_down(&event);
    }

    /// Show `content` spotlighting `target`.
    pub fn show(&self, target: &OnboardingElement, content: &SpotlightContent, options: SpotlightShowOptions) {
        if !self.has_shown.get() {
            self.has_shown.set(true);
            *self.previous_focus.borrow_mut() = self.window.document().active_element();
        }

        *self.target.borrow_mut() = Some(target.clone());
        *self.options.borrow_mut() = options.clone();
        self.set_primary_action_enabled(true);
        self.render_content(content);
        let advance = advance_on_click(&options);
        let advance_only = advance_only(&options);
        let external_ui_participates = options.target_overlay_visible == Some(true)
            || options.allow_target_interaction == Some(true)
            || advance
            || options.hide_next == Some(true);
        self.target_overlay_visible.set(external_ui_participates);
        self.callout_modal.set(!external_ui_participates);

        self.root_hidden.set(false);

        // Rebuild the per-step re-layout listeners.
        self.step_listeners.clear();
        let focus_element = options.focus_target.as_ref().map_or_else(|| target.clone(), |focus_target| focus_target.element.clone());

        if external_ui_participates {
            let (weak, target, focus_element) = (self.weak_self.clone(), target.clone(), focus_element.clone());
            self.step_listeners.add(self.window.add_key_capture_listener(move |event| {
                let Some(overlay) = weak.upgrade() else {
                    return;
                };
                if event.kind != OnboardingEventKind::KeyDown {
                    return;
                }
                let inside_root = overlay.buttons().iter().any(|button| *button.element() == event.target);
                if inside_root || target == event.target || focus_element == event.target {
                    return;
                }
                if event.key == OnboardingKey::Escape {
                    overlay.on_did_skip.fire(OnboardingDismissReason::EscapeKey);
                }
            }));
        }

        // ResizeObserver does not report position-only shifts caused by surrounding content.
        let previous_rect = Rc::new(Cell::new(target.bounding_rect()));
        self.step_listeners.add(self.window.animate({
            let (weak, target, previous_rect) = (self.weak_self.clone(), target.clone(), previous_rect);
            move || {
                let rect = target.bounding_rect();
                if rect != previous_rect.get() {
                    previous_rect.set(rect);
                    if let Some(overlay) = weak.upgrade() {
                        overlay.layout();
                    }
                }
            }
        }));

        // Cancel any pending scheduled frame when the step changes.
        self.step_listeners.add(to_disposable({
            let weak = self.weak_self.clone();
            move || {
                if let Some(overlay) = weak.upgrade() {
                    let scheduled = overlay.scheduled_layout.borrow_mut().take();
                    if let Some(scheduled) = scheduled {
                        scheduled.dispose();
                    }
                }
            }
        }));

        let hide_next = options.hide_next.unwrap_or(advance);
        self.next_button.element.set_display_none(hide_next);
        if advance {
            let advance_after_activation: Rc<dyn Fn()> = {
                let weak = self.weak_self.clone();
                let step_listeners = self.step_listeners.clone();
                Rc::new(move || {
                    let weak = weak.clone();
                    let handle = runtime::set_timeout(
                        move || {
                            if let Some(overlay) = weak.upgrade() {
                                *overlay.previous_focus.borrow_mut() = None;
                                overlay.on_did_click_next.fire(SpotlightAdvanceSource::Target);
                            }
                        },
                        0,
                    );
                    step_listeners.add(to_disposable(move || runtime::clear_timeout(handle)));
                })
            };
            match (&options.on_did_activate_target, advance_only) {
                (Some(on_did_activate_target), false) => {
                    let advance_after_activation = advance_after_activation.clone();
                    self.step_listeners.add(on_did_activate_target.listen(move |_| advance_after_activation()));
                }
                _ => {
                    let weak = self.weak_self.clone();
                    self.step_listeners.add(target.add_event_listener(OnboardingEventKind::Click, true, move |event| {
                        if advance_only {
                            event.prevent_default();
                            event.stop_immediate_propagation();
                            if let Some(overlay) = weak.upgrade() {
                                overlay.on_did_click_next.fire(SpotlightAdvanceSource::Target);
                            }
                            return;
                        }
                        advance_after_activation();
                    }));
                }
            }
        }
        if advance_only {
            for kind in [OnboardingEventKind::KeyDown, OnboardingEventKind::KeyUp] {
                let weak = self.weak_self.clone();
                self.step_listeners.add(focus_element.add_event_listener(kind, true, move |event| {
                    let Some(overlay) = weak.upgrade() else {
                        return;
                    };
                    if event.key == OnboardingKey::Enter || event.key == OnboardingKey::Space {
                        event.prevent_default();
                        event.stop_immediate_propagation();
                        if event.kind == OnboardingEventKind::KeyUp {
                            overlay.on_did_click_next.fire(SpotlightAdvanceSource::Target);
                        }
                    } else if event.kind == OnboardingEventKind::KeyDown {
                        overlay.on_key_down(event);
                    }
                }));
            }
        } else if options.allow_target_interaction == Some(true) || advance || options.hide_next == Some(true) {
            let weak = self.weak_self.clone();
            self.step_listeners.add(focus_element.add_event_listener(OnboardingEventKind::KeyDown, false, move |event| {
                if let Some(overlay) = weak.upgrade() {
                    overlay.on_key_down(event);
                }
            }));
        }

        self.layout();

        // Move focus to the spotlighted control (so keyboard users can activate it
        // to advance) or, otherwise, into the callout's primary action.
        self.focus(if hide_next { &focus_element } else { &self.next_button.element });
    }

    /// Prevents duplicate activation while waiting for an action to be accepted.
    pub fn set_primary_action_enabled(&self, enabled: bool) {
        self.next_button.enabled.set(enabled);
    }

    /// Hide the current step while another target is being resolved.
    pub fn hide(&self) {
        self.step_listeners.clear();
        self.root_hidden.set(true);
        self.target_overlay_visible.set(false);
        *self.target.borrow_mut() = None;
        *self.options.borrow_mut() = SpotlightShowOptions::default();
    }

    /// Recompute the hole and callout positions for the current target.
    pub fn layout(&self) {
        let Some(target) = self.target.borrow().clone() else {
            return;
        };
        if self.root_hidden.get() {
            return;
        }

        let (viewport_width, viewport_height) = self.window.viewport();

        let (left, top, mut width, mut height) = target.bounding_rect();
        if target.is_display_none() {
            width = 0.0;
            height = 0.0;
        }
        if !target.is_connected() || width == 0.0 || height == 0.0 {
            self.root_hidden.set(true);
            self.on_did_lose_target.fire(());
            return;
        }
        let options = self.options.borrow().clone();
        let padding = options.padding.unwrap_or(DEFAULT_HOLE_PADDING);
        let hole_left = (left - padding).max(0.0);
        let hole_top = (top - padding).max(0.0);
        let hole_width = (viewport_width - hole_left).min(width + padding * 2.0);
        let hole_height = (viewport_height - hole_top).min(height + padding * 2.0);

        self.hole.set(Some(IRect { top: hole_top, left: hole_left, width: hole_width, height: hole_height }));

        // When the target is interactive (explicitly, or because the step advances
        // on a target click), arrange the click blockers around the hole so events
        // inside it reach the underlying element.
        let mut blockers = self.blockers.borrow_mut();
        if options.allow_target_interaction == Some(true) || advance_on_click(&options) {
            let right = hole_left + hole_width;
            let bottom = hole_top + hole_height;
            blockers[0] = Some(blocker_rect(0.0, 0.0, viewport_width, hole_top));
            blockers[1] = Some(blocker_rect(right, hole_top, viewport_width - right, hole_height));
            blockers[2] = Some(blocker_rect(0.0, bottom, viewport_width, viewport_height - bottom));
            blockers[3] = Some(blocker_rect(0.0, hole_top, hole_left, hole_height));
        } else {
            blockers[0] = Some(blocker_rect(0.0, 0.0, viewport_width, viewport_height));
            for blocker in blockers.iter_mut().skip(1) {
                *blocker = None;
            }
        }
        drop(blockers);

        self.layout_callout(
            IRect { top: hole_top, left: hole_left, width: hole_width, height: hole_height },
            viewport_width,
            viewport_height,
            options.placement.unwrap_or(SpotlightPlacement::Auto),
        );
    }

    fn layout_callout(&self, anchor: IRect, viewport_width: f64, viewport_height: f64, placement: SpotlightPlacement) {
        let viewport = IRect { top: 0.0, left: 0.0, width: viewport_width, height: viewport_height };
        let view = self.callout_size.get();

        let (anchor_axis_alignment, anchor_position, anchor_alignment) = resolve_placement(placement);
        let result = layout2d(
            viewport,
            view,
            anchor,
            Some(ILayout2DOptions {
                anchor_alignment: Some(anchor_alignment),
                anchor_position: Some(anchor_position),
                anchor_axis_alignment: Some(anchor_axis_alignment),
            }),
        );

        let left = if anchor_axis_alignment == AnchorAxisAlignment::Vertical {
            center_callout(anchor, view.width, viewport_width)
        } else {
            result.left
        };
        let callout = IRect { top: result.top, left, width: view.width, height: view.height };
        let pointer_side = get_pointer_side(anchor, callout, anchor_axis_alignment);
        let offset_callout = offset_callout_for_pointer(callout, pointer_side, viewport_width, viewport_height);

        self.callout.set(offset_callout);
        self.layout_pointer(anchor, offset_callout, pointer_side);
    }

    fn layout_pointer(&self, anchor: IRect, callout: IRect, side: PointerSide) {
        let target_center_x = anchor.left + anchor.width / 2.0;
        let target_center_y = anchor.top + anchor.height / 2.0;
        let pointer_offset = POINTER_SIZE / 2.0;

        if side == PointerSide::Top || side == PointerSide::Bottom {
            let pointer_center_x = clamp(target_center_x, callout.left + POINTER_EDGE_MARGIN, callout.left + callout.width - POINTER_EDGE_MARGIN);
            let top = if side == PointerSide::Bottom { callout.top + callout.height - pointer_offset } else { callout.top - pointer_offset };
            self.pointer.set(Some((side, pointer_center_x - pointer_offset, top)));
            return;
        }

        let pointer_center_y = clamp(target_center_y, callout.top + POINTER_EDGE_MARGIN, callout.top + callout.height - POINTER_EDGE_MARGIN);
        let left = if side == PointerSide::Right { callout.left + callout.width - pointer_offset } else { callout.left - pointer_offset };
        self.pointer.set(Some((side, left, pointer_center_y - pointer_offset)));
    }

    fn render_content(&self, content: &SpotlightContent) {
        *self.title.borrow_mut() = content.title.clone();
        *self.description.borrow_mut() = content.description.clone();
        self.description_links.borrow_mut().clear();
        *self.counter.borrow_mut() =
            if content.step_count > 1 { format!("{} of {}", content.step_index + 1, content.step_count) } else { String::new() };

        self.skip_button.element.set_display_none(content.is_last_step && !content.show_end_tour);
        self.back_button.element.set_display_none(!content.can_go_back);
        self.next_button.set_label(&content.next_button_label.clone().unwrap_or_else(|| {
            if content.is_last_step { "Done".to_string() } else { "Next".to_string() }
        }));
    }

    fn on_key_down(&self, event: &OnboardingEvent) {
        if event.key == OnboardingKey::Escape {
            event.stop_propagation();
            event.prevent_default();
            self.on_did_skip.fire(OnboardingDismissReason::EscapeKey);
            return;
        }

        if event.key == OnboardingKey::Tab {
            self.trap_focus(event);
        }
    }

    fn trap_focus(&self, event: &OnboardingEvent) {
        let focusable = self.collect_focusable();
        if focusable.is_empty() {
            return;
        }

        let active = self.window.document().active_element();
        let current_index = focusable.iter().position(|element| Some(element) == active.as_ref());

        // When focus isn't currently on a tracked element (e.g. it landed on the
        // callout container itself), start from the appropriate end so Tab goes to
        // the first element and Shift+Tab to the last.
        let next_index = match current_index {
            None => {
                if event.shift_key { focusable.len() - 1 } else { 0 }
            }
            Some(current) => {
                if event.shift_key { (current + focusable.len() - 1) % focusable.len() } else { (current + 1) % focusable.len() }
            }
        };

        event.prevent_default();
        event.stop_propagation();
        self.focus(&focusable[next_index]);
    }

    fn focus(&self, element: &OnboardingElement) {
        let focus_target = self.options.borrow().focus_target.clone();
        match focus_target {
            Some(focus_target) if focus_target.element == *element => (focus_target.focus)(),
            _ => element.focus(),
        }
    }

    /// The focusable elements participating in the focus trap, in order: the spotlighted target (when it is
    /// interactive or the Next button is hidden), then the visible action buttons.
    fn collect_focusable(&self) -> Vec<OnboardingElement> {
        let options = self.options.borrow().clone();
        let target = self.target.borrow().clone();
        let interactive = options.allow_target_interaction == Some(true) || advance_on_click(&options) || options.hide_next == Some(true);
        let target_focusables = match (&target, interactive) {
            (Some(target), true) => match &options.focus_target {
                Some(focus_target) => vec![focus_target.element.clone()],
                None => vec![target.clone()],
            },
            _ => Vec::new(),
        };
        let description_focusables = self.description_links.borrow().clone();
        let buttons = [&self.skip_button, &self.back_button, &self.next_button]
            .into_iter()
            .filter(|button| !button.is_hidden())
            .map(|button| button.element.clone());
        target_focusables.into_iter().chain(description_focusables).chain(buttons).filter(is_tabbable).collect()
    }

    pub fn schedule_layout(&self) {
        if self.scheduled_layout.borrow().is_some() {
            return;
        }
        let weak = self.weak_self.clone();
        let scheduled = self.window.schedule_at_next_animation_frame(move || {
            if let Some(overlay) = weak.upgrade() {
                *overlay.scheduled_layout.borrow_mut() = None;
                overlay.layout();
            }
        });
        *self.scheduled_layout.borrow_mut() = Some(scheduled);
    }

    fn restore_focus(&self) {
        let previous = self.previous_focus.borrow_mut().take();
        if let Some(previous) = previous
            && previous.is_connected()
        {
            previous.focus();
        }
    }
}

impl IDisposable for SpotlightOverlay {
    fn dispose(&self) {
        self.container.remove(self);
        self.store.dispose();
    }
}

fn is_tabbable(element: &OnboardingElement) -> bool {
    element.is_connected() && element.tab_index() != -1 && !element.is_display_none() && !element.is_visibility_hidden()
}

fn blocker_rect(left: f64, top: f64, width: f64, height: f64) -> IRect {
    IRect { top, left, width: width.max(0.0), height: height.max(0.0) }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    min.max(value.min(max))
}

fn center_callout(anchor: IRect, callout_width: f64, viewport_width: f64) -> f64 {
    let centered = anchor.left + anchor.width / 2.0 - callout_width / 2.0;
    0.0_f64.max(centered.min(viewport_width - callout_width))
}

fn get_pointer_side(anchor: IRect, callout: IRect, anchor_axis_alignment: AnchorAxisAlignment) -> PointerSide {
    let target_center_x = anchor.left + anchor.width / 2.0;
    let target_center_y = anchor.top + anchor.height / 2.0;
    let callout_center_x = callout.left + callout.width / 2.0;
    let callout_center_y = callout.top + callout.height / 2.0;
    if anchor_axis_alignment == AnchorAxisAlignment::Vertical {
        if callout_center_y < target_center_y { PointerSide::Bottom } else { PointerSide::Top }
    } else if callout_center_x < target_center_x {
        PointerSide::Right
    } else {
        PointerSide::Left
    }
}

fn offset_callout_for_pointer(callout: IRect, side: PointerSide, viewport_width: f64, viewport_height: f64) -> IRect {
    match side {
        PointerSide::Bottom => IRect { top: 0.0_f64.max(callout.top - POINTER_GAP), ..callout },
        PointerSide::Top => IRect { top: (viewport_height - callout.height).min(callout.top + POINTER_GAP), ..callout },
        PointerSide::Right => IRect { left: 0.0_f64.max(callout.left - POINTER_GAP), ..callout },
        PointerSide::Left => IRect { left: (viewport_width - callout.width).min(callout.left + POINTER_GAP), ..callout },
    }
}

fn resolve_placement(placement: SpotlightPlacement) -> (AnchorAxisAlignment, AnchorPosition, AnchorAlignment) {
    match placement {
        SpotlightPlacement::Above => (AnchorAxisAlignment::Vertical, AnchorPosition::Above, AnchorAlignment::Left),
        SpotlightPlacement::Left => (AnchorAxisAlignment::Horizontal, AnchorPosition::Below, AnchorAlignment::Right),
        SpotlightPlacement::Right => (AnchorAxisAlignment::Horizontal, AnchorPosition::Below, AnchorAlignment::Left),
        SpotlightPlacement::Below | SpotlightPlacement::Auto => (AnchorAxisAlignment::Vertical, AnchorPosition::Below, AnchorAlignment::Left),
    }
}
