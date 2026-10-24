//! Porte de `src/vs/workbench/contrib/timeline/common/timeline.ts`: os tipos do serviço de timeline (itens, opções,
//! provedores, eventos) e o contrato [`ITimelineService`].
//!
//! Divergências:
//! - `TimelineProvider` e `WorkspaceTimelineProvider` (interfaces com métodos e `dispose`) são traits: o provedor
//!   implementa o trait e o serviço guarda um `Rc<dyn ...>`, cuja identidade (o `===` do original) é [`same_provider`].
//! - `string | readonly string[]` do `scheme` é [`TimelineScheme`]; `Event<readonly URI[] | undefined>` é
//!   `Event<Option<Vec<URI>>>`; `Event<void>` é `Event<()>`.
//! - `command?: Command` (de `languages.ts`) é o [`ICommandDto`], que tem os mesmos campos mais o `$ident`.
//! - `TimelineRequest.result` é uma [`Promise`] e `tokenSource` é o [`CancellationTokenSource`].
//! - `setUri` (`await viewsService.openView(...)`) devolve `Promise<()>`.
//! - `TimelineTriggerKind` é um `const enum` aqui e outro `enum` em `extHostTypes.ts`; no porte são dois tipos, como
//!   no original.
//! - `IEditableData` é o [`EditableData`] de `contrib::files::browser_files`, onde mora até `common/views.ts` ser
//!   portado por inteiro.
//! - `toKey` e `TimelinePaneId` são [`to_key`] e [`TIMELINE_PANE_ID`].

use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::accessibility::IAccessibilityInformation;

use crate::api::common::ext_host_protocol::ICommandDto;
use crate::contrib::files::browser_files::EditableData;

/// `toKey(extension, source)`.
pub fn to_key(extension: &str, source: &str) -> String {
    format!("{}|{source}", ExtensionIdentifier::to_key(extension))
}

/// `TimelinePaneId`.
pub const TIMELINE_PANE_ID: &str = "timeline";

/// `TimelineItem`.
#[derive(Clone, Debug, Default)]
pub struct TimelineItem {
    /// The handle of the item must be unique across all the timeline items provided by this source.
    pub handle: String,

    /// The identifier of the timeline provider this timeline item is from.
    pub source: String,

    pub id: Option<String>,

    pub label: String,
    pub description: Option<String>,
    pub tooltip: Option<StringOrMarkdown>,

    pub timestamp: f64,

    pub accessibility_information: Option<IAccessibilityInformation>,

    pub icon: Option<URI>,
    pub icon_dark: Option<URI>,
    pub theme_icon: Option<ThemeIcon>,

    pub command: Option<ICommandDto>,
    pub context_value: Option<String>,

    /// The resource's content as of this item, used to compare items.
    pub resource_uri: Option<URI>,

    pub relative_time: Option<String>,
    pub relative_time_full_word: Option<String>,
    pub hide_relative_time: Option<bool>,
}

/// Signals that timelines of a provider changed.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineChangeEvent {
    /// The identifier of the timeline provider this event is from.
    pub id: String,

    /// Whether the workspace timeline changed, rather than timelines of files and folders.
    pub workspace: bool,

    /// The files or folders whose timelines changed, or `None` if all of them changed. Always `None` for the
    /// workspace timeline.
    pub uris: Option<Vec<URI>>,
}

/// `TimelineTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineTriggerKind {
    Invoke = 1,
    Automatic = 2,
}

/// `TimelineOptions`.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineOptions {
    pub cursor: Option<String>,
    pub limit: Option<f64>,
    pub trigger_kind: TimelineTriggerKind,

    /// Internal to the Timeline view: whether the extension host should drop the items it cached for the provider.
    pub reset_cache: Option<bool>,

    /// Internal to the Timeline view: whether the extension host should cache the returned items for commands.
    pub cache_results: Option<bool>,
}

impl TimelineOptions {
    /// As opções só com o `triggerKind`, o resto ausente.
    pub fn new(trigger_kind: TimelineTriggerKind) -> Self {
        Self { cursor: None, limit: None, trigger_kind, reset_cache: None, cache_results: None }
    }
}

/// `Timeline`.
#[derive(Clone, Debug)]
pub struct Timeline {
    /// The identifier of the timeline provider this timeline is from.
    pub source: String,

    pub items: Vec<TimelineItem>,

    /// A provider-defined cursor for requesting the items after the ones returned, `None` if there are no more.
    pub cursor: Option<String>,
}

/// `string | readonly string[]`: os esquemas de recurso de um provedor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineScheme {
    One(String),
    Many(Vec<String>),
}

/// `WorkspaceTimelineProviderDescriptor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceTimelineProviderDescriptor {
    /// An identifier of the source of the timeline items. This can be used to filter sources.
    pub id: String,

    /// A human-readable string describing the source of the timeline items. This can be used as the display label
    /// when filtering sources.
    pub label: String,
}

/// `TimelineProviderDescriptor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineProviderDescriptor {
    pub id: String,
    pub label: String,

    /// The resource scheme(s) this timeline provider is providing entries for.
    pub scheme: TimelineScheme,
}

/// Provides the timelines of files and folders.
pub trait TimelineProvider: IDisposable {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn scheme(&self) -> TimelineScheme;

    /// Fires the files or folders whose timelines changed, or `None` if all of them changed.
    fn on_did_change(&self) -> Option<Event<Option<Vec<URI>>>>;

    fn provide_timeline(&self, uri: &URI, options: &TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>>;
}

/// Provides the timeline of the whole workspace.
pub trait WorkspaceTimelineProvider: IDisposable {
    fn id(&self) -> String;
    fn label(&self) -> String;

    fn on_did_change(&self) -> Option<Event<()>>;

    fn provide_workspace_timeline(&self, options: &TimelineOptions, token: CancellationToken) -> Promise<Option<Timeline>>;
}

/// O `===` entre dois provedores: o mesmo objeto.
pub fn same_provider<T: ?Sized>(a: &Rc<T>, b: &Rc<T>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `TimelineSource`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineSource {
    pub id: String,
    pub label: String,
}

/// Signals that timeline sources were added or removed. A source in both `removed` and `added` remains with a
/// different set of providers, e.g. when only its workspace timeline provider was removed, and is reloaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineProvidersChangeEvent {
    pub added: Option<Vec<String>>,
    pub removed: Option<Vec<String>>,
}

/// `TimelineRequest`.
#[derive(Clone)]
pub struct TimelineRequest {
    pub result: Promise<Option<Timeline>>,
    pub options: TimelineOptions,
    pub source: String,
    pub token_source: CancellationTokenSource,

    /// The file or folder the timeline is requested for, `None` for the workspace timeline.
    pub uri: Option<URI>,
}

/// `TimelineItemIdentifier`: identifies a timeline item across all timeline providers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineItemIdentifier {
    /// The identifier of the timeline provider the item is from.
    pub source: String,

    /// The handle of the item, unique across the items of its provider.
    pub handle: String,
}

/// `TimelineEditable`: a timeline item that is being edited inline in the Timeline view.
#[derive(Clone)]
pub struct TimelineEditable {
    pub item: TimelineItemIdentifier,
    pub data: EditableData,
}

/// `ITimelineService`. Timelines are shown for a file or folder, or for the whole workspace. Throughout the
/// timeline APIs, a `uri` of `None` stands for the workspace timeline.
pub trait ITimelineService {
    fn on_did_change_providers(&self) -> Event<TimelineProvidersChangeEvent>;
    fn on_did_change_timeline(&self) -> Event<TimelineChangeEvent>;

    /// Fires when a timeline is requested to be shown, with `None` for the workspace timeline.
    fn on_did_change_uri(&self) -> Event<Option<URI>>;

    /// Fires when a timeline item starts or stops being edited inline.
    fn on_did_change_editable(&self) -> Event<()>;

    /// The timeline item that is being edited inline, if any.
    fn editable(&self) -> Option<TimelineEditable>;

    fn register_timeline_provider(&self, provider: Rc<dyn TimelineProvider>) -> DisposableRef;
    fn unregister_timeline_provider(&self, id: &str);

    fn register_workspace_timeline_provider(&self, provider: Rc<dyn WorkspaceTimelineProvider>) -> DisposableRef;
    fn unregister_workspace_timeline_provider(&self, id: &str);

    /// All sources, from both timeline and workspace timeline providers.
    fn get_sources(&self) -> Vec<TimelineSource>;

    /// Whether any provider contributes to the workspace timeline.
    fn has_workspace_providers(&self) -> bool;

    fn get_timeline(&self, id: &str, uri: Option<&URI>, options: &TimelineOptions, token_source: CancellationTokenSource) -> Option<TimelineRequest>;

    /// Opens the Timeline view and pins it to the given file or folder, or to the workspace timeline when `uri` is
    /// `None`.
    fn set_uri(&self, uri: Option<URI>) -> Promise<()>;

    /// Starts editing the given timeline item inline in the Timeline view, or stops editing when `data` is `None`.
    fn set_editable(&self, item: TimelineItemIdentifier, data: Option<EditableData>);
}

je_instantiation::service_id!(pub I_TIMELINE_SERVICE: dyn ITimelineService = "timeline");
