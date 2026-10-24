//! Porte de `src/vs/workbench/services/configuration/common/configurationEditing.ts`: escreve um valor de
//! configuração no arquivo certo (usuário local, usuário remoto, workspace ou pasta), validando a chave, o alvo e o
//! escopo, e tratando os erros pelo usuário (notificações com ações).
//!
//! Decisões de tradução:
//! - **Serviços de edição de texto** (`ITextModelService`, `ITextFileService`, `INotificationService`,
//!   `IPreferencesService`, `IEditorService` e `IFilesConfigurationService.enableAutoSaveAfterShortDelay`) ainda não
//!   foram portados. O que o original lê deles é o trait [`ConfigurationEditingServices`], com os mesmos nomes de
//!   operação; o modelo de texto resolvido é o trait [`ConfigurationEditingModel`] (`getValue`, `getOptions`,
//!   `getEOL` e a aplicação de uma edição, que o original faz com `getPositionAt` + `pushEditOperations`).
//! - `ConfigurationEditingError` (um `ErrorNoTelemetry` com `code`) é um [`Error`] sem telemetria com a propriedade
//!   `code`; [`configuration_editing_error_code`] a lê.
//! - `queue.queue(async () => ...)` é a [`Queue`] do `je-base`; os `async` são [`Promise`].

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::{Queue, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json::{self, ParseOptions};
use je_base::json_edit::{EditSegment, set_property};
use je_base::json_formatter::{Edit, FormattingOptions};
use je_base::lifecycle::DisposableRef;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::registry::{ConfigurationScope, configuration_registry, is_override_property, key_from_override_identifiers};
use je_platform::configuration::{ConfigurationOverrides, ConfigurationUpdateOptions, ConfigurationUpdateOverrides, HandleDirtyFile};
use je_platform::files::i_file_service::IFileService;
use je_platform::files::files::{FileOperationError, FileOperationResult};
use je_platform::js::to_json;
use je_platform::resources::fs_path;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};

use super::configuration::{
    APPLICATION_SCOPES, FOLDER_SCOPES, FOLDER_SETTINGS_PATH, IWorkbenchConfigurationService, LAUNCH_CONFIGURATION_KEY, MCP_CONFIGURATION_KEY, TASKS_CONFIGURATION_KEY, TASKS_DEFAULT,
    USER_STANDALONE_CONFIGURATIONS, WORKSPACE_STANDALONE_CONFIGURATIONS,
};
use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

/// `ConfigurationEditingErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationEditingErrorCode {
    /// Error when trying to write a configuration key that is not registered.
    ErrorUnknownKey = 0,
    /// Error when trying to write an application setting into workspace settings.
    ErrorInvalidWorkspaceConfigurationApplication,
    /// Error when trying to write a machne setting into workspace settings.
    ErrorInvalidWorkspaceConfigurationMachine,
    /// Error when trying to write an invalid folder configuration key to folder settings.
    ErrorInvalidFolderConfiguration,
    /// Error when trying to write to user target but not supported for provided key.
    ErrorInvalidUserTarget,
    /// Error when trying to write to user target but not supported for provided key.
    ErrorInvalidWorkspaceTarget,
    /// Error when trying to write a configuration key to folder target.
    ErrorInvalidFolderTarget,
    /// Error when trying to write to language specific setting but not supported for preovided key.
    ErrorInvalidResourceLanguageConfiguration,
    /// Error when trying to write to the workspace configuration without having a workspace opened.
    ErrorNoWorkspaceOpened,
    /// Error when trying to write and save to the configuration file while it is dirty in the editor.
    ErrorConfigurationFileDirty,
    /// Error when trying to write and save to the configuration file while it is not the latest in the disk.
    ErrorConfigurationFileModifiedSince,
    /// Error when trying to write to a configuration file that contains JSON errors.
    ErrorInvalidConfiguration,
    /// Error when trying to write a policy configuration.
    ErrorPolicyConfiguration,
    /// Internal Error.
    ErrorInternal,
}

impl ConfigurationEditingErrorCode {
    const ALL: [ConfigurationEditingErrorCode; 14] = [
        Self::ErrorUnknownKey,
        Self::ErrorInvalidWorkspaceConfigurationApplication,
        Self::ErrorInvalidWorkspaceConfigurationMachine,
        Self::ErrorInvalidFolderConfiguration,
        Self::ErrorInvalidUserTarget,
        Self::ErrorInvalidWorkspaceTarget,
        Self::ErrorInvalidFolderTarget,
        Self::ErrorInvalidResourceLanguageConfiguration,
        Self::ErrorNoWorkspaceOpened,
        Self::ErrorConfigurationFileDirty,
        Self::ErrorConfigurationFileModifiedSince,
        Self::ErrorInvalidConfiguration,
        Self::ErrorPolicyConfiguration,
        Self::ErrorInternal,
    ];
}

/// `ConfigurationEditingError`.
pub fn configuration_editing_error(message: impl Into<String>, code: ConfigurationEditingErrorCode) -> Error {
    Error::no_telemetry(message).with_prop("code", code as i32)
}

/// O `code` de um [`configuration_editing_error`], se o erro for um.
pub fn configuration_editing_error_code(error: &Error) -> Option<ConfigurationEditingErrorCode> {
    let code: usize = error.prop("code")?.parse().ok()?;
    ConfigurationEditingErrorCode::ALL.get(code).copied()
}

/// `IConfigurationValue` (de `configurationEditing.ts`): a chave e o valor a escrever (`undefined` remove).
#[derive(Clone)]
pub struct ConfigurationEditingValue {
    pub key: String,
    pub value: Dynamic,
}

/// `IConfigurationEditingOptions`.
#[derive(Clone, Default)]
pub struct ConfigurationEditingOptions {
    pub options: ConfigurationUpdateOptions,
    /// Scope of configuration to be written into.
    pub scopes: Option<ConfigurationUpdateOverrides>,
}

/// `EditableConfigurationTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditableConfigurationTarget {
    UserLocal = 1,
    UserRemote,
    Workspace,
    WorkspaceFolder,
}

#[derive(Clone)]
struct ConfigurationEditOperation {
    key: String,
    value: Dynamic,
    target: EditableConfigurationTarget,
    json_path: Vec<EditSegment>,
    resource: Option<URI>,
    workspace_stand_alone_configuration_key: Option<&'static str>,
}

/// A ação de uma notificação com opções (`INotificationService.prompt`).
#[derive(Clone)]
pub struct PromptAction {
    pub label: String,
    pub run: Rc<dyn Fn()>,
}

/// O modelo de texto resolvido que a edição lê e altera (`IResolvedTextEditorModel.textEditorModel`).
pub trait ConfigurationEditingModel {
    fn uri(&self) -> URI;
    fn get_value(&self) -> String;
    fn insert_spaces(&self) -> bool;
    fn tab_size(&self) -> usize;
    fn get_eol(&self) -> String;
    /// `applyEditsToBuffer`: troca o trecho do modelo pelo conteúdo da edição e devolve se alterou o texto (`false` se
    /// o texto no trecho já era o conteúdo).
    fn apply_edit(&self, edit: &Edit) -> bool;
    /// `reference.dispose()`.
    fn dispose(&self);
}

/// Os serviços de texto, notificação, preferências e editor que a edição usa (veja a nota do módulo).
pub trait ConfigurationEditingServices {
    /// `textModelResolverService.createModelReference(resource)`.
    fn create_model_reference(&self, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>>;
    /// `textFileService.isDirty(resource)`.
    fn is_dirty(&self, resource: &URI) -> bool;
    /// `textFileService.save(resource, { ignoreErrorHandler: true })`.
    fn save(&self, resource: &URI) -> Promise<()>;
    /// `textFileService.revert(resource)`.
    fn revert(&self, resource: &URI) -> Promise<()>;
    /// `textFileService.write(resource, content, { encoding: 'utf8' })`.
    fn write(&self, resource: &URI, content: &str) -> Promise<()>;
    /// `notificationService.error(message)`.
    fn notify_error(&self, message: &str);
    /// `notificationService.prompt(Severity.Error, message, actions)`.
    fn prompt_error(&self, message: &str, actions: Vec<PromptAction>);
    /// `preferencesService.openUserSettings({ jsonEditor: true })`.
    fn open_user_settings(&self);
    /// `preferencesService.openRemoteSettings({ jsonEditor: true })`.
    fn open_remote_settings(&self);
    /// `preferencesService.openWorkspaceSettings({ jsonEditor: true })`.
    fn open_workspace_settings(&self);
    /// `preferencesService.openFolderSettings({ folderUri, jsonEditor: true })`.
    fn open_folder_settings(&self, folder_uri: &URI);
    /// `editorService.openEditor({ resource, options: { pinned: true } })`.
    fn open_file(&self, resource: &URI);
    /// `filesConfigurationService.enableAutoSaveAfterShortDelay(resource)`.
    fn enable_auto_save_after_short_delay(&self, resource: &URI) -> DisposableRef;
}

/// `ConfigurationEditing`.
pub struct ConfigurationEditing {
    queue: Queue<()>,
    remote_settings_resource: Option<URI>,
    configuration_service: Rc<dyn IWorkbenchConfigurationService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    file_service: Rc<dyn IFileService>,
    services: Rc<dyn ConfigurationEditingServices>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl ConfigurationEditing {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        remote_settings_resource: Option<URI>,
        configuration_service: Rc<dyn IWorkbenchConfigurationService>,
        context_service: Rc<dyn IWorkspaceContextService>,
        user_data_profile_service: Rc<dyn IUserDataProfileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        file_service: Rc<dyn IFileService>,
        services: Rc<dyn ConfigurationEditingServices>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            queue: Queue::new(),
            remote_settings_resource,
            configuration_service,
            context_service,
            user_data_profile_service,
            user_data_profiles_service,
            file_service,
            services,
            uri_identity_service,
        })
    }

    /// `writeConfiguration(target, value, options)`.
    pub fn write_configuration(self: &Rc<Self>, target: EditableConfigurationTarget, value: ConfigurationEditingValue, options: ConfigurationEditingOptions) -> Promise<()> {
        let operation = self.get_configuration_edit_operation(target, &value, &options.scopes.clone().unwrap_or_default());
        // queue up writes to prevent race conditions
        let this = self.clone();
        self.queue.queue(move || {
            async_fn(async move {
                match this.do_write_configuration(operation.clone(), options.clone()).await {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        if options.options.donot_notify_error {
                            return Err(error);
                        }
                        this.on_error(error, operation, options.scopes).await
                    }
                }
            })
        })
    }

    fn do_write_configuration(self: &Rc<Self>, operation: ConfigurationEditOperation, options: ConfigurationEditingOptions) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            this.validate(operation.target, &operation, options.options.handle_dirty_file.is_none(), &options.scopes.clone().unwrap_or_default())?;
            let resource = operation.resource.clone().expect("validate garante o recurso");
            let reference = this.resolve_model_reference(&resource).await?;
            let result = {
                let formatting_options = Self::get_formatting_options(reference.as_ref());
                this.update_configuration(&operation, reference.clone(), formatting_options, &options).await
            };
            reference.dispose();
            result
        })
    }

    fn update_configuration(
        self: &Rc<Self>,
        operation: &ConfigurationEditOperation,
        model: Rc<dyn ConfigurationEditingModel>,
        formatting_options: FormattingOptions,
        options: &ConfigurationEditingOptions,
    ) -> Promise<()> {
        let (this, operation, options) = (self.clone(), operation.clone(), options.clone());
        async_fn(async move {
            if Self::has_parse_errors(&model.get_value(), &operation) {
                return Err(this.to_configuration_editing_error(ConfigurationEditingErrorCode::ErrorInvalidConfiguration, operation.target, &operation));
            }

            let uri = model.uri();
            if this.services.is_dirty(&uri)
                && let Some(handle_dirty_file) = options.options.handle_dirty_file
            {
                match handle_dirty_file {
                    HandleDirtyFile::Save => this.save(&uri, &operation).await?,
                    HandleDirtyFile::Revert => this.services.revert(&uri).await?,
                }
            }

            let edit = this.get_edits(&operation, &model.get_value(), &formatting_options).into_iter().next();
            if let Some(edit) = edit {
                // Optimization: we apply edits to a text model and save it right after. Use the files config service
                // to signal this to the workbench to optimise the UI during this operation.
                let disposable = this.services.enable_auto_save_after_short_delay(&uri);
                let result = if model.apply_edit(&edit) { this.save(&uri, &operation).await } else { Ok(()) };
                disposable.dispose();
                result?;
            }
            Ok(())
        })
    }

    fn save(self: &Rc<Self>, resource: &URI, operation: &ConfigurationEditOperation) -> Promise<()> {
        let (this, operation) = (self.clone(), operation.clone());
        self.services.save(resource).then_result_promise(move |result| match result {
            Ok(()) => Promise::resolved(()),
            Err(error) => {
                if FileOperationError::from_error(&error).map(|e| e.file_operation_result) == Some(FileOperationResult::FileModifiedSince) {
                    return Promise::rejected(this.to_configuration_editing_error(ConfigurationEditingErrorCode::ErrorConfigurationFileModifiedSince, operation.target, &operation));
                }
                Promise::rejected(configuration_editing_error(
                    localize("fsError", "Error while writing to {0}. {1}", &[&Self::stringify_target(operation.target), &error.message]),
                    ConfigurationEditingErrorCode::ErrorInternal,
                ))
            }
        })
    }

    fn get_edits(&self, operation: &ConfigurationEditOperation, model_content: &str, formatting_options: &FormattingOptions) -> Vec<Edit> {
        if !operation.json_path.is_empty() {
            let units: Vec<u16> = model_content.encode_utf16().collect();
            return set_property(&units, &operation.json_path, to_json(&operation.value), formatting_options, None).unwrap_or_default();
        }

        // Without jsonPath, the entire configuration file is being replaced, so we just use JSON.stringify
        let indent = match (formatting_options.insert_spaces, formatting_options.tab_size) {
            (Some(true), Some(tab_size)) if tab_size > 0 => " ".repeat(tab_size),
            _ => "\t".to_owned(),
        };
        let content = to_json(&operation.value).map(|value| je_base::json_formatter::json_stringify(&value, Some(&indent))).unwrap_or_default();
        vec![Edit { content: JsString::from(content), length: model_content.encode_utf16().count(), offset: 0 }]
    }

    fn get_formatting_options(model: &dyn ConfigurationEditingModel) -> FormattingOptions {
        FormattingOptions { insert_spaces: Some(model.insert_spaces()), tab_size: Some(model.tab_size()), eol: Some(JsString::from(model.get_eol())) }
    }

    fn on_error(self: &Rc<Self>, error: Error, operation: ConfigurationEditOperation, scopes: Option<ConfigurationUpdateOverrides>) -> Promise<()> {
        match configuration_editing_error_code(&error) {
            Some(ConfigurationEditingErrorCode::ErrorInvalidConfiguration) => {
                self.on_invalid_configuration_error(&error, &operation);
                Promise::resolved(())
            }
            Some(ConfigurationEditingErrorCode::ErrorConfigurationFileDirty) => {
                self.on_configuration_file_dirty_error(&error, &operation, scopes);
                Promise::resolved(())
            }
            Some(ConfigurationEditingErrorCode::ErrorConfigurationFileModifiedSince) => self.do_write_configuration(
                operation,
                ConfigurationEditingOptions { scopes, options: ConfigurationUpdateOptions { handle_dirty_file: Some(HandleDirtyFile::Revert), ..Default::default() } },
            ),
            _ => {
                self.services.notify_error(&error.message);
                Promise::resolved(())
            }
        }
    }

    fn on_invalid_configuration_error(self: &Rc<Self>, error: &Error, operation: &ConfigurationEditOperation) {
        let open_stand_alone_configuration_action_label = match operation.workspace_stand_alone_configuration_key {
            Some(TASKS_CONFIGURATION_KEY) => Some(localize("openTasksConfiguration", "Open Tasks Configuration", &[])),
            Some(LAUNCH_CONFIGURATION_KEY) => Some(localize("openLaunchConfiguration", "Open Launch Configuration", &[])),
            Some(MCP_CONFIGURATION_KEY) => Some(localize("openMcpConfiguration", "Open MCP Configuration", &[])),
            _ => None,
        };
        let this = self.clone();
        if let Some(label) = open_stand_alone_configuration_action_label {
            let resource = operation.resource.clone().expect("a operação tem recurso");
            self.services.prompt_error(&error.message, vec![PromptAction { label, run: Rc::new(move || this.services.open_file(&resource)) }]);
        } else {
            let operation = operation.clone();
            self.services.prompt_error(&error.message, vec![PromptAction { label: localize("open", "Open Settings", &[]), run: Rc::new(move || this.open_settings(&operation)) }]);
        }
    }

    fn on_configuration_file_dirty_error(self: &Rc<Self>, error: &Error, operation: &ConfigurationEditOperation, scopes: Option<ConfigurationUpdateOverrides>) {
        let open_stand_alone_configuration_action_label = match operation.workspace_stand_alone_configuration_key {
            Some(TASKS_CONFIGURATION_KEY) => Some(localize("openTasksConfiguration", "Open Tasks Configuration", &[])),
            Some(LAUNCH_CONFIGURATION_KEY) => Some(localize("openLaunchConfiguration", "Open Launch Configuration", &[])),
            _ => None,
        };
        let save_and_retry = {
            let (this, operation, scopes) = (self.clone(), operation.clone(), scopes);
            Rc::new(move || {
                let key = match (operation.workspace_stand_alone_configuration_key, operation.key.is_empty()) {
                    (Some(stand_alone), false) => format!("{stand_alone}.{}", operation.key),
                    (Some(stand_alone), true) => stand_alone.to_owned(),
                    (None, _) => operation.key.clone(),
                };
                let _ = this.write_configuration(
                    operation.target,
                    ConfigurationEditingValue { key, value: operation.value.clone() },
                    ConfigurationEditingOptions { options: ConfigurationUpdateOptions { handle_dirty_file: Some(HandleDirtyFile::Save), ..Default::default() }, scopes: scopes.clone() },
                );
            })
        };
        let save_and_retry_label = localize("saveAndRetry", "Save and Retry", &[]);
        let this = self.clone();
        if let Some(label) = open_stand_alone_configuration_action_label {
            let resource = operation.resource.clone().expect("a operação tem recurso");
            self.services.prompt_error(
                &error.message,
                vec![PromptAction { label: save_and_retry_label, run: save_and_retry }, PromptAction { label, run: Rc::new(move || this.services.open_file(&resource)) }],
            );
        } else {
            let operation = operation.clone();
            self.services.prompt_error(
                &error.message,
                vec![
                    PromptAction { label: save_and_retry_label, run: save_and_retry },
                    PromptAction { label: localize("open", "Open Settings", &[]), run: Rc::new(move || this.open_settings(&operation)) },
                ],
            );
        }
    }

    fn open_settings(&self, operation: &ConfigurationEditOperation) {
        match operation.target {
            EditableConfigurationTarget::UserLocal => self.services.open_user_settings(),
            EditableConfigurationTarget::UserRemote => self.services.open_remote_settings(),
            EditableConfigurationTarget::Workspace => self.services.open_workspace_settings(),
            EditableConfigurationTarget::WorkspaceFolder => {
                if let Some(resource) = &operation.resource
                    && let Some(workspace_folder) = self.context_service.get_workspace_folder(resource)
                {
                    self.services.open_folder_settings(&workspace_folder.uri);
                }
            }
        }
    }

    fn to_configuration_editing_error(&self, code: ConfigurationEditingErrorCode, target: EditableConfigurationTarget, operation: &ConfigurationEditOperation) -> Error {
        configuration_editing_error(self.to_error_message(code, target, operation), code)
    }

    fn workspace_folder_name(&self, operation: &ConfigurationEditOperation) -> String {
        operation.resource.as_ref().and_then(|resource| self.context_service.get_workspace_folder(resource)).map_or_else(|| "<<unknown>>".to_owned(), |folder| folder.name)
    }

    fn to_error_message(&self, error: ConfigurationEditingErrorCode, target: EditableConfigurationTarget, operation: &ConfigurationEditOperation) -> String {
        use ConfigurationEditingErrorCode as Code;
        let key = operation.key.as_str();
        let stand_alone = operation.workspace_stand_alone_configuration_key;
        match error {
            // API constraints
            Code::ErrorPolicyConfiguration => localize("errorPolicyConfiguration", "Unable to write {0} because it is configured in system policy.", &[key]),
            Code::ErrorUnknownKey => localize("errorUnknownKey", "Unable to write to {0} because {1} is not a registered configuration.", &[&Self::stringify_target(target), key]),
            Code::ErrorInvalidWorkspaceConfigurationApplication => {
                localize("errorInvalidWorkspaceConfigurationApplication", "Unable to write {0} to Workspace Settings. This setting can be written only into User settings.", &[key])
            }
            Code::ErrorInvalidWorkspaceConfigurationMachine => {
                localize("errorInvalidWorkspaceConfigurationMachine", "Unable to write {0} to Workspace Settings. This setting can be written only into User settings.", &[key])
            }
            Code::ErrorInvalidFolderConfiguration => localize("errorInvalidFolderConfiguration", "Unable to write to Folder Settings because {0} does not support the folder resource scope.", &[key]),
            Code::ErrorInvalidUserTarget => localize("errorInvalidUserTarget", "Unable to write to User Settings because {0} does not support for global scope.", &[key]),
            Code::ErrorInvalidWorkspaceTarget => {
                localize("errorInvalidWorkspaceTarget", "Unable to write to Workspace Settings because {0} does not support for workspace scope in a multi folder workspace.", &[key])
            }
            Code::ErrorInvalidFolderTarget => localize("errorInvalidFolderTarget", "Unable to write to Folder Settings because no resource is provided.", &[]),
            Code::ErrorInvalidResourceLanguageConfiguration => {
                localize("errorInvalidResourceLanguageConfiguration", "Unable to write to Language Settings because {0} is not a resource language setting.", &[key])
            }
            Code::ErrorNoWorkspaceOpened => {
                localize("errorNoWorkspaceOpened", "Unable to write to {0} because no workspace is opened. Please open a workspace first and try again.", &[&Self::stringify_target(target)])
            }

            // User issues
            Code::ErrorInvalidConfiguration => {
                if stand_alone == Some(TASKS_CONFIGURATION_KEY) {
                    return localize("errorInvalidTaskConfiguration", "Unable to write into the tasks configuration file. Please open it to correct errors/warnings in it and try again.", &[]);
                }
                if stand_alone == Some(LAUNCH_CONFIGURATION_KEY) {
                    return localize("errorInvalidLaunchConfiguration", "Unable to write into the launch configuration file. Please open it to correct errors/warnings in it and try again.", &[]);
                }
                if stand_alone == Some(MCP_CONFIGURATION_KEY) {
                    return localize("errorInvalidMCPConfiguration", "Unable to write into the MCP configuration file. Please open it to correct errors/warnings in it and try again.", &[]);
                }
                match target {
                    EditableConfigurationTarget::UserLocal => {
                        localize("errorInvalidConfiguration", "Unable to write into user settings. Please open the user settings to correct errors/warnings in it and try again.", &[])
                    }
                    EditableConfigurationTarget::UserRemote => localize(
                        "errorInvalidRemoteConfiguration",
                        "Unable to write into remote user settings. Please open the remote user settings to correct errors/warnings in it and try again.",
                        &[],
                    ),
                    EditableConfigurationTarget::Workspace => localize(
                        "errorInvalidConfigurationWorkspace",
                        "Unable to write into workspace settings. Please open the workspace settings to correct errors/warnings in the file and try again.",
                        &[],
                    ),
                    EditableConfigurationTarget::WorkspaceFolder => localize(
                        "errorInvalidConfigurationFolder",
                        "Unable to write into folder settings. Please open the '{0}' folder settings to correct errors/warnings in it and try again.",
                        &[&self.workspace_folder_name(operation)],
                    ),
                }
            }
            Code::ErrorConfigurationFileDirty => {
                if stand_alone == Some(TASKS_CONFIGURATION_KEY) {
                    return localize("errorTasksConfigurationFileDirty", "Unable to write into tasks configuration file because the file has unsaved changes. Please save it first and then try again.", &[]);
                }
                if stand_alone == Some(LAUNCH_CONFIGURATION_KEY) {
                    return localize("errorLaunchConfigurationFileDirty", "Unable to write into launch configuration file because the file has unsaved changes. Please save it first and then try again.", &[]);
                }
                if stand_alone == Some(MCP_CONFIGURATION_KEY) {
                    return localize("errorMCPConfigurationFileDirty", "Unable to write into MCP configuration file because the file has unsaved changes. Please save it first and then try again.", &[]);
                }
                match target {
                    EditableConfigurationTarget::UserLocal => localize(
                        "errorConfigurationFileDirty",
                        "Unable to write into user settings because the file has unsaved changes. Please save the user settings file first and then try again.",
                        &[],
                    ),
                    EditableConfigurationTarget::UserRemote => localize(
                        "errorRemoteConfigurationFileDirty",
                        "Unable to write into remote user settings because the file has unsaved changes. Please save the remote user settings file first and then try again.",
                        &[],
                    ),
                    EditableConfigurationTarget::Workspace => localize(
                        "errorConfigurationFileDirtyWorkspace",
                        "Unable to write into workspace settings because the file has unsaved changes. Please save the workspace settings file first and then try again.",
                        &[],
                    ),
                    EditableConfigurationTarget::WorkspaceFolder => localize(
                        "errorConfigurationFileDirtyFolder",
                        "Unable to write into folder settings because the file has unsaved changes. Please save the '{0}' folder settings file first and then try again.",
                        &[&self.workspace_folder_name(operation)],
                    ),
                }
            }
            Code::ErrorConfigurationFileModifiedSince => {
                if stand_alone == Some(TASKS_CONFIGURATION_KEY) {
                    return localize("errorTasksConfigurationFileModifiedSince", "Unable to write into tasks configuration file because the content of the file is newer.", &[]);
                }
                if stand_alone == Some(LAUNCH_CONFIGURATION_KEY) {
                    return localize("errorLaunchConfigurationFileModifiedSince", "Unable to write into launch configuration file because the content of the file is newer.", &[]);
                }
                if stand_alone == Some(MCP_CONFIGURATION_KEY) {
                    return localize("errorMCPConfigurationFileModifiedSince", "Unable to write into MCP configuration file because the content of the file is newer.", &[]);
                }
                match target {
                    EditableConfigurationTarget::UserLocal => localize("errorConfigurationFileModifiedSince", "Unable to write into user settings because the content of the file is newer.", &[]),
                    EditableConfigurationTarget::UserRemote => {
                        localize("errorRemoteConfigurationFileModifiedSince", "Unable to write into remote user settings because the content of the file is newer.", &[])
                    }
                    EditableConfigurationTarget::Workspace => {
                        localize("errorConfigurationFileModifiedSinceWorkspace", "Unable to write into workspace settings because the content of the file is newer.", &[])
                    }
                    EditableConfigurationTarget::WorkspaceFolder => {
                        localize("errorConfigurationFileModifiedSinceFolder", "Unable to write into folder settings because the content of the file is newer.", &[])
                    }
                }
            }
            Code::ErrorInternal => localize("errorUnknown", "Unable to write to {0} because of an internal error.", &[&Self::stringify_target(target)]),
        }
    }

    fn stringify_target(target: EditableConfigurationTarget) -> String {
        match target {
            EditableConfigurationTarget::UserLocal => localize("userTarget", "User Settings", &[]),
            EditableConfigurationTarget::UserRemote => localize("remoteUserTarget", "Remote User Settings", &[]),
            EditableConfigurationTarget::Workspace => localize("workspaceTarget", "Workspace Settings", &[]),
            EditableConfigurationTarget::WorkspaceFolder => localize("folderTarget", "Folder Settings", &[]),
        }
    }

    fn default_resource_value(&self, resource: &URI) -> &'static str {
        let ext_uri = self.uri_identity_service.ext_uri();
        let basename = ext_uri.basename(resource, None);
        let extname = ext_uri.extname(resource);
        let configuration_value = &basename[..basename.len().saturating_sub(extname.len())];
        match configuration_value {
            TASKS_CONFIGURATION_KEY => TASKS_DEFAULT,
            _ => "{}",
        }
    }

    fn resolve_model_reference(self: &Rc<Self>, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let exists = this.file_service.exists(&resource).await?;
            if !exists {
                this.services.write(&resource, this.default_resource_value(&resource)).await?;
            }
            this.services.create_model_reference(&resource).await
        })
    }

    fn has_parse_errors(content: &str, operation: &ConfigurationEditOperation) -> bool {
        // If we write to a workspace standalone file and replace the entire contents (no key provided)
        // we can return here because any parse errors can safely be ignored since all contents are replaced
        if operation.workspace_stand_alone_configuration_key.is_some() && operation.key.is_empty() {
            return false;
        }
        let units: Vec<u16> = content.encode_utf16().collect();
        let mut parse_errors = Vec::new();
        json::parse(&units, &mut parse_errors, ParseOptions { allow_trailing_comma: true, allow_empty_content: true, ..Default::default() });
        !parse_errors.is_empty()
    }

    fn validate(&self, target: EditableConfigurationTarget, operation: &ConfigurationEditOperation, check_dirty: bool, overrides: &ConfigurationUpdateOverrides) -> Result<(), Error> {
        use ConfigurationEditingErrorCode as Code;
        let fail = |code: Code| Err(self.to_configuration_editing_error(code, target, operation));

        if !self.configuration_service.inspect(&operation.key, &ConfigurationOverrides::default()).policy_value().is_undefined() {
            return fail(Code::ErrorPolicyConfiguration);
        }

        let configuration_scope = configuration_registry().get_configuration_properties().get(operation.key.as_str()).and_then(|property| property.scope());

        // Key to update must be a known setting from the registry unless
        //  - the key is standalone configuration (eg: tasks, debug)
        //  - the key is an override identifier
        //  - the operation is to delete the key
        let operation_key = JsString::from(operation.key.as_str());
        if operation.workspace_stand_alone_configuration_key.is_none() {
            let valid_keys = self.configuration_service.keys().default;
            if !valid_keys.contains(&operation_key) && !is_override_property(&operation_key) && !operation.value.is_undefined() {
                return fail(Code::ErrorUnknownKey);
            }
        }

        if let Some(stand_alone) = operation.workspace_stand_alone_configuration_key {
            // Global launches are not supported
            if stand_alone != TASKS_CONFIGURATION_KEY && stand_alone != MCP_CONFIGURATION_KEY && matches!(target, EditableConfigurationTarget::UserLocal | EditableConfigurationTarget::UserRemote) {
                return fail(Code::ErrorInvalidUserTarget);
            }
        }

        // Target cannot be workspace or folder if no workspace opened
        if matches!(target, EditableConfigurationTarget::Workspace | EditableConfigurationTarget::WorkspaceFolder) && self.context_service.get_workbench_state() == WorkbenchState::Empty {
            return fail(Code::ErrorNoWorkspaceOpened);
        }

        if target == EditableConfigurationTarget::Workspace && operation.workspace_stand_alone_configuration_key.is_none() && !is_override_property(&operation_key) {
            if configuration_scope.is_some_and(|scope| APPLICATION_SCOPES.contains(&scope)) {
                return fail(Code::ErrorInvalidWorkspaceConfigurationApplication);
            }
            if configuration_scope == Some(ConfigurationScope::Machine) {
                return fail(Code::ErrorInvalidWorkspaceConfigurationMachine);
            }
        }

        if target == EditableConfigurationTarget::WorkspaceFolder {
            if operation.resource.is_none() {
                return fail(Code::ErrorInvalidFolderTarget);
            }
            if operation.workspace_stand_alone_configuration_key.is_none()
                && !is_override_property(&operation_key)
                && configuration_scope.is_some_and(|scope| !FOLDER_SCOPES.contains(&scope))
            {
                return fail(Code::ErrorInvalidFolderConfiguration);
            }
        }

        if overrides.override_identifiers.as_ref().is_some_and(|identifiers| !identifiers.is_empty()) && configuration_scope != Some(ConfigurationScope::LanguageOverridable) {
            return fail(Code::ErrorInvalidResourceLanguageConfiguration);
        }

        let Some(resource) = &operation.resource else {
            return fail(Code::ErrorInvalidFolderTarget);
        };

        if check_dirty && self.services.is_dirty(resource) {
            return fail(Code::ErrorConfigurationFileDirty);
        }

        Ok(())
    }

    fn get_configuration_edit_operation(&self, target: EditableConfigurationTarget, config: &ConfigurationEditingValue, overrides: &ConfigurationUpdateOverrides) -> ConfigurationEditOperation {
        // Check for standalone workspace configurations
        if !config.key.is_empty() {
            let standalone_configuration_map: &[(&'static str, &'static str)] =
                if target == EditableConfigurationTarget::UserLocal { &USER_STANDALONE_CONFIGURATIONS } else { &WORKSPACE_STANDALONE_CONFIGURATIONS };
            for (key, relative_path) in standalone_configuration_map {
                let resource = self.get_configuration_file_resource(target, key, relative_path, overrides.resource.as_ref(), None);

                // Check for prefix
                if config.key == *key {
                    let json_path = if self.is_workspace_configuration_resource(resource.as_ref()) { vec![EditSegment::Property(JsString::from(*key))] } else { Vec::new() };
                    return ConfigurationEditOperation {
                        key: Self::last_segment(&json_path),
                        json_path,
                        value: config.value.clone(),
                        resource,
                        workspace_stand_alone_configuration_key: Some(*key),
                        target,
                    };
                }

                // Check for prefix.<setting>
                let key_prefix = format!("{key}.");
                if let Some(setting) = config.key.strip_prefix(&key_prefix) {
                    let setting = EditSegment::Property(JsString::from(setting));
                    let json_path =
                        if self.is_workspace_configuration_resource(resource.as_ref()) { vec![EditSegment::Property(JsString::from(*key)), setting] } else { vec![setting] };
                    return ConfigurationEditOperation {
                        key: Self::last_segment(&json_path),
                        json_path,
                        value: config.value.clone(),
                        resource,
                        workspace_stand_alone_configuration_key: Some(*key),
                        target,
                    };
                }
            }
        }

        let key = config.key.clone();
        let configuration_scope = configuration_registry().get_configuration_properties().get(key.as_str()).and_then(|property| property.scope());
        let mut json_path = match overrides.override_identifiers.as_ref().filter(|identifiers| !identifiers.is_empty()) {
            Some(identifiers) => vec![EditSegment::Property(key_from_override_identifiers(identifiers)), EditSegment::Property(JsString::from(key.as_str()))],
            None => vec![EditSegment::Property(JsString::from(key.as_str()))],
        };
        if matches!(target, EditableConfigurationTarget::UserLocal | EditableConfigurationTarget::UserRemote) {
            let resource = self.get_configuration_file_resource(target, &key, "", None, configuration_scope);
            return ConfigurationEditOperation { key, json_path, value: config.value.clone(), resource, workspace_stand_alone_configuration_key: None, target };
        }

        let resource = self.get_configuration_file_resource(target, &key, FOLDER_SETTINGS_PATH, overrides.resource.as_ref(), configuration_scope);
        if self.is_workspace_configuration_resource(resource.as_ref()) {
            json_path.insert(0, EditSegment::Property(JsString::from("settings")));
        }
        ConfigurationEditOperation { key, json_path, value: config.value.clone(), resource, workspace_stand_alone_configuration_key: None, target }
    }

    /// `jsonPath[jsonPath.length - 1]` como texto da chave (`undefined` do original, com caminho vazio, vira o texto
    /// vazio, que o resto trata como "sem chave").
    fn last_segment(json_path: &[EditSegment]) -> String {
        match json_path.last() {
            Some(EditSegment::Property(property)) => property.to_string_lossy(),
            _ => String::new(),
        }
    }

    fn is_workspace_configuration_resource(&self, resource: Option<&URI>) -> bool {
        let workspace = self.context_service.get_workspace();
        match (workspace.configuration(), resource) {
            (Some(configuration), Some(resource)) => fs_path(&configuration) == fs_path(resource),
            _ => false,
        }
    }

    fn get_configuration_file_resource(&self, target: EditableConfigurationTarget, key: &str, relative_path: &str, resource: Option<&URI>, _scope: Option<ConfigurationScope>) -> Option<URI> {
        if target == EditableConfigurationTarget::UserLocal {
            let profile = self.user_data_profile_service.current_profile();
            if key == TASKS_CONFIGURATION_KEY {
                return Some(profile.tasks_resource);
            }
            if key == MCP_CONFIGURATION_KEY {
                return Some(profile.mcp_resource);
            }
            if !profile.is_default && self.configuration_service.is_setting_applied_for_all_profiles(key) {
                return Some(self.user_data_profiles_service.default_profile().settings_resource);
            }
            return Some(profile.settings_resource);
        }
        if target == EditableConfigurationTarget::UserRemote {
            return self.remote_settings_resource.clone();
        }
        let workbench_state = self.context_service.get_workbench_state();
        if workbench_state != WorkbenchState::Empty {
            let workspace = self.context_service.get_workspace();

            if target == EditableConfigurationTarget::Workspace {
                if workbench_state == WorkbenchState::Workspace {
                    return workspace.configuration();
                }
                if workbench_state == WorkbenchState::Folder {
                    return workspace.folders().first().map(|folder| folder.to_resource(relative_path));
                }
            }

            if target == EditableConfigurationTarget::WorkspaceFolder
                && let Some(resource) = resource
                && let Some(folder) = self.context_service.get_workspace_folder(resource)
            {
                return Some(folder.to_resource(relative_path));
            }
        }
        None
    }
}
