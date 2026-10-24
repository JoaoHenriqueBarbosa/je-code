//! Porte de `src/vs/platform/externalServices/common/serviceMachineId.ts`: o identificador da máquina que os
//! serviços externos (a sincronização de configurações, o marketplace) usam.

use crate::files::i_file_service::IFileService;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::promise::Promise;
use je_base::uuid::{generate_uuid, is_uuid};
use je_storage::StorageValue;

use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::storage::{StorageScope, StorageService, StorageTarget};

const SERVICE_MACHINE_ID_KEY: &str = "storage.serviceMachineId";

/// `getServiceMachineId`: o UUID guardado no armazenamento ou no arquivo `serviceMachineIdResource`, criado
/// (e gravado nos dois) quando não existe.
pub fn get_service_machine_id(environment_service: &dyn EnvironmentService, file_service: &Rc<dyn IFileService>, storage_service: Option<Rc<dyn StorageService>>) -> Promise<String> {
    let resource = environment_service.service_machine_id_resource();
    let file_service = file_service.clone();
    async_fn(async move {
        let stored = storage_service.as_ref().and_then(|storage| storage.get(SERVICE_MACHINE_ID_KEY, StorageScope::Application, None)).filter(|uuid| !uuid.is_empty());
        if let Some(uuid) = stored {
            return Ok(uuid);
        }
        let from_file = match file_service.read_file(&resource, None, None).await {
            Ok(contents) => Some(contents.value.to_string()).filter(|value| is_uuid(value)),
            Err(_) => None,
        };
        let uuid = match from_file {
            Some(uuid) => uuid,
            None => {
                let uuid = generate_uuid();
                // noop: falhar em gravar o arquivo não impede de usar o UUID
                let _ = file_service.write_file(&resource, FileContentSource::from(uuid.as_str()), None).await;
                uuid
            }
        };
        if let Some(storage) = &storage_service {
            storage.store(SERVICE_MACHINE_ID_KEY, StorageValue::from(uuid.as_str()), StorageScope::Application, StorageTarget::Machine);
        }
        Ok(uuid)
    })
}
