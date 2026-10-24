//! Porte de `src/vs/workbench/services/textfile/test/browser/textEditorService.test.ts` (suíte `TextEditorService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `teardown`.** O `setup` registra o painel `MyTestEditorForEditorService` e os painéis de recurso de texto e
//!   lado a lado ([`setup`]); o `teardown` (`disposables.clear()`) é o `drop` do `LeakGuard` ao fim do corpo, que também faz o
//!   `ensureNoDisposablesAreLeakedInTestSuite()`. O `workbenchInstantiationService(undefined, disposables)` é o
//!   [`EditorServiceAccessor`] (as divergências dele estão em `support/editor_service_support.rs`); o `TextEditorService` do
//!   caso é um segundo, criado por `create_instance` como no original.
//! - **`toResource.call(this, path)`.** É `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do
//!   caso ([`to_resource`]).
//! - **`assert(input instanceof T)`.** É o `downcast_ref` de `EditorInput::as_any`; `assert.strictEqual(a, b)` e
//!   `assert.notStrictEqual(a, b)` entre entradas são a identidade do ponteiro ([`same`]).
//! - **`disposables.add(input)`.** É [`track`], que registra o descarte da entrada.
//! - **Entrada tipada em `createTextEditor(input)`.** O original recebe um `EditorInput` pela mesma sobrecarga das entradas
//!   sem tipo e o trata pelo `resource` dele (uma `FileEditorInput` não tem `label`, `contents` nem as demais chaves da entrada
//!   sem tipo): o contrato do porte recebe só a [`UntypedEditorInput`], então o caso passa `{ resource }` da entrada.
//! - **`input.resolve()`.** É `FileEditorInput::resolve_file` para o arquivo e `UntitledTextEditorInput::resolve_untitled` para
//!   o sem título, que devolvem o modelo.
//! - **`URI.from({ scheme, path })`.** É `URI::from` com as duas componentes.
//! - **`isLinux`.** É `Platform::detect().is_linux`.
//! - **`FileServiceProvider`.** Está em `support/text_editor_service_support.rs`, com o `NullFileSystemProvider`.
//! - **Relógio.** O original não usa timers falsos: os casos rodam em `run_with_real_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::lifecycle::{IDisposable, id_of, to_disposable};
use je_base::path::{Platform, PathModule};
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::resources::{fs_path, host_environment, uri_file};
use je_workbench::common::editor::diff_editor_input::DiffEditorInput;
use je_workbench::common::editor::editor_input::EditorInput;
use je_workbench::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use je_workbench::common::editor::text_resource_editor_input::TextResourceEditorInput;
use je_workbench::common::editor::{EditorInputRef, TextEditorOptions, UntypedEditorInput};
use je_workbench::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use je_workbench::services::textfile::text_editor_service::{ITextEditorService, TextEditorService};
use je_workbench::services::textfile::textfiles::ITextFileEditorModel;
use je_workbench::services::textmodel_resolver::resolver_service::ITextEditorModel;
use je_workbench::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::test::browser::workbench_test_services::{TestFileEditorInput, register_test_editor};
use support::editor_service_support::{EditorServiceAccessor, register_test_resource_editor, register_test_side_by_side_editor};
use support::text_editor_service_support::FileServiceProvider;

const SUITE_TITLE: &str = "TextEditorService";
const TEST_EDITOR_ID: &str = "MyTestEditorForEditorService";
const TEST_EDITOR_INPUT_ID: &str = "testEditorInputForEditorService";

//#region Utilitários dos casos

/// O `setup` da suíte.
fn setup(disposables: &LeakGuard) {
    disposables.add(register_test_editor(TEST_EDITOR_ID, vec![std::any::TypeId::of::<TestFileEditorInput>()], Some(TEST_EDITOR_INPUT_ID)));
    disposables.add(register_test_resource_editor());
    disposables.add(register_test_side_by_side_editor());
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `disposables.add(input)`: devolve a própria entrada.
fn track(disposables: &LeakGuard, input: EditorInputRef) -> EditorInputRef {
    let tracked = input.clone();
    disposables.add(to_disposable(move || tracked.dispose()));

    input
}

/// `a === b` entre entradas.
fn same(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}

/// `{ resource }`.
fn resource_input(resource: &URI) -> UntypedEditorInput {
    UntypedEditorInput::from_resource(resource.clone())
}

/// `options: { selection: { startLineNumber: 1, startColumn: 1 } }`.
fn selection_options() -> Option<TextEditorOptions> {
    Some(TextEditorOptions {
        selection: Some(TextEditorSelection { start_line_number: 1.0, start_column: 1.0, end_line_number: None, end_column: None }),
        ..TextEditorOptions::default()
    })
}

/// `(input as FileEditorInput)` depois de `assert(input instanceof FileEditorInput)`.
fn as_file_input(input: &EditorInputRef) -> &FileEditorInput {
    input.as_any().downcast_ref::<FileEditorInput>().expect("a entrada é uma FileEditorInput")
}

/// `(input as UntitledTextEditorInput)` depois de `assert(input instanceof UntitledTextEditorInput)`.
fn as_untitled_input(input: &EditorInputRef) -> &UntitledTextEditorInput {
    input.as_any().downcast_ref::<UntitledTextEditorInput>().expect("a entrada é uma UntitledTextEditorInput")
}

/// `new TextEditorService` do caso: `instantiationService.createInstance(TextEditorService)`.
fn create_service(disposables: &LeakGuard, accessor: &EditorServiceAccessor) -> Rc<TextEditorService> {
    disposables.add_rc(accessor.instantiation_service.create_instance::<TextEditorService>(()))
}

/// `disposables.add((await contentInput.resolve() as ITextFileEditorModel))`: o modelo do arquivo, que é descartado ao fim.
async fn resolve_file_model(disposables: &LeakGuard, input: &EditorInputRef) -> Rc<dyn je_workbench::common::editor::editor_model::IEditorModel> {
    let model = as_file_input(input).resolve_file(None).await.unwrap();
    let tracked = model.clone();
    disposables.add(to_disposable(move || tracked.dispose()));

    model
}

/// `(model as TextFileEditorModel)`.
fn as_text_file_editor_model(model: &Rc<dyn je_workbench::common::editor::editor_model::IEditorModel>) -> &TextFileEditorModel {
    model.as_any().downcast_ref::<TextFileEditorModel>().expect("o modelo é um TextFileEditorModel")
}

//#endregion

#[test]
fn create_text_editor_basics() {
    const TITLE: &str = "createTextEditor - basics";
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&disposables);
        let accessor = EditorServiceAccessor::with_default_backup_service(&disposables.store());
        disposables.add_rc(accessor.files());
        let service = create_service(&disposables, &accessor);

        let language_id = "create-input-test";
        disposables.add(accessor.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        // Untyped Input (file)
        let mut input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput { resource: Some(to_resource(TITLE, "/index.html")), options: selection_options(), ..UntypedEditorInput::default() }),
        );
        assert_eq!(fs_path(&as_file_input(&input).resource), fs_path(&to_resource(TITLE, "/index.html")));

        // Untyped Input (file casing)
        input = track(&disposables, service.create_text_editor(resource_input(&to_resource(TITLE, "/index.html"))));
        let input_different_case = track(&disposables, service.create_text_editor(resource_input(&to_resource(TITLE, "/INDEX.html"))));

        if !Platform::detect().is_linux {
            assert!(same(&input, &input_different_case));
            assert_eq!(input.resource().map(|resource| uri_string(&resource)), input_different_case.resource().map(|resource| uri_string(&resource)));
        } else {
            assert!(!same(&input, &input_different_case));
            assert_ne!(input.resource().map(|resource| uri_string(&resource)), input_different_case.resource().map(|resource| uri_string(&resource)));
        }

        // Typed Input
        assert!(same(&track(&disposables, service.create_text_editor(resource_input(&input.resource().expect("a entrada tem recurso")))), &input));

        // Untyped Input (file, encoding)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput {
                resource: Some(to_resource(TITLE, "/index.html")),
                encoding: Some("utf16le".to_string()),
                options: selection_options(),
                ..UntypedEditorInput::default()
            }),
        );
        assert_eq!(as_file_input(&input).get_preferred_encoding().as_deref(), Some("utf16le"));

        // Untyped Input (file, language)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput { resource: Some(to_resource(TITLE, "/index.html")), language_id: Some(language_id.to_string()), ..UntypedEditorInput::default() }),
        );
        assert_eq!(as_file_input(&input).get_preferred_language_id().as_deref(), Some(language_id));
        let mut file_model = resolve_file_model(&disposables, &input).await;
        assert_eq!(ITextFileEditorModel::text_editor_model(as_text_file_editor_model(&file_model)).expect("o modelo de texto existe").get_language_id(), language_id);

        // Untyped Input (file, contents)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput { resource: Some(to_resource(TITLE, "/index.html")), contents: Some("My contents".to_string()), ..UntypedEditorInput::default() }),
        );
        file_model = resolve_file_model(&disposables, &input).await;
        assert_eq!(ITextFileEditorModel::text_editor_model(as_text_file_editor_model(&file_model)).expect("o modelo de texto existe").get_value().to_string_lossy(), "My contents");
        assert!(IWorkingCopy::is_dirty(as_text_file_editor_model(&file_model)));

        // Untyped Input (file, different language)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput { resource: Some(to_resource(TITLE, "/index.html")), language_id: Some("text".to_string()), ..UntypedEditorInput::default() }),
        );
        assert_eq!(as_file_input(&input).get_preferred_language_id().as_deref(), Some("text"));

        // Untyped Input (untitled)
        input = track(&disposables, service.create_text_editor(UntypedEditorInput { resource: None, options: selection_options(), ..UntypedEditorInput::default() }));
        as_untitled_input(&input);

        // Untyped Input (untitled with contents)
        let mut untyped_input = UntypedEditorInput { contents: Some("Hello Untitled".to_string()), options: selection_options(), ..UntypedEditorInput::default() };
        input = track(&disposables, service.create_text_editor(untyped_input.clone()));
        assert!(untyped_input.is_untitled_resource_editor_input());
        let mut model = as_untitled_input(&input).resolve_untitled().await.unwrap();
        let tracked = model.clone();
        disposables.add(to_disposable(move || tracked.dispose()));
        assert_eq!(model.text_editor_model().expect("o modelo de texto existe").get_value().to_string_lossy(), "Hello Untitled");

        // Untyped Input (untitled with language id)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput { resource: None, language_id: Some(language_id.to_string()), options: selection_options(), ..UntypedEditorInput::default() }),
        );
        model = as_untitled_input(&input).resolve_untitled().await.unwrap();
        let tracked = model.clone();
        disposables.add(to_disposable(move || tracked.dispose()));
        assert_eq!(model.get_language_id().as_deref(), Some(language_id));

        // Untyped Input (untitled with file path)
        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput {
                resource: Some(uri_file("/some/path.txt")),
                force_untitled: Some(true),
                options: selection_options(),
                ..UntypedEditorInput::default()
            }),
        );
        assert!(as_untitled_input(&input).has_associated_file_path());

        // Untyped Input (untitled with untitled resource)
        untyped_input = UntypedEditorInput {
            resource: Some(URI::parse("untitled://Untitled-1", false)),
            force_untitled: Some(true),
            options: selection_options(),
            ..UntypedEditorInput::default()
        };
        assert!(untyped_input.is_untitled_resource_editor_input());
        input = track(&disposables, service.create_text_editor(untyped_input.clone()));
        assert!(!as_untitled_input(&input).has_associated_file_path());

        // Untyped input (untitled with custom resource, but forceUntitled)
        untyped_input = UntypedEditorInput { resource: Some(uri_file("/fake")), force_untitled: Some(true), ..UntypedEditorInput::default() };
        assert!(untyped_input.is_untitled_resource_editor_input());
        input = track(&disposables, service.create_text_editor(untyped_input.clone()));
        as_untitled_input(&input);

        // Untyped Input (untitled with custom resource)
        let provider = disposables.add_rc(FileServiceProvider::new("untitled-custom", accessor.file_service.clone()));

        input = track(
            &disposables,
            service.create_text_editor(UntypedEditorInput {
                resource: Some(URI::parse("untitled-custom://some/path", false)),
                force_untitled: Some(true),
                options: selection_options(),
                ..UntypedEditorInput::default()
            }),
        );
        assert!(as_untitled_input(&input).has_associated_file_path());

        provider.dispose();

        // Untyped Input (resource)
        input = track(&disposables, service.create_text_editor(resource_input(&URI::parse("custom:resource", false))));
        assert!(input.as_any().downcast_ref::<TextResourceEditorInput>().is_some());

        // Untyped Input (diff)
        let resource_diff_input = UntypedEditorInput {
            modified: Some(Box::new(resource_input(&to_resource(TITLE, "/modified.html")))),
            original: Some(Box::new(resource_input(&to_resource(TITLE, "/original.html")))),
            ..UntypedEditorInput::default()
        };
        assert!(resource_diff_input.is_resource_diff_editor_input());
        input = track(&disposables, service.create_text_editor(resource_diff_input.clone()));
        let diff_input = input.as_any().downcast_ref::<DiffEditorInput>().expect("a entrada é uma DiffEditorInput");
        track(&disposables, diff_input.modified());
        track(&disposables, diff_input.original());
        let original_resource = uri_string(resource_diff_input.original.as_ref().and_then(|original| original.resource.as_ref()).expect("o diff tem `original`"));
        let modified_resource = uri_string(resource_diff_input.modified.as_ref().and_then(|modified| modified.resource.as_ref()).expect("o diff tem `modified`"));
        assert_eq!(diff_input.original().resource().map(|resource| uri_string(&resource)), Some(original_resource.clone()));
        assert_eq!(diff_input.modified().resource().map(|resource| uri_string(&resource)), Some(modified_resource.clone()));
        let untyped_diff_input = diff_input.to_untyped(None).expect("o diff tem uma entrada sem tipo");
        assert_eq!(untyped_diff_input.original.as_ref().and_then(|original| original.resource.as_ref()).map(uri_string), Some(original_resource));
        assert_eq!(untyped_diff_input.modified.as_ref().and_then(|modified| modified.resource.as_ref()).map(uri_string), Some(modified_resource));

        // Untyped Input (side by side)
        let side_by_side_resource_input = UntypedEditorInput {
            primary: Some(Box::new(resource_input(&to_resource(TITLE, "/primary.html")))),
            secondary: Some(Box::new(resource_input(&to_resource(TITLE, "/secondary.html")))),
            ..UntypedEditorInput::default()
        };
        assert!(side_by_side_resource_input.is_resource_side_by_side_editor_input());
        input = track(&disposables, service.create_text_editor(side_by_side_resource_input.clone()));
        let side_by_side_input = input.as_any().downcast_ref::<SideBySideEditorInput>().expect("a entrada é uma SideBySideEditorInput");
        track(&disposables, side_by_side_input.primary());
        track(&disposables, side_by_side_input.secondary());
        let primary_resource = uri_string(side_by_side_resource_input.primary.as_ref().and_then(|primary| primary.resource.as_ref()).expect("o lado a lado tem `primary`"));
        let secondary_resource = uri_string(side_by_side_resource_input.secondary.as_ref().and_then(|secondary| secondary.resource.as_ref()).expect("o lado a lado tem `secondary`"));
        assert_eq!(side_by_side_input.primary().resource().map(|resource| uri_string(&resource)), Some(primary_resource.clone()));
        assert_eq!(side_by_side_input.secondary().resource().map(|resource| uri_string(&resource)), Some(secondary_resource.clone()));
        let untyped_side_by_side_input = side_by_side_input.to_untyped(None).expect("o lado a lado tem uma entrada sem tipo");
        assert_eq!(untyped_side_by_side_input.primary.as_ref().and_then(|primary| primary.resource.as_ref()).map(uri_string), Some(primary_resource));
        assert_eq!(untyped_side_by_side_input.secondary.as_ref().and_then(|secondary| secondary.resource.as_ref()).map(uri_string), Some(secondary_resource));
        drop(disposables);
    });
}

#[test]
fn create_text_editor_caching() {
    const TITLE: &str = "createTextEditor- caching";
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        setup(&disposables);
        let accessor = EditorServiceAccessor::with_default_backup_service(&disposables.store());
        disposables.add_rc(accessor.files());
        let service = create_service(&disposables, &accessor);

        // Cached Input (Files)
        let file_resource1 = to_resource(TITLE, "/foo/bar/cache1.js");
        let file_editor_input1 = track(&disposables, service.create_text_editor(resource_input(&file_resource1)));

        let file_resource2 = to_resource(TITLE, "/foo/bar/cache2.js");
        let file_editor_input2 = track(&disposables, service.create_text_editor(resource_input(&file_resource2)));

        assert!(!same(&file_editor_input1, &file_editor_input2));

        let file_editor_input1_again = track(&disposables, service.create_text_editor(resource_input(&file_resource1)));
        assert!(same(&file_editor_input1_again, &file_editor_input1));

        file_editor_input1_again.dispose();

        assert!(file_editor_input1.is_disposed());

        let file_editor_input1_again_and_again = track(&disposables, service.create_text_editor(resource_input(&file_resource1)));
        assert!(!same(&file_editor_input1_again_and_again, &file_editor_input1));
        assert!(!file_editor_input1_again_and_again.is_disposed());

        // Cached Input (Resource)
        let custom_resource = |path: &str| URI::from(&UriComponents { scheme: Some("custom".into()), path: Some(path.into()), ..UriComponents::default() }, false);
        let resource1 = custom_resource("/foo/bar/cache1.js");
        let input1 = track(&disposables, service.create_text_editor(resource_input(&resource1)));

        let resource2 = custom_resource("/foo/bar/cache2.js");
        let input2 = track(&disposables, service.create_text_editor(resource_input(&resource2)));

        assert!(!same(&input1, &input2));

        let input1_again = track(&disposables, service.create_text_editor(resource_input(&resource1)));
        assert!(same(&input1_again, &input1));

        input1_again.dispose();

        assert!(input1.is_disposed());

        let input1_again_and_again = track(&disposables, service.create_text_editor(resource_input(&resource1)));
        assert!(!same(&input1_again_and_again, &input1));
        assert!(!input1_again_and_again.is_disposed());
        drop(disposables);
    });
}
