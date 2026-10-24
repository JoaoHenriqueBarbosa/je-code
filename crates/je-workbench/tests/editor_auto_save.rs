//! Porte de `src/vs/workbench/contrib/files/test/browser/editorAutoSave.test.ts` (suíte `EditorAutoSave`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado antes do corpo (o
//!   `disposables` do original é o store dele) e descartado depois dele (`drop(guard)`), que é o `disposables.clear()` do
//!   `teardown` seguido da conferência de vazamentos. O `setup` ([`setup`]) registra o painel `TestTextFileEditor`. O original
//!   não usa timers falsos: os casos rodam com `run_with_real_timers`.
//! - **`createEditorAutoSave`.** O [`create_editor_auto_save`] usa o [`create_accessor`] (ver `support/editor_auto_save_support.rs`):
//!   a configuração `files` do usuário, o `TestFilesConfigurationService`, a parte de editores e o `EditorService` real. O
//!   `IAccessibilitySignalService` que o original substitui não existe no porte do serviço de arquivos, então não entra. A
//!   contribuição é criada com os serviços que o `createInstance` injetaria: o serviço de configuração de arquivos de teste, o
//!   serviço de host, o `EditorService`, o serviço de grupos, o serviço de working copies, o de log, o `TestMarkerService` e o
//!   de identidade de URI do acessor.
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))` com o título completo do caso.
//! - **`Event.toPromise(Event.once(model.onDidChangeDirty))`.** É `Event::to_promise(None)` (que já dispara uma vez só).
//! - **`await editorPane?.group.closeAllEditors()`.** O grupo é a busca pelo `group_id` do painel no serviço de grupos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::path::PathModule;
use je_base::uri::URI;
use je_editor::text_model::TextSource;
use je_instantiation::Injectable;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::browser::parts::editor::editor_auto_save::EditorAutoSave;
use je_workbench::common::editor::{DEFAULT_EDITOR_ASSOCIATION_ID, TextEditorOptions, UntypedEditorInput};
use je_workbench::services::editor::editor_groups_service::IEditorPart;
use je_workbench::services::editor::editor_service::IEditorService;
use je_workbench::services::host::host::IHostService;
use je_workbench::services::textfile::textfiles::{ITextFileEditorModel, ITextFileEditorModelManager};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use je_workbench::test::browser::workbench_test_services::TestHostService;
use je_workbench::test::common::workbench_test_services::TestMarkerService;
use support::editor_auto_save_support::{AutoSaveAccessor, create_accessor};
use support::editor_service_support::{EditorServiceAccessor, register_test_file_editor};

/// O título da suíte, que o `this.test.fullTitle()` do `toResource` usa.
const SUITE_TITLE: &str = "EditorAutoSave";

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();

    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// O `setup`: `disposables.add(registerTestFileEditor())`.
fn setup(disposables: &Rc<DisposableStore>) {
    disposables.add(register_test_file_editor());
}

/// `createEditorAutoSave(autoSaveConfig)`.
async fn create_editor_auto_save(disposables: &Rc<DisposableStore>, auto_save_config: Dynamic) -> EditorServiceAccessor {
    let AutoSaveAccessor { accessor, files_configuration_service } = create_accessor(disposables, auto_save_config);

    accessor.part.when_ready().await.unwrap();

    let host_service: Rc<dyn IHostService> = Rc::new(TestHostService);
    disposables.add_rc(EditorAutoSave::create(
        (),
        (
            files_configuration_service,
            host_service,
            accessor.editor_service.clone(),
            accessor.editor_groups_service.clone(),
            accessor.working_copy_service.clone(),
            accessor.log_service.clone(),
            Rc::new(TestMarkerService),
            accessor.uri_identity_service.clone(),
        ),
    ));

    accessor
}

/// `{ autoSave, autoSaveDelay }`.
fn auto_save_after_delay(delay: i32) -> Dynamic {
    Dynamic::object([("autoSave", Dynamic::from("afterDelay")), ("autoSaveDelay", Dynamic::from(delay))])
}

/// `awaitModelSaved(model)`: `Event.toPromise(Event.once(model.onDidChangeDirty))`.
async fn await_model_saved(model: &Rc<dyn ITextFileEditorModel>) {
    IWorkingCopy::on_did_change_dirty(&**model).to_promise(None).await.unwrap();
}

/// `model.textEditorModel?.setValue('Super Good')`.
fn set_super_good(model: &Rc<dyn ITextFileEditorModel>) {
    if let Some(text_editor_model) = model.text_editor_model() {
        text_editor_model.set_value(TextSource::from("Super Good"));
    }
}

#[test]
fn editor_auto_saves_after_short_delay_if_configured() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_real_timers(async move {
        let disposables = guard.store();
        setup(&disposables);

        let accessor = create_editor_auto_save(&disposables, auto_save_after_delay(1)).await;

        let resource = to_resource("editor auto saves after short delay if configured", "/path/index.txt");

        let model = accessor.files().resolve(&resource, None).await.unwrap();
        disposables.add(model.clone());
        set_super_good(&model);

        assert!(IWorkingCopy::is_dirty(&*model));

        await_model_saved(&model).await;

        assert!(!IWorkingCopy::is_dirty(&*model));

        drop(guard);
    });
}

#[test]
fn editor_auto_saves_on_focus_change_if_configured() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_real_timers(async move {
        let disposables = guard.store();
        setup(&disposables);

        let accessor = create_editor_auto_save(&disposables, Dynamic::object([("autoSave", Dynamic::from("onFocusChange"))])).await;

        let resource = to_resource("editor auto saves on focus change if configured", "/path/index.txt");
        IEditorService::open_editor(
            accessor.editor_service.as_ref(),
            UntypedEditorInput {
                resource: Some(resource.clone()),
                options: Some(TextEditorOptions::with_override_id(DEFAULT_EDITOR_ASSOCIATION_ID)),
                ..UntypedEditorInput::default()
            },
        )
        .await
        .unwrap();

        let model = accessor.files().resolve(&resource, None).await.unwrap();
        disposables.add(model.clone());
        set_super_good(&model);

        assert!(IWorkingCopy::is_dirty(&*model));

        let editor_pane = IEditorService::open_editor(
            accessor.editor_service.as_ref(),
            UntypedEditorInput {
                resource: Some(to_resource("editor auto saves on focus change if configured", "/path/index_other.txt")),
                ..UntypedEditorInput::default()
            },
        )
        .await
        .unwrap();

        await_model_saved(&model).await;

        assert!(!IWorkingCopy::is_dirty(&*model));

        if let Some(group) = editor_pane.and_then(|editor_pane| accessor.editor_groups_service.get_group(editor_pane.group_id())) {
            group.close_all_editors(None).await.unwrap();
        }

        drop(guard);
    });
}
