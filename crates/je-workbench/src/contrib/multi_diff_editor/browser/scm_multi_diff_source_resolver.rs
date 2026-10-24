//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/scmMultiDiffSourceResolver.ts`: o `ScmMultiDiffSourceResolver`
//! (diff dos arquivos de um grupo de recursos do SCM) e o `ScmHistoryItemResolver` (diff das mudanças de um item do
//! histórico), com a URI de origem de cada um.
//!
//! Divergências:
//! - **O que não entra.** `ScmMultiDiffSourceResolverContribution` depende do `IMultiDiffSourceResolverService`, e
//!   `OpenScmGroupAction` do `Action2` com `openEditor({ label, multiDiffSource, options })` e do `IMultiDiffEditorOptions`;
//!   nenhum dos três está portado. Entram juntos com o serviço de resolvedores.
//! - Os serviços do SCM são os contratos de [`crate::contrib::scm::common::scm`] (recorte do que o resolvedor lê).
//! - `observableFromEvent(this, ...)` é `observable_from_event` (o dono só dá nome ao observável). `waitForState(obs)` sem
//!   predicado usa a veracidade do valor, que aqui é `Option::is_some`.
//! - `JSON.parse(uri.query)` é o parser JSON do porte sem comentários nem vírgula final; erro de sintaxe vira `undefined`,
//!   como o `try/catch` do original. `JSON.stringify` omite as propriedades `undefined`.
//! - `badge instanceof ProgressBadge` é `IBadge::is_progress_badge`.
//! - `ValueWithChangeEvent.const(...)` é o [`ConstResources`] (nunca muda, o evento é `Event.None`).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::comparers::compare_paths;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::event::{Event, ValueWithChangeEventLike};
use je_base::json::{ParseOptions, Value, parse as parse_json};
use je_base::observable::base::IObservable;
use je_base::observable::{ObservableExt, ValueWithChangeEventFromObservable, observable_from_event, wait_for_state};
use je_base::path::{PathModule, Platform};
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;

use crate::contrib::multi_diff_editor::browser::multi_diff_source_resolver_service::{
    IMultiDiffSourceResolver, IResolvedMultiDiffSource, MultiDiffEditorItem, MultiDiffResources,
};
use crate::contrib::scm::common::scm::{ISCMProvider, ISCMRepository, ISCMResourceGroup, ISCMService};
use crate::services::activity::activity::IActivityService;

/// `JSON.parse(text)` seguido da checagem `typeof query === 'object' && query !== null`: o objeto, ou `None`.
fn parse_query_object(query: &str) -> Option<je_base::json::Object> {
    let units: Vec<u16> = query.encode_utf16().collect();
    let mut errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    match parse_json(&units, &mut errors, options) {
        Some(Value::Object(object)) if errors.is_empty() => Some(object),
        _ => None,
    }
}

fn string_field(object: &je_base::json::Object, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `fsPath` na plataforma do hospedeiro.
fn fs_path(uri: &URI) -> String {
    je_platform::resources::fs_path(uri)
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `ScmMultiDiffSourceResolver`.
pub struct ScmMultiDiffSourceResolver {
    scm_service: Rc<dyn ISCMService>,
    activity_service: Rc<dyn IActivityService>,
}

impl ScmMultiDiffSourceResolver {
    const SCHEME: &'static str = "scm-multi-diff-source";

    /// `new ScmMultiDiffSourceResolver(scmService, activityService)`.
    pub fn new(scm_service: Rc<dyn ISCMService>, activity_service: Rc<dyn IActivityService>) -> Self {
        Self { scm_service, activity_service }
    }

    /// `getMultiDiffSourceUri(repositoryUri, groupId)`.
    pub fn get_multi_diff_source_uri(repository_uri: &str, group_id: &str) -> URI {
        let query = Dynamic::object([("repositoryUri", Dynamic::string(repository_uri)), ("groupId", Dynamic::string(group_id))]);
        URI::from(
            &UriComponents { scheme: Some(JsString::from(Self::SCHEME)), query: json_stringify(&query).map(|q| JsString::from(q.as_str())), ..UriComponents::default() },
            false,
        )
    }

    /// `parseUri(uri)`: o URI do repositório e o id do grupo.
    fn parse_uri(uri: &URI) -> Option<(URI, String)> {
        if *uri.scheme() != Self::SCHEME {
            return None;
        }
        let query = parse_query_object(&uri.query().to_string_lossy())?;
        let repository_uri = string_field(&query, "repositoryUri")?;
        let group_id = string_field(&query, "groupId")?;
        Some((URI::parse(repository_uri.as_str(), false), group_id))
    }
}

impl IMultiDiffSourceResolver for ScmMultiDiffSourceResolver {
    fn can_handle_uri(&self, uri: &URI) -> bool {
        Self::parse_uri(uri).is_some()
    }

    fn resolve_diff_source(&self, uri: &URI) -> Promise<Rc<dyn IResolvedMultiDiffSource>> {
        let (repository_uri, group_id) = Self::parse_uri(uri).expect("resolve_diff_source recebe um URI que can_handle_uri aceita");
        let scm_service = self.scm_service.clone();
        let activity_service = self.activity_service.clone();
        async_fn(async move {
            let repository_key = repository_uri.to_string(false);
            let repository_source = scm_service.clone();
            let repository = wait_for_state(
                observable_from_event(scm_service.on_did_add_repository(), move |_| {
                    repository_source.repositories().into_iter().find(|r| r.provider().root_uri().is_some_and(|root| root.to_string(false) == repository_key))
                }),
                |repository| repository.is_some(),
                None,
                None,
            )
            .await?
            .expect("o predicado só aceita um repositório");

            let provider = repository.provider();
            let group_provider = provider.clone();
            let group = wait_for_state(
                observable_from_event(provider.on_did_change_resource_groups(), move |_| group_provider.groups().into_iter().find(|g| g.id() == group_id)),
                |group| group.is_some(),
                None,
                None,
            )
            .await?
            .expect("o predicado só aceita um grupo");

            let activity_source = activity_service.clone();
            let scm_activities = observable_from_event(activity_service.on_did_change_activity(), move |_| {
                Rc::new(activity_source.get_view_container_activities("workbench.view.scm"))
            });
            let scm_view_has_no_progress_badge = scm_activities.map(|activities, _| !activities.iter().any(|a| a.badge.is_progress_badge()));
            wait_for_state(scm_view_has_no_progress_badge, |value| *value, None, None).await?;

            let source: Rc<dyn IResolvedMultiDiffSource> = Rc::new(ScmResolvedMultiDiffSource::new(group, repository));
            Ok(source)
        })
    }
}

/// `ScmHistoryItemUriFields`.
struct ScmHistoryItemUriFields {
    repository_id: String,
    history_item_id: String,
    history_item_parent_id: Option<String>,
    history_item_display_id: Option<String>,
}

/// `ScmHistoryItemResolver`.
pub struct ScmHistoryItemResolver {
    scm_service: Rc<dyn ISCMService>,
}

impl ScmHistoryItemResolver {
    pub const SCHEME: &'static str = "scm-history-item";

    /// `new ScmHistoryItemResolver(scmService)`.
    pub fn new(scm_service: Rc<dyn ISCMService>) -> Self {
        Self { scm_service }
    }

    /// `getMultiDiffSourceUri(provider, historyItemId, historyItemParentId, historyItemDisplayId)`.
    pub fn get_multi_diff_source_uri(
        provider: &dyn ISCMProvider,
        history_item_id: &str,
        history_item_parent_id: Option<&str>,
        history_item_display_id: Option<&str>,
    ) -> URI {
        let optional = |value: Option<&str>| value.map_or(Dynamic::Undefined, Dynamic::string);
        let query = Dynamic::object([
            ("repositoryId", Dynamic::string(&provider.id())),
            ("historyItemId", Dynamic::string(history_item_id)),
            ("historyItemParentId", optional(history_item_parent_id)),
            ("historyItemDisplayId", optional(history_item_display_id)),
        ]);
        URI::from(
            &UriComponents {
                scheme: Some(JsString::from(Self::SCHEME)),
                path: provider.root_uri().map(|root| JsString::from(fs_path(&root).as_str())),
                query: json_stringify(&query).map(|q| JsString::from(q.as_str())),
                ..UriComponents::default()
            },
            true,
        )
    }

    /// `parseUri(uri)`.
    fn parse_uri(uri: &URI) -> Option<ScmHistoryItemUriFields> {
        if *uri.scheme() != Self::SCHEME {
            return None;
        }
        let query = parse_query_object(&uri.query().to_string_lossy())?;
        // `typeof x !== 'string' && x !== undefined` é inválido: a propriedade ausente vale, o `null` e os demais tipos não.
        let optional_string = |key: &str| match query.get(key) {
            None => Some(None),
            Some(Value::String(text)) => Some(Some(text.to_string_lossy())),
            Some(_) => None,
        };
        Some(ScmHistoryItemUriFields {
            repository_id: string_field(&query, "repositoryId")?,
            history_item_id: string_field(&query, "historyItemId")?,
            history_item_parent_id: optional_string("historyItemParentId")?,
            history_item_display_id: optional_string("historyItemDisplayId")?,
        })
    }
}

impl IMultiDiffSourceResolver for ScmHistoryItemResolver {
    fn can_handle_uri(&self, uri: &URI) -> bool {
        Self::parse_uri(uri).is_some()
    }

    fn resolve_diff_source(&self, uri: &URI) -> Promise<Rc<dyn IResolvedMultiDiffSource>> {
        let fields = Self::parse_uri(uri).expect("resolve_diff_source recebe um URI que can_handle_uri aceita");
        let scm_service = self.scm_service.clone();
        async_fn(async move {
            let ScmHistoryItemUriFields { repository_id, history_item_id, history_item_parent_id, history_item_display_id } = fields;

            let repository_source = scm_service.clone();
            let repository = wait_for_state(
                observable_from_event(scm_service.on_did_add_repository(), move |_| repository_source.get_repository(&repository_id)),
                |repository| repository.is_some(),
                None,
                None,
            )
            .await?
            .expect("o predicado só aceita um repositório");
            let history_provider = wait_for_state(repository.provider().history_provider(), |provider| provider.is_some(), None, None)
                .await?
                .expect("o predicado só aceita um provedor de histórico");
            let history_item_changes = history_provider
                .provide_history_item_changes(&history_item_id, history_item_parent_id.as_deref())
                .await?
                .unwrap_or_default();

            let path = PathModule::native(&ProcessEnvironment);
            let display_id = history_item_display_id.unwrap_or_else(|| history_item_id.clone());
            let items: Vec<MultiDiffEditorItem> = history_item_changes
                .into_iter()
                .map(|change| {
                    let go_to_file_editor_title = change.modified_uri.as_ref().map(|modified| format!("{} ({display_id})", path.basename(&fs_path(modified), None)));
                    MultiDiffEditorItem::new(change.original_uri, change.modified_uri.clone(), change.modified_uri, go_to_file_editor_title, None)
                })
                .collect();

            let source: Rc<dyn IResolvedMultiDiffSource> = Rc::new(ConstResolvedSource { resources: Rc::new(ConstResources { value: Rc::new(items) }) });
            Ok(source)
        })
    }
}

/// `ValueWithChangeEvent.const(value)`: nunca muda, então o evento é `Event.None`.
struct ConstResources {
    value: Rc<Vec<MultiDiffEditorItem>>,
}

impl ValueWithChangeEventLike<Rc<Vec<MultiDiffEditorItem>>> for ConstResources {
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }

    fn value(&self) -> Rc<Vec<MultiDiffEditorItem>> {
        self.value.clone()
    }
}

/// `{ resources }`: o `IResolvedMultiDiffSource` sem chaves de contexto.
struct ConstResolvedSource {
    resources: Rc<ConstResources>,
}

impl IResolvedMultiDiffSource for ConstResolvedSource {
    fn resources(&self) -> MultiDiffResources {
        self.resources.clone()
    }
}

/// `ScmResolvedMultiDiffSource`.
struct ScmResolvedMultiDiffSource {
    resources: Rc<ValueWithChangeEventFromObservable<Rc<Vec<MultiDiffEditorItem>>>>,
    context_keys: Vec<(String, Dynamic)>,
}

impl ScmResolvedMultiDiffSource {
    fn new(group: Rc<dyn ISCMResourceGroup>, repository: Rc<dyn ISCMRepository>) -> Self {
        let platform = Platform::detect();
        let source_group = group.clone();
        let resources: IObservable<Rc<Vec<MultiDiffEditorItem>>> = observable_from_event(group.on_did_change_resources(), move |_| {
            let mut resources = source_group.resources();
            resources.sort_by(|a, b| compare_paths(&units(&fs_path(&a.source_uri())), &units(&fs_path(&b.source_uri())), false, &platform).cmp(&0));
            Rc::new(
                resources
                    .iter()
                    .map(|e| MultiDiffEditorItem::new(e.multi_diff_editor_original_uri(), e.multi_diff_editor_modified_uri(), Some(e.source_uri()), None, None))
                    .collect::<Vec<_>>(),
            )
        });
        let context_keys = vec![
            ("scmResourceGroup".to_string(), Dynamic::string(&group.id())),
            ("scmProvider".to_string(), Dynamic::string(&repository.provider().provider_id())),
        ];
        Self { resources: Rc::new(ValueWithChangeEventFromObservable { observable: resources }), context_keys }
    }
}

impl IResolvedMultiDiffSource for ScmResolvedMultiDiffSource {
    fn resources(&self) -> MultiDiffResources {
        self.resources.clone()
    }

    fn context_keys(&self) -> Option<Vec<(String, Dynamic)>> {
        Some(self.context_keys.clone())
    }
}
