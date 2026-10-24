//! Porte de `src/vs/workbench/contrib/codeEditor/test/browser/largeFileOptimizations.test.ts` (suíte
//! `LargeFileOptimizationsWarner`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store.add` do original é
//!   `guard.add` ou `guard.add_rc`.
//! - **O laço de casos** (`for (const { lineCount, largeFileOptimizations, expectedNotifications } of [...])`) é a função
//!   `run_case`, chamada por um teste para cada linha da tabela, na mesma ordem.
//! - **O editor `mock<ICodeEditor>()`** (com `onDidChangeModel = Event.None` e `getModel()` devolvendo o modelo) é o
//!   `TestCodeEditor` do `je-editor/tests/support`, criado com o modelo: o `ICodeEditor` do porte é o `CodeEditorWidget`
//!   concreto, e o editor de teste nunca dispara `onDidChangeModel` depois de o aviso nascer.
//! - **`TestConfigurationService` com `updateValue` sobrescrito** para `setUserConfiguration` é o
//!   `stub_update_value(UpdateValueBehavior::SetsUserConfiguration)`.
//! - **`await notification.actions.primary[1].run()`** é a chamada de `run` seguida do esvaziamento das microtarefas
//!   (`runtime::run_microtasks`), que executa o `then` da promessa de `updateValue`.
//! - **`deepStrictEqual`** sobre objetos literais é a comparação de tuplas e vetores com `assert_eq!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

/// O suporte de editor do `je-editor/tests/support` que o `createTestCodeEditor` precisa, no mesmo módulo pai (os arquivos
/// se referem uns aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use editor_support::test_code_editor::{create_test_code_editor, TestCodeEditorInstantiationOptions};
use editor_support::test_text_model::create_text_model_with;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::to_disposable;
use je_base::linked_text::LinkedTextNode;
use je_base::runtime;
use je_base::JsString;
use je_editor::model::RelaxedTextModelCreationOptions;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::{TestConfigurationService, UpdateValueBehavior};
use je_platform::notification::INotificationService;
use je_platform::resources::uri_file;
use je_platform::storage::storage::InMemoryStorageService;
use je_workbench::common::notifications::INotificationsModel;
use je_workbench::contrib::code_editor::large_file_optimizations::LargeFileOptimizationsWarner;
use je_workbench::services::notification::common::notification_service::NotificationService;

const FILENAME: &str = "readme-[Open Empty Editor](command:workbench.action.files.newUntitledFile \"Open README\").txt";

fn run_case(line_count: usize, large_file_optimizations: bool, expected_notifications: usize) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let model = store.add_rc(create_text_model_with(
        "\n".repeat(line_count - 1),
        None,
        RelaxedTextModelCreationOptions { large_file_optimizations: Some(large_file_optimizations), ..Default::default() },
        Some(uri_file(&format!("/{FILENAME}"))),
    ));
    let notification_service = store.add_rc(NotificationService::new(store.add_rc(InMemoryStorageService::new())));
    {
        let notification_service = notification_service.clone();
        store.add(to_disposable(move || {
            for notification in notification_service.model.notifications() {
                notification.close();
            }
        }));
    }
    let configuration_service = TestConfigurationService::new(Some(Dynamic::object([(
        "editor",
        Dynamic::object([("largeFileOptimizations", Dynamic::from(large_file_optimizations))]),
    )])));
    configuration_service.stub_update_value(UpdateValueBehavior::SetsUserConfiguration);

    let editor = store.add_rc(create_test_code_editor(Some(model), TestCodeEditorInstantiationOptions::default()));
    let notifications: Rc<dyn INotificationService> = notification_service.clone();
    let configuration: Rc<dyn IConfigurationService> = configuration_service.clone();
    store.add_rc(LargeFileOptimizationsWarner::new(editor.widget(), notifications, configuration));

    assert_eq!(notification_service.model.notifications().len(), expected_notifications);
    if expected_notifications > 0 {
        let notification = notification_service.model.notifications()[0].clone();
        let message = notification.message();
        let actions = notification.actions();
        assert_eq!(
            (
                message.raw.starts_with(&format!("{FILENAME}: ")),
                message.linked_text.nodes.clone(),
                Some(actions.primary.iter().map(|action| action.label()).collect::<Vec<_>>()),
            ),
            (
                true,
                vec![LinkedTextNode::Text(JsString::from(message.raw.as_str()))],
                Some(vec!["Don't Show Again".to_string(), "Forcefully Enable Features".to_string()]),
            ),
        );

        let _ = actions.primary[1].run(&[]);
        runtime::run_microtasks();
        assert_eq!(
            (
                configuration_service.get_value(Some("editor.largeFileOptimizations"), None),
                notification_service.model.notifications().iter().map(|item| item.message().raw.clone()).collect::<Vec<_>>(),
            ),
            (Dynamic::from(false), vec!["Please reopen file in order for this setting to take effect.".to_string()]),
        );
    }
}

#[test]
fn lines_300000_optimizations_true() {
    run_case(300000, true, 0);
}

#[test]
fn lines_300001_optimizations_true() {
    run_case(300001, true, 1);
}

#[test]
fn lines_300001_optimizations_false() {
    run_case(300001, false, 0);
}
