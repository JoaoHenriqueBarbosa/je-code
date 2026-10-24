//! Porte de `src/vs/workbench/contrib/debug/common/debugger.ts`: o `Debugger`, a contribuição de um tipo de depuração
//! mesclada de todas as extensões que a declaram (atributos, conteúdo inicial do `launch.json`, esquema JSON das
//! configurações, ponto de telemetria do adaptador).
//!
//! Notas de tradução:
//! - **`IDebuggerContribution`** é um objeto aberto (o `launch`, `variables`, `strings`...); é o [`serde_json::Value`] e o
//!   `mixin` do `merge` é [`mixin_contribution`] (não mescla o `type` do nível zero).
//! - **Serviços.** Os parâmetros decorados do construtor são o [`DebuggerServices`]: o gerenciador de adaptadores
//!   ([`IDebuggerAdapterManager`]), o `IDebugService` ([`IDebuggerDebugService`]), o resolvedor de variáveis, a
//!   configuração raiz (`getValue()`), o EOL do `ITextResourcePropertiesService`, o serviço de chaves de contexto, a
//!   autoridade remota limpa (`cleanRemoteAuthority`) e o log. Os `undefined!` dos testes são `None`.
//! - `getSchemaAttributes(definitions)` recebe o mapa das definições (`IJSONSchemaMap`) como objeto JSON mutável. O
//!   `applyDeprecatedVariableMessage` do porte trabalha sobre `Dynamic`: o `pattern` e o `patternErrorMessage` da
//!   propriedade passam por ele e voltam ao JSON.
//! - `Debugger` também implementa [`IDebugger`] (o contrato que a sessão usa).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_extensions::manifest::DebuggerContribution;
use je_extensions::scanner::ExtensionDescription;
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKeyService};
use je_platform::log::LogService;
use je_platform::workspace::WorkspaceFolder;
use serde_json::{Map, Value, json};

use super::debug::{DebugConfigurationProviderTriggerKind, IConfig, IDebugAdapter};
use super::debug_types::{IDebugSessionFull, IDebugger, ITelemetryEndpoint};
use super::debug_utils::is_debugger_main_contribution;
use crate::services::configuration_resolver::common::configuration_resolver_utils::apply_deprecated_variable_message;

/// `debuggerDisabledMessage(debugType)`.
pub fn debugger_disabled_message(debug_type: &str) -> String {
    localize("debuggerDisabled", "Configured debug type '{0}' is installed but not supported in this environment.", &[debug_type])
}

/// O que o `Debugger` usa do `IAdapterManager`.
pub trait IDebuggerAdapterManager {
    fn activate_debuggers(&self, activation_event: &str, debug_type: &str) -> Promise<()>;
    fn create_debug_adapter(&self, session: Rc<dyn IDebugSessionFull>) -> Option<Rc<dyn IDebugAdapter>>;
    fn substitute_variables(&self, debug_type: &str, folder: Option<WorkspaceFolder>, config: IConfig) -> Promise<IConfig>;
    fn run_in_terminal(&self, debug_type: &str, args: Value, session_id: &str) -> Promise<Option<i64>>;
}

/// O que o `Debugger` usa do `IDebugService`.
pub trait IDebuggerDebugService {
    /// `getModel().getSession(parentSessionId)` + `startDebugging(undefined, configuration, { parentSession }, undefined)`.
    fn start_debugging(&self, configuration: IConfig, parent_session_id: &str) -> Promise<bool>;
    /// `getConfigurationManager().hasDebugConfigurationProvider(type, triggerKind?)`.
    fn has_debug_configuration_provider(&self, debug_type: &str, trigger_kind: Option<DebugConfigurationProviderTriggerKind>) -> bool;
}

/// Os serviços do construtor do `Debugger`.
#[derive(Default)]
pub struct DebuggerServices {
    pub adapter_manager: Option<Rc<dyn IDebuggerAdapterManager>>,
    pub debug_service: Option<Rc<dyn IDebuggerDebugService>>,
    /// `configurationService.getValue()`: a configuração raiz.
    pub configuration: Option<Rc<dyn Fn() -> Value>>,
    /// `resourcePropertiesService.getEOL(URI.from({ scheme: untitled, path: '1' }))`.
    pub eol: Option<Rc<dyn Fn() -> String>>,
    /// `configurationResolverService.resolveWithInteractionReplace(folder, config, 'launch', variables, target)`.
    pub resolve_with_interaction_replace: Option<Rc<dyn Fn(Option<WorkspaceFolder>, IConfig, Option<Map<String, Value>>, Option<String>) -> Promise<IConfig>>>,
    pub context_key_service: Option<Rc<dyn IContextKeyService>>,
    /// `cleanRemoteAuthority(environmentService.remoteAuthority, productService)`.
    pub clean_remote_authority: Option<Rc<dyn Fn() -> String>>,
    pub log_service: Option<Rc<dyn LogService>>,
}

fn is_object(value: &Value) -> bool {
    value.is_object()
}

/// O `mixin` interno de `Debugger.merge`: copia as propriedades de `source` em `destination`; `overwrite` controla se as
/// propriedades não estruturadas existentes são sobrescritas, e o `type` do nível zero nunca é mesclado.
pub fn mixin_contribution(destination: &mut Value, source: &Value, overwrite: bool, level: usize) {
    if !is_object(destination) {
        *destination = source.clone();
        return;
    }

    let (Some(destination), Some(source)) = (destination.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        if key == "__proto__" {
            continue;
        }
        let both_objects = destination.get(key).is_some_and(is_object) && is_object(value);
        if both_objects {
            mixin_contribution(destination.get_mut(key).expect("a chave existe"), value, overwrite, level + 1);
        } else if destination.contains_key(key) {
            if overwrite && !(level == 0 && key == "type") {
                destination.insert(key.clone(), value.clone());
            }
        } else {
            destination.insert(key.clone(), value.clone());
        }
    }
}

/// `Debugger`.
pub struct Debugger {
    debugger_contribution: RefCell<Value>,
    merged_extension_descriptions: RefCell<Vec<Rc<ExtensionDescription>>>,
    main_extension_description: RefCell<Option<Rc<ExtensionDescription>>>,
    debugger_when: Option<ContextKeyExpression>,
    debugger_hidden_when: Option<ContextKeyExpression>,
    services: DebuggerServices,
}

impl Debugger {
    pub fn new(dbg_contribution: &Value, extension_description: Rc<ExtensionDescription>, services: DebuggerServices) -> Self {
        let mut debugger_contribution = serde_json::Map::new();
        debugger_contribution.insert("type".to_owned(), dbg_contribution.get("type").cloned().unwrap_or(Value::Null));
        let when = |key: &str| -> Option<ContextKeyExpression> {
            // `typeof this.debuggerContribution.when === 'string' ? ContextKeyExpr.deserialize(...) : undefined`
            dbg_contribution.get(key).and_then(Value::as_str).and_then(|text| ContextKeyExpr::deserialize(Some(text)))
        };
        let debugger_when = when("when");
        let debugger_hidden_when = when("hiddenWhen");
        let this = Self {
            debugger_contribution: RefCell::new(Value::Object(debugger_contribution)),
            merged_extension_descriptions: RefCell::new(Vec::new()),
            main_extension_description: RefCell::new(None),
            debugger_when,
            debugger_hidden_when,
            services,
        };
        this.merge(dbg_contribution, extension_description);
        this
    }

    pub fn merge(&self, other_debugger_contribution: &Value, extension_description: Rc<ExtensionDescription>) {
        // only if not already merged
        let already_merged = self.merged_extension_descriptions.borrow().iter().any(|merged| Rc::ptr_eq(merged, &extension_description));
        if !already_merged {
            // remember all extensions that have been merged for this debugger
            self.merged_extension_descriptions.borrow_mut().push(extension_description.clone());

            // merge new debugger contribution into existing contributions (and don't overwrite values in built-in extensions)
            mixin_contribution(&mut self.debugger_contribution.borrow_mut(), other_debugger_contribution, extension_description.is_builtin, 0);

            // remember the extension that is considered the "main" debugger contribution
            let as_contribution: DebuggerContribution = serde_json::from_value(other_debugger_contribution.clone()).unwrap_or_default();
            if is_debugger_main_contribution(&as_contribution) {
                *self.main_extension_description.borrow_mut() = Some(extension_description);
            }
        }
    }

    fn contribution_text(&self, key: &str) -> Option<String> {
        self.debugger_contribution.borrow().get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
    }

    pub fn label(&self) -> String {
        self.contribution_text("label").unwrap_or_else(|| self.debug_type())
    }

    /// `type`.
    pub fn debug_type(&self) -> String {
        self.debugger_contribution.borrow().get("type").and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    pub fn variables(&self) -> Option<Map<String, Value>> {
        self.debugger_contribution.borrow().get("variables").and_then(Value::as_object).cloned()
    }

    pub fn configuration_snippets(&self) -> Option<Vec<Value>> {
        self.debugger_contribution.borrow().get("configurationSnippets").and_then(Value::as_array).cloned()
    }

    pub fn languages(&self) -> Option<Vec<String>> {
        self.debugger_contribution
            .borrow()
            .get("languages")
            .and_then(Value::as_array)
            .map(|languages| languages.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect())
    }

    pub fn when(&self) -> Option<ContextKeyExpression> {
        self.debugger_when.clone()
    }

    pub fn hidden_when(&self) -> Option<ContextKeyExpression> {
        self.debugger_hidden_when.clone()
    }

    pub fn enabled(&self) -> bool {
        match (&self.debugger_when, &self.services.context_key_service) {
            (None, _) => true,
            (Some(when), Some(service)) => service.context_matches_rules(Some(when)),
            (Some(_), None) => panic!("Cannot read properties of undefined (reading 'contextMatchesRules')"),
        }
    }

    pub fn is_hidden_from_dropdown(&self) -> bool {
        let Some(hidden_when) = &self.debugger_hidden_when else {
            return false;
        };
        self.services.context_key_service.as_ref().expect("o serviço de chaves de contexto").context_matches_rules(Some(hidden_when))
    }

    /// `strings ?? uiMessages`.
    pub fn strings(&self) -> Option<Value> {
        let contribution = self.debugger_contribution.borrow();
        contribution.get("strings").filter(|v| !v.is_null()).or_else(|| contribution.get("uiMessages")).cloned()
    }

    pub fn interested_in_language(&self, language_id: &str) -> bool {
        self.languages().is_some_and(|languages| languages.iter().any(|l| l == language_id))
    }

    pub fn has_initial_configuration(&self) -> bool {
        self.debugger_contribution.borrow().get("initialConfigurations").is_some_and(|v| !v.is_null())
    }

    pub fn has_dynamic_configuration_providers(&self) -> bool {
        self.services
            .debug_service
            .as_ref()
            .expect("o serviço de depuração")
            .has_debug_configuration_provider(&self.debug_type(), Some(DebugConfigurationProviderTriggerKind::Dynamic))
    }

    pub fn has_configuration_provider(&self) -> bool {
        self.services.debug_service.as_ref().expect("o serviço de depuração").has_debug_configuration_provider(&self.debug_type(), None)
    }

    /// `getInitialConfigurationContent(initialConfigs)`.
    pub fn get_initial_configuration_content(&self, initial_configs: Option<Vec<IConfig>>) -> Promise<String> {
        // at this point we got some configs from the package.json and/or from registered DebugConfigurationProviders
        let mut initial_configurations: Vec<Value> =
            self.debugger_contribution.borrow().get("initialConfigurations").and_then(Value::as_array).cloned().unwrap_or_default();
        if let Some(initial_configs) = initial_configs {
            initial_configurations.extend(initial_configs);
        }

        let eol_source = self.services.eol.as_ref().map(|eol| eol()).unwrap_or_default();
        let eol = if eol_source == "\r\n" { "\r\n" } else { "\n" };
        let configs = {
            let mut buffer = Vec::new();
            let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
            let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
            serde::Serialize::serialize(&Value::Array(initial_configurations), &mut serializer).expect("JSON serializável");
            String::from_utf8(buffer).expect("utf-8").split('\n').map(|line| format!("\t{line}")).collect::<Vec<_>>().join(eol).trim().to_owned()
        };
        let comment1 = localize("launch.config.comment1", "Use IntelliSense to learn about possible attributes.", &[]);
        let comment2 = localize("launch.config.comment2", "Hover to view descriptions of existing attributes.", &[]);
        let comment3 = localize("launch.config.comment3", "For more information, visit: {0}", &["https://go.microsoft.com/fwlink/?linkid=830387"]);

        let mut content = [
            "{".to_owned(),
            format!("\t// {comment1}"),
            format!("\t// {comment2}"),
            format!("\t// {comment3}"),
            "\t\"version\": \"0.2.0\",".to_owned(),
            format!("\t\"configurations\": {configs}"),
            "}".to_owned(),
        ]
        .join(eol);

        // fix formatting
        let editor_config = self.services.configuration.as_ref().map(|configuration| configuration()).unwrap_or(Value::Null);
        if editor_config.get("editor").and_then(|e| e.get("insertSpaces")).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false))) {
            let tab_size = editor_config["editor"]["tabSize"].as_u64().unwrap_or(4) as usize;
            content = content.replace('\t', &" ".repeat(tab_size));
        }

        Promise::resolved(content)
    }

    pub fn get_main_extension_descriptor(&self) -> Rc<ExtensionDescription> {
        self.main_extension_description.borrow().clone().unwrap_or_else(|| self.merged_extension_descriptions.borrow()[0].clone())
    }

    /// `getSchemaAttributes(definitions)`.
    pub fn get_schema_attributes(&self, definitions: &mut Map<String, Value>) -> Option<Vec<Value>> {
        let configuration_attributes = self.debugger_contribution.borrow().get("configurationAttributes").and_then(Value::as_object).cloned()?;
        let debug_type = self.debug_type();
        let label = self.label();
        let deprecated = self.contribution_text("deprecated");
        let common_properties: Vec<String> =
            definitions.get("common").and_then(|c| c.get("properties")).and_then(Value::as_object).map(|p| p.keys().cloned().collect()).unwrap_or_default();

        // fill in the default configuration attributes shared by all adapters.
        let mut result = Vec::new();
        let mut updated_attributes = Map::new();
        for (request, attributes) in configuration_attributes {
            let definition_id = format!("{debug_type}:{request}");
            let platform_specific_definition_id = format!("{debug_type}:{request}:platform");
            let default_required: Vec<Value> = ["name", "type", "request"].iter().map(|s| json!(s)).collect();
            let mut attributes = attributes.as_object().cloned().unwrap_or_default();
            let required: Vec<Value> = attributes.get("required").and_then(Value::as_array).cloned().unwrap_or_default();
            attributes.insert(
                "required".to_owned(),
                Value::Array(if required.is_empty() { default_required } else { default_required.into_iter().chain(required).collect() }),
            );
            attributes.insert("additionalProperties".to_owned(), json!(false));
            attributes.insert("type".to_owned(), json!("object"));
            let mut properties = attributes.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
            let deprecation_message = deprecated.clone().or_else(|| if self.enabled() { None } else { Some(debugger_disabled_message(&debug_type)) });
            properties.insert(
                "type".to_owned(),
                json!({
                    "enum": [debug_type],
                    "enumDescriptions": [label],
                    "description": localize("debugType", "Type of configuration.", &[]),
                    "pattern": "^(?!node2)",
                    "deprecationMessage": deprecation_message,
                    "doNotSuggest": deprecated.is_some(),
                    "errorMessage": localize("debugTypeNotRecognised", "The debug type is not recognized. Make sure that you have a corresponding debug extension installed and that it is enabled.", &[]),
                    "patternErrorMessage": localize("node2NotSupported", "\"node2\" is no longer supported, use \"node\" instead and set the \"protocol\" attribute to \"inspector\".", &[]),
                }),
            );
            properties.insert(
                "request".to_owned(),
                json!({
                    "enum": [request],
                    "description": localize("debugRequest", "Request type of configuration. Can be \"launch\" or \"attach\".", &[]),
                }),
            );
            for prop in &common_properties {
                properties.insert(prop.clone(), json!({ "$ref": format!("#/definitions/common/properties/{prop}") }));
            }
            let mut malformed_property_names: Vec<String> = Vec::new();
            let names: Vec<String> = properties.keys().cloned().collect();
            for name in names {
                let property = properties.get_mut(&name).expect("a propriedade existe");
                // A debugger extension may contribute a malformed property whose value is not a schema object (e.g. the bare
                // string 'integer' instead of `{ "type": "integer" }`). Skip those so one bad contribution does not throw and
                // abort schema generation for every debugger.
                if is_object(property) {
                    // Use schema allOf property to get independent error reporting #21113
                    apply_deprecated_variable_message_to_json(property);
                } else {
                    malformed_property_names.push(name);
                }
            }
            if !malformed_property_names.is_empty() {
                let log_service = self.services.log_service.as_ref().expect("o serviço de log");
                log_service.warn(&format!("Ignoring malformed debug configuration schema properties for type '{}': {}", debug_type, malformed_property_names.join(", ")));
            }

            attributes.insert("properties".to_owned(), Value::Object(properties.clone()));
            definitions.insert(definition_id, Value::Object(attributes.clone()));
            definitions.insert(
                platform_specific_definition_id.clone(),
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "properties": properties.iter().filter(|(key, _)| !matches!(key.as_str(), "type" | "request" | "name")).map(|(k, v)| (k.clone(), v.clone())).collect::<Map<String, Value>>(),
                }),
            );

            // Don't add the OS props to the real attributes object so they don't show up in 'definitions'
            let mut attributes_copy = attributes.clone();
            let mut copy_properties = properties.clone();
            copy_properties.insert(
                "windows".to_owned(),
                json!({
                    "$ref": format!("#/definitions/{platform_specific_definition_id}"),
                    "description": localize("debugWindowsConfiguration", "Windows specific launch configuration attributes.", &[]),
                }),
            );
            copy_properties.insert(
                "osx".to_owned(),
                json!({
                    "$ref": format!("#/definitions/{platform_specific_definition_id}"),
                    "description": localize("debugOSXConfiguration", "OS X specific launch configuration attributes.", &[]),
                }),
            );
            copy_properties.insert(
                "linux".to_owned(),
                json!({
                    "$ref": format!("#/definitions/{platform_specific_definition_id}"),
                    "description": localize("debugLinuxConfiguration", "Linux specific launch configuration attributes.", &[]),
                }),
            );
            attributes_copy.insert("properties".to_owned(), Value::Object(copy_properties));

            // o original muda o objeto `attributes` da contribuição no lugar (`required`, `additionalProperties`, `type`, `properties`)
            updated_attributes.insert(request.clone(), Value::Object(attributes));
            result.push(Value::Object(attributes_copy));
        }
        if let Some(contribution) = self.debugger_contribution.borrow_mut().as_object_mut() {
            contribution.insert("configurationAttributes".to_owned(), Value::Object(updated_attributes));
        }

        Some(result)
    }
}

/// `applyDeprecatedVariableMessage(property)` sobre a propriedade JSON: o `pattern` e o `patternErrorMessage` passam pela função
/// do porte (que trabalha sobre `Dynamic`) e voltam ao objeto.
fn apply_deprecated_variable_message_to_json(property: &mut Value) {
    let text = |key: &str| property.get(key).and_then(Value::as_str).map(str::to_owned);
    let mut entries: Vec<(String, Dynamic)> = Vec::new();
    for key in ["pattern", "patternErrorMessage"] {
        if let Some(value) = text(key) {
            entries.push((key.to_owned(), Dynamic::string(&value)));
        }
    }
    let schema = Dynamic::object(entries);
    apply_deprecated_variable_message(&schema);
    for key in ["pattern", "patternErrorMessage"] {
        if let Dynamic::String(value) = schema.get_str(key)
            && schema.get_str(key).is_truthy()
            && let Some(object) = property.as_object_mut()
        {
            object.insert(key.to_owned(), Value::String(value.to_string_lossy()));
        }
    }
}

impl IDebugger for Debugger {
    fn debug_type(&self) -> String {
        Debugger::debug_type(self)
    }

    fn create_debug_adapter(&self, session: Rc<dyn IDebugSessionFull>) -> Promise<Rc<dyn IDebugAdapter>> {
        let adapter_manager = self.services.adapter_manager.clone().expect("o gerenciador de adaptadores");
        let debug_type = self.debug_type();
        je_base::async_::async_fn(async move {
            adapter_manager.activate_debuggers("onDebugAdapterProtocolTracker", &debug_type).await?;
            match adapter_manager.create_debug_adapter(session) {
                Some(da) => Ok(da),
                None => Err(Error::new(localize("cannot.find.da", "Cannot find debug adapter for type '{0}'.", &[&debug_type]))),
            }
        })
    }

    fn run_in_terminal(&self, args: Value, session_id: &str) -> Promise<Option<i64>> {
        self.services.adapter_manager.as_ref().expect("o gerenciador de adaptadores").run_in_terminal(&self.debug_type(), args, session_id)
    }

    fn start_debugging(&self, args: IConfig, parent_session_id: &str) -> Promise<bool> {
        self.services.debug_service.as_ref().expect("o serviço de depuração").start_debugging(args, parent_session_id)
    }

    fn get_custom_telemetry_endpoint(&self) -> Option<ITelemetryEndpoint> {
        let ai_key = self.contribution_text("aiKey")?;

        let clean = self.services.clean_remote_authority.as_ref().map(|clean| clean()).unwrap_or_default();
        let send_error_telemetry = clean != "other";
        Some(ITelemetryEndpoint { id: format!("{}.{}", self.get_main_extension_descriptor().manifest.publisher, self.debug_type()), ai_key, send_error_telemetry })
    }
}

impl Debugger {
    /// `substituteVariables(folder, config)`.
    pub fn substitute_variables(&self, folder: Option<WorkspaceFolder>, config: IConfig) -> Promise<IConfig> {
        let adapter_manager = self.services.adapter_manager.clone().expect("o gerenciador de adaptadores");
        let resolve = self.services.resolve_with_interaction_replace.clone().expect("o resolvedor de variáveis");
        let debug_type = self.debug_type();
        let variables = self.variables();
        je_base::async_::async_fn(async move {
            let substituted_config = adapter_manager.substitute_variables(&debug_type, folder.clone(), config).await?;
            let target = substituted_config.get("__configurationTarget").and_then(Value::as_str).map(str::to_owned);
            resolve(folder, substituted_config, variables, target).await
        })
    }
}
