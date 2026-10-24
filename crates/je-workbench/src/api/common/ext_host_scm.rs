//! Porte de `src/vs/workbench/api/common/extHostSCM.ts`: o lado da extensão do controle de código-fonte
//! (`vscode.scm.createSourceControl`, `SourceControl`, `SourceControlResourceGroup`, `SourceControlInputBox`), o
//! processador de argumentos de comando (`$mid` de recurso, grupo e provedor) e as respostas ao protocolo
//! (`$provideOriginalResource`, `$validateInput`, `$provideHistoryItems`, `$provideArtifacts`...).
//!
//! Divergências:
//! - Os tipos da API `vscode` que o original só importa de `vscode.d.ts` e das propostas (`SourceControlResourceState`,
//!   `SourceControlResourceDecorations`, `QuickDiffProvider`, `SourceControlHistoryProvider`,
//!   `SourceControlArtifactProvider`, `SourceControlActionButton`, os itens de histórico e de artefatos...) são definidos
//!   aqui. Os provedores são traits (os métodos opcionais do original, `provideOriginalResource` e
//!   `resolveHistoryItemChangeRangeChatContext`, têm uma implementação padrão que se comporta como o método ausente), e
//!   `ProviderResult<T>` é [`ProviderResult`], um [`MaybePromise`] de `Option<T>` (`undefined` e `null` são `None`).
//! - Um objeto da API que a extensão cria e mantém (um `vscode.SourceControlResourceState`, que o `Map` da thread compara e
//!   guarda) é um `Rc<`[`SourceControlResourceState`]`>`; um objeto que a thread constrói e a extensão recebe
//!   (`SourceControl`, `SourceControlResourceGroup`, `SourceControlInputBox`) é o próprio `Rc` do tipo. Os getters e setters
//!   são métodos; os que chamam `checkProposedApiEnabled` devolvem [`Result`]. `ExtHostSourceControl`,
//!   `ExtHostSourceControlResourceGroup` e `ExtHostSCMInputBox` guardam o `proxy`, as extensões e os serviços que o original
//!   guarda em campos privados (`#proxy`).
//! - Um objeto da API que atravessa um [`Dynamic`] (o argumento de um comando de recurso, grupo ou provedor) é um objeto com
//!   carga nativa. O grupo e o provedor guardam um [`Weak`] de si mesmos (a identidade do [`Dynamic`] é estável, como a do
//!   objeto do original), o que a [`from_dynamic`](ExtHostSourceControl::from_dynamic) devolve. O recurso é um objeto
//!   com um `Rc` do estado (sem ciclo possível), e cada leitura cria um [`Dynamic`] novo (o que o `Rc` guarda é o mesmo).
//! - O `@debounce(100)` de `eventuallyAddResourceGroups` e `eventuallyUpdateResourceStates` é o [`Debounce`] de
//!   `je_base::decorators`, com um [`Weak`] da fonte (um `Rc` fecharia um ciclo). O `throw` do `toInternal` dentro do
//!   `debounce` (que não tem a quem lançar) vai para [`errors::on_unexpected_error`].
//! - `ExtHostCommands` é guardado como [`Weak`] (o processador de argumentos que a fonte registra nele mora no
//!   `ExtHostCommands`, e um `Rc` fecharia um ciclo); o conversor de comandos é guardado direto.
//! - `SourceControlInputBoxValidationType.from` (de `extHostTypeConverters.ts`) é [`source_control_input_box_validation_type_from`];
//!   como o tipo é um enum, o `default: throw` do original não existe. O `typeof fn !== 'function'` do setter de
//!   `validateInput` não existe pelo mesmo motivo (o tipo é uma função).
//! - O corpo `try/catch` de uma função assíncrona do original (`$resolveHistoryItem`...) captura as rejeições do provedor; uma
//!   exceção síncrona de um método do provedor (que em Rust seria um pânico) não é capturada.
//! - `compareCommands` compara os argumentos com `<` do JavaScript: números e textos como o original, os demais valores
//!   primitivos pelo número que o JavaScript lhes daria, e os objetos pelo texto `[object Object]` (todos iguais entre si).
//! - `structuralEquals(this._actionButton, actionButton)` compara a forma [`Dynamic`] do botão (só as chaves presentes entram,
//!   uma chave com o valor explícito `undefined` não existe aqui).
//! - As estáticas `_handlePool` de `ExtHostSourceControl` e `ExtHostSourceControlResourceGroup` são contadores por thread.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::rc::{Rc, Weak};

use je_base::arrays::{equals as arrays_equals, sort_by_compare, sorted_diff};
use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::collator::locale_compare;
use je_base::comparers::compare_paths;
use je_base::decorators::Debounce;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::equals::structural_equals;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::html_content::IMarkdownString;
use je_base::lifecycle::{DisposableMap, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::themables::ThemeIcon;
use je_base::uri::{encode_uri_component, UriComponents, URI};
use je_base::JsString;
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifierMap;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::resources::{fs_path, uri_file};

use super::ext_host_commands::{ArgumentProcessor, CommandsConverter, ExtHostCommands};
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_protocol::{
    ExtHostSCMShape, IMainContext, ISCMHistoryOptions, ICommandDto, IconPathDto, InputValidationType, MainContext, MainThreadSCMShape,
    MainThreadTelemetryShape, SCMActionButtonCommandDto, SCMActionButtonDto, SCMArtifactDto, SCMArtifactGroupDto, SCMGroupFeatures,
    SCMGroupRegistration, SCMHistoryItemChangeDto, SCMHistoryItemDto, SCMHistoryItemRefDto, SCMHistoryItemRefsChangeEventDto,
    SCMHistoryItemStatisticsDto, SCMHistoryItemTooltipDto, SCMIconDto, SCMProviderFeatures, SCMRawResource, SCMRawResourceSplice,
    SCMRawResourceSplices,
};
use super::ext_host_type_converters::{self as type_converters, ICommandsConverter};
use super::ext_host_types::{self as types, host_object, weak_host_dynamic, Command};
use super::ext_host_values::{is_instance_of, ApiValue};
use crate::api::common::ext_host_document_data::TextDocument;
use crate::common::comments::handle_of;
use crate::services::extensions::extensions::{check_proposed_api_enabled, is_proposed_api_enabled};

type ProviderHandle = u32;
type GroupHandle = u32;
type ResourceStateHandle = u32;

/// `vscode.ProviderResult<T>`: o valor, a promessa dele, ou `undefined`/`null` (`None`).
pub type ProviderResult<T> = MaybePromise<Option<T>>;

type SourceControlMap = RefCell<BTreeMap<ProviderHandle, Rc<ExtHostSourceControl>>>;

// ---- os tipos da API ------------------------------------------------------------------------------------

/// `vscode.SourceControlResourceThemableDecorations.iconPath`: `string | Uri | ThemeIcon`.
#[derive(Clone, Debug)]
pub enum DecorationIconPath {
    Path(String),
    Uri(URI),
    ThemeIcon(ThemeIcon),
}

/// `vscode.SourceControlResourceThemableDecorations`.
#[derive(Clone, Debug, Default)]
pub struct SourceControlResourceThemableDecorations {
    pub icon_path: Option<DecorationIconPath>,
}

/// `vscode.SourceControlResourceDecorations`.
#[derive(Clone, Debug, Default)]
pub struct SourceControlResourceDecorations {
    pub icon_path: Option<DecorationIconPath>,
    pub light: Option<SourceControlResourceThemableDecorations>,
    pub dark: Option<SourceControlResourceThemableDecorations>,
    pub strike_through: Option<bool>,
    pub faded: Option<bool>,
    pub tooltip: Option<String>,
}

/// `vscode.SourceControlResourceState` (com os dois campos da proposta `scmMultiDiffEditor`; o `multiFileDiffEditorModifiedUri`
/// e o `multiDiffEditorOriginalUri` têm esses nomes no original).
#[derive(Clone, Debug)]
pub struct SourceControlResourceState {
    pub resource_uri: URI,
    pub command: Option<Command>,
    pub decorations: Option<SourceControlResourceDecorations>,
    pub context_value: Option<String>,
    pub multi_diff_editor_original_uri: Option<URI>,
    pub multi_file_diff_editor_modified_uri: Option<URI>,
}

impl SourceControlResourceState {
    /// O objeto do estado que atravessa um [`Dynamic`] (`resourceUri` mais a carga nativa).
    pub fn to_dynamic(self: &Rc<Self>) -> Dynamic {
        let object = ObjectRef::with_host(self.clone());
        object.set(&JsString::from("resourceUri"), self.resource_uri.to_dynamic());
        Dynamic::Object(object)
    }

    /// O estado que um [`Dynamic`] criado por [`SourceControlResourceState::to_dynamic`] representa.
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<SourceControlResourceState>> {
        match value {
            Dynamic::Object(object) => object.host::<SourceControlResourceState>(),
            _ => None,
        }
    }
}

/// `vscode.QuickDiffProvider` (com o `label` da proposta `quickDiffProvider`).
pub trait QuickDiffProvider {
    fn label(&self) -> Option<String> {
        None
    }

    /// `provideOriginalResource?(uri, token)`: `None` é o método ausente.
    fn provide_original_resource(&self, _uri: &URI, _token: &CancellationToken) -> Option<ProviderResult<URI>> {
        None
    }
}

/// `vscode.SourceControlInputBoxValidation`.
#[derive(Clone, Debug)]
pub struct SourceControlInputBoxValidation {
    pub message: types::Documentation,
    pub validation_type: types::SourceControlInputBoxValidationType,
}

/// `IValidateInput`.
pub type IValidateInput = Rc<dyn Fn(&str, f64) -> ProviderResult<SourceControlInputBoxValidation>>;

/// `vscode.SourceControlActionButton.command`: `Command & { shortTitle?: string }`.
#[derive(Clone, Debug)]
pub struct SourceControlActionButtonCommand {
    pub command: Command,
    pub short_title: Option<String>,
}

/// `vscode.SourceControlActionButton`.
#[derive(Clone, Debug)]
pub struct SourceControlActionButton {
    pub command: SourceControlActionButtonCommand,
    pub secondary_commands: Option<Vec<Vec<Command>>>,
    pub enabled: bool,
}

/// `vscode.SourceControlHistoryItemRef`.
#[derive(Clone)]
pub struct SourceControlHistoryItemRef {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub revision: Option<String>,
    pub category: Option<String>,
    pub icon: Option<types::IconPath>,
}

/// `vscode.SourceControlHistoryItemStatistics`.
#[derive(Clone, Debug)]
pub struct SourceControlHistoryItemStatistics {
    pub files: f64,
    pub insertions: f64,
    pub deletions: f64,
}

/// `MarkdownString | Array<MarkdownString>`: o `tooltip` de um item de histórico.
#[derive(Clone)]
pub enum SourceControlHistoryItemTooltip {
    One(types::MarkdownString),
    Many(Vec<types::MarkdownString>),
}

/// `vscode.SourceControlHistoryItem`.
#[derive(Clone)]
pub struct SourceControlHistoryItem {
    pub id: String,
    pub parent_ids: Vec<String>,
    pub subject: String,
    pub message: String,
    pub display_id: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub author_icon: Option<types::IconPath>,
    pub timestamp: Option<f64>,
    pub statistics: Option<SourceControlHistoryItemStatistics>,
    pub references: Option<Vec<SourceControlHistoryItemRef>>,
    pub tooltip: Option<SourceControlHistoryItemTooltip>,
}

/// `vscode.SourceControlHistoryItemChange`.
#[derive(Clone, Debug)]
pub struct SourceControlHistoryItemChange {
    pub uri: URI,
    pub original_uri: Option<URI>,
    pub modified_uri: Option<URI>,
}

/// `vscode.SourceControlHistoryItemRefsChangeEvent`.
#[derive(Clone)]
pub struct SourceControlHistoryItemRefsChangeEvent {
    pub added: Vec<SourceControlHistoryItemRef>,
    pub removed: Vec<SourceControlHistoryItemRef>,
    pub modified: Vec<SourceControlHistoryItemRef>,
    pub silent: bool,
}

/// `vscode.SourceControlHistoryOptions` (a mesma forma de `ISCMHistoryOptions`).
pub type SourceControlHistoryOptions = ISCMHistoryOptions;

/// `vscode.SourceControlHistoryProvider`.
pub trait SourceControlHistoryProvider {
    fn current_history_item_ref(&self) -> Option<SourceControlHistoryItemRef>;
    fn current_history_item_remote_ref(&self) -> Option<SourceControlHistoryItemRef>;
    fn current_history_item_base_ref(&self) -> Option<SourceControlHistoryItemRef>;

    /// `onDidChangeCurrentHistoryItemRefs`.
    fn on_did_change_current_history_item_refs(&self) -> Event<()>;

    /// `onDidChangeHistoryItemRefs`.
    fn on_did_change_history_item_refs(&self) -> Event<SourceControlHistoryItemRefsChangeEvent>;

    fn provide_history_item_refs(&self, history_item_refs: Option<&[String]>, token: &CancellationToken) -> ProviderResult<Vec<SourceControlHistoryItemRef>>;
    fn provide_history_items(&self, options: &SourceControlHistoryOptions, token: &CancellationToken) -> ProviderResult<Vec<SourceControlHistoryItem>>;
    fn provide_history_item_changes(
        &self,
        history_item_id: &str,
        history_item_parent_id: Option<&str>,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<SourceControlHistoryItemChange>>;

    fn resolve_history_item(&self, history_item_id: &str, token: &CancellationToken) -> ProviderResult<SourceControlHistoryItem>;
    fn resolve_history_item_chat_context(&self, history_item_id: &str, token: &CancellationToken) -> ProviderResult<String>;

    /// `resolveHistoryItemChangeRangeChatContext?.(...)`: o padrão é o método ausente (`undefined`).
    fn resolve_history_item_change_range_chat_context(
        &self,
        _history_item_id: &str,
        _history_item_parent_id: &str,
        _path: &str,
        _token: &CancellationToken,
    ) -> ProviderResult<String> {
        MaybePromise::Value(None)
    }

    fn resolve_history_item_refs_common_ancestor(&self, history_item_refs: &[String], token: &CancellationToken) -> ProviderResult<String>;
}

/// `vscode.SourceControlArtifactGroup`.
#[derive(Clone)]
pub struct SourceControlArtifactGroup {
    pub id: String,
    pub name: String,
    pub icon: Option<types::IconPath>,
    pub supports_folders: Option<bool>,
}

/// `vscode.SourceControlArtifact`.
#[derive(Clone)]
pub struct SourceControlArtifact {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<types::IconPath>,
    pub timestamp: Option<f64>,
    pub command: Option<Command>,
}

/// `vscode.SourceControlArtifactProvider`.
pub trait SourceControlArtifactProvider {
    /// `onDidChangeArtifacts`: os grupos que mudaram.
    fn on_did_change_artifacts(&self) -> Event<Vec<String>>;
    fn provide_artifact_groups(&self, token: &CancellationToken) -> ProviderResult<Vec<SourceControlArtifactGroup>>;
    fn provide_artifacts(&self, group: &str, token: &CancellationToken) -> ProviderResult<Vec<SourceControlArtifact>>;
}

/// As opções de `createResourceGroup(id, label, options?)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CreateResourceGroupOptions {
    pub multi_diff_editor_enable_view_changes: Option<bool>,
}

/// `SourceControlInputBoxValidationType.from` (`extHostTypeConverters.ts`).
pub fn source_control_input_box_validation_type_from(validation_type: types::SourceControlInputBoxValidationType) -> InputValidationType {
    match validation_type {
        types::SourceControlInputBoxValidationType::Error => InputValidationType::Error,
        types::SourceControlInputBoxValidationType::Warning => InputValidationType::Warning,
        types::SourceControlInputBoxValidationType::Information => InputValidationType::Information,
    }
}

// ---- as funções auxiliares -----------------------------------------------------------------------------

thread_local! {
    static PLATFORM: Platform = Platform::detect();
    static SOURCE_CONTROL_HANDLE_POOL: Cell<u32> = const { Cell::new(0) };
    static GROUP_HANDLE_POOL: Cell<u32> = const { Cell::new(0) };
}

/// `Class._handlePool++`.
fn next_handle(pool: &'static std::thread::LocalKey<Cell<u32>>) -> u32 {
    pool.with(|pool| {
        let handle = pool.get();
        pool.set(handle + 1);
        handle
    })
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `comparePaths(one, other, caseSensitive)` sobre textos.
fn compare_paths_of(one: &str, other: &str, case_sensitive: bool) -> f64 {
    PLATFORM.with(|platform| f64::from(compare_paths(&units(one), &units(other), case_sensitive, platform)))
}

/// `a < b` do JavaScript sobre textos (unidades UTF-16).
fn js_text_less(a: &str, b: &str) -> bool {
    a.encode_utf16().lt(b.encode_utf16())
}

/// O `ToPrimitive` que `<` aplica a um valor.
enum Primitive {
    Number(f64),
    Text(Vec<u16>),
}

fn to_primitive(value: &Dynamic) -> Primitive {
    match value {
        Dynamic::Number(number) => Primitive::Number(*number),
        Dynamic::Bool(flag) => Primitive::Number(f64::from(u8::from(*flag))),
        Dynamic::Null => Primitive::Number(0.0),
        Dynamic::Undefined => Primitive::Number(f64::NAN),
        Dynamic::String(text) => Primitive::Text(text.units().to_vec()),
        _ => Primitive::Text(units("[object Object]")),
    }
}

fn primitive_number(primitive: &Primitive) -> f64 {
    match primitive {
        Primitive::Number(number) => *number,
        Primitive::Text(text) => {
            let text = String::from_utf16_lossy(text);
            let text = text.trim();
            if text.is_empty() { 0.0 } else { text.parse::<f64>().unwrap_or(f64::NAN) }
        }
    }
}

/// `a < b` do JavaScript entre dois valores quaisquer.
fn js_less_than(a: &Dynamic, b: &Dynamic) -> bool {
    match (to_primitive(a), to_primitive(b)) {
        (Primitive::Text(a), Primitive::Text(b)) => a < b,
        (a, b) => primitive_number(&a) < primitive_number(&b),
    }
}

/// `isUri(thing)` seguido do `URI` que ele é.
fn as_uri(value: &Dynamic) -> Option<URI> {
    if is_instance_of(value, "Uri") { URI::from_dynamic(value).ok() } else { None }
}

fn uri_equals(a: &URI, b: &URI) -> bool {
    if a.scheme().to_string_lossy() == schemas::FILE && b.scheme().to_string_lossy() == schemas::FILE && cfg!(target_os = "linux") {
        return a.to_string(false) == b.to_string(false);
    }
    a.to_string(false).to_string_lossy().to_lowercase() == b.to_string(false).to_string_lossy().to_lowercase()
}

/// `getIconResource(decorations)`.
fn get_icon_resource(icon: Option<&DecorationIconPath>) -> Option<SCMIconDto> {
    match icon? {
        DecorationIconPath::Path(path) => Some(SCMIconDto::Uri(uri_file(path))),
        DecorationIconPath::Uri(uri) => Some(SCMIconDto::Uri(uri.clone())),
        DecorationIconPath::ThemeIcon(icon) => Some(SCMIconDto::ThemeIcon(icon.clone())),
    }
}

/// `getHistoryItemIconDto(icon)`.
fn get_history_item_icon_dto(icon: Option<&types::IconPath>) -> Option<IconPathDto> {
    Some(match icon? {
        types::IconPath::Uri(uri) => IconPathDto::Uri(uri.clone()),
        types::IconPath::ThemeIcon(icon) => IconPathDto::ThemeIcon(icon.clone()),
        types::IconPath::LightDark { light, dark } => IconPathDto::Themed { light: light.clone(), dark: dark.clone() },
    })
}

fn to_scm_history_item_ref_dto_with_icon(history_item_ref: &SourceControlHistoryItemRef) -> SCMHistoryItemRefDto {
    SCMHistoryItemRefDto {
        id: history_item_ref.id.clone(),
        name: history_item_ref.name.clone(),
        revision: history_item_ref.revision.clone(),
        category: history_item_ref.category.clone(),
        description: history_item_ref.description.clone(),
        icon: get_history_item_icon_dto(history_item_ref.icon.as_ref()),
    }
}

/// `toSCMHistoryItemDto(historyItem)`.
fn to_scm_history_item_dto(history_item: &SourceControlHistoryItem) -> SCMHistoryItemDto {
    let tooltip = match &history_item.tooltip {
        Some(SourceControlHistoryItemTooltip::Many(items)) => Some(SCMHistoryItemTooltipDto::Many(items.iter().map(type_converters::MarkdownString::from_markdown).collect())),
        Some(SourceControlHistoryItemTooltip::One(item)) => Some(SCMHistoryItemTooltipDto::One(type_converters::MarkdownString::from_markdown(item))),
        None => None,
    };
    SCMHistoryItemDto {
        id: history_item.id.clone(),
        parent_ids: history_item.parent_ids.clone(),
        subject: history_item.subject.clone(),
        message: history_item.message.clone(),
        display_id: history_item.display_id.clone(),
        author: history_item.author.clone(),
        author_icon: get_history_item_icon_dto(history_item.author_icon.as_ref()),
        author_email: history_item.author_email.clone(),
        timestamp: history_item.timestamp,
        statistics: history_item.statistics.as_ref().map(|statistics| SCMHistoryItemStatisticsDto {
            files: statistics.files,
            insertions: statistics.insertions,
            deletions: statistics.deletions,
        }),
        references: history_item.references.as_ref().map(|references| references.iter().map(to_scm_history_item_ref_dto_with_icon).collect()),
        tooltip,
    }
}

/// `toSCMHistoryItemRefDto(historyItemRef)`.
fn to_scm_history_item_ref_dto(history_item_ref: Option<&SourceControlHistoryItemRef>) -> Option<SCMHistoryItemRefDto> {
    history_item_ref.map(to_scm_history_item_ref_dto_with_icon)
}

/// `!iconPath`: um texto vazio também é falso.
fn icon_path_is_truthy(icon_path: &Option<DecorationIconPath>) -> bool {
    match icon_path {
        None => false,
        Some(DecorationIconPath::Path(path)) => !path.is_empty(),
        Some(_) => true,
    }
}

/// `compareResourceThemableDecorations(a, b)`, sobre os `iconPath` dos dois.
fn compare_resource_themable_decorations(a: &Option<DecorationIconPath>, b: &Option<DecorationIconPath>) -> f64 {
    let (a_truthy, b_truthy) = (icon_path_is_truthy(a), icon_path_is_truthy(b));
    if !a_truthy && !b_truthy {
        return 0.0;
    } else if !a_truthy {
        return -1.0;
    } else if !b_truthy {
        return 1.0;
    }

    let path_of = |icon_path: &Option<DecorationIconPath>| match icon_path {
        Some(DecorationIconPath::Path(path)) => path.clone(),
        Some(DecorationIconPath::Uri(uri)) => fs_path(uri),
        Some(DecorationIconPath::ThemeIcon(icon)) => icon.id.clone(),
        None => String::new(),
    };
    compare_paths_of(&path_of(a), &path_of(b), false)
}

fn compare_resource_states_decorations(a: &SourceControlResourceDecorations, b: &SourceControlResourceDecorations) -> f64 {
    if a.strike_through != b.strike_through {
        return if a.strike_through == Some(true) { 1.0 } else { -1.0 };
    }

    if a.faded != b.faded {
        return if a.faded == Some(true) { 1.0 } else { -1.0 };
    }

    if a.tooltip != b.tooltip {
        return locale_compare(a.tooltip.as_deref().unwrap_or(""), b.tooltip.as_deref().unwrap_or("")) as i32 as f64;
    }

    let mut result = compare_resource_themable_decorations(&a.icon_path, &b.icon_path);

    if result != 0.0 {
        return result;
    }

    match (&a.light, &b.light) {
        (Some(a_light), Some(b_light)) => result = compare_resource_themable_decorations(&a_light.icon_path, &b_light.icon_path),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    if result != 0.0 {
        return result;
    }

    match (&a.dark, &b.dark) {
        (Some(a_dark), Some(b_dark)) => result = compare_resource_themable_decorations(&a_dark.icon_path, &b_dark.icon_path),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    result
}

fn compare_commands(a: &Command, b: &Command) -> f64 {
    if a.command != b.command {
        return if js_text_less(&a.command, &b.command) { -1.0 } else { 1.0 };
    }

    if a.title != b.title {
        return if js_text_less(&a.title, &b.title) { -1.0 } else { 1.0 };
    }

    if a.tooltip != b.tooltip {
        match (&a.tooltip, &b.tooltip) {
            (Some(a_tooltip), Some(b_tooltip)) => return if js_text_less(a_tooltip, b_tooltip) { -1.0 } else { 1.0 },
            (Some(_), None) => return 1.0,
            (None, Some(_)) => return -1.0,
            (None, None) => {}
        }
    }

    let (a_arguments, b_arguments) = match (&a.arguments, &b.arguments) {
        (None, None) => return 0.0,
        (None, Some(_)) => return -1.0,
        (Some(_), None) => return 1.0,
        (Some(a_arguments), Some(b_arguments)) => (a_arguments, b_arguments),
    };
    if a_arguments.len() != b_arguments.len() {
        return a_arguments.len() as f64 - b_arguments.len() as f64;
    }

    for (a_arg, b_arg) in a_arguments.iter().zip(b_arguments) {
        if a_arg.strict_equals(b_arg) {
            continue;
        }

        if let (Some(a_uri), Some(b_uri)) = (as_uri(a_arg), as_uri(b_arg))
            && uri_equals(&a_uri, &b_uri)
        {
            continue;
        }

        return if js_less_than(a_arg, b_arg) { -1.0 } else { 1.0 };
    }

    0.0
}

/// `comparePaths(a.fsPath, b.fsPath, true)` de duas URIs presentes.
fn compare_uri_paths(a: &URI, b: &URI) -> f64 {
    compare_paths_of(&fs_path(a), &fs_path(b), true)
}

fn compare_resource_states(a: &SourceControlResourceState, b: &SourceControlResourceState) -> f64 {
    let mut result = compare_uri_paths(&a.resource_uri, &b.resource_uri);

    if result != 0.0 {
        return result;
    }

    match (&a.command, &b.command) {
        (Some(a_command), Some(b_command)) => result = compare_commands(a_command, b_command),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    if result != 0.0 {
        return result;
    }

    match (&a.decorations, &b.decorations) {
        (Some(a_decorations), Some(b_decorations)) => result = compare_resource_states_decorations(a_decorations, b_decorations),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    if result != 0.0 {
        return result;
    }

    match (&a.multi_file_diff_editor_modified_uri, &b.multi_file_diff_editor_modified_uri) {
        (Some(a_uri), Some(b_uri)) => result = compare_uri_paths(a_uri, b_uri),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    if result != 0.0 {
        return result;
    }

    match (&a.multi_diff_editor_original_uri, &b.multi_diff_editor_original_uri) {
        (Some(a_uri), Some(b_uri)) => result = compare_uri_paths(a_uri, b_uri),
        (Some(_), None) => return 1.0,
        (None, Some(_)) => return -1.0,
        (None, None) => {}
    }

    result
}

/// `compareArgs(a, b)`: itera pelos argumentos de `a`.
fn compare_args(a: &[Dynamic], b: &[Dynamic]) -> bool {
    a.iter().enumerate().all(|(index, a_arg)| a_arg.strict_equals(b.get(index).unwrap_or(&Dynamic::Undefined)))
}

fn command_equals(a: &Command, b: &Command) -> bool {
    a.command == b.command
        && a.title == b.title
        && a.tooltip == b.tooltip
        && match (&a.arguments, &b.arguments) {
            (Some(a_arguments), Some(b_arguments)) => compare_args(a_arguments, b_arguments),
            (None, None) => true,
            _ => false,
        }
}

fn command_list_equals(a: &[Command], b: &[Command]) -> bool {
    arrays_equals(Some(a), Some(b), command_equals)
}

/// O `Command` (e o `shortTitle`) como o objeto que `structuralEquals` compara.
fn command_to_dynamic(command: &Command, short_title: Option<&str>) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = vec![("title", Dynamic::string(&command.title)), ("command", Dynamic::string(&command.command))];
    if let Some(tooltip) = &command.tooltip {
        entries.push(("tooltip", Dynamic::string(tooltip)));
    }
    if let Some(arguments) = &command.arguments {
        entries.push(("arguments", Dynamic::array(arguments.iter().cloned())));
    }
    if let Some(short_title) = short_title {
        entries.push(("shortTitle", Dynamic::string(short_title)));
    }
    Dynamic::object(entries)
}

/// O botão de ação como o objeto que `structuralEquals` compara (`undefined` sem botão).
fn action_button_to_dynamic(action_button: Option<&SourceControlActionButton>) -> Dynamic {
    let Some(action_button) = action_button else {
        return Dynamic::Undefined;
    };
    let mut entries: Vec<(&str, Dynamic)> =
        vec![("command", command_to_dynamic(&action_button.command.command, action_button.command.short_title.as_deref()))];
    if let Some(secondary_commands) = &action_button.secondary_commands {
        entries.push((
            "secondaryCommands",
            Dynamic::array(secondary_commands.iter().map(|group| Dynamic::array(group.iter().map(|command| command_to_dynamic(command, None))))),
        ));
    }
    entries.push(("enabled", Dynamic::Bool(action_button.enabled)));
    Dynamic::object(entries)
}

/// `this._commands.converter.toInternal(command, disposables)` de um comando que existe.
fn to_internal_required(converter: &CommandsConverter, command: &Command, disposables: &DisposableStore) -> Result<ICommandDto, Error> {
    converter.to_internal(Some(command), disposables)?.ok_or_else(|| Error::new("toInternal não devolveu o comando"))
}

/// `asPromise(callback)` para um retorno `T | Thenable<T>` já calculado.
fn as_promise<T: Clone + 'static>(value: MaybePromise<T>) -> Promise<T> {
    match value {
        MaybePromise::Value(value) => Promise::resolved(value),
        MaybePromise::Promise(promise) => promise,
    }
}

// ---- ExtHostSCMInputBox ---------------------------------------------------------------------------------

/// `ExtHostSCMInputBox`.
pub struct ExtHostSCMInputBox {
    proxy: Rc<dyn MainThreadSCMShape>,
    ext_host_documents: Rc<ExtHostDocuments>,
    extension: ExtensionDescription,
    source_control_handle: u32,
    document_uri: URI,
    value: RefCell<String>,
    on_did_change: Rc<Emitter<String>>,
    placeholder: RefCell<String>,
    validate_input: RefCell<Option<IValidateInput>>,
    enabled: Cell<bool>,
    visible: Cell<bool>,
}

impl ExtHostSCMInputBox {
    fn new(
        extension: ExtensionDescription,
        ext_host_documents: Rc<ExtHostDocuments>,
        proxy: Rc<dyn MainThreadSCMShape>,
        source_control_handle: u32,
        document_uri: URI,
    ) -> Rc<ExtHostSCMInputBox> {
        Rc::new(ExtHostSCMInputBox {
            proxy,
            ext_host_documents,
            extension,
            source_control_handle,
            document_uri,
            value: RefCell::new(String::new()),
            on_did_change: Emitter::new(),
            placeholder: RefCell::new(String::new()),
            validate_input: RefCell::new(None),
            enabled: Cell::new(true),
            visible: Cell::new(true),
        })
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// `set value(value)`: o `value ?? ''` do original é o tipo (`&str`).
    pub fn set_value(&self, value: &str) {
        self.proxy.rpc_set_input_box_value(self.source_control_handle, value);
        self.update_value(value);
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<String> {
        self.on_did_change.event()
    }

    pub fn placeholder(&self) -> String {
        self.placeholder.borrow().clone()
    }

    pub fn set_placeholder(&self, placeholder: &str) {
        self.proxy.rpc_set_input_box_placeholder(self.source_control_handle, placeholder);
        *self.placeholder.borrow_mut() = placeholder.to_owned();
    }

    /// `get validateInput()`: pede a proposta `scmValidation`.
    pub fn validate_input(&self) -> Result<Option<IValidateInput>, Error> {
        check_proposed_api_enabled(&self.extension, "scmValidation")?;
        Ok(self.validate_input.borrow().clone())
    }

    /// `set validateInput(fn)`: pede a proposta `scmValidation`.
    pub fn set_validate_input(&self, validate_input: Option<IValidateInput>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmValidation")?;
        let enabled = validate_input.is_some();
        *self.validate_input.borrow_mut() = validate_input;
        self.proxy.rpc_set_validation_provider_is_enabled(self.source_control_handle, enabled);
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        if self.enabled.get() == enabled {
            return;
        }
        self.enabled.set(enabled);
        self.proxy.rpc_set_input_box_enablement(self.source_control_handle, enabled);
    }

    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    pub fn set_visible(&self, visible: bool) {
        if self.visible.get() == visible {
            return;
        }
        self.visible.set(visible);
        self.proxy.rpc_set_input_box_visibility(self.source_control_handle, visible);
    }

    /// `get document()`: pede a proposta `scmTextDocument`.
    pub fn document(&self) -> Result<TextDocument, Error> {
        check_proposed_api_enabled(&self.extension, "scmTextDocument")?;
        self.ext_host_documents.get_document(&self.document_uri)
    }

    /// `showValidationMessage(message, type)`: pede a proposta `scmValidation`. A mensagem segue como veio (o original não a
    /// converte).
    pub fn show_validation_message(&self, message: &types::Documentation, validation_type: types::SourceControlInputBoxValidationType) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmValidation")?;
        let message = match message {
            types::Documentation::Text(text) => StringOrMarkdown::String(text.clone()),
            types::Documentation::Markdown(markdown) => StringOrMarkdown::Markdown(IMarkdownString {
                value: markdown.value.clone(),
                is_trusted: markdown.is_trusted.clone(),
                support_theme_icons: markdown.support_theme_icons,
                support_html: markdown.support_html,
                support_alert_syntax: markdown.support_alert_syntax,
                base_uri: markdown.base_uri.clone(),
                uris: None,
            }),
        };
        self.proxy.rpc_show_validation_message(self.source_control_handle, message, source_control_input_box_validation_type_from(validation_type));
        Ok(())
    }

    /// `$onInputBoxValueChange(value)`.
    pub fn on_input_box_value_change(&self, value: &str) {
        self.update_value(value);
    }

    fn update_value(&self, value: &str) {
        *self.value.borrow_mut() = value.to_owned();
        self.on_did_change.fire(value.to_owned());
    }
}

// ---- ExtHostSourceControlResourceGroup -----------------------------------------------------------------

/// `ExtHostSourceControlResourceGroup`.
pub struct ExtHostSourceControlResourceGroup {
    this: Weak<ExtHostSourceControlResourceGroup>,
    value: OnceCell<Dynamic>,
    resource_handle_pool: Cell<u32>,
    resource_states: RefCell<Vec<Rc<SourceControlResourceState>>>,
    resource_states_map: RefCell<HashMap<ResourceStateHandle, Rc<SourceControlResourceState>>>,
    resource_states_commands_map: RefCell<HashMap<ResourceStateHandle, Command>>,
    resource_states_disposables_map: RefCell<HashMap<ResourceStateHandle, Rc<DisposableStore>>>,
    on_did_update_resource_states: Rc<Emitter<()>>,
    disposed: Cell<bool>,
    on_did_dispose: Rc<Emitter<()>>,
    handles_snapshot: RefCell<Vec<u32>>,
    resource_snapshot: RefCell<Vec<Rc<SourceControlResourceState>>>,
    id: String,
    label: RefCell<String>,
    context_value: RefCell<Option<String>>,
    hide_when_empty: Cell<Option<bool>>,
    handle: GroupHandle,
    proxy: Rc<dyn MainThreadSCMShape>,
    commands: Weak<ExtHostCommands>,
    converter: Rc<CommandsConverter>,
    source_control_handle: u32,
    multi_diff_editor_enable_view_changes: bool,
    extension: ExtensionDescription,
}

impl ExtHostSourceControlResourceGroup {
    #[allow(clippy::too_many_arguments)]
    fn new(
        proxy: Rc<dyn MainThreadSCMShape>,
        commands: &Rc<ExtHostCommands>,
        source_control_handle: u32,
        id: &str,
        label: &str,
        multi_diff_editor_enable_view_changes: bool,
        extension: ExtensionDescription,
    ) -> Rc<ExtHostSourceControlResourceGroup> {
        Rc::new_cyclic(|this| ExtHostSourceControlResourceGroup {
            this: this.clone(),
            value: OnceCell::new(),
            resource_handle_pool: Cell::new(0),
            resource_states: RefCell::new(Vec::new()),
            resource_states_map: RefCell::new(HashMap::new()),
            resource_states_commands_map: RefCell::new(HashMap::new()),
            resource_states_disposables_map: RefCell::new(HashMap::new()),
            on_did_update_resource_states: Emitter::new(),
            disposed: Cell::new(false),
            on_did_dispose: Emitter::new(),
            handles_snapshot: RefCell::new(Vec::new()),
            resource_snapshot: RefCell::new(Vec::new()),
            id: id.to_owned(),
            label: RefCell::new(label.to_owned()),
            context_value: RefCell::new(None),
            hide_when_empty: Cell::new(None),
            handle: next_handle(&GROUP_HANDLE_POOL),
            proxy,
            commands: Rc::downgrade(commands),
            converter: commands.converter(),
            source_control_handle,
            multi_diff_editor_enable_view_changes,
            extension,
        })
    }

    /// O objeto do grupo como argumento de comando (o mesmo em toda leitura).
    pub fn to_dynamic(&self) -> Dynamic {
        weak_host_dynamic(&self.value, &self.this)
    }

    /// O grupo de um argumento criado por [`ExtHostSourceControlResourceGroup::to_dynamic`], se ainda estiver vivo.
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<ExtHostSourceControlResourceGroup>> {
        host_object::<ExtHostSourceControlResourceGroup>(value)
    }

    pub fn handle(&self) -> GroupHandle {
        self.handle
    }

    pub fn multi_diff_editor_enable_view_changes(&self) -> bool {
        self.multi_diff_editor_enable_view_changes
    }

    pub fn disposed(&self) -> bool {
        self.disposed.get()
    }

    /// `onDidUpdateResourceStates`.
    pub fn on_did_update_resource_states(&self) -> Event<()> {
        self.on_did_update_resource_states.event()
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> String {
        self.label.borrow().clone()
    }

    pub fn set_label(&self, label: &str) {
        *self.label.borrow_mut() = label.to_owned();
        self.proxy.rpc_update_group_label(self.source_control_handle, self.handle, label);
    }

    pub fn context_value(&self) -> Option<String> {
        self.context_value.borrow().clone()
    }

    pub fn set_context_value(&self, context_value: Option<String>) {
        *self.context_value.borrow_mut() = context_value;
        self.proxy.rpc_update_group(self.source_control_handle, self.handle, self.features());
    }

    pub fn hide_when_empty(&self) -> Option<bool> {
        self.hide_when_empty.get()
    }

    pub fn set_hide_when_empty(&self, hide_when_empty: Option<bool>) {
        self.hide_when_empty.set(hide_when_empty);
        self.proxy.rpc_update_group(self.source_control_handle, self.handle, self.features());
    }

    pub fn features(&self) -> SCMGroupFeatures {
        SCMGroupFeatures { context_value: self.context_value(), hide_when_empty: self.hide_when_empty() }
    }

    pub fn resource_states(&self) -> Vec<Rc<SourceControlResourceState>> {
        self.resource_states.borrow().clone()
    }

    pub fn set_resource_states(&self, resources: Vec<Rc<SourceControlResourceState>>) {
        *self.resource_states.borrow_mut() = resources;
        self.on_did_update_resource_states.fire(());
    }

    pub fn get_resource_state(&self, handle: ResourceStateHandle) -> Option<Rc<SourceControlResourceState>> {
        self.resource_states_map.borrow().get(&handle).cloned()
    }

    /// `$executeResourceCommand(handle, preserveFocus)`.
    pub fn execute_resource_command(&self, handle: ResourceStateHandle, preserve_focus: bool) -> Promise<()> {
        let command = self.resource_states_commands_map.borrow().get(&handle).cloned();
        let Some(command) = command else {
            return Promise::resolved(());
        };
        let Some(commands) = self.commands.upgrade() else {
            return Promise::rejected(Error::new("ExtHostCommands is disposed"));
        };
        let mut arguments = command.arguments.clone().unwrap_or_default();
        arguments.push(Dynamic::Bool(preserve_focus));
        commands.execute_command(&command.command, arguments).then(|_| ())
    }

    /// `_takeResourceStateSnapshot()`.
    fn take_resource_state_snapshot(&self) -> Result<Vec<SCMRawResourceSplice>, Error> {
        let mut snapshot = self.resource_states.borrow().clone();
        sort_by_compare(&mut snapshot, |a, b| compare_resource_states(a, b));
        let previous = self.resource_snapshot.borrow().clone();
        let diffs = sorted_diff(&previous, &snapshot, |a, b| compare_resource_states(a, b));

        let multi_diff_editor_enabled = is_proposed_api_enabled(&self.extension, "scmMultiDiffEditor");
        let mut splices: Vec<(usize, usize, Vec<(SCMRawResource, u32)>)> = Vec::new();
        for diff in diffs {
            let mut to_insert = Vec::new();
            for r in &diff.to_insert {
                let handle = self.resource_handle_pool.get();
                self.resource_handle_pool.set(handle + 1);
                self.resource_states_map.borrow_mut().insert(handle, r.clone());

                let source_uri = r.resource_uri.clone();

                let mut command: Option<ICommandDto> = None;
                if let Some(resource_command) = &r.command {
                    if matches!(resource_command.command.as_str(), "vscode.open" | "vscode.diff" | "vscode.changes") {
                        let disposables = DisposableStore::new();
                        command = self.converter.to_internal(Some(resource_command), &disposables)?;
                        self.resource_states_disposables_map.borrow_mut().insert(handle, disposables);
                    } else {
                        self.resource_states_commands_map.borrow_mut().insert(handle, resource_command.clone());
                    }
                }

                let multi_file_diff_editor_original_uri = if multi_diff_editor_enabled { r.multi_diff_editor_original_uri.clone() } else { None };
                let multi_file_diff_editor_modified_uri = if multi_diff_editor_enabled { r.multi_file_diff_editor_modified_uri.clone() } else { None };

                let decorations = r.decorations.as_ref();
                let icon = decorations.and_then(|decorations| get_icon_resource(decorations.icon_path.as_ref()));
                let light_icon = decorations
                    .and_then(|decorations| decorations.light.as_ref())
                    .and_then(|light| get_icon_resource(light.icon_path.as_ref()))
                    .or_else(|| icon.clone());
                let dark_icon = decorations
                    .and_then(|decorations| decorations.dark.as_ref())
                    .and_then(|dark| get_icon_resource(dark.icon_path.as_ref()))
                    .or_else(|| icon.clone());

                let raw_resource = SCMRawResource {
                    handle,
                    resource_uri: source_uri,
                    icons: (light_icon, dark_icon),
                    tooltip: decorations.and_then(|decorations| decorations.tooltip.clone()).unwrap_or_default(),
                    strike_through: decorations.is_some_and(|decorations| decorations.strike_through == Some(true)),
                    faded: decorations.is_some_and(|decorations| decorations.faded == Some(true)),
                    context_value: r.context_value.clone().unwrap_or_default(),
                    command,
                    multi_file_diff_editor_original_uri,
                    multi_file_diff_editor_modified_uri,
                };
                to_insert.push((raw_resource, handle));
            }
            splices.push((diff.start, diff.delete_count, to_insert));
        }

        let raw_resource_splices: Vec<SCMRawResourceSplice> = splices
            .iter()
            .map(|(start, delete_count, to_insert)| SCMRawResourceSplice {
                start: *start,
                delete_count: *delete_count,
                to_insert: to_insert.iter().map(|(raw_resource, _)| raw_resource.clone()).collect(),
            })
            .collect();

        for (start, delete_count, to_insert) in splices.iter().rev() {
            let handles: Vec<u32> = to_insert.iter().map(|(_, handle)| *handle).collect();
            let handles_to_delete: Vec<u32> = {
                let mut handles_snapshot = self.handles_snapshot.borrow_mut();
                let start = (*start).min(handles_snapshot.len());
                let end = (start + delete_count).min(handles_snapshot.len());
                handles_snapshot.splice(start..end, handles).collect()
            };

            for handle in handles_to_delete {
                self.resource_states_map.borrow_mut().remove(&handle);
                self.resource_states_commands_map.borrow_mut().remove(&handle);
                let disposables = self.resource_states_disposables_map.borrow_mut().remove(&handle);
                if let Some(disposables) = disposables {
                    disposables.dispose();
                }
            }
        }

        *self.resource_snapshot.borrow_mut() = snapshot;
        Ok(raw_resource_splices)
    }
}

impl IDisposable for ExtHostSourceControlResourceGroup {
    fn dispose(&self) {
        self.disposed.set(true);
        self.on_did_dispose.fire(());
        self.on_did_update_resource_states.dispose();
        self.on_did_dispose.dispose();
    }
}

// ---- ExtHostSourceControl -------------------------------------------------------------------------------

/// `ExtHostSourceControl`.
pub struct ExtHostSourceControl {
    this: Weak<ExtHostSourceControl>,
    value: OnceCell<Dynamic>,
    extension: ExtensionDescription,
    proxy: Rc<dyn MainThreadSCMShape>,
    converter: Rc<CommandsConverter>,
    commands: Weak<ExtHostCommands>,
    id: String,
    label: String,
    root_uri: Option<URI>,
    handle: ProviderHandle,
    /// `onDidDisposeParent`.
    pub on_did_dispose_parent: Event<()>,
    on_did_dispose: Rc<Emitter<()>>,
    groups: RefCell<BTreeMap<GroupHandle, Rc<ExtHostSourceControlResourceGroup>>>,
    active_repository_name: RefCell<Option<String>>,
    context_value: RefCell<Option<String>>,
    input_box: Rc<ExtHostSCMInputBox>,
    count: Cell<Option<f64>>,
    quick_diff_provider: RefCell<Option<Rc<dyn QuickDiffProvider>>>,
    secondary_quick_diff_provider: RefCell<Option<Rc<dyn QuickDiffProvider>>>,
    history_provider: RefCell<Option<Rc<dyn SourceControlHistoryProvider>>>,
    history_provider_disposable: Rc<MutableDisposable<DisposableStore>>,
    artifact_provider: RefCell<Option<Rc<dyn SourceControlArtifactProvider>>>,
    artifact_provider_disposable: Rc<MutableDisposable<DisposableStore>>,
    commit_template: RefCell<Option<String>>,
    accept_input_disposables: Rc<MutableDisposable<DisposableStore>>,
    accept_input_command: RefCell<Option<Command>>,
    action_button_disposables: RefCell<Rc<DisposableStore>>,
    action_button: RefCell<Option<SourceControlActionButton>>,
    status_bar_disposables: RefCell<Rc<DisposableStore>>,
    status_bar_commands: RefCell<Option<Vec<Command>>>,
    selected: Cell<bool>,
    on_did_change_selection: Rc<Emitter<bool>>,
    artifact_commands_disposables: Rc<DisposableMap<String, DisposableStore>>,
    created_resource_groups: RefCell<Vec<(Rc<ExtHostSourceControlResourceGroup>, DisposableRef)>>,
    updated_resource_groups: RefCell<Vec<Rc<ExtHostSourceControlResourceGroup>>>,
    add_resource_groups_debounce: Debounce<(), ()>,
    update_resource_states_debounce: Debounce<(), ()>,
}

impl ExtHostSourceControl {
    #[allow(clippy::too_many_arguments)]
    fn new(
        extension: ExtensionDescription,
        ext_host_documents: Rc<ExtHostDocuments>,
        proxy: Rc<dyn MainThreadSCMShape>,
        commands: &Rc<ExtHostCommands>,
        id: &str,
        label: &str,
        root_uri: Option<URI>,
        icon_path: Option<types::IconPath>,
        is_hidden: Option<bool>,
        parent: Option<&Rc<ExtHostSourceControl>>,
    ) -> Rc<ExtHostSourceControl> {
        let handle = next_handle(&SOURCE_CONTROL_HANDLE_POOL);

        let input_box_document_uri = URI::from(
            &UriComponents {
                scheme: Some(JsString::from(schemas::VSCODE_SOURCE_CONTROL)),
                path: Some(JsString::from(format!("{id}/scm{handle}/input"))),
                query: root_uri.as_ref().map(|root_uri| {
                    let encoded = encode_uri_component(root_uri.to_string(false).units());
                    JsString::from(format!("rootUri={}", String::from_utf16_lossy(&encoded)))
                }),
                ..UriComponents::default()
            },
            false,
        );

        let input_box = ExtHostSCMInputBox::new(extension.clone(), ext_host_documents, proxy.clone(), handle, input_box_document_uri.clone());

        let source_control = Rc::new_cyclic(|this: &Weak<ExtHostSourceControl>| {
            let adding = this.clone();
            let updating = this.clone();
            ExtHostSourceControl {
                this: this.clone(),
                value: OnceCell::new(),
                extension,
                proxy: proxy.clone(),
                converter: commands.converter(),
                commands: Rc::downgrade(commands),
                id: id.to_owned(),
                label: label.to_owned(),
                root_uri: root_uri.clone(),
                handle,
                on_did_dispose_parent: parent.map_or_else(Event::none, |parent| parent.on_did_dispose()),
                on_did_dispose: Emitter::new(),
                groups: RefCell::new(BTreeMap::new()),
                active_repository_name: RefCell::new(None),
                context_value: RefCell::new(None),
                input_box,
                count: Cell::new(None),
                quick_diff_provider: RefCell::new(None),
                secondary_quick_diff_provider: RefCell::new(None),
                history_provider: RefCell::new(None),
                history_provider_disposable: MutableDisposable::new(),
                artifact_provider: RefCell::new(None),
                artifact_provider_disposable: MutableDisposable::new(),
                commit_template: RefCell::new(None),
                accept_input_disposables: MutableDisposable::new(),
                accept_input_command: RefCell::new(None),
                action_button_disposables: RefCell::new(DisposableStore::new()),
                action_button: RefCell::new(None),
                status_bar_disposables: RefCell::new(DisposableStore::new()),
                status_bar_commands: RefCell::new(None),
                selected: Cell::new(false),
                on_did_change_selection: Emitter::new(),
                artifact_commands_disposables: DisposableMap::new(),
                created_resource_groups: RefCell::new(Vec::new()),
                updated_resource_groups: RefCell::new(Vec::new()),
                add_resource_groups_debounce: Debounce::without_reducer(100, move |()| {
                    if let Some(source_control) = adding.upgrade() {
                        source_control.add_resource_groups();
                    }
                }),
                update_resource_states_debounce: Debounce::without_reducer(100, move |()| {
                    if let Some(source_control) = updating.upgrade() {
                        source_control.update_resource_states();
                    }
                }),
            }
        });

        source_control.proxy.rpc_register_source_control(
            handle,
            parent.map(|parent| parent.handle),
            id,
            label,
            root_uri,
            get_history_item_icon_dto(icon_path.as_ref()),
            is_hidden,
            input_box_document_uri,
        );
        source_control
    }

    /// O objeto da fonte como argumento de comando (o mesmo em toda leitura).
    pub fn to_dynamic(&self) -> Dynamic {
        weak_host_dynamic(&self.value, &self.this)
    }

    /// A fonte de um argumento criado por [`ExtHostSourceControl::to_dynamic`], se ainda estiver viva.
    pub fn from_dynamic(value: &Dynamic) -> Option<Rc<ExtHostSourceControl>> {
        host_object::<ExtHostSourceControl>(value)
    }

    pub fn handle(&self) -> ProviderHandle {
        self.handle
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn root_uri(&self) -> Option<&URI> {
        self.root_uri.as_ref()
    }

    /// `onDidDispose`.
    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    /// `onDidChangeSelection`.
    pub fn on_did_change_selection(&self) -> Event<bool> {
        self.on_did_change_selection.event()
    }

    pub fn selected(&self) -> bool {
        self.selected.get()
    }

    /// `inputBox`.
    pub fn input_box(&self) -> Rc<ExtHostSCMInputBox> {
        self.input_box.clone()
    }

    /// `get activeRepositoryName()`: pede a proposta `scmProviderOptions`.
    pub fn active_repository_name(&self) -> Result<Option<String>, Error> {
        check_proposed_api_enabled(&self.extension, "scmProviderOptions")?;
        Ok(self.active_repository_name.borrow().clone())
    }

    /// `set activeRepositoryName(value)`: pede a proposta `scmProviderOptions`.
    pub fn set_active_repository_name(&self, active_repository_name: Option<String>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmProviderOptions")?;

        if *self.active_repository_name.borrow() == active_repository_name {
            return Ok(());
        }

        *self.active_repository_name.borrow_mut() = active_repository_name.clone();
        self.update_source_control(SCMProviderFeatures { active_repository_name: Some(active_repository_name), ..SCMProviderFeatures::default() });
        Ok(())
    }

    /// `get contextValue()`: pede a proposta `scmProviderOptions`.
    pub fn context_value(&self) -> Result<Option<String>, Error> {
        check_proposed_api_enabled(&self.extension, "scmProviderOptions")?;
        Ok(self.context_value.borrow().clone())
    }

    /// `set contextValue(value)`: pede a proposta `scmProviderOptions`.
    pub fn set_context_value(&self, context_value: Option<String>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmProviderOptions")?;

        if *self.context_value.borrow() == context_value {
            return Ok(());
        }

        *self.context_value.borrow_mut() = context_value.clone();
        self.update_source_control(SCMProviderFeatures { context_value: Some(context_value), ..SCMProviderFeatures::default() });
        Ok(())
    }

    pub fn count(&self) -> Option<f64> {
        self.count.get()
    }

    pub fn set_count(&self, count: Option<f64>) {
        if self.count.get() == count {
            return;
        }

        self.count.set(count);
        self.update_source_control(SCMProviderFeatures { count: Some(count), ..SCMProviderFeatures::default() });
    }

    pub fn quick_diff_provider(&self) -> Option<Rc<dyn QuickDiffProvider>> {
        self.quick_diff_provider.borrow().clone()
    }

    pub fn set_quick_diff_provider(&self, quick_diff_provider: Option<Rc<dyn QuickDiffProvider>>) {
        *self.quick_diff_provider.borrow_mut() = quick_diff_provider.clone();
        let mut quick_diff_label = None;
        if is_proposed_api_enabled(&self.extension, "quickDiffProvider") {
            quick_diff_label = quick_diff_provider.as_ref().and_then(|provider| provider.label());
        }
        self.update_source_control(SCMProviderFeatures {
            has_quick_diff_provider: Some(quick_diff_provider.is_some()),
            quick_diff_label: Some(quick_diff_label),
            ..SCMProviderFeatures::default()
        });
    }

    /// `get secondaryQuickDiffProvider()`: pede a proposta `quickDiffProvider`.
    pub fn secondary_quick_diff_provider(&self) -> Result<Option<Rc<dyn QuickDiffProvider>>, Error> {
        check_proposed_api_enabled(&self.extension, "quickDiffProvider")?;
        Ok(self.secondary_quick_diff_provider.borrow().clone())
    }

    /// `set secondaryQuickDiffProvider(provider)`: pede a proposta `quickDiffProvider`.
    pub fn set_secondary_quick_diff_provider(&self, secondary_quick_diff_provider: Option<Rc<dyn QuickDiffProvider>>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "quickDiffProvider")?;

        *self.secondary_quick_diff_provider.borrow_mut() = secondary_quick_diff_provider.clone();
        let secondary_quick_diff_label = secondary_quick_diff_provider.as_ref().and_then(|provider| provider.label());
        self.update_source_control(SCMProviderFeatures {
            has_secondary_quick_diff_provider: Some(secondary_quick_diff_provider.is_some()),
            secondary_quick_diff_label: Some(secondary_quick_diff_label),
            ..SCMProviderFeatures::default()
        });
        Ok(())
    }

    /// `get historyProvider()`: pede a proposta `scmHistoryProvider`.
    pub fn history_provider(&self) -> Result<Option<Rc<dyn SourceControlHistoryProvider>>, Error> {
        check_proposed_api_enabled(&self.extension, "scmHistoryProvider")?;
        Ok(self.history_provider.borrow().clone())
    }

    /// `set historyProvider(provider)`: pede a proposta `scmHistoryProvider`.
    pub fn set_history_provider(&self, history_provider: Option<Rc<dyn SourceControlHistoryProvider>>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmHistoryProvider")?;

        *self.history_provider.borrow_mut() = history_provider.clone();
        let store = DisposableStore::new();
        self.history_provider_disposable.set(Some(store.clone()));

        self.update_source_control(SCMProviderFeatures { has_history_provider: Some(history_provider.is_some()), ..SCMProviderFeatures::default() });

        if let Some(history_provider) = history_provider {
            let (proxy, handle, provider) = (self.proxy.clone(), self.handle, history_provider.clone());
            store.add(history_provider.on_did_change_current_history_item_refs().listen(move |_: &()| {
                let history_item_ref = to_scm_history_item_ref_dto(provider.current_history_item_ref().as_ref());
                let history_item_remote_ref = to_scm_history_item_ref_dto(provider.current_history_item_remote_ref().as_ref());
                let history_item_base_ref = to_scm_history_item_ref_dto(provider.current_history_item_base_ref().as_ref());

                proxy.rpc_on_did_change_history_provider_current_history_item_refs(handle, history_item_ref, history_item_remote_ref, history_item_base_ref);
            }));

            let proxy = self.proxy.clone();
            store.add(history_provider.on_did_change_history_item_refs().listen(move |event: &SourceControlHistoryItemRefsChangeEvent| {
                if event.added.is_empty() && event.modified.is_empty() && event.removed.is_empty() {
                    return;
                }

                let to_dtos = |refs: &[SourceControlHistoryItemRef]| refs.iter().map(to_scm_history_item_ref_dto_with_icon).collect::<Vec<_>>();
                proxy.rpc_on_did_change_history_provider_history_item_refs(
                    handle,
                    SCMHistoryItemRefsChangeEventDto {
                        added: to_dtos(&event.added),
                        modified: to_dtos(&event.modified),
                        removed: to_dtos(&event.removed),
                        silent: event.silent,
                    },
                );
            }));
        }
        Ok(())
    }

    /// `get artifactProvider()`: pede a proposta `scmArtifactProvider`.
    pub fn artifact_provider(&self) -> Result<Option<Rc<dyn SourceControlArtifactProvider>>, Error> {
        check_proposed_api_enabled(&self.extension, "scmArtifactProvider")?;
        Ok(self.artifact_provider.borrow().clone())
    }

    /// `set artifactProvider(provider)`: pede a proposta `scmArtifactProvider`.
    pub fn set_artifact_provider(&self, artifact_provider: Option<Rc<dyn SourceControlArtifactProvider>>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmArtifactProvider")?;

        *self.artifact_provider.borrow_mut() = artifact_provider.clone();
        let store = DisposableStore::new();
        self.artifact_provider_disposable.set(Some(store.clone()));

        self.update_source_control(SCMProviderFeatures { has_artifact_provider: Some(artifact_provider.is_some()), ..SCMProviderFeatures::default() });

        if let Some(artifact_provider) = artifact_provider {
            let (proxy, handle) = (self.proxy.clone(), self.handle);
            store.add(artifact_provider.on_did_change_artifacts().listen(move |groups: &Vec<String>| {
                if !groups.is_empty() {
                    proxy.rpc_on_did_change_artifacts(handle, groups.clone());
                }
            }));
        }
        Ok(())
    }

    pub fn commit_template(&self) -> Option<String> {
        self.commit_template.borrow().clone()
    }

    pub fn set_commit_template(&self, commit_template: Option<String>) {
        if *self.commit_template.borrow() == commit_template {
            return;
        }

        *self.commit_template.borrow_mut() = commit_template.clone();
        self.update_source_control(SCMProviderFeatures { commit_template: Some(commit_template), ..SCMProviderFeatures::default() });
    }

    pub fn accept_input_command(&self) -> Option<Command> {
        self.accept_input_command.borrow().clone()
    }

    pub fn set_accept_input_command(&self, accept_input_command: Option<Command>) -> Result<(), Error> {
        let store = DisposableStore::new();
        self.accept_input_disposables.set(Some(store.clone()));

        *self.accept_input_command.borrow_mut() = accept_input_command.clone();

        let internal = self.converter.to_internal(accept_input_command.as_ref(), &store)?;
        self.update_source_control(SCMProviderFeatures { accept_input_command: Some(internal), ..SCMProviderFeatures::default() });
        Ok(())
    }

    /// `get actionButton()`: pede a proposta `scmActionButton`.
    pub fn action_button(&self) -> Result<Option<SourceControlActionButton>, Error> {
        check_proposed_api_enabled(&self.extension, "scmActionButton")?;
        Ok(self.action_button.borrow().clone())
    }

    /// `set actionButton(button)`: pede a proposta `scmActionButton`.
    pub fn set_action_button(&self, action_button: Option<SourceControlActionButton>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "scmActionButton")?;

        // We have to do this check before converting the command to it's internal
        // representation since that would always create a command with a unique
        // identifier
        let current = action_button_to_dynamic(self.action_button.borrow().as_ref());
        if structural_equals(&current, &action_button_to_dynamic(action_button.as_ref())) {
            return Ok(());
        }

        // In order to prevent disposing the action button command that are still rendered in the UI
        // until the next UI update, we ensure to dispose them after the update has been completed.
        let old_action_button_disposables = self.action_button_disposables.replace(DisposableStore::new());
        let action_button_disposables = self.action_button_disposables.borrow().clone();

        *self.action_button.borrow_mut() = action_button.clone();

        let action_button_dto = match &action_button {
            Some(action_button) => {
                let mut secondary_commands = None;
                if let Some(groups) = &action_button.secondary_commands {
                    let mut converted = Vec::with_capacity(groups.len());
                    for command_group in groups {
                        let mut converted_group = Vec::with_capacity(command_group.len());
                        for command in command_group {
                            converted_group.push(to_internal_required(&self.converter, command, &action_button_disposables)?);
                        }
                        converted.push(converted_group);
                    }
                    secondary_commands = Some(converted);
                }
                Some(SCMActionButtonDto {
                    command: SCMActionButtonCommandDto {
                        command: to_internal_required(&self.converter, &action_button.command.command, &action_button_disposables)?,
                        short_title: action_button.command.short_title.clone(),
                    },
                    secondary_commands,
                    enabled: action_button.enabled,
                })
            }
            None => None,
        };

        self.proxy
            .rpc_update_source_control(self.handle, SCMProviderFeatures { action_button: Some(action_button_dto), ..SCMProviderFeatures::default() })
            .finally(move || old_action_button_disposables.dispose());
        Ok(())
    }

    pub fn status_bar_commands(&self) -> Option<Vec<Command>> {
        self.status_bar_commands.borrow().clone()
    }

    pub fn set_status_bar_commands(&self, status_bar_commands: Option<Vec<Command>>) -> Result<(), Error> {
        let unchanged = match (&*self.status_bar_commands.borrow(), &status_bar_commands) {
            (Some(current), Some(next)) => command_list_equals(current, next),
            _ => false,
        };
        if unchanged {
            return Ok(());
        }

        // In order to prevent disposing status bar commands that are still rendered in the UI
        // until the next UI update, we ensure to dispose them after the update has been completed.
        let old_status_bar_disposables = self.status_bar_disposables.replace(DisposableStore::new());
        let status_bar_disposables = self.status_bar_disposables.borrow().clone();

        *self.status_bar_commands.borrow_mut() = status_bar_commands.clone();

        let mut internal = Vec::new();
        for command in status_bar_commands.iter().flatten() {
            internal.push(to_internal_required(&self.converter, command, &status_bar_disposables)?);
        }

        self.proxy
            .rpc_update_source_control(self.handle, SCMProviderFeatures { status_bar_commands: Some(internal), ..SCMProviderFeatures::default() })
            .finally(move || old_status_bar_disposables.dispose());
        Ok(())
    }

    /// `this.#proxy.$updateSourceControl(this.handle, features)`.
    fn update_source_control(&self, features: SCMProviderFeatures) -> Promise<()> {
        self.proxy.rpc_update_source_control(self.handle, features)
    }

    /// `createResourceGroup(id, label, options?)`.
    pub fn create_resource_group(&self, id: &str, label: &str, options: Option<CreateResourceGroupOptions>) -> Rc<ExtHostSourceControlResourceGroup> {
        let multi_diff_editor_enable_view_changes = is_proposed_api_enabled(&self.extension, "scmMultiDiffEditor")
            && options.and_then(|options| options.multi_diff_editor_enable_view_changes) == Some(true);
        let commands = self.commands.upgrade().expect("o ExtHostCommands está vivo enquanto a fonte cria um grupo");
        let group = ExtHostSourceControlResourceGroup::new(
            self.proxy.clone(),
            &commands,
            self.handle,
            id,
            label,
            multi_diff_editor_enable_view_changes,
            self.extension.clone(),
        );
        let this = self.this.clone();
        let group_handle = group.handle;
        let disposable = group.on_did_dispose().once().listen(move |_: &()| {
            if let Some(source_control) = this.upgrade() {
                source_control.created_resource_groups.borrow_mut().retain(|(created, _)| created.handle != group_handle);
            }
        });
        self.created_resource_groups.borrow_mut().push((group.clone(), disposable));
        self.eventually_add_resource_groups();
        group
    }

    /// `eventuallyAddResourceGroups()`, com o `@debounce(100)`.
    pub fn eventually_add_resource_groups(&self) {
        self.add_resource_groups_debounce.call(());
    }

    fn add_resource_groups(&self) {
        let mut groups: Vec<SCMGroupRegistration> = Vec::new();
        let mut splices: Vec<SCMRawResourceSplices> = Vec::new();

        let created: Vec<(Rc<ExtHostSourceControlResourceGroup>, DisposableRef)> = self.created_resource_groups.borrow().clone();
        for (group, disposable) in created {
            disposable.dispose();

            let (updating, updated_group) = (self.this.clone(), Rc::downgrade(&group));
            let update_listener = group.on_did_update_resource_states().listen(move |_: &()| {
                let (Some(source_control), Some(group)) = (updating.upgrade(), updated_group.upgrade()) else {
                    return;
                };
                {
                    let mut updated = source_control.updated_resource_groups.borrow_mut();
                    if !updated.iter().any(|candidate| Rc::ptr_eq(candidate, &group)) {
                        updated.push(group);
                    }
                }
                source_control.eventually_update_resource_states();
            });

            let (disposing, group_handle) = (self.this.clone(), group.handle);
            group.on_did_dispose().once().listen(move |_: &()| {
                let Some(source_control) = disposing.upgrade() else {
                    return;
                };
                source_control.updated_resource_groups.borrow_mut().retain(|candidate| candidate.handle != group_handle);
                update_listener.dispose();
                source_control.groups.borrow_mut().remove(&group_handle);
                source_control.proxy.rpc_unregister_group(source_control.handle, group_handle);
            });

            groups.push((group.handle, group.id.clone(), group.label(), group.features(), group.multi_diff_editor_enable_view_changes));

            match group.take_resource_state_snapshot() {
                Ok(snapshot) if !snapshot.is_empty() => splices.push(SCMRawResourceSplices { handle: group.handle, splices: snapshot }),
                Ok(_) => {}
                Err(error) => errors::on_unexpected_error(&error),
            }

            self.groups.borrow_mut().insert(group.handle, group);
        }

        self.proxy.rpc_register_groups(self.handle, groups, splices);
        self.created_resource_groups.borrow_mut().clear();
    }

    /// `eventuallyUpdateResourceStates()`, com o `@debounce(100)`.
    pub fn eventually_update_resource_states(&self) {
        self.update_resource_states_debounce.call(());
    }

    fn update_resource_states(&self) {
        let mut splices: Vec<SCMRawResourceSplices> = Vec::new();

        let updated: Vec<Rc<ExtHostSourceControlResourceGroup>> = self.updated_resource_groups.borrow().clone();
        for group in updated {
            match group.take_resource_state_snapshot() {
                Ok(snapshot) if snapshot.is_empty() => {}
                Ok(snapshot) => splices.push(SCMRawResourceSplices { handle: group.handle, splices: snapshot }),
                Err(error) => errors::on_unexpected_error(&error),
            }
        }

        if !splices.is_empty() {
            self.proxy.rpc_splice_resource_states(self.handle, splices);
        }

        self.updated_resource_groups.borrow_mut().clear();
    }

    pub fn get_resource_group(&self, handle: GroupHandle) -> Option<Rc<ExtHostSourceControlResourceGroup>> {
        self.groups.borrow().get(&handle).cloned()
    }

    pub fn set_selection_state(&self, selected: bool) {
        self.selected.set(selected);
        self.on_did_change_selection.fire(selected);
    }

    /// `provideArtifacts(group, token)`.
    pub fn provide_artifacts(&self, group: &str, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactDto>>> {
        let this = self.this.upgrade().expect("a fonte está viva enquanto um método dela roda");
        let group = group.to_owned();
        async_fn(async move {
            let provider_disposables = this.artifact_provider_disposable.value();
            let artifact_provider = this.artifact_provider()?;
            let artifacts = match artifact_provider {
                Some(provider) => await_maybe(provider.provide_artifacts(&group, &token)).await?,
                None => await_maybe(MaybePromise::Value(None)).await?,
            };
            if provider_disposables.as_ref().is_none_or(|provider_disposables| provider_disposables.is_disposed()) || token.is_cancellation_requested() {
                return Ok(None);
            }

            let commands_disposables = DisposableStore::new();
            let artifacts_dto = match artifacts {
                Some(artifacts) => {
                    let mut artifacts_dto = Vec::with_capacity(artifacts.len());
                    for artifact in &artifacts {
                        artifacts_dto.push(SCMArtifactDto {
                            id: artifact.id.clone(),
                            name: artifact.name.clone(),
                            description: artifact.description.clone(),
                            icon: get_history_item_icon_dto(artifact.icon.as_ref()),
                            timestamp: artifact.timestamp,
                            command: match &artifact.command {
                                Some(command) => this.converter.to_internal(Some(command), &commands_disposables)?,
                                None => None,
                            },
                        });
                    }
                    Some(artifacts_dto)
                }
                None => None,
            };

            if let Some(previous) = this.artifact_commands_disposables.get(&group) {
                previous.dispose();
            }
            this.artifact_commands_disposables.set(group, commands_disposables, false);

            Ok(artifacts_dto)
        })
    }
}

impl IDisposable for ExtHostSourceControl {
    fn dispose(&self) {
        self.accept_input_disposables.dispose();
        self.action_button_disposables.borrow().dispose();
        self.status_bar_disposables.borrow().dispose();
        self.history_provider_disposable.dispose();
        self.artifact_provider_disposable.dispose();
        self.artifact_commands_disposables.dispose();

        let groups: Vec<Rc<ExtHostSourceControlResourceGroup>> = self.groups.borrow().values().cloned().collect();
        for group in groups {
            group.dispose();
        }
        self.proxy.rpc_unregister_source_control(self.handle);

        self.on_did_change_selection.dispose();
        self.on_did_dispose.fire(());
        self.on_did_dispose.dispose();
    }
}

// ---- ExtHostSCM ----------------------------------------------------------------------------------------

/// O `processArgument` que `ExtHostSCM` registra em `commands.registerArgumentProcessor`.
struct SCMArgumentProcessor {
    source_controls: Rc<SourceControlMap>,
}

impl SCMArgumentProcessor {
    fn source_control(&self, handle: &Dynamic) -> Option<Rc<ExtHostSourceControl>> {
        self.source_controls.borrow().get(&handle_of(handle)?).cloned()
    }
}

impl ArgumentProcessor for SCMArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        if !arg.is_truthy() {
            return arg.clone();
        }
        let Dynamic::Number(mid) = arg.get_str("$mid") else {
            return arg.clone();
        };
        let Some(mid) = MarshalledId::from_i32(mid as i32).filter(|id| f64::from(*id as i32) == mid) else {
            return arg.clone();
        };

        match mid {
            MarshalledId::ScmResource => {
                let Some(source_control) = self.source_control(&arg.get_str("sourceControlHandle")) else {
                    return arg.clone();
                };
                let Some(group) = handle_of(&arg.get_str("groupHandle")).and_then(|handle| source_control.get_resource_group(handle)) else {
                    return arg.clone();
                };
                handle_of(&arg.get_str("handle"))
                    .and_then(|handle| group.get_resource_state(handle))
                    .map_or(Dynamic::Undefined, |state| state.to_dynamic())
            }
            MarshalledId::ScmResourceGroup => {
                let Some(source_control) = self.source_control(&arg.get_str("sourceControlHandle")) else {
                    return arg.clone();
                };
                handle_of(&arg.get_str("groupHandle"))
                    .and_then(|handle| source_control.get_resource_group(handle))
                    .map_or(Dynamic::Undefined, |group| group.to_dynamic())
            }
            MarshalledId::ScmProvider => self.source_control(&arg.get_str("handle")).map_or_else(|| arg.clone(), |source_control| source_control.to_dynamic()),
            _ => arg.clone(),
        }
    }
}

/// `ExtHostSCM`.
pub struct ExtHostSCM {
    proxy: Rc<dyn MainThreadSCMShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
    source_controls: Rc<SourceControlMap>,
    source_controls_by_extension: Rc<RefCell<ExtensionIdentifierMap<Vec<Rc<ExtHostSourceControl>>>>>,
    on_did_change_active_provider: Rc<Emitter<Rc<ExtHostSourceControl>>>,
    selected_source_control_handle: Cell<Option<u32>>,
    commands: Rc<ExtHostCommands>,
    ext_host_documents: Rc<ExtHostDocuments>,
    log_service: LogServiceRef,
}

impl ExtHostSCM {
    pub fn new(main_context: &dyn IMainContext, commands: Rc<ExtHostCommands>, ext_host_documents: Rc<ExtHostDocuments>, log_service: LogServiceRef) -> Rc<ExtHostSCM> {
        let source_controls: Rc<SourceControlMap> = Rc::new(RefCell::new(BTreeMap::new()));
        commands.register_argument_processor(Rc::new(SCMArgumentProcessor { source_controls: source_controls.clone() }));
        Rc::new(ExtHostSCM {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_SCM),
            telemetry: main_context.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
            source_controls,
            source_controls_by_extension: Rc::new(RefCell::new(ExtensionIdentifierMap::new())),
            on_did_change_active_provider: Emitter::new(),
            selected_source_control_handle: Cell::new(None),
            commands,
            ext_host_documents,
            log_service,
        })
    }

    /// `onDidChangeActiveProvider`.
    pub fn on_did_change_active_provider(&self) -> Event<Rc<ExtHostSourceControl>> {
        self.on_did_change_active_provider.event()
    }

    fn trace(&self, name: &str, args: &[LogArg]) {
        self.log_service.trace_args(name, args);
    }

    fn source_control(&self, handle: ProviderHandle) -> Option<Rc<ExtHostSourceControl>> {
        self.source_controls.borrow().get(&handle).cloned()
    }

    /// `createSourceControl(extension, id, label, rootUri, iconPath, isHidden, parent)`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_source_control(
        &self,
        extension: &ExtensionDescription,
        id: &str,
        label: &str,
        root_uri: Option<URI>,
        icon_path: Option<types::IconPath>,
        is_hidden: Option<bool>,
        parent: Option<&Rc<ExtHostSourceControl>>,
    ) -> Rc<ExtHostSourceControl> {
        let root_uri_text = root_uri.as_ref().map_or_else(|| "undefined".to_owned(), |root_uri| root_uri.to_string(false).to_string_lossy());
        self.trace(
            "ExtHostSCM#createSourceControl",
            &[LogArg::from(extension.identifier.value.as_str()), LogArg::from(id), LogArg::from(label), LogArg::from(root_uri_text.as_str())],
        );

        self.telemetry.rpc_public_log2(
            "api/scm/createSourceControl",
            Some(Dynamic::object([("extensionId", Dynamic::string(&extension.identifier.value))])),
        );

        let parent_source_control = parent.and_then(|parent| self.source_controls.borrow().values().find(|candidate| Rc::ptr_eq(candidate, parent)).cloned());
        let source_control = ExtHostSourceControl::new(
            extension.clone(),
            self.ext_host_documents.clone(),
            self.proxy.clone(),
            &self.commands,
            id,
            label,
            root_uri,
            icon_path,
            is_hidden,
            parent_source_control.as_ref(),
        );
        self.source_controls.borrow_mut().insert(source_control.handle, source_control.clone());

        {
            let mut by_extension = self.source_controls_by_extension.borrow_mut();
            let mut source_controls = by_extension.get(&extension.identifier.value).cloned().unwrap_or_default();
            source_controls.push(source_control.clone());
            by_extension.set(&extension.identifier.value, source_controls);
        }

        let (source_controls, by_extension, log_service) = (self.source_controls.clone(), self.source_controls_by_extension.clone(), self.log_service.clone());
        let (extension_id, id, label, handle) = (extension.identifier.value.clone(), id.to_owned(), label.to_owned(), source_control.handle);
        let root_uri_text = root_uri_text.clone();
        source_control.on_did_dispose().once().listen(move |_: &()| {
            log_service.trace_args(
                "ExtHostSCM#disposeSourceControl",
                &[LogArg::from(extension_id.as_str()), LogArg::from(id.as_str()), LogArg::from(label.as_str()), LogArg::from(root_uri_text.as_str())],
            );

            source_controls.borrow_mut().remove(&handle);

            let mut by_extension = by_extension.borrow_mut();
            let current = by_extension.get(&extension_id).cloned();
            if let Some(mut current) = current {
                if let Some(index) = current.iter().position(|candidate| candidate.handle == handle) {
                    current.remove(index);
                }

                if current.is_empty() {
                    by_extension.delete(&extension_id);
                } else {
                    by_extension.set(&extension_id, current);
                }
            }
        });

        source_control
    }

    /// `getLastInputBox(extension)` (obsoleto).
    pub fn get_last_input_box(&self, extension: &ExtensionDescription) -> Option<Rc<ExtHostSCMInputBox>> {
        self.trace("ExtHostSCM#getLastInputBox", &[LogArg::from(extension.identifier.value.as_str())]);

        let source_controls = self.source_controls_by_extension.borrow().get(&extension.identifier.value).cloned();
        source_controls.and_then(|source_controls| source_controls.last().map(|source_control| source_control.input_box()))
    }

    /// `$provideOriginalResource` e `$provideSecondaryOriginalResource`, que só diferem no provedor lido.
    fn provide_original_resource(
        &self,
        name: &str,
        source_control_handle: u32,
        uri: &URI,
        token: &CancellationToken,
        provider_of: impl FnOnce(&ExtHostSourceControl) -> Option<Rc<dyn QuickDiffProvider>>,
    ) -> Promise<Option<URI>> {
        self.trace(name, &[LogArg::from(f64::from(source_control_handle)), LogArg::from(uri.to_string(false).to_string_lossy().as_str())]);

        let Some(provider) = self.source_control(source_control_handle).and_then(|source_control| provider_of(&source_control)) else {
            return Promise::resolved(None);
        };
        match provider.provide_original_resource(uri, token) {
            Some(result) => as_promise(result),
            None => Promise::resolved(None),
        }
    }
}

/// O provedor (de histórico ou de artefatos) de uma fonte, e o resultado de uma chamada dele; sem fonte ou sem provedor o
/// resultado é `undefined` (o `?.` do original).
async fn call_provider<P: ?Sized + 'static, T: Clone + 'static>(
    source_controls: Rc<SourceControlMap>,
    handle: ProviderHandle,
    get_provider: impl FnOnce(&ExtHostSourceControl) -> Result<Option<Rc<P>>, Error> + 'static,
    call: impl FnOnce(&P) -> ProviderResult<T> + 'static,
) -> Result<Option<T>, Error> {
    let source_control = source_controls.borrow().get(&handle).cloned();
    let provider = match source_control {
        Some(source_control) => get_provider(&source_control)?,
        None => None,
    };
    let result = match &provider {
        Some(provider) => call(provider),
        None => MaybePromise::Value(None),
    };
    await_maybe(result).await
}

impl ExtHostSCM {
    /// O `try { ... } catch (err) { this.logService.error(name, err); return undefined; }` das funções assíncronas do original.
    fn guarded<T: Clone + 'static>(&self, name: &'static str, body: impl Future<Output = Result<Option<T>, Error>> + 'static) -> Promise<Option<T>> {
        let log_service = self.log_service.clone();
        async_fn(async move {
            match body.await {
                Ok(value) => Ok(value),
                Err(error) => {
                    log_service.error_args(&LogMessage::from(name), &[LogArg::from(&error)]);
                    Ok(None)
                }
            }
        })
    }
}

impl ExtHostSCMShape for ExtHostSCM {
    fn rpc_provide_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>> {
        self.provide_original_resource("ExtHostSCM#$provideOriginalResource", source_control_handle, uri, &token, |source_control| source_control.quick_diff_provider())
    }

    fn rpc_provide_secondary_original_resource(&self, source_control_handle: u32, uri: &URI, token: CancellationToken) -> Promise<Option<URI>> {
        self.provide_original_resource("ExtHostSCM#$provideSecondaryOriginalResource", source_control_handle, uri, &token, |source_control| {
            source_control.secondary_quick_diff_provider().ok().flatten()
        })
    }

    fn rpc_on_input_box_value_change(&self, source_control_handle: u32, value: &str) {
        self.trace("ExtHostSCM#$onInputBoxValueChange", &[LogArg::from(f64::from(source_control_handle))]);

        if let Some(source_control) = self.source_control(source_control_handle) {
            source_control.input_box.on_input_box_value_change(value);
        }
    }

    fn rpc_execute_resource_command(&self, source_control_handle: u32, group_handle: u32, handle: u32, preserve_focus: bool) -> Promise<()> {
        self.trace(
            "ExtHostSCM#$executeResourceCommand",
            &[LogArg::from(f64::from(source_control_handle)), LogArg::from(f64::from(group_handle)), LogArg::from(f64::from(handle))],
        );

        let Some(source_control) = self.source_control(source_control_handle) else {
            return Promise::resolved(());
        };

        let Some(group) = source_control.get_resource_group(group_handle) else {
            return Promise::resolved(());
        };

        group.execute_resource_command(handle, preserve_focus)
    }

    fn rpc_validate_input(&self, source_control_handle: u32, value: &str, cursor_position: f64) -> Promise<Option<(StringOrMarkdown, f64)>> {
        self.trace("ExtHostSCM#$validateInput", &[LogArg::from(f64::from(source_control_handle))]);

        let Some(source_control) = self.source_control(source_control_handle) else {
            return Promise::resolved(None);
        };

        let validate_input = match source_control.input_box.validate_input() {
            Ok(Some(validate_input)) => validate_input,
            Ok(None) => return Promise::resolved(None),
            Err(error) => return Promise::rejected(error),
        };

        let result = validate_input(value, cursor_position);
        async_fn(async move {
            let Some(result) = await_maybe(result).await? else {
                return Ok(None);
            };

            let Some(message) = type_converters::MarkdownString::from_strict(Some(&result.message)) else {
                return Ok(None);
            };

            Ok(Some((message, f64::from(result.validation_type.value()))))
        })
    }

    fn rpc_set_selected_source_control(&self, selected_source_control_handle: Option<u32>) -> Promise<()> {
        self.trace(
            "ExtHostSCM#$setSelectedSourceControl",
            &[selected_source_control_handle.map_or_else(|| LogArg::from(&Dynamic::Undefined), |handle| LogArg::from(f64::from(handle)))],
        );
        if self.selected_source_control_handle.get() == selected_source_control_handle {
            return Promise::resolved(());
        }

        if let Some(source_control) = selected_source_control_handle.and_then(|handle| self.source_control(handle)) {
            source_control.set_selection_state(true);
        }

        if let Some(source_control) = self.selected_source_control_handle.get().and_then(|handle| self.source_control(handle)) {
            source_control.set_selection_state(false);
        }

        self.selected_source_control_handle.set(selected_source_control_handle);
        Promise::resolved(())
    }

    fn rpc_provide_history_item_refs(&self, source_control_handle: u32, history_item_refs: Option<Vec<String>>, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemRefDto>>> {
        let source_controls = self.source_controls.clone();
        self.guarded("ExtHostSCM#$provideHistoryItemRefs", async move {
            let refs = call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.provide_history_item_refs(history_item_refs.as_deref(), &token),
            )
            .await?;
            Ok(refs.map(|refs| refs.iter().map(to_scm_history_item_ref_dto_with_icon).collect()))
        })
    }

    fn rpc_provide_history_items(&self, source_control_handle: u32, options: ISCMHistoryOptions, token: CancellationToken) -> Promise<Option<Vec<SCMHistoryItemDto>>> {
        let source_controls = self.source_controls.clone();
        self.guarded("ExtHostSCM#$provideHistoryItems", async move {
            let history_items = call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.provide_history_items(&options, &token),
            )
            .await?;
            Ok(history_items.map(|items| items.iter().map(to_scm_history_item_dto).collect()))
        })
    }

    fn rpc_provide_history_item_changes(
        &self,
        source_control_handle: u32,
        history_item_id: &str,
        history_item_parent_id: Option<&str>,
        token: CancellationToken,
    ) -> Promise<Option<Vec<SCMHistoryItemChangeDto>>> {
        let source_controls = self.source_controls.clone();
        let (history_item_id, history_item_parent_id) = (history_item_id.to_owned(), history_item_parent_id.map(str::to_owned));
        self.guarded("ExtHostSCM#$provideHistoryItemChanges", async move {
            let changes = call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.provide_history_item_changes(&history_item_id, history_item_parent_id.as_deref(), &token),
            )
            .await?;
            Ok(changes.map(|changes| {
                changes
                    .into_iter()
                    .map(|change| SCMHistoryItemChangeDto { uri: change.uri, original_uri: change.original_uri, modified_uri: change.modified_uri })
                    .collect()
            }))
        })
    }

    fn rpc_resolve_history_item(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<SCMHistoryItemDto>> {
        let source_controls = self.source_controls.clone();
        let history_item_id = history_item_id.to_owned();
        self.guarded("ExtHostSCM#$resolveHistoryItem", async move {
            let history_item = call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.resolve_history_item(&history_item_id, &token),
            )
            .await?;
            Ok(history_item.map(|history_item| to_scm_history_item_dto(&history_item)))
        })
    }

    fn rpc_resolve_history_item_chat_context(&self, source_control_handle: u32, history_item_id: &str, token: CancellationToken) -> Promise<Option<String>> {
        let source_controls = self.source_controls.clone();
        let history_item_id = history_item_id.to_owned();
        self.guarded("ExtHostSCM#$resolveHistoryItemChatContext", async move {
            call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.resolve_history_item_chat_context(&history_item_id, &token),
            )
            .await
        })
    }

    fn rpc_resolve_history_item_change_range_chat_context(
        &self,
        source_control_handle: u32,
        history_item_id: &str,
        history_item_parent_id: &str,
        path: &str,
        token: CancellationToken,
    ) -> Promise<Option<String>> {
        let source_controls = self.source_controls.clone();
        let (history_item_id, history_item_parent_id, path) = (history_item_id.to_owned(), history_item_parent_id.to_owned(), path.to_owned());
        self.guarded("ExtHostSCM#$resolveHistoryItemChangeRangeChatContext", async move {
            call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.resolve_history_item_change_range_chat_context(&history_item_id, &history_item_parent_id, &path, &token),
            )
            .await
        })
    }

    fn rpc_resolve_history_item_refs_common_ancestor(&self, source_control_handle: u32, history_item_refs: Vec<String>, token: CancellationToken) -> Promise<Option<String>> {
        let source_controls = self.source_controls.clone();
        self.guarded("ExtHostSCM#$resolveHistoryItemRefsCommonAncestor", async move {
            call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.history_provider(),
                move |provider| provider.resolve_history_item_refs_common_ancestor(&history_item_refs, &token),
            )
            .await
        })
    }

    fn rpc_provide_artifact_groups(&self, source_control_handle: u32, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactGroupDto>>> {
        let source_controls = self.source_controls.clone();
        self.guarded("ExtHostSCM#$provideArtifactGroups", async move {
            let groups = call_provider(
                source_controls,
                source_control_handle,
                |source_control| source_control.artifact_provider(),
                move |provider| provider.provide_artifact_groups(&token),
            )
            .await?;
            Ok(groups.map(|groups| {
                groups
                    .iter()
                    .map(|group| SCMArtifactGroupDto {
                        id: group.id.clone(),
                        name: group.name.clone(),
                        icon: get_history_item_icon_dto(group.icon.as_ref()),
                        supports_folders: group.supports_folders,
                    })
                    .collect()
            }))
        })
    }

    fn rpc_provide_artifacts(&self, source_control_handle: u32, group: &str, token: CancellationToken) -> Promise<Option<Vec<SCMArtifactDto>>> {
        match self.source_control(source_control_handle) {
            Some(source_control) => source_control.provide_artifacts(group, token),
            None => Promise::resolved(None),
        }
    }
}
