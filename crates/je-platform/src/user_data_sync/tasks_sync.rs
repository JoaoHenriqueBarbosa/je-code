//! Porte de `src/vs/platform/userDataSync/common/tasksSync.ts`: o sincronizador de `tasks.json` e o inicializador
//! dele.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_json_synchronizer`]):
//! - `TasksSynchroniser extends AbstractJsonSynchronizer` é [`AbstractJsonSynchronizer`] sobre [`TasksSyncKind`],
//!   que dá `getContentFromSyncContent` e `toSyncContent`. O construtor é [`TasksSynchroniser::for_profile`] (o
//!   `new` da base é genérico).
//! - `ITasksSyncContent` (`{ tasks?: string }`) é um objeto JSON com a chave `tasks` ou vazio.
//! - `TasksInitializer.isEmpty` devolve `fileService.exists(tasksResource)`, ou seja, "vazio" quando o arquivo
//!   EXISTE: o porte mantém a condição do original.

use crate::files::i_file_service::IFileService;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::json::{Object, Value};
use je_base::promise::Promise;
use je_base::uri::URI;

use super::abstract_json_synchronizer::{AbstractJsonSynchronizer, JsonSyncKind, get_sync_content_property};
use super::abstract_synchronizer::{AbstractInitializer, InitializerImpl, SynchroniserServices};
use super::user_data_sync::{IRemoteUserData, IUserDataSyncLogService, IUserDataSyncResource, SyncResource};
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::log::LogServiceRef;
use crate::storage::StorageService;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_base::resources::ExtUri;

/// `getTasksContentFromSyncContent`: `JSON.parse(syncContent).tasks ?? null`; o erro vai para o log.
pub fn get_tasks_content_from_sync_content(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<String> {
    get_sync_content_property(sync_content, "tasks", log_service)
}

/// O que `TasksSynchroniser` dá à base: o conteúdo da chave `tasks` e o objeto `{ tasks }`.
pub struct TasksSyncKind;

impl JsonSyncKind for TasksSyncKind {
    fn get_content_from_sync_content(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<String> {
        get_tasks_content_from_sync_content(sync_content, log_service)
    }

    fn to_sync_content(tasks: Option<&str>) -> Value {
        let mut content = Object::new();
        if let Some(tasks) = tasks.filter(|tasks| !tasks.is_empty()) {
            content.insert(JsString::from("tasks"), Value::from(tasks));
        }
        Value::Object(content)
    }
}

/// `TasksSynchroniser`.
pub type TasksSynchroniser = AbstractJsonSynchronizer<TasksSyncKind>;

impl TasksSynchroniser {
    pub fn for_profile(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        let tasks_resource = profile.tasks_resource.clone();
        Self::new(tasks_resource, IUserDataSyncResource { sync_resource: SyncResource::Tasks, profile }, collection, "tasks.json", services)
    }
}

/// `TasksInitializer`. Dá `Deref` para o [`AbstractInitializer`].
pub struct TasksInitializer {
    base: Rc<AbstractInitializer>,
    tasks_resource: URI,
}

impl Deref for TasksInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl TasksInitializer {
    pub fn new(
        file_service: Rc<dyn IFileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let tasks_resource = user_data_profiles_service.default_profile().tasks_resource;
        let base = AbstractInitializer::new(SyncResource::Tasks, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
        let this = Rc::new(Self { base, tasks_resource });
        let weak: Weak<dyn InitializerImpl> = Rc::downgrade(&this) as Weak<dyn InitializerImpl>;
        this.base.bind(weak);
        this
    }
}

impl InitializerImpl for TasksInitializer {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()> {
        let (base, tasks_resource) = (self.base.clone(), self.tasks_resource.clone());
        async_fn(async move {
            let tasks_content = remote_user_data.sync_data.as_ref().and_then(|sync_data| get_tasks_content_from_sync_content(&sync_data.content, &base.log_service));
            let Some(tasks_content) = tasks_content.filter(|tasks_content| !tasks_content.is_empty()) else {
                base.log_service.info("Skipping initializing tasks because remote tasks does not exist.");
                return Ok(());
            };

            let is_empty = base.file_service.exists(&tasks_resource).await?;
            if !is_empty {
                base.log_service.info("Skipping initializing tasks because local tasks exist.");
                return Ok(());
            }

            base.file_service.write_file(&tasks_resource, FileContentSource::from(tasks_content.as_str()), None).await?;

            base.update_last_sync_user_data(remote_user_data, Vec::new()).await
        })
    }
}
