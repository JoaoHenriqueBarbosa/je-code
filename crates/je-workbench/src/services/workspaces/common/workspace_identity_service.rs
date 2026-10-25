//! Porte de `src/vs/workbench/services/workspaces/common/workspaceIdentityService.ts`: o serviço que descreve as pastas do
//! workspace pela identidade de edit session (a URL remota do repositório, por exemplo) e, dado o estado de outra máquina,
//! decide se ele serve para o workspace atual e como trocar os URIs das pastas dela pelos das pastas daqui.
//!
//! Notas de tradução:
//! - **`readonly _serviceBrand`** não existe: o identificador do serviço é o [`I_WORKSPACE_IDENTITY_SERVICE`].
//!   **`registerSingleton(..., InstantiationType.Delayed)`** não é portado: quem monta os serviços faz o registro.
//! - **`matches(...)`: `Promise<((obj: unknown) => unknown) | false>`** é um `Promise<Option<UriReplacer>>`: o `false` é
//!   `None`. O `UriReplacer` é o `uriReplacer(obj, depth = 0)`, que recebe o valor por valor e devolve o valor com os URIs
//!   trocados (o original muda arrays e objetos no lugar e devolve o mesmo objeto; só um URI na raiz, que o original não
//!   consegue trocar no lugar, sai trocado no retorno, então quem chama deve usar o retorno).
//! - **`unknown`** é o [`Marshalled`] (o JSON com `URI`, `VSBuffer` e `Uint8Array` que o `marshalling.parse` produz).
//!   `obj instanceof VSBuffer || obj instanceof Uint8Array` são as variantes `Buffer` e `Bytes`.
//! - **Uma string não vazia** cai no ramo `for (const key in obj)` do original, que atribui `obj[key]` num primitivo e, em
//!   modo estrito, lança `TypeError`; o porte lança o mesmo erro (quem chama, como o `WorkspaceStateSynchroniser`, o
//!   captura com `try`).
//! - **`{ [key: string]: string }`** (os dois dicionários de `matches`) é o [`Object`] do JSON, que mantém a ordem de
//!   propriedades do JavaScript (chaves de índice primeiro) e a posição da chave reatribuída.
//! - **`Map<IWorkspaceFolder, string>`** é um `Vec` de pares, na ordem de inserção (a chave é a identidade do objeto).
//! - **`await` em laço** é o `async_fn` com `.await?`.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::json::{Object, Value};
use je_base::marshalling::Marshalled;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::edit_sessions::{EditSessionIdentityMatch, IEditSessionIdentityService};
use je_platform::resources::ext_uri;
use je_platform::user_data_sync::user_data_sync::IWorkspaceStateFolder;
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};

/// O `(obj: unknown) => unknown` que `matches` devolve.
pub type UriReplacer = Rc<dyn Fn(Marshalled) -> Marshalled>;

/// `IWorkspaceIdentityService`.
pub trait IWorkspaceIdentityService {
    /// `Promise<((obj: unknown) => unknown) | false>`: `None` é o `false` (o estado não serve para este workspace).
    fn matches(&self, folders: Vec<IWorkspaceStateFolder>, cancellation_token: CancellationToken) -> Promise<Option<UriReplacer>>;

    fn get_workspace_state_folders(&self, cancellation_token: CancellationToken) -> Promise<Vec<IWorkspaceStateFolder>>;
}

service_id!(pub I_WORKSPACE_IDENTITY_SERVICE: dyn IWorkspaceIdentityService = "IWorkspaceIdentityService");

/// `WorkspaceIdentityService`.
pub struct WorkspaceIdentityService {
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    edit_session_identity_service: Rc<dyn IEditSessionIdentityService>,
}

impl WorkspaceIdentityService {
    pub fn new(workspace_context_service: Rc<dyn IWorkspaceContextService>, edit_session_identity_service: Rc<dyn IEditSessionIdentityService>) -> Self {
        Self { workspace_context_service, edit_session_identity_service }
    }
}

impl IWorkspaceIdentityService for WorkspaceIdentityService {
    fn get_workspace_state_folders(&self, cancellation_token: CancellationToken) -> Promise<Vec<IWorkspaceStateFolder>> {
        let workspace_context_service = self.workspace_context_service.clone();
        let edit_session_identity_service = self.edit_session_identity_service.clone();

        async_fn(async move {
            let mut workspace_state_folders = Vec::new();

            for workspace_folder in workspace_context_service.get_workspace().folders() {
                let workspace_folder_identity = edit_session_identity_service.get_edit_session_identifier(&workspace_folder, cancellation_token.clone()).await?;
                let Some(workspace_folder_identity) = workspace_folder_identity.filter(|identity| !identity.is_empty()) else {
                    continue;
                };
                workspace_state_folders.push(IWorkspaceStateFolder { resource_uri: folder_uri_text(&workspace_folder).to_string_lossy(), workspace_folder_identity });
            }

            Ok(workspace_state_folders)
        })
    }

    fn matches(&self, incoming_workspace_folders: Vec<IWorkspaceStateFolder>, cancellation_token: CancellationToken) -> Promise<Option<UriReplacer>> {
        let workspace_context_service = self.workspace_context_service.clone();
        let edit_session_identity_service = self.edit_session_identity_service.clone();

        async_fn(async move {
            let mut incoming_to_current_workspace_folder_uris = Object::new();

            let mut incoming_identities_to_incoming_workspace_folders = Object::new();
            for workspace_folder in &incoming_workspace_folders {
                incoming_identities_to_incoming_workspace_folders.insert(
                    JsString::from(workspace_folder.workspace_folder_identity.as_str()),
                    Value::String(JsString::from(workspace_folder.resource_uri.as_str())),
                );
            }

            // Precompute the identities of the current workspace folders
            let mut current_workspace_folders_to_identities: Vec<(WorkspaceFolder, String)> = Vec::new();
            for workspace_folder in workspace_context_service.get_workspace().folders() {
                let workspace_folder_identity = edit_session_identity_service.get_edit_session_identifier(&workspace_folder, cancellation_token.clone()).await?;
                let Some(workspace_folder_identity) = workspace_folder_identity.filter(|identity| !identity.is_empty()) else {
                    continue;
                };
                current_workspace_folders_to_identities.push((workspace_folder, workspace_folder_identity));
            }

            // Match the current workspace folders to the incoming workspace folders
            for (current_workspace_folder, current_workspace_folder_identity) in &current_workspace_folders_to_identities {
                // Happy case: identities do not need further disambiguation
                let incoming_workspace_folder = match incoming_identities_to_incoming_workspace_folders.get(current_workspace_folder_identity) {
                    Some(Value::String(incoming_folder)) if !incoming_folder.is_empty() => Some(incoming_folder.clone()),
                    _ => None,
                };
                if let Some(incoming_workspace_folder) = incoming_workspace_folder {
                    // There is an incoming workspace folder with the exact same identity as the current workspace folder
                    incoming_to_current_workspace_folder_uris.insert(incoming_workspace_folder, Value::String(folder_uri_text(current_workspace_folder)));
                    continue;
                }

                // Unhappy case: compare the identity of the current workspace folder to all incoming workspace folder identities
                let mut has_complete_match = false;
                for (incoming_identity, incoming_folder) in incoming_identities_to_incoming_workspace_folders.iter() {
                    let identity_match = edit_session_identity_service
                        .provide_edit_session_identity_match(
                            current_workspace_folder,
                            current_workspace_folder_identity,
                            &incoming_identity.to_string_lossy(),
                            cancellation_token.clone(),
                        )
                        .await?;
                    if identity_match == Some(EditSessionIdentityMatch::Complete) {
                        let incoming_folder = match incoming_folder {
                            Value::String(incoming_folder) => incoming_folder.clone(),
                            _ => JsString::new(),
                        };
                        incoming_to_current_workspace_folder_uris.insert(incoming_folder, Value::String(folder_uri_text(current_workspace_folder)));
                        has_complete_match = true;
                        break;
                    }
                }

                if has_complete_match {
                    continue;
                }

                return Ok(None);
            }

            let uri_replacer: UriReplacer = Rc::new(move |obj| replace_uris(obj, 0, &incoming_to_current_workspace_folder_uris));
            Ok(Some(uri_replacer))
        })
    }
}

/// `workspaceFolder.uri.toString()`.
fn folder_uri_text(workspace_folder: &WorkspaceFolder) -> JsString {
    workspace_folder.uri.to_string(false)
}

/// `convertUri`: o URI reposicionado sob a pasta atual que corresponde à pasta de onde ele veio, ou ele mesmo.
fn convert_uri(incoming_to_current_workspace_folder_uris: &Object, uri_to_convert: &URI) -> URI {
    let ext_uri = ext_uri();

    // Figure out which current folder the incoming URI is a child of
    for (incoming_folder_uri_key, current_workspace_folder_uri) in incoming_to_current_workspace_folder_uris.iter() {
        let incoming_folder_uri = URI::parse(incoming_folder_uri_key.clone(), false);
        if ext_uri.is_equal_or_parent(&incoming_folder_uri, uri_to_convert, false, false) {
            let Value::String(current_workspace_folder_uri) = current_workspace_folder_uri else {
                continue;
            };

            // Compute the relative file path section of the uri to convert relative to the folder it came from
            let relative_file_path = ext_uri.relative_path(&incoming_folder_uri, uri_to_convert);

            // Reparent the relative file path under the current workspace folder it belongs to
            if let Some(relative_file_path) = relative_file_path
                && !relative_file_path.is_empty()
            {
                return ext_uri.join_path(&URI::parse(current_workspace_folder_uri.clone(), false), &[relative_file_path.as_str()]);
            }
        }
    }

    // No conversion was possible; return the original URI
    uri_to_convert.clone()
}

/// A veracidade do JavaScript (`!obj`) de um valor marshalled.
fn is_falsy(obj: &Marshalled) -> bool {
    match obj {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(value) => !value,
        Marshalled::Number(value) => *value == 0.0 || value.is_nan(),
        Marshalled::String(value) => value.is_empty(),
        _ => false,
    }
}

/// `uriReplacer`: procura recursivamente os URIs no valor e os troca pelos das pastas atuais do workspace.
fn replace_uris(obj: Marshalled, depth: usize, incoming_to_current_workspace_folder_uris: &Object) -> Marshalled {
    if is_falsy(&obj) || depth > 200 {
        return obj;
    }

    match obj {
        Marshalled::Uri(uri) => Marshalled::Uri(convert_uri(incoming_to_current_workspace_folder_uris, &uri)),
        Marshalled::Array(items) => Marshalled::Array(items.into_iter().map(|item| replace_uris(item, depth + 1, incoming_to_current_workspace_folder_uris)).collect()),
        // walk object
        Marshalled::Object(entries) => {
            Marshalled::Object(entries.into_iter().map(|(key, value)| (key, replace_uris(value, depth + 1, incoming_to_current_workspace_folder_uris))).collect())
        }
        // `for (const key in "texto")` percorre os índices e `obj[key] = ...` num primitivo lança em modo estrito.
        Marshalled::String(text) => errors::throw(Error::named("TypeError", format!("Cannot assign to read only property '0' of string '{text}'"))),
        // `VSBuffer`, `Uint8Array` e os valores sem propriedades próprias enumeráveis (número, `true`, `Date`, `RegExp`).
        other => other,
    }
}
