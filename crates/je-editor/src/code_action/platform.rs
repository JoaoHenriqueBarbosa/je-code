//! Os recortes de `platform` e de `editor/browser/services` que o `codeAction` consome e que o porte ainda não tem
//! por inteiro: `IProgress`/`Progress`, `IProgressRunner`, `IProgressIndicator`, `IEditorProgressService`
//! (`platform/progress/common/progress.ts`), `ActionSet` (`platform/actionWidget/common/actionWidget.ts`),
//! `IBulkEditService` com `IBulkEditOptions` e `IBulkEditResult` (`editor/browser/services/bulkEditService.ts`) e
//! `IAccessibilitySignalService` com os dois sinais que o `codeAction` toca.
//!
//! Cada recorte traz só o que o `codeAction` usa, com a forma do original; quando o módulo inteiro for portado
//! para `je-platform`, estes tipos vão para lá e este arquivo some.
//!
//! Decisões de tradução:
//! - `IProgressIndicator.show` tem duas sobrecargas (`infinite: true` e `total: number`): são [`ProgressTotal`].
//! - `showWhile(promise: Promise<unknown>, delay?)` recebe qualquer promessa: é o [`Thenable`] (o trait que a
//!   promessa do porte já implementa para saber quando assenta).
//! - `Progress.None` é [`ProgressNone`], que aceita qualquer tipo de item.
//! - `ActionSet<T>` é o trait [`ActionSet`], com os cinco membros do original e `IDisposable`.
//! - `IBulkEditService.apply(edit: ResourceEdit[] | WorkspaceEdit, options?)` só tem a forma `WorkspaceEdit`.
//!   `hasPreviewHandler` e `setPreviewHandler` têm corpo padrão (sem tratador), e o `BulkEditService` do workbench os
//!   sobrescreve.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{Promise, Thenable};

use je_base::cancellation::CancellationToken;
use je_platform::progress::progress::{IProgress as PlatformProgress, ProgressStep};

use crate::bulk_edit_service::BulkEditPreviewHandler;
use crate::code_action::languages::WorkspaceEdit;
use crate::code_editor_widget::CodeEditorWidget;
use crate::text_model_edit_source::TextModelEditSource;
use crate::undo_redo::UndoRedoSource;

/// `IProgress<T>`.
pub trait IProgress<T> {
    fn report(&self, item: T);
}

/// `Progress<T>`: guarda o último item e o repassa ao `callback`.
pub struct Progress<T: Clone> {
    callback: Box<dyn Fn(&T)>,
    value: RefCell<Option<T>>,
}

impl<T: Clone> Progress<T> {
    pub fn new(callback: impl Fn(&T) + 'static) -> Self {
        Self { callback: Box::new(callback), value: RefCell::new(None) }
    }

    /// `value`.
    pub fn value(&self) -> Option<T> {
        self.value.borrow().clone()
    }
}

impl<T: Clone> IProgress<T> for Progress<T> {
    fn report(&self, item: T) {
        *self.value.borrow_mut() = Some(item.clone());
        (self.callback)(&item);
    }
}

/// `Progress.None`: ignora o que recebe.
pub struct ProgressNone;

impl<T> IProgress<T> for ProgressNone {
    fn report(&self, _item: T) {}
}

/// `IProgressRunner`.
pub trait IProgressRunner {
    fn total(&self, value: f64);
    fn worked(&self, value: f64);
    fn done(&self);
}

/// As duas sobrecargas de `IProgressIndicator.show`: `infinite: true` ou `total: number`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProgressTotal {
    Infinite,
    Total(f64),
}

/// `IProgressIndicator`.
pub trait IProgressIndicator {
    /// `show(infinite | total, delay?)`.
    fn show(&self, total: ProgressTotal, delay: Option<u64>) -> Rc<dyn IProgressRunner>;

    /// `showWhile(promise, delay?)`.
    fn show_while(&self, promise: &dyn Thenable, delay: Option<u64>) -> Promise<()>;
}

/// `IEditorProgressService`: o indicador de progresso local ao editor.
pub trait IEditorProgressService: IProgressIndicator {}

je_instantiation::service_id!(pub I_EDITOR_PROGRESS_SERVICE: dyn IEditorProgressService = "editorProgressService");

/// `ActionSet<T>`.
pub trait ActionSet<T>: IDisposable {
    fn valid_actions(&self) -> Vec<T>;
    fn all_actions(&self) -> Vec<T>;
    fn has_auto_fix(&self) -> bool;
    fn has_ai_fix(&self) -> bool;
    fn all_ai_fixes(&self) -> bool;
}

/// `IBulkEditOptions` (ver a nota do módulo).
///
/// `progress` é o `IProgress<IProgressStep>` da plataforma (o de `je_platform::progress`, o que o `withProgress`
/// entrega), `undoRedoSource` é um `Rc` porque o [`UndoRedoSource`] é compartilhado por referência e `undoRedoGroupId`
/// é o `id` de um [`crate::undo_redo::UndoRedoGroup`].
#[derive(Clone, Default)]
pub struct BulkEditOptions {
    pub editor: Option<Rc<CodeEditorWidget>>,
    pub progress: Option<Rc<dyn PlatformProgress<ProgressStep>>>,
    pub token: Option<CancellationToken>,
    pub show_preview: Option<bool>,
    pub label: Option<String>,
    pub code: Option<String>,
    pub quotable_label: Option<String>,
    pub undo_redo_source: Option<Rc<UndoRedoSource>>,
    pub undo_redo_group_id: Option<u64>,
    pub confirm_before_undo: Option<bool>,
    pub respect_auto_save_config: Option<bool>,
    pub reason: Option<TextModelEditSource>,
}

/// `IBulkEditResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkEditResult {
    pub aria_summary: String,
    pub is_applied: bool,
}

/// `IBulkEditService` (ver a nota do módulo).
pub trait IBulkEditService {
    fn apply(&self, edit: &WorkspaceEdit, options: BulkEditOptions) -> Promise<BulkEditResult>;

    /// `hasPreviewHandler()`. Sem tratador de pré-visualização (o caso dos dublês) é `false`.
    fn has_preview_handler(&self) -> bool {
        false
    }

    /// `setPreviewHandler(handler)`. Quem não guarda o tratador (o caso dos dublês) devolve um descarte vazio.
    fn set_preview_handler(&self, _handler: BulkEditPreviewHandler) -> DisposableRef {
        je_base::lifecycle::none()
    }
}

je_instantiation::delayed_proxy!(pub BulkEditServiceProxy for dyn IBulkEditService {
    methods {
        fn apply(&self, edit: &WorkspaceEdit, options: BulkEditOptions) -> Promise<BulkEditResult>;
        fn has_preview_handler(&self) -> bool;
        fn set_preview_handler(&self, handler: BulkEditPreviewHandler) -> DisposableRef;
    }
    events {}
});

je_instantiation::service_id!(pub I_BULK_EDIT_SERVICE: dyn IBulkEditService = "IWorkspaceEditService", proxy BulkEditServiceProxy);

/// Os sinais de `AccessibilitySignal` que o `codeAction`, o serviço de estado de execução dos notebooks e o status de terminal
/// das tarefas tocam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilitySignal {
    CodeActionTriggered,
    CodeActionApplied,
    NotebookCellCompleted,
    NotebookCellFailed,
    TaskCompleted,
    TaskFailed,
}

/// `IAccessibilitySignalService`, só com `playSignal`.
pub trait IAccessibilitySignalService {
    fn play_signal(&self, signal: AccessibilitySignal);
}
