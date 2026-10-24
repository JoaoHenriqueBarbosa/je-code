//! Porte de `src/vs/platform/userDataSync/common/userDataSyncLocalStoreService.ts`: o histórico local dos
//! recursos (o backup que cada sincronizador grava antes de aplicar uma mudança) em
//! `<userDataSyncHome>/[<coleção>/]<recurso>/<AAAAMMDDTHHMMSS>.json`.
//!
//! Divergências:
//! - `stats.sort()` do original ordena objetos `IFileStat`, que o JavaScript converte em texto
//!   (`[object Object]`): a ordenação não muda nada e a ordem é a de `resolve`. Aqui `sort` não existe pelo
//!   mesmo motivo, e `getAllResourceRefs` só inverte a ordem de `resolve` (o `reverse()` do original).
//! - `new Date(a, m, d, h, mi, s).getTime()` é o `civil::DateTime` no fuso do sistema; um nome de arquivo que
//!   não forma uma data válida dá `NaN`, como no original.
//! - `cleanUp`, chamado no construtor, é uma promessa que ninguém espera; como nunca rejeita, não há erro solto.

use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, promises};
use je_base::buffer::VSBuffer;
use je_base::date::{local_time_zone, to_local_iso_string};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::URI;
use jiff::civil::DateTime;

use super::user_data_sync::{ALL_SYNC_RESOURCES, IResourceRefHandle, IUserDataSyncLocalStoreService, IUserDataSyncLogService, ServerResource};
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::files::files::{FileOperationResult, FileStat, PartialFileDeleteOptions, to_file_operation_result};
use crate::log::{LogMessage, Logger};
use crate::resources::join_path;
use crate::user_data_profile::IUserDataProfilesService;

/// `UserDataSyncLocalStoreService`.
pub struct UserDataSyncLocalStoreService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    environment_service: Rc<dyn EnvironmentService>,
    file_service: Rc<FileService>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: IUserDataSyncLogService,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
}

/// `/^\d{8}T\d{6}(\.json)?$/`.
fn is_backup_file_name(name: &str) -> bool {
    let stem = name.strip_suffix(".json").unwrap_or(name);
    let bytes = stem.as_bytes();
    bytes.len() == 15 && bytes[8] == b'T' && bytes.iter().enumerate().all(|(index, byte)| index == 8 || byte.is_ascii_digit())
}

/// `getCreationTime`: a data do nome do arquivo, em milissegundos, no fuso do sistema.
fn get_creation_time(stat: &FileStat) -> f64 {
    let part = |from: usize, to: usize| stat.name.get(from..to).and_then(|text| text.parse::<i16>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (part(0, 4), part(4, 6), part(6, 8), part(9, 11), part(11, 13), part(13, 15)) else {
        return f64::NAN;
    };
    let to_i8 = |value: i16| i8::try_from(value).ok();
    let (Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (to_i8(month), to_i8(day), to_i8(hour), to_i8(minute), to_i8(second)) else { return f64::NAN };
    DateTime::new(year, month, day, hour, minute, second, 0)
        .and_then(|date_time| date_time.to_zoned(local_time_zone()))
        .map_or(f64::NAN, |zoned| zoned.timestamp().as_millisecond() as f64)
}

impl UserDataSyncLocalStoreService {
    pub fn new(
        environment_service: Rc<dyn EnvironmentService>,
        file_service: Rc<FileService>,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: IUserDataSyncLogService,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self { me: me.clone(), base: Disposable::new(), environment_service, file_service, configuration_service, log_service, user_data_profiles_service });
        let _ = this.clean_up();
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    fn clean_up(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            for profile in this.user_data_profiles_service.profiles() {
                for resource in ALL_SYNC_RESOURCES {
                    let collection = if profile.is_default { None } else { Some(profile.id.as_str()) };
                    let folder = this.get_resource_backup_home(ServerResource::from(resource), collection, None);
                    if let Err(error) = this.clean_up_backup(folder).await {
                        this.log_service.error_args(&LogMessage::from(&error), &[]);
                    }
                }
            }

            let stat = match this.file_service.resolve(&this.environment_service.user_data_sync_home(), None).await {
                Ok(stat) => stat,
                Err(error) => {
                    if to_file_operation_result(&error) != FileOperationResult::FileNotFound {
                        this.log_service.error_args(&LogMessage::from(&error), &[]);
                    }
                    return Ok(());
                }
            };

            for child in stat.children.iter().flatten() {
                let is_known = ALL_SYNC_RESOURCES.iter().any(|resource| resource.as_str() == child.name) || this.user_data_profiles_service.profiles().iter().any(|profile| profile.id == child.name);
                if child.is_directory && !is_known {
                    this.log_service.info_args("Deleting non existing profile from backup", &[crate::log::LogArg::from(child.resource.path().to_string_lossy().as_str())]);
                    if let Err(error) = this.file_service.del(&child.resource, Some(PartialFileDeleteOptions { recursive: Some(true), ..PartialFileDeleteOptions::default() })).await {
                        this.log_service.error_args(&LogMessage::from(&error), &[]);
                    }
                }
            }
            Ok(())
        })
    }

    fn get_resource_backup_home(&self, resource: ServerResource, collection: Option<&str>, root: Option<&URI>) -> URI {
        let root = root.cloned().unwrap_or_else(|| self.environment_service.user_data_sync_home());
        match collection.filter(|collection| !collection.is_empty()) {
            Some(collection) => join_path(&root, &[collection, resource.as_str()]),
            None => join_path(&root, &[resource.as_str()]),
        }
    }

    fn clean_up_backup(&self, folder: URI) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let result: Result<(), je_base::errors::Error> = async {
                if !this.file_service.exists(&folder).await.unwrap_or(false) {
                    return Ok(());
                }
                let stat = this.file_service.resolve(&folder, None).await?;
                if let Some(children) = &stat.children {
                    let all: Vec<&FileStat> = children.iter().filter(|stat| stat.is_file && is_backup_file_name(&stat.name)).collect();
                    let duration = match this.configuration_service.get_value(Some("sync.localBackupDuration"), None) {
                        Dynamic::Number(days) if days != 0.0 && !days.is_nan() => days,
                        _ => 30.0, /* Default 30 days */
                    };
                    let back_up_max_age = 1000.0 * 60.0 * 60.0 * 24.0 * duration;
                    let mut to_delete: Vec<&FileStat> = all.iter().copied().filter(|stat| date_now() as f64 - get_creation_time(stat) > back_up_max_age).collect();
                    let remaining = all.len() - to_delete.len();
                    if remaining < 10 {
                        to_delete = to_delete.into_iter().skip(10 - remaining).collect();
                    }
                    let deletions: Vec<Promise<()>> = to_delete
                        .into_iter()
                        .map(|stat| {
                            let (this, resource) = (this.clone(), stat.resource.clone());
                            async_fn(async move {
                                this.log_service.info_args("Deleting from backup", &[crate::log::LogArg::from(resource.path().to_string_lossy().as_str())]);
                                this.file_service.del(&resource, None).await
                            })
                        })
                        .collect();
                    promises::settled(deletions).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                this.log_service.error_args(&LogMessage::from(&error), &[]);
            }
            Ok(())
        })
    }
}

impl IDisposable for UserDataSyncLocalStoreService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncLocalStoreService for UserDataSyncLocalStoreService {
    fn write_resource(&self, resource: ServerResource, content: &str, c_time: f64, collection: Option<&str>, root: Option<&URI>) -> Promise<()> {
        let folder = self.get_resource_backup_home(resource, collection, root);
        let iso = to_local_iso_string(c_time, &local_time_zone()).replace(['-', ':'], "");
        let file = join_path(&folder, &[&format!("{}.json", iso.split('.').next().unwrap_or(&iso))]);
        let (this, content) = (self.rc(), content.to_string());
        async_fn(async move {
            if let Err(error) = this.file_service.write_file(&file, FileContentSource::from(VSBuffer::from_string(&content)), None).await {
                this.log_service.error_args(&LogMessage::from(&error), &[]);
            }
            Ok(())
        })
    }

    fn get_all_resource_refs(&self, resource: ServerResource, collection: Option<&str>, root: Option<&URI>) -> Promise<Vec<IResourceRefHandle>> {
        let folder = self.get_resource_backup_home(resource, collection, root);
        let this = self.rc();
        async_fn(async move {
            match this.file_service.resolve(&folder, None).await {
                Ok(stat) => {
                    if let Some(children) = &stat.children {
                        let all: Vec<&FileStat> = children.iter().filter(|stat| stat.is_file && !stat.name.starts_with("lastSync")).rev().collect();
                        return Ok(all.into_iter().map(|stat| IResourceRefHandle { r#ref: stat.name.clone(), created: get_creation_time(stat) }).collect());
                    }
                }
                Err(error) => {
                    if to_file_operation_result(&error) != FileOperationResult::FileNotFound {
                        return Err(error);
                    }
                }
            }
            Ok(Vec::new())
        })
    }

    fn resolve_resource_content(&self, resource: ServerResource, r#ref: &str, collection: Option<&str>, root: Option<&URI>) -> Promise<Option<String>> {
        let folder = self.get_resource_backup_home(resource, collection, root);
        let file = join_path(&folder, &[r#ref]);
        let this = self.rc();
        async_fn(async move {
            match this.file_service.read_file(&file, None, None).await {
                Ok(content) => Ok(Some(content.value.to_string())),
                Err(error) => {
                    this.log_service.error_args(&LogMessage::from(&error), &[]);
                    Ok(None)
                }
            }
        })
    }
}
