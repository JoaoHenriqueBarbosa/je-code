//! Porte de `src/vs/workbench/contrib/customEditor/common/customEditor.ts`: o contrato `ICustomEditorService`, o do gerente de
//! modelos (`ICustomEditorModelManager`), o do modelo (`ICustomEditorModel`) e as descrições dos editores
//! (`CustomEditorInfo` e `CustomEditorInfoCollection`).
//!
//! Divergências:
//! - **Propriedades** viram métodos (`viewType`, `resource`, `backupId`, `canHotExit`, os eventos), e `_serviceBrand` não
//!   existe. `IReference<ICustomEditorModel>` é a [`Reference`] de `je-base` ([`CustomEditorModelReference`]) e as
//!   promessas do original são [`Promise`].
//! - **`instanceof`.** `model instanceof MainThreadCustomEditorModel` e `instanceof CustomTextEditorModel` são o
//!   `downcast_ref` de [`ICustomEditorModel::as_any`].
//! - **`isReadonly(): boolean | IMarkdownString`** é o [`Readonly`]; `revert` e `saveCustomEditor*` recebem as opções por
//!   valor (`Option`).
//! - **Uniões de string** (`CustomEditorPriority`, `CustomEditorDiffEditorLayout`) são enums com `as_str`. O
//!   `CustomEditorPriorityInfo` (`Omit<RegisteredEditorPriorityInfo, 'merge'>`) é a struct [`CustomEditorPriorityInfo`], só
//!   com `editor` e `diff`. `matches(resource)` lê o `filenamePattern` como texto (`globMatchesResource`).
//! - **`defaultEditor`/`bestAvailableEditor`** são métodos; a identidade `otherEditor === editor` é a posição na lista.
//! - As chaves de contexto `CONTEXT_ACTIVE_CUSTOM_EDITOR_ID` e `CONTEXT_FOCUSED_CUSTOM_EDITOR_IS_EDITABLE` são as funções
//!   [`context_active_custom_editor_id`] e [`context_focused_custom_editor_is_editable`] (o registro preguiçoso de
//!   `common/contextkeys.rs`).

use std::any::Any;
use std::rc::Rc;

use je_base::arrays::distinct;
use je_base::event::Event;
use je_base::glob::PatternArg;
use je_base::lifecycle::{DisposableRef, IDisposable, Reference};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};

use crate::common::editor::{RevertOptions, SaveOptions};
use crate::services::editor::editor_resolver_service::{RegisteredEditorPriority, glob_matches_resource, priority_to_rank};
use crate::services::files_configuration::files_configuration_service::Readonly;

/// `CONTEXT_ACTIVE_CUSTOM_EDITOR_ID`.
pub fn context_active_custom_editor_id() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = RawContextKey::new(
            "activeCustomEditorId",
            "",
            ContextKeyMeta::Info { ty: "string".to_owned(), description: "The viewType of the currently active custom editor.".to_owned() },
        );
    }
    KEY.with(Clone::clone)
}

/// `CONTEXT_FOCUSED_CUSTOM_EDITOR_IS_EDITABLE`.
pub fn context_focused_custom_editor_is_editable() -> RawContextKey {
    thread_local! { static KEY: RawContextKey = RawContextKey::new("focusedCustomEditorIsEditable", false, ContextKeyMeta::Hide); }
    KEY.with(Clone::clone)
}

/// `CustomEditorCapabilities`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CustomEditorCapabilities {
    pub supports_multiple_editors_per_document: Option<bool>,
    pub is_text_editor: Option<bool>,
    pub supports_inline_diff: Option<bool>,
    pub supports_side_by_side_diff: Option<bool>,
}

/// `IReference<ICustomEditorModel>`.
pub type CustomEditorModelReference = Reference<Rc<dyn ICustomEditorModel>>;

/// `ICustomEditorService`.
pub trait ICustomEditorService {
    fn models(&self) -> Rc<dyn ICustomEditorModelManager>;

    fn get_custom_editor(&self, view_type: &str) -> Option<CustomEditorInfo>;
    fn get_all_custom_editors(&self, resource: &URI) -> CustomEditorInfoCollection;
    fn get_contributed_custom_editors(&self, resource: &URI) -> CustomEditorInfoCollection;
    fn get_user_configured_custom_editors(&self, resource: &URI) -> CustomEditorInfoCollection;

    fn register_custom_editor_capabilities(&self, view_type: &str, options: CustomEditorCapabilities) -> DisposableRef;
    fn get_custom_editor_capabilities(&self, view_type: &str) -> Option<CustomEditorCapabilities>;
}

service_id!(pub I_CUSTOM_EDITOR_SERVICE: dyn ICustomEditorService = "customEditorService");

/// `ICustomEditorModelManager`.
pub trait ICustomEditorModelManager {
    fn get_all_models(&self, resource: &URI) -> Promise<Vec<Rc<dyn ICustomEditorModel>>>;

    fn get(&self, resource: &URI, view_type: &str) -> Promise<Option<Rc<dyn ICustomEditorModel>>>;

    fn try_retain(&self, resource: &URI, view_type: &str) -> Option<Promise<CustomEditorModelReference>>;

    fn add(&self, resource: &URI, view_type: &str, model: Promise<Rc<dyn ICustomEditorModel>>) -> Promise<CustomEditorModelReference>;

    fn dispose_all_models_for_view(&self, view_type: &str);

    fn dispose_all_models_for_resource(&self, resource: &URI);
}

/// `ICustomEditorModel`.
pub trait ICustomEditorModel: IDisposable {
    /// Para o `instanceof` das classes concretas do modelo.
    fn as_any(&self) -> &dyn Any;

    fn view_type(&self) -> String;
    fn resource(&self) -> URI;
    fn backup_id(&self) -> Option<String>;
    fn can_hot_exit(&self) -> bool;

    fn is_readonly(&self) -> Readonly;
    fn on_did_change_readonly(&self) -> Event<()>;

    fn is_orphaned(&self) -> bool;
    fn on_did_change_orphaned(&self) -> Event<()>;

    fn is_dirty(&self) -> bool;
    fn on_did_change_dirty(&self) -> Event<()>;

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()>;

    fn save_custom_editor(&self, options: Option<SaveOptions>) -> Promise<Option<URI>>;
    fn save_custom_editor_as(&self, resource: &URI, target_resource: &URI, current_options: Option<SaveOptions>) -> Promise<bool>;
}

/// `CustomEditorPriority`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomEditorPriority {
    Default,
    Builtin,
    Option,
    Explicit,
}

impl CustomEditorPriority {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Builtin => "builtin",
            Self::Option => "option",
            Self::Explicit => "explicit",
        }
    }
}

/// `CustomEditorDiffEditorLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomEditorDiffEditorLayout {
    Inline,
    SideBySide,
}

impl CustomEditorDiffEditorLayout {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::SideBySide => "sideBySide",
        }
    }
}

/// `CustomEditorSelector`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomEditorSelector {
    pub filename_pattern: Option<String>,
}

/// `CustomEditorPriorityInfo`: o `RegisteredEditorPriorityInfo` sem o `merge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomEditorPriorityInfo {
    pub editor: RegisteredEditorPriority,
    pub diff: RegisteredEditorPriority,
}

/// `CustomEditorDescriptor`.
#[derive(Clone, Debug)]
pub struct CustomEditorDescriptor {
    pub id: String,
    pub display_name: String,
    pub provider_display_name: String,
    pub priority: CustomEditorPriorityInfo,
    pub selector: Vec<CustomEditorSelector>,
}

/// `CustomEditorInfo`.
#[derive(Clone, Debug)]
pub struct CustomEditorInfo {
    pub id: String,
    pub display_name: String,
    pub provider_display_name: String,
    pub priority: CustomEditorPriorityInfo,
    pub selector: Vec<CustomEditorSelector>,
}

impl CustomEditorInfo {
    pub fn new(descriptor: CustomEditorDescriptor) -> Self {
        Self {
            id: descriptor.id,
            display_name: descriptor.display_name,
            provider_display_name: descriptor.provider_display_name,
            priority: descriptor.priority,
            selector: descriptor.selector,
        }
    }

    /// `matches(resource)`.
    pub fn matches(&self, resource: &URI) -> bool {
        self.selector.iter().any(|selector| {
            selector.filename_pattern.as_deref().is_some_and(|pattern| !pattern.is_empty() && glob_matches_resource(PatternArg::Str(pattern), resource))
        })
    }
}

/// `CustomEditorInfoCollection`.
#[derive(Clone, Debug)]
pub struct CustomEditorInfoCollection {
    pub all_editors: Vec<CustomEditorInfo>,
}

impl CustomEditorInfoCollection {
    pub fn new(editors: Vec<CustomEditorInfo>) -> Self {
        Self { all_editors: distinct(&editors, |editor| editor.id.clone()) }
    }

    /// `length`.
    pub fn length(&self) -> usize {
        self.all_editors.len()
    }

    /// Find the single default editor to use (if any) by looking at the editor's priority and the other contributed
    /// editors (`defaultEditor`).
    pub fn default_editor(&self) -> Option<&CustomEditorInfo> {
        self.all_editors.iter().enumerate().find_map(|(index, editor)| match editor.priority.editor {
            RegisteredEditorPriority::Default | RegisteredEditorPriority::Builtin => {
                // A default editor must have higher priority than all other contributed editors.
                self.all_editors
                    .iter()
                    .enumerate()
                    .all(|(other_index, other_editor)| other_index == index || is_lower_priority(other_editor, editor))
                    .then_some(editor)
            }
            _ => None,
        })
    }

    /// Find the best available editor to use.
    ///
    /// Unlike the `defaultEditor`, a bestAvailableEditor can exist even if there are other editors with the same priority
    /// (`bestAvailableEditor`).
    pub fn best_available_editor(&self) -> Option<&CustomEditorInfo> {
        let mut editors: Vec<&CustomEditorInfo> = self.all_editors.iter().collect();
        editors.sort_by_key(|editor| priority_to_rank(editor.priority.editor));
        editors.into_iter().next()
    }
}

fn is_lower_priority(other_editor: &CustomEditorInfo, editor: &CustomEditorInfo) -> bool {
    priority_to_rank(other_editor.priority.editor) < priority_to_rank(editor.priority.editor)
}
