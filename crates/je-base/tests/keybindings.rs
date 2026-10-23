//! Porte de `src/vs/base/test/common/keybindings.test.ts` (suíte `keyCodes`), caso a caso e na mesma ordem.
//!
//! `keyCodes` é o nome que o original dá à suíte desse arquivo, herdado do `keyCodes.test.ts`.

use je_base::key_codes::{KeyCode, ScanCode};
use je_base::keybindings::{KeyCodeChord, ScanCodeChord};

#[test]
fn issue_173325_wrong_interpretations_of_special_keys_e_g_equal_is_mistaken_for_v() {
    let a = KeyCodeChord::new(true, false, false, false, KeyCode::KeyV);
    let b = ScanCodeChord::new(true, false, false, false, ScanCode::Equal);
    assert!(a.get_hash_code() != b.get_hash_code());
}
