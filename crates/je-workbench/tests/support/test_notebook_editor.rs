//! Porte parcial de `src/vs/workbench/contrib/notebook/test/browser/testNotebookEditor.ts`: só o que
//! `notebookCommon.test.ts` usa dele, que é `setupInstantiationService` e `TestCell` (mais o
//! `NullNotebookLoggingService` de que `TestCell` precisa). O resto do arquivo (o editor de teste, `withTestNotebook`,
//! `NotebookEditorTestModel`, `TestNotebookExecutionStateService`...) depende do editor de notebook e fica para as
//! fatias dele.
//!
//! Divergências:
//! - `setupInstantiationService` registra só o `ILanguageService` (o `LanguageService`); os outros serviços que o
//!   original registra (undo/redo, configuração, tema, modelo, contexto, lista, layout, log, área de transferência,
//!   armazenamento, confiança, kernel, teclado, barra de status, outline, detecção de linguagem...) não são lidos por
//!   nenhum caso portado. O `TestInstantiationService` é um `InstantiationService` sobre um `ServiceCollection`.
//! - `TestCell extends NotebookCellTextModel` é composição com `Deref` para o [`NotebookCellTextModel`]; a igualdade
//!   é por identidade (o `===` do original, usado por `diff` e pelos `deepStrictEqual` de células).
//! - `public source: string` do `TestCell` é o campo `source` (texto); o `source` do `ICellDto2` é o mesmo texto.

use std::fmt;
use std::ops::Deref;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::mime::mimes;
use je_base::uri::URI;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE, LanguageService};
use je_editor::model::DefaultEndOfLine;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_workbench::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, CellUri, ICellDto2, IOutputDto, TransientOptions};
use je_workbench::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;

struct NullNotebookLoggingService;

impl INotebookLoggingService for NullNotebookLoggingService {
    fn info(&self, _category: &str, _output: &str) {}
    fn warn(&self, _category: &str, _output: &str) {}
    fn error(&self, _category: &str, _output: &str) {}
    fn debug(&self, _category: &str, _output: &str) {}
    fn trace(&self, _category: &str, _output: &str) {}
}

pub struct TestCell {
    pub view_type: String,
    pub source: String,
    base: Rc<NotebookCellTextModel>,
}

impl TestCell {
    pub fn new(
        view_type: &str,
        handle: i64,
        source: &str,
        language: &str,
        cell_kind: CellKind,
        outputs: Vec<IOutputDto>,
        language_service: Rc<dyn ILanguageService>,
    ) -> Self {
        let base = NotebookCellTextModel::new(
            CellUri::generate(&URI::parse("test:///fake/notebook", false), handle),
            handle,
            ICellDto2 {
                source: JsString::from(source),
                language: language.to_string(),
                mime: Some(mimes::TEXT.to_string()),
                cell_kind,
                outputs,
                metadata: None,
                internal_metadata: None,
                collapse_state: None,
            },
            TransientOptions::default(),
            language_service,
            DefaultEndOfLine::Lf,
            None, // defaultCollapseConfig
            None, // languageDetectionService
            Rc::new(NullNotebookLoggingService),
        );
        Self { view_type: view_type.to_string(), source: source.to_string(), base }
    }
}

impl Deref for TestCell {
    type Target = NotebookCellTextModel;

    fn deref(&self) -> &NotebookCellTextModel {
        &self.base
    }
}

impl IDisposable for TestCell {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl PartialEq for TestCell {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl fmt::Debug for TestCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TestCell({}, {:?})", self.view_type, self.source)
    }
}

/// `setupInstantiationService(disposables)`.
pub fn setup_instantiation_service(disposables: &DisposableStore) -> Rc<InstantiationService> {
    let language_service = disposables.add_rc(LanguageService::new(false));
    let language_service: Rc<dyn ILanguageService> = language_service;
    disposables.add_rc(InstantiationService::new(ServiceCollection::new().with(I_LANGUAGE_SERVICE, language_service)))
}

/// `instantiationService.get(ILanguageService)`.
pub fn get_language_service(instantiation_service: &InstantiationService) -> Rc<dyn ILanguageService> {
    instantiation_service.invoke_function(|accessor| accessor.get(I_LANGUAGE_SERVICE))
}
