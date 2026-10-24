//! `vscode.extensions`: `all`, `getExtension(id)` e a classe `Extension` (`id`, `extensionUri`, `extensionPath`, `packageJSON`,
//! `extensionKind`, `isFromDifferentExtensionHost`, `isActive`, `exports`, `activate()`), sobre os registros e o ativador do
//! `ExtHostExtensionService` (`extHost.api.impl.ts`: o literal `extensions` e a classe `Extension`).
//!
//! Falta: `allAcrossExtensionHosts` e `getExtension(id, true)` (propostas `extensionsAny`, que o host não habilita) e `onDidChange` (o
//! `ExtensionDescriptionRegistry` do porte ainda não expõe o evento).
//!
//! Divergências:
//! - O `vscode` é um só para todas as extensões, então a extensão de origem de `activate()` (o `extensionId` do `reason`) é sempre
//!   `je.exthost`, e o `extensionKind` é sempre `ExtensionKind.UI` (o host em processo é local, o `initData.remote.isRemote` do
//!   original vale `false`).
//! - O motor entrega objeto comum por cópia, então `Extension` é uma instância de classe nativa que carrega só o identificador, e cada
//!   propriedade é lida do registro no instante da leitura: `packageJSON` e `exports` chegam como cópias novas (um objeto comum que a
//!   extensão devolveu em `activate` perde a identidade; função e instância de classe nativa ficam), e `getExtension(id) ===
//!   getExtension(id)` falha. Uma extensão removida do registro faz a leitura lançar.
//! - `packageJSON` é o manifesto (`package.json`, com as chaves que o tipo não conhece) mais `identifier`, `uuid`, `isBuiltin`,
//!   `isUserBuiltin`, `isUnderDevelopment`, `extensionLocation`, `preRelease` e `publisherDisplayName`; `targetPlatform` não está
//!   exposto.
//! - O serviço de extensões nasce depois do objeto `vscode` (ver [`ExtensionServiceSlot`]); ler `extensions.all` antes de o host
//!   ligar o serviço lança.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::promise::{Promise, Resolver};
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::services::extensions::extension_activation::ExtensionActivationReason;
use je_workbench::services::extensions::extension_description_registry::{ExtensionDescriptionRegistry, IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription};
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::accessor_namespace;
use crate::api::commands::promise_to_js;
use crate::api::HOST_EXTENSION;
use crate::convert::{dynamic_to_js, uri_to_js};
use crate::ext_host_extension_service::ExtHostExtensionService;

/// `ExtensionKind.UI`.
const EXTENSION_KIND_UI: f64 = 1.0;

/// Onde o host guarda o `ExtHostExtensionService` depois de criá-lo: o objeto `vscode` é montado antes do serviço (o serviço usa a tabela
/// de descartáveis do host), e `vscode.extensions` o lê aqui a cada chamada.
pub struct ExtensionServiceSlot {
    service: RefCell<Option<Rc<ExtHostExtensionService>>>,
    /// O registro das extensões deste host, que assenta quando o serviço é ligado (o `getExtensionRegistry()` do original espera o
    /// host estar pronto; o serviço de depuração o pede ao nascer, antes de o serviço de extensões existir).
    registry: Promise<Rc<ExtensionDescriptionRegistry>>,
    registry_resolver: Resolver<Rc<ExtensionDescriptionRegistry>>,
}

impl ExtensionServiceSlot {
    pub fn new() -> Rc<Self> {
        let (registry, registry_resolver) = Promise::new();
        Rc::new(Self { service: RefCell::new(None), registry, registry_resolver })
    }

    pub fn bind(&self, service: Rc<ExtHostExtensionService>) {
        self.registry_resolver.resolve(service.my_registry().clone());
        *self.service.borrow_mut() = Some(service);
    }

    /// `getExtensionRegistry()`: o registro das extensões do host, quando o serviço for ligado.
    pub fn extension_registry(&self) -> Promise<Rc<ExtensionDescriptionRegistry>> {
        self.registry.clone()
    }

    /// As extensões deste host (vazio enquanto o serviço não foi ligado).
    pub(crate) fn extensions(&self) -> Vec<SharedExtensionDescription> {
        self.service.borrow().as_ref().map_or_else(Vec::new, |service| service.my_registry().get_all_extension_descriptions())
    }

    fn service(&self) -> Result<Rc<ExtHostExtensionService>, String> {
        self.service.borrow().clone().ok_or_else(|| "vscode.extensions: o serviço de extensões do host ainda não foi iniciado".to_owned())
    }
}

/// O valor JSON como dado do motor, na ordem das chaves.
pub(crate) fn json_to_js(value: &serde_json::Value) -> JsData {
    match value {
        serde_json::Value::Null => JsData::Null,
        serde_json::Value::Bool(flag) => JsData::Bool(*flag),
        serde_json::Value::Number(number) => JsData::Num(number.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(text) => JsData::Str(text.clone()),
        serde_json::Value::Array(items) => JsData::Array(items.iter().map(json_to_js).collect()),
        serde_json::Value::Object(entries) => JsData::Object(entries.iter().map(|(key, item)| (key.clone(), json_to_js(item))).collect()),
    }
}

fn instance(id: &str) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("Extension".to_owned())), ("_id".to_owned(), JsData::Str(id.to_owned()))])
}

/// O objeto `packageJSON` (ver as divergências do módulo).
fn package_json(description: &ExtensionDescription) -> Result<JsData, String> {
    let manifest = serde_json::to_value(&description.manifest).map_err(|error| format!("o manifesto de '{}' não vira JSON: {error}", description.identifier.value))?;
    let JsData::Object(mut fields) = json_to_js(&manifest) else { return Err("o manifesto de uma extensão precisa ser um objeto".to_owned()) };
    let mut set = |name: &str, value: JsData| {
        fields.retain(|(key, _)| key != name);
        fields.push((name.to_owned(), value));
    };
    set("identifier", JsData::Object(vec![("value".to_owned(), JsData::Str(description.identifier.value.clone()))]));
    if let Some(uuid) = &description.uuid {
        set("uuid", JsData::Str(uuid.clone()));
    }
    set("isBuiltin", JsData::Bool(description.is_builtin));
    set("isUserBuiltin", JsData::Bool(description.is_user_builtin));
    set("isUnderDevelopment", JsData::Bool(description.is_under_development));
    set("extensionLocation", uri_to_js(&URI::parse(description.extension_location.as_str(), false)));
    set("preRelease", JsData::Bool(description.pre_release));
    if let Some(name) = &description.publisher_display_name {
        set("publisherDisplayName", JsData::Str(name.clone()));
    }
    Ok(JsData::Object(fields))
}

/// `getExtensionExports`: `null` se a extensão não está ativada, `undefined` se o `activate` dela não devolveu nada.
fn exports_of(service: &ExtHostExtensionService, identifier: &ExtensionIdentifier) -> JsData {
    match service.get_extension_exports(identifier) {
        None => JsData::Null,
        Some(None) => JsData::Undefined,
        Some(Some(exports)) => dynamic_to_js(&exports),
    }
}

/// `packageJSON.api === 'none'`: a extensão não expõe `exports`.
fn exposes_no_api(description: &ExtensionDescription) -> bool {
    serde_json::to_value(&description.manifest).ok().is_some_and(|manifest| manifest.get("api").and_then(serde_json::Value::as_str) == Some("none"))
}

fn description_of(slot: &ExtensionServiceSlot, this: &JsData) -> Result<(Rc<ExtHostExtensionService>, SharedExtensionDescription), String> {
    let Some(JsData::Str(id)) = crate::convert::field(this, "_id") else { return Err("esperava uma Extension".to_owned()) };
    let service = slot.service()?;
    let description = service.my_registry().get_extension_description(id).ok_or_else(|| format!("a extensão '{id}' não está mais registrada"))?;
    Ok((service, description))
}

fn getter(slot: &Rc<ExtensionServiceSlot>, name: &str, read: impl Fn(&Rc<ExtHostExtensionService>, &ExtensionDescription) -> Result<JsData, String> + 'static) -> (String, MethodCallback) {
    let slot = slot.clone();
    (
        name.to_owned(),
        Rc::new(move |this, _| {
            let (service, description) = description_of(&slot, this)?;
            read(&service, &description)
        }),
    )
}

/// A classe `Extension` (não é exportada em `vscode`: é uma interface no `vscode.d.ts`; o protótipo existe para os objetos que o host
/// entrega).
pub fn define_class(runner: &NodeRunner, slot: &Rc<ExtensionServiceSlot>) {
    let getters = vec![
        getter(slot, "id", |_, description| Ok(JsData::Str(description.identifier.value.clone()))),
        getter(slot, "extensionUri", |_, description| Ok(uri_to_js(&URI::parse(description.extension_location.as_str(), false)))),
        getter(slot, "extensionPath", |_, description| Ok(JsData::Str(je_platform::resources::fs_path(&URI::parse(description.extension_location.as_str(), false))))),
        getter(slot, "packageJSON", |_, description| package_json(description)),
        getter(slot, "extensionKind", |_, _| Ok(JsData::Num(EXTENSION_KIND_UI))),
        getter(slot, "isFromDifferentExtensionHost", |_, _| Ok(JsData::Bool(false))),
        getter(slot, "isActive", |service, description| Ok(JsData::Bool(service.is_activated(&description.identifier)))),
        getter(slot, "exports", |service, description| {
            if exposes_no_api(description) {
                return Ok(JsData::Undefined);
            }
            Ok(exports_of(service, &description.identifier))
        }),
    ];
    let (activate_slot, activate_runner) = (slot.clone(), runner.clone());
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "activate".to_owned(),
        0,
        Rc::new(move |this, _| {
            let (service, description) = description_of(&activate_slot, this)?;
            let identifier = description.identifier.clone();
            let reason = ExtensionActivationReason { startup: false, extension_id: ExtensionIdentifier::new(HOST_EXTENSION), activation_event: "api".to_owned() };
            let no_api = exposes_no_api(&description);
            let activated = service.activate_by_id_with_errors(identifier.clone(), reason);
            let exports_service = service.clone();
            Ok(promise_to_js(&activate_runner, activated, move |()| {
                if no_api {
                    JsData::Undefined
                } else {
                    exports_of(&exports_service, &identifier)
                }
            }))
        }),
    )];
    runner.define_class(ClassSpec {
        name: "Extension".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("Extension não tem construtor público: use extensions.getExtension".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}

/// O namespace `extensions`.
pub fn namespace(runner: &NodeRunner, slot: &Rc<ExtensionServiceSlot>) -> JsData {
    let get_slot = slot.clone();
    let all_slot = slot.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![(
        "getExtension".to_owned(),
        2,
        Rc::new(move |_, args| {
            let id = crate::convert::string_arg(args, 0, "getExtension: o identificador")?;
            let service = get_slot.service()?;
            Ok(service.my_registry().get_extension_description(&id).map_or(JsData::Undefined, |description| instance(&description.identifier.value)))
        }),
    )];
    let getters: Vec<(String, MethodCallback)> = vec![(
        "all".to_owned(),
        Rc::new(move |_, _| {
            let service = all_slot.service()?;
            Ok(JsData::Array(service.my_registry().get_all_extension_descriptions().iter().map(|description| instance(&description.identifier.value)).collect()))
        }),
    )];
    accessor_namespace(runner, "extensions", methods, getters)
}
