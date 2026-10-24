//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/outputCopyTests.test.ts` (suíte `Cell Output Clipboard Tests`), caso a caso e na mesma ordem. Os 8 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. Os casos `async` rodam com
//!   `run_with_real_timers`, e `await copyCellOutput(...)` é o `.await` da promessa devolvida.
//! - `class ClipboardService { clipboardContent; writeText(value) }` é o [`ClipboardService`], com os outros membros do
//!   `IClipboardService` ausentes (o `as unknown as IClipboardService` do original), e `new class extends mock<ILogService>() { }` é o
//!   [`MockLogService`], em que todo membro lança `TypeError` (nenhum caso chega a logar).
//! - `{ model: { outputs } } as ICellOutputViewModel` e `{ outputsViewModels, model: { outputs } } as ICellViewModel` são o
//!   [`TestOutputViewModel`] e o [`TestCell`], objetos que só sabem o que o `cellOutputTextHelper` lê, e `{ outputs }` (o `model`) é o
//!   [`TestOutput`], um `ICellOutput` em que só `outputs` responde. O ciclo `outputViewModel.cellViewModel = cellViewModel` é um `Rc`
//!   de cada lado, que vaza no fim do caso (são só objetos de dados, sem `Disposable`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::JsString;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::log::{LogArg, LogLevel, LogMessage, LogService, Logger};
use je_workbench::contrib::notebook::browser::view_model::cell_output_text_helper::{ICellOutputViewModel, ICellOutputsOwner, copy_cell_output};
use je_workbench::contrib::notebook::common::notebook_common::{ICellOutput, IOutputDto, IOutputItemDto};

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `class ClipboardService`.
#[derive(Default)]
struct ClipboardService {
    clipboard_content: RefCell<String>,
}

impl ClipboardService {
    fn clipboard_content(&self) -> String {
        self.clipboard_content.borrow().clone()
    }
}

impl IClipboardService for ClipboardService {
    fn write_text(&self, text: &str) -> Promise<()> {
        *self.clipboard_content.borrow_mut() = text.to_string();
        Promise::resolved(())
    }

    fn read_text(&self) -> Promise<String> {
        absent("readText")
    }

    fn read_find_text(&self) -> Promise<JsString> {
        absent("readFindText")
    }

    fn write_find_text(&self, _text: &JsString) -> Promise<()> {
        absent("writeFindText")
    }

    fn write_resources(&self, _resources: Vec<URI>) -> Promise<()> {
        absent("writeResources")
    }

    fn read_resources(&self) -> Promise<Vec<URI>> {
        absent("readResources")
    }

    fn has_resources(&self) -> Promise<bool> {
        absent("hasResources")
    }
}

/// `new class extends mock<ILogService>() { }`.
struct MockLogService;

impl IDisposable for MockLogService {
    fn dispose(&self) {}
}

impl Logger for MockLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        absent("onDidChangeLogLevel")
    }

    fn get_level(&self) -> LogLevel {
        absent("getLevel")
    }

    fn set_level(&self, _level: LogLevel) {
        absent("setLevel")
    }

    fn trace_args(&self, _message: &str, _args: &[LogArg]) {
        absent("trace")
    }

    fn debug_args(&self, _message: &str, _args: &[LogArg]) {
        absent("debug")
    }

    fn info_args(&self, _message: &str, _args: &[LogArg]) {
        absent("info")
    }

    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {
        absent("warn")
    }

    fn error_args(&self, _message: &LogMessage, _args: &[LogArg]) {
        absent("error")
    }

    fn flush(&self) {
        absent("flush")
    }
}

/// `{ outputs }`: o `model` de uma saída.
struct TestOutput {
    outputs: Vec<IOutputItemDto>,
}

impl IDisposable for TestOutput {
    fn dispose(&self) {}
}

impl ICellOutput for TestOutput {
    fn version_id(&self) -> i64 {
        absent("versionId")
    }

    fn outputs(&self) -> Vec<IOutputItemDto> {
        self.outputs.clone()
    }

    fn metadata(&self) -> Option<Dynamic> {
        absent("metadata")
    }

    fn output_id(&self) -> String {
        absent("outputId")
    }

    fn alternative_output_id(&self) -> String {
        absent("alternativeOutputId")
    }

    fn on_did_change_data(&self) -> Event<()> {
        absent("onDidChangeData")
    }

    fn replace_data(&self, _items: IOutputDto) {
        absent("replaceData")
    }

    fn append_data(&self, _items: Vec<IOutputItemDto>) {
        absent("appendData")
    }

    fn appended_since_version(&self, _version_id: i64, _mime: &str) -> Option<VSBuffer> {
        absent("appendedSinceVersion")
    }

    fn as_dto(&self) -> IOutputDto {
        absent("asDto")
    }

    fn bump_version(&self) {
        absent("bumpVersion")
    }
}

/// `{ outputsViewModels, model: { outputs } } as ICellViewModel`.
#[derive(Default)]
struct TestCell {
    outputs_view_models: RefCell<Vec<Rc<TestOutputViewModel>>>,
    model_outputs: RefCell<Vec<Rc<dyn ICellOutput>>>,
}

impl ICellOutputsOwner for TestCell {
    fn outputs_view_models(&self) -> Vec<Rc<dyn ICellOutputViewModel>> {
        self.outputs_view_models.borrow().iter().map(|output| output.clone() as Rc<dyn ICellOutputViewModel>).collect()
    }

    fn model_outputs(&self) -> Vec<Rc<dyn ICellOutput>> {
        self.model_outputs.borrow().clone()
    }
}

/// `{ model: { outputs } } as ICellOutputViewModel`.
struct TestOutputViewModel {
    model: Rc<dyn ICellOutput>,
    cell_view_model: RefCell<Option<Rc<TestCell>>>,
}

impl ICellOutputViewModel for TestOutputViewModel {
    fn model(&self) -> Rc<dyn ICellOutput> {
        self.model.clone()
    }

    fn cell_view_model(&self) -> Option<Rc<dyn ICellOutputsOwner>> {
        self.cell_view_model.borrow().clone().map(|cell| cell as Rc<dyn ICellOutputsOwner>)
    }
}

fn dto(text: &str, mime: &str) -> IOutputItemDto {
    IOutputItemDto { data: VSBuffer::from_string(text), mime: mime.to_string() }
}

/// `createOutputViewModel(outputs, cellViewModel?)`.
fn create_output_view_model(outputs: Vec<IOutputItemDto>, cell_view_model: Option<&Rc<TestCell>>) -> Rc<TestOutputViewModel> {
    let model: Rc<dyn ICellOutput> = Rc::new(TestOutput { outputs });
    let output_view_model = Rc::new(TestOutputViewModel { model: model.clone(), cell_view_model: RefCell::new(None) });

    let cell_view_model = match cell_view_model {
        Some(cell_view_model) => {
            cell_view_model.outputs_view_models.borrow_mut().push(output_view_model.clone());
            cell_view_model.model_outputs.borrow_mut().push(model);
            cell_view_model.clone()
        }
        None => {
            let cell_view_model = Rc::new(TestCell::default());
            cell_view_model.outputs_view_models.borrow_mut().push(output_view_model.clone());
            cell_view_model.model_outputs.borrow_mut().push(model);
            cell_view_model
        }
    };

    *output_view_model.cell_view_model.borrow_mut() = Some(cell_view_model);
    output_view_model
}

/// O `TestCell` dono de uma saída (`output.cellViewModel as ICellViewModel`).
fn cell_of(output: &Rc<TestOutputViewModel>) -> Rc<TestCell> {
    output.cell_view_model.borrow().clone().expect("a saída tem a célula dona")
}

fn log_service() -> Rc<dyn LogService> {
    Rc::new(MockLogService)
}

#[test]
fn copy_text_plain_output() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mime_type = "text/plain";
        let clipboard = Rc::new(ClipboardService::default());

        let output_dto = dto("output content", "text/plain");
        let output = create_output_view_model(vec![output_dto], None);

        copy_cell_output(Some(mime_type), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "output content");
    });
}

#[test]
fn nothing_copied_for_invalid_mimetype() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let output_dtos = vec![dto("output content", "bad"), dto("output 2", "unknown")];
        let output = create_output_view_model(output_dtos, None);

        copy_cell_output(Some("bad"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "");
    });
}

#[test]
fn text_copied_if_available_instead_of_invalid_mime_type() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let output_dtos = vec![dto("output content", "bad"), dto("text content", "text/plain")];
        let output = create_output_view_model(output_dtos, None);

        copy_cell_output(Some("bad"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "text content");
    });
}

#[test]
fn selected_mimetype_is_preferred() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let output_dtos = vec![dto("plain text", "text/plain"), dto("html content", "text/html")];
        let output = create_output_view_model(output_dtos, None);

        copy_cell_output(Some("text/html"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "html content");
    });
}

#[test]
fn copy_subsequent_output() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let output = create_output_view_model(vec![dto("first", "text/plain")], None);
        let output2 = create_output_view_model(vec![dto("second", "text/plain")], Some(&cell_of(&output)));
        let output3 = create_output_view_model(vec![dto("third", "text/plain")], Some(&cell_of(&output)));

        copy_cell_output(Some("text/plain"), &*output2, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "second");

        copy_cell_output(Some("text/plain"), &*output3, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "third");
    });
}

#[test]
fn adjacent_stream_outputs_are_concanented() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let output = create_output_view_model(vec![dto("stdout", "application/vnd.code.notebook.stdout")], None);
        let cell = cell_of(&output);
        create_output_view_model(vec![dto("stderr", "application/vnd.code.notebook.stderr")], Some(&cell));
        create_output_view_model(vec![dto("text content", "text/plain")], Some(&cell));
        create_output_view_model(vec![dto("non-adjacent", "application/vnd.code.notebook.stdout")], Some(&cell));

        copy_cell_output(Some("application/vnd.code.notebook.stdout"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "stdoutstderr");
    });
}

#[test]
fn error_output_uses_the_value_in_the_stack() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let data = VSBuffer::from_string(r#"{"name":"Error Name","message":"error message","stack":"error stack"}"#);
        let output = create_output_view_model(vec![IOutputItemDto { data, mime: "application/vnd.code.notebook.error".to_string() }], None);

        copy_cell_output(Some("application/vnd.code.notebook.error"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "error stack");
    });
}

#[test]
fn error_without_stack_uses_the_name_and_message() {
    run_with_real_timers(async move {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let clipboard = Rc::new(ClipboardService::default());

        let data = VSBuffer::from_string(r#"{"name":"Error Name","message":"error message"}"#);
        let output = create_output_view_model(vec![IOutputItemDto { data, mime: "application/vnd.code.notebook.error".to_string() }], None);

        copy_cell_output(Some("application/vnd.code.notebook.error"), &*output, clipboard.clone(), log_service()).await.unwrap();

        assert_eq!(clipboard.clipboard_content(), "Error Name: error message");
    });
}
