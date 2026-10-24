//! Porte de `src/vs/workbench/services/untitled/common/untitledTextEditorHandler.ts`: o serializador de entradas de editor
//! sem título (`UntitledTextEditorInputSerializer`, que guarda o recurso, a linguagem e o encoding para o hot exit) e o
//! handler que reabre o editor de uma working copy sem título (`UntitledTextEditorWorkingCopyEditorHandler`).
//!
//! Divergências:
//! - **Injeção.** Os dois são [`Injectable`] sem argumentos estáticos e com os serviços decorados na ordem do construtor.
//! - **`IInstantiationService.invokeFunction`** do `deserialize` é [`InstantiationService::invoke_function`], e o
//!   `accessor.get(ITextEditorService)` é o `get` do [`ServicesAccessor`](je_instantiation::ServicesAccessor).
//! - **JSON.** `JSON.stringify` do objeto serializado é o [`json_stringify`] do [`Dynamic`] (que omite os valores
//!   `undefined`, como o JavaScript); `JSON.parse` é o `serde_json`, e um texto inválido lança, como o `JSON.parse`.
//!   `resource.toJSON()` e `URI.revive` são o `uri_to_dynamic` do `query_builder` e a reconstrução do `UriState`.
//! - **`handles` e `createEditor`.** `boolean | Promise<boolean>` e `EditorInput | Promise<EditorInput>` são
//!   [`MaybePromise`]: esta classe devolve sempre o valor direto.
//! - **`IWorkbenchContribution`.** O contrato de contribuição ainda não foi portado: a classe só traz o `ID` e, como todo
//!   `Disposable`, o `dispose`.
//! - **`editorInput as UntitledTextEditorInput`** é o `downcast_ref` por [`EditorInput::as_any`].

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{Error, throw};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::promise::MaybePromise;
use je_base::resources::to_local_resource;
use je_base::uri::{MARSHALLED_ID_URI, URI, UriComponents, UriState};
use je_editor::modes_registry::PLAINTEXT_LANGUAGE_ID;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::resources::ext_uri;

use crate::common::editor::editor_factory_registry::IEditorSerializer;
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputRef, UntypedEditorInput};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::files_configuration::files_configuration_service::IFilesConfigurationService;
use crate::services::path::path_service::IPathService;
use crate::services::search::query_builder::uri_to_dynamic;
use crate::services::textfile::text_editor_service::{I_TEXT_EDITOR_SERVICE, ITextEditorService};
use crate::services::textfile::textfiles::IEncodingSupport;
use crate::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use crate::services::working_copy::working_copy::{NO_TYPE_ID, WorkingCopyIdentifier};
use crate::services::working_copy::working_copy_editor_service::{IWorkingCopyEditorHandler, IWorkingCopyEditorService};

/// `Option<string>` como o valor `string | undefined` de um campo serializado.
fn optional_string(value: Option<String>) -> Dynamic {
    value.map_or(Dynamic::Undefined, Dynamic::from)
}

/// `URI.revive(resourceJSON)`: a reconstrução do que `resource.toJSON()` serializou.
fn revive_uri(resource_json: &serde_json::Value) -> URI {
    let component = |key: &str| resource_json.get(key).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).map(JsString::from);

    let state = UriState {
        mid: Some(MARSHALLED_ID_URI),
        external: component("external"),
        fs_path: component("fsPath"),
        sep: (resource_json.get("_sep").and_then(serde_json::Value::as_u64) == Some(1)).then_some(1),
        components: UriComponents {
            scheme: component("scheme"),
            authority: component("authority"),
            path: component("path"),
            query: component("query"),
            fragment: component("fragment"),
        },
    };

    URI::revive(&state, ProcessEnvironment.platform())
}

/// `UntitledTextEditorInputSerializer`.
pub struct UntitledTextEditorInputSerializer {
    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    path_service: Rc<dyn IPathService>,
}

impl Injectable for UntitledTextEditorInputSerializer {
    type Args = ();
    type Deps = (Rc<dyn IFilesConfigurationService>, Rc<dyn IWorkbenchEnvironmentService>, Rc<dyn IPathService>);

    fn create((): Self::Args, (files_configuration_service, environment_service, path_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { files_configuration_service, environment_service, path_service })
    }
}

impl IEditorSerializer for UntitledTextEditorInputSerializer {
    fn can_serialize(&self, editor_input: &EditorInputRef) -> bool {
        self.files_configuration_service.is_hot_exit_enabled() && !editor_input.is_disposed()
    }

    fn serialize(&self, editor_input: &EditorInputRef) -> Option<String> {
        if !self.can_serialize(editor_input) {
            return None;
        }

        let untitled_text_editor_input = editor_input.as_any().downcast_ref::<UntitledTextEditorInput>()?;

        let mut resource = untitled_text_editor_input.resource.clone();
        if untitled_text_editor_input.has_associated_file_path() {
            resource = to_local_resource(&resource, self.environment_service.remote_authority().as_deref(), &self.path_service.default_uri_scheme()); // untitled with associated file path use the local schema
        }

        // Language: only remember language if it is either specific (not text)
        // or if the language was explicitly set by the user. We want to preserve
        // this information across restarts and not set the language unless
        // this is the case.
        let language_id_candidate = untitled_text_editor_input.get_language_id();
        let language_id = if language_id_candidate.as_deref() != Some(PLAINTEXT_LANGUAGE_ID) || untitled_text_editor_input.has_language_set_explicitly() {
            language_id_candidate
        } else {
            None
        };

        // `modeId` should be `languageId` but is kept for backwards compatibility
        let serialized = Dynamic::object([
            ("resourceJSON", uri_to_dynamic(&resource)),
            ("modeId", optional_string(language_id)),
            ("encoding", optional_string(untitled_text_editor_input.get_encoding())),
        ]);

        json_stringify(&serialized)
    }

    fn deserialize(&self, instantiation_service: &InstantiationService, serialized_editor_input: &str) -> Option<EditorInputRef> {
        instantiation_service.invoke_function(|accessor| {
            let deserialized: serde_json::Value = match serde_json::from_str(serialized_editor_input) {
                Ok(deserialized) => deserialized,
                Err(error) => throw(Error::named("SyntaxError", error.to_string())),
            };
            let resource = deserialized.get("resourceJSON").filter(|resource_json| resource_json.is_object()).map(revive_uri);
            let language_id = deserialized.get("modeId").and_then(serde_json::Value::as_str).map(str::to_string);
            let encoding = deserialized.get("encoding").and_then(serde_json::Value::as_str).map(str::to_string);

            Some(accessor.get(I_TEXT_EDITOR_SERVICE).create_text_editor(UntypedEditorInput {
                resource,
                language_id,
                encoding,
                force_untitled: Some(true),
                ..UntypedEditorInput::default()
            }))
        })
    }
}

/// `UntitledTextEditorWorkingCopyEditorHandler`.
pub struct UntitledTextEditorWorkingCopyEditorHandler {
    base: Rc<Disposable>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    path_service: Rc<dyn IPathService>,
    text_editor_service: Rc<dyn ITextEditorService>,
    untitled_text_editor_service: Rc<dyn IUntitledTextEditorService>,
}

impl Injectable for UntitledTextEditorWorkingCopyEditorHandler {
    type Args = ();
    type Deps = (
        Rc<dyn IWorkingCopyEditorService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IPathService>,
        Rc<dyn ITextEditorService>,
        Rc<dyn IUntitledTextEditorService>,
    );

    fn create(
        (): Self::Args,
        (working_copy_editor_service, environment_service, path_service, text_editor_service, untitled_text_editor_service): Self::Deps,
    ) -> Rc<Self> {
        let this = Rc::new(Self { base: Disposable::new(), environment_service, path_service, text_editor_service, untitled_text_editor_service });

        let handler: Rc<dyn IWorkingCopyEditorHandler> = this.clone();
        this.base.register(working_copy_editor_service.register_handler(handler));

        this
    }
}

impl UntitledTextEditorWorkingCopyEditorHandler {
    pub const ID: &'static str = "workbench.contrib.untitledTextEditorWorkingCopyEditorHandler";

    fn handles_working_copy(&self, working_copy: &WorkingCopyIdentifier) -> bool {
        *working_copy.resource.scheme() == schemas::UNTITLED && working_copy.type_id == NO_TYPE_ID
    }
}

impl IDisposable for UntitledTextEditorWorkingCopyEditorHandler {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkingCopyEditorHandler for UntitledTextEditorWorkingCopyEditorHandler {
    fn handles(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<bool> {
        MaybePromise::Value(self.handles_working_copy(working_copy))
    }

    fn is_open(&self, working_copy: &WorkingCopyIdentifier, editor: &EditorInputRef) -> bool {
        if !self.handles_working_copy(working_copy) {
            return false;
        }

        // Also match editors that are not UntitledTextEditorInput but share the
        // same untitled resource (e.g. a text-based custom editor that wraps the
        // same underlying text model). Without this, the backup restorer would
        // create a duplicate UntitledTextEditorInput alongside the custom editor.
        ext_uri().is_equal(Some(&working_copy.resource), editor.resource().as_ref(), false)
    }

    fn create_editor(&self, working_copy: &WorkingCopyIdentifier) -> MaybePromise<EditorInputRef> {
        // If the untitled has an associated resource,
        // ensure to restore the local resource it had
        let editor_input_resource = if self.untitled_text_editor_service.is_untitled_with_associated_resource(&working_copy.resource) {
            to_local_resource(&working_copy.resource, self.environment_service.remote_authority().as_deref(), &self.path_service.default_uri_scheme())
        } else {
            working_copy.resource.clone()
        };

        MaybePromise::Value(self.text_editor_service.create_text_editor(UntypedEditorInput {
            resource: Some(editor_input_resource),
            force_untitled: Some(true),
            ..UntypedEditorInput::default()
        }))
    }
}
