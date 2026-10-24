//! Porte de `src/vs/workbench/test/browser/parts/editor/diffEditorInput.test.ts` (suíte `Diff editor input`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `disposables` e o
//!   `teardown` do original são o próprio guarda.
//! - `workbenchInstantiationService(undefined, disposables)` só serve aqui para dar o `IEditorService` ao construtor da
//!   entrada: é o `TestEditorService`.
//! - `class MyEditorInput extends EditorInput` é a struct local; `assert.strictEqual(a, b)` entre entradas compara a
//!   identidade do objeto ([`id_of`]).
//! - `assert.deepStrictEqual({...}, {...})` sobre os três booleanos é a comparação das tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{IDisposable, id_of};
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase, UntypedEditorOptions, matches_by_default};
use je_workbench::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorResourceAccessor, TextEditorOptions, UntypedEditorInput, Verbosity, is_diff_editor_input,
    is_resource_diff_editor_input, is_resource_side_by_side_editor_input,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::test::browser::workbench_test_services::TestEditorService;

/// `class MyEditorInput extends EditorInput` do arquivo de teste.
struct MyEditorInput {
    base: EditorInputBase,
    resource: Option<URI>,
    description: Option<String>,
}

impl MyEditorInput {
    /// `constructor(public resource: URI | undefined = undefined, private readonly _description?: string)`.
    fn new(resource: Option<URI>, description: Option<&str>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource, description: description.map(str::to_string) })
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

    fn get_description(&self, _verbosity: Option<Verbosity>) -> Option<String> {
        self.description.clone()
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
    let input = disposables.add_rc(MyEditorInput::new(None, None));
    let on_will_dispose_counter = counter.clone();
    disposables.add(input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    let other_input = disposables.add_rc(MyEditorInput::new(None, None));
    let on_will_dispose_counter = counter.clone();
    disposables.add(other_input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    let diff_input = DiffEditorInput::new(Some("name"), Some("description"), input.clone(), other_input.clone(), None, editor_service);

    assert!(is_diff_editor_input(Some(&typed(&diff_input))));
    assert!(!is_diff_editor_input(Some(&typed(&input))));

    assert_eq!(id_of(&diff_input.original()), id_of(&input));
    assert_eq!(id_of(&diff_input.modified()), id_of(&other_input));
    assert!(diff_input.matches(&typed(&diff_input)));
    assert!(!diff_input.matches(&typed(&other_input)));

    diff_input.dispose();
    assert_eq!(counter.get(), 0);
}

#[test]
fn forces_descriptions_only_when_they_distinguish_identical_names() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();
    let has_force_description = |original_description: Option<&str>, modified_description: Option<&str>| {
        let input = disposables.add_rc(MyEditorInput::new(None, original_description));
        let other_input = disposables.add_rc(MyEditorInput::new(None, modified_description));
        let diff_input = disposables.add_rc(DiffEditorInput::new(None, None, input, other_input, None, editor_service.clone()));
        diff_input.has_capability(EditorInputCapabilities::FORCE_DESCRIPTION)
    };

    assert_eq!(
        (has_force_description(Some("folder"), Some("folder")), has_force_description(Some("folder-a"), Some("folder-b")), has_force_description(None, None)),
        // identical, different, missing
        (false, true, false)
    );
}

#[test]
fn to_untyped() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let input = disposables.add_rc(MyEditorInput::new(Some(uri_file("foo/bar1")), None));
    let other_input = disposables.add_rc(MyEditorInput::new(Some(uri_file("foo/bar2")), None));

    let diff_input = DiffEditorInput::new(Some("name"), Some("description"), input.clone(), other_input.clone(), None, editor_service);

    let untyped_diff_input = diff_input.to_untyped(None).map(EditorInputOrUntyped::Untyped);
    assert!(is_resource_diff_editor_input(untyped_diff_input.as_ref()));
    assert!(!is_resource_side_by_side_editor_input(untyped_diff_input.as_ref()));
    assert!(diff_input.matches(untyped_diff_input.as_ref().expect("o diff tem forma sem tipo")));
}

#[test]
fn disposes_when_input_inside_disposes() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let counter = Rc::new(Cell::new(0));
    let input = disposables.add_rc(MyEditorInput::new(None, None));
    let other_input = disposables.add_rc(MyEditorInput::new(None, None));

    let diff_input = disposables.add_rc(DiffEditorInput::new(Some("name"), Some("description"), input.clone(), other_input.clone(), None, editor_service.clone()));
    let on_will_dispose_counter = counter.clone();
    disposables.add(diff_input.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    input.dispose();

    let input = disposables.add_rc(MyEditorInput::new(None, None));
    let other_input = disposables.add_rc(MyEditorInput::new(None, None));

    let diff_input2 = disposables.add_rc(DiffEditorInput::new(Some("name"), Some("description"), input, other_input.clone(), None, editor_service));
    let on_will_dispose_counter = counter.clone();
    disposables.add(diff_input2.on_will_dispose().listen(move |_| on_will_dispose_counter.set(on_will_dispose_counter.get() + 1)));

    other_input.dispose();
    assert_eq!(counter.get(), 2);
}
