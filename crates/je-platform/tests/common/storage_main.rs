//! A montagem do `StorageMainService` dos testes portados: o ambiente nativo, o serviço de identidade de URIs, o estado e o
//! `UserDataProfilesMainService` que o serviço lê, como o `createStorageService` de `storageMainService.test.ts` (que
//! os cria um a um). Os serviços entram no `disposables` do chamador.

use je_instantiation::Injectable;
use std::rc::Rc;

use je_base::lifecycle::DisposableStore;
use je_platform::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};
use je_platform::environment::native::NativeEnvironmentService;
use je_platform::files::file_service::FileService;
use je_platform::log::LogServiceRef;
use je_platform::product::ProductService;
use je_platform::resources::host_environment;
use je_platform::state::{SaveStrategy, StateServiceImpl};
use je_platform::storage::{StorageMainOptions, StorageMainService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data_profile::user_data_profiles_handler::LifecycleMainService;
use je_platform::user_data_profile::{UserDataProfilesEnvironment, UserDataProfilesMainEnvironment, UserDataProfilesMainService};

/// O `StorageMainService` e o serviço de perfis que ele usa.
pub struct StorageMainServiceSetup {
    pub storage_main_service: Rc<StorageMainService>,
    pub user_data_profiles_service: Rc<UserDataProfilesMainService>,
}

/// Cria o `StorageMainService`. `storage_options` é o `getStorageOptions()` sobrescrito (`None` usa o do original, que
/// só usa memória durante os testes de extensão).
pub fn create_storage_main_service(
    disposables: &DisposableStore,
    storage_options: Option<StorageMainOptions>,
    environment_service: Rc<NativeEnvironmentService>,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
    lifecycle_main_service: Rc<dyn LifecycleMainService>,
) -> StorageMainServiceSetup {
    let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service.clone(),)));

    let state_service = disposables.add_rc(StateServiceImpl::new(SaveStrategy::Delayed, environment_service.state_resource(), log_service.clone(), file_service.clone()));
    let user_data_profiles_service = disposables.add_rc(UserDataProfilesMainService::new(
        UserDataProfilesMainEnvironment {
            profiles: UserDataProfilesEnvironment {
                user_roaming_data_home: environment_service.user_roaming_data_home(),
                cache_home: environment_service.cache_home(),
                agent_sessions_workspace: Some(environment_service.agent_sessions_workspace()),
            },
            extensions_path: environment_service.extensions_path(),
            user_home: environment_service.user_home(),
            agent_plugins_dir: environment_service.args().get_string("agent-plugins-dir"),
            data_folder_name: ProductService::default().data_folder_name,
        },
        state_service,
        file_service.clone(),
        uri_identity_service.ext_uri(),
        log_service.clone(),
        host_environment(),
    ));

    let storage_main_service = match storage_options {
        Some(storage_options) => {
            StorageMainService::with_storage_options(storage_options, log_service, environment_service, user_data_profiles_service.clone(), lifecycle_main_service, file_service, uri_identity_service)
        }
        None => StorageMainService::new(log_service, environment_service, user_data_profiles_service.clone(), lifecycle_main_service, file_service, uri_identity_service),
    };
    disposables.add_rc(storage_main_service.clone());

    StorageMainServiceSetup { storage_main_service, user_data_profiles_service }
}
