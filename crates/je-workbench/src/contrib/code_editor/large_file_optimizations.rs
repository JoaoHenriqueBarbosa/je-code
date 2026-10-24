//! Porte de `src/vs/workbench/contrib/codeEditor/browser/largeFileOptimizations.ts`: o aviso de arquivo grande.
//!
//! Notas de tradução:
//! - **`extends Disposable`**: a struct guarda um [`Disposable`] por dentro, com o ouvinte de `onDidChangeModel`
//!   registrado. O ouvinte e o `Weak` do editor evitam o ciclo entre o editor e a contribuição.
//! - **`@INotificationService` e `@IConfigurationService`** são os serviços recebidos pelo construtor, e o
//!   `registerEditorContribution(...)` do fim do arquivo é [`register_large_file_optimizations_warner`], que os captura
//!   no construtor registrado (o `EditorContributionCtor` do porte não resolve serviços sozinho).
//! - **`nls.localize({ key, comment }, ...)`** é o `localize(key, texto, args)`; o `comment` não tem destino no porte.
//! - **`path.basename(model.uri.path)`** é o `basename` do `PathModule` posix sobre o caminho da URI.
//! - **`updateValue(...).then(ok, erro)`** é o `then_result`: o sucesso mostra o aviso de reabrir o arquivo e a falha
//!   vai para `error`, e a promessa resultante sempre resolve.

use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_editor::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use je_editor::editor_extensions::register_editor_contribution;
use je_platform::configuration::service::IConfigurationService;
use je_platform::notification::{
    BasePromptChoice, INotificationService, NeverShowAgainOptions, NotificationMessage, NotificationProperties, PromptChoice,
    PromptChoiceKind, PromptOptions, Severity,
};

/// A chave de configuração que desliga as otimizações de arquivo grande.
const LARGE_FILE_OPTIMIZATIONS_KEY: &str = "editor.largeFileOptimizations";

/// `LargeFileOptimizationsWarner`: mostra uma mensagem ao abrir um arquivo grande que teve a memória otimizada (e
/// recursos desligados).
pub struct LargeFileOptimizationsWarner {
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    notification_service: Rc<dyn INotificationService>,
    configuration_service: Rc<dyn IConfigurationService>,
}

impl LargeFileOptimizationsWarner {
    pub const ID: &'static str = "editor.contrib.largeFileOptimizationsWarner";

    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        notification_service: Rc<dyn INotificationService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            editor: Rc::downgrade(editor),
            notification_service,
            configuration_service,
        });

        let weak = Rc::downgrade(&this);
        this.base.register(editor.on_did_change_model().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update();
            }
        }));
        this.update();
        this
    }

    fn update(&self) {
        let Some(editor) = self.editor.upgrade() else { return };
        let Some(model) = editor.get_model() else {
            return;
        };

        if model.is_too_large_for_tokenization() {
            let file_name = PathModule::posix(&ProcessEnvironment).basename(&model.uri().path().to_string_lossy(), None);
            let message = localize(
                "largeFile",
                "{0}: tokenization, wrapping, folding, codelens, word highlighting and sticky scroll have been turned off for this large file in order to reduce memory usage and avoid freezing or crashing.",
                &[file_name.as_str()],
            );

            let (notification_service, configuration_service) = (self.notification_service.clone(), self.configuration_service.clone());
            let remove_optimizations = PromptChoiceKind::Plain(PromptChoice {
                base: BasePromptChoice {
                    telemetry_id: None,
                    label: localize("removeOptimizations", "Forcefully Enable Features", &[]),
                    keep_open: false,
                    run: Rc::new(move || {
                        let notification_service = notification_service.clone();
                        configuration_service.update_value(LARGE_FILE_OPTIMIZATIONS_KEY, Dynamic::from(false), None, None).then_result(
                            move |result| {
                                match result {
                                    Ok(()) => notification_service.info(vec![NotificationMessage::Text(localize(
                                        "reopenFilePrompt",
                                        "Please reopen file in order for this setting to take effect.",
                                        &[],
                                    ))]),
                                    Err(error) => notification_service.error(vec![NotificationMessage::Error(error)]),
                                }
                                Ok(())
                            },
                        );
                    }),
                },
                is_secondary: false,
            });

            self.notification_service.prompt(
                Severity::Info,
                NotificationMessage::Text(message),
                vec![remove_optimizations],
                Some(PromptOptions {
                    properties: NotificationProperties {
                        never_show_again: Some(NeverShowAgainOptions { id: Self::ID.to_string(), is_secondary: false, scope: None }),
                        ..NotificationProperties::default()
                    },
                    on_cancel: None,
                }),
            );
        }
    }
}

impl IDisposable for LargeFileOptimizationsWarner {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for LargeFileOptimizationsWarner {}

/// `registerEditorContribution(LargeFileOptimizationsWarner.ID, LargeFileOptimizationsWarner,
/// EditorContributionInstantiation.AfterFirstRender)`: o registro leva os serviços que o construtor injeta.
pub fn register_large_file_optimizations_warner(
    notification_service: Rc<dyn INotificationService>,
    configuration_service: Rc<dyn IConfigurationService>,
) {
    register_editor_contribution(
        LargeFileOptimizationsWarner::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            LargeFileOptimizationsWarner::new(editor, notification_service.clone(), configuration_service.clone())
        }),
        EditorContributionInstantiation::AfterFirstRender,
    );
}
