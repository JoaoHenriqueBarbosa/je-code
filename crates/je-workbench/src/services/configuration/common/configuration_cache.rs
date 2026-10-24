//! Porte de `src/vs/workbench/services/configuration/common/configurationCache.ts`: o cache em disco das
//! configurações (padrões, usuário remoto, workspaces e pastas) que o navegador usa para abrir rápido.
//!
//! O `async` do original é [`Promise`]; os erros de `read` viram o texto vazio, como no original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{Queue, async_fn};
use je_base::buffer::VSBuffer;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::files::{FileOperationError, FileOperationResult, PartialFileDeleteOptions};
use je_platform::resources::join_path;

use super::configuration::{ConfigurationKey, ConfigurationKeyType, IConfigurationCache};

/// `ConfigurationCache`.
pub struct ConfigurationCache {
    cache_home: URI,
    donot_cache_resources_with_schemes: Vec<String>,
    cached_configurations: RefCell<HashMap<String, Rc<CachedConfiguration>>>,
    file_service: Rc<dyn IFileService>,
}

impl ConfigurationCache {
    pub fn new(donot_cache_resources_with_schemes: Vec<String>, environment_service: &dyn EnvironmentService, file_service: Rc<dyn IFileService>) -> Self {
        Self { cache_home: environment_service.cache_home(), donot_cache_resources_with_schemes, cached_configurations: RefCell::new(HashMap::new()), file_service }
    }

    fn get_cached_configuration(&self, key: &ConfigurationKey) -> Rc<CachedConfiguration> {
        let k = format!("{}:{}", key.type_.as_str(), key.key);
        self.cached_configurations
            .borrow_mut()
            .entry(k)
            .or_insert_with(|| Rc::new(CachedConfiguration::new(key, &self.cache_home, self.file_service.clone())))
            .clone()
    }
}

impl IConfigurationCache for ConfigurationCache {
    fn needs_caching(&self, resource: &URI) -> bool {
        // Cache all non native resources
        !self.donot_cache_resources_with_schemes.iter().any(|scheme| resource.scheme() == scheme.as_str())
    }

    fn read(&self, key: &ConfigurationKey) -> Promise<String> {
        self.get_cached_configuration(key).read()
    }

    fn write(&self, key: &ConfigurationKey, content: &str) -> Promise<()> {
        self.get_cached_configuration(key).save(content.to_owned())
    }

    fn remove(&self, key: &ConfigurationKey) -> Promise<()> {
        self.get_cached_configuration(key).remove()
    }
}

struct CachedConfiguration {
    queue: Queue<()>,
    cached_configuration_folder_resource: URI,
    cached_configuration_file_resource: URI,
    file_service: Rc<dyn IFileService>,
}

impl CachedConfiguration {
    fn new(key: &ConfigurationKey, cache_home: &URI, file_service: Rc<dyn IFileService>) -> Self {
        let cached_configuration_folder_resource = join_path(cache_home, &["CachedConfigurations", key.type_.as_str(), key.key.as_str()]);
        let file_name = if key.type_ == ConfigurationKeyType::Workspaces { "workspace.json" } else { "configuration.json" };
        let cached_configuration_file_resource = join_path(&cached_configuration_folder_resource, &[file_name]);
        Self { queue: Queue::new(), cached_configuration_folder_resource, cached_configuration_file_resource, file_service }
    }

    fn read(self: &Rc<Self>) -> Promise<String> {
        let this = self.clone();
        async_fn(async move {
            match this.file_service.read_file(&this.cached_configuration_file_resource, None, None).await {
                Ok(content) => Ok(content.value.to_string()),
                Err(_) => Ok(String::new()),
            }
        })
    }

    fn save(self: &Rc<Self>, content: String) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let created = this.create_cached_folder().await?;
            if created {
                let target = this.clone();
                this.queue
                    .queue(move || {
                        async_fn(async move {
                            target.file_service.write_file(&target.cached_configuration_file_resource, VSBuffer::from_string(&content).into(), None).await?;
                            Ok(())
                        })
                    })
                    .await?;
            }
            Ok(())
        })
    }

    fn remove(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let target = this.clone();
            let result = this
                .queue
                .queue(move || {
                    target.file_service.del(
                        &target.cached_configuration_folder_resource,
                        Some(PartialFileDeleteOptions { recursive: Some(true), use_trash: Some(false), atomic: None }),
                    )
                })
                .await;
            match result {
                Err(error) if FileOperationError::from_error(&error).map(|e| e.file_operation_result) != Some(FileOperationResult::FileNotFound) => Err(error),
                _ => Ok(()),
            }
        })
    }

    fn create_cached_folder(self: &Rc<Self>) -> Promise<bool> {
        let this = self.clone();
        async_fn::<bool>(async move {
            if this.file_service.exists(&this.cached_configuration_folder_resource).await? {
                return Ok(true);
            }
            match this.file_service.create_folder(&this.cached_configuration_folder_resource).await {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            }
        })
    }
}
