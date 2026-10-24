//! Porte parcial de `src/vs/workbench/contrib/files/browser/editors/fileEditorHandler.ts`: o `FileEditorInputSerializer`
//! (que guarda o recurso, o recurso preferido, o nome, a descrição, a codificação e a linguagem preferidos de uma
//! `FileEditorInput` para restaurar o editor). O `FileEditorWorkingCopyEditorHandler` entra com o consumidor dele (o
//! registro de contribuições do workbench, que ainda não foi portado).
//!
//! Divergências:
//! - **Injeção.** O serializador é um [`Injectable`] sem argumentos estáticos nem serviços.
//! - **`IInstantiationService.invokeFunction`** do `deserialize` é [`InstantiationService::invoke_function`], e o
//!   `accessor.get(ITextEditorService)` é o `get` do [`ServicesAccessor`](je_instantiation::ServicesAccessor).
//! - **JSON.** `JSON.stringify` do objeto serializado é o [`json_stringify`] do [`Dynamic`] (que omite os valores
//!   `undefined`, como o JavaScript); `JSON.parse` é o `serde_json`, e um texto inválido lança, como o `JSON.parse`.
//!   `resource.toJSON()` e `URI.revive` são o `uri_to_dynamic` do `query_builder` e a reconstrução do `UriState`.
//!   O `preferredResourceJSON` guarda o próprio URI no original, que o `JSON.stringify` serializa por `toJSON`.
//! - **`editorInput as FileEditorInput`** é o `downcast_ref` por [`EditorInput::as_any`].

use std::rc::Rc;

use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{Error, throw};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::resources::ext_uri;

use crate::common::editor::editor_factory_registry::IEditorSerializer;
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputRef, UntypedEditorInput};
use crate::contrib::files::browser::editors::file_editor_input::FileEditorInput;
use crate::services::search::query_builder::uri_to_dynamic;
use crate::services::textfile::text_editor_service::{I_TEXT_EDITOR_SERVICE, ITextEditorService};
use crate::services::textfile::textfiles::IEncodingSupport;
use crate::services::untitled::untitled_text_editor_handler::revive_uri;

/// `Option<string>` como o valor `string | undefined` de um campo serializado.
fn optional_string(value: Option<String>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::from)
}

/// O campo de texto do objeto serializado, ou `None` quando não existe.
fn string_field(serialized: &serde_json::Value, key: &str) -> Option<String> {
    serialized.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

/// `FileEditorInputSerializer`.
pub struct FileEditorInputSerializer;

impl Injectable for FileEditorInputSerializer {
    type Args = ();
    type Deps = ();

    fn create((): Self::Args, (): Self::Deps) -> Rc<Self> {
        Rc::new(Self)
    }
}

impl IEditorSerializer for FileEditorInputSerializer {
    fn can_serialize(&self, _editor_input: &EditorInputRef) -> bool {
        true
    }

    fn serialize(&self, editor_input: &EditorInputRef) -> Option<String> {
        let file_editor_input = editor_input.as_any().downcast_ref::<FileEditorInput>()?;
        let resource: URI = file_editor_input.resource.clone();
        let preferred_resource = file_editor_input.preferred_resource().unwrap_or_else(|| resource.clone());

        let serialized_file_editor_input = Dynamic::object([
            ("resourceJSON", uri_to_dynamic(&resource)),
            // only storing preferredResource if it differs from the resource
            (
                "preferredResourceJSON",
                if ext_uri().is_equal(Some(&resource), Some(&preferred_resource), false) { Dynamic::Undefined } else { uri_to_dynamic(&preferred_resource) },
            ),
            ("name", optional_string(file_editor_input.get_preferred_name())),
            ("description", optional_string(file_editor_input.get_preferred_description())),
            ("encoding", optional_string(IEncodingSupport::get_encoding(file_editor_input))),
            // only using the preferred user associated language here if available to not store redundant data
            // (`modeId` should be `languageId` but is kept for backwards compatibility)
            ("modeId", optional_string(file_editor_input.get_preferred_language_id())),
        ]);

        json_stringify(&serialized_file_editor_input)
    }

    fn deserialize(&self, instantiation_service: &InstantiationService, serialized_editor_input: &str) -> Option<EditorInputRef> {
        instantiation_service.invoke_function(|accessor| {
            let serialized: serde_json::Value = match serde_json::from_str(serialized_editor_input) {
                Ok(serialized) => serialized,
                Err(error) => throw(Error::named("SyntaxError", error.to_string())),
            };
            let resource = serialized.get("resourceJSON").filter(|resource_json| resource_json.is_object()).map(revive_uri);
            let preferred_resource = serialized.get("preferredResourceJSON").filter(|resource_json| resource_json.is_object()).map(revive_uri);

            let file_editor_input = accessor.get(I_TEXT_EDITOR_SERVICE).create_text_editor(UntypedEditorInput {
                resource,
                label: string_field(&serialized, "name"),
                description: string_field(&serialized, "description"),
                encoding: string_field(&serialized, "encoding"),
                language_id: string_field(&serialized, "modeId"),
                force_file: Some(true),
                ..UntypedEditorInput::default()
            });
            if let Some(preferred_resource) = preferred_resource
                && let Some(file_editor_input) = file_editor_input.as_any().downcast_ref::<FileEditorInput>()
            {
                file_editor_input.set_preferred_resource(&preferred_resource);
            }

            Some(file_editor_input)
        })
    }
}
