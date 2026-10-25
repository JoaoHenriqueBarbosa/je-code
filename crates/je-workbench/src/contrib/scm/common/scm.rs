//! Porte de `src/vs/workbench/contrib/scm/common/scm.ts` e do `ISCMHistoryProvider` de `history.ts`: o contrato do serviço
//! de SCM (`ISCMService`), do repositório, do provedor, do grupo, do recurso e da caixa de entrada (`ISCMInput`), com a
//! validação e os tipos de evento. A implementação é o [`scm_service`](super::scm_service).
//!
//! Divergências:
//! - Os `readonly _serviceBrand: undefined` não existem: o identificador do serviço é o [`I_SCM_SERVICE`].
//! - `ISCMService.getRepository(idOrResource: string | URI)` é um par de métodos, já que Rust não tem sobrecarga:
//!   [`ISCMService::get_repository`] (pelo id) e [`ISCMService::get_repository_by_resource`] (pela raiz do repositório).
//!   `repositories: Iterable<ISCMRepository>` é um `Vec` (um instantâneo, na ordem de registro).
//! - `ISCMProvider.rootUri?: URI` é `Option<URI>`; `parentId?: string` e `isHidden?: boolean` são `Option<String>` e `bool`
//!   (o ausente é `false`); `historyProvider: IObservable<ISCMHistoryProvider | undefined>` é um
//!   `IObservable<Option<Rc<dyn ISCMHistoryProvider>>>` (a igualdade do observável é a identidade do `Rc`).
//! - `ISCMResource.multiDiffEditorOriginalUri` e `multiDiffEditorModifiedUri` são `Option<URI>`.
//! - `ISCMHistoryProvider.provideHistoryItemChanges` devolve `Promise<Option<Vec<..>>>` (o `undefined` do original é `None`).
//! - `IInputValidation.message: string | IMarkdownString` é o enum [`InputValidationMessage`]; `IInputValidator` é um
//!   `Rc<dyn Fn(&str, usize) -> Promise<Option<IInputValidation>>>`.
//! - Os pares `get`/`set` de `ISCMInput` (`placeholder`, `enabled`, `visible`, `validateInput`) são métodos
//!   `x()`/`set_x()`, e o `setValue(value, fromKeyboard)` da interface é o `set_value(value, transient, reason)` da classe
//!   `SCMInput`, que é a assinatura que os chamadores do histórico usam.
//! - `ISCMRepository` e `ISCMProvider` estendem `IDisposable`, como no original.
//!
//! Fora do porte, porque dependem de peças que ainda não existem ou não têm leitor nesta fatia:
//! - `ISCMProvider.inputBoxTextModel` (`ITextModel`), `artifactProvider` (`artifact.ts` não foi portado), `actionButton`,
//!   `statusBarCommands`, `acceptInputCommand`, `iconPath`, `activeRepositoryName`, `contextValue`, `count`, `commitTemplate`,
//!   `onDidChangeResources` e `getOriginalResource`, que só o `activity.ts`, a vista e a ponte da extensão leem;
//! - de `ISCMResource`: `resourceGroup`, `decorations` (o tipo [`ISCMResourceDecorations`] está portado), `contextValue`,
//!   `command` e `open`; de `ISCMResourceGroup`: `provider`, `resourceTree`, `contextValue`, `hideWhenEmpty`, `onDidChange`
//!   e `multiDiffEditorEnableViewChanges`;
//! - `ISCMActionButtonDescriptor`, `ISCMActionButton`, `ISCMTitleMenu`, `ISCMRepositoryMenus`, `ISCMMenus` e o
//!   `ISCMViewService` com as chaves de ordenação e de seleção (dependem de `IMenu` ligado ao `ISCMArtifactGroup`).

use std::rc::Rc;

use je_base::event::Event;
use je_base::html_content::IMarkdownString;
use je_base::lifecycle::IDisposable;
use je_base::observable::base::IObservable;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::history::ISCMHistoryItemChange;

pub const VIEWLET_ID: &str = "workbench.view.scm";
pub const VIEW_PANE_ID: &str = "workbench.scm";
pub const REPOSITORIES_VIEW_PANE_ID: &str = "workbench.scm.repositories";
pub const HISTORY_VIEW_PANE_ID: &str = "workbench.scm.history";

/// `ViewMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Tree,
}

impl ViewMode {
    /// O valor de texto do original (`'list'` e `'tree'`).
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::List => "list",
            ViewMode::Tree => "tree",
        }
    }
}

/// `IBaselineResourceProvider`.
pub trait IBaselineResourceProvider {
    fn get_baseline_resource(&self, resource: &URI) -> Promise<URI>;
}

/// `icon?: URI | ThemeIcon` de `ISCMResourceDecorations`.
#[derive(Clone, Debug, PartialEq)]
pub enum ScmResourceIcon {
    Uri(URI),
    ThemeIcon(ThemeIcon),
}

/// `ISCMResourceDecorations`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ISCMResourceDecorations {
    pub icon: Option<ScmResourceIcon>,
    pub icon_dark: Option<ScmResourceIcon>,
    pub tooltip: Option<String>,
    pub strike_through: Option<bool>,
    pub faded: Option<bool>,
}

/// `ISCMHistoryProvider` (só `provideHistoryItemChanges`).
pub trait ISCMHistoryProvider {
    fn provide_history_item_changes(&self, history_item_id: &str, history_item_parent_id: Option<&str>) -> Promise<Option<Vec<ISCMHistoryItemChange>>>;
}

/// `ISCMResource`.
pub trait ISCMResource {
    fn source_uri(&self) -> URI;
    fn multi_diff_editor_original_uri(&self) -> Option<URI> {
        None
    }
    fn multi_diff_editor_modified_uri(&self) -> Option<URI> {
        None
    }
}

/// `ISCMResourceGroup`.
pub trait ISCMResourceGroup {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn resources(&self) -> Vec<Rc<dyn ISCMResource>>;
    fn on_did_change_resources(&self) -> Event<()>;
}

/// `ISCMProvider`.
pub trait ISCMProvider: IDisposable {
    fn id(&self) -> String;
    fn parent_id(&self) -> Option<String> {
        None
    }
    fn provider_id(&self) -> String;
    fn label(&self) -> String;
    fn name(&self) -> String;
    fn groups(&self) -> Vec<Rc<dyn ISCMResourceGroup>>;
    fn on_did_change_resource_groups(&self) -> Event<()>;
    fn root_uri(&self) -> Option<URI>;
    fn is_hidden(&self) -> bool {
        false
    }
    fn history_provider(&self) -> IObservable<Option<Rc<dyn ISCMHistoryProvider>>>;
}

/// `ISCMInputValueProviderContext`.
#[derive(Clone, Debug, PartialEq)]
pub struct ISCMInputValueProviderContext {
    pub resource_group_id: String,
    pub resources: Vec<URI>,
}

/// `InputValidationType` (os valores numéricos são os do original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputValidationType {
    Error = 0,
    Warning = 1,
    Information = 2,
}

/// `message: string | IMarkdownString` de `IInputValidation`.
#[derive(Clone, Debug)]
pub enum InputValidationMessage {
    Text(String),
    Markdown(IMarkdownString),
}

/// `IInputValidation`.
#[derive(Clone, Debug)]
pub struct IInputValidation {
    pub message: InputValidationMessage,
    pub r#type: InputValidationType,
}

/// `IInputValidator`: `(value, cursorPosition) => Promise<IInputValidation | undefined>`.
pub type IInputValidator = Rc<dyn Fn(&str, usize) -> Promise<Option<IInputValidation>>>;

/// `SCMInputChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SCMInputChangeReason {
    HistoryPrevious,
    HistoryNext,
}

/// `ISCMInputChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ISCMInputChangeEvent {
    pub value: String,
    pub reason: Option<SCMInputChangeReason>,
}

/// `ISCMInput`.
pub trait ISCMInput {
    fn repository(&self) -> Rc<dyn ISCMRepository>;

    fn value(&self) -> String;
    /// `setValue(value, transient, reason?)`: `transient` é `true` para o valor que não entra no histórico.
    fn set_value(&self, value: &str, transient: bool, reason: Option<SCMInputChangeReason>);
    fn on_did_change(&self) -> Event<ISCMInputChangeEvent>;

    fn placeholder(&self) -> String;
    fn set_placeholder(&self, placeholder: &str);
    fn on_did_change_placeholder(&self) -> Event<String>;

    fn validate_input(&self) -> IInputValidator;
    fn set_validate_input(&self, validate_input: IInputValidator);
    fn on_did_change_validate_input(&self) -> Event<()>;

    fn enabled(&self) -> bool;
    fn set_enabled(&self, enabled: bool);
    fn on_did_change_enablement(&self) -> Event<bool>;

    fn visible(&self) -> bool;
    fn set_visible(&self, visible: bool);
    fn on_did_change_visibility(&self) -> Event<bool>;

    fn set_focus(&self);
    fn on_did_change_focus(&self) -> Event<()>;

    fn show_validation_message(&self, message: InputValidationMessage, r#type: InputValidationType);
    fn on_did_change_validation_message(&self) -> Event<IInputValidation>;

    fn clear_validation(&self);
    fn on_did_clear_validation(&self) -> Event<()>;

    fn show_next_history_value(&self);
    fn show_previous_history_value(&self);
}

/// `ISCMRepository`.
pub trait ISCMRepository: IDisposable {
    fn id(&self) -> String;
    fn provider(&self) -> Rc<dyn ISCMProvider>;
    fn input(&self) -> Rc<dyn ISCMInput>;
}

/// `ISCMService`.
pub trait ISCMService {
    fn on_did_add_repository(&self) -> Event<Rc<dyn ISCMRepository>>;
    fn on_did_remove_repository(&self) -> Event<Rc<dyn ISCMRepository>>;
    fn repositories(&self) -> Vec<Rc<dyn ISCMRepository>>;
    fn repository_count(&self) -> usize {
        self.repositories().len()
    }

    fn register_scm_provider(&self, provider: Rc<dyn ISCMProvider>) -> Rc<dyn ISCMRepository>;

    /// `getRepository(id: string)`.
    fn get_repository(&self, id: &str) -> Option<Rc<dyn ISCMRepository>>;
    /// `getRepository(resource: URI)`: o repositório visível cuja raiz está mais perto do recurso.
    fn get_repository_by_resource(&self, resource: &URI) -> Option<Rc<dyn ISCMRepository>>;
}

service_id!(pub I_SCM_SERVICE: dyn ISCMService = "scm");
