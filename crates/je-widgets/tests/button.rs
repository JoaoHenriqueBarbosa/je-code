//! Porte de `src/vs/base/test/browser/ui/button/button.test.ts` (suíte `Button`), caso a caso e na mesma ordem.

mod support;

use je_render::Rgba;
use je_ui::{Event, EventKind, Ui};
use je_widgets::button::{Button, ButtonOptions, IButton, unthemed_button_styles, ButtonStyles};
use support::{create_container, ensure_no_disposables_are_leaked_in_test_suite};

const RED: Rgba = Rgba::opaque(0xFF, 0x00, 0x00);
const BLUE: Rgba = Rgba::opaque(0x00, 0x00, 0xFF);

#[test]
fn disabling_a_hovered_button_clears_its_hover_background() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = create_container(&ui, 0.0, 0.0);
    let button = store.add_rc(Button::new(
        &ui,
        container,
        ButtonOptions { styles: ButtonStyles { button_background: Some(RED), button_hover_background: Some(BLUE), ..unthemed_button_styles() }, ..ButtonOptions::default() },
    ));
    let background = || ui.tree().style(button.element()).background;

    let mut backgrounds = vec![background()];
    ui.dispatch(&mut Event::new(EventKind::PointerEnter, button.element()));
    backgrounds.push(background());
    button.set_enabled(false);
    backgrounds.push(background());
    ui.dispatch(&mut Event::new(EventKind::PointerLeave, button.element()));
    ui.dispatch(&mut Event::new(EventKind::PointerEnter, button.element()));
    backgrounds.push(background());

    assert_eq!(backgrounds, [Some(RED), Some(BLUE), Some(RED), Some(RED)]);
}
