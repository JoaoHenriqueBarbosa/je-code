//! Porte de `src/vs/workbench/api/common/extHostProgress.ts`: o lado da extensão de `vscode.window.withProgress`.
//!
//! Divergências:
//! - `ProgressOptions` (de `vscode.d.ts`) é o struct [`ProgressOptions`] com o que o host lê (`location`, `title`,
//!   `cancellable`). Como o tipo não tem `telemetryId` nem outro campo extra, uma extensão não consegue passar um (no
//!   original o TypeScript também recusa, mas em tempo de execução o objeto extra seria simplesmente ignorado).
//! - O `task: (progress, token) => Thenable<R>` é [`ProgressTaskFn`]; `Thenable<R>` é [`Promise`]. Uma exceção lançada
//!   (pânico com [`Error`]) pelo `task` é capturada, vira o `progressEnd` e a rejeição do `withProgress` (que é `async`
//!   no original). O `ProgressLocation.from` que lança vira a rejeição do mesmo jeito.
//! - `@throttle(100, merge, () => Object.create(null))` de `ProgressCallback.throttledReport` é o [`Throttle`] do
//!   `je-base`, com o `IProgressStepDto` vazio como valor inicial.
//! - O `source` de `withProgressFromSource` é `string | INotificationSource` ([`NotificationSourceOrText`]).
//! - `IExtHostRpcService` é o [`IMainContext`] (como nos outros arquivos `extHost*`), e `IExtHostProgress` (o
//!   decorator de serviço) não existe.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::decorators::Throttle;
use je_base::errors::{self, on_unexpected_external_error};
use je_base::promise::Promise;
use je_extensions::scanner::ExtensionDescription;
use je_platform::notification::notification::{NotificationSource, NotificationSourceOrText};
use je_platform::notification::notification_telemetry::NotificationTelemetryId;
use je_platform::progress::progress::{Progress, ProgressCancellable};

use super::ext_host_protocol::{ExtHostProgressShape, IMainContext, IProgressOptionsDto, IProgressStepDto, MainContext, MainThreadProgressShape};
use super::ext_host_type_converters::{ProgressLocation, ProgressLocationArg};

/// `vscode.ProgressOptions`.
#[derive(Clone, Debug)]
pub struct ProgressOptions {
    pub location: ProgressLocationArg,
    pub title: Option<String>,
    pub cancellable: Option<bool>,
}

/// O `task` de `withProgress`: recebe o progresso e o token de cancelamento e devolve a promessa do trabalho.
pub type ProgressTaskFn<R> = Box<dyn FnOnce(Rc<Progress<IProgressStepDto>>, CancellationToken) -> Promise<R>>;

/// `ExtHostProgress`.
pub struct ExtHostProgress {
    proxy: Rc<dyn MainThreadProgressShape>,
    handles: Cell<u32>,
    map_handle_to_cancellation_source: Rc<RefCell<HashMap<u32, CancellationTokenSource>>>,
}

impl ExtHostProgress {
    pub fn new(ext_host_rpc: &dyn IMainContext) -> Rc<Self> {
        Rc::new(Self {
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_PROGRESS),
            handles: Cell::new(0),
            map_handle_to_cancellation_source: Rc::default(),
        })
    }

    fn next_handle(&self) -> u32 {
        let handle = self.handles.get();
        self.handles.set(handle + 1);
        handle
    }

    /// `withProgress(extension, options, task)`.
    pub fn with_progress<R: Clone + 'static>(&self, extension: &ExtensionDescription, options: ProgressOptions, task: ProgressTaskFn<R>) -> Promise<R> {
        let handle = self.next_handle();
        let ProgressOptions { location, title, cancellable } = options;
        let display_name = extension.manifest.display_name.as_deref().filter(|name| !name.is_empty());
        let source = NotificationSource {
            label: display_name.map_or_else(|| extension.manifest.name.clone(), str::to_owned),
            id: extension.identifier.value.clone(),
        };

        let location = match ProgressLocation::from(&location) {
            Ok(location) => location,
            Err(error) => return Promise::rejected(error),
        };
        let mut dto = IProgressOptionsDto::new(location);
        dto.title = title;
        dto.source = Some(NotificationSourceOrText::Source(source));
        dto.cancellable = cancellable.map(ProgressCancellable::Flag);
        let extension_id = (!extension.is_under_development).then_some(extension.identifier.value.as_str());
        self.proxy.rpc_start_progress(handle, dto, extension_id).catch(|error| on_unexpected_external_error(&error));
        self.with_progress_handle(handle, task, cancellable.unwrap_or(false))
    }

    /// `withProgressFromSource(source, options, task, telemetryId)`.
    pub fn with_progress_from_source<R: Clone + 'static>(
        &self,
        source: NotificationSourceOrText,
        options: ProgressOptions,
        task: ProgressTaskFn<R>,
        telemetry_id: Option<NotificationTelemetryId>,
    ) -> Promise<R> {
        let handle = self.next_handle();
        let ProgressOptions { location, title, cancellable } = options;

        let location = match ProgressLocation::from(&location) {
            Ok(location) => location,
            Err(error) => return Promise::rejected(error),
        };
        let mut dto = IProgressOptionsDto::new(location);
        dto.title = title;
        dto.source = Some(source);
        dto.cancellable = cancellable.map(ProgressCancellable::Flag);
        dto.telemetry_id = telemetry_id;
        self.proxy.rpc_start_progress(handle, dto, None).catch(|error| on_unexpected_external_error(&error));
        self.with_progress_handle(handle, task, cancellable.unwrap_or(false))
    }

    /// `_withProgress`.
    fn with_progress_handle<R: Clone + 'static>(&self, handle: u32, task: ProgressTaskFn<R>, cancellable: bool) -> Promise<R> {
        let mut source: Option<CancellationTokenSource> = None;
        if cancellable {
            let created = CancellationTokenSource::new(None);
            self.map_handle_to_cancellation_source.borrow_mut().insert(handle, created.clone());
            source = Some(created);
        }

        let progress_end: Rc<dyn Fn()> = {
            let proxy = self.proxy.clone();
            let map = self.map_handle_to_cancellation_source.clone();
            let source = source.clone();
            Rc::new(move || {
                proxy.rpc_progress_end(handle);
                map.borrow_mut().remove(&handle);
                if let Some(source) = &source {
                    source.dispose_with(false);
                }
            })
        };

        let token = match (&source, cancellable) {
            (Some(source), true) => source.token(),
            _ => CancellationToken::None,
        };
        let progress = progress_callback(self.proxy.clone(), handle);
        let p = match errors::catch(|| task(progress, token)) {
            Ok(p) => p,
            Err(error) => {
                progress_end();
                return Promise::rejected(error);
            }
        };

        let on_settled = progress_end.clone();
        let _ = p.then_result(move |_| {
            on_settled();
            Ok(())
        });
        p
    }
}

impl ExtHostProgressShape for ExtHostProgress {
    /// `$acceptProgressCanceled`.
    fn rpc_accept_progress_canceled(&self, handle: u32) {
        let source = self.map_handle_to_cancellation_source.borrow_mut().remove(&handle);
        if let Some(source) = source {
            source.cancel();
        }
    }
}

/// `mergeProgress`.
fn merge_progress(mut result: IProgressStepDto, current_value: &IProgressStepDto) -> IProgressStepDto {
    result.message.clone_from(&current_value.message);
    if let Some(increment) = current_value.increment {
        result.increment = Some(result.increment.map_or(increment, |total| total + increment));
    }

    result
}

/// `ProgressCallback`: o `Progress` que repassa cada passo ao `$progressReport`, no máximo a cada 100 ms (os passos do
/// intervalo se fundem com [`merge_progress`]).
fn progress_callback(proxy: Rc<dyn MainThreadProgressShape>, handle: u32) -> Rc<Progress<IProgressStepDto>> {
    let throttled_report = Throttle::new(
        100,
        |result: Option<IProgressStepDto>, current_value: IProgressStepDto| merge_progress(result.unwrap_or_default(), &current_value),
        Some(Rc::new(IProgressStepDto::default)),
        move |p: IProgressStepDto| proxy.rpc_progress_report(handle, p),
    );
    Rc::new(Progress::new(move |p| throttled_report.call(p)))
}
