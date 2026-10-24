//! Porte de `src/vs/workbench/test/browser/parts/editor/editorInput.test.ts` (suíte `EditorInput`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables` e o
//!   `teardown` do original são o próprio guarda.
//! - `class MyEditorInput extends EditorInput` é a struct local, que guarda o `EditorInputBase`.
//! - `isEditorInput(undefined)`, `isEditorInput({ resource })` e `isEditorInput({})` são o `None` e as entradas sem tipo
//!   ([`UntypedEditorInput`]); `isUntitledResourceEditorInput(input as any)` é a entrada tipada.
//!
//! Fora deste arquivo (e por quê): os casos `Untpyed inputs properly match TextResourceEditorInput`, `... FileEditorInput`,
//! `... MergeEditorInput`, `... UntitledTextEditorInput` e `... DiffEditorInput`, que criam essas entradas pelo
//! `workbenchInstantiationService` (`FileEditorInput`, `MergeEditorInput`, `UntitledTextEditorInput` e o
//! `TextResourceEditorInput` com os serviços de modelo de texto ainda não foram portados).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::{
    EditorInputOrUntyped, TextEditorOptions, UntypedEditorInput, is_editor_input, is_resource_diff_editor_input, is_resource_editor_input,
    is_resource_merge_editor_input, is_resource_side_by_side_editor_input, is_untitled_resource_editor_input,
};
use je_workbench::test::browser::workbench_test_services::TestEditorInput;

/// `class MyEditorInput extends EditorInput { readonly resource = undefined; typeId = 'myEditorInput'; }`.
struct MyEditorInput {
    base: EditorInputBase,
}

impl MyEditorInput {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()) })
    }
}

impl IDisposable for MyEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for MyEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        "myEditorInput".to_string()
    }

    fn resource(&self) -> Option<URI> {
        None
    }
}

fn typed<T: EditorInput + 'static>(editor: &Rc<T>) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

/// `{ resource, options: { override } }`.
fn untyped_with_override(resource: URI, override_id: &str) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Untyped(UntypedEditorInput {
        resource: Some(resource),
        options: Some(TextEditorOptions::with_override_id(override_id)),
        ..UntypedEditorInput::default()
    })
}

#[test]
fn basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let counter = Rc::new(Cell::new(0));
    let input = disposables.add_rc(MyEditorInput::new());
    let other_input = disposables.add_rc(MyEditorInput::new());

    assert!(is_editor_input(Some(&typed(&input))));
    assert!(!is_editor_input(None));
    assert!(!is_editor_input(Some(&EditorInputOrUntyped::Untyped(UntypedEditorInput::from_resource(uri_file("/"))))));
    assert!(!is_editor_input(Some(&EditorInputOrUntyped::Untyped(UntypedEditorInput::default()))));

    assert!(!is_resource_editor_input(Some(&typed(&input))));
    assert!(!is_untitled_resource_editor_input(Some(&typed(&input))));
    assert!(!is_resource_diff_editor_input(Some(&typed(&input))));
    assert!(!is_resource_merge_editor_input(Some(&typed(&input))));
    assert!(!is_resource_side_by_side_editor_input(Some(&typed(&input))));

    assert!(input.matches(&typed(&input)));
    assert!(!input.matches(&typed(&other_input)));
    assert!(!input.get_name().is_empty());

    let on_will_dispose_counter = counter.clone();
    disposables.add(input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    input.dispose();
    assert_eq!(counter.get(), 1);
}

#[test]
fn untyped_matches() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let test_input_id = "untypedMatches";
    let test_input_resource = uri_file("/fake");
    let test_input = disposables.add_rc(TestEditorInput::new(test_input_resource.clone(), test_input_id));
    let test_untyped_input = untyped_with_override(test_input_resource.clone(), test_input_id);
    let tet_untyped_input_wrong_resource = untyped_with_override(uri_file("/incorrectFake"), test_input_id);
    let test_untyped_input_wrong_id = untyped_with_override(test_input_resource, "wrongId");
    let test_untyped_input_wrong = untyped_with_override(uri_file("/incorrectFake"), "wrongId");

    assert!(test_input.matches(&test_untyped_input));
    assert!(!test_input.matches(&tet_untyped_input_wrong_resource));
    assert!(!test_input.matches(&test_untyped_input_wrong_id));
    assert!(!test_input.matches(&test_untyped_input_wrong));
}
