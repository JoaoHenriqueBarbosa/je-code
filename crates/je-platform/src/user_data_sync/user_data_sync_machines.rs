//! Porte de `src/vs/platform/userDataSync/common/userDataSyncMachines.ts`: a lista de máquinas que sincronizam
//! (o recurso `machines` do servidor), com a máquina atual, os nomes e a habilitação de cada uma.
//!
//! Divergências:
//! - `isWeb` é falso e não existe `embedderIdentifier` no [`IProductService`] (só o produto web o declara): o nome
//!   da máquina é `<plataforma> (<nameShort>) #<n>`. A plataforma é a do [`Platform::detect`].
//! - `IMachineData` guarda só os campos que o original declara (`id`, `name`, `disabled`, `platform`); um campo
//!   que outro cliente acrescente ao JSON não é preservado na reescrita. Na reescrita as chaves saem na ordem
//!   `id`, `name`, `platform`, `disabled` (a ordem em que o original as cria). Um JSON sem `version` ou sem
//!   `machines` legível é uma lista vazia de versão desconhecida (o original lançaria um erro de tipo ao ler a
//!   propriedade que falta; o de versão incompatível sai igual).
//! - `renameMachine(machineId, name, manifest?)` tem o `manifest` opcional só na classe: o trait tem
//!   [`IUserDataSyncMachinesService::rename_machine`] e a classe [`UserDataSyncMachinesService::rename_machine_with_manifest`].
//! - `new RegExp(`${escapeRegExpCharacters(namePrefix)}\\s#(\\d+)`).exec(name)` é a busca do prefixo seguido de um
//!   espaço em branco, `#` e dígitos, na posição mais à esquerda onde casa.

use crate::files::i_file_service::IFileService;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::json::{Object, Value};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::platform::{Platform, PlatformKind};
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_storage::StorageValue;

use super::abstract_synchronizer::{parse_json, stringify_json};
use super::user_data_sync::{IUserData, IUserDataManifest, IUserDataSyncLogService, IUserDataSyncStoreService, ServerResource};
use crate::environment::environment::EnvironmentService;
use crate::external_services::service_machine_id::get_service_machine_id;
use crate::files::file_service::FileService;
use crate::log::LogMessage;
use crate::product::IProductService;
use crate::storage::{StorageScope, StorageService, StorageTarget};

// region: dados

/// `IMachineData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IMachineData {
    pub id: String,
    pub name: String,
    pub disabled: Option<bool>,
    pub platform: Option<String>,
}

impl IMachineData {
    fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("id"), Value::String(JsString::from(self.id.as_str())));
        object.insert(JsString::from("name"), Value::String(JsString::from(self.name.as_str())));
        if let Some(platform) = &self.platform {
            object.insert(JsString::from("platform"), Value::String(JsString::from(platform.as_str())));
        }
        if let Some(disabled) = self.disabled {
            object.insert(JsString::from("disabled"), Value::Bool(disabled));
        }
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let text = |name: &str| match object.get(name) {
            Some(Value::String(text)) => Some(text.to_string_lossy()),
            _ => None,
        };
        Some(Self {
            id: text("id")?,
            name: text("name")?,
            disabled: match object.get("disabled") {
                Some(Value::Bool(disabled)) => Some(*disabled),
                _ => None,
            },
            platform: text("platform"),
        })
    }
}

/// `IMachinesData`. A versão é um número porque o JSON do servidor pode trazer qualquer valor.
#[derive(Clone, Debug, PartialEq)]
pub struct IMachinesData {
    pub version: f64,
    pub machines: Vec<IMachineData>,
}

impl IMachinesData {
    fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("version"), Value::Number(self.version));
        object.insert(JsString::from("machines"), Value::Array(self.machines.iter().map(IMachineData::to_json).collect()));
        Value::Object(object)
    }

    fn from_json(value: &Value) -> Self {
        let Value::Object(object) = value else { return Self { version: f64::NAN, machines: Vec::new() } };
        Self {
            version: match object.get("version") {
                Some(Value::Number(version)) => *version,
                _ => f64::NAN,
            },
            machines: match object.get("machines") {
                Some(Value::Array(machines)) => machines.iter().filter_map(IMachineData::from_json).collect(),
                _ => Vec::new(),
            },
        }
    }
}

/// `IUserDataSyncMachine`: `Readonly<IMachineData> & { readonly isCurrent: boolean }`.
#[derive(Clone, Debug, PartialEq)]
pub struct IUserDataSyncMachine {
    pub id: String,
    pub name: String,
    pub disabled: Option<bool>,
    pub platform: Option<String>,
    pub is_current: bool,
}

// endregion

// region: plataforma

const SAFARI: &str = "Safari";
const CHROME: &str = "Chrome";
const EDGE: &str = "Edge";
const FIREFOX: &str = "Firefox";
const ANDROID: &str = "Android";

/// `isWebPlatform`.
pub fn is_web_platform(platform: &str) -> bool {
    matches!(platform, SAFARI | CHROME | EDGE | FIREFOX | ANDROID) || platform == PlatformKind::Web.name()
}

/// `getPlatformName`.
fn get_platform_name() -> String {
    let platform = Platform::detect();
    if platform.is_safari() {
        return SAFARI.to_string();
    }
    if platform.is_chrome() {
        return CHROME.to_string();
    }
    if platform.is_edge() {
        return EDGE.to_string();
    }
    if platform.is_firefox() {
        return FIREFOX.to_string();
    }
    if platform.is_android() {
        return ANDROID.to_string();
    }
    (if platform.is_web { PlatformKind::Web } else { platform.kind }).name().to_string()
}

/// O índice `n` do primeiro `<prefix>\s#<n>` do nome, o `parseInt(matches[1])` de `nameRegEx.exec(name)`.
fn name_index_of(name: &str, prefix: &str) -> Option<u64> {
    for (start, _) in name.match_indices(prefix) {
        let rest = &name[start + prefix.len()..];
        let mut characters = rest.chars();
        if !characters.next().is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(after_space) = characters.as_str().strip_prefix('#') else { continue };
        let digits_end = after_space.find(|character: char| !character.is_ascii_digit()).unwrap_or(after_space.len());
        if digits_end == 0 {
            continue;
        }
        return Some(after_space[..digits_end].parse().unwrap_or(u64::MAX));
    }
    None
}

// endregion

// region: serviço

/// `IUserDataSyncMachinesService`.
pub trait IUserDataSyncMachinesService {
    fn on_did_change(&self) -> Event<()>;

    fn get_machines(&self, manifest: Option<IUserDataManifest>) -> Promise<Vec<IUserDataSyncMachine>>;

    fn add_current_machine(&self, manifest: Option<IUserDataManifest>) -> Promise<()>;
    fn remove_current_machine(&self, manifest: Option<IUserDataManifest>) -> Promise<()>;
    fn rename_machine(&self, machine_id: &str, name: &str) -> Promise<()>;
    fn set_enablements(&self, enablements: &[(String, bool)]) -> Promise<()>;
}

service_id!(pub I_USER_DATA_SYNC_MACHINES_SERVICE: dyn IUserDataSyncMachinesService = "IUserDataSyncMachinesService");

const CURRENT_MACHINE_NAME_KEY: &str = "sync.currentMachineName";
const VERSION: f64 = 1.0;

/// `UserDataSyncMachinesService`.
pub struct UserDataSyncMachinesService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<()>>,
    current_machine_id_promise: Promise<String>,
    user_data: RefCell<Option<IUserData>>,
    storage_service: Rc<dyn StorageService>,
    user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
    log_service: IUserDataSyncLogService,
    product_service: Rc<dyn IProductService>,
}

impl UserDataSyncMachinesService {
    pub fn new(
        environment_service: Rc<dyn EnvironmentService>,
        file_service: Rc<dyn IFileService>,
        storage_service: Rc<dyn StorageService>,
        user_data_sync_store_service: Rc<dyn IUserDataSyncStoreService>,
        log_service: IUserDataSyncLogService,
        product_service: Rc<dyn IProductService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let current_machine_id_promise = get_service_machine_id(environment_service.as_ref(), &file_service, Some(storage_service.clone()));
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            on_did_change,
            current_machine_id_promise,
            user_data: RefCell::new(None),
            storage_service,
            user_data_sync_store_service,
            log_service,
            product_service,
        })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço de máquinas está vivo enquanto é usado")
    }

    /// `renameMachine(machineId, name, manifest)`.
    pub fn rename_machine_with_manifest(&self, machine_id: &str, name: &str, manifest: Option<IUserDataManifest>) -> Promise<()> {
        let this = self.rc();
        let (machine_id, name) = (machine_id.to_string(), name.to_string());
        async_fn(async move {
            let mut machine_data = this.read_machines_data(manifest).await?;
            if let Some(machine) = machine_data.machines.iter_mut().find(|machine| machine.id == machine_id) {
                machine.name = name.clone();
                this.write_machines_data(&machine_data).await?;
                let current_machine_id = this.current_machine_id_promise.clone().await?;
                if machine_id == current_machine_id {
                    this.storage_service.store(CURRENT_MACHINE_NAME_KEY, StorageValue::from(name.as_str()), StorageScope::Application, StorageTarget::Machine);
                }
            }
            Ok(())
        })
    }

    fn compute_current_machine_name(&self, machines: &[IMachineData]) -> String {
        if let Some(previous_name) = self.storage_service.get(CURRENT_MACHINE_NAME_KEY, StorageScope::Application, None).filter(|previous_name| !previous_name.is_empty()) {
            if !machines.iter().any(|machine| machine.name == previous_name) {
                return previous_name;
            }
            self.storage_service.remove(CURRENT_MACHINE_NAME_KEY, StorageScope::Application);
        }

        let name_prefix = format!("{} ({})", get_platform_name(), self.product_service.name_short());
        let mut name_index = 0;
        for machine in machines {
            let index = name_index_of(&machine.name, &name_prefix).unwrap_or(0);
            name_index = name_index.max(index);
        }
        format!("{name_prefix} #{}", name_index.saturating_add(1))
    }

    fn read_machines_data(&self, manifest: Option<IUserDataManifest>) -> Promise<IMachinesData> {
        let this = self.rc();
        async_fn(async move {
            let user_data = this.read_user_data(manifest).await?;
            *this.user_data.borrow_mut() = Some(user_data.clone());
            let machines_data = this.parse(&user_data);
            if machines_data.version != VERSION {
                return Err(Error::new(localize(
                    "error incompatible",
                    "Cannot read machines data as the current version is incompatible. Please update {0} and try again.",
                    &[this.product_service.name_long()],
                )));
            }
            Ok(machines_data)
        })
    }

    fn write_machines_data(&self, machines_data: &IMachinesData) -> Promise<()> {
        let this = self.rc();
        let content = stringify_json(&machines_data.to_json());
        async_fn(async move {
            let r#ref = this.user_data.borrow().as_ref().map(|user_data| user_data.r#ref.clone()).filter(|r#ref| !r#ref.is_empty());
            let r#ref = this.user_data_sync_store_service.write_resource(ServerResource::Machines, &content, r#ref.as_deref(), None, None).await?;
            *this.user_data.borrow_mut() = Some(IUserData { r#ref, content: Some(content) });
            this.on_did_change.fire(());
            Ok(())
        })
    }

    fn read_user_data(&self, manifest: Option<IUserDataManifest>) -> Promise<IUserData> {
        let this = self.rc();
        async_fn(async move {
            let current = this.user_data.borrow().clone();
            if let Some(user_data) = &current {
                let latest_ref = manifest.as_ref().and_then(|manifest| manifest.latest.as_ref()).and_then(|latest| latest.get(ServerResource::Machines.as_str()).cloned());

                // Last time synced resource and latest resource on server are same
                if latest_ref.as_deref() == Some(user_data.r#ref.as_str()) {
                    return Ok(user_data.clone());
                }

                // There is no resource on server and last time it was synced with no resource
                if latest_ref.is_none() && user_data.content.is_none() {
                    return Ok(user_data.clone());
                }
            }

            this.user_data_sync_store_service.read_resource(ServerResource::Machines, current, None, None).await
        })
    }

    fn parse(&self, user_data: &IUserData) -> IMachinesData {
        if let Some(content) = &user_data.content {
            match parse_json(content) {
                Ok(value) => return IMachinesData::from_json(&value),
                Err(error) => self.log_service.error_args(&LogMessage::from(&error), &[]),
            }
        }
        IMachinesData { version: VERSION, machines: Vec::new() }
    }
}

impl IDisposable for UserDataSyncMachinesService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataSyncMachinesService for UserDataSyncMachinesService {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn get_machines(&self, manifest: Option<IUserDataManifest>) -> Promise<Vec<IUserDataSyncMachine>> {
        let this = self.rc();
        async_fn(async move {
            let current_machine_id = this.current_machine_id_promise.clone().await?;
            let machine_data = this.read_machines_data(manifest).await?;
            Ok(machine_data
                .machines
                .into_iter()
                .map(|machine| {
                    let is_current = machine.id == current_machine_id;
                    IUserDataSyncMachine { id: machine.id, name: machine.name, disabled: machine.disabled, platform: machine.platform, is_current }
                })
                .collect())
        })
    }

    fn add_current_machine(&self, manifest: Option<IUserDataManifest>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let current_machine_id = this.current_machine_id_promise.clone().await?;
            let mut machine_data = this.read_machines_data(manifest).await?;
            if !machine_data.machines.iter().any(|machine| machine.id == current_machine_id) {
                let name = this.compute_current_machine_name(&machine_data.machines);
                machine_data.machines.push(IMachineData { id: current_machine_id, name, disabled: None, platform: Some(get_platform_name()) });
                this.write_machines_data(&machine_data).await?;
            }
            Ok(())
        })
    }

    fn remove_current_machine(&self, manifest: Option<IUserDataManifest>) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let current_machine_id = this.current_machine_id_promise.clone().await?;
            let mut machine_data = this.read_machines_data(manifest).await?;
            let updated_machines: Vec<IMachineData> = machine_data.machines.iter().filter(|machine| machine.id != current_machine_id).cloned().collect();
            if updated_machines.len() != machine_data.machines.len() {
                machine_data.machines = updated_machines;
                this.write_machines_data(&machine_data).await?;
            }
            Ok(())
        })
    }

    fn rename_machine(&self, machine_id: &str, name: &str) -> Promise<()> {
        self.rename_machine_with_manifest(machine_id, name, None)
    }

    fn set_enablements(&self, enablements: &[(String, bool)]) -> Promise<()> {
        let this = self.rc();
        let enablements = enablements.to_vec();
        async_fn(async move {
            let mut machine_data = this.read_machines_data(None).await?;
            for (machine_id, enabled) in enablements {
                if let Some(machine) = machine_data.machines.iter_mut().find(|machine| machine.id == machine_id) {
                    machine.disabled = if enabled { None } else { Some(true) };
                }
            }
            this.write_machines_data(&machine_data).await
        })
    }
}

// endregion
