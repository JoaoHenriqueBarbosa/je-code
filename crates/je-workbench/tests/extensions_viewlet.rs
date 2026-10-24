//! Porte de `src/vs/workbench/contrib/extensions/test/browser/extensionsViewlet.test.ts` (suíte `Extensions viewlet focus
//! styles`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Sem DOM.** O original monta um `.monaco-workbench.<tema>` real, foca o botão e lê o `getComputedStyle`; o `setup` pula
//!   o caso quando a janela não tem foco. Aqui não há DOM nem estilo computado: o caso confere a regra do CSS
//!   (`outline: var(--vscode-strokeThickness) solid var(--vscode-focusBorder); outline-offset: 2px`) pelo
//!   [`message_action_button_focus_outline`], com as mesmas variáveis (`--vscode-focusBorder: #0078d4`,
//!   `--vscode-strokeThickness: 1px`). `focused` e `focusVisible` (o foco por `focus()` depois de focar o `input`, que dá
//!   `:focus-visible`) são do navegador e não têm como ser medidos; ficam como recorte e o esperado `rgb(0, 120, 212)` é
//!   `(0, 120, 212)`.
//! - **O `for` sobre os temas** vira um `#[test]` por tema, na mesma ordem (`vs`, `vs-dark`, `hc-black`, `hc-light`).
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`; o `container.remove()` do `store.add(toDisposable(...))` não
//!   existe sem DOM.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::extensions::extensions_viewlet::{FocusOutline, message_action_button_focus_outline};

fn notification_action_has_a_separated_focus_outline() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let focus_border = (0x00, 0x78, 0xd4);
    let stroke_thickness = 1.0;

    assert_eq!(
        message_action_button_focus_outline(stroke_thickness, focus_border),
        FocusOutline { style: "solid", width: 1.0, color: (0, 120, 212), offset: 2.0 }
    );
}

#[test]
fn notification_action_has_a_separated_focus_outline_in_vs() {
    notification_action_has_a_separated_focus_outline();
}

#[test]
fn notification_action_has_a_separated_focus_outline_in_vs_dark() {
    notification_action_has_a_separated_focus_outline();
}

#[test]
fn notification_action_has_a_separated_focus_outline_in_hc_black() {
    notification_action_has_a_separated_focus_outline();
}

#[test]
fn notification_action_has_a_separated_focus_outline_in_hc_light() {
    notification_action_has_a_separated_focus_outline();
}
