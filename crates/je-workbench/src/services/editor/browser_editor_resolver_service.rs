//! Porte de `src/vs/workbench/services/editor/browser/editorResolverService.ts`: o `EditorResolverService`, que guarda os
//! editores registrados (padrão glob, prioridade, fábricas de entrada), lê as associações do usuário e resolve uma entrada
//! sem tipo para o editor que a abre, no grupo que o `findGroup` escolhe.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original; a classe
//!   herda de `Disposable`, aqui um `Disposable` guardado no campo `base`. `registerSingleton(..., Eager)` é [`register`].
//!   Os contratos que o porte ainda não tem por inteiro são os reduzidos de
//!   [`editor_group_contracts`](super::editor_group_contracts) (`IEditorGroupsService`) e de
//!   [`extension_service_contract`](super::extension_service_contract) (`IExtensionService`).
//! - **`invokeFunction(findGroup, ...)`** é `invoke_function` do serviço de instanciação chamando
//!   [`find_group`](super::editor_group_finder::find_group); o `createInstance(SideBySideEditorInput, ...)` busca o
//!   `IEditorService` no accessor e chama o construtor.
//! - **Estado.** Os `Map<string | IRelativePattern, Map<string, RegisteredEditors>>` são listas de pares na ordem de
//!   inserção (um `Map` do JavaScript a guarda), com a chave comparada como em [`GlobPattern`]. O `insert` de `arrays.ts` que
//!   devolve o `remove` por identidade é um número de registro por entrada. `_flattenedEditors` e `_shouldReFlattenEditors`
//!   são atualizados nos mesmos pontos do original (`flatten_editors_map` no lugar de `this._flattenedEditors =
//!   this._flattenEditorsMap()`).
//! - **Funções `async`.** O corpo roda já, de forma síncrona, até o primeiro `await` ([`async_fn`]); o `return promise`
//!   do `resolveEditor` (side by side) custa as duas rodadas da adoção ([`async_fn_maybe`]); o `await` de um valor que pode
//!   não ser promessa é [`await_maybe`]; o `await` de um `async` privado (`doResolveEditor`,
//!   `doHandleConflictingDefaults`, `moveExistingEditorForResource`) é uma promessa devolvida por [`async_fn`], que custa a
//!   rodada do original. `resolveUntypedInputAndGroup` devolve `MaybePromise`, e o `await` só acontece no ramo da promessa.
//! - **`resolveUntypedInputAndGroup`** nunca devolve `undefined` no original (o tipo de retorno o admite, o corpo não): o
//!   tipo daqui não tem o ramo, e `if (!resolvedUntypedAndGroup) return NONE` não existe. O mesmo vale para
//!   `resource.scheme === undefined || resource === null`, que um `URI` com esquema texto nunca satisfaz, e para
//!   `isEditorInputWithOptions(editor)`, que é sempre falso para uma `IUntypedEditorInput` (a forma não tem `editor`).
//! - **`resolveEditor` recebe a entrada por referência** e trabalha sobre uma cópia: o original altera o `options` do
//!   objeto recebido (`untypedEditor.options = ...`); o resultado carrega as opções resolvidas.
//! - **Opções (`{ ...a, ...b }`)**: o que o segundo define vence ([`RegisteredEditorOptions::merged`],
//!   [`EditorInputFactoryObject::merged`]); `{ override: id, ...options }` só põe o `override` quando as opções não o têm.
//! - **Ordenação.** O `sort` do original usa um comparador que não é ordem total quando se misturam padrões de texto e
//!   relativos de mesma prioridade; [`sort_by_compare`] reproduz a sequência de chamadas do V8. O comprimento de um padrão
//!   é o de unidades UTF-16 (`string.length`).
//! - **JSON do armazenamento.** `JSON.parse` e `JSON.stringify` são `parse_json` e `stringify_json` do sincronizador de
//!   configurações (`user_data_sync::abstract_synchronizer`). O cache, que o original lê com `new Set(JSON.parse(...))`, só
//!   guarda os textos (os padrões são sempre texto) e lança `TypeError` quando o valor guardado não é uma lista.
//! - **Comando `_workbench.editor.configureDefaultEditor`.** O `executeCommand(CONFIGURE_DEFAULT_EDITOR_COMMAND_ID,
//!   untypedInput, currentEditor, group.id)` passa objetos vivos, e o `Dynamic` dos argumentos de comando não carrega
//!   objetos do porte. Os três argumentos viajam numa [`ConfigureDefaultEditorArgs`] guardada numa tabela da thread e o
//!   comando recebe um só argumento, o número que a identifica: o tratador (que fica no `openWithEditorPicker`) lê
//!   [`ConfigureDefaultEditorArgs::from_command_args`], e a entrada é solta quando a promessa do comando assenta. A
//!   constante do identificador mora aqui até o `openWithEditorPicker.ts` ser portado.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::{distinct, sort_by_compare};
use je_base::async_::{async_fn, async_fn_maybe, await_maybe};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Event, PauseableEmitter};
use je_base::glob::PatternArg;
use je_base::json::{Object as JsonObject, Value as JsonValue};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, id_of, to_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriComponents};
use je_instantiation::{Injectable, InstantiationService, InstantiationType, register_singleton, service_impl};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::log::{LogServiceRef, Logger};
use je_platform::notification::notification::{BasePromptChoice, INotificationService, PromptChoice, PromptChoiceKind, Severity};
use je_platform::opener::editor_types::{EditorActivation, EditorOverride, EditorResolution};
use je_platform::resources::ext_uri;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_sync::abstract_synchronizer::{parse_json, stringify_json};
use je_storage::StorageValue;

use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::side_by_side_editor_input::SideBySideEditorInput;
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorResourceAccessor, EditorResourceAccessorOptions, GroupIdentifier,
    SideBySideEditor, TextEditorOptions, UntypedEditorInput, default_editor_association, is_same_editor,
};
use crate::services::editor::editor_group_contracts::{EditorInputWithOptionsAndGroup, IEditorGroup, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_group_finder::{FindGroupEditor, find_group};
use crate::services::editor::editor_service::I_EDITOR_SERVICE;
use crate::services::editor::editor_resolver_service::{
    DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, EDITORS_ASSOCIATIONS_SETTING_ID, EditorAssociation, EditorAssociations, EditorInputFactoryFunction, EditorInputFactoryObject, EditorMatchRule,
    EditorMatchRuleOrigin, EditorMatches, EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions, EditorResolverServiceGetEditorsOptions,
    GlobPattern, I_EDITOR_RESOLVER_SERVICE, IEditorResolverService, RegisteredEditorInfo, RegisteredEditorOptions, RegisteredEditorPriority, RegisteredEditorRegistrationInfo,
    RegisteredEditorRegistrationPriority, ResolvedEditor, ResolvedStatus, SinglePerResource, glob_matches_resource, is_unconfigured_universal_optional_editor_match,
    priority_to_rank, to_registered_editor_priority_info,
};
use crate::services::editor::extension_service_contract::IExtensionService;

/// `CONFIGURE_DEFAULT_EDITOR_COMMAND_ID` de `openWithEditorPicker.ts`.
pub const CONFIGURE_DEFAULT_EDITOR_COMMAND_ID: &str = "_workbench.editor.configureDefaultEditor";

/// Os argumentos do comando [`CONFIGURE_DEFAULT_EDITOR_COMMAND_ID`] (`untypedEditor`, `currentEditor`, `groupId`).
#[derive(Clone)]
pub struct ConfigureDefaultEditorArgs {
    pub untyped_editor: UntypedEditorInput,
    pub current_editor: EditorInputRef,
    pub group_id: GroupIdentifier,
}

thread_local! {
    static PENDING_CONFIGURE_DEFAULT_EDITOR_ARGS: RefCell<HashMap<u64, ConfigureDefaultEditorArgs>> = RefCell::default();
    static NEXT_CONFIGURE_DEFAULT_EDITOR_TOKEN: Cell<u64> = const { Cell::new(1) };
}

impl ConfigureDefaultEditorArgs {
    /// Guarda os argumentos e devolve o número que os identifica (o único argumento do comando).
    fn stash(self) -> u64 {
        let token = NEXT_CONFIGURE_DEFAULT_EDITOR_TOKEN.with(|next| next.replace(next.get() + 1));
        PENDING_CONFIGURE_DEFAULT_EDITOR_ARGS.with(|pending| pending.borrow_mut().insert(token, self));
        token
    }

    /// Solta os argumentos guardados sob o número.
    fn release(token: u64) {
        PENDING_CONFIGURE_DEFAULT_EDITOR_ARGS.with(|pending| pending.borrow_mut().remove(&token));
    }

    /// Lê os argumentos do comando que o resolvedor executa: `None` quando o primeiro argumento não é um número guardado.
    pub fn from_command_args(args: &[Dynamic]) -> Option<ConfigureDefaultEditorArgs> {
        let Some(Dynamic::Number(token)) = args.first() else {
            return None;
        };
        PENDING_CONFIGURE_DEFAULT_EDITOR_ARGS.with(|pending| pending.borrow().get(&(*token as u64)).cloned())
    }
}

const CACHE_STORAGE_ID: &str = "editorOverrideService.cache";
const CONFLICTING_DEFAULTS_STORAGE_ID: &str = "editorOverrideService.conflictingDefaults";

/// `RegisteredEditor`.
#[derive(Clone)]
struct RegisteredEditor {
    glob_pattern: GlobPattern,
    editor_info: RegisteredEditorInfo,
    options: RegisteredEditorOptions,
    editor_factory_object: EditorInputFactoryObject,
}

/// `RegisteredEditors`: `Array<RegisteredEditor>`.
type RegisteredEditors = Vec<RegisteredEditor>;

/// O que `_flattenedEditors` guarda: um `Map<string | IRelativePattern, RegisteredEditors>` na ordem de inserção.
type FlattenedEditors = Vec<(GlobPattern, RegisteredEditors)>;

/// Um elemento de `Map<string, RegisteredEditors>`: o editor com o número do registro, para o `remove` por identidade.
struct Registration {
    id: u64,
    editor: RegisteredEditor,
}

/// `EditorSelection` sem a variante `Requested`: o `DefaultEditorSelection` do original. `NoEditor` é a seleção sem editor.
#[derive(Clone)]
enum DefaultEditorSelection {
    UserAssociation { editor: RegisteredEditor, association: EditorAssociation },
    EditorPriority { editor: RegisteredEditor, conflicting_default: bool },
    NoEditor,
}

impl DefaultEditorSelection {
    fn editor(&self) -> Option<&RegisteredEditor> {
        match self {
            DefaultEditorSelection::UserAssociation { editor, .. } | DefaultEditorSelection::EditorPriority { editor, .. } => Some(editor),
            DefaultEditorSelection::NoEditor => None,
        }
    }

    fn conflicting_default(&self) -> bool {
        matches!(self, DefaultEditorSelection::EditorPriority { conflicting_default: true, .. })
    }
}

/// `EditorSelection`.
enum EditorSelection {
    /// `EditorSelectionSource.Requested`.
    Requested(RegisteredEditor),
    Default(DefaultEditorSelection),
}

impl EditorSelection {
    /// `{ editor, conflictingDefault }`.
    fn into_parts(self) -> (Option<RegisteredEditor>, bool) {
        match self {
            EditorSelection::Requested(editor) => (Some(editor), false),
            EditorSelection::Default(selection) => {
                let conflicting_default = selection.conflicting_default();
                (selection.editor().cloned(), conflicting_default)
            }
        }
    }
}

/// `normalizeRegisteredEditorInfo`.
fn normalize_registered_editor_info(editor_info: &RegisteredEditorRegistrationInfo) -> RegisteredEditorInfo {
    RegisteredEditorInfo {
        id: editor_info.id.clone(),
        label: editor_info.label.clone(),
        detail: editor_info.detail.clone(),
        priority: to_registered_editor_priority_info(editor_info.priority),
    }
}

/// `EditorAssociationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditorAssociationType {
    Editor,
    DiffEditor,
    MergeEditor,
}

/// `isResourceDiffEditorInput(editor) ? DiffEditor : isResourceMergeEditorInput(editor) ? MergeEditor : Editor`.
fn association_type_of(editor: &UntypedEditorInput) -> EditorAssociationType {
    if editor.is_resource_diff_editor_input() {
        EditorAssociationType::DiffEditor
    } else if editor.is_resource_merge_editor_input() {
        EditorAssociationType::MergeEditor
    } else {
        EditorAssociationType::Editor
    }
}

/// `EditorResourceAccessor.getCanonicalUri(editor, { supportSideBySide })`.
fn canonical_uri(editor: &UntypedEditorInput, side: SideBySideEditor) -> Option<URI> {
    EditorResourceAccessor::get_canonical_uri(
        Some(&EditorInputOrUntyped::Untyped(editor.clone())),
        Some(&EditorResourceAccessorOptions { support_side_by_side: Some(side), filter_by_scheme: None }),
    )
}

/// `URI.from({ scheme: Schemas.untitled })`.
fn untitled_uri() -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), ..UriComponents::default() }, false)
}

/// `DEFAULT_EDITOR_ASSOCIATION.id` como `override` de editor.
fn default_editor_override() -> EditorOverride {
    EditorOverride::Id(JsString::from(DEFAULT_EDITOR_ASSOCIATION_ID))
}

/// `if (override)`: um id não vazio ou uma resolução.
fn override_is_truthy(editor_override: &EditorOverride) -> bool {
    match editor_override {
        EditorOverride::Id(id) => !id.is_empty(),
        EditorOverride::Resolution(_) => true,
    }
}

/// `string.length`: as unidades UTF-16.
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `!editor.options?.canSupportResource || editor.options.canSupportResource(resource)`.
fn can_support_resource(editor: &RegisteredEditor, resource: &URI) -> bool {
    editor.options.can_support_resource.as_ref().is_none_or(|can_support_resource| can_support_resource(resource))
}

/// `mergeEditorAssociationSettings`.
fn merge_editor_association_settings(default_associations: &Dynamic, workspace_associations: &Dynamic, user_associations: &Dynamic) -> EditorAssociations {
    let raw_associations = ObjectRef::new();
    for (key, value) in workspace_associations.own_entries() {
        raw_associations.set(&key, value);
    }
    // We want to apply the default associations and user associations on top of the workspace associations but ignore duplicate keys.
    let defaults_and_user = ObjectRef::new();
    for (key, value) in default_associations.own_entries().into_iter().chain(user_associations.own_entries()) {
        defaults_and_user.set(&key, value);
    }
    for (key, value) in defaults_and_user.entries() {
        if raw_associations.get(&key).is_none_or(|existing| existing.is_undefined()) {
            raw_associations.set(&key, value);
        }
    }
    raw_associations
        .entries()
        .into_iter()
        .map(|(key, value)| EditorAssociation { filename_pattern: Some(key.to_string_lossy()), view_type: view_type_text(&value) })
        .collect()
}

/// O `viewType` de uma entrada do objeto da configuração (um texto; o que não é texto vira o texto do valor).
fn view_type_text(value: &Dynamic) -> String {
    match value {
        Dynamic::String(text) => text.to_string_lossy(),
        other => je_platform::js::to_js_string(other).to_string_lossy(),
    }
}

/// `toEditorAssociationSetting`: `Object.create(null)` com os padrões não vazios, menos o excluído.
fn to_editor_association_setting(associations: &[EditorAssociation], excluded_pattern: Option<&str>) -> ObjectRef {
    let setting_object = ObjectRef::new();
    for association in associations {
        if let Some(pattern) = association.filename_pattern.as_deref()
            && !pattern.is_empty()
            && Some(pattern) != excluded_pattern
        {
            setting_object.set(&JsString::from(pattern), Dynamic::string(&association.view_type));
        }
    }
    setting_object
}

/// O texto guardado do cache (`new Set(JSON.parse(text))`).
fn parse_cache(text: &str) -> Vec<String> {
    let value = parse_json(text).unwrap_or_else(|error| errors::throw(error));
    let JsonValue::Array(items) = value else {
        errors::throw(Error::type_error("object is not iterable (cannot read property Symbol(Symbol.iterator))"));
    };
    let mut cache: Vec<String> = Vec::new();
    for item in items {
        if let JsonValue::String(entry) = item {
            let entry = entry.to_string_lossy();
            if !cache.contains(&entry) {
                cache.push(entry);
            }
        }
    }
    cache
}

/// Os `Set<string>` do original (`add` ignora o repetido).
fn insert_unique(set: &mut Vec<String>, entry: &str) {
    if !set.iter().any(|existing| existing == entry) {
        set.push(entry.to_string());
    }
}

/// O que o resolvedor guarda dos editores registrados.
struct ResolverState {
    /// `_editors`: `Map<string | IRelativePattern, Map<string, RegisteredEditors>>`.
    editors: Vec<(GlobPattern, Vec<(String, Vec<Registration>)>)>,
    /// `_flattenedEditors`.
    flattened_editors: FlattenedEditors,
    /// `_shouldReFlattenEditors`.
    should_re_flatten_editors: bool,
    /// `cache`.
    cache: Option<Vec<String>>,
}

/// Um editor aberto com o grupo em que está.
type ExistingEditor = (EditorInputRef, Rc<dyn IEditorGroup>);

/// `EditorResolverService`.
pub struct EditorResolverService {
    this: Weak<EditorResolverService>,
    base: Rc<Disposable>,

    // Events
    on_did_change_editor_registrations: Rc<PauseableEmitter<()>>,

    // Data Stores
    state: RefCell<ResolverState>,
    next_registration_id: Cell<u64>,

    editor_group_service: Rc<dyn IEditorGroupsService>,
    instantiation_service: Rc<InstantiationService>,
    configuration_service: Rc<dyn IConfigurationService>,
    command_service: Rc<dyn ICommandService>,
    notification_service: Rc<dyn INotificationService>,
    storage_service: Rc<dyn StorageService>,
    extension_service: Rc<dyn IExtensionService>,
    log_service: LogServiceRef,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl Injectable for EditorResolverService {
    type Args = ();
    type Deps = (
        Rc<dyn IEditorGroupsService>,
        Rc<InstantiationService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn ICommandService>,
        Rc<dyn INotificationService>,
        Rc<dyn StorageService>,
        Rc<dyn IExtensionService>,
        LogServiceRef,
        Rc<dyn IUriIdentityService>,
    );

    fn create(
        _: (),
        (
            editor_group_service,
            instantiation_service,
            configuration_service,
            command_service,
            notification_service,
            storage_service,
            extension_service,
            log_service,
            uri_identity_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_editor_registrations = PauseableEmitter::new();
        base.register(on_did_change_editor_registrations.clone());

        // Read in the cache on statup
        let cache = parse_cache(&storage_service.get(CACHE_STORAGE_ID, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_string()));
        storage_service.remove(CACHE_STORAGE_ID, StorageScope::Profile);

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change_editor_registrations,
            state: RefCell::new(ResolverState { editors: Vec::new(), flattened_editors: Vec::new(), should_re_flatten_editors: true, cache: Some(cache) }),
            next_registration_id: Cell::new(0),
            editor_group_service,
            instantiation_service,
            configuration_service,
            command_service,
            notification_service,
            storage_service,
            extension_service,
            log_service,
            uri_identity_service,
        });

        let weak = this.this.clone();
        this.base.register(this.storage_service.on_will_save_state().listen(move |_| {
            // We want to store the glob patterns we would activate on, this allows us to know if we need to await the ext host on startup for opening a resource
            if let Some(this) = weak.upgrade() {
                this.cache_editors();
            }
        }));

        // When extensions have registered we no longer need the cache
        let weak = this.this.clone();
        this.base.register(this.extension_service.on_did_register_extensions().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().cache = None;
            }
        }));

        this
    }
}

service_impl!(EditorResolverService => dyn IEditorResolverService, disposable);

/// `registerSingleton(IEditorResolverService, EditorResolverService, InstantiationType.Eager)`.
pub fn register() {
    register_singleton::<EditorResolverService>(I_EDITOR_RESOLVER_SERVICE, InstantiationType::Eager);
}

impl IDisposable for EditorResolverService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorResolverService {
    /// `resolveUntypedInputAndGroup`.
    fn resolve_untyped_input_and_group(
        &self,
        editor: &UntypedEditorInput,
        preferred_group: Option<&PreferredGroup>,
    ) -> MaybePromise<(UntypedEditorInput, Rc<dyn IEditorGroup>, Option<EditorActivation>)> {
        let untyped_editor = editor.clone();

        // Use the untyped editor to find a group
        let find_group_editor = FindGroupEditor::Untyped(untyped_editor.clone());
        match self.instantiation_service.invoke_function(|accessor| find_group(accessor, &find_group_editor, preferred_group)) {
            MaybePromise::Promise(find_group_result) => {
                MaybePromise::Promise(find_group_result.then(move |(group, activation)| (untyped_editor, group, activation)))
            }
            MaybePromise::Value((group, activation)) => MaybePromise::Value((untyped_editor, group, activation)),
        }
    }

    /// O corpo de `resolveEditor` depois do caso especial do lado a lado.
    async fn resolve_single_editor(self: Rc<Self>, editor: UntypedEditorInput, preferred_group: Option<PreferredGroup>) -> Result<ResolvedEditor, Error> {
        let none = || Ok(ResolvedEditor::Status(ResolvedStatus::None));

        let (mut untyped_editor, group, activation) = match self.resolve_untyped_input_and_group(&editor, preferred_group.as_ref()) {
            MaybePromise::Value(resolved) => resolved,
            MaybePromise::Promise(resolved) => resolved.await?,
        };
        if let Some(activation) = activation {
            let mut options = untyped_editor.options.take().unwrap_or_default();
            options.activation = Some(activation);
            untyped_editor.options = Some(options);
        }

        let resource = canonical_uri(&untyped_editor, SideBySideEditor::Primary);

        // If it was resolved before we await for the extensions to activate and then proceed with resolution or else the backing extensions won't be registered
        let editor_association_type = association_type_of(&untyped_editor);
        let cache_active = self.state.borrow().cache.is_some();
        if cache_active
            && let Some(resource) = &resource
            && (self.resource_matches_cache(resource) || self.resource_matches_user_association(resource, editor_association_type))
        {
            self.extension_service.when_installed_extensions_registered().await?;
        }

        // Undefined resource -> untilted. Other malformed URI's are unresolvable
        let resource = resource.unwrap_or_else(untitled_uri);

        // Resolved the editor ID as much as possible, now find a given editor
        let override_ = untyped_editor.options.as_ref().and_then(|options| options.override_.clone());
        let (selected_editor, mut conflicting_default) = self.get_editor(&resource, override_.as_ref(), editor_association_type).into_parts();
        // If no editor was found and this was a typed editor or an editor with an explicit override we could not resolve it
        let mut selected_editor = match selected_editor {
            Some(selected_editor) => selected_editor,
            None if override_.as_ref().is_some_and(override_is_truthy) => return none(),
            None => {
                // Simple untyped editors that we could not resolve will be resolved to the default editor
                let (resolved_editor, resolved_conflicting_default) =
                    self.get_editor(&resource, Some(&default_editor_override()), editor_association_type).into_parts();
                conflicting_default = resolved_conflicting_default;
                match resolved_editor {
                    Some(selected_editor) => selected_editor,
                    None => return none(),
                }
            }
        };

        // In the special case of diff editors we do some more work to determine the correct editor for both sides
        if untyped_editor.is_resource_diff_editor_input() && override_.is_none() {
            let resource2 = canonical_uri(&untyped_editor, SideBySideEditor::Secondary).unwrap_or_else(untitled_uri);
            let (selected_editor2, _) = self.get_editor(&resource2, None, editor_association_type).into_parts();
            if selected_editor2.is_none_or(|selected_editor2| selected_editor.editor_info.id != selected_editor2.editor_info.id) {
                let (selected_diff, conflicting_default_diff) =
                    self.get_editor(&resource, Some(&default_editor_override()), editor_association_type).into_parts();
                conflicting_default = conflicting_default_diff;
                match selected_diff {
                    Some(selected_diff) => selected_editor = selected_diff,
                    None => return none(),
                }
            }
        }

        // If no override we take the selected editor id so that matches works with the isActive check
        let mut options = untyped_editor.options.take().unwrap_or_default();
        if options.override_.is_none() {
            options.override_ = Some(EditorOverride::Id(JsString::from(selected_editor.editor_info.id.as_str())));
        }
        untyped_editor.options = Some(options);

        // Check if diff can be created based on prescene of factory function
        if selected_editor.editor_factory_object.create_diff_editor_input.is_none() && untyped_editor.is_resource_diff_editor_input() {
            return none();
        }

        let input = self.do_resolve_editor(&untyped_editor, &group, &selected_editor).await?;
        if conflicting_default && let Some(input) = &input {
            // Show the conflicting default dialog
            self.do_handle_conflicting_defaults(&resource, &selected_editor.editor_info.label, &untyped_editor, &input.editor, &group).await?;
        }

        if let Some(input) = input {
            if input.editor.editor_id().as_deref() != Some(selected_editor.editor_info.id.as_str()) {
                self.log_service.warn(&format!(
                    "Editor ID Mismatch: {} !== {}. This will cause bugs. Please ensure editorInput.editorId matches the registered id",
                    input.editor.editor_id().unwrap_or_else(|| "undefined".to_string()),
                    selected_editor.editor_info.id
                ));
            }
            return Ok(ResolvedEditor::Editor(EditorInputWithOptionsAndGroup::new(input, group)));
        }
        Ok(ResolvedEditor::Status(ResolvedStatus::Abort))
    }

    /// `doResolveSideBySideEditor`.
    fn do_resolve_side_by_side_editor(self: &Rc<Self>, editor: &UntypedEditorInput, preferred_group: Option<&PreferredGroup>) -> Promise<ResolvedEditor> {
        let this = self.clone();
        let (editor, preferred_group) = (editor.clone(), preferred_group.cloned());
        async_fn(async move {
            let none = || Ok(ResolvedEditor::Status(ResolvedStatus::None));
            let (Some(primary), Some(secondary)) = (editor.primary.as_deref(), editor.secondary.as_deref()) else {
                return none();
            };

            let ResolvedEditor::Editor(primary_resolved_editor) = this.resolve_editor(primary, preferred_group.as_ref()).await? else {
                return none();
            };
            let primary_group = PreferredGroup::Group(primary_resolved_editor.group.clone());
            let ResolvedEditor::Editor(secondary_resolved_editor) = this.resolve_editor(secondary, Some(&primary_group)).await? else {
                return none();
            };

            let editor_service = this.instantiation_service.invoke_function(|accessor| accessor.get(I_EDITOR_SERVICE));
            let side_by_side_editor: EditorInputRef = SideBySideEditorInput::new(
                editor.label.as_deref(),
                editor.description.as_deref(),
                secondary_resolved_editor.editor,
                primary_resolved_editor.editor,
                editor_service,
            );
            Ok(ResolvedEditor::Editor(EditorInputWithOptionsAndGroup {
                editor: side_by_side_editor,
                options: editor.options.clone(),
                group: primary_resolved_editor.group,
            }))
        })
    }

    /// `_flattenEditorsMap`, junto da atribuição `this._flattenedEditors = ...` de quem o chama: given the nested nature
    /// of the editors map, we merge factories of the same glob and id to make it flat and easier to work with.
    fn flatten_editors_map(&self) {
        let mut state = self.state.borrow_mut();
        // If we shouldn't be re-flattening (due to lack of update) then return early
        if !state.should_re_flatten_editors {
            return;
        }
        state.should_re_flatten_editors = false;
        let mut editors: FlattenedEditors = Vec::new();
        for (glob_pattern, value) in &state.editors {
            let mut registered_editors: RegisteredEditors = Vec::new();
            for (_, editors_with_id) in value {
                let mut registered_editor: Option<RegisteredEditor> = None;
                // Merge all editors with the same id and glob pattern together
                for Registration { editor, .. } in editors_with_id {
                    let registered_editor = registered_editor.get_or_insert_with(|| RegisteredEditor {
                        editor_info: editor.editor_info.clone(),
                        glob_pattern: editor.glob_pattern.clone(),
                        options: RegisteredEditorOptions::default(),
                        editor_factory_object: EditorInputFactoryObject::default(),
                    });
                    // Merge options and factories
                    registered_editor.options = registered_editor.options.merged(&editor.options);
                    registered_editor.editor_factory_object = registered_editor.editor_factory_object.merged(&editor.editor_factory_object);
                }
                if let Some(registered_editor) = registered_editor {
                    registered_editors.push(registered_editor);
                }
            }
            editors.push((glob_pattern.clone(), registered_editors));
        }
        state.flattened_editors = editors;
    }

    /// `_registeredEditors`: all editors as an array. Possible to contain duplicates.
    fn registered_editors(&self) -> RegisteredEditors {
        self.state.borrow().flattened_editors.iter().flat_map(|(_, editors)| editors.iter().cloned()).collect()
    }

    /// `_flattenedEditors`.
    fn flattened_editors(&self) -> FlattenedEditors {
        self.state.borrow().flattened_editors.clone()
    }

    fn create_editor_match_rule(
        &self,
        resource: &URI,
        association_type: EditorAssociationType,
        editor: &RegisteredEditor,
        associations: &[EditorAssociation],
        selection: &DefaultEditorSelection,
    ) -> EditorMatchRule {
        let priority = self.get_effective_priority(&editor.editor_info, association_type);
        if selection.editor().is_none() && editor.editor_info.id == DEFAULT_EDITOR_ASSOCIATION_ID {
            return EditorMatchRule {
                editor: editor.editor_info.clone(),
                priority,
                association_pattern: self.get_default_association_pattern(resource, None),
                origin: EditorMatchRuleOrigin::Fallback,
            };
        }

        if let Some(selected_editor) = selection.editor()
            && selected_editor.editor_info.id == editor.editor_info.id
        {
            match selection {
                DefaultEditorSelection::UserAssociation { association, .. } => {
                    return EditorMatchRule {
                        editor: editor.editor_info.clone(),
                        priority,
                        association_pattern: association
                            .filename_pattern
                            .clone()
                            .unwrap_or_else(|| self.get_default_association_pattern(resource, Some(selected_editor))),
                        origin: EditorMatchRuleOrigin::UserAssociation { association: association.clone() },
                    };
                }
                DefaultEditorSelection::EditorPriority { .. } => {
                    return EditorMatchRule {
                        editor: editor.editor_info.clone(),
                        priority,
                        association_pattern: self.get_default_association_pattern(resource, Some(selected_editor)),
                        origin: EditorMatchRuleOrigin::EditorRegistration { glob_pattern: selected_editor.glob_pattern.clone() },
                    };
                }
                DefaultEditorSelection::NoEditor => {}
            }
        }

        let association = associations.iter().find(|association| association.view_type == editor.editor_info.id);
        if let Some(association) = association
            && priority != RegisteredEditorPriority::Exclusive
        {
            return EditorMatchRule {
                editor: editor.editor_info.clone(),
                priority,
                association_pattern: association.filename_pattern.clone().unwrap_or_else(|| self.get_default_association_pattern(resource, Some(editor))),
                origin: EditorMatchRuleOrigin::UserAssociation { association: association.clone() },
            };
        }

        EditorMatchRule {
            editor: editor.editor_info.clone(),
            priority,
            association_pattern: self.get_default_association_pattern(resource, Some(editor)),
            origin: EditorMatchRuleOrigin::EditorRegistration { glob_pattern: editor.glob_pattern.clone() },
        }
    }

    fn get_default_association_pattern(&self, resource: &URI, selected_editor: Option<&RegisteredEditor>) -> String {
        if let Some(pattern) = selected_editor.and_then(|selected_editor| selected_editor.glob_pattern.as_str())
            && glob_matches_resource(PatternArg::Str(pattern), resource)
        {
            return pattern.to_string();
        }

        format!("*{}", ext_uri().extname(resource))
    }

    fn get_associations_for_resource_by_type(&self, resource: &URI, association_type: EditorAssociationType) -> EditorAssociations {
        if association_type == EditorAssociationType::Editor {
            return self.get_associations_for_resource(resource);
        }

        let mode_associations = self.get_associations_for_resource_from_setting(resource, DIFF_EDITORS_ASSOCIATIONS_SETTING_ID);
        if !mode_associations.is_empty() {
            return mode_associations;
        }

        self.get_associations_for_resource(resource)
            .into_iter()
            .filter(|association| !self.is_explicit_for_association_type(&association.view_type, association_type))
            .collect()
    }

    /// Whether the editor requires an association for the given input kind instead of inheriting one from another input
    /// kind.
    fn is_explicit_for_association_type(&self, view_type: &str, association_type: EditorAssociationType) -> bool {
        let editor = self.registered_editors().into_iter().find(|editor| editor.editor_info.id == view_type);
        editor.is_some_and(|editor| self.get_effective_priority(&editor.editor_info, association_type) == RegisteredEditorPriority::Explicit)
    }

    fn get_associations_for_resource_from_setting(&self, resource: &URI, setting_id: &str) -> EditorAssociations {
        self.get_matching_associations_for_resource(resource, &self.get_all_user_associations_for_setting(setting_id))
    }

    fn get_raw_associations_for_resource_by_type(&self, resource: &URI, association_type: EditorAssociationType) -> EditorAssociations {
        if association_type == EditorAssociationType::Editor {
            return self.get_raw_associations_for_resource_from_setting(resource, EDITORS_ASSOCIATIONS_SETTING_ID);
        }

        let diff_associations = self.get_raw_associations_for_resource_from_setting(resource, DIFF_EDITORS_ASSOCIATIONS_SETTING_ID);
        if !diff_associations.is_empty() { diff_associations } else { self.get_raw_associations_for_resource_from_setting(resource, EDITORS_ASSOCIATIONS_SETTING_ID) }
    }

    fn get_raw_associations_for_resource_from_setting(&self, resource: &URI, setting_id: &str) -> EditorAssociations {
        self.get_matching_raw_associations_for_resource(resource, &self.get_all_user_associations_for_setting(setting_id))
    }

    fn get_matching_associations_for_resource(&self, resource: &URI, associations: &[EditorAssociation]) -> EditorAssociations {
        let matching_associations = self.get_matching_raw_associations_for_resource(resource, associations);
        let registered_editors = self.registered_editors();
        matching_associations.into_iter().filter(|association| registered_editors.iter().any(|editor| editor.editor_info.id == association.view_type)).collect()
    }

    fn get_matching_raw_associations_for_resource(&self, resource: &URI, associations: &[EditorAssociation]) -> EditorAssociations {
        let mut matching_associations: EditorAssociations = associations
            .iter()
            .filter(|association| association.filename_pattern.as_deref().is_some_and(|pattern| !pattern.is_empty() && glob_matches_resource(PatternArg::Str(pattern), resource)))
            .cloned()
            .collect();
        // Sort matching associations based on glob length as a longer glob will be more specific
        sort_by_compare(&mut matching_associations, |a, b| {
            let length = |association: &EditorAssociation| association.filename_pattern.as_deref().map_or(0, js_length) as f64;
            length(b) - length(a)
        });
        matching_associations
    }

    fn get_all_user_associations_for_setting(&self, setting_id: &str) -> EditorAssociations {
        let inspected_editor_associations = self.configuration_service.inspect(setting_id, &ConfigurationOverrides::default());
        merge_editor_association_settings(
            &inspected_editor_associations.default_value(),
            &inspected_editor_associations.workspace_value(),
            &inspected_editor_associations.user_value(),
        )
    }

    fn get_default_editor_id_after_removing_association(
        &self,
        resource: &URI,
        association_type: EditorAssociationType,
        setting_id: &str,
        glob_pattern: &str,
    ) -> Option<String> {
        let remaining_setting_associations = self.get_associations_after_removing_association(setting_id, glob_pattern)?;

        let mut associations = self.get_matching_associations_for_resource(resource, &remaining_setting_associations);
        if association_type == EditorAssociationType::DiffEditor && associations.is_empty() {
            associations = self
                .get_associations_for_resource(resource)
                .into_iter()
                .filter(|association| !self.is_explicit_for_association_type(&association.view_type, association_type))
                .collect();
        }

        let editors = self.find_matching_editors(resource, association_type, Some(&associations));
        let selection = self.get_default_editor_selection(resource, association_type, false, Some(&editors), Some(&associations));
        if selection.conflicting_default() {
            None
        } else {
            Some(selection.editor().map_or_else(|| DEFAULT_EDITOR_ASSOCIATION_ID.to_string(), |editor| editor.editor_info.id.clone()))
        }
    }

    fn get_associations_after_removing_association(&self, setting_id: &str, glob_pattern: &str) -> Option<EditorAssociations> {
        let inspected_associations = self.configuration_service.inspect(setting_id, &ConfigurationOverrides::default());
        let defined = |value: Dynamic| u32::from(!value.is_undefined());

        let explicit_user_target_count = defined(inspected_associations.user_remote_value()) + defined(inspected_associations.user_local_value());
        let user_target_count = if explicit_user_target_count != 0 { explicit_user_target_count } else { defined(inspected_associations.user_value()) };
        let configured_target_count = defined(inspected_associations.workspace_folder_value())
            + defined(inspected_associations.workspace_value())
            + user_target_count
            + defined(inspected_associations.application_value());
        if configured_target_count != 1 {
            return None;
        }

        let updated_setting =
            Dynamic::Object(to_editor_association_setting(&self.get_all_user_associations_for_setting(setting_id), Some(glob_pattern)));
        if !inspected_associations.workspace_value().is_undefined() {
            return Some(merge_editor_association_settings(&inspected_associations.default_value(), &updated_setting, &inspected_associations.user_value()));
        }
        if user_target_count == 1 {
            return Some(merge_editor_association_settings(&inspected_associations.default_value(), &inspected_associations.workspace_value(), &updated_setting));
        }
        None
    }

    fn update_user_associations_for_setting(&self, setting_id: &str, glob_pattern: &str, editor_id: &str) {
        let new_setting_object = to_editor_association_setting(&self.get_all_user_associations_for_setting(setting_id), None);
        new_setting_object.set(&JsString::from(glob_pattern), Dynamic::string(editor_id));
        drop(self.configuration_service.update_value(setting_id, Dynamic::Object(new_setting_object), None, None));
    }

    fn remove_user_association_for_setting(&self, setting_id: &str, glob_pattern: &str) {
        let current_associations = self.get_all_user_associations_for_setting(setting_id);
        if !current_associations.iter().any(|association| association.filename_pattern.as_deref() == Some(glob_pattern)) {
            return;
        }
        drop(self.configuration_service.update_value(
            setting_id,
            Dynamic::Object(to_editor_association_setting(&current_associations, Some(glob_pattern))),
            None,
            None,
        ));
    }

    fn find_matching_editors(&self, resource: &URI, association_type: EditorAssociationType, user_settings: Option<&[EditorAssociation]>) -> RegisteredEditors {
        let default_user_settings;
        let user_settings = match user_settings {
            Some(user_settings) => user_settings,
            None => {
                default_user_settings = self.get_associations_for_resource_by_type(resource, association_type);
                default_user_settings.as_slice()
            }
        };

        // The user setting should be respected even if the editor doesn't specify that resource in package.json
        let mut matching_editors: RegisteredEditors = Vec::new();
        // Then all glob patterns
        for (key, editors) in self.flattened_editors() {
            for editor in editors {
                if association_type == EditorAssociationType::DiffEditor && editor.editor_factory_object.create_diff_editor_input.is_none() {
                    continue;
                }
                if association_type == EditorAssociationType::MergeEditor && editor.editor_factory_object.create_merge_editor_input.is_none() {
                    continue;
                }
                if !can_support_resource(&editor, resource) {
                    continue;
                }

                let found_in_settings = user_settings.iter().any(|setting| setting.view_type == editor.editor_info.id);
                if (found_in_settings && self.get_effective_priority(&editor.editor_info, association_type) != RegisteredEditorPriority::Exclusive)
                    || glob_matches_resource(key.as_arg(), resource)
                {
                    matching_editors.push(editor);
                }
            }
        }
        // Return the editors sorted by their priority
        sort_by_compare(&mut matching_editors, |a, b| {
            let a_priority = self.get_effective_priority(&a.editor_info, association_type);
            let b_priority = self.get_effective_priority(&b.editor_info, association_type);
            // Very crude if priorities match longer glob wins as longer globs are normally more specific
            if priority_to_rank(b_priority) == priority_to_rank(a_priority)
                && let (Some(b_pattern), Some(a_pattern)) = (b.glob_pattern.as_str(), a.glob_pattern.as_str())
            {
                return js_length(b_pattern) as f64 - js_length(a_pattern) as f64;
            }
            f64::from(priority_to_rank(b_priority) - priority_to_rank(a_priority))
        });
        matching_editors
    }

    /// Given a resource and an editorId selects the best possible editor.
    fn get_editor(&self, resource: &URI, editor_id: Option<&EditorOverride>, association_type: EditorAssociationType) -> EditorSelection {
        if let Some(EditorOverride::Id(editor_id)) = editor_id
            && !editor_id.is_empty()
        {
            // Specific id passed in doesn't have to match the resource, it can be anything
            let editor = self.find_editor(resource, association_type, &self.registered_editors(), &editor_id.to_string_lossy());
            return match editor {
                Some(editor) => EditorSelection::Requested(editor),
                None => EditorSelection::Default(DefaultEditorSelection::NoEditor),
            };
        }

        let exclusive_only = matches!(editor_id, Some(EditorOverride::Resolution(EditorResolution::ExclusiveOnly)));
        EditorSelection::Default(self.get_default_editor_selection(resource, association_type, exclusive_only, None, None))
    }

    fn get_default_editor_selection(
        &self,
        resource: &URI,
        association_type: EditorAssociationType,
        exclusive_only: bool,
        editors: Option<&[RegisteredEditor]>,
        associations_from_setting: Option<&[EditorAssociation]>,
    ) -> DefaultEditorSelection {
        let default_editors;
        let editors = match editors {
            Some(editors) => editors,
            None => {
                default_editors = self.find_matching_editors(resource, association_type, None);
                default_editors.as_slice()
            }
        };
        let default_associations;
        let associations_from_setting = match associations_from_setting {
            Some(associations) => associations,
            None => {
                default_associations = self.get_associations_for_resource_by_type(resource, association_type);
                default_associations.as_slice()
            }
        };

        // We only want minPriority+ if no user defined setting is found, else we won't resolve an editor
        let min_priority = if exclusive_only { RegisteredEditorPriority::Exclusive } else { RegisteredEditorPriority::Builtin };
        let mut possible_editors: RegisteredEditors = editors
            .iter()
            .filter(|editor| {
                priority_to_rank(self.get_effective_priority(&editor.editor_info, association_type)) >= priority_to_rank(min_priority)
                    && editor.editor_info.id != DEFAULT_EDITOR_ASSOCIATION_ID
            })
            .cloned()
            .collect();
        if possible_editors.is_empty() {
            let association = if !exclusive_only { associations_from_setting.first() } else { None };
            let editor = association.and_then(|association| self.find_editor(resource, association_type, editors, &association.view_type));
            return match (editor, association) {
                (Some(editor), Some(association)) => DefaultEditorSelection::UserAssociation { editor, association: association.clone() },
                _ => DefaultEditorSelection::NoEditor,
            };
        }
        // If the editor is exclusive we use that, else use the user setting, else we check canSupportResource, else take the viewtype of first possible editor
        let configured_editor = associations_from_setting.first().and_then(|association| self.find_editor(resource, association_type, editors, &association.view_type));
        let exclusive_editor = (self.get_effective_priority(&possible_editors[0].editor_info, association_type) == RegisteredEditorPriority::Exclusive)
            .then(|| possible_editors[0].clone());
        if let Some(configured_editor) = configured_editor
            && exclusive_editor.is_none()
        {
            return DefaultEditorSelection::UserAssociation { editor: configured_editor, association: associations_from_setting[0].clone() };
        }

        let selected_editor = exclusive_editor
            .or_else(|| possible_editors.iter().find(|editor| can_support_resource(editor, resource)).cloned())
            .unwrap_or_else(|| possible_editors[0].clone());

        let mut conflicting_default = false;

        // Filter out exclusive before we check for conflicts as exclusive editors cannot be manually chosen
        // similar to above, need to check canSupportResource if nothing is exclusive
        possible_editors.retain(|editor| self.get_effective_priority(&editor.editor_info, association_type) != RegisteredEditorPriority::Exclusive);
        possible_editors.retain(|editor| can_support_resource(editor, resource));
        if associations_from_setting.is_empty() && possible_editors.len() > 1 {
            conflicting_default = true;
        }

        DefaultEditorSelection::EditorPriority { editor: selected_editor, conflicting_default }
    }

    fn find_editor(&self, resource: &URI, association_type: EditorAssociationType, editors: &[RegisteredEditor], editor_id: &str) -> Option<RegisteredEditor> {
        editors
            .iter()
            .find(|editor| {
                if association_type == EditorAssociationType::DiffEditor && editor.editor_factory_object.create_diff_editor_input.is_none() {
                    return false;
                }
                if association_type == EditorAssociationType::MergeEditor && editor.editor_factory_object.create_merge_editor_input.is_none() {
                    return false;
                }
                editor.editor_info.id == editor_id && can_support_resource(editor, resource)
            })
            .cloned()
    }

    fn get_effective_priority(&self, editor_info: &RegisteredEditorInfo, association_type: EditorAssociationType) -> RegisteredEditorPriority {
        match association_type {
            EditorAssociationType::DiffEditor => editor_info.priority.diff,
            EditorAssociationType::MergeEditor => editor_info.priority.merge,
            EditorAssociationType::Editor => editor_info.priority.editor,
        }
    }

    /// `createEditorInput`/`createDiffEditorInput`/... : roda a fábrica e devolve a entrada com as opções (`options ??
    /// options` do original), ou `None` quando o editor não tem a fábrica.
    async fn create_input(
        factory: Option<EditorInputFactoryFunction>,
        editor: &UntypedEditorInput,
        group: &Rc<dyn IEditorGroup>,
        options: Option<TextEditorOptions>,
    ) -> Result<Option<EditorInputWithOptions>, Error> {
        let Some(factory) = factory else {
            return Ok(None);
        };
        let input_with_options = await_maybe(factory(editor, group)).await?;
        Ok(Some(EditorInputWithOptions { editor: input_with_options.editor, options: input_with_options.options.or(options) }))
    }

    /// `doResolveEditor`.
    fn do_resolve_editor(self: &Rc<Self>, editor: &UntypedEditorInput, group: &Rc<dyn IEditorGroup>, selected_editor: &RegisteredEditor) -> Promise<Option<EditorInputWithOptions>> {
        let this = self.clone();
        let (editor, group, selected_editor) = (editor.clone(), group.clone(), selected_editor.clone());
        async_fn(async move {
            let mut options = editor.options.clone();
            let resource = canonical_uri(&editor, SideBySideEditor::Primary);
            // If no activation option is provided, populate it.
            if let Some(options) = options.as_mut()
                && options.activation.is_none()
                && options.preserve_focus == Some(true)
            {
                options.activation = Some(EditorActivation::Restore);
            }

            let factories = &selected_editor.editor_factory_object;

            // If it's a merge editor we trigger the create merge editor input
            if editor.is_resource_merge_editor_input() {
                return Self::create_input(factories.create_merge_editor_input.clone(), &editor, &group, options).await;
            }

            // If it's a diff editor we trigger the create diff editor input
            if editor.is_resource_diff_editor_input() {
                return Self::create_input(factories.create_diff_editor_input.clone(), &editor, &group, options).await;
            }

            // If it's a diff list editor we trigger the create diff list editor input
            if editor.is_resource_multi_diff_editor_input() {
                return Self::create_input(factories.create_multi_diff_editor_input.clone(), &editor, &group, options).await;
            }

            if editor.is_resource_side_by_side_editor_input() {
                return Err(Error::new("Untyped side by side editor input not supported here."));
            }

            if editor.is_untitled_resource_editor_input() {
                return Self::create_input(factories.create_untitled_editor_input.clone(), &editor, &group, options).await;
            }

            // Should no longer have an undefined resource so lets throw an error if that's somehow the case
            let Some(resource) = resource else {
                return Err(Error::new("Undefined resource on non untitled editor input."));
            };

            // If the editor states it can only be opened once per resource we must close all existing ones except one and move the new one into the group
            let single_editor_per_resource = selected_editor.options.single_per_resource.as_ref().is_some_and(SinglePerResource::resolve);
            if single_editor_per_resource {
                let existing_editors = this.find_existing_editors_for_resource(&resource, &selected_editor.editor_info.id);
                if !existing_editors.is_empty() {
                    return Ok(Self::move_existing_editor_for_resource(existing_editors, group.clone()).await?.map(|editor| EditorInputWithOptions { editor, options }));
                }
            }

            // If no factory is above, return flow back to caller letting them know we could not resolve it
            // Respect options passed back
            Self::create_input(factories.create_editor_input.clone(), &editor, &group, options).await
        })
    }

    /// Moves the first existing editor for a resource to the target group unless already opened there. Additionally will
    /// close any other editors that are open for that resource and viewtype besides the first one found. Resolves to the
    /// moved editor input or `None` if the editor could not be moved.
    fn move_existing_editor_for_resource(existing_editors_for_resource: Vec<ExistingEditor>, target_group: Rc<dyn IEditorGroup>) -> Promise<Option<EditorInputRef>> {
        async_fn(async move {
            let (editor_to_use, group_to_use) = existing_editors_for_resource[0].clone();

            // We should only have one editor but if there are multiple we close the others
            for (editor, group) in &existing_editors_for_resource {
                if !is_same_editor(editor, &editor_to_use) {
                    let closed = group.close_editor(Some(editor)).await?;
                    if !closed {
                        return Ok(None);
                    }
                }
            }

            // Move the editor already opened to the target group
            if target_group.id() != group_to_use.id() {
                let moved = group_to_use.move_editor(&editor_to_use, &target_group);
                if !moved {
                    return Ok(None);
                }
            }

            Ok(Some(editor_to_use))
        })
    }

    /// Given a resource and an editorId, returns all editors open for that resource and editorId.
    fn find_existing_editors_for_resource(&self, resource: &URI, editor_id: &str) -> Vec<ExistingEditor> {
        let mut out: Vec<ExistingEditor> = Vec::new();
        let ordered_groups = distinct(&self.editor_group_service.groups(), id_of);
        let ext_uri = self.uri_identity_service.ext_uri();

        for group in ordered_groups {
            for editor in group.editors() {
                let original_uri = EditorResourceAccessor::get_original_uri(Some(&EditorInputOrUntyped::Typed(editor.clone())), None);
                if (ext_uri.is_equal(editor.resource().as_ref(), Some(resource), false) || ext_uri.is_equal(original_uri.as_ref(), Some(resource), false))
                    && editor.editor_id().as_deref() == Some(editor_id)
                {
                    out.push((editor, group.clone()));
                }
            }
        }
        out
    }

    /// `doHandleConflictingDefaults`.
    fn do_handle_conflicting_defaults(
        self: &Rc<Self>,
        resource: &URI,
        editor_name: &str,
        untyped_input: &UntypedEditorInput,
        current_editor: &EditorInputRef,
        group: &Rc<dyn IEditorGroup>,
    ) -> Promise<()> {
        let this = self.clone();
        let (resource, editor_name, untyped_input, current_editor, group) =
            (resource.clone(), editor_name.to_string(), untyped_input.clone(), current_editor.clone(), group.clone());
        async_fn(async move {
            let association_type = association_type_of(&untyped_input);
            let editors = this.find_matching_editors(&resource, association_type, None);
            let stored_choices_text =
                this.storage_service.get(CONFLICTING_DEFAULTS_STORAGE_ID, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_string());
            let stored_choices = match parse_json(&stored_choices_text).unwrap_or_else(|error| errors::throw(error)) {
                JsonValue::Object(stored_choices) => stored_choices,
                _ => JsonObject::new(),
            };
            let stored_choices = Rc::new(RefCell::new(stored_choices));
            let glob_for_resource = format!("*{}", ext_uri().extname(&resource));
            // Writes to the storage service that a choice has been made for the currently installed editors
            let write_current_editors_to_storage: Rc<dyn Fn()> = {
                let stored_choices = stored_choices.clone();
                let storage_service = this.storage_service.clone();
                let glob_for_resource = glob_for_resource.clone();
                let editor_ids: Vec<JsonValue> = editors.iter().map(|editor| JsonValue::String(JsString::from(editor.editor_info.id.as_str()))).collect();
                Rc::new(move || {
                    stored_choices.borrow_mut().insert(JsString::from(glob_for_resource.as_str()), JsonValue::Array(editor_ids.clone()));
                    let json = stringify_json(&JsonValue::Object(stored_choices.borrow().clone()));
                    storage_service.store(CONFLICTING_DEFAULTS_STORAGE_ID, StorageValue::String(json), StorageScope::Profile, StorageTarget::Machine);
                })
            };

            // If the user has already made a choice for this editor we don't want to ask them again
            let current_editor_id = current_editor.editor_id();
            let already_chosen = matches!(
                stored_choices.borrow().get(&glob_for_resource),
                Some(JsonValue::Array(editor_ids)) if editor_ids.iter().any(|editor_id| matches!(editor_id, JsonValue::String(editor_id) if Some(editor_id.to_string_lossy()) == current_editor_id))
            );
            if already_chosen {
                return Ok(());
            }

            let configure_default: Rc<dyn Fn()> = {
                let command_service = this.command_service.clone();
                let args = ConfigureDefaultEditorArgs { untyped_editor: untyped_input, current_editor, group_id: group.id() };
                Rc::new(move || {
                    let token = args.clone().stash();
                    // O resultado não é lido, como no original.
                    drop(
                        command_service
                            .execute_command(CONFIGURE_DEFAULT_EDITOR_COMMAND_ID, vec![Dynamic::Number(token as f64)])
                            .finally(move || ConfigureDefaultEditorArgs::release(token)),
                    );
                })
            };
            let choice = |label: String, run: Rc<dyn Fn()>| {
                PromptChoiceKind::Plain(PromptChoice { base: BasePromptChoice { telemetry_id: None, label, keep_open: false, run }, is_secondary: false })
            };
            let handle = this.notification_service.prompt(
                Severity::Warning,
                localize("editorResolver.conflictingDefaults", "There are multiple default editors available for the resource.", &[]).into(),
                vec![
                    choice(localize("editorResolver.configureDefault", "Configure Default", &[]), configure_default),
                    choice(localize("editorResolver.keepDefault", "Keep {0}", &[editor_name.as_str()]), write_current_editors_to_storage.clone()),
                ],
                None,
            );
            // If the user pressed X we assume they want to keep the current editor as default
            let on_close_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let subscription = handle.on_did_close().listen({
                let on_close_listener = on_close_listener.clone();
                move |_| {
                    write_current_editors_to_storage();
                    let listener = on_close_listener.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                }
            });
            *on_close_listener.borrow_mut() = Some(subscription);
            Ok(())
        })
    }

    fn cache_editors(&self) {
        // Create a set to store glob patterns
        let mut cache_storage: Vec<String> = Vec::new();

        // Store just the relative pattern pieces without any path info
        for (glob_pattern, contrib_point) in self.flattened_editors() {
            let non_optional = contrib_point
                .iter()
                .any(|contribution| contribution.editor_info.priority.editor != RegisteredEditorPriority::Option && contribution.editor_info.id != DEFAULT_EDITOR_ASSOCIATION_ID);
            // Don't keep a cache of the optional ones as those wouldn't be opened on start anyways
            if !non_optional {
                continue;
            }
            match &glob_pattern {
                GlobPattern::Relative(pattern) => insert_unique(&mut cache_storage, &pattern.pattern),
                GlobPattern::Pattern(pattern) => insert_unique(&mut cache_storage, pattern),
            }
        }

        // Also store the users settings as those would have to activate on startup as well
        let user_associations = self.get_all_user_associations().into_iter().chain(self.get_all_user_associations_for_setting(DIFF_EDITORS_ASSOCIATIONS_SETTING_ID));
        for association in user_associations {
            if let Some(pattern) = association.filename_pattern.as_deref()
                && !pattern.is_empty()
            {
                insert_unique(&mut cache_storage, pattern);
            }
        }
        let cache = JsonValue::Array(cache_storage.iter().map(|pattern| JsonValue::String(JsString::from(pattern.as_str()))).collect());
        self.storage_service.store(CACHE_STORAGE_ID, StorageValue::String(stringify_json(&cache)), StorageScope::Profile, StorageTarget::Machine);
    }

    /// Checks if a resource matches any user-configured editor association that points to a non-default editor. This
    /// ensures that on first startup (when the cache is empty), we still wait for extensions to register before resolving
    /// the editor, so that user-configured custom editors are available.
    fn resource_matches_user_association(&self, resource: &URI, association_type: EditorAssociationType) -> bool {
        self.get_raw_associations_for_resource_by_type(resource, association_type).iter().any(|association| association.view_type != DEFAULT_EDITOR_ASSOCIATION_ID)
    }

    fn resource_matches_cache(&self, resource: &URI) -> bool {
        let state = self.state.borrow();
        let Some(cache) = &state.cache else {
            return false;
        };

        cache.iter().any(|cache_entry| glob_matches_resource(PatternArg::Str(cache_entry), resource))
    }
}

impl IEditorResolverService for EditorResolverService {
    fn get_associations_for_resource(&self, resource: &URI) -> EditorAssociations {
        self.get_associations_for_resource_from_setting(resource, EDITORS_ASSOCIATIONS_SETTING_ID)
    }

    fn get_editor_matches(&self, resource: &URI, options: Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches {
        self.flatten_editors_map();

        let association_type = if options.is_some_and(|options| options.is_diff_editor == Some(true)) { EditorAssociationType::DiffEditor } else { EditorAssociationType::Editor };
        let associations = self.get_associations_for_resource_by_type(resource, association_type);
        let editors = self.find_matching_editors(resource, association_type, Some(&associations));
        let selection = self.get_default_editor_selection(resource, association_type, false, Some(&editors), Some(&associations));
        let natural_editors = self.find_matching_editors(resource, association_type, Some(&[]));
        let natural_selection = self.get_default_editor_selection(resource, association_type, false, Some(&natural_editors), Some(&[]));

        let unique_editors = distinct(&editors, |editor| editor.editor_info.id.clone());
        let default_editor_id = selection.editor().map_or_else(|| DEFAULT_EDITOR_ASSOCIATION_ID.to_string(), |editor| editor.editor_info.id.clone());
        let natural_default_editor_id = natural_selection.editor().map_or_else(|| DEFAULT_EDITOR_ASSOCIATION_ID.to_string(), |editor| editor.editor_info.id.clone());
        let mut matches: Vec<EditorMatchRule> =
            unique_editors.iter().map(|editor| self.create_editor_match_rule(resource, association_type, editor, &associations, &selection)).collect();
        if !matches.iter().any(|match_| match_.editor.id == DEFAULT_EDITOR_ASSOCIATION_ID)
            && (default_editor_id == DEFAULT_EDITOR_ASSOCIATION_ID || natural_default_editor_id == DEFAULT_EDITOR_ASSOCIATION_ID)
        {
            let default_editor = self.registered_editors().into_iter().find(|editor| editor.editor_info.id == DEFAULT_EDITOR_ASSOCIATION_ID);
            let editor_info = default_editor.map(|default_editor| default_editor.editor_info).unwrap_or_else(|| {
                let default_association = default_editor_association();
                RegisteredEditorInfo {
                    id: DEFAULT_EDITOR_ASSOCIATION_ID.to_string(),
                    label: default_association.display_name,
                    detail: Some(default_association.provider_display_name),
                    priority: to_registered_editor_priority_info(RegisteredEditorRegistrationPriority::Single(RegisteredEditorPriority::Builtin)),
                }
            });
            matches.insert(
                0,
                EditorMatchRule {
                    editor: editor_info,
                    priority: RegisteredEditorPriority::Builtin,
                    association_pattern: self.get_default_association_pattern(resource, None),
                    origin: EditorMatchRuleOrigin::Fallback,
                },
            );
        }

        let default_rule_index = matches.iter().position(|match_| match_.editor.id == default_editor_id);
        let natural_default_rule_index = matches.iter().position(|match_| match_.editor.id == natural_default_editor_id);
        let (Some(default_rule_index), Some(natural_default_rule_index)) = (default_rule_index, natural_default_rule_index) else {
            errors::throw(Error::new("The effective and natural default editors must be matching editors."));
        };

        EditorMatches::new(matches, default_rule_index, natural_default_rule_index, selection.conflicting_default())
    }

    fn set_default_editor(&self, resource: &URI, editor_id: &str, for_diff_editor: Option<bool>) {
        let for_diff_editor = for_diff_editor == Some(true);
        let setting_id = if for_diff_editor { DIFF_EDITORS_ASSOCIATIONS_SETTING_ID } else { EDITORS_ASSOCIATIONS_SETTING_ID };
        let association_type = if for_diff_editor { EditorAssociationType::DiffEditor } else { EditorAssociationType::Editor };
        let matches = self.get_editor_matches(resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(for_diff_editor) }));
        let current_association = match &matches.default_rule().origin {
            EditorMatchRuleOrigin::UserAssociation { association } => Some(association),
            _ => None,
        };
        let association_pattern = matches.default_rule().association_pattern.clone();
        if editor_id == matches.natural_default_rule().editor.id {
            if current_association.is_some()
                && self.get_default_editor_id_after_removing_association(resource, association_type, setting_id, &association_pattern).as_deref() == Some(editor_id)
            {
                self.remove_user_association_for_setting(setting_id, &association_pattern);
                return;
            }
            if current_association.is_none() && matches.default_rule().editor.id == editor_id && !matches.conflicting_default() {
                return;
            }
        }

        self.update_user_associations_for_setting(setting_id, &association_pattern, editor_id);
    }

    fn remove_diff_editor_association(&self, resource: &URI, filename_pattern: &str) {
        let matching_diff_association = self
            .get_raw_associations_for_resource_from_setting(resource, DIFF_EDITORS_ASSOCIATIONS_SETTING_ID)
            .into_iter()
            .find(|association| association.filename_pattern.as_deref() == Some(filename_pattern));
        if let Some(pattern) = matching_diff_association.as_ref().and_then(|association| association.filename_pattern.as_deref())
            && !pattern.is_empty()
        {
            self.remove_user_association_for_setting(DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, pattern);
        }
    }

    fn on_did_change_editor_registrations(&self) -> Event<()> {
        self.on_did_change_editor_registrations.event()
    }

    fn buffer_change_events(&self, callback: &dyn Fn()) {
        /// O `finally { resume() }`.
        struct Resume<'a>(&'a PauseableEmitter<()>);

        impl Drop for Resume<'_> {
            fn drop(&mut self) {
                self.0.resume();
            }
        }

        self.on_did_change_editor_registrations.pause();
        let _resume = Resume(&self.on_did_change_editor_registrations);
        callback();
    }

    fn register_editor(
        &self,
        glob_pattern: GlobPattern,
        editor_info: RegisteredEditorRegistrationInfo,
        options: RegisteredEditorOptions,
        editor_factory_object: EditorInputFactoryObject,
    ) -> DisposableRef {
        let registered_editor_info = normalize_registered_editor_info(&editor_info);
        let registration_id = self.next_registration_id.replace(self.next_registration_id.get() + 1);
        {
            let mut state = self.state.borrow_mut();
            let glob_index = match state.editors.iter().position(|(registered_glob, _)| *registered_glob == glob_pattern) {
                Some(index) => index,
                None => {
                    state.editors.push((glob_pattern.clone(), Vec::new()));
                    state.editors.len() - 1
                }
            };
            let registered_editor = &mut state.editors[glob_index].1;
            let id_index = match registered_editor.iter().position(|(id, _)| *id == registered_editor_info.id) {
                Some(index) => index,
                None => {
                    registered_editor.push((registered_editor_info.id.clone(), Vec::new()));
                    registered_editor.len() - 1
                }
            };
            registered_editor[id_index].1.push(Registration {
                id: registration_id,
                editor: RegisteredEditor { glob_pattern: glob_pattern.clone(), editor_info: registered_editor_info.clone(), options, editor_factory_object },
            });
            state.should_re_flatten_editors = true;
        }
        self.on_did_change_editor_registrations.fire(());

        let weak = self.this.clone();
        let editor_id = editor_info.id;
        to_disposable(move || {
            let Some(this) = weak.upgrade() else {
                return;
            };
            {
                let mut state = this.state.borrow_mut();
                let registered_editor = state.editors.iter_mut().find(|(registered_glob, _)| *registered_glob == glob_pattern).map(|(_, registered_editor)| registered_editor);
                if let Some(registered_editor) = registered_editor
                    && let Some(id_index) = registered_editor.iter().position(|(id, _)| *id == editor_id)
                {
                    registered_editor[id_index].1.retain(|registration| registration.id != registration_id);
                    if registered_editor[id_index].1.is_empty() {
                        registered_editor.remove(id_index);
                    }
                }
                state.should_re_flatten_editors = true;
            }
            this.on_did_change_editor_registrations.fire(());
        })
    }

    fn resolve_editor(&self, editor: &UntypedEditorInput, preferred_group: Option<&PreferredGroup>) -> Promise<ResolvedEditor> {
        let this = self.this.upgrade().expect("o resolvedor de editores já foi liberado");
        let (editor, preferred_group) = (editor.clone(), preferred_group.cloned());
        async_fn_maybe(async move {
            // Update the flattened editors
            this.flatten_editors_map();

            // Special case: side by side editors requires us to
            // independently resolve both sides and then build
            // a side by side editor with the result
            if editor.is_resource_side_by_side_editor_input() {
                return Ok(MaybePromise::Promise(this.do_resolve_side_by_side_editor(&editor, preferred_group.as_ref())));
            }

            Ok(MaybePromise::Value(this.resolve_single_editor(editor, preferred_group).await?))
        })
    }

    fn get_editors(&self, resource: &URI, options: Option<&EditorResolverServiceGetEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        self.flatten_editors_map();

        // By resource
        let editor_matches =
            self.get_editor_matches(resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: options.and_then(|options| options.is_diff_editor) }));
        if editor_matches.has_exclusive_match() {
            return Vec::new();
        }
        let exclude_unconfigured = options.is_some_and(|options| options.exclude_unconfigured_universal_optional_editors == Some(true));
        editor_matches
            .matches()
            .iter()
            .filter(|match_| {
                !exclude_unconfigured
                    || !is_unconfigured_universal_optional_editor_match(match_)
                    || options.and_then(|options| options.current_editor_id.as_deref()) == Some(match_.editor.id.as_str())
            })
            .map(|match_| match_.editor.clone())
            .collect()
    }

    fn get_all_editors(&self, options: Option<&EditorResolverServiceGetAllEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        self.flatten_editors_map();

        // All
        let mut editors = self.registered_editors();
        if options.is_some_and(|options| options.exclude_exclusive_editors == Some(true)) {
            editors.retain(|editor| editor.editor_info.priority.editor != RegisteredEditorPriority::Exclusive);
        }
        if options.is_some_and(|options| options.is_diff_editor == Some(true)) {
            editors.retain(|editor| editor.editor_factory_object.create_diff_editor_input.is_some());
        }
        distinct(&editors.into_iter().map(|editor| editor.editor_info).collect::<Vec<_>>(), |editor| editor.id.clone())
    }

    fn get_binary_diff_fallback_editor(&self, resource: &URI) -> Option<String> {
        self.flatten_editors_map();

        // `findMatchingEditors(..., DiffEditor)` only keeps editors that provide a diff editor factory
        // and sorts them by their diff priority. It still includes `explicit` editors (they match by glob),
        // which is exactly what we want here: an `explicit` editor opts out of diffs for text files, but is
        // the better choice than the generic binary fallback when the text diff editor cannot render the
        // content. We exclude the built-in default text editor since that is the editor that already
        // failed to render the binary content.
        self.find_matching_editors(resource, EditorAssociationType::DiffEditor, None)
            .into_iter()
            .find(|editor| editor.editor_info.id != DEFAULT_EDITOR_ASSOCIATION_ID)
            .map(|editor| editor.editor_info.id)
    }

    fn get_all_user_associations(&self) -> EditorAssociations {
        self.get_all_user_associations_for_setting(EDITORS_ASSOCIATIONS_SETTING_ID)
    }
}
