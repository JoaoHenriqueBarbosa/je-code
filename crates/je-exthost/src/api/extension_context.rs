//! O `ExtensionContext` que o `activate` de cada extensão recebe: `_loadExtensionContext` de
//! `workbench/api/common/extHostExtensionService.ts`, com `globalState`, `workspaceState` (o `ExtensionGlobalMemento` e o
//! `ExtensionMemento` de `extHostMemento.ts` sobre o `ExtHostStorage`), `secrets` (o `ExtensionSecrets` sobre o `ExtHostSecretState`),
//! `subscriptions`, os URIs e caminhos de extensão, armazenamento e log, `extensionMode`, `extension` e
//! `environmentVariableCollection`.
//!
//! Divergências:
//! - O original devolve o contexto congelado com acessores (`get extensionUri()`, `get extension()`...); aqui os valores são lidos uma
//!   vez, na montagem do contexto (o `extension` é a instância de `Extension` que carrega só o identificador, ver `api::extensions`).
//! - Os arrays e objetos que `Memento.get` devolve são cópias novas a cada chamada (o motor entrega objeto por cópia); o original
//!   devolve a mesma referência guardada.
//! - O `ExtHostStorage` e o `ExtHostSecretState` só existem se o binder do host registrou `MainThreadStorage` e `MainThreadSecretState`
//!   (o host sobe sem eles em quem não os tem): sem eles `globalState` e `workspaceState` (ou `secrets`) ficam fora do contexto. Do
//!   mesmo modo, os URIs de armazenamento e de log dependem de [`crate::in_process::IMainThreadBinder::extension_storage_environment`]
//!   (`storageUri` é `undefined` sem workspace, como no original) e `environmentVariableCollection` do `MainThreadTerminalService`.
//! - `languageModelAccessInformation`, `extensionRuntime` e `messagePassingProtocol` (propostas de API e o `ExtHostLanguageModels`, fora
//!   do recorte do porte) não existem no contexto. `extensionMode` nunca é `Test` (o host não tem `extensionTestsLocationURI`) e
//!   `extension.extensionKind` é sempre `UI`.

use std::path::Path;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_workbench::api::common::ext_host_custom_editors::IExtensionStoragePaths;
use je_workbench::api::common::ext_host_memento::{ExtensionGlobalMemento, ExtensionMemento};
use je_workbench::api::common::ext_host_secrets::ExtensionSecrets;
use je_workbench::api::common::ext_host_storage::{ExtHostSecretState, ExtHostStorage};
use je_workbench::services::extensions::extension_description_registry::SharedExtensionDescription;
use zjsc::api::embed_node::{JsData, JsHandle, NodeRunner};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::environment_variable_collection;
use crate::api::events::event_function;
use crate::api::extensions;
use crate::convert::{js_to_dynamic, string_arg, uri_to_js};
use crate::extension_storage_paths::ExtensionStoragePaths;

/// `ExtensionMode.Production` e `ExtensionMode.Development`.
const EXTENSION_MODE_PRODUCTION: f64 = 1.0;
const EXTENSION_MODE_DEVELOPMENT: f64 = 2.0;

/// Os serviços do host de que o contexto depende; cada um é opcional (ver as divergências do módulo).
#[derive(Clone, Default)]
pub struct ExtensionContextServices {
    pub storage: Option<Rc<ExtHostStorage>>,
    pub secret_state: Option<Rc<ExtHostSecretState>>,
    pub storage_paths: Option<Rc<ExtensionStoragePaths>>,
    /// A classe `EnvironmentVariableCollection` foi definida (o host tem o `ExtHostTerminalService`).
    pub environment_variable_collection: bool,
}

/// O que `_loadExtensionContext` entrega: o contexto, o array `subscriptions` que a extensão preenche e o `extensionInternalStore`
/// (os mementos e os segredos, descartados com a extensão).
#[derive(Clone)]
pub struct LoadedContext {
    pub context: Dynamic,
    pub subscriptions: Rc<JsHandle>,
    pub internal_store: Vec<Rc<dyn IDisposable>>,
}

/// As funções `keys`, `get` e `update` do `vscode.Memento` (e `setKeysForSync` do global) como campos do objeto.
fn memento_to_js(runner: &NodeRunner, memento: &Rc<ExtensionMemento>, global: Option<&Rc<ExtensionGlobalMemento>>) -> JsData {
    let keys = {
        let memento = memento.clone();
        runner.make_function("keys", 0, Rc::new(move |_| Ok(JsData::Array(memento.keys().into_iter().map(JsData::Str).collect()))))
    };
    let get = {
        let memento = memento.clone();
        runner.make_function(
            "get",
            2,
            Rc::new(move |args| {
                let key = string_arg(args, 0, "Memento.get: a chave")?;
                let default_value = args.get(1).map(js_to_dynamic).filter(|value| !value.is_undefined());
                Ok(memento.get(&key, default_value).map_or(JsData::Undefined, |value| crate::convert::dynamic_to_js(&value)))
            }),
        )
    };
    let update = {
        let (memento, runner) = (memento.clone(), runner.clone());
        runner.clone().make_function(
            "update",
            2,
            Rc::new(move |args| {
                let key = string_arg(args, 0, "Memento.update: a chave")?;
                let value = args.get(1).map_or(Dynamic::Undefined, js_to_dynamic);
                Ok(promise_to_js(&runner, memento.update(&key, value), |()| JsData::Undefined))
            }),
        )
    };
    let mut fields = vec![
        ("keys".to_owned(), JsData::Handle(Rc::new(keys))),
        ("get".to_owned(), JsData::Handle(Rc::new(get))),
        ("update".to_owned(), JsData::Handle(Rc::new(update))),
    ];
    if let Some(global) = global {
        let global = global.clone();
        let set_keys_for_sync = runner.make_function(
            "setKeysForSync",
            1,
            Rc::new(move |args| {
                let Some(JsData::Array(items)) = args.first() else { return Err("Memento.setKeysForSync: as chaves precisam ser um array".to_owned()) };
                global.set_keys_for_sync(items.iter().filter_map(|item| if let JsData::Str(key) = item { Some(key.clone()) } else { None }).collect());
                Ok(JsData::Undefined)
            }),
        );
        fields.push(("setKeysForSync".to_owned(), JsData::Handle(Rc::new(set_keys_for_sync))));
    }
    JsData::Object(fields)
}

/// O `vscode.SecretStorage`: `get`, `store`, `delete`, `keys` e `onDidChange`.
fn secrets_to_js(runner: &NodeRunner, table: &Rc<DisposableTable>, secrets: &Rc<ExtensionSecrets>) -> JsData {
    let get = {
        let (secrets, runner) = (secrets.clone(), runner.clone());
        runner.clone().make_function(
            "get",
            1,
            Rc::new(move |args| {
                let key = string_arg(args, 0, "SecretStorage.get: a chave")?;
                Ok(promise_to_js(&runner, secrets.get(&key), |value| value.clone().map_or(JsData::Undefined, JsData::Str)))
            }),
        )
    };
    let store = {
        let (secrets, runner) = (secrets.clone(), runner.clone());
        runner.clone().make_function(
            "store",
            2,
            Rc::new(move |args| {
                let key = string_arg(args, 0, "SecretStorage.store: a chave")?;
                let value = string_arg(args, 1, "SecretStorage.store: o valor")?;
                Ok(promise_to_js(&runner, secrets.store(&key, &value), |()| JsData::Undefined))
            }),
        )
    };
    let delete = {
        let (secrets, runner) = (secrets.clone(), runner.clone());
        runner.clone().make_function(
            "delete",
            1,
            Rc::new(move |args| {
                let key = string_arg(args, 0, "SecretStorage.delete: a chave")?;
                Ok(promise_to_js(&runner, secrets.delete(&key), |()| JsData::Undefined))
            }),
        )
    };
    let keys = {
        let (secrets, runner) = (secrets.clone(), runner.clone());
        runner.clone().make_function(
            "keys",
            0,
            Rc::new(move |_| Ok(promise_to_js(&runner, secrets.keys(), |keys| JsData::Array(keys.iter().cloned().map(JsData::Str).collect())))),
        )
    };
    JsData::Object(vec![
        ("get".to_owned(), JsData::Handle(Rc::new(get))),
        ("store".to_owned(), JsData::Handle(Rc::new(store))),
        ("delete".to_owned(), JsData::Handle(Rc::new(delete))),
        ("keys".to_owned(), JsData::Handle(Rc::new(keys))),
        ("onDidChange".to_owned(), event_function(runner, table, "onDidChange", secrets.on_did_change(), |key| JsData::Object(vec![("key".to_owned(), JsData::Str(key.clone()))]))),
    ])
}

/// `_loadExtensionContext(extensionDescription, extensionInternalStore)`: espera os mementos ficarem prontos e monta o contexto.
pub fn load_extension_context(
    runner: &NodeRunner,
    services: &ExtensionContextServices,
    table: &Rc<DisposableTable>,
    extension_description: &SharedExtensionDescription,
) -> Promise<LoadedContext> {
    let runner = runner.clone();
    let table = table.clone();
    let description = extension_description.clone();
    let storage = services.storage.clone();
    let secret_state = services.secret_state.clone();
    let storage_paths = services.storage_paths.clone();
    let has_environment_variable_collection = services.environment_variable_collection;
    async_fn(async move {
        let identifier = description.identifier.value.clone();
        let mut internal_store: Vec<Rc<dyn IDisposable>> = Vec::new();
        let global_state = storage.as_ref().map(|storage| ExtensionGlobalMemento::new(&identifier, &description.manifest.version, storage.clone()));
        let workspace_state = storage.as_ref().map(|storage| ExtensionMemento::new(&identifier, false, storage.clone()));
        let secrets = secret_state.as_ref().map(|secret_state| ExtensionSecrets::new(&identifier, secret_state.clone()));
        if let Some(global_state) = &global_state {
            internal_store.push(global_state.clone());
        }
        if let Some(workspace_state) = &workspace_state {
            internal_store.push(workspace_state.clone());
        }
        if let Some(secrets) = &secrets {
            internal_store.push(secrets.clone());
        }

        if let Some(global_state) = &global_state {
            global_state.when_ready().await?;
        }
        if let Some(workspace_state) = &workspace_state {
            workspace_state.when_ready().await?;
        }

        let subscriptions = Rc::new(runner.eval("[]", "je-exthost/subscriptions.js").map_err(Error::new)?);
        let extension_uri = je_base::uri::URI::parse(&description.extension_location, false);
        let extension_path = je_platform::resources::fs_path(&extension_uri);
        let as_absolute_path = {
            let base = extension_path.clone();
            Rc::new(runner.make_function(
                "asAbsolutePath",
                1,
                Rc::new(move |args| match args.first() {
                    Some(JsData::Str(relative)) => Ok(JsData::Str(Path::new(&base).join(relative).to_string_lossy().into_owned())),
                    _ => Err("asAbsolutePath: o caminho relativo precisa ser texto".to_owned()),
                }),
            ))
        };

        let mut fields: Vec<(String, JsData)> = Vec::new();
        if let Some(global_state) = &global_state {
            fields.push(("globalState".to_owned(), memento_to_js(&runner, &global_state.memento().clone(), Some(global_state))));
        }
        if let Some(workspace_state) = &workspace_state {
            fields.push(("workspaceState".to_owned(), memento_to_js(&runner, workspace_state, None)));
        }
        if let Some(secrets) = &secrets {
            fields.push(("secrets".to_owned(), secrets_to_js(&runner, &table, secrets)));
        }
        fields.push(("subscriptions".to_owned(), JsData::Handle(subscriptions.clone())));
        fields.push(("extensionUri".to_owned(), uri_to_js(&extension_uri)));
        fields.push(("extensionPath".to_owned(), JsData::Str(extension_path)));
        fields.push(("asAbsolutePath".to_owned(), JsData::Handle(as_absolute_path)));
        if let Some(paths) = &storage_paths {
            let storage_uri = paths.workspace_value(&description);
            let global_storage_uri = paths.global_value(&description);
            let log_uri = paths.log_uri(&description);
            let fs_path = |uri: &je_base::uri::URI| JsData::Str(je_platform::resources::fs_path(uri));
            fields.push(("storagePath".to_owned(), storage_uri.as_ref().map_or(JsData::Undefined, fs_path)));
            fields.push(("globalStoragePath".to_owned(), fs_path(&global_storage_uri)));
            fields.push(("logPath".to_owned(), fs_path(&log_uri)));
            fields.push(("logUri".to_owned(), uri_to_js(&log_uri)));
            fields.push(("storageUri".to_owned(), storage_uri.as_ref().map_or(JsData::Undefined, uri_to_js)));
            fields.push(("globalStorageUri".to_owned(), uri_to_js(&global_storage_uri)));
        }
        let extension_mode = if description.is_under_development { EXTENSION_MODE_DEVELOPMENT } else { EXTENSION_MODE_PRODUCTION };
        fields.push(("extensionMode".to_owned(), JsData::Num(extension_mode)));
        fields.push(("extension".to_owned(), extensions::instance(&identifier)));
        if has_environment_variable_collection {
            fields.push(("environmentVariableCollection".to_owned(), environment_variable_collection::instance(&identifier)));
        }
        Ok(LoadedContext { context: js_to_dynamic(&JsData::Object(fields)), subscriptions, internal_store })
    })
}
