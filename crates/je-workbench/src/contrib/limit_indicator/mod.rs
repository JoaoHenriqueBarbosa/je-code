//! Porte de `src/vs/workbench/contrib/limitIndicator/browser/limitIndicator.contribution.ts`: a
//! `LimitIndicatorContribution`, que usa o indicador de status de linguagem para mostrar quais recursos foram limitados por
//! desempenho (faixas de dobra, decoradores de cor e decorações de diagnóstico), e o `DiagnosticDecorationLimitReporter`.
//!
//! Divergências:
//! - `extends Disposable` vira um [`Disposable`] por dentro; os construtores devolvem `Rc<Self>`. Os serviços são parâmetros
//!   do construtor, na ordem do original.
//! - `number | string | false` de `LimitInfo.limited` é o [`Limited`]; `FoldingController.limitReporter`,
//!   `ColorDetector.limitReporter` e o `DiagnosticDecorationLimitReporter` implementam [`LimitInfo`].
//! - `getCodeEditor(activeControl)` é o [`get_code_editor`]: o controle de código é ele mesmo, o de diff é o editor
//!   modificado. A identidade do controle (`activeControl === control`) é o endereço do objeto em [`control_key`].
//! - `ILanguageStatus.detail` é `String` neste porte; os itens só nascem com detalhe, então o `undefined` do original não
//!   aparece.
//! - O registro (`registerWorkbenchContribution` na fase `Restored` e `registerEditorContribution` `Lazy`) é [`register`], que
//!   recebe de fora o `IMarkerDecorationsService` de cada editor, como o `register_folding`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::severity::Severity;
use je_editor::code_action::languages::Command;
use je_editor::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use je_editor::color_detector::{ColorDetector, DecoratorLimitReporter};
use je_editor::config::editor_options::EditorOption;
use je_editor::editor_extensions::register_editor_contribution;
use je_editor::folding::folding_controller::{FoldingController, RangesLimitReporter};
use je_editor::language_selector::LanguageSelector;
use je_editor::multicursor::listen_weak;
use je_editor::services::marker_decorations::IMarkerDecorationsService;
use je_editor::view_model_decorations::filter_validation_decorations;
use je_instantiation::ServicesAccessor;

use crate::common::contributions::{IWorkbenchContribution, workbench_contributions_registry};
use crate::services::editor::editor_service::{ActiveTextEditorControl, I_EDITOR_SERVICE, IEditorService};
use crate::services::language_status::language_status_service::{
    ILanguageStatus, ILanguageStatusService, I_LANGUAGE_STATUS_SERVICE, LanguageStatusLabel,
};
use crate::services::lifecycle::lifecycle::LifecyclePhase;

const OPEN_SETTINGS_COMMAND: &str = "workbench.action.openSettings";

thread_local! {
    /// O registro da contribuição de editor acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

fn configure_settings_label() -> String {
    localize("status.button.configure", "Configure", &[])
}

/// `number | string | false`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Limited {
    Count(i64),
    Message(String),
    False,
}

/// `LimitInfo`.
pub trait LimitInfo {
    fn on_did_change(&self) -> Event<()>;

    fn limited(&self) -> Limited;
}

fn limited_of_count(limited: Option<usize>) -> Limited {
    limited.map_or(Limited::False, |count| Limited::Count(count as i64))
}

impl LimitInfo for DecoratorLimitReporter {
    fn on_did_change(&self) -> Event<()> {
        DecoratorLimitReporter::on_did_change(self)
    }

    fn limited(&self) -> Limited {
        limited_of_count(DecoratorLimitReporter::limited(self))
    }
}

impl LimitInfo for RangesLimitReporter {
    fn on_did_change(&self) -> Event<()> {
        RangesLimitReporter::on_did_change(self)
    }

    fn limited(&self) -> Limited {
        limited_of_count(RangesLimitReporter::limited(self))
    }
}

/// `LanguageFeatureAccessor`.
struct LanguageFeatureAccessor {
    id: &'static str,
    name: String,
    label: String,
    source: String,
    settings_id: Option<&'static str>,
    get_limit_reporter: fn(&Rc<CodeEditorWidget>) -> Option<Rc<dyn LimitInfo>>,
}

/// `ColorDecorationAccessor`.
fn color_decoration_accessor() -> LanguageFeatureAccessor {
    LanguageFeatureAccessor {
        id: "decoratorsLimitInfo",
        name: localize("colorDecoratorsStatusItem.name", "Color Decorator Status", &[]),
        label: localize("status.limitedColorDecorators.short", "Color decorators", &[]),
        source: localize("colorDecoratorsStatusItem.source", "Color Decorators", &[]),
        settings_id: Some("editor.colorDecoratorsLimit"),
        get_limit_reporter: |editor| ColorDetector::get(editor).map(|detector| -> Rc<dyn LimitInfo> { detector.limit_reporter().clone() }),
    }
}

/// `FoldingRangeAccessor`.
fn folding_range_accessor() -> LanguageFeatureAccessor {
    LanguageFeatureAccessor {
        id: "foldingLimitInfo",
        name: localize("foldingRangesStatusItem.name", "Folding Status", &[]),
        label: localize("status.limitedFoldingRanges.short", "Folding ranges", &[]),
        source: localize("foldingRangesStatusItem.source", "Folding", &[]),
        settings_id: Some("editor.foldingMaximumRegions"),
        get_limit_reporter: |editor| FoldingController::get(editor).map(|controller| -> Rc<dyn LimitInfo> { controller.limit_reporter().clone() }),
    }
}

/// `DiagnosticDecorationAccessor`.
fn diagnostic_decoration_accessor() -> LanguageFeatureAccessor {
    LanguageFeatureAccessor {
        id: "diagnosticsLimitInfo",
        name: localize("diagnosticHighlightsStatusItem.name", "Diagnostic Highlight Status", &[]),
        label: localize("status.limitedDiagnosticHighlights.short", "Diagnostic highlights", &[]),
        source: localize("diagnosticHighlightsStatusItem.source", "Diagnostics", &[]),
        settings_id: None,
        get_limit_reporter: |editor| DiagnosticDecorationLimitReporter::get(editor).map(|reporter| -> Rc<dyn LimitInfo> { reporter }),
    }
}

/// `getCodeEditor(activeControl)`: o editor de código, ou o modificado de um editor de diferenças.
fn get_code_editor(control: &Option<ActiveTextEditorControl>) -> Option<Rc<CodeEditorWidget>> {
    match control {
        Some(ActiveTextEditorControl::Code(editor)) => editor.code_editor_widget(),
        Some(ActiveTextEditorControl::Diff { modified, .. }) => modified.code_editor_widget(),
        None => None,
    }
}

/// A identidade do controle ativo (o `===` do original): o endereço de cada editor.
fn control_key(control: &Option<ActiveTextEditorControl>) -> Option<Vec<usize>> {
    let address = |editor: &Rc<dyn crate::services::editor::editor_service::IActiveCodeEditor>| Rc::as_ptr(editor) as *const () as usize;
    match control {
        Some(ActiveTextEditorControl::Code(editor)) => Some(vec![address(editor)]),
        Some(ActiveTextEditorControl::Diff { original, modified }) => Some(vec![address(original), address(modified)]),
        None => None,
    }
}

/// `LimitIndicatorContribution`.
pub struct LimitIndicatorContribution {
    base: Rc<Disposable>,
}

impl LimitIndicatorContribution {
    pub fn new(editor_service: Rc<dyn IEditorService>, language_status_service: Rc<dyn ILanguageStatusService>) -> Rc<Self> {
        let base = Disposable::new();

        let accessors = [color_decoration_accessor(), folding_range_accessor(), diagnostic_decoration_accessor()];
        let status_entries: Vec<Rc<LanguageStatusEntry>> =
            accessors.into_iter().map(|accessor| LanguageStatusEntry::new(Rc::clone(&language_status_service), accessor)).collect();
        for entry in &status_entries {
            base.register(entry.clone());
        }

        let control: Rc<RefCell<Option<Vec<usize>>>> = Rc::new(RefCell::new(None));
        let on_active_editor_changed: Rc<dyn Fn()> = {
            let editor_service = Rc::clone(&editor_service);
            Rc::new(move || {
                let active_control = editor_service.active_text_editor_control();
                let key = control_key(&active_control);
                if key == *control.borrow() {
                    return;
                }
                *control.borrow_mut() = key;
                let editor = get_code_editor(&active_control);

                for status_entry in &status_entries {
                    status_entry.on_active_editor_changed(editor.as_ref());
                }
            })
        };
        let listener = Rc::clone(&on_active_editor_changed);
        base.register(editor_service.on_did_active_editor_change().listen(move |_| listener()));

        on_active_editor_changed();
        Rc::new(Self { base })
    }
}

impl IDisposable for LimitIndicatorContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkbenchContribution for LimitIndicatorContribution {}

/// `DiagnosticDecorationLimitReporter`.
pub struct DiagnosticDecorationLimitReporter {
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    marker_decorations_service: Rc<dyn IMarkerDecorationsService>,
    on_did_change: Rc<Emitter<()>>,
    limited: RefCell<Option<String>>,
}

impl DiagnosticDecorationLimitReporter {
    pub const ID: &'static str = "editor.contrib.diagnosticDecorationLimitReporter";

    /// `DiagnosticDecorationLimitReporter.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<DiagnosticDecorationLimitReporter>> {
        editor.get_contribution_as::<DiagnosticDecorationLimitReporter>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, marker_decorations_service: Rc<dyn IMarkerDecorationsService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change: Rc<Emitter<()>> = Emitter::new();
        base.register(on_did_change.clone());

        let this = Rc::new_cyclic(|me: &Weak<DiagnosticDecorationLimitReporter>| {
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &DiagnosticDecorationLimitReporter, _| this.update()));
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &DiagnosticDecorationLimitReporter, e| {
                if e.has_changed(EditorOption::RenderValidationDecorations) || e.has_changed(EditorOption::ReadOnly) {
                    this.update();
                }
            }));
            base.register(listen_weak(me, marker_decorations_service.on_did_change_marker_limit(), |this: &DiagnosticDecorationLimitReporter, model| {
                let current = this.editor.upgrade().and_then(|editor| editor.get_model());
                if current.is_some_and(|current| Rc::ptr_eq(&current, model)) {
                    this.update();
                }
            }));
            DiagnosticDecorationLimitReporter {
                base: base.clone(),
                editor: Rc::downgrade(editor),
                marker_decorations_service: Rc::clone(&marker_decorations_service),
                on_did_change,
                limited: RefCell::new(None),
            }
        });
        this.update();
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `limited` (`false` é `None`).
    pub fn limited(&self) -> Option<String> {
        self.limited.borrow().clone()
    }

    fn update(&self) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let model = editor.get_model();
        let marker_limit = model
            .filter(|_| !filter_validation_decorations(editor.get_options()))
            .and_then(|model| self.marker_decorations_service.get_marker_limits(&model.uri()));
        let Some(marker_limit) = marker_limit else {
            if self.limited.borrow().is_some() {
                *self.limited.borrow_mut() = None;
                self.on_did_change.fire(());
            }
            return;
        };
        let limited = Self::get_limited_message(marker_limit.is_markers_truncated, marker_limit.is_markers_source_truncated);
        if limited != *self.limited.borrow() {
            *self.limited.borrow_mut() = limited;
            self.on_did_change.fire(());
        }
    }

    fn get_limited_message(limited: Option<i64>, is_markers_source_truncated: bool) -> Option<String> {
        if let (true, Some(limited)) = (is_markers_source_truncated, limited) {
            return Some(localize(
                "status.limitedDiagnosticsAndHighlights.details",
                "Some diagnostics are not shown, and only {0} diagnostics are highlighted in the editor for performance reasons.",
                &[&limited.to_string()],
            ));
        }
        if is_markers_source_truncated {
            return Some(localize("status.limitedDiagnostics.details", "Some diagnostics are not shown for performance reasons.", &[]));
        }
        if let Some(limited) = limited {
            return Some(localize(
                "status.limitedDiagnosticHighlights.details",
                "Only {0} diagnostics are highlighted in the editor for performance reasons.",
                &[&limited.to_string()],
            ));
        }
        None
    }
}

impl LimitInfo for DiagnosticDecorationLimitReporter {
    fn on_did_change(&self) -> Event<()> {
        DiagnosticDecorationLimitReporter::on_did_change(self)
    }

    fn limited(&self) -> Limited {
        DiagnosticDecorationLimitReporter::limited(self).map_or(Limited::False, Limited::Message)
    }
}

impl IDisposable for DiagnosticDecorationLimitReporter {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for DiagnosticDecorationLimitReporter {}

/// `LanguageStatusEntry`.
struct LanguageStatusEntry {
    me: Weak<LanguageStatusEntry>,
    language_status_service: Rc<dyn ILanguageStatusService>,
    accessor: LanguageFeatureAccessor,
    limit_status_item: RefCell<Option<DisposableRef>>,
    indicator_change_listener: RefCell<Option<DisposableRef>>,
}

impl LanguageStatusEntry {
    fn new(language_status_service: Rc<dyn ILanguageStatusService>, accessor: LanguageFeatureAccessor) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            language_status_service,
            accessor,
            limit_status_item: RefCell::new(None),
            indicator_change_listener: RefCell::new(None),
        })
    }

    fn on_active_editor_changed(&self, editor: Option<&Rc<CodeEditorWidget>>) -> bool {
        if let Some(listener) = self.indicator_change_listener.borrow_mut().take() {
            listener.dispose();
        }

        let info = editor.and_then(|editor| (self.accessor.get_limit_reporter)(editor));
        self.update_status_item(info.as_ref());
        if let Some(info) = info {
            let me = self.me.clone();
            let observed = Rc::clone(&info);
            *self.indicator_change_listener.borrow_mut() = Some(info.on_did_change().listen(move |_| {
                if let Some(this) = me.upgrade() {
                    this.update_status_item(Some(&observed));
                }
            }));
            return true;
        }
        false
    }

    fn update_status_item(&self, info: Option<&Rc<dyn LimitInfo>>) {
        if let Some(item) = self.limit_status_item.borrow_mut().take() {
            item.dispose();
        }
        let detail = info.and_then(|info| match info.limited() {
            Limited::Message(message) => Some(message),
            Limited::Count(count) => Some(localize("status.limited.details", "only {0} shown for performance reasons", &[&count.to_string()])),
            Limited::False => None,
        });
        if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
            let command = self.accessor.settings_id.map(|settings_id| Command {
                id: OPEN_SETTINGS_COMMAND.to_string(),
                title: configure_settings_label(),
                tooltip: None,
                arguments: Some(vec![Dynamic::string(settings_id)]),
            });
            let status = ILanguageStatus {
                id: self.accessor.id.to_string(),
                selector: LanguageSelector::from("*"),
                name: self.accessor.name.clone(),
                severity: Severity::Warning,
                label: LanguageStatusLabel::Text(self.accessor.label.clone()),
                detail,
                command,
                accessibility_info: None,
                source: self.accessor.source.clone(),
                busy: false,
            };
            *self.limit_status_item.borrow_mut() = Some(self.language_status_service.add_status(status));
        }
    }
}

impl IDisposable for LanguageStatusEntry {
    fn dispose(&self) {
        if let Some(item) = self.limit_status_item.borrow_mut().take() {
            item.dispose();
        }
        if let Some(listener) = self.indicator_change_listener.borrow_mut().take() {
            listener.dispose();
        }
    }
}

/// Os efeitos do carregamento do módulo original: a contribuição `LimitIndicatorContribution` (fase `Restored`) e o
/// `registerEditorContribution(DiagnosticDecorationLimitReporter.ID, ..., Lazy)`. `marker_decorations_service` entrega o
/// `IMarkerDecorationsService` de cada editor (o `createInstance` do original). A parte do editor é idempotente.
pub fn register(marker_decorations_service: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> Rc<dyn IMarkerDecorationsService>>) {
    workbench_contributions_registry().register_workbench_contribution(
        Rc::new(|accessor: &ServicesAccessor| {
            let contribution: Rc<dyn IWorkbenchContribution> =
                LimitIndicatorContribution::new(accessor.get(I_EDITOR_SERVICE), accessor.get(I_LANGUAGE_STATUS_SERVICE));
            contribution
        }),
        LifecyclePhase::Restored,
    );

    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        DiagnosticDecorationLimitReporter::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            DiagnosticDecorationLimitReporter::new(editor, marker_decorations_service(editor))
        }),
        EditorContributionInstantiation::Lazy,
    );
}
