//! Teste de unidade do `ExtHostProgress` (`extHostProgress.ts`; o oráculo não tem teste dele, então não é porte) contra um dublê do
//! `MainThreadProgressShape` que registra as chamadas.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_exthost::local_context::LocalContext;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::progress::progress::IProgress;
use je_workbench::api::common::ext_host_progress::{ExtHostProgress, ProgressOptions, ProgressTaskFn};
use je_workbench::api::common::ext_host_protocol::{ExtHostProgressShape, IMainContext, IProgressOptionsDto, IProgressStepDto, MainContext, MainThreadProgressShape};
use je_workbench::api::common::ext_host_type_converters::ProgressLocationArg;

#[derive(Default)]
struct RecordingProgress {
    calls: RefCell<Vec<String>>,
}

impl IDisposable for RecordingProgress {
    fn dispose(&self) {}
}

impl MainThreadProgressShape for RecordingProgress {
    fn rpc_start_progress(&self, handle: u32, options: IProgressOptionsDto, extension_id: Option<&str>) -> Promise<()> {
        self.calls.borrow_mut().push(format!("start {handle} title={:?} cancellable={:?} ext={extension_id:?}", options.title, options.cancellable.is_some()));
        Promise::resolved(())
    }

    fn rpc_progress_report(&self, handle: u32, message: IProgressStepDto) {
        self.calls.borrow_mut().push(format!("report {handle} {:?} {:?}", message.message, message.increment));
    }

    fn rpc_progress_end(&self, handle: u32) {
        self.calls.borrow_mut().push(format!("end {handle}"));
    }
}

fn extension() -> ExtensionDescription {
    ExtensionDescription {
        id: "pub.ext".to_owned(),
        identifier: ExtensionIdentifier::new("pub.ext"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "file:///ext".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest { name: "ext".to_owned(), publisher: "pub".to_owned(), version: "1.0.0".to_owned(), ..ExtensionManifest::default() },
    }
}

fn fixture() -> (Rc<ExtHostProgress>, Rc<RecordingProgress>) {
    let proxy = Rc::new(RecordingProgress::default());
    let context = LocalContext::new();
    let shape: Rc<dyn MainThreadProgressShape> = proxy.clone();
    context.set_erased(MainContext::MAIN_THREAD_PROGRESS.sid, Rc::new(shape));
    let context_ref: &dyn IMainContext = &*context;
    (ExtHostProgress::new(context_ref), proxy)
}

fn notification(title: &str, cancellable: bool) -> ProgressOptions {
    ProgressOptions { location: ProgressLocationArg::Value(15), title: Some(title.to_owned()), cancellable: Some(cancellable) }
}

fn task<R: Clone + 'static>(body: impl FnOnce(Rc<je_platform::progress::progress::Progress<IProgressStepDto>>, CancellationToken) -> Promise<R> + 'static) -> ProgressTaskFn<R> {
    Box::new(body)
}

fn pump() {
    for _ in 0..16 {
        je_base::runtime::turn();
    }
}

#[test]
fn with_progress_starts_reports_and_ends_when_the_task_settles() {
    let (progress, proxy) = fixture();
    let (work, resolver) = Promise::<u32>::new();
    let result = progress.with_progress(
        &extension(),
        notification("Trabalhando", false),
        task(move |report, _token| {
            report.report(IProgressStepDto { message: Some("metade".to_owned()), increment: Some(50.0), total: None });
            work
        }),
    );
    pump();
    assert_eq!(proxy.calls.borrow()[0], "start 0 title=Some(\"Trabalhando\") cancellable=true ext=Some(\"pub.ext\")");
    assert!(proxy.calls.borrow().iter().any(|call| call == "report 0 Some(\"metade\") Some(50.0)"));
    assert!(!proxy.calls.borrow().iter().any(|call| call.starts_with("end")), "a tarefa ainda não terminou");

    resolver.resolve(7);
    pump();
    assert_eq!(result.result().expect("assentou").expect("sem erro"), 7);
    assert_eq!(proxy.calls.borrow().last().expect("chamou"), "end 0");
}

#[test]
fn a_failing_task_still_ends_the_progress() {
    let (progress, proxy) = fixture();
    let result = progress.with_progress(&extension(), notification("x", false), task(|_, _| Promise::<u32>::rejected(Error::new("falhou"))));
    pump();
    assert_eq!(result.result().expect("assentou").expect_err("rejeita").message, "falhou");
    assert_eq!(proxy.calls.borrow().last().expect("chamou"), "end 0");
}

#[test]
fn handles_increase_per_call_and_cancel_reaches_the_token_of_a_cancellable_task() {
    let (progress, proxy) = fixture();
    let seen_token: Rc<RefCell<Option<CancellationToken>>> = Rc::default();
    let sink = seen_token.clone();
    let (first_work, first_resolver) = Promise::<()>::new();
    let _ = progress.with_progress(
        &extension(),
        notification("a", true),
        task(move |_, token| {
            *sink.borrow_mut() = Some(token);
            first_work
        }),
    );
    let _ = progress.with_progress(&extension(), notification("b", false), task(|_, _| Promise::resolved(())));
    pump();
    assert!(proxy.calls.borrow().iter().any(|call| call.starts_with("start 1 title=Some(\"b\")")));

    let token = seen_token.borrow().clone().expect("a tarefa recebeu um token");
    assert!(!token.is_cancellation_requested());
    progress.rpc_accept_progress_canceled(0);
    assert!(token.is_cancellation_requested(), "o cancelamento do workbench chega ao token da extensão");
    first_resolver.resolve(());
    pump();
}
