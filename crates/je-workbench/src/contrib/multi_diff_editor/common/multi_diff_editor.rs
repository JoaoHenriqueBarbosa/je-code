//! Porte de `src/vs/workbench/contrib/multiDiffEditor/common/multiDiffEditor.ts`: as configurações experimentais do editor de
//! diff de vários arquivos (variante e lista de arquivos), as chaves de contexto da lista, e o estado compartilhado (visível,
//! modo de exibição e posição) guardado no armazenamento do perfil.
//!
//! Divergências:
//! - **Registro explícito.** O `Registry.as<IConfigurationRegistry>(...).registerConfiguration(...)` do fim do arquivo roda no
//!   carregamento do módulo; aqui é [`register_multi_diff_editor_configuration`], chamada por quem monta o workbench (e pelos
//!   testes). As chaves de contexto são criadas na primeira chamada de cada função, como em `common/contextkeys.rs`.
//! - **`MultiDiffEditorVariant`** vem de `editor/common/multiDiffEditor.ts` (não portado): o tipo `['cards', 'noCards',
//!   'noCardsNonCompact']` entra aqui como [`MultiDiffEditorVariant`].
//! - **`diffEditor.hideOriginalLineNumbers`** é registrada por `editor/common/config/editorConfigurationSchema.ts` (não
//!   portado). O recorte [`register_hide_original_line_numbers_setting`] registra só essa propriedade, no nó `editor` com o
//!   escopo `LANGUAGE_OVERRIDABLE` do nó-base, e some quando o esquema do editor for portado.
//! - O argumento `owner: object` das funções `observable...` só dá nome ao observável e não entra. `store` é o
//!   `DisposableStore` que recebe a escuta do `onDidChangeValue`.
//! - `observableConfigValue<T>` recebe a conversão do valor dinâmico (ver `platform_observable_utils.rs`). `getBoolean` e `get`
//!   do armazenamento devolvem `Option` (o `undefined` do original).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::DisposableStore;
use je_base::nls::localize;
use je_base::observable::base::IObservable;
use je_base::observable::{DerivedOptions, ObservableExt, StrictEquals, derived_opts, observable_from_event};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::RawContextKey;
use je_platform::observable::platform_observable_utils::observable_config_value;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

pub const MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING: &str = "multiDiffEditor.experimental.variant";
pub const DEFAULT_MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT: MultiDiffEditorExperimentalVariant = MultiDiffEditorExperimentalVariant::NoCards;
pub const MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING: &str = "multiDiffEditor.experimental.fileList";

/// `MultiDiffEditorExperimentalVariant`: `'cards' | 'noCards'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorExperimentalVariant {
    Cards,
    NoCards,
}

impl MultiDiffEditorExperimentalVariant {
    /// O texto do valor da configuração.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cards => "cards",
            Self::NoCards => "noCards",
        }
    }

    /// O valor da configuração como a variante (`None` para outro texto ou outro tipo).
    fn from_dynamic(value: &Dynamic) -> Option<Self> {
        match value {
            Dynamic::String(text) if *text == "cards" => Some(Self::Cards),
            Dynamic::String(text) if *text == "noCards" => Some(Self::NoCards),
            _ => None,
        }
    }
}

impl StrictEquals for MultiDiffEditorExperimentalVariant {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `MultiDiffEditorVariant` (`editor/common/multiDiffEditor.ts`): `'cards' | 'noCards' | 'noCardsNonCompact'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorVariant {
    Cards,
    NoCards,
    NoCardsNonCompact,
}

impl StrictEquals for MultiDiffEditorVariant {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// `getWorkbenchMultiDiffEditorVariant(variant)`.
pub fn get_workbench_multi_diff_editor_variant(variant: MultiDiffEditorExperimentalVariant) -> MultiDiffEditorVariant {
    match variant {
        MultiDiffEditorExperimentalVariant::Cards => MultiDiffEditorVariant::Cards,
        MultiDiffEditorExperimentalVariant::NoCards => MultiDiffEditorVariant::NoCards,
    }
}

/// `observableWorkbenchMultiDiffEditorVariant(owner, configurationService)`.
pub fn observable_workbench_multi_diff_editor_variant(configuration_service: Rc<dyn IConfigurationService>) -> IObservable<MultiDiffEditorVariant> {
    observable_config_value(
        MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING,
        DEFAULT_MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT,
        configuration_service,
        MultiDiffEditorExperimentalVariant::from_dynamic,
    )
    .map(|variant, _| get_workbench_multi_diff_editor_variant(variant))
}

const MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY: &str = "multiDiffEditor.fileListHidden";

fn raw_key(key: &str, default_value: impl Into<Dynamic>, description: String) -> RawContextKey {
    RawContextKey::new(key, default_value, description)
}

/// `MultiDiffEditorFileListAvailableContext`.
pub fn multi_diff_editor_file_list_available_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListAvailable",
            false,
            localize("multiDiffEditorFileListAvailable", "Whether the active multi diff editor supports the changed files list", &[]),
        );
    }
    KEY.with(Clone::clone)
}

/// `MultiDiffEditorFileListEnabledContext`.
pub fn multi_diff_editor_file_list_enabled_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListEnabled",
            false,
            localize("multiDiffEditorFileListEnabled", "Whether the changed files list is enabled in the active multi diff editor", &[]),
        );
    }
    KEY.with(Clone::clone)
}

/// `MultiDiffEditorFileListVisibleContext`: se a lista está desenhada, incluindo o recolhimento responsivo.
pub fn multi_diff_editor_file_list_visible_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListVisible",
            false,
            localize("multiDiffEditorFileListVisible", "Whether the multi diff editor shows the list of changed files", &[]),
        );
    }
    KEY.with(Clone::clone)
}

/// `MultiDiffEditorFileListFocusedContext`.
pub fn multi_diff_editor_file_list_focused_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListFocus",
            true,
            localize("multiDiffEditorFileListFocus", "Whether focus is in the multi diff editor changed files list", &[]),
        );
    }
    KEY.with(Clone::clone)
}

/// `observableMultiDiffEditorFileListVisible(owner, configurationService, storageService, store)`: observa se a lista de arquivos
/// deve aparecer. Todos os editores compartilham o estado, então alternar a lista num editor atualiza os outros.
pub fn observable_multi_diff_editor_file_list_visible(
    configuration_service: Rc<dyn IConfigurationService>,
    storage_service: Rc<dyn StorageService>,
    store: &DisposableStore,
) -> IObservable<bool> {
    let enabled = observable_config_value(MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING, false, configuration_service, |value| match value {
        Dynamic::Bool(enabled) => Some(*enabled),
        _ => None,
    });
    let event = storage_service.on_did_change_value(StorageScope::Profile, Some(MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY), store);
    let hidden = observable_from_event(event, move |_| {
        storage_service.get_boolean(MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY, StorageScope::Profile, Some(false)).unwrap_or(false)
    });
    derived_opts(DerivedOptions::default(), move |reader| enabled.read(reader) && !hidden.read(reader))
}

/// `toggleMultiDiffEditorFileList(storageService)`: mostra a lista se o usuário a escondeu, e esconde se estava à mostra.
pub fn toggle_multi_diff_editor_file_list(storage_service: &dyn StorageService) {
    if storage_service.get_boolean(MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY, StorageScope::Profile, Some(false)).unwrap_or(false) {
        storage_service.remove(MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY, StorageScope::Profile);
    } else {
        storage_service.store(MULTI_DIFF_EDITOR_FILE_LIST_HIDDEN_STORAGE_KEY, true.into(), StorageScope::Profile, StorageTarget::User);
    }
}

/// `MultiDiffEditorFileListViewMode`: `'list' | 'tree'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorFileListViewMode {
    List,
    Tree,
}

impl MultiDiffEditorFileListViewMode {
    /// O texto do modo, o mesmo do valor guardado e da chave de contexto.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Tree => "tree",
        }
    }
}

impl StrictEquals for MultiDiffEditorFileListViewMode {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

const MULTI_DIFF_EDITOR_FILE_LIST_VIEW_MODE_STORAGE_KEY: &str = "multiDiffEditor.fileListViewMode";

/// `MultiDiffEditorFileListViewModeContext`.
pub fn multi_diff_editor_file_list_view_mode_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListViewMode",
            "list",
            localize(
                "multiDiffEditorFileListViewMode",
                "Whether the multi diff editor file list shows the files as a flat list or as a tree of folders",
                &[],
            ),
        );
    }
    KEY.with(Clone::clone)
}

/// `observableMultiDiffEditorFileListViewMode(owner, storageService, store)`: o modo de exibição da lista, compartilhado por
/// todos os editores.
pub fn observable_multi_diff_editor_file_list_view_mode(storage_service: Rc<dyn StorageService>, store: &DisposableStore) -> IObservable<MultiDiffEditorFileListViewMode> {
    let event = storage_service.on_did_change_value(StorageScope::Profile, Some(MULTI_DIFF_EDITOR_FILE_LIST_VIEW_MODE_STORAGE_KEY), store);
    observable_from_event(event, move |_| {
        if storage_service.get(MULTI_DIFF_EDITOR_FILE_LIST_VIEW_MODE_STORAGE_KEY, StorageScope::Profile, None).as_deref() == Some("tree") {
            MultiDiffEditorFileListViewMode::Tree
        } else {
            MultiDiffEditorFileListViewMode::List
        }
    })
}

/// `setMultiDiffEditorFileListViewMode(storageService, viewMode)`.
pub fn set_multi_diff_editor_file_list_view_mode(storage_service: &dyn StorageService, view_mode: MultiDiffEditorFileListViewMode) {
    if view_mode == MultiDiffEditorFileListViewMode::Tree {
        storage_service.store(MULTI_DIFF_EDITOR_FILE_LIST_VIEW_MODE_STORAGE_KEY, view_mode.as_str().into(), StorageScope::Profile, StorageTarget::User);
    } else {
        storage_service.remove(MULTI_DIFF_EDITOR_FILE_LIST_VIEW_MODE_STORAGE_KEY, StorageScope::Profile);
    }
}

/// `MultiDiffEditorFileListPosition`: `'left' | 'right'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiDiffEditorFileListPosition {
    Left,
    Right,
}

impl MultiDiffEditorFileListPosition {
    /// O texto da posição, o mesmo do valor guardado e da chave de contexto.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl StrictEquals for MultiDiffEditorFileListPosition {
    fn strict_equals(&self, other: &Self) -> bool {
        self == other
    }
}

const MULTI_DIFF_EDITOR_FILE_LIST_POSITION_STORAGE_KEY: &str = "multiDiffEditor.fileListPosition";

/// `MultiDiffEditorFileListPositionContext`.
pub fn multi_diff_editor_file_list_position_context() -> RawContextKey {
    thread_local! {
        static KEY: RawContextKey = raw_key(
            "multiDiffEditorFileListPosition",
            "right",
            localize("multiDiffEditorFileListPosition", "The position of the multi diff editor file list", &[]),
        );
    }
    KEY.with(Clone::clone)
}

/// `observableMultiDiffEditorFileListPosition(owner, storageService, store)`: a posição da lista, compartilhada por todos os
/// editores.
pub fn observable_multi_diff_editor_file_list_position(storage_service: Rc<dyn StorageService>, store: &DisposableStore) -> IObservable<MultiDiffEditorFileListPosition> {
    let event = storage_service.on_did_change_value(StorageScope::Profile, Some(MULTI_DIFF_EDITOR_FILE_LIST_POSITION_STORAGE_KEY), store);
    observable_from_event(event, move |_| {
        if storage_service.get(MULTI_DIFF_EDITOR_FILE_LIST_POSITION_STORAGE_KEY, StorageScope::Profile, None).as_deref() == Some("left") {
            MultiDiffEditorFileListPosition::Left
        } else {
            MultiDiffEditorFileListPosition::Right
        }
    })
}

/// `setMultiDiffEditorFileListPosition(storageService, position)`.
pub fn set_multi_diff_editor_file_list_position(storage_service: &dyn StorageService, position: MultiDiffEditorFileListPosition) {
    if position == MultiDiffEditorFileListPosition::Left {
        storage_service.store(MULTI_DIFF_EDITOR_FILE_LIST_POSITION_STORAGE_KEY, position.as_str().into(), StorageScope::Profile, StorageTarget::User);
    } else {
        storage_service.remove(MULTI_DIFF_EDITOR_FILE_LIST_POSITION_STORAGE_KEY, StorageScope::Profile);
    }
}

/// O `registerConfiguration` do fim de `multiDiffEditor.ts`.
pub fn register_multi_diff_editor_configuration() {
    let experimental_tags = || Dynamic::array([Dynamic::string("experimental")]);
    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("multiDiffEditor".to_string()),
            title: Some(localize("multiDiffEditorConfigurationTitle", "Multi Diff Editor", &[])),
            type_: Some(Dynamic::string("object")),
            ..ConfigurationNodeData::default()
        }
        .with_properties([
            (
                MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT_SETTING,
                PropertySchema::from_entries([
                    ("type", Dynamic::string("string")),
                    ("enum", Dynamic::array([Dynamic::string("cards"), Dynamic::string("noCards")])),
                    (
                        "enumDescriptions",
                        Dynamic::array([
                            Dynamic::string(&localize("multiDiffEditor.experimental.variant.cards", "Use the compact card-based variant.", &[])),
                            Dynamic::string(&localize("multiDiffEditor.experimental.variant.noCards", "Use the compact variant without cards.", &[])),
                        ]),
                    ),
                    ("default", Dynamic::string(DEFAULT_MULTI_DIFF_EDITOR_EXPERIMENTAL_VARIANT.as_str())),
                    (
                        "description",
                        Dynamic::string(&localize("multiDiffEditor.experimental.variant", "Controls the experimental variant of the multi diff editor.", &[])),
                    ),
                    ("scope", ConfigurationScope::Window.to_dynamic()),
                    ("tags", experimental_tags()),
                    ("experiment", Dynamic::object([("mode", Dynamic::string("auto"))])),
                ]),
            ),
            (
                MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING,
                PropertySchema::from_entries([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(false)),
                    (
                        "description",
                        Dynamic::string(&localize(
                            "multiDiffEditor.experimental.fileList",
                            "Controls whether newly created multi diff editors support a list of the changed files alongside the diffs. Reload the window to apply this change to existing editors.",
                            &[],
                        )),
                    ),
                    ("scope", ConfigurationScope::Window.to_dynamic()),
                    ("tags", experimental_tags()),
                ]),
            ),
        ]),
    ));
}

/// O recorte de `editorConfigurationSchema.ts`: a propriedade `diffEditor.hideOriginalLineNumbers` (ver o cabeçalho).
pub fn register_hide_original_line_numbers_setting() {
    configuration_registry().register_configuration(ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("editor".to_string()),
            order: Some(5.0),
            type_: Some(Dynamic::string("object")),
            title: Some(localize("editorConfigurationTitle", "Editor", &[])),
            scope: Some(ConfigurationScope::LanguageOverridable),
            ..ConfigurationNodeData::default()
        }
        .with_properties([(
            "diffEditor.hideOriginalLineNumbers",
            PropertySchema::from_entries([
                ("type", Dynamic::array([Dynamic::string("boolean"), Dynamic::string("null")])),
                ("default", Dynamic::Null),
                (
                    "description",
                    Dynamic::string(&localize(
                        "hideOriginalLineNumbers",
                        "Controls whether original line numbers are hidden in inline diffs. When set to null, uses the editor's default: hidden in compact multi diff editors and visible in other diff editors.",
                        &[],
                    )),
                ),
            ]),
        )]),
    ));
}
