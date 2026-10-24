//! Porte de `src/vs/workbench/contrib/files/test/browser/fileActions.test.ts` (suítes `Files - Paste`, `Files - Increment
//! file name simple` e `Files - Increment file name smart`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** Na suíte `Files - Paste` o guarda é o `disposables` que o caso passa ao
//!   `workbenchInstantiationService`; nas suítes de nome ele é declarado no fim da suíte e vale para cada caso (o `LeakGuard`
//!   no começo de cada um).
//! - **`workbenchInstantiationService({ configurationService: () => configurationService }, disposables)`.** O
//!   `TestInstantiationService` com os serviços que os handlers leem, nas versões de teste do porte: `TestFileService`,
//!   o `TestConfigurationService` do caso, o `UriIdentityService` real sobre o arquivo de teste, `TestEditorService`,
//!   `TestDialogService` (nenhum diálogo abre neste caso) e o `TestNotificationService` embrulhado pelo espião.
//! - **`instantiationService.spy(INotificationService, 'error')`.** É o [`SpyNotificationService`]: delega ao
//!   `TestNotificationService` e guarda os argumentos de cada chamada de `error` (`notification.args`).
//! - **`instantiationService.stub(IExplorerService, { getContext, getEditable, setToCopy, applyBulkEdit })`.** É o
//!   [`StubExplorerService`]; os membros que o objeto do original não define falham se chamados (`TypeError`).
//! - **`let context = [source]; ... context = [destination]`.** O `context` é uma célula que o caso troca entre os handlers.
//! - **`new FileOperationError(message, FileOperationResult.FILE_MOVE_CONFLICT)` e `throw error`.** É a promessa rejeitada
//!   com o [`FileOperationError::into_error`], e `error.message` é o `message` do erro.
//! - **`await instantiationService.invokeFunction(handler)`.** O `invoke_function` devolve a promessa do handler, e o
//!   `await` é `.await.unwrap()` dentro de `run_with_faked_timers`.
//! - **Casos de nome.** `assert.strictEqual(result, esperado)` é `assert_eq!`; os identificadores levam o prefixo `simple_`
//!   e `smart_` porque as duas suítes repetem os títulos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "../../je-editor/tests/support/test_clipboard_service.rs"]
mod test_clipboard_service;
#[path = "../../je-editor/tests/support/test_instantiation_service.rs"]
mod test_instantiation_service;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::services::clipboard_service::{I_CLIPBOARD_SERVICE, IClipboardService};
use je_instantiation::Injectable;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::dialogs::{Confirmation, ConfirmationResult, I_DIALOG_SERVICE, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::files::files::{FileOperationError, FileOperationResult};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::notification::{
    I_NOTIFICATION_SERVICE, INotificationService, Notification, NotificationFilterSetting, NotificationHandle, NotificationMessage, NotificationSource,
    NotificationSourceFilter, NotificationsFilter, PromptChoiceKind, PromptOptions, Severity, StatusHandle, StatusMessageOptions,
};
use je_platform::resources::uri_file;
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_workbench::contrib::files::browser_files::{
    EditableItem, ExplorerBulkEditOptions, ExplorerReveal, I_EXPLORER_SERVICE, IExplorerService,
};
use je_workbench::contrib::files::common_files::IncrementalNaming;
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::contrib::files::file_actions::{cut_file_handler, increment_file_name, paste_file_handler};
use je_workbench::services::editor::editor_service::I_EDITOR_SERVICE;
use je_workbench::test::browser::workbench_test_services::TestEditorService;
use je_workbench::test::common::workbench_test_services::{NullFilesConfigurationService, TestFileService, TestNotificationService};
use test_clipboard_service::TestClipboardService;
use test_instantiation_service::TestInstantiationService;

/// `instantiationService.spy(INotificationService, 'error')`: o serviço de notificação de teste, com os argumentos de cada
/// chamada de `error` guardados.
struct SpyNotificationService {
    inner: TestNotificationService,
    error_args: RefCell<Vec<Vec<String>>>,
}

impl SpyNotificationService {
    fn new() -> Self {
        Self { inner: TestNotificationService, error_args: RefCell::new(Vec::new()) }
    }

    /// `notification.args`: os argumentos de cada chamada de `error`.
    fn args(&self) -> Vec<Vec<String>> {
        self.error_args.borrow().clone()
    }
}

impl INotificationService for SpyNotificationService {
    fn on_did_change_filter(&self) -> Event<()> {
        self.inner.on_did_change_filter()
    }

    fn set_filter(&self, filter: NotificationFilterSetting) {
        self.inner.set_filter(filter);
    }

    fn get_filter(&self, source: Option<&NotificationSource>) -> NotificationsFilter {
        self.inner.get_filter(source)
    }

    fn get_filters(&self) -> Vec<NotificationSourceFilter> {
        self.inner.get_filters()
    }

    fn remove_filter(&self, source_id: &str) {
        self.inner.remove_filter(source_id);
    }

    fn notify(&self, notification: Notification) -> Rc<dyn NotificationHandle> {
        self.inner.notify(notification)
    }

    fn info(&self, message: Vec<NotificationMessage>) {
        self.inner.info(message);
    }

    fn warn(&self, message: Vec<NotificationMessage>) {
        self.inner.warn(message);
    }

    fn error(&self, message: Vec<NotificationMessage>) {
        let texts = message
            .iter()
            .map(|message| match message {
                NotificationMessage::Text(text) => text.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        self.error_args.borrow_mut().push(texts);
        self.inner.error(message);
    }

    fn prompt(&self, severity: Severity, message: NotificationMessage, choices: Vec<PromptChoiceKind>, options: Option<PromptOptions>) -> Rc<dyn NotificationHandle> {
        self.inner.prompt(severity, message, choices, options)
    }

    fn status(&self, message: NotificationMessage, options: Option<StatusMessageOptions>) -> Rc<dyn StatusHandle> {
        self.inner.status(message, options)
    }
}

/// `TestDialogService` do `workbenchInstantiationService`: nenhum diálogo abre neste caso, então qualquer chamada falha.
struct TestDialogService;

fn dialog_not_exercised(method: &str) -> ! {
    unreachable!("TestDialogService.{method} não é exercitado por esta suíte")
}

impl IDialogService for TestDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        dialog_not_exercised("confirm")
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        dialog_not_exercised("prompt")
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        dialog_not_exercised("input")
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        dialog_not_exercised("info")
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        dialog_not_exercised("warn")
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        dialog_not_exercised("error")
    }

    fn about(&self) -> Promise<()> {
        dialog_not_exercised("about")
    }
}

/// O objeto que o caso registra como `IExplorerService`: só `getContext`, `getEditable`, `setToCopy` e `applyBulkEdit`.
struct StubExplorerService {
    /// `context`.
    context: RefCell<Vec<Rc<ExplorerItem>>>,
    clipboard_service: Rc<TestClipboardService>,
    /// O `error` que `applyBulkEdit` lança.
    error: Error,
}

fn explorer_not_defined(member: &str) -> ! {
    je_base::errors::throw(Error::type_error(format!("explorerService.{member} is not a function")))
}

impl IExplorerService for StubExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        explorer_not_defined("roots")
    }

    fn get_context(&self, _respect_multi_selection: bool, _ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        self.context.borrow().clone()
    }

    fn get_editable(&self) -> Option<EditableItem> {
        None
    }

    fn find_closest(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        explorer_not_defined("findClosest")
    }

    fn set_to_copy(&self, items: Vec<Rc<ExplorerItem>>, _cut: bool) -> Promise<()> {
        self.clipboard_service.write_resources(items.iter().map(|item| item.resource()).collect())
    }

    fn apply_bulk_edit(&self, _edit: Vec<ResourceFileEdit>, _options: ExplorerBulkEditOptions) -> Promise<()> {
        Promise::rejected(self.error.clone())
    }

    fn select(&self, _resource: &URI, _reveal: Option<ExplorerReveal>) -> Promise<()> {
        explorer_not_defined("select")
    }
}

// Files - Paste

#[test]
fn paste_cut_reports_an_existing_destination_without_claiming_the_source_was_deleted_206471() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();

    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
        "explorer",
        Dynamic::object([("incrementalNaming", Dynamic::string("simple"))]),
    )])));
    // workbenchInstantiationService({ configurationService: () => configurationService }, disposables)
    let instantiation_service = TestInstantiationService::new();
    let file_service = disposables.add_rc(TestFileService::new());
    let file_service_contract: Rc<dyn IFileService> = file_service.clone();
    let configuration_contract: Rc<dyn IConfigurationService> = configuration_service.clone();
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract.clone(),)));
    let uri_identity_contract: Rc<dyn IUriIdentityService> = uri_identity_service.clone();
    instantiation_service.stub(I_FILE_SERVICE, file_service_contract.clone());
    instantiation_service.stub(I_CONFIGURATION_SERVICE, configuration_contract.clone());
    instantiation_service.stub(I_URI_IDENTITY_SERVICE, uri_identity_contract);
    instantiation_service.stub(I_EDITOR_SERVICE, Rc::new(TestEditorService));
    instantiation_service.stub(I_DIALOG_SERVICE, Rc::new(TestDialogService));

    let file_service = file_service_contract;
    let clipboard_service = Rc::new(TestClipboardService::new());
    instantiation_service.stub(I_CLIPBOARD_SERVICE, clipboard_service.clone());
    let notification = Rc::new(SpyNotificationService::new());
    instantiation_service.stub(I_NOTIFICATION_SERVICE, notification.clone());

    let files_configuration_service: Rc<dyn je_workbench::services::files_configuration::files_configuration_service::IFilesConfigurationService> =
        Rc::new(NullFilesConfigurationService);
    let source = ExplorerItem::new(
        uri_file("/workspace/some-file"),
        file_service.clone(),
        configuration_contract.clone(),
        files_configuration_service.clone(),
        None,
        Some(false),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    let destination = ExplorerItem::new(
        uri_file("/workspace/some-folder"),
        file_service,
        configuration_contract,
        files_configuration_service,
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        false,
    );
    let error = FileOperationError::new("The target already exists at the destination.", FileOperationResult::FileMoveConflict);
    let explorer_service = Rc::new(StubExplorerService {
        context: RefCell::new(vec![source]),
        clipboard_service: clipboard_service.clone(),
        error: error.clone().into_error(),
    });
    instantiation_service.stub(I_EXPLORER_SERVICE, explorer_service.clone());

    run_with_faked_timers(async move {
        instantiation_service.invoke_function(|accessor| cut_file_handler(accessor)).await.unwrap();
        *explorer_service.context.borrow_mut() = vec![destination];
        instantiation_service.invoke_function(|accessor| paste_file_handler(accessor, None)).await.unwrap();

        assert_eq!(notification.args(), vec![vec![error.message.clone()]]);
    });
}

// Files - Increment file name simple

#[test]
fn simple_increment_file_name_without_any_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy.js");
}

#[test]
fn simple_increment_file_name_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 2.js");
}

#[test]
fn simple_increment_file_name_with_suffix_version_with_leading_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 005.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 6.js");
}

#[test]
fn simple_increment_file_name_with_suffix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 9007199254740992.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 9007199254740992 copy.js");
}

#[test]
fn simple_increment_file_name_with_just_version_in_name() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "copy.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "copy copy.js");
}

#[test]
fn simple_increment_file_name_with_just_version_in_name_v2() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "copy 2.js";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "copy 2 copy.js");
}

#[test]
fn simple_increment_file_name_without_any_extension_or_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy");
}

#[test]
fn simple_increment_file_name_without_any_extension_or_version_trailing_dot() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy.");
}

#[test]
fn simple_increment_file_name_without_any_extension_or_version_leading_dot() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = ".test";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, ".test copy");
}

#[test]
fn simple_increment_file_name_without_any_extension_or_version_leading_dot_v2() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "..test";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, ". copy.test");
}

#[test]
fn simple_increment_file_name_without_any_extension_but_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 5";
    let result = increment_file_name(name, false, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 6");
}

#[test]
fn simple_increment_folder_name_without_any_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy");
}

#[test]
fn simple_increment_folder_name_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 2");
}

#[test]
fn simple_increment_folder_name_with_suffix_version_leading_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 005";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 6");
}

#[test]
fn simple_increment_folder_name_with_suffix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 9007199254740992";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 9007199254740992 copy");
}

#[test]
fn simple_increment_folder_name_with_just_version_in_name() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "copy";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "copy copy");
}

#[test]
fn simple_increment_folder_name_with_just_version_in_name_v2() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "copy 2";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "copy 2 copy");
}

#[test]
fn simple_increment_folder_name_with_extension_but_without_any_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.js";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test.js copy");
}

#[test]
fn simple_increment_folder_name_with_extension_and_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.js copy 5";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test.js copy 6");
}

#[test]
fn simple_increment_file_folder_name_with_suffix_version_special_case_1() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 0";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy");
}

#[test]
fn simple_increment_file_folder_name_with_suffix_version_special_case_2() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test copy 1";
    let result = increment_file_name(name, true, IncrementalNaming::Simple);
    assert_eq!(result, "test copy 2");
}

// Files - Increment file name smart

#[test]
fn smart_increment_file_name_without_any_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test.1.js");
}

#[test]
fn smart_increment_folder_name_without_any_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test.1");
}

#[test]
fn smart_increment_file_name_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.1.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test.2.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_trailing_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.001.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test.002.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_trailing_zeros_changing_length() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.009.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test.010.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_dash_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test-1.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test-2.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_dash_as_separator_trailing_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test-001.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test-002.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_dash_as_separator_trailing_zeros_changnig_length() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test-099.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test-100.js");
}

#[test]
fn smart_increment_file_name_with_suffix_version_with_underscore_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test_1.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test_2.js");
}

#[test]
fn smart_increment_folder_name_with_suffix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.1";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test.2");
}

#[test]
fn smart_increment_folder_name_with_suffix_version_trailing_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.001";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test.002");
}

#[test]
fn smart_increment_folder_name_with_suffix_version_with_dash_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test-1";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test-2");
}

#[test]
fn smart_increment_folder_name_with_suffix_version_with_underscore_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test_1";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test_2");
}

#[test]
fn smart_increment_file_name_with_suffix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.9007199254740992.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "test.9007199254740992.1.js");
}

#[test]
fn smart_increment_folder_name_with_suffix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "test.9007199254740992";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "test.9007199254740992.1");
}

#[test]
fn smart_increment_file_name_with_prefix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1.test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "2.test.js");
}

#[test]
fn smart_increment_file_name_with_just_version_in_name() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "2.js");
}

#[test]
fn smart_increment_file_name_with_just_version_in_name_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "9007199254740992.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "9007199254740992.1.js");
}

#[test]
fn smart_increment_file_name_with_prefix_version_trailing_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "001.test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "002.test.js");
}

#[test]
fn smart_increment_file_name_with_prefix_version_with_dash_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1-test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "2-test.js");
}

#[test]
fn smart_increment_file_name_with_prefix_version_with_underscore_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1_test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "2_test.js");
}

#[test]
fn smart_increment_file_name_with_prefix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "9007199254740992.test.js";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "9007199254740992.test.1.js");
}

#[test]
fn smart_increment_file_name_with_just_version_and_no_extension() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "001004";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "001005");
}

#[test]
fn smart_increment_file_name_with_just_version_and_no_extension_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "9007199254740992";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "9007199254740992.1");
}

#[test]
fn smart_increment_file_name_with_no_extension_and_no_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "file";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "file1");
}

#[test]
fn smart_increment_file_name_with_no_extension() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "file1";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "file2");
}

#[test]
fn smart_increment_file_name_with_no_extension_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "file9007199254740992";
    let result = increment_file_name(name, false, IncrementalNaming::Smart);
    assert_eq!(result, "file9007199254740992.1");
}

#[test]
fn smart_increment_folder_name_with_prefix_version() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1.test";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "2.test");
}

#[test]
fn smart_increment_folder_name_with_prefix_version_too_big_number() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "9007199254740992.test";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "9007199254740992.test.1");
}

#[test]
fn smart_increment_folder_name_with_prefix_version_trailing_zeros() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "001.test";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "002.test");
}

#[test]
fn smart_increment_folder_name_with_prefix_version_with_dash_as_separator() {
    let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let name = "1-test";
    let result = increment_file_name(name, true, IncrementalNaming::Smart);
    assert_eq!(result, "2-test");
}
