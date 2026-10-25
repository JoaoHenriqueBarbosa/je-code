//! Porte parcial de `src/vs/platform/quickinput/test/browser/quickinput.test.ts` (suíte `QuickInput`).
//!
//! Portados: os dois casos de `isKeyModified`, que só dependem do contrato de `common/quickInput.ts`.
//!
//! Não portados (20 casos): todos os que exercitam o `QuickInputController` e o `QuickPick` de
//! `browser/quickInputController.ts` e `browser/quickInput.ts` (movimento de fechamento, âncoras, `pick`, `input`,
//! `keepScrollPosition`, `selectedItems`, `activeItems`, `data-quick-input-id`). Essas duas classes ainda não existem no
//! porte (`je-platform/src/quickinput` tem só o contrato, o acesso rápido e os utilitários), então os casos entram junto
//! com o controlador nativo, sem fachada.

use je_platform::quickinput::quick_input::{IKeyMods, NO_KEY_MODS, is_key_modified};

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
