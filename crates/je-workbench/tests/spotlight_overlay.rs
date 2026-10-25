//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/spotlightOverlay.test.ts` (suíte `SpotlightOverlay`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso, dentro do corpo
//!   (que roda em `run_with_real_timers`, porque o overlay agenda quadros e timers); `disposables.add` vira
//!   `guard.add`, `guard.add_rc` ou o `store` do [`Env`].
//! - **DOM.** Não há DOM: `createContainer()` é o [`SpotlightContainer`], `createTarget(container, left, top, width,
//!   height)` é [`Env::target`] (`set_position` e `set_size`) e as consultas por classe viram o estado do overlay
//!   nativo: `.spotlight-overlay` (`style.display`) é `is_hidden()`, `.spotlight-hole` é `hole()`, `.spotlight-blocker`
//!   é `blockers()` (`None` é `display: none`), `.spotlight-callout` é `callout()` e `is_callout_modal()` (o
//!   `aria-modal`), `.spotlight-callout-pointer` é `pointer()`, `.spotlight-callout-title`,
//!   `.spotlight-callout-description` e `.spotlight-callout-counter` são `title()`, `description()` e `counter()`, e
//!   `getElementsByClassName('monaco-button')` é `container.buttons()` (`textContent` é `label()`, `style.display ===
//!   'none'` é `is_hidden()` e `aria-disabled` é o inverso de `is_enabled()`).
//! - **`offsetWidth` e `offsetHeight` do balão** vêm de quem desenha: os casos que posicionam o balão chamam
//!   `set_callout_size` antes do `show`, e as medidas esperadas usam esse tamanho.
//! - **`FakeResizeObserver`.** O porte não tem `ResizeObserver`: a mudança de tamanho ou de posição do alvo é vista
//!   por quadro e a da janela entra por `schedule_layout`. O caso que confere os alvos observados confere isso.
//! - **Eventos.** `element.click()`, `dispatchEvent(new KeyboardEvent(...))` e `addEventListener` são `click`,
//!   `key_down`, `key_up` e `add_event_listener`. O `keydown` do balão é `callout_key_down`; o do botão, `key_down`.
//!   O `Button` de `defaultButtonStyles` é [`Env::button`]: um elemento que dispara `onDidClick` no clique e em
//!   Enter ou Espaço (consumindo o evento), como o `Button` do original.
//! - **Hit test (`elementFromPoint`).** Não há camadas: "o picker recebe o clique" é o overlay ceder à UI externa
//!   (`has_target_overlay_visible` e balão sem `aria-modal`) e os três botões continuarem acionáveis.
//! - **Subárvore do alvo.** O controle aninhado do alvo é o próprio alvo (o ouvinte dele em bolha faz o papel do
//!   `nestedButton`), e o primeiro focável do alvo é o próprio alvo.
//! - **Links do markdown.** Quem desenha o markdown informa os links por `set_description_links`.
//! - **`getAnimations()`.** Não há transição do buraco: o deslocamento é conferido direto no estado.
//! - **`await timeout(0)` e `scheduleAtNextAnimationFrame(mainWindow, resolve)`.** São `timeout(0)` e
//!   [`Env::next_frame`].
//! - **`for (...) test(...)`.** Vira um teste por combinação, na ordem do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers, timeout};
use je_base::event::Emitter;
use je_base::html_content::IMarkdownString;
use je_base::js_string::JsString;
use je_base::layout::IRect;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_workbench::contrib::onboarding::onboarding_scenario::OnboardingDismissReason;
use je_workbench::contrib::onboarding::onboarding_target::{OnboardingElement, OnboardingEventKind, OnboardingKey, OnboardingWindow};
use je_workbench::contrib::onboarding::spotlight_overlay::{
    PointerSide, SpotlightAdvanceSource, SpotlightContainer, SpotlightContent, SpotlightOverlay, SpotlightShowOptions,
};
use je_workbench::contrib::onboarding::spotlight_types::{SpotlightDescription, SpotlightPlacement, SpotlightTargetClickBehavior};

/// `content(overrides)`: os campos que todo caso usa, sobrescritos com `..content()`.
fn content() -> SpotlightContent {
    SpotlightContent {
        title: "My Title".into(),
        description: "My description".into(),
        next_button_label: None,
        step_index: 1,
        step_count: 3,
        can_go_back: true,
        is_last_step: false,
        show_end_tour: false,
    }
}

fn rect(left: f64, top: f64, width: f64, height: f64) -> IRect {
    IRect { top, left, width, height }
}

fn advance_on_click(value: bool) -> Option<SpotlightTargetClickBehavior> {
    Some(SpotlightTargetClickBehavior::Bool(value))
}

fn source_name(source: SpotlightAdvanceSource) -> &'static str {
    match source {
        SpotlightAdvanceSource::Button => "button",
        SpotlightAdvanceSource::Target => "target",
    }
}

fn strings() -> Rc<RefCell<Vec<String>>> {
    Rc::new(RefCell::new(Vec::new()))
}

/// Os rótulos dos botões que não têm `display: none`.
fn visible_buttons(container: &SpotlightContainer) -> Vec<String> {
    container.buttons().iter().filter(|button| !button.is_hidden()).map(|button| button.label()).collect()
}

struct Env {
    store: Rc<DisposableStore>,
    window: OnboardingWindow,
    container: Rc<SpotlightContainer>,
    overlay: Rc<SpotlightOverlay>,
}

/// O `Button` de `defaultButtonStyles`: o elemento e o `onDidClick`.
struct Button {
    element: OnboardingElement,
    on_did_click: Rc<Emitter<()>>,
}

impl Env {
    /// `createContainer()` e o `new SpotlightOverlay(container, FakeResizeObserver)`.
    fn new(guard: &LeakGuard) -> Self {
        let window = OnboardingWindow::new();
        let container = SpotlightContainer::new();
        let overlay = guard.add_rc(SpotlightOverlay::new(&window, &container));
        Self { store: guard.store(), window, container, overlay }
    }

    /// Um elemento conectado, sem caixa.
    fn plain_element(&self) -> OnboardingElement {
        let element = OnboardingElement::new(&self.window);
        element.append_to_body();
        let attached = element.clone();
        self.store.add(to_disposable(move || attached.remove()));
        element
    }

    /// `createTarget(container, left, top, width, height)`.
    fn target(&self, left: f64, top: f64, width: f64, height: f64) -> OnboardingElement {
        let element = self.plain_element();
        element.set_size(width, height);
        element.set_position(left, top);
        element
    }

    /// `new Button(container, defaultButtonStyles)`: dispara `onDidClick` no clique e em Enter ou Espaço.
    fn button(&self, guard: &LeakGuard, left: f64, top: f64, width: f64, height: f64) -> Button {
        let element = self.target(left, top, width, height);
        let on_did_click = guard.add_rc(Emitter::<()>::new());
        guard.add(element.add_event_listener(OnboardingEventKind::Click, false, {
            let on_did_click = on_did_click.clone();
            move |_| on_did_click.fire(())
        }));
        guard.add(element.add_event_listener(OnboardingEventKind::KeyDown, false, {
            let on_did_click = on_did_click.clone();
            move |event| {
                if event.key == OnboardingKey::Enter || event.key == OnboardingKey::Space {
                    on_did_click.fire(());
                    event.prevent_default();
                    event.stop_propagation();
                }
            }
        }));
        Button { element, on_did_click }
    }

    /// `disposables.add(overlay.onDidClickNext(source => events.push(source)))`.
    fn record_next(&self, events: &Rc<RefCell<Vec<String>>>) {
        let events = events.clone();
        self.store.add(self.overlay.on_did_click_next().listen(move |source| events.borrow_mut().push(source_name(*source).to_string())));
    }

    fn record_skip(&self, events: &Rc<RefCell<Vec<String>>>) {
        let events = events.clone();
        self.store.add(self.overlay.on_did_skip().listen(move |_| events.borrow_mut().push("skip".to_string())));
    }

    fn record_previous(&self, events: &Rc<RefCell<Vec<String>>>) {
        let events = events.clone();
        self.store.add(self.overlay.on_did_click_previous().listen(move |_| events.borrow_mut().push("back".to_string())));
    }

    /// `new Promise(resolve => scheduleAtNextAnimationFrame(mainWindow, resolve))`.
    fn next_frame(&self) -> Promise<()> {
        let (window, store) = (self.window.clone(), self.store.clone());
        Promise::with_executor(move |resolver| {
            store.add(window.schedule_at_next_animation_frame(move || resolver.resolve(())));
        })
    }

    /// Os `offsetTop` do buraco, do balão e do ponteiro.
    fn tops(&self) -> [f64; 3] {
        [
            self.overlay.hole().expect("o buraco").top,
            self.overlay.callout().top,
            self.overlay.pointer().expect("o ponteiro").2,
        ]
    }
}

async fn settled(promise: Promise<()>) {
    promise.await.expect("a promessa resolveu");
}

#[test]
fn show_renders_content_and_positions_the_hole_around_the_target_padding() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 50.0, 200.0, 40.0);

        env.overlay.show(&target, &content(), SpotlightShowOptions::default());

        assert_eq!(
            (env.overlay.is_hidden(), env.overlay.title(), env.overlay.description(), env.overlay.counter(), env.overlay.hole()),
            (false, "My Title".to_string(), SpotlightDescription::from("My description"), "2 of 3".to_string(), Some(rect(94.0, 44.0, 212.0, 52.0)))
        );
    });
}

#[test]
fn vertical_placement_centers_the_callout_over_the_target() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(300.0, 200.0, 120.0, 30.0);
        env.overlay.set_callout_size(280.0, 120.0);

        env.overlay.show(&target, &content(), SpotlightShowOptions { placement: Some(SpotlightPlacement::Above), ..Default::default() });

        let callout = env.overlay.callout();
        assert_eq!(
            (callout.left, callout.top + callout.height, env.overlay.pointer()),
            (
                300.0 + 60.0 - (callout.width / 2.0),
                184.0,
                Some((PointerSide::Bottom, 300.0 + 60.0 - 5.0, callout.top + callout.height - 5.0)),
            )
        );
    });
}

#[test]
fn shows_progress_only_for_multi_step_tours_and_clears_it_when_reused_for_one_step() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 50.0, 200.0, 40.0);
        let progress: Vec<String> = [(0, 1), (0, 2), (1, 2), (0, 1)]
            .into_iter()
            .map(|(step_index, step_count)| {
                env.overlay.show(
                    &target,
                    &SpotlightContent { step_index, step_count, can_go_back: step_index > 0, is_last_step: step_index == step_count - 1, ..content() },
                    SpotlightShowOptions::default(),
                );
                env.overlay.counter()
            })
            .collect();

        assert_eq!(progress, vec!["".to_string(), "1 of 2".to_string(), "2 of 2".to_string(), "".to_string()]);
    });
}

#[test]
fn next_back_skip_buttons_fire_the_corresponding_events() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);

        let fired = strings();
        env.record_skip(&fired);
        env.record_previous(&fired);
        env.record_next(&fired);

        env.overlay.show(&target, &content(), SpotlightShowOptions::default());

        // Buttons are appended in order: Skip, Back, Next.
        let buttons = env.container.buttons();
        buttons[0].click();
        buttons[1].click();
        buttons[2].click();

        assert_eq!(*fired.borrow(), vec!["skip".to_string(), "back".to_string(), "button".to_string()]);
    });
}

#[test]
fn primary_actions_and_cancellation_remain_clickable_above_an_expanded_picker() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 100.0, 80.0, 30.0);
        let fired = strings();
        env.record_skip(&fired);
        env.record_previous(&fired);
        env.record_next(&fired);
        env.overlay.show(
            &target,
            &SpotlightContent { is_last_step: true, next_button_label: Some("Use Copilot".into()), show_end_tour: true, ..content() },
            SpotlightShowOptions { target_overlay_visible: Some(true), allow_target_interaction: Some(true), ..Default::default() },
        );

        let buttons = env.container.buttons();
        let labels: Vec<String> = buttons.iter().map(|button| button.label()).collect();
        let clickable: Vec<bool> = buttons
            .iter()
            .map(|button| {
                let before = fired.borrow().len();
                button.click();
                !button.is_hidden() && fired.borrow().len() == before + 1
            })
            .collect();
        // O picker externo recebe o clique fora do balão: o overlay cede a ele e não é modal.
        let picker_clickable = env.overlay.has_target_overlay_visible() && !env.overlay.is_callout_modal();

        assert_eq!(
            (labels, clickable, picker_clickable),
            (vec!["End Tour".to_string(), "Back".to_string(), "Use Copilot".to_string()], vec![true, true, true], true)
        );
    });
}

#[test]
fn disables_a_pending_primary_action_and_resets_it_for_the_next_step() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 100.0, 80.0, 30.0);
        let actions = strings();
        env.record_next(&actions);
        env.overlay.show(&target, &content(), SpotlightShowOptions::default());
        let primary = env.container.buttons()[2].clone();
        env.overlay.set_primary_action_enabled(false);
        primary.click();
        assert_eq!((actions.borrow().len(), !primary.is_enabled()), (0, true));

        env.overlay.show(&target, &content(), SpotlightShowOptions::default());
        primary.click();
        assert_eq!((actions.borrow().len(), !primary.is_enabled()), (1, false));
    });
}

#[test]
fn advance_on_target_click_hides_next_and_advances_when_the_target_is_clicked() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 100.0, 80.0, 30.0);

        let advanced = strings();
        env.record_next(&advanced);

        env.overlay.show(&target, &content(), SpotlightShowOptions { advance_on_target_click: advance_on_click(true), ..Default::default() });

        let next = env.container.buttons()[2].clone();
        let (viewport_width, viewport_height) = env.window.viewport();
        target.click();
        settled(timeout(0)).await;

        assert_eq!(
            (next.is_hidden(), env.overlay.blockers(), advanced.borrow().clone()),
            (
                true,
                [
                    Some(rect(0.0, 0.0, viewport_width, 94.0)),
                    Some(rect(186.0, 94.0, viewport_width - 186.0, 42.0)),
                    Some(rect(0.0, 136.0, viewport_width, viewport_height - 136.0)),
                    Some(rect(0.0, 94.0, 94.0, 42.0)),
                ],
                vec!["target".to_string()]
            )
        );
    });
}

#[test]
fn advance_on_target_click_observes_a_nested_control_that_stops_propagation() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let previous = env.plain_element();
        previous.focus();
        let target = env.target(100.0, 100.0, 80.0, 30.0);
        let nested_button = env.plain_element();
        let events = strings();
        // O `nestedButton` é o próprio alvo: o ouvinte dele em bolha roda depois do de captura do overlay.
        guard.add(target.add_event_listener(OnboardingEventKind::Click, false, {
            let (events, nested_button) = (events.clone(), nested_button.clone());
            move |event| {
                event.stop_propagation();
                nested_button.focus();
                events.borrow_mut().push("control".to_string());
            }
        }));
        guard.add(env.overlay.on_did_click_next().listen({
            let (events, overlay) = (events.clone(), env.overlay.clone());
            move |source| {
                events.borrow_mut().push(source_name(*source).to_string());
                overlay.dispose();
            }
        }));

        env.overlay.show(&target, &content(), SpotlightShowOptions { advance_on_target_click: advance_on_click(true), ..Default::default() });
        target.click();
        settled(timeout(0)).await;

        assert_eq!(
            (events.borrow().clone(), env.window.document().active_element() == Some(nested_button.clone())),
            (vec!["control".to_string(), "target".to_string()], true)
        );
    });
}

#[test]
fn only_done_is_shown_on_a_last_step_without_back() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);

        env.overlay.show(&target, &SpotlightContent { can_go_back: false, is_last_step: true, ..content() }, SpotlightShowOptions::default());

        let buttons = env.container.buttons();
        assert_eq!(
            (buttons[0].is_hidden(), buttons[1].is_hidden(), buttons[2].label()),
            (true, true, "Done".to_string())
        );
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Activation {
    Click,
    Enter,
    Space,
}

async fn uses_the_control_activation_event(activation: Activation) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let previous = env.plain_element();
    let destination = env.plain_element();
    previous.focus();
    let button = env.button(&guard, 100.0, 100.0, 100.0, 30.0);
    let events = strings();
    guard.add(button.on_did_click.event().listen({
        let (events, destination) = (events.clone(), destination.clone());
        move |_| {
            events.borrow_mut().push("action".to_string());
            destination.focus();
        }
    }));
    guard.add(env.overlay.on_did_click_next().listen({
        let (events, overlay) = (events.clone(), env.overlay.clone());
        move |source| {
            events.borrow_mut().push(source_name(*source).to_string());
            overlay.dispose();
        }
    }));
    env.overlay.show(
        &button.element,
        &SpotlightContent { is_last_step: true, next_button_label: Some("Understood".into()), ..content() },
        SpotlightShowOptions {
            advance_on_target_click: advance_on_click(true),
            hide_next: Some(false),
            on_did_activate_target: Some(button.on_did_click.event()),
            ..Default::default()
        },
    );
    match activation {
        Activation::Click => {
            button.element.click();
        }
        Activation::Enter => {
            button.element.key_down(OnboardingKey::Enter, false);
        }
        Activation::Space => {
            button.element.key_down(OnboardingKey::Space, false);
        }
    }
    settled(timeout(0)).await;
    let destination_focused = env.window.document().active_element() == Some(destination.clone());
    button.element.click();
    settled(timeout(0)).await;
    assert_eq!(
        (events.borrow().clone(), destination_focused),
        (vec!["action".to_string(), "target".to_string(), "action".to_string()], true)
    );
}

#[test]
fn uses_the_control_activation_event_for_click_without_advancing_twice_or_restoring_stale_focus() {
    run_with_real_timers(uses_the_control_activation_event(Activation::Click));
}

#[test]
fn uses_the_control_activation_event_for_enter_without_advancing_twice_or_restoring_stale_focus() {
    run_with_real_timers(uses_the_control_activation_event(Activation::Enter));
}

#[test]
fn uses_the_control_activation_event_for_space_without_advancing_twice_or_restoring_stale_focus() {
    run_with_real_timers(uses_the_control_activation_event(Activation::Space));
}

#[test]
fn advance_only_consumes_mouse_and_keyboard_activation_while_keeping_the_acknowledgment_available() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.button(&guard, 100.0, 100.0, 100.0, 30.0);
        let events = strings();
        guard.add(target.on_did_click.event().listen({
            let events = events.clone();
            move |_| events.borrow_mut().push("native".to_string())
        }));
        env.record_next(&events);
        env.record_skip(&events);
        env.overlay.show(
            &target.element,
            &SpotlightContent { can_go_back: false, is_last_step: true, next_button_label: Some("Understood".into()), ..content() },
            SpotlightShowOptions {
                advance_on_target_click: Some(SpotlightTargetClickBehavior::AdvanceOnly),
                hide_next: Some(false),
                on_did_activate_target: Some(target.on_did_click.event()),
                ..Default::default()
            },
        );
        let next = env.container.buttons().last().cloned().expect("o botão Próximo");
        // No original o alvo ("Archive", `target.label = 'Archive'`) é um `.monaco-button` do mesmo contêiner e vem antes
        // dos três botões do balão; aqui o alvo vive fora de `container.buttons()`, então entra na frente da lista.
        let visible: Vec<String> = std::iter::once("Archive".to_string()).chain(visible_buttons(&env.container)).collect();
        next.key_down(OnboardingKey::Tab, false);
        let target_focused = env.window.document().active_element() == Some(target.element.clone());
        target.element.click();
        for key in [OnboardingKey::Enter, OnboardingKey::Space] {
            target.element.key_down(key, false);
            target.element.key_up(key, false);
        }
        next.click();
        target.element.key_down(OnboardingKey::Escape, false);
        env.overlay.hide();
        target.element.click();

        assert_eq!(
            (visible, target_focused, events.borrow().clone()),
            (
                vec!["Archive".to_string(), "Understood".to_string()],
                true,
                ["target", "target", "target", "button", "skip", "native"].map(String::from).to_vec()
            )
        );
    });
}

#[test]
fn custom_acknowledgment_is_the_only_visible_button_and_escape_still_skips() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);
        let skip_reasons: Rc<RefCell<Vec<OnboardingDismissReason>>> = Rc::new(RefCell::new(Vec::new()));
        let advances = strings();
        guard.add(env.overlay.on_did_skip().listen({
            let skip_reasons = skip_reasons.clone();
            move |reason| skip_reasons.borrow_mut().push(*reason)
        }));
        env.record_next(&advances);

        env.overlay.show(
            &target,
            &SpotlightContent { can_go_back: false, is_last_step: true, next_button_label: Some("Understood".into()), ..content() },
            SpotlightShowOptions::default(),
        );

        let buttons = env.container.buttons();
        let next = buttons[2].clone();
        next.key_down(OnboardingKey::Escape, false);
        env.overlay.callout_key_down(OnboardingKey::Escape, false);
        next.key_down(OnboardingKey::Tab, true);
        next.click();

        assert_eq!(
            (
                visible_buttons(&env.container),
                env.window.document().active_element() == Some(next.element().clone()),
                skip_reasons.borrow().clone(),
                advances.borrow().clone(),
            ),
            (
                vec!["Understood".to_string()],
                true,
                vec![OnboardingDismissReason::EscapeKey, OnboardingDismissReason::EscapeKey],
                vec!["button".to_string()]
            )
        );
    });
}

#[test]
fn escape_in_external_target_ui_skips_without_consuming_the_native_dismissal() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);
        let popup = env.plain_element();
        let skip_reasons: Rc<RefCell<Vec<OnboardingDismissReason>>> = Rc::new(RefCell::new(Vec::new()));
        let native_dismissals = Rc::new(std::cell::Cell::new(0));
        guard.add(env.overlay.on_did_skip().listen({
            let skip_reasons = skip_reasons.clone();
            move |reason| skip_reasons.borrow_mut().push(*reason)
        }));
        guard.add(popup.add_event_listener(OnboardingEventKind::KeyDown, false, {
            let (popup, native_dismissals) = (popup.clone(), native_dismissals.clone());
            move |event| {
                if event.key == OnboardingKey::Escape {
                    native_dismissals.set(native_dismissals.get() + 1);
                    popup.remove();
                }
            }
        }));

        env.overlay.show(&target, &content(), SpotlightShowOptions { target_overlay_visible: Some(true), ..Default::default() });
        let not_prevented = popup.key_down(OnboardingKey::Escape, false);

        assert_eq!(
            (skip_reasons.borrow().clone(), native_dismissals.get(), popup.is_connected(), !not_prevented),
            (vec![OnboardingDismissReason::EscapeKey], 1, false, false)
        );
    });
}

#[test]
fn custom_label_and_hidden_end_tour_reset_between_shows() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);
        let mut snapshots: Vec<Vec<String>> = Vec::new();
        let understood = SpotlightContent { can_go_back: false, is_last_step: true, next_button_label: Some("Understood".into()), ..content() };

        env.overlay.show(&target, &understood, SpotlightShowOptions::default());
        snapshots.push(visible_buttons(&env.container));
        env.overlay.show(&target, &content(), SpotlightShowOptions::default());
        snapshots.push(visible_buttons(&env.container));
        env.overlay.show(&target, &understood, SpotlightShowOptions::default());
        env.overlay.hide();
        env.overlay.show(&target, &content(), SpotlightShowOptions::default());
        snapshots.push(visible_buttons(&env.container));
        env.overlay.show(&target, &SpotlightContent { can_go_back: false, is_last_step: true, ..content() }, SpotlightShowOptions::default());
        snapshots.push(visible_buttons(&env.container));

        let labels = |labels: &[&str]| labels.iter().map(|label| label.to_string()).collect::<Vec<String>>();
        assert_eq!(
            snapshots,
            vec![labels(&["Understood"]), labels(&["End Tour", "Back", "Next"]), labels(&["End Tour", "Back", "Next"]), labels(&["Done"])]
        );
    });
}

#[test]
fn allow_target_interaction_arranges_click_blockers_around_the_target() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        // O primeiro focável do alvo (o `targetButton` do original) é o próprio alvo.
        let target = env.target(100.0, 100.0, 80.0, 30.0);
        let visible = |env: &Env| env.overlay.blockers().map(|blocker| blocker.is_some());

        env.overlay.show(&target, &content(), SpotlightShowOptions { allow_target_interaction: Some(false), ..Default::default() });
        assert_eq!((visible(&env), env.overlay.is_callout_modal()), ([true, false, false, false], true));

        env.overlay.show(&target, &content(), SpotlightShowOptions { allow_target_interaction: Some(true), ..Default::default() });
        let next = env.container.buttons()[2].clone();
        next.element().focus();
        next.key_down(OnboardingKey::Tab, false);

        assert_eq!(
            (visible(&env), env.overlay.has_target_overlay_visible(), env.overlay.is_callout_modal(), env.window.document().active_element()),
            ([true, true, true, true], true, false, Some(target.clone()))
        );
    });
}

#[test]
fn hide_next_routes_target_keyboard_events_through_the_focus_trap() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(100.0, 100.0, 80.0, 30.0);
        target.set_tab_index(0);

        env.overlay.show(&target, &content(), SpotlightShowOptions { hide_next: Some(true), ..Default::default() });
        let buttons = env.container.buttons();
        target.key_down(OnboardingKey::Tab, false);

        assert_eq!(
            (buttons[2].is_hidden(), env.overlay.is_callout_modal(), env.window.document().active_element()),
            (true, false, Some(buttons[0].element().clone()))
        );
    });
}

#[test]
fn observes_the_target_and_container_for_re_layout() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);

        env.overlay.show(&target, &content(), SpotlightShowOptions::default());
        // O alvo é observado por quadro: a nova caixa dele chega ao buraco sem nenhum `layout` explícito.
        target.set_size(80.0, 50.0);
        settled(env.next_frame()).await;
        let after_target_resize = env.overlay.hole().expect("o buraco").width;
        // O contêiner (a janela) é observado por `schedule_layout`.
        env.window.set_viewport(40.0, 768.0);
        env.overlay.schedule_layout();
        settled(env.next_frame()).await;
        let after_container_resize = env.overlay.hole().expect("o buraco").width;

        assert_eq!((after_target_resize, after_container_resize), (92.0, 40.0));
    });
}

#[test]
fn follows_position_only_shifts_without_a_resize_or_workbench_layout_event() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(300.0, 300.0, 120.0, 30.0);
        env.overlay.set_callout_size(280.0, 100.0);
        env.overlay.show(&target, &content(), SpotlightShowOptions { placement: Some(SpotlightPlacement::Above), ..Default::default() });
        let before = env.tops();

        target.set_position(300.0, 280.0);
        settled(env.next_frame()).await;
        let after = env.tops();
        let shifts = [after[0] - before[0], after[1] - before[1], after[2] - before[2]];
        env.overlay.hide();
        target.set_position(300.0, 260.0);
        settled(env.next_frame()).await;

        assert_eq!((shifts, env.overlay.hole_top_style()), ([-20.0, -20.0, -20.0], Some("274px".to_string())));
    });
}

async fn hides_rather_than_highlighting_the_origin_when_the_target_disappears(detach: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let target = env.target(100.0, 100.0, 120.0, 30.0);
    let lost = Rc::new(std::cell::Cell::new(0));
    guard.add(env.overlay.on_did_lose_target().listen({
        let lost = lost.clone();
        move |_| lost.set(lost.get() + 1)
    }));
    env.overlay.show(&target, &content(), SpotlightShowOptions::default());
    if detach {
        target.remove();
    } else {
        target.set_display_none(true);
    }
    settled(env.next_frame()).await;
    env.overlay.layout();

    assert_eq!((env.overlay.is_hidden(), env.overlay.hole_top_style(), lost.get()), (true, Some("94px".to_string()), 1));
}

#[test]
fn hides_rather_than_highlighting_the_origin_when_the_target_disappears_detach() {
    run_with_real_timers(hides_rather_than_highlighting_the_origin_when_the_target_disappears(true));
}

#[test]
fn hides_rather_than_highlighting_the_origin_when_the_target_disappears_hide() {
    run_with_real_timers(hides_rather_than_highlighting_the_origin_when_the_target_disappears(false));
}

#[test]
fn focus_trap_includes_links_rendered_in_a_markdown_description() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);
        let markdown = IMarkdownString { value: JsString::from("See [the docs](https://example.com) for more."), ..Default::default() };

        env.overlay.show(&target, &SpotlightContent { description: SpotlightDescription::Markdown(markdown), ..content() }, SpotlightShowOptions::default());

        // O desenho do markdown cria o link; ele entra na armadilha de foco.
        let link = env.plain_element();
        env.overlay.set_description_links(vec![link.clone()]);
        assert!(matches!(env.overlay.description(), SpotlightDescription::Markdown(_)), "expected a link to be rendered from the markdown description");

        // With the link focused, Shift+Tab should cycle to the last button (Next),
        // proving the link participates in the trap rather than being skipped.
        link.focus();
        env.overlay.callout_key_down(OnboardingKey::Tab, true);

        assert_eq!(env.window.document().active_element(), env.container.buttons().last().map(|button| button.element().clone()));
    });
}

#[test]
fn dispose_removes_the_overlay_from_the_dom() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let target = env.target(0.0, 0.0, 50.0, 50.0);
        env.overlay.show(&target, &content(), SpotlightShowOptions::default());

        env.overlay.dispose();

        assert_eq!(env.container.overlays().len(), 0);
    });
}
