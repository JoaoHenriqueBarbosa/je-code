//! Porte de `src/vs/workbench/services/configuration/common/jsonEditingService.ts`: o `JSONEditingService`, que edita um
//! arquivo JSON por um modelo de texto e o salva em seguida.
//!
//! Decisões de tradução:
//! - **Serviços de texto.** O original recebe `ITextModelService`, `ITextFileService` e `IFilesConfigurationService`,
//!   que ainda não foram portados. O que ele lê deles é o mesmo recorte que a `ConfigurationEditing` usa, o trait
//!   [`ConfigurationEditingServices`] (`createModelReference`, `save`, `write` e `enableAutoSaveAfterShortDelay`), com o
//!   modelo resolvido como [`ConfigurationEditingModel`]. O `IFileService` é o [`FileService`] (só para `exists`).
//! - `queue.queue(() => ...)` é a [`Queue`] do `je-base`; os `async` são [`Promise`].
//! - O parâmetro `save` de `IJSONEditingService.write` não é lido pelo original (a classe sempre salva), e aqui também
//!   não.

use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{Queue, async_fn};
use je_base::errors::Error;
use je_base::json::{self, ParseOptions};
use je_base::json_edit::set_property;
use je_base::json_formatter::{Edit, FormattingOptions};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::i_file_service::IFileService;
use je_platform::js::to_json;

use super::configuration_editing::{ConfigurationEditingModel, ConfigurationEditingServices};
use super::json_editing::{IJsonEditingService, JsonEditingErrorCode, JsonValue, json_editing_error};

/// `JSONEditingService`.
pub struct JsonEditingService {
    this: Weak<Self>,
    queue: Queue<()>,
    file_service: Rc<dyn IFileService>,
    services: Rc<dyn ConfigurationEditingServices>,
}

impl JsonEditingService {
    pub fn new(file_service: Rc<dyn IFileService>, services: Rc<dyn ConfigurationEditingServices>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), queue: Queue::new(), file_service, services })
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn do_write_configuration(self: &Rc<Self>, resource: URI, values: Vec<JsonValue>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let reference = this.resolve_and_validate(&resource).await?;
            let result = this.write_to_buffer(reference.clone(), values).await;
            reference.dispose();
            result
        })
    }

    fn write_to_buffer(self: &Rc<Self>, model: Rc<dyn ConfigurationEditingModel>, values: Vec<JsonValue>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // Optimization: we apply edits to a text model and save it right after. Use the files config service to
            // signal this to the workbench to optimise the UI during this operation. For example, avoids to briefly
            // show dirty indicators.
            let disposable = this.services.enable_auto_save_after_short_delay(&model.uri());

            let mut has_edits = false;
            for value in &values {
                let edit = this.get_edits(model.as_ref(), value).into_iter().next();
                has_edits = edit.is_some_and(|edit| model.apply_edit(&edit)) || has_edits;
            }
            let result = if has_edits { this.services.save(&model.uri()).await } else { Ok(()) };
            disposable.dispose();
            result
        })
    }

    fn get_edits(&self, model: &dyn ConfigurationEditingModel, configuration_value: &JsonValue) -> Vec<Edit> {
        let (tab_size, insert_spaces) = (model.tab_size(), model.insert_spaces());
        let eol = model.get_eol();
        let JsonValue { path, value } = configuration_value;

        // With empty path the entire file is being replaced, so we just use JSON.stringify
        if path.is_empty() {
            let indent = if insert_spaces { " ".repeat(tab_size) } else { "\t".to_owned() };
            let content = to_json(value).map(|value| je_base::json_formatter::json_stringify(&value, Some(&indent))).unwrap_or_default();
            let length = content.encode_utf16().count();
            return vec![Edit { content: JsString::from(content), length, offset: 0 }];
        }

        let units: Vec<u16> = model.get_value().encode_utf16().collect();
        let formatting_options = FormattingOptions { insert_spaces: Some(insert_spaces), tab_size: Some(tab_size), eol: Some(JsString::from(eol)) };
        set_property(&units, path, to_json(value), &formatting_options, None).unwrap_or_default()
    }

    fn resolve_model_reference(self: &Rc<Self>, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let exists = this.file_service.exists(&resource).await?;
            if !exists {
                this.services.write(&resource, "{}").await?;
            }
            this.services.create_model_reference(&resource).await
        })
    }

    fn has_parse_errors(model: &dyn ConfigurationEditingModel) -> bool {
        let units: Vec<u16> = model.get_value().encode_utf16().collect();
        let mut parse_errors = Vec::new();
        json::parse(&units, &mut parse_errors, ParseOptions { allow_trailing_comma: true, allow_empty_content: true, ..Default::default() });
        !parse_errors.is_empty()
    }

    fn resolve_and_validate(self: &Rc<Self>, resource: &URI) -> Promise<Rc<dyn ConfigurationEditingModel>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let reference = this.resolve_model_reference(&resource).await?;

            if Self::has_parse_errors(reference.as_ref()) {
                reference.dispose();
                return Err(Self::reject(JsonEditingErrorCode::ErrorInvalidFile));
            }

            Ok(reference)
        })
    }

    fn reject(code: JsonEditingErrorCode) -> Error {
        json_editing_error(Self::to_error_message(code), code)
    }

    fn to_error_message(error: JsonEditingErrorCode) -> String {
        match error {
            // User issues
            JsonEditingErrorCode::ErrorInvalidFile => localize("errorInvalidFile", "Unable to write into the file. Please open the file to correct errors/warnings in the file and try again.", &[]),
        }
    }
}

impl IJsonEditingService for JsonEditingService {
    fn write(&self, resource: &URI, values: Vec<JsonValue>, _save: bool) -> Promise<()> {
        let (this, resource) = (self.rc(), resource.clone());
        // queue up writes to prevent race conditions
        self.queue.queue(move || this.do_write_configuration(resource, values))
    }
}
