//! Porte de `src/vs/platform/quickinput/test/browser/quickinput.test.ts` (suíte `QuickInput`), caso a caso e na ordem do original.
//!
//! Notas de tradução:
//! - **DOM.** O `fixture` é um `ViewElement`; `widget.style.display` é a propriedade `display` do estilo (`''` quando ausente),
//!   `widget.inert` é `QuickInputController::widget_inert` e `classList.contains` é `class_list_contains`. `getBoundingClientRect()` é
//!   `ViewElement::rect()` (e a caixa de filtro, `QuickInputUi::filter_container_rect`). `controller.layout({ height: 20, width: 40 }, 0)`
//!   é `layout` com o mesmo tamanho.
//! - **Animações.** `widget.getAnimations()[0]` é `closing_animation()`; `animation.finish()` e o cancelamento por movimento
//!   reduzido são `finish()` e `cancel()` dela, que disparam os eventos `finish` e `cancel` que o controlador escuta.
//!   `getComputedStyle(widget).animationName` é `animation_name()`.
//! - **Relógio.** `sinon.useFakeTimers()` é o relógio virtual do `runtime` (`run_with_faked_timers` e `advance_fake_time`); `await` de
//!   uma `Promise` é o `await` do original.
//! - **Âncoras.** O `HTMLElement` de âncora é o retângulo dele (`QuickInputAnchor`). Nos casos de identidade (`explicit.anchor ===
//!   explicitAnchor`) os elementos do original são `div` sem tamanho, distintos por identidade; aqui as âncoras ficam em posições
//!   diferentes, que é o que o `QuickInputAnchor` compara.
//! - **Lista.** `quickpick.scrollTop` é `scroll_top` da lista do widget, e a entrada `.quick-input-list-entry` com
//!   `data-quick-input-id` é a linha de `QuickInputList::rows`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::timeout;
use je_base::event::{Emitter, Event};
use je_base::layout::ISize;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime::advance_fake_time;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};
use je_platform::quickinput::quick_input::{
    IInputOptions, IKeyMods, IPickOptions, IQuickInput, IQuickItem, IQuickPick, IQuickPickItem, ItemActivation, NO_KEY_MODS, PickActiveItem, QuickInputAnchor,
    QuickInputAnchorPosition, QuickPickInput, QuickPicks, is_key_modified,
};
use je_platform::quickinput::quick_input_browser::IQuickInputListUi;
use je_platform::quickinput::quick_input_controller::{IQuickInputOptions, IQuickInputStyles, QuickInputController};
use je_platform::storage::storage::{IStorageService, InMemoryStorageService};

struct FixtureLayoutService {
    container: Rc<ViewElement>,
    layout: Rc<Emitter<LayoutContainerEvent>>,
}

impl ILayoutService for FixtureLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn active_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn get_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        self.layout.event()
    }

    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 0.0, inner_height: 0.0, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

/// O `setup()` do original: o `fixture` e o `QuickInputController` com os serviços de teste, já com o layout inicial.
struct Env {
    fixture: Rc<ViewElement>,
    controller: Rc<QuickInputController>,
}

fn setup(guard: &LeakGuard) -> Env {
    let fixture = ViewElement::new();
    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(FixtureLayoutService { container: fixture.clone(), layout });
    let context_key_service: Rc<dyn IContextKeyService> = guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
    let storage_service: Rc<dyn IStorageService> = guard.add_rc(InMemoryStorageService::new());
    let controller = guard.add_rc(QuickInputController::new(
        IQuickInputOptions {
            container: fixture.clone(),
            id_prefix: "testQuickInput".to_owned(),
            ignore_focus_out: Rc::new(|| true),
            return_focus: Rc::new(|| {}),
            back_keybinding_label: Rc::new(|| None),
            set_context_key: Rc::new(|_| {}),
            link_opener_delegate: Rc::new(|_| {}),
            styles: IQuickInputStyles::default(),
            hover_delegate: None,
            is_dark_theme: Rc::new(|| true),
        },
        layout_service,
        context_key_service,
        storage_service,
    ));
    // initial layout
    controller.layout(ISize { width: 40.0, height: 20.0 }, 0.0);
    Env { fixture, controller }
}

/// `{ label }`.
fn pick_item(label: &str) -> IQuickPickItem {
    IQuickPickItem { base: IQuickItem { label: label.into(), ..Default::default() }, ..Default::default() }
}

fn input_items(items: &[IQuickPickItem]) -> Vec<QuickPickInput> {
    items.iter().cloned().map(QuickPickInput::Item).collect()
}

fn display_of(element: &ViewElement) -> String {
    element.style_get_property("display").unwrap_or_default()
}

/// O `{ display, closing, inert, visible }` que os casos de movimento registram.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WidgetState {
    display: String,
    closing: bool,
    inert: bool,
    visible: bool,
}

fn widget_state(env: &Env) -> WidgetState {
    let widget = env.controller.widget();
    WidgetState {
        display: display_of(&widget),
        closing: widget.class_list_contains("quick-input-widget-closing"),
        inert: env.controller.widget_inert(),
        visible: env.controller.is_visible(),
    }
}

fn state(display: &str, closing: bool, inert: bool, visible: bool) -> WidgetState {
    WidgetState { display: display.to_owned(), closing, inert, visible }
}

#[test]
fn close_motion_requires_modern_ui_with_motion_enabled() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let quickpick = guard.add_rc(env.controller.create_quick_pick());
        let mut states = Vec::new();

        env.fixture.class_list_add("modern-ui");
        env.fixture.class_list_add("monaco-reduce-motion");
        quickpick.show();
        quickpick.hide();
        states.push(widget_state(&env));

        env.fixture.class_list_remove("monaco-reduce-motion");
        env.fixture.class_list_add("monaco-enable-motion");
        quickpick.show();
        quickpick.hide();
        states.push(widget_state(&env));

        quickpick.show();
        states.push(widget_state(&env));

        quickpick.hide();
        advance_fake_time(150);
        states.push(widget_state(&env));

        assert_eq!(
            states,
            vec![state("none", false, false, false), state("", true, true, false), state("", false, false, true), state("none", false, false, false)]
        );
    });
}

/// `for (const action of ['accept', 'cancel'])`.
fn hides_immediately_when_css_suppresses_close_motion(cancel: bool) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        env.fixture.class_list_add("modern-ui");
        env.fixture.class_list_add("monaco-enable-motion");
        let quickpick = guard.add_rc(env.controller.create_quick_pick());
        quickpick.set_items(vec![QuickPickInput::Item(pick_item("item"))]);
        let weak = Rc::downgrade(&quickpick);
        guard.add(quickpick.on_did_accept().listen(move |_| {
            if let Some(quickpick) = weak.upgrade() {
                quickpick.hide();
            }
        }));
        env.controller.widget().style_set_property("animation", "none");

        quickpick.show();
        if cancel {
            env.controller.cancel(None).await.unwrap();
        } else {
            env.controller.accept(None).await.unwrap();
        }

        assert_eq!(widget_state(&env), state("none", false, false, false));
    });
}

#[test]
fn hides_immediately_on_accept_when_css_suppresses_close_motion() {
    hides_immediately_when_css_suppresses_close_motion(false);
}

#[test]
fn hides_immediately_on_cancel_when_css_suppresses_close_motion() {
    hides_immediately_when_css_suppresses_close_motion(true);
}

#[test]
fn finishes_closing_when_the_close_animation_finishes() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        env.fixture.class_list_add("modern-ui");
        env.fixture.class_list_add("monaco-enable-motion");
        let quickpick = guard.add_rc(env.controller.create_quick_pick());
        quickpick.show();
        quickpick.hide();
        let animation = env.controller.closing_animation().expect("a animação de fechamento existe");
        animation.finish();

        let current = widget_state(&env);
        assert_eq!((current.display, current.closing, current.inert), ("none".to_owned(), false, false));
    });
}

#[test]
fn finishes_closing_when_reduced_motion_cancels_the_animation() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        env.fixture.class_list_add("modern-ui");
        env.fixture.class_list_add("monaco-enable-motion");
        let quickpick = guard.add_rc(env.controller.create_quick_pick());
        quickpick.show();
        quickpick.hide();
        let animation = env.controller.closing_animation().expect("a animação de fechamento existe");
        env.fixture.class_list_remove("monaco-enable-motion");
        env.fixture.class_list_add("monaco-reduce-motion");
        // O navegador cancela a animação quando o movimento reduzido liga.
        animation.cancel();
        let animation_count = usize::from(animation.is_running());

        let current = widget_state(&env);
        assert_eq!((animation_count, current.display, current.closing, current.inert), (0, "none".to_owned(), false, false));
    });
}

#[test]
fn reopening_discards_the_previous_close_animation_callbacks_and_timeout() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        env.fixture.class_list_add("modern-ui");
        env.fixture.class_list_add("monaco-enable-motion");
        let quickpick = guard.add_rc(env.controller.create_quick_pick());
        quickpick.show();
        quickpick.hide();
        let animation = env.controller.closing_animation().expect("a animação de fechamento existe");

        quickpick.show();
        advance_fake_time(150);
        let reopened = widget_state(&env);
        quickpick.hide();
        animation.finish();
        animation.cancel();

        assert_eq!(reopened, state("", false, false, true));
        assert_eq!(widget_state(&env), state("", true, true, false));
        advance_fake_time(150);
    });
}

#[test]
fn title_bar_is_hidden_when_empty() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    let title_bar = env.controller.ui().title_bar().clone();
    let mut states = Vec::new();

    quickpick.show();
    states.push(display_of(&title_bar));

    quickpick.set_title(Some("Title".to_owned()));
    states.push(display_of(&title_bar));

    quickpick.set_title(None);
    quickpick.set_buttons(vec![env.controller.back_button()]);
    states.push(display_of(&title_bar));

    quickpick.set_buttons(Vec::new());
    states.push(display_of(&title_bar));

    assert_eq!(states, vec!["none", "", "", "none"]);
}

/// O `div` de âncora do original (`left: 80px; top: 40px; width: 300px; height: 26px`).
fn anchor_80_40() -> QuickInputAnchor {
    QuickInputAnchor { x: 80.0, y: 40.0, width: Some(300.0), height: Some(26.0) }
}

#[test]
fn overlay_picker_aligns_its_input_with_the_anchor_and_bypasses_motion() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    env.fixture.set_size(600.0, 400.0);
    env.fixture.class_list_add("modern-ui");
    env.fixture.class_list_add("monaco-enable-motion");
    env.controller.layout(ISize { width: 600.0, height: 400.0 }, 0.0);

    let anchor = anchor_80_40();
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    quickpick.set_anchor(Some(anchor));
    quickpick.set_anchor_position(Some(QuickInputAnchorPosition::Overlay));
    quickpick.show();

    let widget = env.controller.widget();
    let input = env.controller.ui().filter_container_rect();
    let alignment_delta = (input.left - anchor.x, input.top - anchor.y, input.width - anchor.width.unwrap(), input.height - anchor.height.unwrap());
    let open_state = (alignment_delta, env.controller.animation_name(), widget.class_list_contains("quick-input-widget-overlay"));

    quickpick.hide();

    assert_eq!(open_state, ((0.0, 0.0, 0.0, 0.0), "none", true));
    let closed = widget_state(&env);
    assert_eq!((closed.display, closed.closing, closed.inert), ("none".to_owned(), false, false));
}

#[test]
fn positions_an_anchored_picker_below_its_anchor_when_requested() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    env.fixture.set_size(600.0, 400.0);
    env.controller.layout(ISize { width: 600.0, height: 400.0 }, 0.0);

    let anchor = anchor_80_40();
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    quickpick.set_anchor(Some(anchor));
    quickpick.set_anchor_position(Some(QuickInputAnchorPosition::Below));
    quickpick.show();

    let widget = env.controller.widget();
    let anchor_bottom_with_gap = anchor.y + anchor.height.unwrap() + 4.0;
    assert_eq!(widget.rect().top, anchor_bottom_with_gap);
}

#[test]
fn scoped_anchors_apply_only_to_the_next_unanchored_quick_pick() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let anchor = QuickInputAnchor::point(10.0, 10.0);
        let explicit_anchor = QuickInputAnchor::point(20.0, 20.0);
        let explicit_state: Rc<Cell<(bool, Option<QuickInputAnchorPosition>)>> = Rc::new(Cell::new((false, None)));
        let inherited_state: Rc<Cell<(bool, Option<QuickInputAnchorPosition>)>> = Rc::new(Cell::new((false, None)));
        let (controller, store) = (env.controller.clone(), guard.store());
        let (explicit_out, inherited_out) = (explicit_state.clone(), inherited_state.clone());
        env.controller
            .with_quick_input_anchor(Some(anchor), Some(QuickInputAnchorPosition::Below), move || {
                let input = store.add_rc(controller.create_input_box());
                input.show();
                input.hide();
                let explicit = store.add_rc(controller.create_quick_pick());
                explicit.set_anchor(Some(explicit_anchor));
                explicit.set_anchor_position(Some(QuickInputAnchorPosition::Above));
                explicit.show();
                explicit_out.set((explicit.anchor() == Some(explicit_anchor), explicit.anchor_position()));
                explicit.hide();
                let inherited = store.add_rc(controller.create_quick_pick());
                inherited.show();
                inherited_out.set((inherited.anchor() == Some(anchor), inherited.anchor_position()));
                inherited.hide();
                Promise::resolved(())
            })
            .await
            .unwrap();
        let subsequent = guard.add_rc(env.controller.create_quick_pick());
        subsequent.show();

        assert_eq!(explicit_state.get(), (true, Some(QuickInputAnchorPosition::Above)));
        assert_eq!(inherited_state.get(), (true, Some(QuickInputAnchorPosition::Below)));
        assert_eq!((subsequent.anchor(), subsequent.anchor_position()), (None, None));
    });
}

#[test]
fn serializes_scoped_anchor_operations() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let first_anchor = QuickInputAnchor::point(10.0, 10.0);
        let second_anchor = QuickInputAnchor::point(20.0, 20.0);
        let first_started = Rc::new(DeferredPromise::<()>::new());
        let release_first = DeferredPromise::<()>::new();
        let order = Rc::new(RefCell::new(Vec::<String>::new()));
        let first_picker_anchored = Rc::new(Cell::new(false));
        let second_picker_anchored = Rc::new(Cell::new(false));

        let first = {
            let (controller, store, order, anchored) = (env.controller.clone(), guard.store(), order.clone(), first_picker_anchored.clone());
            let (started, release) = (first_started.clone(), release_first.promise.clone());
            env.controller.with_quick_input_anchor(Some(first_anchor), Some(QuickInputAnchorPosition::Below), move || {
                order.borrow_mut().push("first:start".to_owned());
                let _ = started.complete(());
                release.then(move |()| {
                    let picker = store.add_rc(controller.create_quick_pick());
                    picker.show();
                    anchored.set(picker.anchor() == Some(first_anchor));
                    picker.hide();
                    order.borrow_mut().push("first:end".to_owned());
                })
            })
        };
        first_started.promise.clone().await.unwrap();
        let second = {
            let (controller, store, order, anchored) = (env.controller.clone(), guard.store(), order.clone(), second_picker_anchored.clone());
            env.controller.with_quick_input_anchor(Some(second_anchor), Some(QuickInputAnchorPosition::Above), move || {
                order.borrow_mut().push("second:start".to_owned());
                let picker = store.add_rc(controller.create_quick_pick());
                picker.show();
                anchored.set(picker.anchor() == Some(second_anchor));
                picker.hide();
                order.borrow_mut().push("second:end".to_owned());
                Promise::resolved(())
            })
        };
        Promise::resolved(()).await.unwrap();
        let order_while_first_is_pending = order.borrow().clone();
        let _ = release_first.complete(());
        Promise::all(vec![first, second]).await.unwrap();

        assert_eq!(order_while_first_is_pending, vec!["first:start"]);
        assert_eq!(*order.borrow(), vec!["first:start", "first:end", "second:start", "second:end"]);
        assert!(first_picker_anchored.get());
        assert!(second_picker_anchored.get());
    });
}

#[test]
fn pick_basecase() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let item = pick_item("foo");

        let shown = Rc::new(Cell::new(false));
        let on_show = shown.clone();
        guard.add(env.controller.on_show().listen(move |()| on_show.set(true)));
        let pick_promise = env.controller.pick(QuickPicks::Items(input_items(&[item.clone(), pick_item("bar")])), None, None);
        timeout(0).await.unwrap();
        assert!(shown.get());

        env.controller.accept(None).await.unwrap();
        let pick = pick_promise.await.unwrap();

        assert_eq!(pick.map(|picked| picked.base.label), Some(item.base.label));
    });
}

#[test]
fn pick_active_item_is_honored() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let item = pick_item("foo");

        let options = IPickOptions { active_item: Some(PickActiveItem::Item(item.clone())), ..Default::default() };
        let pick_promise = env.controller.pick(QuickPicks::Items(input_items(&[pick_item("bar"), item.clone()])), Some(&options), None);
        timeout(0).await.unwrap();

        env.controller.accept(None).await.unwrap();
        let pick = pick_promise.await.unwrap();

        assert_eq!(pick.map(|picked| picked.base.label), Some(item.base.label));
    });
}

#[test]
fn input_basecase() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);

        let options = IInputOptions { value: Some("foo".to_owned()), ..Default::default() };
        let input_promise = env.controller.input(Some(&options), None);
        timeout(0).await.unwrap();

        env.controller.accept(None).await.unwrap();
        let value = input_promise.await.unwrap();

        assert_eq!(value.as_deref(), Some("foo"));
    });
}

#[test]
fn on_did_change_value_gets_triggered_when_value_is_set() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());

    let value: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let received = value.clone();
    guard.add(quickpick.on_did_change_value().listen(move |changed| *received.borrow_mut() = Some(changed.clone())));

    // Trigger a change
    quickpick.set_value("changed".to_owned());

    assert_eq!(*value.borrow(), Some(quickpick.value()));
}

/// Os 1000 itens `item N` dos casos de `keepScrollPosition`.
fn thousand_items() -> Vec<IQuickPickItem> {
    (0..1000).map(|index| pick_item(&format!("item {index}"))).collect()
}

#[test]
fn keep_scroll_position_works_with_active_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    let list = env.controller.ui().list_widget().clone();

    let items = thousand_items();
    quickpick.set_items(input_items(&items));
    // setting the active item should cause the quick pick to scroll to the bottom
    quickpick.set_active_items(vec![items[items.len() - 1].clone()]);
    quickpick.show();

    let cursor_top = list.scroll_top();

    assert_ne!(cursor_top, 0.0);

    quickpick.set_keep_scroll_position(true);
    quickpick.set_active_items(vec![items[0].clone()]);
    assert_eq!(cursor_top, list.scroll_top());

    quickpick.set_keep_scroll_position(false);
    quickpick.set_active_items(vec![items[0].clone()]);
    assert_eq!(list.scroll_top(), 0.0);
}

#[test]
fn keep_scroll_position_works_with_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    let list = env.controller.ui().list_widget().clone();

    let items = thousand_items();
    quickpick.set_items(input_items(&items));
    // setting the active item should cause the quick pick to scroll to the bottom
    quickpick.set_active_items(vec![items[items.len() - 1].clone()]);
    quickpick.show();

    let cursor_top = list.scroll_top();
    assert_ne!(cursor_top, 0.0);

    quickpick.set_keep_scroll_position(true);
    quickpick.set_items(input_items(&items));
    assert_eq!(cursor_top, list.scroll_top());

    quickpick.set_keep_scroll_position(false);
    quickpick.set_items(input_items(&items));
    assert_eq!(list.scroll_top(), 0.0);
}

#[test]
fn selected_items_verify_previous_selected_items_does_not_hang_over_to_next_set_of_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    quickpick.set_items(vec![QuickPickInput::Item(pick_item("step 1"))]);
    quickpick.show();

    let accepted = Rc::new(Cell::new(false));
    let (weak, flag) = (Rc::downgrade(&quickpick), accepted.clone());
    guard.add(quickpick.on_did_accept().listen(move |_| {
        if let Some(quickpick) = weak.upgrade() {
            quickpick.set_can_select_many(true);
            quickpick.set_items(input_items(&[pick_item("a"), pick_item("b"), pick_item("c")]));
        }
        flag.set(true);
    }));

    // accept 'step 1'
    let _ = env.controller.accept(None);
    assert!(accepted.get());

    // accept in multi-select
    let _ = env.controller.accept(None);

    // Since we don't select any items, the selected items should be empty
    assert_eq!(quickpick.selected_items().len(), 0);
}

#[test]
fn active_items_verify_on_did_change_active_is_triggered_after_setting_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());

    // Setup listener for verification
    let active_items_from_event: Rc<RefCell<Vec<IQuickPickItem>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = active_items_from_event.clone();
    guard.add(quickpick.on_did_change_active().listen(move |items| sink.borrow_mut().extend(items.iter().cloned())));

    quickpick.show();

    let item = pick_item("step 1");
    quickpick.set_items(vec![QuickPickInput::Item(item.clone())]);

    let from_event = active_items_from_event.borrow();
    assert_eq!(from_event.len(), 1);
    assert_eq!(from_event[0].base.label, item.base.label);
    assert_eq!(quickpick.active_items().len(), 1);
    assert_eq!(quickpick.active_items()[0].base.label, item.base.label);
}

#[test]
fn active_items_verify_setting_item_activation_to_none_still_triggers_on_did_change_active_after_selection_207832() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    let item = pick_item("step 1");
    quickpick.set_items(vec![QuickPickInput::Item(item.clone())]);
    quickpick.show();
    assert_eq!(quickpick.active_items()[0].base.label, item.base.label);

    // Setup listener for verification
    let active_items_from_event: Rc<RefCell<Vec<IQuickPickItem>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = active_items_from_event.clone();
    guard.add(quickpick.on_did_change_active().listen(move |items| sink.borrow_mut().extend(items.iter().cloned())));

    // Trigger a change
    quickpick.set_item_activation(ItemActivation::None);
    quickpick.set_items(vec![QuickPickInput::Item(item)]);

    assert_eq!(active_items_from_event.borrow().len(), 0);
    assert_eq!(quickpick.active_items().len(), 0);
}

#[test]
fn id_is_exposed_as_dom_metadata_and_cleared_when_absent() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let quickpick = guard.add_rc(env.controller.create_quick_pick());
    let list = env.controller.ui().list_widget().clone();
    let with_id = IQuickPickItem { base: IQuickItem { id: Some("item-id".to_owned()), label: "item with id".into(), ..Default::default() }, ..Default::default() };
    quickpick.set_items(vec![QuickPickInput::Item(with_id)]);
    quickpick.show();

    let id = list.rows().first().and_then(|row| row.data_quick_input_id.clone());

    quickpick.set_items(vec![QuickPickInput::Item(pick_item("item without id"))]);
    let recycled_id = list.rows().first().and_then(|row| row.data_quick_input_id.clone());

    assert_eq!((id, recycled_id), (Some("item-id".to_owned()), None));
}

#[test]
fn is_key_modified_returns_false_when_no_modifiers_are_pressed() {
    assert!(!is_key_modified(&NO_KEY_MODS));
    assert!(!is_key_modified(&IKeyMods { ctrl_cmd: false, alt: false, shift: false }));
}

#[test]
fn is_key_modified_returns_true_when_any_modifier_is_pressed() {
    assert!(is_key_modified(&IKeyMods { ctrl_cmd: true, alt: false, shift: false }));
    assert!(is_key_modified(&IKeyMods { ctrl_cmd: false, alt: true, shift: false }));
    assert!(is_key_modified(&IKeyMods { ctrl_cmd: false, alt: false, shift: true }));
    assert!(is_key_modified(&IKeyMods { ctrl_cmd: true, alt: true, shift: true }));
}
