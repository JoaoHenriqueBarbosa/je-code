//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/telemetry/arcTelemetryReporter.ts`.
//!
//! Representação:
//! - o construtor do original recebe `_timesMs`, `_documentValueBeforeTrackedEdit`, `_document`, `_gitRepo`,
//!   `_trackedEdit`, `_sendTelemetryEvent` e `_dispose` antes do `@ITelemetryService`: os sete são os `Args` do
//!   [`ArcTelemetryReporterArgs`] e o serviço de telemetria é a dependência injetada;
//! - `_document: { value: IObservableWithChange<StringText, { edit: BaseStringEdit }> }` é um
//!   `IDocumentWithAnnotatedEdits<EditKeySourceData>`, o único documento que o `editSourceTrackingImpl` e o
//!   `arcTelemetrySender` passam (o delta é um [`AnnotatedEdit`]);
//! - `_trackedEdit: BaseStringEdit` é o parâmetro de tipo `E`;
//! - o `TimeoutTimer` de `_reportAfter` guarda um `Weak` de si no fecho (o original o captura direto).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::TimeoutTimer;
use je_base::edit_arc_tracker::IEditArcLineCountInfo;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::{IObservable, Observable, run_on_change};
use je_editor::edits::string_edit::{AnnotatedStringEdit, BaseStringEdit, BaseStringReplacement};
use je_editor::text::abstract_text::StringText;
use je_instantiation::Injectable;
use je_platform::telemetry::telemetry::{I_TELEMETRY_SERVICE, ITelemetryService};

use super::scm_adapter::IScmRepoAdapter;
use crate::contrib::edit_telemetry::browser::helpers::document_with_annotated_edits::{AnnotatedEdit, EditKeySourceData, IDocumentWithAnnotatedEdits};
use crate::contrib::edit_telemetry::common::arc_tracker::ArcTracker;

/// `ArcTelemetryReporterData`.
pub struct ArcTelemetryReporterData {
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub time_delay_ms: u64,
    pub did_branch_change: bool,
    pub arc: usize,
    pub original_char_count: usize,

    pub current_line_count: usize,
    pub current_deleted_line_count: usize,
    pub original_line_count: usize,
    pub original_deleted_line_count: usize,
}

/// Os argumentos estáticos do construtor do `ArcTelemetryReporter`.
pub struct ArcTelemetryReporterArgs<E> {
    pub times_ms: Vec<u64>,
    pub document_value_before_tracked_edit: Rc<StringText>,
    pub document: Rc<dyn IDocumentWithAnnotatedEdits<EditKeySourceData>>,
    /// _markedEdits -> document.value
    pub git_repo: IObservable<Option<Rc<dyn IScmRepoAdapter>>>,
    pub tracked_edit: E,
    pub send_telemetry_event: Rc<dyn Fn(&ArcTelemetryReporterData)>,
    pub dispose: Rc<dyn Fn()>,
}

/// O estado que o `_report` lê, compartilhado com os fechos dos temporizadores.
struct ReporterState {
    arc_tracker: RefCell<ArcTracker>,
    initial_branch_name: Option<String>,
    initial_line_counts: IEditArcLineCountInfo,
    git_repo: IObservable<Option<Rc<dyn IScmRepoAdapter>>>,
    telemetry_service: Rc<dyn ITelemetryService>,
    send_telemetry_event: Rc<dyn Fn(&ArcTelemetryReporterData)>,
}

impl ReporterState {
    /// `_report(timeMs)`.
    fn report(&self, time_ms: u64) {
        let current_branch = self.git_repo.get().and_then(|repo| repo.head_branch_name_obs().get());
        let did_branch_change = current_branch != self.initial_branch_name;
        let arc_tracker = self.arc_tracker.borrow();
        let current_line_counts = arc_tracker.get_line_count_info();

        (self.send_telemetry_event)(&ArcTelemetryReporterData {
            telemetry_service: self.telemetry_service.clone(),
            time_delay_ms: time_ms,
            did_branch_change,
            arc: arc_tracker.get_accepted_restrained_characters_count(),
            original_char_count: arc_tracker.get_original_character_count(),

            current_line_count: current_line_counts.inserted_line_counts,
            current_deleted_line_count: current_line_counts.deleted_line_counts,
            original_line_count: self.initial_line_counts.inserted_line_counts,
            original_deleted_line_count: self.initial_line_counts.deleted_line_counts,
        });
    }
}

/// `ArcTelemetryReporter`.
pub struct ArcTelemetryReporter<E> {
    base: Rc<Disposable>,
    _tracked_edit: std::marker::PhantomData<E>,
}

impl<E: BaseStringEdit + 'static> Injectable for ArcTelemetryReporter<E>
where
    E::Replacement: BaseStringReplacement,
{
    type Args = ArcTelemetryReporterArgs<E>;
    type Deps = (Rc<dyn ITelemetryService>,);

    fn create(args: Self::Args, (telemetry_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        let arc_tracker = ArcTracker::new(&*args.document_value_before_tracked_edit, &args.tracked_edit);
        let initial_line_counts = arc_tracker.get_line_count_info();
        let initial_branch_name = args.git_repo.get().and_then(|repo| repo.head_branch_name_obs().get());

        let state = Rc::new(ReporterState {
            arc_tracker: RefCell::new(arc_tracker),
            initial_branch_name,
            initial_line_counts,
            git_repo: args.git_repo,
            telemetry_service,
            send_telemetry_event: args.send_telemetry_event,
        });

        let tracker_state = state.clone();
        base.register(run_on_change::<Rc<StringText>, AnnotatedEdit<EditKeySourceData>>(args.document.value(), move |_val, _prev_val, changes| {
            let edits: Vec<AnnotatedStringEdit<EditKeySourceData>> = changes.iter().map(|c| c.edit.clone()).collect();
            if let Some(edit) = AnnotatedStringEdit::<EditKeySourceData>::compose_or_undefined(&edits) {
                tracker_state.arc_tracker.borrow_mut().handle_edits(&edit);
            }
        }));

        let count = args.times_ms.len();
        for (i, time_ms) in args.times_ms.iter().copied().enumerate() {
            if time_ms == 0 {
                state.report(time_ms);
            } else {
                let timer = TimeoutTimer::new();
                let weak_timer = Rc::downgrade(&timer);
                let timer_state = state.clone();
                let on_last = if i == count - 1 { Some(args.dispose.clone()) } else { None };
                timer.set_if_not_set(
                    move || {
                        timer_state.report(time_ms);
                        if let Some(timer) = weak_timer.upgrade() {
                            timer.dispose();
                        }
                        if let Some(cb) = on_last {
                            cb();
                        }
                    },
                    time_ms,
                );
                base.register(timer);
            }
        }

        Rc::new(Self { base, _tracked_edit: std::marker::PhantomData })
    }
}

impl<E> IDisposable for ArcTelemetryReporter<E> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
