//! Porte de `src/vs/platform/progress/common/progress.ts`: o `IProgressService`, as opções e os passos de progresso,
//! `IProgress` e `Progress`.
//!
//! Divergências:
//! - **Uniões estruturais.** `withProgress` aceita `IProgressOptions | IProgressDialogOptions |
//!   IProgressNotificationOptions | IProgressWindowOptions | IProgressCompositeOptions`, que só diferem por campos
//!   opcionais a mais. [`ProgressOptions`] é a união achatada: todos os campos das cinco interfaces, cada um opcional.
//!   `ProgressLocation | string` é [`ProgressLocationId`] (o `string` é o id de uma visão); `string | NotificationText`
//!   é [`NotificationPart`]; `boolean | string` de `cancellable` é [`ProgressCancellable`].
//! - **Genéricos.** `withProgress<R>(options, task, onDidCancel): Promise<R>` não é objeto-seguro com `R` livre: o único
//!   chamador deste porte (a ponte `MainThreadProgress`) devolve `Promise<void>`, então `R` é `()` (o mesmo recorte de
//!   `je_workbench::services::textfile::collaborators`, que ainda tem a sua cópia reduzida).
//! - `withProgress` lança de forma síncrona com opções inválidas: aqui é um pânico com [`Error`](je_base::errors::Error)
//!   (`errors::throw`), que o chamador captura com `errors::catch`.
//! - `IProgress<T>` é um trait, e `Progress<T>` guarda o último valor e chama o callback (o `callback: (data: T) =>
//!   unknown` do original). `Progress.None` é [`Progress::none`].
//! - NÃO portados (nenhum consumidor ainda): `IProgressIndicator`, `IProgressRunner`, `emptyProgressRunner`, `IOperation`,
//!   `LongRunningOperation` e `UnmanagedProgress`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::promise::Promise;

use crate::notification::notification::{NotificationPriority, NotificationSourceOrText};
use crate::notification::notification_legacy::LegacyExtensionLinkParsing;
use crate::notification::notification_message::NotificationPart;
use crate::notification::notification_telemetry::NotificationTelemetry;

/// `ProgressLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressLocation {
    Explorer = 1,
    Scm = 3,
    Extensions = 5,
    Window = 10,
    Notification = 15,
    Dialog = 20,
}

/// `ProgressLocation | string`: o local conhecido ou o id de uma visão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressLocationId {
    Location(ProgressLocation),
    View(String),
}

impl From<ProgressLocation> for ProgressLocationId {
    fn from(location: ProgressLocation) -> Self {
        ProgressLocationId::Location(location)
    }
}

/// `cancellable?: boolean | string`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressCancellable {
    Flag(bool),
    Label(String),
}

/// `type?: 'loading' | 'syncing'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressType {
    Loading,
    Syncing,
}

/// `IProgressOptions` e as interfaces que a estendem (ver as divergências do módulo).
#[derive(Clone)]
pub struct ProgressOptions {
    pub location: ProgressLocationId,
    /// Used only if this operation is presented as a workbench notification.
    pub telemetry: Option<NotificationTelemetry>,
    pub title: Option<NotificationPart>,
    /// Preserves link parsing for extension API progress. Core callers must use literal text or `NotificationText`.
    pub legacy_extension_link_parsing: Option<LegacyExtensionLinkParsing>,
    pub source: Option<NotificationSourceOrText>,
    pub total: Option<f64>,
    pub cancellable: Option<ProgressCancellable>,
    pub buttons: Option<Vec<String>>,
    /// `IProgressNotificationOptions.primaryActions`.
    pub primary_actions: Option<Vec<ActionRef>>,
    /// `IProgressNotificationOptions.secondaryActions`.
    pub secondary_actions: Option<Vec<ActionRef>>,
    /// `delay` de `IProgressNotificationOptions`, `IProgressDialogOptions` e `IProgressCompositeOptions`.
    pub delay: Option<f64>,
    /// `IProgressNotificationOptions.priority`.
    pub priority: Option<NotificationPriority>,
    /// `type` de `IProgressNotificationOptions` e `IProgressWindowOptions`.
    pub kind: Option<ProgressType>,
    /// `IProgressDialogOptions.detail`.
    pub detail: Option<String>,
    /// `IProgressDialogOptions.sticky`.
    pub sticky: Option<bool>,
    /// `IProgressWindowOptions.command`.
    pub command: Option<String>,
}

impl ProgressOptions {
    /// As opções só com o local, o resto ausente.
    pub fn new(location: impl Into<ProgressLocationId>) -> Self {
        Self {
            location: location.into(),
            telemetry: None,
            title: None,
            legacy_extension_link_parsing: None,
            source: None,
            total: None,
            cancellable: None,
            buttons: None,
            primary_actions: None,
            secondary_actions: None,
            delay: None,
            priority: None,
            kind: None,
            detail: None,
            sticky: None,
            command: None,
        }
    }
}

/// `IProgressStep`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressStep {
    pub message: Option<NotificationPart>,
    pub increment: Option<f64>,
    pub total: Option<f64>,
}

/// `IProgress<T>`.
pub trait IProgress<T> {
    fn report(&self, item: T);
}

/// `Progress<T>`.
pub struct Progress<T: Clone + 'static> {
    value: RefCell<Option<T>>,
    callback: Box<dyn Fn(T)>,
}

impl<T: Clone + 'static> Progress<T> {
    pub fn new(callback: impl Fn(T) + 'static) -> Self {
        Self { value: RefCell::new(None), callback: Box::new(callback) }
    }

    /// `Progress.None`: um progresso que descarta o que recebe.
    pub fn none() -> Rc<dyn IProgress<T>> {
        Rc::new(Self::new(|_| {}))
    }

    /// `get value()`.
    pub fn value(&self) -> Option<T> {
        self.value.borrow().clone()
    }
}

impl<T: Clone + 'static> IProgress<T> for Progress<T> {
    fn report(&self, item: T) {
        *self.value.borrow_mut() = Some(item.clone());
        (self.callback)(item);
    }
}

/// A tarefa de `withProgress`: recebe o `IProgress` e devolve a promessa do trabalho.
pub type ProgressTask = Box<dyn FnOnce(Rc<dyn IProgress<ProgressStep>>) -> Promise<()>>;

/// `IProgressService`: a service that can be used to report progress to various locations of the UI.
pub trait IProgressService {
    /// `withProgress(options, task, onDidCancel)`; o `choice` de `onDidCancel` é o índice do botão escolhido.
    fn with_progress(&self, options: ProgressOptions, task: ProgressTask, on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()>;
}

je_instantiation::service_id!(pub I_PROGRESS_SERVICE: dyn IProgressService = "progressService");
