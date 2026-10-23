//! Porte de `src/vs/platform/update/test/electron-main/win32UpdateType.test.ts` (suíte `Win32UpdateType`), caso a caso e na mesma ordem.
//!
//! Divergências: cada `for` do original que gera um `test(...)` por alvo vira um `#[test]` com o laço dentro
//! (a mensagem de falha traz o alvo). `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar.

use je_platform::update::UpdateType;
use je_platform::update::win32_update_type::{get_win32_update_type, is_inno_setup_install};

#[test]
fn detects_setup_from_its_product_target() {
    for target in ["user", "system"] {
        assert_eq!((is_inno_setup_install(Some(target)), get_win32_update_type(Some(target))), (true, UpdateType::Setup), "alvo {target:?}");
    }
}

#[test]
fn treats_product_target_as_an_archive() {
    for target in [None, Some(""), Some("archive"), Some("portable"), Some("unknown"), Some("USER"), Some("SYSTEM")] {
        assert_eq!((is_inno_setup_install(target), get_win32_update_type(target)), (false, UpdateType::Archive), "alvo {target:?}");
    }
}
