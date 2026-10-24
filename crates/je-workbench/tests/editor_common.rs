//! Porte de `src/vs/workbench/test/browser/parts/editor/editor.test.ts` (suíte `Workbench editor utils`), caso a caso e na
//! mesma ordem, no que toca o modelo de dados de `common/editor.ts`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `workbenchInstantiationService(...)` só serve aqui para dar o `IEditorService` às entradas lado a lado e de diff: é o
//!   `TestEditorService`.
//! - `EditorResourceAccessor - typed inputs`: o `UntitledTextEditorInput` do original (que o `UntitledTextEditorService`
//!   cria, com o recurso `untitled:Untitled-1`) é um `TestEditorInput` com esse recurso; o que o caso mede é o acessor, que
//!   só lê `resource` e o esquema, e não depende da classe da entrada.
//! - `toString()` de URI é `to_string(false)`; `getCanonicalUri(x, { supportSideBySide: BOTH })` com resultado
//!   `{ primary, secondary }` é a variante `_sides` do acessor.
//!
//! Fora deste arquivo (e por quê): `fillActiveEditorSelection` (2 casos, `browser/parts/editor/editor.ts` e
//! `TextEditorPaneSelection`), `isEditorIdentifier` e `isEditorInputWithOptionsAndGroup` (checagens de forma sobre `unknown`
//! que o sistema de tipos do Rust resolve em tempo de compilação, e o `IEditorGroup` não foi portado), os 4 casos de
//! `createEditorPart` (parte de editores com interface) e os 6 `whenEditorClosed` (`EditorService`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_platform::resources::uri_file;
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_input::{EditorInput, EditorInputBase};
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::{
    EditorInputCapabilities, EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, EditorUri, SchemeFilter, SideBySideEditor,
    UntypedEditorInput, is_resource_diff_editor_input, is_resource_editor_input, is_resource_merge_editor_input, is_resource_side_by_side_editor_input,
    is_text_editor_view_state, is_untitled_resource_editor_input,
};
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::test::browser::workbench_test_services::{TestEditorInput, TestEditorService, TestFileEditorInput};

/// `class TestEditorInputWithPreferredResource extends TestEditorInput implements EditorInputWithPreferredResource`.
struct TestEditorInputWithPreferredResource {
    base: EditorInputBase,
    resource: URI,
    preferred_resource: URI,
    type_id: String,
}

impl TestEditorInputWithPreferredResource {
    fn new(resource: URI, preferred_resource: URI, type_id: &str) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), resource, preferred_resource, type_id: type_id.to_string() })
    }
}

impl IDisposable for TestEditorInputWithPreferredResource {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for TestEditorInputWithPreferredResource {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        self.type_id.clone()
    }

    fn editor_id(&self) -> Option<String> {
        Some(self.type_id.clone())
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn preferred_resource(&self) -> Option<URI> {
        Some(self.preferred_resource.clone())
    }
}

fn editor_service() -> Rc<dyn IEditorService> {
    Rc::new(TestEditorService)
}

fn typed<T: EditorInput + 'static>(editor: &Rc<T>) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Typed(editor.clone())
}

fn untyped(editor: &UntypedEditorInput) -> EditorInputOrUntyped {
    EditorInputOrUntyped::Untyped(editor.clone())
}

fn uri_with_scheme(resource: URI, scheme: &str) -> URI {
    resource.with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// `resource?.toString()`.
fn uri_string(resource: Option<&URI>) -> Option<String> {
    resource.map(|resource| resource.to_string(false).to_string_lossy())
}

fn resource_string(resource: &URI) -> Option<String> {
    uri_string(Some(resource))
}

fn options(side_by_side: Option<SideBySideEditor>, filter: Option<SchemeFilter>) -> EditorResourceAccessorOptions {
    EditorResourceAccessorOptions { support_side_by_side: side_by_side, filter_by_scheme: filter }
}

fn scheme(scheme: &str) -> Option<SchemeFilter> {
    Some(SchemeFilter::from(scheme))
}

fn schemes(first: &str, second: &str) -> Option<SchemeFilter> {
    Some(SchemeFilter::from([first, second].as_slice()))
}

/// `EditorResourceAccessor.getCanonicalUri(editor, options)?.toString()`.
fn canonical(editor: &EditorInputOrUntyped, side_by_side: Option<SideBySideEditor>, filter: Option<SchemeFilter>) -> Option<String> {
    uri_string(EditorResourceAccessor::get_canonical_uri(Some(editor), Some(&options(side_by_side, filter))).as_ref())
}

/// `EditorResourceAccessor.getOriginalUri(editor, options)?.toString()`.
fn original(editor: &EditorInputOrUntyped, side_by_side: Option<SideBySideEditor>, filter: Option<SchemeFilter>) -> Option<String> {
    uri_string(EditorResourceAccessor::get_original_uri(Some(editor), Some(&options(side_by_side, filter))).as_ref())
}

/// `(EditorResourceAccessor.getCanonicalUri(editor, { supportSideBySide: BOTH, ... }) as { primary, secondary })`.
fn canonical_sides(editor: &EditorInputOrUntyped, filter: Option<SchemeFilter>) -> (Option<String>, Option<String>) {
    sides(EditorResourceAccessor::get_canonical_uri_sides(Some(editor), Some(&options(Some(SideBySideEditor::Both), filter))))
}

/// `(EditorResourceAccessor.getOriginalUri(editor, { supportSideBySide: BOTH, ... }) as { primary, secondary })`.
fn original_sides(editor: &EditorInputOrUntyped, filter: Option<SchemeFilter>) -> (Option<String>, Option<String>) {
    sides(EditorResourceAccessor::get_original_uri_sides(Some(editor), Some(&options(Some(SideBySideEditor::Both), filter))))
}

fn sides(result: Option<EditorUri>) -> (Option<String>, Option<String>) {
    match result {
        Some(EditorUri::Sides { primary, secondary }) => (uri_string(primary.as_ref()), uri_string(secondary.as_ref())),
        Some(EditorUri::Resource(_)) | None => panic!("o acessor devia devolver os dois lados"),
    }
}

/// As asserções de uma entrada simples (`untitled`, `file`) contra o acessor: o `resource` com todos os modos de lado a lado e
/// os filtros de esquema, para a URI canônica e para a original.
fn assert_simple_editor(editor: &EditorInputOrUntyped, resource: &URI, own_scheme: &str, other_scheme: &str) {
    let expected = resource_string(resource);
    let both = SideBySideEditor::Both;

    for accessor in [canonical, original] {
        assert_eq!(accessor(editor, None, None), expected);
        assert_eq!(accessor(editor, Some(SideBySideEditor::Primary), None), expected);
        assert_eq!(accessor(editor, Some(SideBySideEditor::Any), None), expected);
        assert_eq!(accessor(editor, Some(SideBySideEditor::Secondary), None), expected);
        assert_eq!(accessor(editor, Some(both), None), expected);
        assert_eq!(accessor(editor, None, scheme(own_scheme)), expected);
        assert_eq!(accessor(editor, None, schemes(schemas::FILE, schemas::UNTITLED)), expected);
        assert!(accessor(editor, None, scheme(other_scheme)).is_none());
    }
}

/// As asserções de uma entrada composta (diff, lado a lado) com `file` no lado primário e `untitled` no secundário.
fn assert_compound_editor(editor: &EditorInputOrUntyped, file: &URI, untitled: &URI) {
    let (file, untitled) = (resource_string(file), resource_string(untitled));
    let primary = SideBySideEditor::Primary;
    let secondary = SideBySideEditor::Secondary;

    for accessor in [canonical, original] {
        assert!(accessor(editor, None, None).is_none());
        assert!(accessor(editor, None, scheme(schemas::FILE)).is_none());

        assert_eq!(accessor(editor, Some(primary), None), file);
        assert_eq!(accessor(editor, Some(primary), scheme(schemas::FILE)), file);
        assert_eq!(accessor(editor, Some(primary), schemes(schemas::FILE, schemas::UNTITLED)), file);

        assert_eq!(accessor(editor, Some(secondary), None), untitled);
        assert_eq!(accessor(editor, Some(secondary), scheme(schemas::UNTITLED)), untitled);
        assert_eq!(accessor(editor, Some(secondary), schemes(schemas::FILE, schemas::UNTITLED)), untitled);
    }

    for accessor in [canonical_sides, original_sides] {
        assert_eq!(accessor(editor, None).0, file);
        assert_eq!(accessor(editor, scheme(schemas::FILE)).0, file);
        assert_eq!(accessor(editor, schemes(schemas::FILE, schemas::UNTITLED)).0, file);

        assert_eq!(accessor(editor, None).1, untitled);
        assert_eq!(accessor(editor, scheme(schemas::UNTITLED)).1, untitled);
        assert_eq!(accessor(editor, schemes(schemas::FILE, schemas::UNTITLED)).1, untitled);
    }
}

#[test]
fn untyped_check_functions() {
    let file = || uri_file("/");
    let resource_input = |resource: URI| UntypedEditorInput::from_resource(resource);
    let diff_input = || UntypedEditorInput {
        original: Some(Box::new(resource_input(file()))),
        modified: Some(Box::new(resource_input(file()))),
        ..UntypedEditorInput::default()
    };
    let side_by_side_input = || UntypedEditorInput {
        primary: Some(Box::new(resource_input(file()))),
        secondary: Some(Box::new(resource_input(file()))),
        ..UntypedEditorInput::default()
    };

    assert!(!is_resource_editor_input(None));
    assert!(!is_resource_editor_input(Some(&untyped(&UntypedEditorInput::default()))));
    assert!(!is_resource_editor_input(Some(&untyped(&diff_input()))));
    assert!(is_resource_editor_input(Some(&untyped(&resource_input(file())))));

    assert!(!is_untitled_resource_editor_input(None));
    assert!(is_untitled_resource_editor_input(Some(&untyped(&UntypedEditorInput::default()))));
    assert!(is_untitled_resource_editor_input(Some(&untyped(&resource_input(uri_with_scheme(file(), schemas::UNTITLED))))));
    assert!(is_untitled_resource_editor_input(Some(&untyped(&UntypedEditorInput { force_untitled: Some(true), ..resource_input(file()) }))));

    assert!(!is_resource_diff_editor_input(None));
    assert!(!is_resource_diff_editor_input(Some(&untyped(&UntypedEditorInput::default()))));
    assert!(!is_resource_diff_editor_input(Some(&untyped(&resource_input(file())))));
    assert!(is_resource_diff_editor_input(Some(&untyped(&diff_input()))));
    assert!(is_resource_diff_editor_input(Some(&untyped(&UntypedEditorInput {
        primary: side_by_side_input().primary,
        secondary: side_by_side_input().secondary,
        ..diff_input()
    }))));
    assert!(!is_resource_diff_editor_input(Some(&untyped(&side_by_side_input()))));

    assert!(!is_resource_side_by_side_editor_input(None));
    assert!(!is_resource_side_by_side_editor_input(Some(&untyped(&UntypedEditorInput::default()))));
    assert!(!is_resource_side_by_side_editor_input(Some(&untyped(&resource_input(file())))));
    assert!(is_resource_side_by_side_editor_input(Some(&untyped(&side_by_side_input()))));
    assert!(!is_resource_side_by_side_editor_input(Some(&untyped(&diff_input()))));
    assert!(!is_resource_side_by_side_editor_input(Some(&untyped(&UntypedEditorInput {
        original: diff_input().original,
        modified: diff_input().modified,
        ..side_by_side_input()
    }))));

    let merge_input = UntypedEditorInput {
        input1: Some(Box::new(resource_input(file()))),
        input2: Some(Box::new(resource_input(file()))),
        base: Some(Box::new(resource_input(file()))),
        result: Some(Box::new(resource_input(file()))),
        ..UntypedEditorInput::default()
    };
    assert!(!is_resource_merge_editor_input(None));
    assert!(!is_resource_merge_editor_input(Some(&untyped(&UntypedEditorInput::default()))));
    assert!(!is_resource_merge_editor_input(Some(&untyped(&resource_input(file())))));
    assert!(is_resource_merge_editor_input(Some(&untyped(&merge_input))));
}

#[test]
fn editor_input_capabilities() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    let test_input1 = disposables.add_rc(TestFileEditorInput::new(uri_file("resource1"), "testTypeId"));
    let test_input2 = disposables.add_rc(TestFileEditorInput::new(uri_file("resource2"), "testTypeId"));

    test_input1.set_capabilities(EditorInputCapabilities::NONE);
    assert!(test_input1.has_capability(EditorInputCapabilities::NONE));
    assert!(!test_input1.has_capability(EditorInputCapabilities::READONLY));
    assert!(!test_input1.is_readonly().is_readonly());
    assert!(!test_input1.has_capability(EditorInputCapabilities::UNTITLED));
    assert!(!test_input1.has_capability(EditorInputCapabilities::REQUIRES_TRUST));
    assert!(!test_input1.has_capability(EditorInputCapabilities::SINGLETON));

    test_input1.set_capabilities(test_input1.capabilities() | EditorInputCapabilities::READONLY);
    assert!(test_input1.has_capability(EditorInputCapabilities::READONLY));
    assert!(test_input1.is_readonly().is_readonly());
    assert!(!test_input1.has_capability(EditorInputCapabilities::NONE));
    assert!(!test_input1.has_capability(EditorInputCapabilities::UNTITLED));
    assert!(!test_input1.has_capability(EditorInputCapabilities::REQUIRES_TRUST));
    assert!(!test_input1.has_capability(EditorInputCapabilities::SINGLETON));

    test_input1.set_capabilities(EditorInputCapabilities::NONE);
    test_input2.set_capabilities(EditorInputCapabilities::NONE);

    let side_by_side_input = disposables.add_rc(SideBySideEditorInput::new(Some("name"), None, test_input1.clone(), test_input2.clone(), editor_service));
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::MULTIPLE_EDITORS));
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::READONLY));
    assert!(!side_by_side_input.is_readonly().is_readonly());
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::UNTITLED));
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::REQUIRES_TRUST));
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::SINGLETON));

    test_input1.set_capabilities(test_input1.capabilities() | EditorInputCapabilities::READONLY);
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::READONLY));
    assert!(!side_by_side_input.is_readonly().is_readonly());

    test_input2.set_capabilities(test_input2.capabilities() | EditorInputCapabilities::READONLY);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::READONLY));
    assert!(side_by_side_input.is_readonly().is_readonly());

    test_input1.set_capabilities(test_input1.capabilities() | EditorInputCapabilities::UNTITLED);
    assert!(!side_by_side_input.has_capability(EditorInputCapabilities::UNTITLED));

    test_input2.set_capabilities(test_input2.capabilities() | EditorInputCapabilities::UNTITLED);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::UNTITLED));

    test_input1.set_capabilities(test_input1.capabilities() | EditorInputCapabilities::REQUIRES_TRUST);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::REQUIRES_TRUST));

    test_input2.set_capabilities(test_input2.capabilities() | EditorInputCapabilities::REQUIRES_TRUST);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::REQUIRES_TRUST));

    test_input1.set_capabilities(test_input1.capabilities() | EditorInputCapabilities::SINGLETON);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::SINGLETON));

    test_input2.set_capabilities(test_input2.capabilities() | EditorInputCapabilities::SINGLETON);
    assert!(side_by_side_input.has_capability(EditorInputCapabilities::SINGLETON));
}

#[test]
fn editor_resource_accessor_typed_inputs() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let editor_service = editor_service();

    assert!(EditorResourceAccessor::get_canonical_uri(None, None).is_none());
    assert!(EditorResourceAccessor::get_original_uri(None, None).is_none());

    let untitled_resource = URI::from(&UriComponents { scheme: Some(schemas::UNTITLED.into()), path: Some("Untitled-1".into()), ..UriComponents::default() }, false);
    let untitled = disposables.add_rc(TestEditorInput::new(untitled_resource.clone(), "editorResourceUntitledTest"));
    assert_simple_editor(&typed(&untitled), &untitled_resource, schemas::UNTITLED, schemas::FILE);

    let file_resource = uri_file("/some/path.txt");
    let file = disposables.add_rc(TestEditorInput::new(file_resource.clone(), "editorResourceFileTest"));
    assert_simple_editor(&typed(&file), &file_resource, schemas::FILE, schemas::UNTITLED);

    let diff_input = DiffEditorInput::new(Some("name"), Some("description"), untitled.clone(), file.clone(), None, editor_service.clone());
    let side_by_side_input = SideBySideEditorInput::new(Some("name"), Some("description"), untitled.clone(), file.clone(), editor_service);
    assert_compound_editor(&typed(&diff_input), &file_resource, &untitled_resource);
    assert_compound_editor(&typed(&side_by_side_input), &file_resource, &untitled_resource);

    let resource = uri_file("/some/path.txt");
    let preferred_resource = uri_file("/some/PATH.txt");
    let file_with_preferred_resource =
        disposables.add_rc(TestEditorInputWithPreferredResource::new(uri_file("/some/path.txt"), uri_file("/some/PATH.txt"), "editorResourceFileTest"));

    assert_eq!(canonical(&typed(&file_with_preferred_resource), None, None), resource_string(&resource));
    assert_eq!(original(&typed(&file_with_preferred_resource), None, None), resource_string(&preferred_resource));
}

#[test]
fn editor_resource_accessor_untyped_inputs() {
    assert!(EditorResourceAccessor::get_canonical_uri(None, None).is_none());
    assert!(EditorResourceAccessor::get_original_uri(None, None).is_none());

    let untitled_uri = URI::from(
        &UriComponents { scheme: Some(schemas::UNTITLED.into()), authority: Some("foo".into()), path: Some("/bar".into()), ..UriComponents::default() },
        false,
    );
    let untitled = UntypedEditorInput::from_resource(untitled_uri.clone());
    assert_simple_editor(&untyped(&untitled), &untitled_uri, schemas::UNTITLED, schemas::FILE);

    let file_uri = uri_file("/some/path.txt");
    let file = UntypedEditorInput::from_resource(file_uri.clone());
    assert_simple_editor(&untyped(&file), &file_uri, schemas::FILE, schemas::UNTITLED);

    let diff_input = UntypedEditorInput { original: Some(Box::new(untitled.clone())), modified: Some(Box::new(file.clone())), ..UntypedEditorInput::default() };
    let side_by_side_input = UntypedEditorInput { primary: Some(Box::new(file.clone())), secondary: Some(Box::new(untitled.clone())), ..UntypedEditorInput::default() };
    for untyped_input in [diff_input, side_by_side_input] {
        assert_compound_editor(&untyped(&untyped_input), &file_uri, &untitled_uri);
    }

    let file_merge = UntypedEditorInput {
        input1: Some(Box::new(UntypedEditorInput::from_resource(uri_file("/some/remote.txt")))),
        input2: Some(Box::new(UntypedEditorInput::from_resource(uri_file("/some/local.txt")))),
        base: Some(Box::new(UntypedEditorInput::from_resource(uri_file("/some/base.txt")))),
        result: Some(Box::new(UntypedEditorInput::from_resource(uri_file("/some/merged.txt")))),
        ..UntypedEditorInput::default()
    };

    let merged = resource_string(&uri_file("/some/merged.txt"));
    assert_eq!(canonical(&untyped(&file_merge), None, None), merged);
    assert_eq!(original(&untyped(&file_merge), None, None), merged);
}

#[test]
fn is_text_editor_view_state_cases() {
    let object = |entries: Vec<(&str, Dynamic)>| Dynamic::object(entries);

    assert!(!is_text_editor_view_state(&Dynamic::Undefined));
    assert!(!is_text_editor_view_state(&object(Vec::new())));

    let code_editor_view_state = object(vec![
        ("contributionsState", object(Vec::new())),
        ("cursorState", Dynamic::array(Vec::new())),
        (
            "viewState",
            object(vec![
                ("scrollLeft", Dynamic::Number(0.0)),
                ("firstPosition", object(vec![("lineNumber", Dynamic::Number(1.0)), ("column", Dynamic::Number(1.0))])),
                ("firstPositionDeltaTop", Dynamic::Number(1.0)),
            ]),
        ),
    ]);

    assert!(is_text_editor_view_state(&code_editor_view_state));

    let diff_editor_view_state = object(vec![("original", code_editor_view_state.clone()), ("modified", code_editor_view_state)]);

    assert!(is_text_editor_view_state(&diff_editor_view_state));
}
