//! Porte de `src/vs/workbench/test/browser/parts/editor/editorModel.test.ts` (suíte `EditorModel`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables` e o
//!   `teardown` do original são o próprio guarda (`disposables.add(...)` é `add` e `add_rc`).
//! - `await model.resolve()` é a chamada direta: a promessa do `EditorModel` já nasce resolvida.
//!
//! Fora deste arquivo (e por quê): o caso `BaseTextEditorModel`, que precisa do `ModelService`, do `LanguageService`, do
//! `UndoRedoService`, do `LanguageDetectionService` e do `TestAccessibilityService`, e do `BaseTextEditorModel`
//! (`textEditorModel.ts`), que ainda não foram portados.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_workbench::common::editor::editor_model::{EditorModel, IEditorModel};

#[test]
fn basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let counter = Rc::new(Cell::new(0));

    let model = disposables.add_rc(EditorModel::new());

    let on_will_dispose_counter = counter.clone();
    disposables.add(model.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    let _ = model.resolve();
    assert!(!model.is_disposed());
    assert!(model.is_resolved());
    model.dispose();
    assert_eq!(counter.get(), 1);
    assert!(model.is_disposed());
}
