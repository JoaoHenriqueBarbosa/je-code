//! Porte de `src/vs/workbench/services/extensions/common/extensionStorageMigration.ts`: a migração do armazenamento de uma
//! extensão para outro identificador (o estado, no armazenamento, e os arquivos, na pasta da extensão).
//!
//! Notas de tradução:
//! - **`instantiationService.invokeFunction(async serviceAccessor => ...)`.** O acessor só vale durante a invocação, e o corpo
//!   `async` do original lê todos os serviços antes do primeiro `await`. Aqui a invocação lê os oito serviços, na ordem do
//!   original, e a parte `async` (a movimentação dos arquivos) roda no [`async_fn`] sobre eles.
//! - **`catch (error)` e `(<FileSystemProviderError>error).code`.** É `to_file_system_provider_error_code` do erro, comparado
//!   com `FileNotFound`; `getErrorMessage(error)` é a mensagem do erro.
//! - **`storageService.store(storageMigratedKey, true, ...)`.** O `true` é o [`StorageValue::Bool`].

use je_base::async_::async_fn;
use je_base::uri::URI;
use je_instantiation::InstantiationService;
use je_platform::environment::environment::I_ENVIRONMENT_SERVICE;
use je_platform::files::files::{FileSystemProviderErrorCode, to_file_system_provider_error_code};
use je_platform::files::i_file_service::I_FILE_SERVICE;
use je_platform::log::{I_LOG_SERVICE, LogArg, Logger};
use je_platform::storage::{I_STORAGE_SERVICE, StorageScope, StorageTarget, StorageValue};
use je_platform::uri_identity::uri_identity::I_URI_IDENTITY_SERVICE;
use je_platform::user_data_profile::I_USER_DATA_PROFILES_SERVICE;
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;
use je_base::promise::Promise;
use serde_json::Value;

use crate::services::extension_management::extension_storage::{ExtensionStorageTarget, I_EXTENSION_STORAGE_SERVICE};

/// `if (value)`: a veracidade do JavaScript.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0 && !number.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// An extension storage has following
///  - State: Stored using storage service with extension id as key and state as value.
///  - Resources: Stored under a location scoped to the extension.
pub fn migrate_extension_storage(from_extension_id: &str, to_extension_id: &str, global: bool, instantiation_service: &InstantiationService) -> Promise<()> {
    let (environment_service, user_data_profiles_service, extension_storage_service, storage_service, uri_identity_service, file_service, workspace_context_service, log_service) =
        instantiation_service.invoke_function(|service_accessor| {
            (
                service_accessor.get(I_ENVIRONMENT_SERVICE),
                service_accessor.get(I_USER_DATA_PROFILES_SERVICE),
                service_accessor.get(I_EXTENSION_STORAGE_SERVICE),
                service_accessor.get(I_STORAGE_SERVICE),
                service_accessor.get(I_URI_IDENTITY_SERVICE),
                service_accessor.get(I_FILE_SERVICE),
                service_accessor.get(I_WORKSPACE_CONTEXT_SERVICE),
                service_accessor.get(I_LOG_SERVICE),
            )
        });
    let from_extension_id = from_extension_id.to_owned();
    let to_extension_id = to_extension_id.to_owned();

    async_fn(async move {
        let storage_migrated_key = format!("extensionStorage.migrate.{from_extension_id}-{to_extension_id}");
        let migrate_lower_case_storage_key = if from_extension_id.to_lowercase() == to_extension_id.to_lowercase() {
            Some(format!("extension.storage.migrateFromLowerCaseKey.{}", from_extension_id.to_lowercase()))
        } else {
            None
        };

        if from_extension_id == to_extension_id {
            return Ok(());
        }

        let ext_uri = uri_identity_service.ext_uri();
        let get_extension_storage_location = |extension_id: &str, global: bool| -> URI {
            if global {
                // Extension id is lower cased for global storage
                return ext_uri.join_path(&user_data_profiles_service.default_profile().global_storage_home, &[extension_id.to_lowercase().as_str()]);
            }
            ext_uri.join_path(&environment_service.workspace_storage_home(), &[workspace_context_service.get_workspace().id().as_str(), extension_id])
        };

        let storage_scope = if global { StorageScope::Profile } else { StorageScope::Workspace };
        let scope_name = if global { "global" } else { "workspace" };
        let already_migrated = storage_service.get_boolean(&storage_migrated_key, storage_scope, Some(false)).unwrap_or(false)
            || migrate_lower_case_storage_key.as_deref().is_some_and(|key| storage_service.get_boolean(key, storage_scope, Some(false)).unwrap_or(false));
        if !already_migrated {
            log_service.info(&format!("Migrating {scope_name} extension storage from {from_extension_id} to {to_extension_id}..."));
            // Migrate state
            let from_target = ExtensionStorageTarget::from(from_extension_id.as_str());
            let to_target = ExtensionStorageTarget::from(to_extension_id.as_str());
            let value = extension_storage_service.get_extension_state(&from_target, global);
            if let Some(value) = value.filter(is_truthy) {
                extension_storage_service.set_extension_state(&to_target, Some(&value), global);
                extension_storage_service.set_extension_state(&from_target, None, global);
            }

            // Migrate stored files
            let from_path = get_extension_storage_location(&from_extension_id, global);
            let to_path = get_extension_storage_location(&to_extension_id, global);
            if !ext_uri.is_equal(Some(&from_path), Some(&to_path), false) {
                if let Err(error) = file_service.move_(&from_path, &to_path, Some(true)).await
                    && !matches!(to_file_system_provider_error_code(Some(&error)), FileSystemProviderErrorCode::FileNotFound)
                {
                    log_service.info_args(
                        &format!("Error while migrating {scope_name} file storage from '{from_extension_id}' to '{to_extension_id}'"),
                        &[LogArg::from(error.message.as_str())],
                    );
                }
            }
            log_service.info(&format!("Migrated {scope_name} extension storage from {from_extension_id} to {to_extension_id}"));
            storage_service.store(&storage_migrated_key, StorageValue::Bool(true), storage_scope, StorageTarget::Machine);
        }

        Ok(())
    })
}
