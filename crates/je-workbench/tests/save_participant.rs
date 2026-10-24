//! Porte de `src/vs/workbench/contrib/codeEditor/test/browser/saveParticipant.test.ts` (suíte `Save Participants`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois: `workbenchInstantiationService`, `TestServiceAccessor` e o gerenciador de
//!   modelos (`disposables.add(<TextFileEditorModelManager>accessor.textFileService.files)`). O `teardown`
//!   (`disposables.clear()`) é o descarte do guarda ao sair de escopo. Os casos `async` rodam em `run_with_faked_timers`.
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`], e `toResource.call(this, path)` é [`to_resource`] com o título do caso.
//! - **`new TestConfigurationService()` e `setUserConfiguration('files', {...})`** são o `TestConfigurationService::new(None)`
//!   e `set_user_configuration`.
//! - **`new XParticipant(configService, undefined!)`**: o `ICodeEditorService` ausente é `None`; os modelos dos casos não
//!   estão ligados a editor, então `findEditor` não consulta o serviço.
//! - **`participant.participate(model, { reason: SaveReason.EXPLICIT })`** não passa `progress` nem `token`: o porte
//!   passa um progresso que ignora tudo e `CancellationToken::None`.
//! - **`model.textEditorModel.undo()`/`redo()`** são síncronos no porte (o original devolve uma promessa já resolvida).
//! - **`pushEditOperations(..., [{ range, text, forceMoveMarkers: false }], computer)`** é o `push_edit_operations` com a
//!   `SingleEditOperation` equivalente e o calculador de cursor que devolve a seleção do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use je_base::buffer::{encode_base64, VSBuffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{to_disposable, IDisposable};
use je_base::path::PathModule;
use je_base::uri::URI;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::resources::{host_environment, uri_file};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::SaveReason;
use je_workbench::contrib::code_editor::save_participants::{FinalNewLineParticipant, TrimFinalNewLinesParticipant, TrimWhitespaceParticipant};
use je_workbench::services::textfile::collaborators::{IProgress, ProgressStep};
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use je_workbench::services::textfile::textfiles::{snapshot_to_string, ITextFileEditorModel, ITextFileSaveParticipant, TextFileSaveParticipantContext};
use je_workbench::services::working_copy::working_copy::IWorkingCopy;
use support::text_file_support::TextFileTestAccessor;

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Save Participants";

/// `Progress.None`.
struct NoProgress;

impl IProgress<ProgressStep> for NoProgress {
    fn report(&self, _item: ProgressStep) {}
}

/// O `setup`: o acessor do serviço de arquivos de texto e o gerenciador de modelos como descartável.
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let manager_slot: Rc<RefCell<Option<Rc<TextFileEditorModelManager>>>> = Rc::default();
    let slot = manager_slot.clone();
    disposables.add(to_disposable(move || {
        let manager = slot.borrow_mut().take();
        if let Some(manager) = manager {
            for model in manager.models() {
                model.dispose();
            }
        }
    }));

    let accessor = TextFileTestAccessor::new(&disposables.store());

    let files = accessor.files();
    *manager_slot.borrow_mut() = Some(files.clone());
    disposables.add_rc(files);

    accessor
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `snapshotToString(model.createSnapshot()!)`.
fn snapshot_text(model: &dyn ITextFileEditorModel) -> String {
    let mut snapshot = model.create_snapshot().expect("o modelo tem snapshot");
    snapshot_to_string(&mut *snapshot)
}

/// `model.textEditorModel.setValue(value)`.
fn set_value(model: &TextFileEditorModel, value: &str) {
    model.text_editor_model().expect("o modelo de texto existe").set_value(value.into());
}

/// `model.textEditorModel.getEOL()`.
fn eol_of(model: &TextFileEditorModel) -> String {
    String::from_utf16_lossy(model.text_editor_model().expect("o modelo de texto existe").get_eol().as_units())
}

/// `await model.resolve()` do modelo criado pelo caso.
async fn resolved_model(disposables: &LeakGuard, accessor: &TextFileTestAccessor, title: &str, path: &str) -> Rc<TextFileEditorModel> {
    let model = disposables.add_rc(accessor.create_model(&to_resource(title, path), Some("utf8"), None));
    IEditorModel::resolve(&*model).await.unwrap();
    model
}

/// `new TestConfigurationService()` com `setUserConfiguration('files', { [key]: true })`.
fn files_configuration(key: &str) -> Rc<dyn IConfigurationService> {
    let configuration_service = TestConfigurationService::new(None);
    let _ = configuration_service.set_user_configuration("files", Dynamic::object([(key, Dynamic::from(true))]), None);
    configuration_service
}

/// `await participant.participate(model, { reason: SaveReason.EXPLICIT })`.
async fn participate(participant: &dyn ITextFileSaveParticipant, model: &Rc<TextFileEditorModel>) {
    let model: Rc<dyn ITextFileEditorModel> = model.clone();
    participant
        .participate(model, TextFileSaveParticipantContext { reason: SaveReason::Explicit, saved_from: None }, Rc::new(NoProgress), CancellationToken::None)
        .await
        .unwrap();
}

#[test]
fn insert_final_new_line() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let model = resolved_model(&disposables, &accessor, "insert final new line", "/path/final_new_line.txt").await;

        let participant = FinalNewLineParticipant::new(files_configuration("insertFinalNewline"), None);

        // No new line for empty lines
        let mut line_content = String::new();
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), line_content);

        // No new line if last line already empty
        line_content = format!("Hello New Line{}", eol_of(&model));
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), line_content);

        // New empty line added (single line)
        line_content = "Hello New Line".to_string();
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), format!("{line_content}{}", eol_of(&model)));

        // New empty line added (multi line)
        let eol = eol_of(&model);
        line_content = format!("Hello New Line{eol}Hello New Line{eol}Hello New Line");
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), format!("{line_content}{}", eol_of(&model)));
    });
}

#[test]
fn trim_final_new_lines() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let model = resolved_model(&disposables, &accessor, "trim final new lines", "/path/trim_final_new_line.txt").await;

        let participant = TrimFinalNewLinesParticipant::new(files_configuration("trimFinalNewlines"), None);
        let text_content = "Trim New Line";
        let eol = eol_of(&model);

        // No new line removal if last line is not new line
        let mut line_content = text_content.to_string();
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), line_content);

        // No new line removal if last line is single new line
        line_content = format!("{text_content}{eol}");
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), line_content);

        // Remove new line (single line with two new lines)
        line_content = format!("{text_content}{eol}{eol}");
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), format!("{text_content}{eol}"));

        // Remove new lines (multiple lines with multiple new lines)
        line_content = format!("{text_content}{eol}{text_content}{eol}{eol}{eol}");
        set_value(&model, &line_content);
        participate(&participant, &model).await;
        assert_eq!(snapshot_text(&*model), format!("{text_content}{eol}{text_content}{eol}"));
    });
}

#[test]
fn trim_final_new_lines_bug_39750() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let model = resolved_model(&disposables, &accessor, "trim final new lines bug#39750", "/path/trim_final_new_line.txt").await;

        let participant = TrimFinalNewLinesParticipant::new(files_configuration("trimFinalNewlines"), None);
        let text_content = "Trim New Line";

        // single line
        set_value(&model, text_content);

        // apply edits and push to undo stack.
        let text_edits = [SingleEditOperation::new(Range::new(1, 14, 1, 14), Some(".".into()), false)];
        let text_model = model.text_editor_model().expect("o modelo de texto existe");
        text_model.push_edit_operations(Some(vec![Selection::new(1, 14, 1, 14)]), &text_edits, Some(&mut |_edits| Some(vec![Selection::new(1, 15, 1, 15)])), None, None);

        // undo
        text_model.undo();
        assert_eq!(snapshot_text(&*model), text_content);

        // trim final new lines should not mess the undo stack
        participate(&participant, &model).await;
        text_model.redo();
        assert_eq!(snapshot_text(&*model), format!("{text_content}."));
    });
}

#[test]
fn trim_final_new_lines_bug_46075() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let model = resolved_model(&disposables, &accessor, "trim final new lines bug#46075", "/path/trim_final_new_line.txt").await;

        let participant = TrimFinalNewLinesParticipant::new(files_configuration("trimFinalNewlines"), None);
        let text_content = "Test";
        let eol = eol_of(&model);
        let content = format!("{text_content}{eol}{eol}");
        set_value(&model, &content);

        // save many times
        for _ in 0..10 {
            participate(&participant, &model).await;
        }

        // confirm trimming
        assert_eq!(snapshot_text(&*model), format!("{text_content}{eol}"));

        // undo should go back to previous content immediately
        let text_model = model.text_editor_model().expect("o modelo de texto existe");
        text_model.undo();
        assert_eq!(snapshot_text(&*model), format!("{text_content}{eol}{eol}"));
        text_model.redo();
        assert_eq!(snapshot_text(&*model), format!("{text_content}{eol}"));
    });
}

#[test]
fn trim_whitespace() {
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);
        let model = resolved_model(&disposables, &accessor, "trim whitespace", "/path/trim_final_new_line.txt").await;

        let participant = TrimWhitespaceParticipant::new(files_configuration("trimTrailingWhitespace"), None);
        let text_content = "Test";
        let content = format!("{text_content} \t");
        set_value(&model, &content);

        // save many times
        for _ in 0..10 {
            participate(&participant, &model).await;
        }

        // confirm trimming
        assert_eq!(snapshot_text(&*model), text_content);
    });
}
