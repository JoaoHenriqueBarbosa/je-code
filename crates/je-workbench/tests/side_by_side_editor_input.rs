//! Porte de `src/vs/workbench/test/browser/parts/editor/sideBySideEditorInput.test.ts` (suíte `SideBySideEditorInput`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables` e o
//!   `teardown` do original são o próprio guarda.
//! - `workbenchInstantiationService(undefined, disposables)` só serve aqui para dar o `IEditorService` ao construtor da
//!   entrada: é o `TestEditorService`.
//! - `class MyEditorInput extends EditorInput` é a struct local; `assert.strictEqual(a, b)` entre entradas compara a
//!   identidade do objeto ([`id_of`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{IDisposable, id_of};
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase, UntypedEditorOptions, matches_by_default};
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    EditorInputOrUntyped, EditorResourceAccessor, TextEditorOptions, UntypedEditorInput, is_resource_side_by_side_editor_input, is_side_by_side_editor_input,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestFileEditorInput};

/// `class MyEditorInput extends EditorInput` do arquivo de teste.
struct MyEditorInput {
    base: EditorInputBase,
    resource: Option<URI>,
}

impl MyEditorInput {
    /// `constructor(public resource: URI | undefined = undefined)`.
    fn new(resource: Option<URI>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource })
    }

    fn fire_capabilities_change_event(&self) {
        self.base.fire_did_change_capabilities();
    }

    fn fire_dirty_change_event(&self) {
        self.base.fire_did_change_dirty();
    }

    fn fire_label_change_event(&self) {
        self.base.fire_did_change_label();
    }
}

impl IDisposable for MyEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `resource?.toString()`.
fn uri_string(resource: Option<&URI>) -> Option<String> {
    resource.map(|resource| resource.to_string(false).to_string_lossy())
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
        self.resource.clone()
    }

    fn to_untyped(&self, _options: Option<&UntypedEditorOptions>) -> Option<UntypedEditorInput> {
        Some(UntypedEditorInput {
            resource: self.resource.clone(),
            options: Some(TextEditorOptions::with_override_id(&self.editor_type_id())),
            ..UntypedEditorInput::default()
        })
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        if matches_by_default(self, other_input) {
            return true;
        }

        let resource = EditorResourceAccessor::get_canonical_uri(Some(other_input), None);
        uri_string(resource.as_ref()) == uri_string(self.resource.as_ref())
    }
}

fn typed<T: EditorInput + 'static>(editor: &Rc<T>) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

fn editor_service() -> Rc<dyn IEditorService> {
    Rc::new(TestEditorService)
}

#[test]
fn basics() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let counter = Rc::new(Cell::new(0));
    let input = disposables.add_rc(MyEditorInput::new(Some(uri_file("/fake"))));
    let on_will_dispose_counter = counter.clone();
    disposables.add(input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    let other_input = disposables.add_rc(MyEditorInput::new(Some(uri_file("/fake2"))));
    let on_will_dispose_counter = counter.clone();
    disposables.add(other_input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(Some("name"), Some("description"), input.clone(), other_input.clone(), editor_service.clone()));
    assert_eq!(side_by_side_input.get_name(), "name");
    assert_eq!(side_by_side_input.get_description(None).as_deref(), Some("description"));

    assert!(is_side_by_side_editor_input(Some(&typed(&side_by_side_input))));
    assert!(!is_side_by_side_editor_input(Some(&typed(&input))));

    assert_eq!(id_of(&side_by_side_input.secondary()), id_of(&input));
    assert_eq!(id_of(&side_by_side_input.primary()), id_of(&other_input));
    assert!(side_by_side_input.matches(&typed(&side_by_side_input)));
    assert!(!side_by_side_input.matches(&typed(&other_input)));

    side_by_side_input.dispose();
    assert_eq!(counter.get(), 0);

    let side_by_side_input_same = disposables.add_rc(SideBySideEditorInput::new(None, None, input.clone(), input.clone(), editor_service));
    assert_eq!(side_by_side_input_same.get_name(), input.get_name());
    assert_eq!(side_by_side_input_same.get_description(None), input.get_description(None));
    assert_eq!(side_by_side_input_same.get_title(None), input.get_title(None));
    assert_eq!(uri_string(side_by_side_input_same.resource().as_ref()), uri_string(input.resource().as_ref()));
}

#[test]
fn events_dispatching() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let input = disposables.add_rc(MyEditorInput::new(None));
    let other_input = disposables.add_rc(MyEditorInput::new(None));

    let side_by_side_inut = disposables.add_rc(SideBySideEditorInput::new(Some("name"), Some("description"), other_input.clone(), input.clone(), editor_service));

    assert!(is_side_by_side_editor_input(Some(&typed(&side_by_side_inut))));

    let capabilities_change_counter = Rc::new(Cell::new(0));
    let counter = capabilities_change_counter.clone();
    disposables.add(side_by_side_inut.on_did_change_capabilities().listen(move |_| counter.set(counter.get() + 1)));

    let dirty_change_counter = Rc::new(Cell::new(0));
    let counter = dirty_change_counter.clone();
    disposables.add(side_by_side_inut.on_did_change_dirty().listen(move |_| counter.set(counter.get() + 1)));

    let label_change_counter = Rc::new(Cell::new(0));
    let counter = label_change_counter.clone();
    disposables.add(side_by_side_inut.on_did_change_label().listen(move |_| counter.set(counter.get() + 1)));

    input.fire_capabilities_change_event();
    assert_eq!(capabilities_change_counter.get(), 1);

    other_input.fire_capabilities_change_event();
    assert_eq!(capabilities_change_counter.get(), 2);

    input.fire_dirty_change_event();
    other_input.fire_dirty_change_event();
    assert_eq!(dirty_change_counter.get(), 1);

    input.fire_label_change_event();
    other_input.fire_label_change_event();
    assert_eq!(label_change_counter.get(), 2);
}

#[test]
fn to_untyped() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let primary_input = disposables.add_rc(MyEditorInput::new(Some(uri_file("/fake"))));
    let secondary_input = disposables.add_rc(MyEditorInput::new(Some(uri_file("/fake2"))));

    let side_by_side_input =
        disposables.add_rc(SideBySideEditorInput::new(Some("Side By Side Test"), None, secondary_input.clone(), primary_input.clone(), editor_service));

    let untyped_side_by_side_input = side_by_side_input.to_untyped(None).map(EditorInputOrUntyped::Untyped);
    assert!(is_resource_side_by_side_editor_input(untyped_side_by_side_input.as_ref()));
}

#[test]
fn untyped_matches() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let primary_input = disposables.add_rc(TestFileEditorInput::new(uri_file("/fake"), "primaryId"));
    let secondary_input = disposables.add_rc(TestFileEditorInput::new(uri_file("/fake2"), "secondaryId"));
    let side_by_side_input =
        disposables.add_rc(SideBySideEditorInput::new(Some("Side By Side Test"), None, secondary_input.clone(), primary_input.clone(), editor_service));

    // `{ resource, options: { override } }`
    let untyped_side = |resource: &str, override_id: &str| UntypedEditorInput {
        resource: Some(uri_file(resource)),
        options: Some(TextEditorOptions::with_override_id(override_id)),
        ..UntypedEditorInput::default()
    };
    // `{ primary, secondary }`
    let side_by_side_untyped = |primary: UntypedEditorInput, secondary: UntypedEditorInput| {
        EditorInputOrUntyped::Untyped(UntypedEditorInput { primary: Some(Box::new(primary)), secondary: Some(Box::new(secondary)), ..UntypedEditorInput::default() })
    };

    let side_by_side_untyped1 = side_by_side_untyped(untyped_side("/fake", "primaryId"), untyped_side("/fake2", "secondaryId"));

    assert!(side_by_side_input.matches(&side_by_side_untyped1));

    let side_by_side_untyped2 = side_by_side_untyped(untyped_side("/fake", "primaryIdWrong"), untyped_side("/fake2", "secondaryId"));

    assert!(!side_by_side_input.matches(&side_by_side_untyped2));

    let side_by_side_untyped3 = side_by_side_untyped(untyped_side("/fake", "primaryId"), untyped_side("/fake2Wrong", "secondaryId"));

    assert!(!side_by_side_input.matches(&side_by_side_untyped3));
}
