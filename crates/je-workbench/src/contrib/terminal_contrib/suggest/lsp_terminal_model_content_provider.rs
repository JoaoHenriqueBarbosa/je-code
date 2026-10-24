//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/lspTerminalModelContentProvider.ts`: o provedor do
//! conteúdo do documento virtual que o terminal expõe ao servidor de linguagem (LSP): o que o usuário executou com sucesso e
//! o que está digitando no prompt.
//!
//! Decisões de tradução:
//! - As dependências injetadas (`ITextModelService`, `IModelService` e `ILanguageService`) são parâmetros do
//!   [`LspTerminalModelContentProvider::new`], que devolve um `Rc` porque o provedor se registra no `ITextModelService`.
//! - `ITextModel` é o `TextModel` do editor (o `IModelService` do porte devolve o modelo concreto), e `model.getValue()` e
//!   `model.setValue(text)` são `get_value` e `set_value`. O texto do modelo é UTF-16 ([`JsString`]), e `lastIndexOf` e
//!   `substring` contam unidades UTF-16.
//! - `MutableDisposable` do `_onCommandFinishedListener` é o [`MutableDisposable`] do `je_base::lifecycle`.
//! - `async provideTextContent` é o `MaybePromise` do contrato [`ITextModelContentProvider`], já resolvido.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable};
use je_base::network::schemas;
use je_base::promise::MaybePromise;
use je_base::uri::{URI, UriComponents};
use je_base::js_string::{self, JsString};
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_platform::terminal::capabilities::capabilities::{CommandDetection, ICommandDetectionCapability, ITerminalCapabilityStore};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;
use je_platform::terminal::terminal::ShellType;

use super::lsp_terminal_util::VSCODE_LSP_TERMINAL_PROMPT_TRACKER;
use crate::services::textmodel_resolver::resolver_service::{ITextModelContentProvider, ITextModelService};

/// `ILspTerminalModelContentProvider` (o `dispose(): void` do original é o do [`IDisposable`]).
pub trait ILspTerminalModelContentProvider: ITextModelContentProvider + IDisposable {
    /// Sets or updates content for a terminal virtual document.
    /// This is when user has executed succesful command in terminal.
    /// Transfer the content to virtual document, and relocate delimiter to get terminal prompt ready for next prompt.
    fn set_content(&self, content: &str);
}

/// `LspTerminalModelContentProvider`.
pub struct LspTerminalModelContentProvider {
    base: Rc<Disposable>,
    command_detection: RefCell<Option<Rc<dyn ICommandDetectionCapability>>>,
    capabilities_store: Rc<dyn ITerminalCapabilityStore>,
    virtual_terminal_document_uri: URI,
    shell_type: RefCell<Option<ShellType>>,
    on_command_finished_listener: Rc<MutableDisposable<dyn IDisposable>>,
    model_service: Rc<dyn IModelService>,
    language_service: Rc<dyn ILanguageService>,
}

impl LspTerminalModelContentProvider {
    pub const SCHEME: &'static str = schemas::VSCODE_TERMINAL;

    /// `constructor(capabilityStore, terminalId, virtualTerminalDocument, shellType, ...)`: o `terminalId` não é lido pela classe.
    pub fn new(
        capability_store: Rc<dyn ITerminalCapabilityStore>,
        _terminal_id: i32,
        virtual_terminal_document: URI,
        shell_type: Option<ShellType>,
        text_model_service: Rc<dyn ITextModelService>,
        model_service: Rc<dyn IModelService>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_command_finished_listener = MutableDisposable::<dyn IDisposable>::new();
        base.register(on_command_finished_listener.clone());
        let command_detection = capability_store.get::<CommandDetection>();
        let this = Rc::new(Self {
            base,
            command_detection: RefCell::new(command_detection),
            capabilities_store: capability_store,
            virtual_terminal_document_uri: virtual_terminal_document,
            shell_type: RefCell::new(shell_type),
            on_command_finished_listener,
            model_service,
            language_service,
        });
        let provider: Rc<dyn ITextModelContentProvider> = this.clone();
        this.base.register(text_model_service.register_text_model_content_provider(Self::SCHEME, provider));
        this.register_terminal_command_finished_listener();
        this
    }

    // Listens to onDidChangeShellType event from `terminal.suggest.contribution.ts`
    pub fn shell_type_changed(&self, shell_type: Option<ShellType>) {
        *self.shell_type.borrow_mut() = shell_type;
    }

    /// Real-time conversion of terminal input to virtual document happens here.
    /// This is when user types in terminal, and we want to track the input.
    /// We want to track the input and update the virtual document.
    /// Note: This is for non-executed command.
    pub fn track_prompt_input_to_virtual_file(&self, content: &str) {
        *self.command_detection.borrow_mut() = self.capabilities_store.get::<CommandDetection>();
        let model = self.model_service.get_model(&self.virtual_terminal_document_uri);
        if self.shell_type.borrow().is_some()
            && let Some(model) = model
        {
            // Keep content only up to delimiter
            let sanitized_existing_content = Self::content_before_delimiter(&model.get_value());

            // Combine base content with new content
            let new_content = format!("{}{VSCODE_LSP_TERMINAL_PROMPT_TRACKER}{content}", sanitized_existing_content.to_string_lossy());
            model.set_value(TextSource::from(new_content.as_str()));
        }
    }

    /// `delimiterIndex !== -1 ? existingContent.substring(0, delimiterIndex) : existingContent`.
    fn content_before_delimiter(existing_content: &JsString) -> JsString {
        let delimiter = JsString::from(VSCODE_LSP_TERMINAL_PROMPT_TRACKER);
        match js_string::last_index_of(existing_content.units(), delimiter.units(), usize::MAX) {
            Some(delimiter_index) => JsString::from(js_string::substring(existing_content.units(), 0, delimiter_index as i64)),
            None => existing_content.clone(),
        }
    }

    fn attach_listener(this: &Rc<Self>) {
        if this.on_command_finished_listener.value().is_some() {
            return;
        }

        // Inconsistent repro: Covering case where commandDetection is available but onCommandFinished becomes available later
        let command_detection = this.command_detection.borrow().clone();
        if let Some(command_detection) = command_detection {
            let weak: Weak<Self> = Rc::downgrade(this);
            let listener = command_detection.on_command_finished().listen(move |command| {
                let Some(this) = weak.upgrade() else { return };
                if command.exit_code() == Some(0.0) && this.shell_type.borrow().is_some() {
                    this.set_content(&command.command());
                }
            });
            this.base.register(listener.clone());
            this.on_command_finished_listener.set(Some(listener));
        }
    }

    fn register_terminal_command_finished_listener(self: &Rc<Self>) {
        Self::attach_listener(self);

        // Listen to onDidAddCapabilityType because command detection is not available until later
        let weak: Weak<Self> = Rc::downgrade(self);
        let subscription = self.capabilities_store.on_did_add_command_detection_capability().listen(move |command_detection| {
            let Some(this) = weak.upgrade() else { return };
            *this.command_detection.borrow_mut() = Some(command_detection.clone());
            Self::attach_listener(&this);
        });
        self.base.register(subscription);
    }
}

impl ILspTerminalModelContentProvider for LspTerminalModelContentProvider {
    fn set_content(&self, content: &str) {
        let model = self.model_service.get_model(&self.virtual_terminal_document_uri);
        if self.shell_type.borrow().is_some()
            && let Some(model) = model
        {
            let existing_content = model.get_value();
            if existing_content.is_empty() {
                model.set_value(TextSource::from(VSCODE_LSP_TERMINAL_PROMPT_TRACKER));
            } else {
                // If we are appending to existing content, remove delimiter, attach new content, and re-add delimiter
                let sanitized_existing_content = Self::content_before_delimiter(&existing_content);
                let new_content = format!("{}\n{content}\n{VSCODE_LSP_TERMINAL_PROMPT_TRACKER}", sanitized_existing_content.to_string_lossy());
                model.set_value(TextSource::from(new_content.as_str()));
            }
        }
    }
}

impl ITextModelContentProvider for LspTerminalModelContentProvider {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        if let Some(existing) = self.model_service.get_model(resource)
            && !existing.is_disposed()
        {
            return MaybePromise::Value(Some(existing));
        }

        let language_id = self.language_service.guess_language_id_by_filepath_or_first_line(Some(resource), None);

        let language_selection = match &language_id {
            Some(language_id) => self.language_service.create_by_id(Some(language_id)),
            None => self.language_service.create_by_id(Some("plaintext")),
        };

        MaybePromise::Value(Some(self.model_service.create_model(TextSource::from(""), Some(language_selection.as_ref()), Some(resource), false)))
    }
}

impl IDisposable for LspTerminalModelContentProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// Creates a terminal language virtual URI.
///
/// Nota do original: o caminho ainda não é genérico por sistema operacional (issue 249477 do VS Code).
pub fn create_terminal_language_virtual_uri(terminal_id: i32, language_extension: &str) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::VSCODE_TERMINAL)),
            authority: None,
            path: Some(JsString::from(format!("/terminal{terminal_id}.{language_extension}").as_str())),
            query: None,
            fragment: None,
        },
        false,
    )
}
