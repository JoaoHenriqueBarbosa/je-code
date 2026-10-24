//! Porte parcial de `src/vs/workbench/contrib/update/test/browser/releaseNotesRenderer.test.ts` (suíte `Conditional blocks`, casos
//! que não renderizam markdown), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **Recorte.** A suíte `Release notes renderer` (`Should render TOC`, `keeps shorthand as inline code without the tryout
//!   renderer`, `Should render code settings`) e os dois últimos casos de `Conditional blocks` (`renderReleaseNotesMarkdown passes
//!   stable quality correctly` e `... insider quality correctly`) chamam `renderReleaseNotesMarkdown`, que depende do `marked` com
//!   extensões, do sanitizador de HTML, do `SimpleSettingRenderer` e do `assertSnapshot`; nada disso existe no porte ainda e os
//!   casos ficam sem porte, não excluídos.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `new Set(['IN_PRODUCT', 'STABLE'])` é o [`conditions`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::collections::HashSet;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::update::browser::release_notes_editor::process_conditional_blocks;

fn conditions(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn in_product_block_is_revealed_when_in_product_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF IN_PRODUCT %\nin-product content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(result.contains("in-product content"));
    assert!(!result.contains("%IF"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn web_block_is_removed_when_only_in_product_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF WEB %\nweb-only content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(!result.contains("web-only content"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn stable_block_is_revealed_when_stable_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF STABLE %\nstable content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(result.contains("stable content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn stable_block_is_removed_when_insiders_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF STABLE %\nstable content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "INSIDERS"]));
    assert!(!result.contains("stable content"));
    assert!(result.contains("before"));
    assert!(result.contains("after"));
}

#[test]
fn insiders_block_is_revealed_when_insiders_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF INSIDERS %\ninsiders content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "INSIDERS"]));
    assert!(result.contains("insiders content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn insiders_block_is_removed_when_stable_is_active() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "before\n<!-- %IF INSIDERS %\ninsiders content\n%ENDIF % -->\nafter";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(!result.contains("insiders content"));
}

#[test]
fn conditions_are_case_insensitive() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = "<!-- %IF in_product %\ncontent\n%endif % -->";
    let result = process_conditional_blocks(text, &conditions(&["IN_PRODUCT"]));
    assert!(result.contains("content"));
    assert!(!result.contains("%IF"));
}

#[test]
fn multiple_conditional_blocks_in_same_document() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [
        "shared content",
        "<!-- %IF IN_PRODUCT %",
        "in-product only",
        "%ENDIF % -->",
        "<!-- %IF WEB %",
        "web only",
        "%ENDIF % -->",
        "<!-- %IF STABLE %",
        "stable only",
        "%ENDIF % -->",
        "<!-- %IF INSIDERS %",
        "insiders only",
        "%ENDIF % -->",
        "more shared content",
    ]
    .join("\n");
    let result = process_conditional_blocks(&text, &conditions(&["IN_PRODUCT", "STABLE"]));
    assert!(result.contains("shared content"));
    assert!(result.contains("in-product only"));
    assert!(!result.contains("web only"));
    assert!(result.contains("stable only"));
    assert!(!result.contains("insiders only"));
    assert!(result.contains("more shared content"));
}
