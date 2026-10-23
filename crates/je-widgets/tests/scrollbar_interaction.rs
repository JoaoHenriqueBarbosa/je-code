//! Testes próprios do `ScrollableElement` sobre o `je-ui`: roda do mouse, arraste do slider e visibilidade.
//! Não há original em `src/vs/base/test`; o comportamento esperado vem de ler `scrollableElement.ts`.

mod support;

use je_base::lifecycle::IDisposable;
use je_base::scrollable::{INewScrollDimensions, ScrollbarVisibility};
use je_render::kurbo::Rect;
use je_ui::{DeltaMode, PointerInput, PointerInputKind, Role, Ui, WheelData};
use je_widgets::scrollbar::scrollable_element::AbstractScrollableElement;
use je_widgets::scrollbar::scrollable_element_options::ScrollableElementCreationOptions;
use support::ensure_no_disposables_are_leaked_in_test_suite;

fn setup(ui: &Ui) -> std::rc::Rc<AbstractScrollableElement> {
    let content = ui.tree_mut().create_detached(Role::Generic);
    let element = AbstractScrollableElement::scrollable_element(ui, content, ScrollableElementCreationOptions::default());
    let root = ui.root();
    {
        let mut tree = ui.tree_mut();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 200.0, 200.0));
        tree.append_child(root, element.get_dom_node());
        tree.set_bounds(element.get_dom_node(), Rect::new(0.0, 0.0, 100.0, 100.0));
    }
    element.set_scroll_dimensions(&INewScrollDimensions { width: Some(100.0), height: Some(100.0), scroll_width: Some(100.0), scroll_height: Some(1000.0) });
    element
}

#[test]
fn wheel_scrolls_by_notches_and_is_consumed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = setup(&ui);
    // um entalhe de roda física em linhas: 50 pixels
    let not_prevented = ui.wheel_input(WheelData::new(10.0, 10.0, 0.0, 1.0, DeltaMode::Line));
    assert!(!not_prevented, "a roda que rolou é consumida");
    assert_eq!(element.get_scroll_position().scroll_top, 50.0);
    // em pixels, 40 pixels valem um entalhe
    ui.wheel_input(WheelData::new(10.0, 10.0, 0.0, 40.0, DeltaMode::Pixel));
    assert_eq!(element.get_scroll_position().scroll_top, 100.0);
    // não passa do fim
    for _ in 0..40 {
        ui.wheel_input(WheelData::new(10.0, 10.0, 0.0, 1.0, DeltaMode::Line));
    }
    assert_eq!(element.get_scroll_position().scroll_top, 900.0);
    assert!(ui.wheel_input(WheelData::new(10.0, 10.0, 0.0, 1.0, DeltaMode::Line)), "no limite a roda não é consumida");
    element.dispose();
}

#[test]
fn slider_drag_scrolls() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let element = setup(&ui);
    // barra vertical: 10 de largura encostada na direita, slider de 10% => 20 (mínimo)
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, 95.0, 5.0));
    // o `reveal` troca a classe num `setTimeout(0)`
    je_base::runtime::with_faked_timers(|| je_base::runtime::advance_fake_time(1));
    ui.pointer_input(PointerInput::new(PointerInputKind::Down, 95.0, 5.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, 95.0, 45.0));
    assert!(element.get_scroll_position().scroll_top > 0.0);
    let after_drag = element.get_scroll_position().scroll_top;
    ui.pointer_input(PointerInput::new(PointerInputKind::Up, 95.0, 45.0));
    ui.pointer_input(PointerInput::new(PointerInputKind::Move, 95.0, 90.0));
    assert_eq!(element.get_scroll_position().scroll_top, after_drag, "solto, o movimento não rola");
    element.dispose();
}

#[test]
fn hidden_scrollbar_never_shows() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let content = ui.tree_mut().create_detached(Role::Generic);
    let element = AbstractScrollableElement::scrollable_element(
        &ui,
        content,
        ScrollableElementCreationOptions { vertical: Some(ScrollbarVisibility::Hidden), ..Default::default() },
    );
    element.set_scroll_dimensions(&INewScrollDimensions { width: Some(100.0), height: Some(100.0), scroll_width: Some(100.0), scroll_height: Some(1000.0) });
    je_base::runtime::with_faked_timers(|| je_base::runtime::advance_fake_time(1000));
    let tree = ui.tree();
    let vertical = tree.children(element.get_dom_node()).iter().copied().find(|c| tree.has_class(*c, "vertical")).expect("barra vertical");
    assert!(tree.has_class(vertical, "invisible"));
    drop(tree);
    element.dispose();
}
