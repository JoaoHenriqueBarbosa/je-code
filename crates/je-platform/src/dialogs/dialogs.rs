//! Porte de `src/vs/platform/dialogs/common/dialogs.ts`.
//!
//! Divergências: o `T` do resultado de um botão de prompt (`IPromptButton<T>`) é [`Dynamic`]; `IMarkdownString` é
//! [`je_base::html_content::MarkdownString`]; as sobrecargas de `prompt` (cancelamento personalizado, padrão ou nenhum)
//! são um método só, com o resultado como [`PromptResult`]; `run` devolve `T | Promise<T>` como [`MaybePromise`].

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::html_content::MarkdownString;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::resources::ext_uri_biased_ignore_path_case;
use crate::telemetry::ITelemetryData;

/// `DialogType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogType {
    None,
    Info,
    Error,
    Question,
    Warning,
}

/// `Severity | DialogType` de `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogTypeOrSeverity {
    Severity(Severity),
    Type(DialogType),
}

/// `ICheckbox`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkbox {
    pub label: String,
    pub checked: Option<bool>,
}

/// `ICheckboxResult`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckboxResult {
    pub checkbox_checked: Option<bool>,
}

/// `ICustomDialogMarkdown`.
#[derive(Clone)]
pub struct CustomDialogMarkdown {
    pub markdown: MarkdownString,
    pub classes: Option<Vec<String>>,
}

/// `ICustomDialogOptions`.
#[derive(Clone, Default)]
pub struct CustomDialogOptions {
    pub button_details: Option<Vec<String>>,
    pub markdown_details: Option<Vec<CustomDialogMarkdown>>,
    pub classes: Option<Vec<String>>,
    pub icon: Option<ThemeIcon>,
    pub disable_close_action: bool,
    /// `'horizontal' | 'vertical'`.
    pub alignment: Option<String>,
}

/// `boolean | ICustomDialogOptions` de `custom`.
#[derive(Clone)]
pub enum CustomDialog {
    Flag(bool),
    Options(CustomDialogOptions),
}

/// `string | IMarkdownString` de `detail`.
#[derive(Clone)]
pub enum DialogDetail {
    Text(String),
    Markdown(MarkdownString),
}

/// `IBaseDialogOptions`.
#[derive(Clone)]
pub struct BaseDialogOptions {
    pub dialog_type: Option<DialogTypeOrSeverity>,
    pub title: Option<String>,
    pub message: String,
    pub detail: Option<DialogDetail>,
    pub checkbox: Option<Checkbox>,
    pub custom: Option<CustomDialog>,
    pub token: Option<CancellationToken>,
}

impl BaseDialogOptions {
    pub fn new(message: impl Into<String>) -> Self {
        Self { dialog_type: None, title: None, message: message.into(), detail: None, checkbox: None, custom: None, token: None }
    }
}

/// `IConfirmation`.
#[derive(Clone)]
pub struct Confirmation {
    pub base: BaseDialogOptions,
    pub primary_button: Option<String>,
    pub cancel_button: Option<String>,
}

/// `IConfirmationResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfirmationResult {
    pub checkbox_checked: Option<bool>,
    pub confirmed: bool,
}

/// `IInputElement`.
#[derive(Clone, Debug, Default)]
pub struct InputElement {
    /// `'text' | 'password'`.
    pub input_type: Option<String>,
    pub value: Option<String>,
    pub placeholder: Option<String>,
}

/// `IInput`.
#[derive(Clone)]
pub struct Input {
    pub confirmation: Confirmation,
    pub inputs: Vec<InputElement>,
}

/// `IInputResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputResult {
    pub confirmation: ConfirmationResult,
    pub values: Option<Vec<String>>,
}

/// `IPromptBaseButton<T>`: `run(checkbox)` devolve `T | Promise<T>`.
#[derive(Clone)]
pub struct PromptBaseButton {
    pub run: Rc<dyn Fn(CheckboxResult) -> MaybePromise<Dynamic>>,
}

/// `IPromptButton<T>`.
#[derive(Clone)]
pub struct PromptButton {
    pub label: String,
    pub base: PromptBaseButton,
}

/// `IPromptCancelButton<T>`.
#[derive(Clone)]
pub struct PromptCancelButton {
    pub label: Option<String>,
    pub base: PromptBaseButton,
}

/// `IPromptCancelButton<T> | true | string` de `cancelButton`.
#[derive(Clone)]
pub enum PromptCancel {
    Default,
    Label(String),
    Button(PromptCancelButton),
}

/// `IPrompt<T>`.
#[derive(Clone)]
pub struct Prompt {
    pub base: BaseDialogOptions,
    pub buttons: Option<Vec<PromptButton>>,
    pub cancel_button: Option<PromptCancel>,
}

/// `IPromptResult<T>` e `IPromptResultWithCancel<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct PromptResult {
    pub checkbox_checked: Option<bool>,
    pub result: Option<Dynamic>,
}

/// `IAsyncPromptResult<T>`.
#[derive(Clone)]
pub struct AsyncPromptResult {
    pub checkbox_checked: Option<bool>,
    pub result: Option<Promise<Dynamic>>,
}

/// `IPickAndOpenOptions`.
#[derive(Clone, Default)]
pub struct PickAndOpenOptions {
    pub force_new_window: bool,
    pub default_uri: Option<URI>,
    pub telemetry_extra_data: Option<ITelemetryData>,
    pub available_file_systems: Option<Vec<String>>,
    pub remote_authority: Option<Option<String>>,
}

/// `FileFilter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    pub extensions: Vec<String>,
    pub name: String,
}

/// `{ withMnemonic, withoutMnemonic } | string` de `saveLabel` e `openLabel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogLabel {
    Plain(String),
    WithMnemonic { with_mnemonic: String, without_mnemonic: String },
}

/// `ISaveDialogOptions`.
#[derive(Clone, Default)]
pub struct SaveDialogOptions {
    pub title: Option<String>,
    pub default_uri: Option<URI>,
    pub filters: Option<Vec<FileFilter>>,
    pub save_label: Option<DialogLabel>,
    pub available_file_systems: Option<Vec<String>>,
}

/// `IOpenDialogOptions`.
#[derive(Clone, Default)]
pub struct OpenDialogOptions {
    pub title: Option<String>,
    pub default_uri: Option<URI>,
    pub open_label: Option<DialogLabel>,
    pub can_select_files: Option<bool>,
    pub can_select_folders: Option<bool>,
    pub can_select_many: bool,
    pub filters: Option<Vec<FileFilter>>,
    pub available_file_systems: Option<Vec<String>>,
}

/// `IDialogHandler`.
pub trait IDialogHandler {
    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResultCell>;
    fn prompt(&self, prompt: Prompt) -> Promise<AsyncPromptResultCell>;
    fn input(&self, input: Input) -> Promise<InputResultCell>;
    fn about(&self, title: &str, details: &str, details_to_copy: &str) -> Promise<()>;
}

/// `Promise` exige `Clone`: nomes para os resultados que já o são.
pub type ConfirmationResultCell = ConfirmationResult;
pub type AsyncPromptResultCell = AsyncPromptResult;
pub type InputResultCell = InputResult;
pub type PromptResultCell = PromptResult;

/// `IDialogService`.
pub trait IDialogService {
    fn on_will_show_dialog(&self) -> Event<()>;
    fn on_did_show_dialog(&self) -> Event<()>;
    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResultCell>;
    fn prompt(&self, prompt: Prompt) -> Promise<PromptResultCell>;
    fn input(&self, input: Input) -> Promise<InputResultCell>;
    fn info(&self, message: &str, detail: Option<&str>) -> Promise<()>;
    fn warn(&self, message: &str, detail: Option<&str>) -> Promise<()>;
    fn error(&self, message: &str, detail: Option<&str>) -> Promise<()>;
    fn about(&self) -> Promise<()>;
}

service_id!(pub I_DIALOG_SERVICE: dyn IDialogService = "dialogService");

/// `ConfirmResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmResult {
    Save,
    DontSave,
    Cancel,
}

/// `IFileDialogService`.
pub trait IFileDialogService {
    fn default_file_path(&self, scheme_filter: Option<&str>) -> Promise<URI>;
    fn default_folder_path(&self, scheme_filter: Option<&str>) -> Promise<URI>;
    fn default_workspace_path(&self, scheme_filter: Option<&str>) -> Promise<URI>;
    fn pick_file_folder_and_open(&self, options: PickAndOpenOptions) -> Promise<()>;
    fn pick_file_and_open(&self, options: PickAndOpenOptions) -> Promise<()>;
    fn pick_folder_and_open(&self, options: PickAndOpenOptions) -> Promise<()>;
    fn pick_workspace_and_open(&self, options: PickAndOpenOptions) -> Promise<()>;
    fn pick_file_to_save(&self, default_uri: &URI, available_file_systems: Option<Vec<String>>) -> Promise<Option<URI>>;
    fn preferred_home(&self, scheme_filter: Option<&str>) -> Promise<URI>;
    fn show_save_dialog(&self, options: SaveDialogOptions) -> Promise<Option<URI>>;
    fn show_save_confirm(&self, file_names_or_resources: &[FileNameOrResource]) -> Promise<ConfirmResult>;
    fn show_open_dialog(&self, options: OpenDialogOptions) -> Promise<Option<Vec<URI>>>;
}

service_id!(pub I_FILE_DIALOG_SERVICE: dyn IFileDialogService = "fileDialogService");

/// `string | URI`.
#[derive(Clone, Debug)]
pub enum FileNameOrResource {
    Name(String),
    Resource(URI),
}

/// `INativeOpenDialogOptions`.
#[derive(Clone, Default)]
pub struct NativeOpenDialogOptions {
    pub force_new_window: bool,
    pub default_path: Option<String>,
    pub telemetry_event_name: Option<String>,
    pub telemetry_extra_data: Option<ITelemetryData>,
}

const MAX_CONFIRM_FILES: usize = 10;

/// `getFileNamesMessage`.
pub fn get_file_names_message(file_names_or_resources: &[FileNameOrResource]) -> String {
    let ext_uri = ext_uri_biased_ignore_path_case();
    let mut message: Vec<String> = file_names_or_resources
        .iter()
        .take(MAX_CONFIRM_FILES)
        .map(|file_name_or_resource| match file_name_or_resource {
            FileNameOrResource::Name(name) => name.clone(),
            FileNameOrResource::Resource(resource) => ext_uri.basename(resource, None),
        })
        .collect();

    if file_names_or_resources.len() > MAX_CONFIRM_FILES {
        if file_names_or_resources.len() - MAX_CONFIRM_FILES == 1 {
            message.push(localize("moreFile", "...1 additional file not shown", &[]));
        } else {
            message.push(localize("moreFiles", "...{0} additional files not shown", &[&(file_names_or_resources.len() - MAX_CONFIRM_FILES).to_string()]));
        }
    }

    message.push(String::new());
    message.join("\n")
}

/// A lógica comum de `AbstractDialogHandler`: os botões de cada tipo de diálogo, o tipo e o resultado do prompt.
pub struct AbstractDialogHandler;

#[derive(Clone, Copy)]
enum DialogKind {
    Confirmation,
    Prompt,
    Input,
}

impl AbstractDialogHandler {
    pub fn get_confirmation_buttons(dialog: &Confirmation) -> Vec<String> {
        Self::get_buttons(&DialogRef::Confirmation(dialog), DialogKind::Confirmation)
    }

    pub fn get_prompt_buttons(dialog: &Prompt) -> Vec<String> {
        Self::get_buttons(&DialogRef::Prompt(dialog), DialogKind::Prompt)
    }

    pub fn get_input_buttons(dialog: &Input) -> Vec<String> {
        Self::get_buttons(&DialogRef::Input(dialog), DialogKind::Input)
    }

    fn get_buttons(dialog: &DialogRef, kind: DialogKind) -> Vec<String> {
        // We put buttons in the order of "default" button first and "cancel" button last. There maybe later
        // processing when presenting the buttons based on OS standards.
        let mut buttons: Vec<String> = Vec::new();
        let cancel = || localize("cancelButton", "Cancel", &[]);

        match (kind, dialog) {
            (DialogKind::Confirmation, DialogRef::Confirmation(confirmation)) => {
                buttons.push(confirmation.primary_button.clone().filter(|b| !b.is_empty()).unwrap_or_else(|| localize("yesButton", "&&Yes", &[])));
                buttons.push(confirmation.cancel_button.clone().filter(|b| !b.is_empty()).unwrap_or_else(cancel));
            }
            (DialogKind::Prompt, DialogRef::Prompt(prompt)) => {
                if let Some(prompt_buttons) = prompt.buttons.as_ref().filter(|b| !b.is_empty()) {
                    buttons.extend(prompt_buttons.iter().map(|button| button.label.clone()));
                }
                match &prompt.cancel_button {
                    Some(PromptCancel::Default) => buttons.push(cancel()),
                    Some(PromptCancel::Label(label)) if !label.is_empty() => buttons.push(label.clone()),
                    Some(PromptCancel::Label(_)) | None => {}
                    Some(PromptCancel::Button(button)) => buttons.push(button.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(cancel)),
                }
                if buttons.is_empty() {
                    buttons.push(localize("okButton", "&&OK", &[]));
                }
            }
            (DialogKind::Input, DialogRef::Input(input)) => {
                buttons.push(input.confirmation.primary_button.clone().filter(|b| !b.is_empty()).unwrap_or_else(|| localize("okButton", "&&OK", &[])));
                buttons.push(input.confirmation.cancel_button.clone().filter(|b| !b.is_empty()).unwrap_or_else(cancel));
            }
            _ => {}
        }
        buttons
    }

    /// `getDialogType`.
    pub fn get_dialog_type(dialog_type: Option<DialogTypeOrSeverity>) -> Option<DialogType> {
        match dialog_type? {
            DialogTypeOrSeverity::Type(t) => Some(t),
            DialogTypeOrSeverity::Severity(Severity::Info) => Some(DialogType::Info),
            DialogTypeOrSeverity::Severity(Severity::Error) => Some(DialogType::Error),
            DialogTypeOrSeverity::Severity(Severity::Warning) => Some(DialogType::Warning),
            DialogTypeOrSeverity::Severity(Severity::Ignore) => Some(DialogType::None),
        }
    }

    /// `getPromptResult`: roda o botão escolhido (a promessa do resultado é a dele, ou já resolvida).
    pub fn get_prompt_result(prompt: &Prompt, button_index: usize, checkbox_checked: Option<bool>) -> AsyncPromptResult {
        let mut prompt_buttons: Vec<PromptBaseButton> = prompt.buttons.iter().flatten().map(|b| b.base.clone()).collect();
        if let Some(PromptCancel::Button(button)) = &prompt.cancel_button {
            prompt_buttons.push(button.base.clone());
        }
        let result = prompt_buttons.get(button_index).map(|button| match (button.run)(CheckboxResult { checkbox_checked }) {
            MaybePromise::Value(value) => Promise::resolved(value),
            MaybePromise::Promise(promise) => promise,
        });
        AsyncPromptResult { result: Some(result.unwrap_or_else(|| Promise::resolved(Dynamic::Undefined))), checkbox_checked }
    }
}

enum DialogRef<'a> {
    Confirmation(&'a Confirmation),
    Prompt(&'a Prompt),
    Input(&'a Input),
}
