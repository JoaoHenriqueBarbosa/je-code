//! Porte de `src/vs/workbench/api/common/extHostLanguages.ts`: o lado da extensão de `vscode.languages` que não é provedor:
//! `getLanguages`, `changeLanguage` (`setTextDocumentLanguage`), `tokenAtPosition`, `computeFullSyntaxHighlighting`,
//! `onDidChangeSyntaxHighlighting` e `createLanguageStatusItem`, sobre o proxy `MainThreadLanguages`.
//!
//! Divergências:
//! - `vscode.LanguageStatusItem` (um literal com getters e setters que fecham sobre `data`) é o [`LanguageStatusItem`], com métodos
//!   `set_*` e leitores; `text2` (proposta `languageStatusText`) não existe (é o mesmo `text`, atrás de uma proposta que o host em
//!   Rust não confere por falta de identidade da extensão chamadora).
//! - `IURITransformer | undefined` não existe (os dois lados enxergam as mesmas URIs); `console.warn` é `eprintln!`.
//! - `disposableTimeout(fn, 0)` é `set_timeout` com `clear_timeout` no descarte; o `commandDisposables` é o `DisposableStore`.
//! - `tokenAtPosition` recebe o [`TextDocument`] e a [`Position`] da API e devolve [`TokenInformation`] (`{ type, range }`); o
//!   `TokenType.to` do original é o `StandardTokenType::from_value` (os valores coincidem).
//! - `vscode.Command` de `command` é o [`Command`] de `ext_host_types` e `vscode.AccessibilityInformation` é o
//!   [`IAccessibilityInformation`] da plataforma.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::errors::{on_unexpected_error, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::{clear_timeout, set_timeout, TimerId};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_platform::accessibility::IAccessibilityInformation;

use super::ext_host_document_data::TextDocument;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_protocol::{ExtHostLanguagesShape, ILanguageStatus, IMainContext, ISyntaxHighlightingResultDto, MainContext, MainThreadLanguagesShape};
use super::ext_host_type_converters::geometry::{Position as PositionConverter, Range as RangeConverter};
use super::ext_host_type_converters::language_selector::{DocumentSelectorInput, LanguageSelector};
use super::ext_host_type_converters::ICommandsConverter;
use super::ext_host_types::{Command, LanguageStatusSeverity, Position, Range, StandardTokenType};

/// `vscode.TokenInformation`.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenInformation {
    pub token_type: StandardTokenType,
    pub range: Range,
}

/// `ExtHostLanguages`.
pub struct ExtHostLanguages {
    proxy: Rc<dyn MainThreadLanguagesShape>,
    language_ids: RefCell<Vec<String>>,
    on_did_change_syntax_highlighting: Rc<Emitter<()>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<dyn ICommandsConverter>,
    handle_pool: Cell<u32>,
    ids: Rc<RefCell<HashSet<String>>>,
}

impl ExtHostLanguages {
    pub fn new(main_context: &dyn IMainContext, documents: Rc<ExtHostDocuments>, commands: Rc<dyn ICommandsConverter>) -> Rc<Self> {
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_LANGUAGES),
            language_ids: RefCell::new(Vec::new()),
            on_did_change_syntax_highlighting: Emitter::new(),
            documents,
            commands,
            handle_pool: Cell::new(0),
            ids: Rc::new(RefCell::new(HashSet::new())),
        })
    }

    /// `onDidChangeSyntaxHighlighting`.
    pub fn on_did_change_syntax_highlighting(&self) -> Event<()> {
        self.on_did_change_syntax_highlighting.event()
    }

    pub fn compute_full_syntax_highlighting(&self, source: &str, language_id: &str) -> Promise<ISyntaxHighlightingResultDto> {
        self.proxy.rpc_compute_full_syntax_highlighting(source, language_id)
    }

    /// `getLanguages()`.
    pub fn get_languages(&self) -> Vec<String> {
        self.language_ids.borrow().clone()
    }

    /// `changeLanguage(uri, languageId)`: o documento, depois que o workbench trocou a linguagem.
    pub fn change_language(&self, uri: &URI, language_id: &str) -> Promise<TextDocument> {
        let (documents, uri) = (self.documents.clone(), uri.clone());
        self.proxy.rpc_change_language(uri.clone(), language_id).then_result(move |outcome| {
            outcome?;
            match documents.get_document_data(&uri) {
                Some(data) => Ok(data.document()),
                None => Err(Error::new(format!("document '{}' NOT found", uri.to_string(false)))),
            }
        })
    }

    /// `tokenAtPosition(document, position)`.
    pub fn token_at_position(&self, document: &TextDocument, position: &Position) -> Promise<TokenInformation> {
        let version_now = document.version();
        let pos = PositionConverter::from(position);
        let (document, position) = (document.clone(), position.clone());
        self.proxy.rpc_tokens_at_position(document.uri().clone(), pos).then_result(move |outcome| {
            let default_range = || TokenInformation {
                token_type: StandardTokenType::Other,
                range: document
                    .get_word_range_at_position(&position, None)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| Range::new(position.clone(), position.clone())),
            };
            let Some(info) = outcome? else {
                // no result
                return Ok(default_range());
            };
            let range = RangeConverter::to(&info.range)?;
            let token_type = StandardTokenType::from_value(info.token_type as i32).unwrap_or(StandardTokenType::Other);
            if !range.contains_position(&position) {
                // bogous result
                return Ok(default_range());
            }
            if version_now != document.version() {
                // concurrent change
                return Ok(default_range());
            }
            Ok(TokenInformation { token_type, range })
        })
    }

    /// `createLanguageStatusItem(extension, id, selector)`.
    pub fn create_language_status_item(&self, extension: &ExtensionDescription, id: &str, selector: DocumentSelectorInput) -> Result<Rc<LanguageStatusItem>, Error> {
        let handle = self.handle_pool.get();
        self.handle_pool.set(handle + 1);

        // enforce extension unique identifier
        let fully_qualified_id = format!("{}/{id}", extension.identifier.value);
        if !self.ids.borrow_mut().insert(fully_qualified_id.clone()) {
            return Err(Error::new(format!("LanguageStatusItem with id '{id}' ALREADY exists")));
        }

        let extension_label = extension.manifest.display_name.clone().unwrap_or_else(|| extension.manifest.name.clone());
        let item = Rc::new_cyclic(|this| LanguageStatusItem {
            this: this.clone(),
            proxy: self.proxy.clone(),
            commands: self.commands.clone(),
            ids: self.ids.clone(),
            handle,
            fully_qualified_id,
            extension_label: extension_label.clone(),
            extension_id: extension.identifier.value.clone(),
            data: RefCell::new(LanguageStatusData {
                selector,
                id: id.to_owned(),
                name: Some(extension_label),
                severity: LanguageStatusSeverity::Information,
                command: None,
                text: String::new(),
                detail: Some(String::new()),
                accessibility_information: None,
                busy: false,
            }),
            soon_handle: Cell::new(None),
            command_disposables: DisposableStore::new(),
        });
        item.update_async();
        Ok(item)
    }
}

impl ExtHostLanguagesShape for ExtHostLanguages {
    fn rpc_accept_language_ids(&self, ids: Vec<String>) {
        *self.language_ids.borrow_mut() = ids;
    }

    fn rpc_accept_syntax_highlighting_theme_changed(&self) {
        self.on_did_change_syntax_highlighting.fire(());
    }
}

/// O `data` do item (o `Omit<vscode.LanguageStatusItem, 'dispose' | 'text2'>` do original).
struct LanguageStatusData {
    selector: DocumentSelectorInput,
    id: String,
    name: Option<String>,
    severity: LanguageStatusSeverity,
    command: Option<Command>,
    text: String,
    detail: Option<String>,
    accessibility_information: Option<IAccessibilityInformation>,
    busy: bool,
}

/// `vscode.LanguageStatusItem`.
pub struct LanguageStatusItem {
    this: Weak<LanguageStatusItem>,
    proxy: Rc<dyn MainThreadLanguagesShape>,
    commands: Rc<dyn ICommandsConverter>,
    ids: Rc<RefCell<HashSet<String>>>,
    handle: u32,
    fully_qualified_id: String,
    extension_label: String,
    extension_id: String,
    data: RefCell<LanguageStatusData>,
    soon_handle: Cell<Option<TimerId>>,
    command_disposables: Rc<DisposableStore>,
}

impl LanguageStatusItem {
    pub fn id(&self) -> String {
        self.data.borrow().id.clone()
    }

    pub fn name(&self) -> Option<String> {
        self.data.borrow().name.clone()
    }

    pub fn set_name(&self, value: Option<String>) {
        self.data.borrow_mut().name = value;
        self.update_async();
    }

    pub fn selector(&self) -> DocumentSelectorInput {
        self.data.borrow().selector.clone()
    }

    pub fn set_selector(&self, value: DocumentSelectorInput) {
        self.data.borrow_mut().selector = value;
        self.update_async();
    }

    pub fn text(&self) -> String {
        self.data.borrow().text.clone()
    }

    pub fn set_text(&self, value: &str) {
        self.data.borrow_mut().text = value.to_owned();
        self.update_async();
    }

    pub fn detail(&self) -> Option<String> {
        self.data.borrow().detail.clone()
    }

    pub fn set_detail(&self, value: Option<String>) {
        self.data.borrow_mut().detail = value;
        self.update_async();
    }

    pub fn severity(&self) -> LanguageStatusSeverity {
        self.data.borrow().severity
    }

    pub fn set_severity(&self, value: LanguageStatusSeverity) {
        self.data.borrow_mut().severity = value;
        self.update_async();
    }

    pub fn accessibility_information(&self) -> Option<IAccessibilityInformation> {
        self.data.borrow().accessibility_information.clone()
    }

    pub fn set_accessibility_information(&self, value: Option<IAccessibilityInformation>) {
        self.data.borrow_mut().accessibility_information = value;
        self.update_async();
    }

    pub fn command(&self) -> Option<Command> {
        self.data.borrow().command.clone()
    }

    pub fn set_command(&self, value: Option<Command>) {
        self.data.borrow_mut().command = value;
        self.update_async();
    }

    pub fn busy(&self) -> bool {
        self.data.borrow().busy
    }

    pub fn set_busy(&self, value: bool) {
        self.data.borrow_mut().busy = value;
        self.update_async();
    }

    fn update_async(&self) {
        if let Some(previous) = self.soon_handle.take() {
            clear_timeout(previous);
        }

        if !self.ids.borrow().contains(&self.fully_qualified_id) {
            eprintln!("LanguageStatusItem ({}) from {} has been disposed and CANNOT be updated anymore", self.data.borrow().id, self.extension_id);
            return; // disposed in the meantime
        }

        let this = self.this.clone();
        self.soon_handle.set(Some(set_timeout(
            move || {
                if let Some(this) = this.upgrade() {
                    this.soon_handle.set(None);
                    this.flush();
                }
            },
            0,
        )));
    }

    fn flush(&self) {
        self.command_disposables.clear();
        let data = self.data.borrow();
        let command = match data.command.as_ref().map(|command| self.commands.to_internal(Some(command), &self.command_disposables)) {
            Some(Ok(command)) => command,
            Some(Err(error)) => {
                on_unexpected_error(&error);
                None
            }
            None => None,
        };
        self.proxy.rpc_set_language_status(
            self.handle,
            ILanguageStatus {
                id: self.fully_qualified_id.clone(),
                name: data.name.clone().unwrap_or_else(|| self.extension_label.clone()),
                source: self.extension_label.clone(),
                selector: LanguageSelector::from(Some(&data.selector)),
                label: data.text.clone(),
                detail: data.detail.clone().unwrap_or_default(),
                severity: match data.severity {
                    LanguageStatusSeverity::Error => Severity::Error,
                    LanguageStatusSeverity::Warning => Severity::Warning,
                    LanguageStatusSeverity::Information => Severity::Info,
                },
                command,
                accessibility_info: data.accessibility_information.clone(),
                busy: data.busy,
            },
        );
    }
}

impl IDisposable for LanguageStatusItem {
    fn dispose(&self) {
        self.command_disposables.dispose();
        if let Some(previous) = self.soon_handle.take() {
            clear_timeout(previous);
        }
        self.proxy.rpc_remove_language_status(self.handle);
        self.ids.borrow_mut().remove(&self.fully_qualified_id);
    }
}
