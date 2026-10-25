//! Porte de `src/vs/workbench/contrib/format/browser/formatActionsMultiple.ts`: o [`DefaultFormatter`] (a configuração
//! `editor.defaultFormatter`, o seletor de formatador, o diálogo e a notificação que pedem a configuração e o item de status
//! de linguagem do conflito) e as ações "Format Document With..." e "Format Selection With...".
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são os campos de [`DefaultFormatterServices`], na ordem do original;
//!   [`DefaultFormatterServices::from_accessor`] os lê do `ServicesAccessor`. `IWorkbenchContribution` e o
//!   `registerWorkbenchContribution` são o contrato de `common/contributions.rs` (o construtor da contribuição é um fecho
//!   sobre o accessor).
//! - **Efeitos do módulo.** O original registra a contribuição, a configuração e as duas ações ao carregar o módulo; aqui o
//!   [`register`] faz os quatro registros, e quem monta o workbench (ou o teste) o chama.
//! - **Provedores.** `DocumentFormattingEditProvider | DocumentRangeFormattingEditProvider` é o [`FormattingEditProvider`], com
//!   uma implementação para cada trait de provedor. Os métodos genéricos `<T extends FormattingEditProvider>` recebem
//!   `Vec<Rc<T>>`. O `T | string` de `_analyzeFormatter` é o [`FormatterOrMessage`].
//! - **`IIndexedPick`.** O item com `index` é um `IQuickPickItem` cujo `id` carrega o índice como texto (o `index` do original);
//!   o item "Configure Default Formatter..." tem o id `configure` (o original o distingue por identidade de objeto). O separador
//!   é o `IQuickPickSeparator` vazio, e o `activeItem` é o `PickActiveItem::Item` do item padrão.
//! - **Extensões.** `getExtension(id)` do `IExtensionService` do porte devolve só o local e o nome de exibição, então a extensão
//!   configurada é procurada em `extensions()` (o `getExtension` do original é a busca nessa lista pelo identificador, sem
//!   diferenciar caixa). `toExtension` é o de `services/extensions`. `a.name.localeCompare(b.name)` é a comparação do nome em
//!   minúsculas (o porte não tem o `Intl.Collator`).
//! - **Configuração.** `enum`, `enumItemLabels` e `markdownEnumDescriptions` referenciam os três arrays estáticos, que o
//!   `_updateConfigValues` esvazia e preenche depois (o `length = 0` e o `push` do original sobre o mesmo array): são
//!   `ArrayRef` compartilhados por thread.
//! - **Serviços das ações.** `instaService.invokeFunction(formatDocumentWithProvider, ...)` lê os serviços do `accessor`; aqui
//!   o [`formatting_services`] monta o `FormattingServices` do `je-editor`. O accessor só vale durante a
//!   chamada, então as ações leem todos os serviços antes do primeiro `await`.
//! - **Status.** `CancellationTokenSource.dispose(true)` é `dispose_with(true)`. O `selector.pattern` é o `fsPath` do documento.
//!   O comando do status chama o `_pickAndPersistDefaultFormatter` e descarta a promessa (o `CommandHandler` devolve `Dynamic`).

use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{ArrayRef, Dynamic};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_base::JsString;
use je_editor::code_action::languages::Command as LanguageCommand;
use je_editor::code_action::platform::I_ACCESSIBILITY_SIGNAL_SERVICE;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::config::editor_configuration_schema::editor_configuration_base_node;
use je_editor::core_commands::{has_non_empty_selection, writable};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::editor_context_keys::{has_multiple_document_formatting_provider, has_multiple_document_selection_formatting_provider};
use je_editor::editor_extensions::{
    ActionLabel, ActionOptions, CommandOptions, EditorAction, EditorActionBase, EditorActionContextMenuOptions, ServicesAccessor,
    register_editor_action,
};
use je_editor::editor_worker::I_EDITOR_WORKER_SERVICE;
use je_editor::format::{
    DocumentFormatter, DocumentRangeFormatter, EditorOrModel, FormattingConflicts, FormattingKind, FormattingMode, FormattingServices,
    IFormattingEditProviderSelector, format_document_ranges_with_provider, format_document_with_provider,
    get_real_and_synthetic_document_formatters_ordered,
};
use je_editor::language_selector::{FilterPattern, LanguageFilter, LanguageSelector};
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::language_features_service::{
    DocumentFormattingEditProvider, DocumentRangeFormattingEditProvider, LanguageFeaturesService,
};
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::commands::commands::{Command as RegisteredCommand, commands_registry};
use je_platform::configuration::registry::{PropertySchema, configuration_registry, ConfigurationNode};
use je_platform::configuration::service::{IConfigurationService, I_CONFIGURATION_SERVICE, update_overrides_of};
use je_platform::configuration::ConfigurationOverrides;
use je_platform::contextkey::ContextKeyExpr;
use je_platform::dialogs::dialogs::{Confirmation, BaseDialogOptions, DialogDetail, IDialogService, I_DIALOG_SERVICE};
use je_platform::log::I_LOG_SERVICE;
use je_platform::notification::notification::{
    BasePromptChoice, INotificationService, I_NOTIFICATION_SERVICE, NotificationPriority, NotificationProperties, PromptChoice,
    PromptChoiceKind, PromptOptions, Severity,
};
use je_platform::quickinput::quick_input::{
    I_QUICK_INPUT_SERVICE, IPickOptions, IQuickInputService, IQuickItem, IQuickPickItem, IQuickPickSeparator, PickActiveItem,
    QuickPickInput, QuickPicks,
};
use je_platform::resources::fs_path;

use crate::api::common::editor_execute_commands::I_LANGUAGE_FEATURES_SERVICE;
use crate::common::contributions::{IWorkbenchContribution, workbench_contributions_registry};
use crate::services::editor::editor_service::{ActiveTextEditorControl, IEditorService, I_EDITOR_SERVICE};
use crate::services::extension_management::extension_management::{
    IWorkbenchExtensionEnablementService, I_WORKBENCH_EXTENSION_ENABLEMENT_SERVICE,
};
use crate::services::extensions::extensions::{IExtensionService, I_EXTENSION_SERVICE, to_extension};
use crate::services::language_status::language_status_service::{
    ILanguageStatus, ILanguageStatusService, I_LANGUAGE_STATUS_SERVICE, LanguageStatusLabel,
};
use crate::services::lifecycle::lifecycle::LifecyclePhase;

/// `IEditorCancellationTokens`: o identificador mora em `je_editor::keybinding_cancellation`, onde o serviço é definido.
pub use je_editor::keybinding_cancellation::I_EDITOR_CANCELLATION_TOKENS;

/// `DocumentFormattingEditProvider | DocumentRangeFormattingEditProvider` (`FormattingEditProvider`).
pub trait FormattingEditProvider: 'static {
    fn provider_extension_id(&self) -> Option<String>;
    fn provider_display_name(&self) -> Option<String>;
}

impl FormattingEditProvider for dyn DocumentFormattingEditProvider {
    fn provider_extension_id(&self) -> Option<String> {
        self.extension_id().map(|id| id.value.clone())
    }

    fn provider_display_name(&self) -> Option<String> {
        self.display_name().map(str::to_owned)
    }
}

impl FormattingEditProvider for dyn DocumentRangeFormattingEditProvider {
    fn provider_extension_id(&self) -> Option<String> {
        self.extension_id().map(|id| id.value.clone())
    }

    fn provider_display_name(&self) -> Option<String> {
        self.display_name().map(str::to_owned)
    }
}

/// O `T | string` de `_analyzeFormatter`: o formatador achado ou a mensagem que explica por que não há.
pub enum FormatterOrMessage<T: ?Sized> {
    Formatter(Rc<T>),
    Message(String),
}

impl<T: ?Sized> Clone for FormatterOrMessage<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Formatter(formatter) => Self::Formatter(Rc::clone(formatter)),
            Self::Message(message) => Self::Message(message.clone()),
        }
    }
}

thread_local! {
    static EXTENSION_IDS: ArrayRef = ArrayRef::new(Vec::new());
    static EXTENSION_ITEM_LABELS: ArrayRef = ArrayRef::new(Vec::new());
    static EXTENSION_DESCRIPTIONS: ArrayRef = ArrayRef::new(Vec::new());
}

/// O id do item "Configure Default Formatter..." do seletor.
const CONFIGURE_PICK_ID: &str = "configure";

/// Os serviços que o construtor de `DefaultFormatter` recebe.
#[derive(Clone)]
pub struct DefaultFormatterServices {
    pub extension_service: Rc<dyn IExtensionService>,
    pub extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    pub config_service: Rc<dyn IConfigurationService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub language_features_service: Rc<LanguageFeaturesService>,
    pub language_status_service: Rc<dyn ILanguageStatusService>,
    pub editor_service: Rc<dyn IEditorService>,
}

/// Os três serviços que `showFormatterPick` lê do accessor (`IQuickInputService`, `IConfigurationService`, `ILanguageService`).
#[derive(Clone)]
pub struct PickServices {
    pub quick_input_service: Rc<dyn IQuickInputService>,
    pub config_service: Rc<dyn IConfigurationService>,
    pub language_service: Rc<dyn ILanguageService>,
}

impl PickServices {
    pub fn from_accessor(accessor: &ServicesAccessor) -> Self {
        Self {
            quick_input_service: accessor.get(I_QUICK_INPUT_SERVICE),
            config_service: accessor.get(I_CONFIGURATION_SERVICE),
            language_service: accessor.get(I_LANGUAGE_SERVICE),
        }
    }
}

impl DefaultFormatterServices {
    /// Os serviços do seletor entre os decorados do construtor.
    fn picker(&self) -> PickServices {
        PickServices {
            quick_input_service: Rc::clone(&self.quick_input_service),
            config_service: Rc::clone(&self.config_service),
            language_service: Rc::clone(&self.language_service),
        }
    }

    /// Os serviços decorados do construtor, lidos do `accessor`.
    pub fn from_accessor(accessor: &ServicesAccessor) -> Self {
        Self {
            extension_service: accessor.get(I_EXTENSION_SERVICE),
            extension_enablement_service: accessor.get(I_WORKBENCH_EXTENSION_ENABLEMENT_SERVICE),
            config_service: accessor.get(I_CONFIGURATION_SERVICE),
            notification_service: accessor.get(I_NOTIFICATION_SERVICE),
            dialog_service: accessor.get(I_DIALOG_SERVICE),
            quick_input_service: accessor.get(I_QUICK_INPUT_SERVICE),
            language_service: accessor.get(I_LANGUAGE_SERVICE),
            language_features_service: accessor.get(I_LANGUAGE_FEATURES_SERVICE),
            language_status_service: accessor.get(I_LANGUAGE_STATUS_SERVICE),
            editor_service: accessor.get(I_EDITOR_SERVICE),
        }
    }
}

/// O descarte de uma promessa sem dono: uma rejeição vai para `onUnexpectedError`.
fn detach<T: Clone + 'static>(promise: Promise<T>) {
    let reported = async_fn(async move {
        promise.await?;
        Ok(())
    });
    let _ = reported.catch(|error| errors::on_unexpected_error(&error));
}

/// `{ resource: document.uri, overrideIdentifier: document.getLanguageId() }`.
fn document_overrides(document: &TextModel) -> ConfigurationOverrides {
    ConfigurationOverrides { override_identifier: Some(JsString::from(document.get_language_id())), resource: Some(document.uri()) }
}

/// `configService.getValue<string>(DefaultFormatter.configName, overrides)`: o texto, ou `None` quando o valor não é um texto.
fn configured_default_formatter(config_service: &dyn IConfigurationService, overrides: &ConfigurationOverrides) -> Option<String> {
    match config_service.get_value(Some(DefaultFormatter::CONFIG_NAME), Some(overrides)) {
        Dynamic::String(value) => Some(value.to_string_lossy()),
        _ => None,
    }
}

/// `languageService.getLanguageName(languageId) || languageId`.
fn language_name(language_service: &dyn ILanguageService, document: &TextModel) -> String {
    language_service
        .get_language_name(&document.get_language_id())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| document.get_language_id())
}

/// `DefaultFormatter._maybeQuotes(s)`.
fn maybe_quotes(text: &str) -> String {
    if text.chars().any(char::is_whitespace) { format!("'{text}'") } else { text.to_owned() }
}

/// O índice que o `id` de um item do seletor carrega.
fn pick_index(item: &IQuickPickItem) -> Option<usize> {
    item.base.id.as_deref().and_then(|id| id.parse::<usize>().ok())
}

/// `formatters.map(...)` dos itens do seletor, com o rótulo e a descrição que `label_of` e `description_of` montam.
fn indexed_picks<T: FormattingEditProvider + ?Sized>(
    formatters: &[Rc<T>],
    label_of: impl Fn(&T) -> String,
    description_of: impl Fn(&T) -> Option<String>,
) -> Vec<IQuickPickItem> {
    formatters
        .iter()
        .enumerate()
        .map(|(index, formatter)| IQuickPickItem {
            base: IQuickItem {
                id: Some(index.to_string()),
                label: JsString::from(label_of(formatter)),
                description: description_of(formatter).map(JsString::from),
                ..IQuickItem::default()
            },
            ..IQuickPickItem::default()
        })
        .collect()
}

/// O `quickInputService.pick(picks, { placeHolder: 'Select a default formatter for ...' })` seguido do
/// `updateValue` do formatador escolhido, que `_pickAndPersistDefaultFormatter` e o ramo "configure" de `showFormatterPick`
/// compartilham: o índice escolhido, quando o formatador dele tem `extensionId` e a configuração foi gravada.
fn pick_and_persist_among<T: FormattingEditProvider + ?Sized>(
    picker: &PickServices,
    formatters: Vec<Rc<T>>,
    picks: Vec<IQuickPickItem>,
    document: Rc<TextModel>,
) -> Promise<Option<usize>> {
    let services = picker.clone();
    async_fn(async move {
        let lang_name = language_name(&*services.language_service, &document);
        let options = IPickOptions {
            place_holder: Some(localize("select", "Select a default formatter for '{0}' files", &[&maybe_quotes(&lang_name)])),
            ..IPickOptions::default()
        };
        let pick = services
            .quick_input_service
            .pick(QuickPicks::Items(picks.into_iter().map(QuickPickInput::Item).collect()), Some(&options), None)
            .await?;
        let Some(index) = pick.as_ref().and_then(pick_index) else {
            return Ok(None);
        };
        let Some(extension_id) = formatters[index].provider_extension_id() else {
            return Ok(None);
        };
        let overrides = document_overrides(&document);
        detach(services.config_service.update_value(
            DefaultFormatter::CONFIG_NAME,
            Dynamic::string(&extension_id),
            Some(update_overrides_of(&overrides)),
            None,
        ));
        Ok(Some(index))
    })
}

/// `DefaultFormatter._pickAndPersistDefaultFormatter(formatter, document)`.
fn pick_and_persist_default_formatter<T: FormattingEditProvider + ?Sized>(
    services: &DefaultFormatterServices,
    formatters: Vec<Rc<T>>,
    document: Rc<TextModel>,
) -> Promise<Option<Rc<T>>> {
    let picks = indexed_picks(
        &formatters,
        |formatter| {
            formatter
                .provider_display_name()
                .filter(|name| !name.is_empty())
                .or_else(|| formatter.provider_extension_id())
                .unwrap_or_else(|| "?".to_owned())
        },
        |formatter| formatter.provider_extension_id(),
    );
    let services = services.clone();
    async_fn(async move {
        let picked = pick_and_persist_among(&services.picker(), formatters.clone(), picks, document).await?;
        Ok(picked.map(|index| Rc::clone(&formatters[index])))
    })
}

/// `DefaultFormatter._analyzeFormatter(kind, formatter, document)`.
fn analyze_formatter<T: FormattingEditProvider + ?Sized>(
    services: &DefaultFormatterServices,
    kind: FormattingKind,
    formatter: Vec<Rc<T>>,
    document: Rc<TextModel>,
) -> Promise<FormatterOrMessage<T>> {
    let services = services.clone();
    async_fn(async move {
        let default_formatter_id = configured_default_formatter(&*services.config_service, &document_overrides(&document));

        if let Some(default_formatter_id) = default_formatter_id.as_deref().filter(|id| !id.is_empty()) {
            // good -> formatter configured
            let default_formatter = formatter.iter().find(|candidate| {
                ExtensionIdentifier::equals(candidate.provider_extension_id().as_deref(), Some(default_formatter_id))
            });
            if let Some(default_formatter) = default_formatter {
                // formatter available
                return Ok(FormatterOrMessage::Formatter(Rc::clone(default_formatter)));
            }

            // bad -> formatter gone
            services.extension_service.when_installed_extensions_registered().await?;
            let extension = services
                .extension_service
                .extensions()
                .into_iter()
                .find(|extension| ExtensionIdentifier::equals(Some(&extension.identifier.value), Some(default_formatter_id)));
            if let Some(extension) = extension {
                if services.extension_enablement_service.is_enabled(&to_extension(&extension)) {
                    // formatter does not target this file
                    let lang_name = language_name(&*services.language_service, &document);
                    let extension_name = extension
                        .manifest
                        .display_name
                        .clone()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| extension.manifest.name.clone());
                    let detail = if kind == FormattingKind::File {
                        localize(
                            "miss.1",
                            "Extension '{0}' is configured as formatter but it cannot format '{1}'-files",
                            &[&extension_name, &lang_name],
                        )
                    } else {
                        localize(
                            "miss.2",
                            "Extension '{0}' is configured as formatter but it can only format '{1}'-files as a whole, not selections or parts of it.",
                            &[&extension_name, &lang_name],
                        )
                    };
                    return Ok(FormatterOrMessage::Message(detail));
                }
            }
        } else if formatter.len() == 1 {
            // ok -> nothing configured but only one formatter available
            return Ok(FormatterOrMessage::Formatter(Rc::clone(&formatter[0])));
        }

        let lang_name = language_name(&*services.language_service, &document);
        let message = match default_formatter_id.as_deref().filter(|id| !id.is_empty()) {
            None => localize(
                "config.needed",
                "There are multiple formatters for '{0}' files. One of them should be configured as default formatter.",
                &[&maybe_quotes(&lang_name)],
            ),
            Some(default_formatter_id) => localize(
                "config.bad",
                "Extension '{0}' is configured as formatter but not available. Select a different default formatter to continue.",
                &[default_formatter_id],
            ),
        };

        Ok(FormatterOrMessage::Message(message))
    })
}

/// `DefaultFormatter._selectFormatter(formatter, document, mode, kind)`.
fn select_formatter<T: FormattingEditProvider + ?Sized>(
    services: &DefaultFormatterServices,
    formatter: Vec<Rc<T>>,
    document: Rc<TextModel>,
    mode: FormattingMode,
    kind: FormattingKind,
) -> Promise<Option<Rc<T>>> {
    let services = services.clone();
    async_fn(async move {
        let message = match analyze_formatter(&services, kind, formatter.clone(), Rc::clone(&document)).await? {
            FormatterOrMessage::Formatter(formatter) => return Ok(Some(formatter)),
            FormatterOrMessage::Message(message) => message,
        };

        if mode != FormattingMode::Silent {
            // running from a user action -> show modal dialog so that users configure
            // a default formatter
            let result = services
                .dialog_service
                .confirm(Confirmation {
                    base: BaseDialogOptions {
                        detail: Some(DialogDetail::Text(message)),
                        ..BaseDialogOptions::new(localize("miss", "Configure Default Formatter", &[]))
                    },
                    // && denotes a mnemonic
                    primary_button: Some(localize("do.config", "&&Configure...", &[])),
                    cancel_button: None,
                })
                .await?;
            if result.confirmed {
                return pick_and_persist_default_formatter(&services, formatter, document).await;
            }
        } else {
            // no user action -> show a silent notification and proceed
            let (prompt_services, prompt_formatter, prompt_document) = (services.clone(), formatter, document);
            services.notification_service.prompt(
                Severity::Info,
                message.into(),
                vec![PromptChoiceKind::Plain(PromptChoice {
                    base: BasePromptChoice {
                        telemetry_id: None,
                        label: localize("do.config.notification", "Configure...", &[]),
                        keep_open: false,
                        run: Rc::new(move || {
                            detach(pick_and_persist_default_formatter(
                                &prompt_services,
                                prompt_formatter.clone(),
                                Rc::clone(&prompt_document),
                            ));
                        }),
                    },
                    is_secondary: false,
                })],
                Some(PromptOptions {
                    properties: NotificationProperties { priority: Some(NotificationPriority::Silent), ..NotificationProperties::default() },
                    on_cancel: None,
                }),
            );
        }
        Ok(None)
    })
}

/// O seletor que o `DefaultFormatter` instala em `FormattingConflicts.setFormatterSelector`.
struct DefaultFormatterSelector {
    services: DefaultFormatterServices,
}

impl IFormattingEditProviderSelector for DefaultFormatterSelector {
    fn select_document_formatter(
        &self,
        formatter: Vec<DocumentFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentFormatter>> {
        select_formatter(&self.services, formatter, Rc::clone(document), mode, kind)
    }

    fn select_document_range_formatter(
        &self,
        formatter: Vec<DocumentRangeFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentRangeFormatter>> {
        select_formatter(&self.services, formatter, Rc::clone(document), mode, kind)
    }
}

/// `DefaultFormatter`.
pub struct DefaultFormatter {
    this: Weak<DefaultFormatter>,
    base: Rc<Disposable>,
    language_status_store: Rc<DisposableStore>,
    services: DefaultFormatterServices,
}

impl IWorkbenchContribution for DefaultFormatter {}

impl IDisposable for DefaultFormatter {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl DefaultFormatter {
    /// `DefaultFormatter.configName`.
    pub const CONFIG_NAME: &'static str = "editor.defaultFormatter";

    /// `DefaultFormatter.extensionIds`.
    pub fn extension_ids() -> ArrayRef {
        EXTENSION_IDS.with(ArrayRef::clone)
    }

    /// `DefaultFormatter.extensionItemLabels`.
    pub fn extension_item_labels() -> ArrayRef {
        EXTENSION_ITEM_LABELS.with(ArrayRef::clone)
    }

    /// `DefaultFormatter.extensionDescriptions`.
    pub fn extension_descriptions() -> ArrayRef {
        EXTENSION_DESCRIPTIONS.with(ArrayRef::clone)
    }

    pub fn new(services: DefaultFormatterServices) -> Rc<Self> {
        let base = Disposable::new();
        let language_status_store = DisposableStore::new();
        base.register(language_status_store.clone());

        let this = Rc::new_cyclic(|this: &Weak<DefaultFormatter>| Self {
            this: this.clone(),
            base,
            language_status_store,
            services,
        });
        this.register_listeners();
        this.update_config_values();
        this
    }

    /// `this._store.add(event(listener))`, com o `this` seguro por `Weak` para o ouvinte não manter a classe viva.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&DefaultFormatter, &T) + 'static) {
        let weak = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    fn register_listeners(&self) {
        let services = &self.services;
        self.listen(services.extension_service.on_did_change_extensions(), |this, _| this.update_config_values());
        self.base.register(FormattingConflicts::set_formatter_selector(Rc::new(DefaultFormatterSelector { services: services.clone() })));
        self.listen(services.editor_service.on_did_active_editor_change(), |this, _| this.update_status());
        let features = &services.language_features_service;
        self.listen(features.document_formatting_edit_provider.on_did_change(), |this, _| this.update_status());
        self.listen(features.document_range_formatting_edit_provider.on_did_change(), |this, _| this.update_status());
        self.listen(features.document_formatting_edit_provider.on_did_change(), |this, _| this.update_config_values());
        self.listen(features.document_range_formatting_edit_provider.on_did_change(), |this, _| this.update_config_values());
        self.listen(services.config_service.on_did_change_configuration(), |this, event| {
            if event.affects_configuration(DefaultFormatter::CONFIG_NAME, None) {
                this.update_status();
            }
        });
    }

    /// `_updateConfigValues()`.
    fn update_config_values(&self) {
        let services = self.services.clone();
        detach(async_fn(async move {
            services.extension_service.when_installed_extensions_registered().await?;
            let mut extensions = services.extension_service.extensions();

            // Get all formatter providers to identify which extensions actually contribute formatters
            let features = &services.language_features_service;
            let mut formatter_extension_ids = std::collections::HashSet::<String>::new();
            for formatter in features.document_formatting_edit_provider.all_no_model() {
                if let Some(extension_id) = formatter.extension_id() {
                    formatter_extension_ids.insert(ExtensionIdentifier::to_key(&extension_id.value));
                }
            }
            for formatter in features.document_range_formatting_edit_provider.all_no_model() {
                if let Some(extension_id) = formatter.extension_id() {
                    formatter_extension_ids.insert(ExtensionIdentifier::to_key(&extension_id.value));
                }
            }

            extensions.sort_by(|a, b| {
                // Ultimate boost: extensions that actually contribute formatters
                let contributes_formatter_a = formatter_extension_ids.contains(&ExtensionIdentifier::to_key(&a.identifier.value));
                let contributes_formatter_b = formatter_extension_ids.contains(&ExtensionIdentifier::to_key(&b.identifier.value));

                if contributes_formatter_a && !contributes_formatter_b {
                    return std::cmp::Ordering::Less;
                } else if !contributes_formatter_a && contributes_formatter_b {
                    return std::cmp::Ordering::Greater;
                }

                // Secondary boost: category-based sorting
                let boost = |categories: &Option<Vec<String>>| {
                    categories.as_ref().is_some_and(|categories| {
                        categories.iter().any(|category| category == "Formatters" || category == "Programming Languages")
                    })
                };
                let boost_a = boost(&a.manifest.categories);
                let boost_b = boost(&b.manifest.categories);

                if boost_a && !boost_b {
                    std::cmp::Ordering::Less
                } else if !boost_a && boost_b {
                    std::cmp::Ordering::Greater
                } else {
                    a.manifest.name.to_lowercase().cmp(&b.manifest.name.to_lowercase())
                }
            });

            let (ids, labels, descriptions) = (Self::extension_ids(), Self::extension_item_labels(), Self::extension_descriptions());
            ids.clear();
            labels.clear();
            descriptions.clear();

            ids.push(Dynamic::Null);
            labels.push(Dynamic::string(&localize("null", "None", &[])));
            descriptions.push(Dynamic::string(&localize("nullFormatterDescription", "None", &[])));

            for extension in &extensions {
                let manifest = &extension.manifest;
                if manifest.main.as_deref().is_some_and(|main| !main.is_empty())
                    || manifest.browser.as_deref().is_some_and(|browser| !browser.is_empty())
                {
                    ids.push(Dynamic::string(&extension.identifier.value));
                    labels.push(Dynamic::string(manifest.display_name.as_deref().unwrap_or("")));
                    descriptions.push(Dynamic::string(manifest.description.as_deref().unwrap_or("")));
                }
            }
            Ok(())
        }));
    }

    // --- status item

    /// `_updateStatus()`.
    fn update_status(&self) {
        self.language_status_store.clear();

        // `getCodeEditor(activeTextEditorControl)`: o editor de código, ou o lado modificado de um editor de diff.
        let editor = match self.services.editor_service.active_text_editor_control() {
            Some(ActiveTextEditorControl::Code(editor)) => editor.code_editor_widget(),
            Some(ActiveTextEditorControl::Diff { modified, .. }) => modified.code_editor_widget(),
            None => None,
        };
        let Some(document) = editor.and_then(|editor| editor.get_model()) else {
            return;
        };

        let features = &self.services.language_features_service;
        let formatter = get_real_and_synthetic_document_formatters_ordered(
            &features.document_formatting_edit_provider,
            &features.document_range_formatting_edit_provider,
            &document,
        );

        if formatter.is_empty() {
            return;
        }

        let cts = CancellationTokenSource::new(None);
        let token = cts.token();
        self.language_status_store.add(to_disposable(move || cts.dispose_with(true)));

        let analyzed = analyze_formatter(&self.services, FormattingKind::File, formatter.clone(), Rc::clone(&document));
        let weak = self.this.clone();
        detach(async_fn(async move {
            let result = analyzed.await?;
            if token.is_cancellation_requested() {
                return Ok(());
            }
            let FormatterOrMessage::Message(detail) = result else {
                return Ok(());
            };
            if let Some(this) = weak.upgrade() {
                this.add_conflict_status(formatter, document, detail);
            }
            Ok(())
        }));
    }

    /// O corpo do `.then` de `_updateStatus`: o comando "Configure..." e o item de status do conflito.
    fn add_conflict_status(&self, formatter: Vec<DocumentFormatter>, document: Rc<TextModel>, detail: String) {
        let command = LanguageCommand::new(
            format!("formatter/configure/dfl/{}", generate_uuid()),
            localize("do.config.command", "Configure...", &[]),
        );
        let (services, command_formatter, command_document) = (self.services.clone(), formatter, Rc::clone(&document));
        self.language_status_store.add(commands_registry().register_command(RegisteredCommand {
            id: command.id.clone(),
            handler: Rc::new(move |_accessor, _args| {
                detach(pick_and_persist_default_formatter(&services, command_formatter.clone(), Rc::clone(&command_document)));
                Dynamic::Undefined
            }),
            metadata: None,
        }));
        self.language_status_store.add(self.services.language_status_service.add_status(ILanguageStatus {
            id: "formatter.conflict".to_owned(),
            name: localize("summary", "Formatter Conflicts", &[]),
            selector: LanguageSelector::Filter(LanguageFilter {
                language: Some(document.get_language_id()),
                pattern: Some(FilterPattern::Glob(fs_path(&document.uri()))),
                ..LanguageFilter::default()
            }),
            severity: Severity::Error,
            label: LanguageStatusLabel::Text(localize("formatter", "Formatting", &[])),
            detail,
            busy: false,
            source: String::new(),
            command: Some(command),
            accessibility_info: None,
        }));
    }
}

/// `showFormatterPick(accessor, model, formatters)`: o índice do formatador escolhido, ou `None` se o seletor foi dispensado ou
/// o usuário foi configurar o formatador padrão.
fn show_formatter_pick<T: FormattingEditProvider + ?Sized>(
    services: &PickServices,
    model: Rc<TextModel>,
    formatters: Vec<Rc<T>>,
) -> Promise<Option<usize>> {
    let overrides = document_overrides(&model);
    let default_formatter = configured_default_formatter(&*services.config_service, &overrides);

    let mut default_formatter_pick: Option<IQuickPickItem> = None;
    let picks = indexed_picks(
        &formatters,
        |provider| provider.provider_display_name().unwrap_or_default(),
        |provider| {
            ExtensionIdentifier::equals(provider.provider_extension_id().as_deref(), default_formatter.as_deref())
                .then(|| localize("def", "(default)", &[]))
        },
    );
    for (pick, provider) in picks.iter().zip(&formatters) {
        if ExtensionIdentifier::equals(provider.provider_extension_id().as_deref(), default_formatter.as_deref()) {
            // autofocus default pick
            default_formatter_pick = Some(pick.clone());
        }
    }

    let configure_pick = IQuickPickItem {
        base: IQuickItem {
            id: Some(CONFIGURE_PICK_ID.to_owned()),
            label: JsString::from(localize("config", "Configure Default Formatter...", &[])),
            ..IQuickItem::default()
        },
        ..IQuickPickItem::default()
    };

    let services = services.clone();
    async_fn(async move {
        let mut items: Vec<QuickPickInput> = picks.iter().cloned().map(QuickPickInput::Item).collect();
        items.push(QuickPickInput::Separator(IQuickPickSeparator::default()));
        items.push(QuickPickInput::Item(configure_pick));
        let options = IPickOptions {
            place_holder: Some(localize("format.placeHolder", "Select a formatter", &[])),
            active_item: default_formatter_pick.map(PickActiveItem::Item),
            ..IPickOptions::default()
        };
        let pick = services.quick_input_service.pick(QuickPicks::Items(items), Some(&options), None).await?;
        let Some(pick) = pick else {
            // dismissed
            return Ok(None);
        };

        if pick.base.id.as_deref() == Some(CONFIGURE_PICK_ID) {
            // config default
            pick_and_persist_among(&services, formatters, picks, model).await?;
            return Ok(None);
        }

        // picked one
        Ok(pick_index(&pick))
    })
}

/// `isFormatDocumentMultipleArgs(args)`: o `formatter` dos argumentos, quando é um texto.
fn format_document_multiple_args(args: &Dynamic) -> Option<String> {
    match args {
        Dynamic::Object(_) => match args.get_str("formatter") {
            Dynamic::String(formatter) => Some(formatter.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}

/// O que `instaService.invokeFunction(formatDocumentWithProvider | formatDocumentRangesWithProvider, ...)` lê do accessor.
pub fn formatting_services(accessor: &ServicesAccessor) -> FormattingServices {
    FormattingServices {
        worker_service: accessor.get(I_EDITOR_WORKER_SERVICE),
        language_features_service: (*accessor.get(I_LANGUAGE_FEATURES_SERVICE)).clone(),
        log_service: accessor.get(I_LOG_SERVICE),
        accessibility_signal_service: accessor.get(I_ACCESSIBILITY_SIGNAL_SERVICE),
        editor_cancellation_tokens: accessor.get(I_EDITOR_CANCELLATION_TOKENS),
    }
}

/// O `accessor` de uma ação, ou a rejeição de quem a chamou sem ele.
fn require_accessor(accessor: Option<&ServicesAccessor>) -> Result<&ServicesAccessor, Promise<()>> {
    accessor.ok_or_else(|| Promise::rejected(Error::new("A ação de formatação precisa do ServicesAccessor do comando")))
}

/// `FormatDocumentMultipleAction`.
pub struct FormatDocumentMultipleAction {
    action: EditorActionBase,
}

impl FormatDocumentMultipleAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: ContextKeyExpr::and([Some(writable()), Some(has_multiple_document_formatting_provider().expr())]),
                    ..CommandOptions::new("editor.action.formatDocument.multiple")
                },
                context_menu_opts: vec![EditorActionContextMenuOptions {
                    group: "1_modification".to_owned(),
                    order: 1.3,
                    when: None,
                    menu_id: None,
                }],
                label: ActionLabel::Plain {
                    label: localize("formatDocument.label.multiple", "Format Document With...", &[]),
                    alias: "Format Document...".to_owned(),
                },
            }),
        }
    }
}

impl Default for FormatDocumentMultipleAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for FormatDocumentMultipleAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let accessor = match require_accessor(accessor) {
            Ok(accessor) => accessor,
            Err(rejected) => return rejected,
        };
        let Some(model) = editor.get_model() else {
            return Promise::resolved(());
        };
        let features = accessor.get(I_LANGUAGE_FEATURES_SERVICE);
        let providers = get_real_and_synthetic_document_formatters_ordered(
            &features.document_formatting_edit_provider,
            &features.document_range_formatting_edit_provider,
            &model,
        );
        let formatter_argument = format_document_multiple_args(args);
        // O seletor só é usado (e só lê os serviços dele) quando o argumento `formatter` não vem.
        let picker_services = formatter_argument.is_none().then(|| PickServices::from_accessor(accessor));
        let services = formatting_services(accessor);
        let editor = Rc::clone(editor);

        async_fn(async move {
            let provider = match formatter_argument {
                Some(formatter) => {
                    let found = providers
                        .iter()
                        .find(|provider| ExtensionIdentifier::equals(provider.extension_id().map(|id| id.value.as_str()), Some(&formatter)));
                    match found {
                        Some(provider) => Rc::clone(provider),
                        None => return Ok(()),
                    }
                }
                None => {
                    let picker_services = picker_services.expect("o seletor lê os serviços quando o argumento não vem");
                    match show_formatter_pick(&picker_services, Rc::clone(&model), providers.clone()).await? {
                        Some(pick) => Rc::clone(&providers[pick]),
                        None => return Ok(()),
                    }
                }
            };

            format_document_with_provider(&services, &provider, &EditorOrModel::Editor(editor), FormattingMode::Explicit, &CancellationToken::None, None)
                .await?;
            Ok(())
        })
    }
}

/// `FormatSelectionMultipleAction`.
pub struct FormatSelectionMultipleAction {
    action: EditorActionBase,
}

impl FormatSelectionMultipleAction {
    pub fn new() -> Self {
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: ContextKeyExpr::and([
                        ContextKeyExpr::and([Some(writable())]),
                        Some(has_multiple_document_selection_formatting_provider().expr()),
                    ]),
                    ..CommandOptions::new("editor.action.formatSelection.multiple")
                },
                context_menu_opts: vec![EditorActionContextMenuOptions {
                    group: "1_modification".to_owned(),
                    order: 1.31,
                    when: ContextKeyExpr::and([Some(has_non_empty_selection())]),
                    menu_id: None,
                }],
                label: ActionLabel::Plain {
                    label: localize("formatSelection.label.multiple", "Format Selection With...", &[]),
                    alias: "Format Code...".to_owned(),
                },
            }),
        }
    }
}

impl Default for FormatSelectionMultipleAction {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorAction for FormatSelectionMultipleAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, _args: &Dynamic) -> Promise<()> {
        let accessor = match require_accessor(accessor) {
            Ok(accessor) => accessor,
            Err(rejected) => return rejected,
        };
        let (Some(model), Some(selection)) = (editor.get_model(), editor.get_selection()) else {
            return Promise::resolved(());
        };
        let features = accessor.get(I_LANGUAGE_FEATURES_SERVICE);

        let mut range = Range::new(selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column);
        if selection.is_empty() {
            range = Range::new(selection.start_line_number, 1, selection.start_line_number, model.get_line_max_column(selection.start_line_number));
        }

        let provider = features.document_range_formatting_edit_provider.ordered(&model, false);
        let picker_services = PickServices::from_accessor(accessor);
        let services = formatting_services(accessor);
        let editor = Rc::clone(editor);

        async_fn(async move {
            if let Some(pick) = show_formatter_pick(&picker_services, model, provider.clone()).await? {
                format_document_ranges_with_provider(
                    &services,
                    &provider[pick],
                    &EditorOrModel::Editor(editor),
                    vec![range],
                    &CancellationToken::None,
                    true,
                )
                .await?;
            }
            Ok(())
        })
    }
}

/// Os efeitos do carregamento do módulo original: a contribuição `DefaultFormatter` (fase `Restored`), a configuração
/// `editor.defaultFormatter` e as duas ações de formatar com.
pub fn register() {
    workbench_contributions_registry().register_workbench_contribution(
        Rc::new(|accessor: &ServicesAccessor| {
            let contribution: Rc<dyn IWorkbenchContribution> = DefaultFormatter::new(DefaultFormatterServices::from_accessor(accessor));
            contribution
        }),
        LifecyclePhase::Restored,
    );

    let formatter_schema = PropertySchema::from_entries(vec![
        (
            "description",
            Dynamic::string(&localize(
                "formatter.default",
                "Defines a default formatter which takes precedence over all other formatter settings. Must be the identifier of an extension contributing a formatter.",
                &[],
            )),
        ),
        ("type", Dynamic::array([Dynamic::string("string"), Dynamic::Null])),
        ("default", Dynamic::Null),
        ("enum", Dynamic::Array(DefaultFormatter::extension_ids())),
        ("enumItemLabels", Dynamic::Array(DefaultFormatter::extension_item_labels())),
        ("markdownEnumDescriptions", Dynamic::Array(DefaultFormatter::extension_descriptions())),
    ]);
    configuration_registry().register_configuration(ConfigurationNode::new(
        editor_configuration_base_node().with_properties(vec![(DefaultFormatter::CONFIG_NAME.to_owned(), formatter_schema)]),
    ));

    register_editor_action(FormatDocumentMultipleAction::new);
    register_editor_action(FormatSelectionMultipleAction::new);
}
