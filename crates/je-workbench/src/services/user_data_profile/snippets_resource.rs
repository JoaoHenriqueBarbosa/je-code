//! Porte de `src/vs/workbench/services/userDataProfile/browser/snippetsResource.ts`: a leitura e a escrita dos snippets
//! de um perfil (`SnippetsResource`) e a inicialização dos snippets do perfil corrente (`SnippetsResourceInitializer`).
//!
//! Notas de tradução:
//! - **Injeção.** Os parâmetros decorados do construtor são o `Deps` do [`Injectable`], na mesma ordem; os dois
//!   construtores não têm argumentos estáticos.
//! - **`async`.** Cada método `async` é um [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`.
//! - **`JSON.parse`.** É o `parse_json` do sincronizador. Os valores de `snippets` que não são texto não têm tradução
//!   fiel para `VSBuffer.fromString` e rejeitam com `TypeError`; a chave é lida como texto (uma chave com surrogate
//!   isolado perde o surrogate).
//! - **`excluded`.** O `ResourceSet` opcional é compartilhado (`Rc<RefCell<..>>`), ver [`IProfileResource`].
//! - **Fora desta porta.** `SnippetsResourceTreeItem` (a árvore do editor de perfis) depende de `IProfileResourceTreeItem`,
//!   do `API_OPEN_EDITOR_COMMAND_ID` e do `ITreeItem` com `checkbox` vivo, que entram com o editor de perfis.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::json::{Object, Value};
use je_base::map::ResourceSet;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_instantiation::Injectable;
use je_platform::files::file_service::FileContentSource;
use je_platform::files::files::{FileOperationError, FileOperationResult};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::LogService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::user_data_profile::UserDataProfile;
use je_platform::user_data_sync::abstract_synchronizer::{parse_json, stringify_json};

use super::user_data_profile::{IProfileResource, IProfileResourceInitializer, IUserDataProfileService};

/// Resolves the target resource for a snippet `key` relative to `snippetsHome`, ensuring the result stays contained
/// within `snippetsHome`. The `key` originates from an imported (and therefore untrusted) profile, so it can contain
/// path traversal segments (e.g. `../../../foo`). `joinPath` normalizes such segments, which would otherwise allow
/// writing files outside the profile directory. Returns `None` when the resolved resource escapes `snippetsHome`.
fn to_snippet_resource(ext_uri: &ExtUri, snippets_home: &URI, key: &str) -> Option<URI> {
    let resource = ext_uri.join_path(snippets_home, &[key]);
    if !ext_uri.is_equal_or_parent(&resource, snippets_home, false, false) || ext_uri.is_equal(Some(&resource), Some(snippets_home), false) {
        return None;
    }
    Some(resource)
}

/// `for (const key in snippetsContent.snippets)` sobre o `JSON.parse(content)`: as entradas na ordem do objeto.
fn snippets_entries(content: &str) -> Result<Vec<(JsString, Value)>, Error> {
    match parse_json(content)? {
        Value::Null => Err(Error::type_error("Cannot read properties of null (reading 'snippets')")),
        Value::Object(object) => Ok(match object.get("snippets") {
            Some(Value::Object(snippets)) => snippets.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
            _ => Vec::new(),
        }),
        _ => Ok(Vec::new()),
    }
}

/// O laço de escrita do `apply` e do `initialize`: grava cada snippet cuja chave fica dentro de `snippetsHome` e avisa
/// no log os que escapam. `snippets_home` é lido a cada volta, como o original lê o perfil corrente.
fn write_snippets(
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: Rc<dyn LogService>,
    snippets_home: impl Fn() -> URI + 'static,
    content: String,
    owner: &'static str,
) -> Promise<()> {
    async_fn(async move {
        for (key, value) in snippets_entries(&content)? {
            let key = key.to_string_lossy();
            let Some(resource) = to_snippet_resource(&uri_identity_service.ext_uri(), &snippets_home(), &key) else {
                log_service.warn(&format!("{owner}: Ignoring snippet with key '{key}' as it escapes the snippets folder."));
                continue;
            };
            let Value::String(text) = value else {
                return Err(Error::type_error(format!("Invalid content for the snippet '{key}'")));
            };
            file_service.write_file(&resource, FileContentSource::Buffer(VSBuffer::from_string(&text.to_string_lossy())), None).await?;
        }
        Ok(())
    })
}

/// `SnippetsResourceInitializer`.
pub struct SnippetsResourceInitializer {
    user_data_profile_service: Rc<dyn IUserDataProfileService>,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for SnippetsResourceInitializer {
    type Args = ();
    type Deps = (Rc<dyn IUserDataProfileService>, Rc<dyn IFileService>, Rc<dyn IUriIdentityService>, Rc<dyn LogService>);

    fn create(_: (), (user_data_profile_service, file_service, uri_identity_service, log_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { user_data_profile_service, file_service, uri_identity_service, log_service })
    }
}

impl IProfileResourceInitializer for SnippetsResourceInitializer {
    fn initialize(&self, content: &str) -> Promise<()> {
        let user_data_profile_service = self.user_data_profile_service.clone();
        write_snippets(
            self.file_service.clone(),
            self.uri_identity_service.clone(),
            self.log_service.clone(),
            move || user_data_profile_service.current_profile().snippets_home,
            content.to_owned(),
            "SnippetsResourceInitializer",
        )
    }
}

/// `SnippetsResource`.
pub struct SnippetsResource {
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    log_service: Rc<dyn LogService>,
}

impl Injectable for SnippetsResource {
    type Args = ();
    type Deps = (Rc<dyn IFileService>, Rc<dyn IUriIdentityService>, Rc<dyn LogService>);

    fn create(_: (), (file_service, uri_identity_service, log_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { file_service, uri_identity_service, log_service })
    }
}

impl SnippetsResource {
    /// `getSnippets`: o conteúdo de cada arquivo de snippet do perfil, pela chave relativa a `snippetsHome`.
    fn get_snippets(&self, profile: &UserDataProfile, excluded: Option<Rc<RefCell<ResourceSet>>>) -> Promise<Object> {
        let snippets_resources = self.get_snippets_resources(profile, excluded);
        let file_service = self.file_service.clone();
        let uri_identity_service = self.uri_identity_service.clone();
        let snippets_home = profile.snippets_home.clone();
        async_fn(async move {
            let mut snippets = Object::new();
            for resource in snippets_resources.await? {
                let key = uri_identity_service.ext_uri().relative_path(&snippets_home, &resource).unwrap_or_default();
                let content = file_service.read_file(&resource, None, None).await?;
                snippets.insert(JsString::from(key), Value::String(JsString::from(content.value.to_string())));
            }
            Ok(snippets)
        })
    }

    /// `getSnippetsResources`: os arquivos `.json` e `.code-snippets` de `snippetsHome`, sem os de `excluded`.
    pub fn get_snippets_resources(&self, profile: &UserDataProfile, excluded: Option<Rc<RefCell<ResourceSet>>>) -> Promise<Vec<URI>> {
        let file_service = self.file_service.clone();
        let uri_identity_service = self.uri_identity_service.clone();
        let snippets_home = profile.snippets_home.clone();
        async_fn(async move {
            let stat = match file_service.resolve(&snippets_home, None).await {
                Ok(stat) => stat,
                // No snippets
                Err(error) if FileOperationError::from_error(&error).is_some_and(|e| e.file_operation_result == FileOperationResult::FileNotFound) => {
                    return Ok(Vec::new());
                }
                Err(error) => return Err(error),
            };
            let ext_uri = uri_identity_service.ext_uri();
            let mut snippets = Vec::new();
            for child in stat.children.unwrap_or_default() {
                let resource = child.resource;
                if excluded.as_ref().is_some_and(|excluded| excluded.borrow().has(&resource)) {
                    continue;
                }
                let extension = ext_uri.extname(&resource);
                if extension == ".json" || extension == ".code-snippets" {
                    snippets.push(resource);
                }
            }
            Ok(snippets)
        })
    }
}

impl IProfileResource for SnippetsResource {
    fn get_content(&self, profile: &UserDataProfile, excluded: Option<Rc<RefCell<ResourceSet>>>) -> Promise<String> {
        let snippets = self.get_snippets(profile, excluded);
        async_fn(async move {
            let mut content = Object::new();
            content.insert(JsString::from("snippets"), Value::Object(snippets.await?));
            Ok(stringify_json(&Value::Object(content)))
        })
    }

    fn apply(&self, content: &str, profile: &UserDataProfile) -> Promise<()> {
        let snippets_home = profile.snippets_home.clone();
        write_snippets(
            self.file_service.clone(),
            self.uri_identity_service.clone(),
            self.log_service.clone(),
            move || snippets_home.clone(),
            content.to_owned(),
            "SnippetsResource",
        )
    }
}
