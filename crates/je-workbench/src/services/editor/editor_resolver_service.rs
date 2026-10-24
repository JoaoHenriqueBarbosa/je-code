//! Porte de `src/vs/workbench/services/editor/common/editorResolverService.ts`: os contratos do serviço que resolve uma
//! entrada de editor sem tipo para o editor registrado que a abre (associações do usuário, prioridades, fábricas de
//! entrada), mais as configurações `workbench.editorAssociations`, `workbench.diffEditorAssociations` e
//! `workbench.editor.hiddenEditorTypes`.
//!
//! Divergências:
//! - **Registro da configuração.** `configurationRegistry.registerConfiguration(...)` roda na carga do módulo do original;
//!   aqui é [`register_editor_associations_configuration`], que o arranque do workbench chama uma vez. O
//!   `workbenchConfigurationNodeBase` (`common/configuration.ts`, ainda sem porte próprio) é montado por
//!   `workbench_configuration_node_base`: `id: 'workbench'`, `order: 7`, título e `type: 'object'`. As propriedades
//!   `agentsWindow` e `experiment` ficam no esquema como valores dinâmicos, como as demais chaves de esquema do registro.
//! - **`glob.IRelativePattern | string`** é [`GlobPattern`]. Um padrão relativo é guardado em `Rc` e tem identidade (o
//!   `Map<string | IRelativePattern, ...>` do resolvedor o distingue por referência, e dois objetos iguais são chaves
//!   diferentes); um texto vale por valor.
//! - **Uniões de string** (`RegisteredEditorPriority`) e **`const enum`** (`ResolvedStatus`, `EditorMatchRuleSource`) são
//!   enums. A união discriminada `EditorMatchRule` é [`EditorMatchRule`] com a [`EditorMatchRuleOrigin`] carregando o que
//!   cada `source` tem (`association`, `globPattern`), e [`EditorMatchRule::source`] devolve o discriminante.
//! - **`EditorMatches`** valida os índices e as regras no construtor e lança `RangeError`, como o original. O
//!   `Object.freeze` é a imutabilidade do próprio tipo: os campos são privados e só há leitura.
//! - **`AtLeastOne<EditorInputFactories>`** é restrição de tipo do TypeScript; [`EditorInputFactoryObject`] é a struct com as
//!   cinco fábricas opcionais, que o resolvedor também usa vazia como ponto de partida da fusão (`{}`).
//! - **Funções de fábrica** recebem `&UntypedEditorInput` (as cinco formas `IResource*EditorInput` são a mesma struct) e
//!   o grupo, e devolvem `MaybePromise<EditorInputWithOptions>` (`EditorInputWithOptions | Promise<EditorInputWithOptions>`).
//! - **`getEditors`** tem duas sobrecargas no original (por recurso e geral): são [`IEditorResolverService::get_editors`] e
//!   [`IEditorResolverService::get_all_editors`]. `bufferChangeEvents(callback: Function)` recebe `&dyn Fn()`.
//! - **`IUntypedEditorInput`** é o [`UntypedEditorInput`] do modelo de entradas, e o `resolveEditor` o recebe por
//!   referência (o original o altera no lugar; aqui o resultado traz as opções resolvidas).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::glob::{self, GlobOptions, PatternArg, RelativePattern};
use je_base::lifecycle::DisposableRef;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, PropertySchema, configuration_registry};
use je_platform::resources::ext_uri;

use crate::common::editor::{EditorInputWithOptions, UntypedEditorInput};
use crate::services::editor::editor_groups_service::{EditorInputWithOptionsAndGroup, IEditorGroup, PreferredGroup};

// region: Editor Associations

/// `EditorAssociation`: static values for registered editors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorAssociation {
    pub view_type: String,
    pub filename_pattern: Option<String>,
}

/// `EditorAssociations`.
pub type EditorAssociations = Vec<EditorAssociation>;

pub const EDITORS_ASSOCIATIONS_SETTING_ID: &str = "workbench.editorAssociations";
pub const DIFF_EDITORS_ASSOCIATIONS_SETTING_ID: &str = "workbench.diffEditorAssociations";
pub const HIDDEN_EDITOR_TYPES_SETTING_ID: &str = "workbench.editor.hiddenEditorTypes";

/// Setting that controls whether the Markdown editor is the default editor for `*.md` files in the Agents window. Gated
/// behind an experiment so it can be rolled out gradually. Defaults to on.
pub const MARKDOWN_DEFAULT_EDITOR_AGENTS_WINDOW_SETTING_ID: &str = "workbench.editor.markdownDefaultEditorInAgentsWindow";

fn markdown_editor_agents_window_default(markdown_default_editor: Option<bool>) -> &'static str {
    if markdown_default_editor == Some(true) { "vscode.markdown.editor" } else { "vscode.markdown.preview.editor" }
}

/// As opções de `editorsAssociationsAgentsWindowDefault` (`markdownDefaultEditor`, `integratedBrowserAvailable`) e de
/// `diffEditorsAssociationsAgentsWindowDefault` (só `markdownDefaultEditor`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AgentsWindowDefaultOptions {
    pub markdown_default_editor: Option<bool>,
    pub integrated_browser_available: Option<bool>,
}

/// Builds the default value for `workbench.editorAssociations` in the Agents window. Shared so that dynamic
/// re-registrations of the setting preserve the override.
///
/// Pass `markdown_default_editor: Some(false)` to use the Markdown preview editor for `*.md` files.
pub fn editors_associations_agents_window_default(options: Option<AgentsWindowDefaultOptions>) -> Vec<(String, String)> {
    let mut associations = Vec::new();
    if options.and_then(|options| options.integrated_browser_available).unwrap_or(!Platform::detect().is_web) {
        associations.push(("*.html".to_string(), "workbench.editor.browser".to_string()));
    }
    associations.push(("*.md".to_string(), markdown_editor_agents_window_default(options.and_then(|options| options.markdown_default_editor)).to_string()));
    associations
}

/// `diffEditorsAssociationsAgentsWindowDefault`.
pub fn diff_editors_associations_agents_window_default(options: Option<AgentsWindowDefaultOptions>) -> Vec<(String, String)> {
    vec![("*.md".to_string(), markdown_editor_agents_window_default(options.and_then(|options| options.markdown_default_editor)).to_string())]
}

/// Um `Record<string, string>` como objeto do esquema.
fn associations_to_dynamic(associations: Vec<(String, String)>) -> Dynamic {
    Dynamic::object(associations.into_iter().map(|(pattern, editor)| (pattern, Dynamic::string(&editor))))
}

/// `workbenchConfigurationNodeBase` (`common/configuration.ts`).
fn workbench_configuration_node_base() -> ConfigurationNodeData {
    ConfigurationNodeData {
        id: Some("workbench".to_string()),
        order: Some(7.0),
        title: Some(localize("workbenchConfigurationTitle", "Workbench", &[])),
        type_: Some(Dynamic::string("object")),
        ..ConfigurationNodeData::default()
    }
}

/// `agentsWindow: { default }`.
fn agents_window_default(default: Dynamic) -> Dynamic {
    Dynamic::object([("default", default)])
}

/// `IEditorType`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorType {
    pub id: String,
    pub display_name: String,
    pub provider_display_name: String,
}

/// `configurationRegistry.registerConfiguration(editorAssociationsConfigurationNode)`.
pub fn register_editor_associations_configuration() {
    let markdown_default_editor = PropertySchema::from_entries([
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(true)),
        ("tags", Dynamic::array([Dynamic::string("experimental")])),
        ("experiment", Dynamic::object([("mode", Dynamic::string("startup"))])),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "editor.markdownDefaultEditorInAgentsWindow",
                "Controls whether the Markdown editor is used as the default editor for Markdown files in the Agents window.",
                &[],
            )),
        ),
    ]);

    let editor_associations = PropertySchema::from_entries([
        ("type", Dynamic::string("object")),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "editor.editorAssociations",
                "Configure [glob patterns](https://aka.ms/vscode-glob-patterns) to editors (for example `\"*.hex\": \"hexEditor.hexedit\"`). These have precedence over the default behavior.",
                &[],
            )),
        ),
        ("additionalProperties", Dynamic::object([("type", Dynamic::string("string"))])),
        ("agentsWindow", agents_window_default(associations_to_dynamic(editors_associations_agents_window_default(None)))),
    ]);

    let diff_editor_associations = PropertySchema::from_entries([
        ("type", Dynamic::string("object")),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "editor.diffEditorAssociations",
                "Configure [glob patterns](https://aka.ms/vscode-glob-patterns) to editors for diff views (for example `\"*.md\": \"vscode.markdown.preview.editor\"`). These override `workbench.editorAssociations` for diffs.",
                &[],
            )),
        ),
        ("additionalProperties", Dynamic::object([("type", Dynamic::string("string"))])),
        ("agentsWindow", agents_window_default(associations_to_dynamic(diff_editors_associations_agents_window_default(None)))),
    ]);

    let hidden_editor_types = PropertySchema::from_entries([
        ("type", Dynamic::string("array")),
        ("default", Dynamic::array([])),
        ("items", Dynamic::object([("type", Dynamic::string("string"))])),
        (
            "markdownDescription",
            Dynamic::string(&localize(
                "editor.hiddenEditorTypes",
                "Configure editor types that are hidden from the editor type picker. The active editor type remains visible.",
                &[],
            )),
        ),
        ("agentsWindow", agents_window_default(Dynamic::array([Dynamic::string("vscode.markdown.preview.editor")]))),
    ]);

    configuration_registry().register_configuration(ConfigurationNode::new(workbench_configuration_node_base().with_properties([
        (MARKDOWN_DEFAULT_EDITOR_AGENTS_WINDOW_SETTING_ID, markdown_default_editor),
        (EDITORS_ASSOCIATIONS_SETTING_ID, editor_associations),
        (DIFF_EDITORS_ASSOCIATIONS_SETTING_ID, diff_editor_associations),
        (HIDDEN_EDITOR_TYPES_SETTING_ID, hidden_editor_types),
    ])));
}

// endregion

// region: EditorResolverService types

/// `RegisteredEditorPriority`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegisteredEditorPriority {
    Builtin,
    Option,
    Exclusive,
    Default,
    /// The editor is not automatically used for this kind of input or opted into by an association from another input
    /// kind. It requires an association for this input kind or an explicit open.
    Explicit,
}

impl RegisteredEditorPriority {
    /// O valor de texto do `enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            RegisteredEditorPriority::Builtin => "builtin",
            RegisteredEditorPriority::Option => "option",
            RegisteredEditorPriority::Exclusive => "exclusive",
            RegisteredEditorPriority::Default => "default",
            RegisteredEditorPriority::Explicit => "explicit",
        }
    }
}

/// `ResolvedStatus`: if we didn't resolve an editor dictates what to do with the opening state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedStatus {
    /// Do not continue with opening the editor.
    Abort = 1,
    /// Continue as if the resolution has been disabled as the service could not resolve one.
    None = 2,
}

/// `ResolvedEditor`: `EditorInputWithOptionsAndGroup | ResolvedStatus`.
#[derive(Clone)]
pub enum ResolvedEditor {
    Editor(EditorInputWithOptionsAndGroup),
    Status(ResolvedStatus),
}

/// `glob.IRelativePattern | string`. O texto vale por valor e o padrão relativo por identidade (`Rc`), como as chaves
/// de um `Map` do JavaScript.
#[derive(Clone)]
pub enum GlobPattern {
    Pattern(String),
    Relative(Rc<RelativePattern>),
}

impl GlobPattern {
    /// O padrão como argumento do glob.
    pub fn as_arg(&self) -> PatternArg<'_> {
        match self {
            GlobPattern::Pattern(pattern) => PatternArg::Str(pattern),
            GlobPattern::Relative(pattern) => PatternArg::Relative(pattern),
        }
    }

    /// `typeof globPattern === 'string'`: o texto, ou `None` para um padrão relativo.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            GlobPattern::Pattern(pattern) => Some(pattern),
            GlobPattern::Relative(_) => None,
        }
    }
}

impl PartialEq for GlobPattern {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (GlobPattern::Pattern(a), GlobPattern::Pattern(b)) => a == b,
            (GlobPattern::Relative(a), GlobPattern::Relative(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl From<&str> for GlobPattern {
    fn from(pattern: &str) -> Self {
        GlobPattern::Pattern(pattern.to_string())
    }
}

impl From<RelativePattern> for GlobPattern {
    fn from(pattern: RelativePattern) -> Self {
        GlobPattern::Relative(Rc::new(pattern))
    }
}

/// `singlePerResource?: boolean | (() => boolean)`.
#[derive(Clone)]
pub enum SinglePerResource {
    Bool(bool),
    Function(Rc<dyn Fn() -> bool>),
}

impl SinglePerResource {
    /// `typeof singlePerResource === 'function' ? singlePerResource() : singlePerResource`.
    pub fn resolve(&self) -> bool {
        match self {
            SinglePerResource::Bool(value) => *value,
            SinglePerResource::Function(function) => function(),
        }
    }
}

/// `RegisteredEditorOptions`.
#[derive(Clone, Default)]
pub struct RegisteredEditorOptions {
    /// If your editor cannot be opened in multiple groups for the same resource.
    pub single_per_resource: Option<SinglePerResource>,

    /// Whether or not you can support opening the given resource. If omitted we assume you can open everything.
    pub can_support_resource: Option<Rc<dyn Fn(&URI) -> bool>>,
}

impl RegisteredEditorOptions {
    /// `{ ...self, ...other }`: o que `other` define vence.
    pub fn merged(&self, other: &RegisteredEditorOptions) -> RegisteredEditorOptions {
        RegisteredEditorOptions {
            single_per_resource: other.single_per_resource.clone().or_else(|| self.single_per_resource.clone()),
            can_support_resource: other.can_support_resource.clone().or_else(|| self.can_support_resource.clone()),
        }
    }
}

/// `IEditorResolverServiceGetEditorsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorResolverServiceGetEditorsOptions {
    /// Excludes optional editors registered for `*`, unless they are configured or currently active.
    pub exclude_unconfigured_universal_optional_editors: Option<bool>,
    pub current_editor_id: Option<String>,
    pub is_diff_editor: Option<bool>,
}

/// `IEditorResolverServiceGetEditorMatchesOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorResolverServiceGetEditorMatchesOptions {
    pub is_diff_editor: Option<bool>,
}

/// `IEditorResolverServiceGetAllEditorsOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorResolverServiceGetAllEditorsOptions {
    /// Excludes registrations whose editor priority is exclusive.
    pub exclude_exclusive_editors: Option<bool>,

    /// Excludes registrations that cannot create a diff editor.
    pub is_diff_editor: Option<bool>,
}

/// `EditorMatchRuleSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorMatchRuleSource {
    UserAssociation,
    EditorRegistration,
    Fallback,
}

/// O que cada `source` de uma [`EditorMatchRule`] carrega: a associação do usuário, o padrão do registro, nada.
#[derive(Clone)]
pub enum EditorMatchRuleOrigin {
    UserAssociation { association: EditorAssociation },
    EditorRegistration { glob_pattern: GlobPattern },
    Fallback,
}

/// `EditorMatchRule`: the effective rule that makes one editor choice available for a resource.
#[derive(Clone)]
pub struct EditorMatchRule {
    pub editor: RegisteredEditorInfo,
    pub priority: RegisteredEditorPriority,
    pub association_pattern: String,
    pub origin: EditorMatchRuleOrigin,
}

impl EditorMatchRule {
    /// `source`.
    pub fn source(&self) -> EditorMatchRuleSource {
        match self.origin {
            EditorMatchRuleOrigin::UserAssociation { .. } => EditorMatchRuleSource::UserAssociation,
            EditorMatchRuleOrigin::EditorRegistration { .. } => EditorMatchRuleSource::EditorRegistration,
            EditorMatchRuleOrigin::Fallback => EditorMatchRuleSource::Fallback,
        }
    }
}

/// `isUnconfiguredUniversalOptionalEditorMatch`.
pub fn is_unconfigured_universal_optional_editor_match(rule: &EditorMatchRule) -> bool {
    matches!(&rule.origin, EditorMatchRuleOrigin::EditorRegistration { glob_pattern } if glob_pattern.as_str() == Some("*"))
        && rule.priority == RegisteredEditorPriority::Option
}

/// `new RangeError(message)`.
fn range_error(message: &str) -> ! {
    errors::throw(Error::named("RangeError", message))
}

/// `EditorMatches`: an immutable snapshot containing one effective rule per matching editor and the rule selecting its
/// default.
#[derive(Clone)]
pub struct EditorMatches {
    matches: Vec<EditorMatchRule>,
    default_rule_index: usize,
    natural_default_rule_index: usize,
    conflicting_default: bool,
    has_exclusive_match: bool,
}

impl EditorMatches {
    pub fn new(matches: Vec<EditorMatchRule>, default_rule_index: usize, natural_default_rule_index: usize, conflicting_default: bool) -> Self {
        if default_rule_index >= matches.len() {
            range_error("The default editor rule must be an item in the matches array.");
        }
        if natural_default_rule_index >= matches.len() {
            range_error("The natural default editor rule must be an item in the matches array.");
        }
        let distinct_ids: std::collections::HashSet<&str> = matches.iter().map(|rule| rule.editor.id.as_str()).collect();
        if distinct_ids.len() != matches.len() {
            range_error("Each editor must have exactly one effective match rule.");
        }
        if conflicting_default && matches[default_rule_index].source() != EditorMatchRuleSource::EditorRegistration {
            range_error("Only a registered editor default can conflict with another default.");
        }
        if matches
            .iter()
            .enumerate()
            .any(|(index, rule)| rule.source() == EditorMatchRuleSource::Fallback && index != default_rule_index && index != natural_default_rule_index)
        {
            range_error("A fallback rule must select the effective or natural default editor.");
        }

        let has_exclusive_match = matches.iter().any(|rule| rule.priority == RegisteredEditorPriority::Exclusive);
        Self { matches, default_rule_index, natural_default_rule_index, conflicting_default, has_exclusive_match }
    }

    /// `matches`.
    pub fn matches(&self) -> &[EditorMatchRule] {
        &self.matches
    }

    /// `defaultRuleIndex`.
    pub fn default_rule_index(&self) -> usize {
        self.default_rule_index
    }

    /// `defaultRule`.
    pub fn default_rule(&self) -> &EditorMatchRule {
        &self.matches[self.default_rule_index]
    }

    /// `naturalDefaultRuleIndex`.
    pub fn natural_default_rule_index(&self) -> usize {
        self.natural_default_rule_index
    }

    /// `naturalDefaultRule`.
    pub fn natural_default_rule(&self) -> &EditorMatchRule {
        &self.matches[self.natural_default_rule_index]
    }

    /// `conflictingDefault`.
    pub fn conflicting_default(&self) -> bool {
        self.conflicting_default
    }

    /// `hasExclusiveMatch`.
    pub fn has_exclusive_match(&self) -> bool {
        self.has_exclusive_match
    }
}

/// `RegisteredEditorPriorityInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisteredEditorPriorityInfo {
    pub editor: RegisteredEditorPriority,
    pub diff: RegisteredEditorPriority,
    pub merge: RegisteredEditorPriority,
}

/// `RegisteredEditorPriorityConfiguration`: o `RegisteredEditorPriorityInfo` com o `merge` opcional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisteredEditorPriorityConfiguration {
    pub editor: RegisteredEditorPriority,
    pub diff: RegisteredEditorPriority,
    pub merge: Option<RegisteredEditorPriority>,
}

/// `RegisteredEditorInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredEditorInfo {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub priority: RegisteredEditorPriorityInfo,
}

/// `RegisteredEditorPriority | RegisteredEditorPriorityConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisteredEditorRegistrationPriority {
    Single(RegisteredEditorPriority),
    Configuration(RegisteredEditorPriorityConfiguration),
}

/// `RegisteredEditorRegistrationInfo`: o `RegisteredEditorInfo` com a prioridade em qualquer das duas formas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredEditorRegistrationInfo {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub priority: RegisteredEditorRegistrationPriority,
}

/// `toRegisteredEditorPriorityInfo`.
pub fn to_registered_editor_priority_info(priority: RegisteredEditorRegistrationPriority) -> RegisteredEditorPriorityInfo {
    match priority {
        RegisteredEditorRegistrationPriority::Configuration(priority) => {
            RegisteredEditorPriorityInfo { editor: priority.editor, diff: priority.diff, merge: priority.merge.unwrap_or(priority.editor) }
        }
        RegisteredEditorRegistrationPriority::Single(priority) => RegisteredEditorPriorityInfo { editor: priority, diff: priority, merge: priority },
    }
}

impl From<RegisteredEditorPriority> for RegisteredEditorRegistrationPriority {
    fn from(priority: RegisteredEditorPriority) -> Self {
        RegisteredEditorRegistrationPriority::Single(priority)
    }
}

/// `EditorInputFactoryFunction` e as demais fábricas (`UntitledEditorInputFactoryFunction`,
/// `DiffEditorInputFactoryFunction`, `MultiDiffEditorInputFactoryFunction`, `MergeEditorInputFactoryFunction`): a entrada
/// sem tipo da forma de cada uma e o grupo, para a entrada tipada com opções, já pronta ou por promessa.
pub type EditorInputFactoryFunction = Rc<dyn Fn(&UntypedEditorInput, &Rc<dyn IEditorGroup>) -> MaybePromise<EditorInputWithOptions>>;
pub type UntitledEditorInputFactoryFunction = EditorInputFactoryFunction;
pub type DiffEditorInputFactoryFunction = EditorInputFactoryFunction;
pub type MultiDiffEditorInputFactoryFunction = EditorInputFactoryFunction;
pub type MergeEditorInputFactoryFunction = EditorInputFactoryFunction;

/// `EditorInputFactoryObject`: as fábricas de entrada de um registro (no original `AtLeastOne<EditorInputFactories>`).
#[derive(Clone, Default)]
pub struct EditorInputFactoryObject {
    pub create_editor_input: Option<EditorInputFactoryFunction>,
    pub create_untitled_editor_input: Option<UntitledEditorInputFactoryFunction>,
    pub create_diff_editor_input: Option<DiffEditorInputFactoryFunction>,
    pub create_multi_diff_editor_input: Option<MultiDiffEditorInputFactoryFunction>,
    pub create_merge_editor_input: Option<MergeEditorInputFactoryFunction>,
}

impl EditorInputFactoryObject {
    /// `{ ...self, ...other }`: o que `other` define vence.
    pub fn merged(&self, other: &EditorInputFactoryObject) -> EditorInputFactoryObject {
        EditorInputFactoryObject {
            create_editor_input: other.create_editor_input.clone().or_else(|| self.create_editor_input.clone()),
            create_untitled_editor_input: other.create_untitled_editor_input.clone().or_else(|| self.create_untitled_editor_input.clone()),
            create_diff_editor_input: other.create_diff_editor_input.clone().or_else(|| self.create_diff_editor_input.clone()),
            create_multi_diff_editor_input: other.create_multi_diff_editor_input.clone().or_else(|| self.create_multi_diff_editor_input.clone()),
            create_merge_editor_input: other.create_merge_editor_input.clone().or_else(|| self.create_merge_editor_input.clone()),
        }
    }
}

/// `IEditorResolverService`.
pub trait IEditorResolverService {
    /// Given a resource finds the editor associations that match it from the user's settings.
    fn get_associations_for_resource(&self, resource: &URI) -> EditorAssociations;

    /// Returns an immutable snapshot of the editors matching a resource and the rule selecting its default.
    fn get_editor_matches(&self, resource: &URI, options: Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches;

    /// Sets an editor as the default for a resource, removing a redundant association when restoring the natural default
    /// supplied by editor registrations or the Text Editor fallback.
    ///
    /// `for_diff_editor`: when `true`, the diff editor association (`workbench.diffEditorAssociations`) is updated instead
    /// of the general editor association (`workbench.editorAssociations`).
    fn set_default_editor(&self, resource: &URI, editor_id: &str, for_diff_editor: Option<bool>);

    /// Removes the user's diff editor association (`workbench.diffEditorAssociations`) for the given glob pattern if it
    /// applies to the resource, so that the general editor association also applies to diffs.
    fn remove_diff_editor_association(&self, resource: &URI, filename_pattern: &str);

    /// Emitted when an editor is registered or unregistered.
    fn on_did_change_editor_registrations(&self) -> Event<()>;

    /// Given a callback, run the callback pausing the registration emitter.
    fn buffer_change_events(&self, callback: &dyn Fn());

    /// Registers a specific editor. Editors with the same glob pattern and ID will be grouped together by the resolver.
    /// This allows for registration of the factories in different locations.
    fn register_editor(
        &self,
        glob_pattern: GlobPattern,
        editor_info: RegisteredEditorRegistrationInfo,
        options: RegisteredEditorOptions,
        editor_factory_object: EditorInputFactoryObject,
    ) -> DisposableRef;

    /// Given an editor resolves it to the suitable [`ResolvedEditor`] based on user extensions, settings, and built-in
    /// editors. Returns an `EditorInputWithOptionsAndGroup` if there is an available editor or a status of how to proceed.
    fn resolve_editor(&self, editor: &UntypedEditorInput, preferred_group: Option<&PreferredGroup>) -> Promise<ResolvedEditor>;

    /// Given a resource returns all the editor ids that match that resource. If there is exclusive editor we return an
    /// empty array.
    fn get_editors(&self, resource: &URI, options: Option<&EditorResolverServiceGetEditorsOptions>) -> Vec<RegisteredEditorInfo>;

    /// A set of all the editors that are registered to the editor resolver.
    fn get_all_editors(&self, options: Option<&EditorResolverServiceGetAllEditorsOptions>) -> Vec<RegisteredEditorInfo>;

    /// Returns the id of the best editor that can render a *diff* for the resource, excluding the built-in default text
    /// editor. This intentionally includes editors that opted out of diffs via a `explicit` priority: such editors opt out
    /// for text files, but when the default text diff editor cannot render the content (e.g. it is binary) a custom diff
    /// editor is preferable to the generic "cannot display" fallback. Returns `None` when no such (diff-capable) editor
    /// exists.
    fn get_binary_diff_fallback_editor(&self, resource: &URI) -> Option<String>;

    /// Get a complete list of editor associations.
    fn get_all_user_associations(&self) -> EditorAssociations;
}

service_id!(pub I_EDITOR_RESOLVER_SERVICE: dyn IEditorResolverService = "editorResolverService");

// endregion

// region: Util functions

/// `priorityToRank`.
pub fn priority_to_rank(priority: RegisteredEditorPriority) -> i32 {
    match priority {
        RegisteredEditorPriority::Exclusive => 5,
        RegisteredEditorPriority::Default => 4,
        RegisteredEditorPriority::Builtin => 3,
        // Text editor is priority 2
        RegisteredEditorPriority::Option => 1,
        RegisteredEditorPriority::Explicit => 0,
    }
}

/// `globMatchesResource`.
pub fn glob_matches_resource(glob_pattern: PatternArg<'_>, resource: &URI) -> bool {
    let excluded_schemes = [schemas::EXTENSION, schemas::WEBVIEW_PANEL, schemas::VSCODE_WORKSPACE_TRUST, schemas::VSCODE_SETTINGS];
    // We want to say that the above schemes match no glob patterns
    if excluded_schemes.iter().any(|scheme| *resource.scheme() == *scheme) {
        return false;
    }
    let match_on_path = matches!(glob_pattern, PatternArg::Str(pattern) if pattern.contains('/'));
    let target = if match_on_path { format!("{}:{}", resource.scheme(), resource.path()) } else { ext_uri().basename(resource, None) };
    glob::match_pattern(glob_pattern, &target, &GlobOptions { ignore_case: Some(true), ..GlobOptions::default() })
}

// endregion
