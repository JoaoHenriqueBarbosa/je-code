//! Porte de `src/vs/workbench/api/common/extHostTimeline.ts`: o lado da extensão dos provedores de timeline
//! (`vscode.workspace.registerTimelineProvider`), os itens que ficam guardados para os comandos do Timeline view e
//! o processador de argumentos que os devolve às extensões.
//!
//! Divergências:
//! - **Itens e valores da API.** Um `vscode.TimelineItem`, o `iconPath`, o `tooltip` (`MarkdownString`), o `command` e o
//!   `accessibilityInformation` que a extensão atribui são [`Dynamic`] (ver `ext_host_values`): o `===` do original é a
//!   identidade do objeto ([`Dynamic::strict_equals`]), e `Object.freeze` é o `freeze` do `Dynamic`. O argumento que
//!   o Timeline view passa aos comandos (`TimelineActionContext`) também é um [`Dynamic`] com `$mid`.
//! - **`vscode.TimelineProvider`/`WorkspaceTimelineProvider`** são os structs [`ApiTimelineProvider`] e
//!   [`ApiWorkspaceTimelineProvider`] (campos e fechos no lugar dos métodos do objeto), `Event<Uri | Uri[] | undefined>`
//!   é `Event<Option<ApiTimelineChange>>` e `ProviderResult<Timeline>` é `MaybePromise<Option<ApiTimeline>>`.
//! - **`ExtHostCommands`.** O construtor só usa `registerArgumentProcessor`; [`ArgumentProcessorRegistry`] é essa fatia
//!   do contrato (o `ExtHostCommands` a implementa), e é o que o teste substitui, como o `mock<ExtHostCommands>()` do
//!   original. `CommandsConverter` é o [`ICommandsConverter`].
//! - **Erros.** `registerTimelineProvider` e `registerWorkspaceTimelineProvider` devolvem `Result` (o `throw` de
//!   `_checkId`); o resto que lança no original (propriedade com tipo errado) é [`errors::throw`] dentro do processador
//!   de argumentos, que não tem como devolver erro, ou o erro do `Promise` em `$getTimeline`.
//! - `IExtHostTimeline` e o `createDecorator` do serviço não existem (a injeção é do registro de serviços, ainda
//!   inexistente); `$getTimeline` é o `rpc_get_timeline` do [`ExtHostTimelineShape`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{number_to_js_string, Dynamic, ObjectRef, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{to_disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::accessibility::IAccessibilityInformation;

use super::ext_host_commands::{ArgumentProcessor, ExtHostCommands};
use super::ext_host_protocol::{ExtHostTimelineShape, IMainContext, MainContext, MainThreadTimelineShape};
use super::ext_host_type_converters::{ICommandsConverter, MarkdownString as MarkdownStringConverter};
use super::ext_host_types::{self as types, Command};
use super::ext_host_values::{is_instance_of, tag, ApiValue};
use crate::contrib::timeline::common::timeline::{
    Timeline, TimelineItem, TimelineOptions, TimelineProviderDescriptor, TimelineScheme, TimelineTriggerKind, WorkspaceTimelineProviderDescriptor,
};
use crate::services::extensions::extensions::is_proposed_api_enabled;

/// `vscode.TimelineOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiTimelineOptions {
    pub cursor: Option<String>,
    pub limit: Option<f64>,
    pub trigger_kind: types::TimelineTriggerKind,
}

/// `vscode.Timeline`: os itens são os `vscode.TimelineItem` da extensão.
#[derive(Clone)]
pub struct ApiTimeline {
    pub items: Vec<Dynamic>,
    pub cursor: Option<String>,
}

/// `Uri | Uri[]`: o que `onDidChangeTimeline` dispara (o `undefined` é o `None` do `Option` que o envolve).
#[derive(Clone, Debug)]
pub enum ApiTimelineChange {
    Uri(URI),
    Uris(Vec<URI>),
}

/// O `provideTimeline` de um provedor: `ProviderResult<Timeline>`.
pub type ProvideApiTimeline = Rc<dyn Fn(&URI, &ApiTimelineOptions, CancellationToken) -> MaybePromise<Option<ApiTimeline>>>;

/// `vscode.TimelineProvider`.
#[derive(Clone)]
pub struct ApiTimelineProvider {
    pub id: String,
    pub label: String,
    pub on_did_change_timeline: Option<Event<Option<ApiTimelineChange>>>,
    pub provide_timeline: ProvideApiTimeline,
}

/// O `provideWorkspaceTimeline` de um provedor: `ProviderResult<Timeline>`.
pub type ProvideApiWorkspaceTimeline = Rc<dyn Fn(&ApiTimelineOptions, CancellationToken) -> MaybePromise<Option<ApiTimeline>>>;

/// `vscode.WorkspaceTimelineProvider`.
#[derive(Clone)]
pub struct ApiWorkspaceTimelineProvider {
    pub id: String,
    pub label: String,
    pub on_did_change_workspace_timeline: Option<Event<()>>,
    pub provide_workspace_timeline: ProvideApiWorkspaceTimeline,
}

/// A fatia de `ExtHostCommands` que o `ExtHostTimeline` usa.
pub trait ArgumentProcessorRegistry {
    /// `registerArgumentProcessor(processor)`.
    fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>);
}

impl ArgumentProcessorRegistry for ExtHostCommands {
    fn register_argument_processor(&self, processor: Rc<dyn ArgumentProcessor>) {
        ExtHostCommands::register_argument_processor(self, processor);
    }
}

/// O argumento que o Timeline view passa aos comandos para um item: `{ $mid, handle, source, uri }`.
fn is_timeline_action_context(arg: &Dynamic) -> bool {
    matches!(arg, Dynamic::Object(_)) && matches!(arg.get_str("$mid"), Dynamic::Number(mid) if mid == f64::from(MarshalledId::TimelineActionContext as i32))
}

/// `value` como texto, ou o `TypeError` de uma propriedade com tipo errado.
fn string_property(value: &Dynamic, key: &str) -> Result<String, Error> {
    match value.get_str(key) {
        Dynamic::String(text) => Ok(text.to_string_lossy()),
        _ => Err(Error::type_error(format!("TimelineItem.{key} must be a string"))),
    }
}

fn optional_string_property(value: &Dynamic, key: &str) -> Result<Option<String>, Error> {
    match value.get_str(key) {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        Dynamic::String(text) => Ok(Some(text.to_string_lossy())),
        _ => Err(Error::type_error(format!("TimelineItem.{key} must be a string"))),
    }
}

fn optional_uri(value: &Dynamic) -> Result<Option<URI>, Error> {
    if value.is_nullish() { Ok(None) } else { URI::from_dynamic(value).map(Some) }
}

/// `Object.freeze(value)`.
fn freeze(value: Dynamic) -> Dynamic {
    match &value {
        Dynamic::Object(object) => object.freeze(),
        Dynamic::Array(array) => array.freeze(),
        _ => {}
    }
    value
}

fn set_property(object: &ObjectRef, key: &str, value: Dynamic) {
    object.set(&JsString::from(key), value);
}

/// `toReadonlyCopy`: copies the public properties of a timeline item, except its command, without sharing mutable
/// values with it.
fn to_readonly_copy(item: &Dynamic) -> Dynamic {
    let copy = types::new_timeline_item(item.get_str("label"), item.get_str("timestamp"));
    set_property(&copy, "id", item.get_str("id"));
    set_property(&copy, "iconPath", copy_icon_path(&item.get_str("iconPath")));
    set_property(&copy, "description", item.get_str("description"));
    let tooltip = item.get_str("tooltip");
    set_property(&copy, "tooltip", if is_instance_of(&tooltip, "MarkdownString") { copy_markdown_string(&tooltip) } else { tooltip });
    set_property(&copy, "resourceUri", item.get_str("resourceUri"));
    set_property(&copy, "contextValue", item.get_str("contextValue"));
    let accessibility_information = item.get_str("accessibilityInformation");
    set_property(
        &copy,
        "accessibilityInformation",
        if accessibility_information.is_truthy() { freeze(Dynamic::object(accessibility_information.own_entries())) } else { accessibility_information },
    );
    freeze(Dynamic::Object(copy))
}

fn copy_icon_path(icon_path: &Dynamic) -> Dynamic {
    if is_instance_of(icon_path, "ThemeIcon") {
        let color = icon_path.get_str("color");
        let color = if color.is_truthy() { freeze(tag(Dynamic::object([("id", color.get_str("id"))]), "ThemeColor")) } else { color };
        return freeze(tag(Dynamic::object([("id", icon_path.get_str("id")), ("color", color)]), ThemeIcon::CLASS));
    }
    if matches!(icon_path, Dynamic::Object(_)) && icon_path.has_property(&JsString::from("light")) && icon_path.has_property(&JsString::from("dark")) {
        return freeze(Dynamic::object([("light", icon_path.get_str("light")), ("dark", icon_path.get_str("dark"))]));
    }
    icon_path.clone()
}

fn copy_markdown_string(markdown: &Dynamic) -> Dynamic {
    let is_trusted = markdown.get_str("isTrusted");
    let is_trusted = if let Dynamic::Object(_) = &is_trusted {
        let Dynamic::Array(enabled_commands) = is_trusted.get_str("enabledCommands") else {
            errors::throw(Error::type_error("MarkdownString.isTrusted.enabledCommands must be an array"));
        };
        let mut entries: Vec<(JsString, Dynamic)> = is_trusted.own_entries();
        let enabled_commands = freeze(Dynamic::array(enabled_commands.to_vec()));
        match entries.iter_mut().find(|(key, _)| *key == "enabledCommands") {
            Some(slot) => slot.1 = enabled_commands,
            None => entries.push((JsString::from("enabledCommands"), enabled_commands)),
        }
        freeze(Dynamic::object(entries))
    } else {
        is_trusted
    };
    tag(
        Dynamic::object([
            ("value", markdown.get_str("value")),
            ("supportThemeIcons", Dynamic::Bool(markdown.get_str("supportThemeIcons").is_truthy())),
            ("isTrusted", is_trusted),
            ("supportHtml", markdown.get_str("supportHtml")),
            ("supportAlertSyntax", markdown.get_str("supportAlertSyntax")),
            ("baseUri", markdown.get_str("baseUri")),
        ]),
        "MarkdownString",
    )
}

/// O `provide` de uma registração: o provedor de arquivos (`uri` presente) ou o do workspace.
type Provide = Rc<dyn Fn(Option<&URI>, &ApiTimelineOptions, CancellationToken) -> MaybePromise<Option<ApiTimeline>>>;

/// `TimelineProviderRegistration`: a registered timeline or workspace timeline provider and the items it returned,
/// which are passed to commands.
struct TimelineProviderRegistration {
    id: String,
    extension: ExtensionIdentifier,
    items_by_uri: RefCell<HashMap<Option<String>, HashMap<String, Dynamic>>>,
    command_disposables: Rc<DisposableStore>,
    disposed: Cell<bool>,
    provide: Provide,
    command_converter: Rc<dyn ICommandsConverter>,
}

impl TimelineProviderRegistration {
    fn new(id: &str, extension: &ExtensionIdentifier, provide: Provide, command_converter: Rc<dyn ICommandsConverter>) -> Rc<Self> {
        Rc::new(Self {
            id: id.to_owned(),
            extension: extension.clone(),
            items_by_uri: RefCell::new(HashMap::new()),
            command_disposables: DisposableStore::new(),
            disposed: Cell::new(false),
            provide,
            command_converter,
        })
    }

    fn provide_timeline(self: &Rc<Self>, uri: Option<URI>, options: TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>> {
        let this = self.clone();
        async_fn(async move {
            if options.reset_cache == Some(true) {
                this.command_disposables.clear();

                // For now, only allow the caching of a single Uri
                this.items_by_uri.borrow_mut().clear();
            }

            let api_options = ApiTimelineOptions {
                cursor: options.cursor.clone(),
                limit: options.limit,
                trigger_kind: if options.trigger_kind == TimelineTriggerKind::Invoke {
                    types::TimelineTriggerKind::Invoke
                } else {
                    types::TimelineTriggerKind::Automatic
                },
            };
            let result = await_maybe((this.provide)(uri.as_ref(), &api_options, token)).await?;
            let Some(result) = result else {
                return Ok(None);
            };
            if this.disposed.get() {
                return Ok(None);
            }

            let mut items = Vec::with_capacity(result.items.len());
            let mut cached: Vec<(String, Dynamic)> = Vec::new();
            for item in &result.items {
                let converted = this.convert_item(item)?;
                cached.push((converted.handle.clone(), item.clone()));
                items.push(converted);
            }
            if options.cache_results == Some(true) {
                let uri_key = uri.as_ref().map(|uri| uri.to_string(false).to_string_lossy());
                this.items_by_uri.borrow_mut().entry(uri_key).or_default().extend(cached);
            }

            Ok(Some(Timeline { source: this.id.clone(), cursor: result.cursor, items }))
        })
    }

    fn get_item(&self, uri: Option<&URI>, handle: &str) -> Option<Dynamic> {
        let uri_key = uri.map(|uri| uri.to_string(false).to_string_lossy());
        self.items_by_uri.borrow().get(&uri_key)?.get(handle).cloned()
    }

    fn convert_item(&self, item: &Dynamic) -> Result<TimelineItem, Error> {
        let id = optional_string_property(item, "id")?;
        let timestamp = match item.get_str("timestamp") {
            Dynamic::Number(timestamp) => timestamp,
            _ => return Err(Error::type_error("TimelineItem.timestamp must be a number")),
        };
        let handle = format!("{}|{}", self.id, id.clone().unwrap_or_else(|| number_to_js_string(timestamp)));

        let mut icon = None;
        let mut icon_dark = None;
        let mut theme_icon = None;
        let icon_path = item.get_str("iconPath");
        if icon_path.is_truthy() {
            if is_instance_of(&icon_path, "ThemeIcon") {
                theme_icon = Some(ThemeIcon::from_dynamic(&icon_path)?);
            } else if is_instance_of(&icon_path, "Uri") {
                icon = Some(URI::from_dynamic(&icon_path)?);
                icon_dark = icon.clone();
            } else {
                icon = Some(URI::from_dynamic(&icon_path.get_str("light"))?);
                icon_dark = Some(URI::from_dynamic(&icon_path.get_str("dark"))?);
            }
        }

        let tooltip = item.get_str("tooltip");
        let tooltip = if is_instance_of(&tooltip, "MarkdownString") {
            Some(StringOrMarkdown::Markdown(MarkdownStringConverter::from_markdown(&types::MarkdownString::from_dynamic(&tooltip)?)))
        } else if let Dynamic::String(text) = &tooltip {
            Some(StringOrMarkdown::String(text.to_string_lossy()))
        } else {
            None
        };

        let accessibility_information = item.get_str("accessibilityInformation");
        let accessibility_information = if accessibility_information.is_nullish() {
            None
        } else {
            Some(IAccessibilityInformation {
                label: string_property(&accessibility_information, "label")?,
                role: optional_string_property(&accessibility_information, "role")?,
            })
        };

        let command = item.get_str("command");
        let command = if command.is_truthy() {
            self.command_converter.to_internal(Some(&Command::from_dynamic(&command)?), &self.command_disposables)?
        } else {
            None
        };

        Ok(TimelineItem {
            handle,
            source: self.id.clone(),
            id,
            label: string_property(item, "label")?,
            description: optional_string_property(item, "description")?,
            tooltip,
            timestamp,
            accessibility_information,
            icon,
            icon_dark,
            theme_icon,
            command,
            context_value: optional_string_property(item, "contextValue")?,
            resource_uri: optional_uri(&item.get_str("resourceUri"))?,
            relative_time: None,
            relative_time_full_word: None,
            hide_relative_time: None,
        })
    }
}

impl IDisposable for TimelineProviderRegistration {
    fn dispose(&self) {
        self.disposed.set(true);
        self.items_by_uri.borrow_mut().clear();
        self.command_disposables.dispose();
    }
}

type Registrations = RefCell<HashMap<String, Rc<TimelineProviderRegistration>>>;

/// Os provedores registrados, de arquivos e do workspace: o que `$getTimeline` e o processador de argumentos leem.
#[derive(Default)]
struct ProviderRegistry {
    providers: Registrations,
    workspace_providers: Registrations,
}

impl ProviderRegistry {
    /// `_resolveItem`.
    fn resolve_item(&self, context: &Dynamic, extension: Option<&ExtensionDescription>) -> Option<Dynamic> {
        let extension = extension.filter(|extension| is_proposed_api_enabled(extension, "timeline"))?;

        let uri = optional_uri(&context.get_str("uri")).unwrap_or_else(|error| errors::throw(error));
        let source = string_property(context, "source").unwrap_or_else(|error| errors::throw(error));
        let handle = string_property(context, "handle").unwrap_or_else(|error| errors::throw(error));
        let registrations = if uri.is_none() { &self.workspace_providers } else { &self.providers };
        let registration = registrations.borrow().get(&source).cloned()?;
        let item = registration.get_item(uri.as_ref(), &handle)?;

        // Only the extension that provided the item gets the original object, other extensions get a read-only copy of its public properties.
        if ExtensionIdentifier::equals(Some(&registration.extension.value), Some(&extension.identifier.value)) {
            return Some(item);
        }

        Some(to_readonly_copy(&item))
    }
}

/// O processador de argumentos que o construtor registra.
struct TimelineArgumentProcessor {
    registry: Rc<ProviderRegistry>,
}

impl ArgumentProcessor for TimelineArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, extension: Option<&ExtensionDescription>) -> Dynamic {
        if is_timeline_action_context(arg) {
            return self.registry.resolve_item(arg, extension).unwrap_or(Dynamic::Undefined);
        }

        // The items selected in the Timeline view
        if let Dynamic::Array(contexts) = arg {
            let contexts = contexts.to_vec();
            if !contexts.is_empty() && contexts.iter().all(is_timeline_action_context) {
                let items: Vec<Dynamic> = contexts.iter().filter_map(|context| self.registry.resolve_item(context, extension)).collect();
                return if items.is_empty() { Dynamic::Undefined } else { Dynamic::array(items) };
            }
        }

        arg.clone()
    }
}

/// `ExtHostTimeline`.
pub struct ExtHostTimeline {
    proxy: Rc<dyn MainThreadTimelineShape>,
    registry: Rc<ProviderRegistry>,
}

impl ExtHostTimeline {
    pub fn new(main_context: &dyn IMainContext, commands: &dyn ArgumentProcessorRegistry) -> Rc<Self> {
        let registry = Rc::new(ProviderRegistry::default());
        commands.register_argument_processor(Rc::new(TimelineArgumentProcessor { registry: registry.clone() }));
        Rc::new(Self { proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_TIMELINE), registry })
    }

    pub fn register_timeline_provider(
        &self,
        scheme: TimelineScheme,
        provider: ApiTimelineProvider,
        extension: &ExtensionIdentifier,
        command_converter: Rc<dyn ICommandsConverter>,
    ) -> Result<DisposableRef, Error> {
        Self::check_id(&provider.id, extension, &self.registry.providers, &self.registry.workspace_providers)?;

        let provide_timeline = provider.provide_timeline.clone();
        let registration = TimelineProviderRegistration::new(
            &provider.id,
            extension,
            Rc::new(move |uri, options, token| match uri {
                Some(uri) => provide_timeline(uri, options, token),
                None => MaybePromise::Value(None),
            }),
            command_converter,
        );
        let proxy = self.proxy.clone();
        let on_did_change = provider.on_did_change_timeline.as_ref().map(|event| {
            let proxy = proxy.clone();
            let id = provider.id.clone();
            event.listen(move |change| {
                let uris = match change {
                    None => None,
                    Some(ApiTimelineChange::Uris(uris)) => Some(uris.clone()),
                    Some(ApiTimelineChange::Uri(uri)) => Some(vec![uri.clone()]),
                };
                proxy.rpc_emit_timeline_change_event(&id, uris);
            })
        });

        self.registry.providers.borrow_mut().insert(provider.id.clone(), registration.clone());
        self.proxy.rpc_register_timeline_provider(TimelineProviderDescriptor { id: provider.id.clone(), label: provider.label.clone(), scheme });

        let registry = self.registry.clone();
        let id = provider.id;
        Ok(to_disposable(move || {
            registry.providers.borrow_mut().remove(&id);
            proxy.rpc_unregister_timeline_provider(&id);
            if let Some(on_did_change) = on_did_change {
                on_did_change.dispose();
            }
            registration.dispose();
        }))
    }

    pub fn register_workspace_timeline_provider(
        &self,
        provider: ApiWorkspaceTimelineProvider,
        extension: &ExtensionIdentifier,
        command_converter: Rc<dyn ICommandsConverter>,
    ) -> Result<DisposableRef, Error> {
        Self::check_id(&provider.id, extension, &self.registry.workspace_providers, &self.registry.providers)?;

        let provide_workspace_timeline = provider.provide_workspace_timeline.clone();
        let registration = TimelineProviderRegistration::new(
            &provider.id,
            extension,
            Rc::new(move |_uri, options, token| provide_workspace_timeline(options, token)),
            command_converter,
        );
        let proxy = self.proxy.clone();
        let on_did_change = provider.on_did_change_workspace_timeline.as_ref().map(|event| {
            let proxy = proxy.clone();
            let id = provider.id.clone();
            event.listen(move |_| proxy.rpc_emit_workspace_timeline_change_event(&id))
        });

        self.registry.workspace_providers.borrow_mut().insert(provider.id.clone(), registration.clone());
        self.proxy.rpc_register_workspace_timeline_provider(WorkspaceTimelineProviderDescriptor { id: provider.id.clone(), label: provider.label.clone() });

        let registry = self.registry.clone();
        let id = provider.id;
        Ok(to_disposable(move || {
            registry.workspace_providers.borrow_mut().remove(&id);
            proxy.rpc_unregister_workspace_timeline_provider(&id);
            if let Some(on_did_change) = on_did_change {
                on_did_change.dispose();
            }
            registration.dispose();
        }))
    }

    /// Ids must be unique per kind of provider, and a timeline and a workspace timeline provider may only share an id
    /// when they come from the same extension.
    fn check_id(id: &str, extension: &ExtensionIdentifier, same_kind: &Registrations, other_kind: &Registrations) -> Result<(), Error> {
        let other = other_kind.borrow().get(id).cloned();
        if same_kind.borrow().contains_key(id)
            || other.is_some_and(|other| !ExtensionIdentifier::equals(Some(&other.extension.value), Some(&extension.value)))
        {
            return Err(Error::new(format!("Timeline Provider {id} already exists.")));
        }
        Ok(())
    }
}

impl ExtHostTimelineShape for ExtHostTimeline {
    fn rpc_get_timeline(&self, id: &str, uri: Option<URI>, options: TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>> {
        let registrations = if uri.is_none() { &self.registry.workspace_providers } else { &self.registry.providers };
        let registration = registrations.borrow().get(id).cloned();
        match registration {
            Some(registration) => registration.provide_timeline(uri, options, token),
            None => Promise::resolved(None),
        }
    }
}
