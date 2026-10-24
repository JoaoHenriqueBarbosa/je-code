//! Porte parcial de `src/vs/workbench/contrib/files/browser/files.ts`: o `IExplorerService`.
//!
//! O contrato traz só o que o recortar, copiar e colar do explorador chama (`roots`, `getContext`, `getEditable`,
//! `findClosest`, `setToCopy`, `applyBulkEdit` e `select`); o resto (`sortOrderConfiguration`, `hasViewFocus`,
//! `setEditable`, `getEditableData`, `isEditable`, `findClosestRoot`, `refresh`, `isCut`, `registerView`, `getViewId`) e
//! o `IExplorerView` entram com o `ExplorerService` e a vista do explorador. As funções livres do módulo
//! (`getResourceForCommand`, `getMultiSelectedResources`, `getOpenEditorsViewMultiSelection`) entram com os comandos que
//! as usam.
//!
//! O `IEditableData` é de `src/vs/workbench/common/views.ts` e vive aqui até esse módulo ser portado.

use std::rc::Rc;

use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_instantiation::service_id;

use super::explorer_model::ExplorerItem;
use crate::services::textfile::collaborators::ProgressLocation;

/// O `{ content: string; severity: Severity }` de `IEditableData.validationMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationMessage {
    pub content: String,
    pub severity: Severity,
}

/// `IEditableData`.
#[derive(Clone)]
pub struct EditableData {
    pub validation_message: Rc<dyn Fn(&str) -> Option<ValidationMessage>>,
    /// `placeholder?: string | null`.
    pub placeholder: Option<String>,
    /// `startingValue?: string | null`.
    pub starting_value: Option<String>,
    pub on_finish: Rc<dyn Fn(String, bool) -> Promise<()>>,
}

/// O `{ stat: ExplorerItem; data: IEditableData }` de `getEditable()`.
#[derive(Clone)]
pub struct EditableItem {
    pub stat: Rc<ExplorerItem>,
    pub data: EditableData,
}

/// O `options` de `applyBulkEdit`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplorerBulkEditOptions {
    pub undo_label: String,
    pub progress_label: String,
    pub confirm_before_undo: Option<bool>,
    /// `ProgressLocation.Explorer | ProgressLocation.Window`.
    pub progress_location: Option<ProgressLocation>,
}

/// `reveal?: boolean | string` de `select`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplorerReveal {
    Flag(bool),
    Mode(String),
}

/// `IExplorerService` (ver a nota do módulo).
pub trait IExplorerService {
    /// `roots`.
    fn roots(&self) -> Vec<Rc<ExplorerItem>>;

    /// `getContext(respectMultiSelection, ignoreNestedChildren?)`.
    fn get_context(&self, respect_multi_selection: bool, ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>>;

    /// `getEditable()`.
    fn get_editable(&self) -> Option<EditableItem>;

    /// `findClosest(resource)`.
    fn find_closest(&self, resource: &URI) -> Option<Rc<ExplorerItem>>;

    /// `setToCopy(stats, cut)`.
    fn set_to_copy(&self, stats: Vec<Rc<ExplorerItem>>, cut: bool) -> Promise<()>;

    /// `applyBulkEdit(edit, options)`.
    fn apply_bulk_edit(&self, edit: Vec<ResourceFileEdit>, options: ExplorerBulkEditOptions) -> Promise<()>;

    /// Selects and reveal the file element provided by the given resource if its found in the explorer. Will try to
    /// resolve the path in case the explorer is not yet expanded to the file yet.
    fn select(&self, resource: &URI, reveal: Option<ExplorerReveal>) -> Promise<()>;
}

service_id!(pub I_EXPLORER_SERVICE: dyn IExplorerService = "explorerService");
