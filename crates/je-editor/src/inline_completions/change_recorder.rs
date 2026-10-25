//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/changeRecorder.ts`: o `TextModelChangeRecorder`, que
//! registra o motivo de cada mudança do texto (no canal de log `textModelChanges`, em nível `Trace`, e no log estruturado
//! `editor.inlineSuggest.logChangeReason.commandId`).
//!
//! Notas de tradução:
//! - o `StructuredLogger` (`structuredLogger.ts`) depende do `IDataChannelService`, que não tem porte: o logger que o
//!   original cria com `createInstance(StructuredLogger.cast<...>(), key)` entra injetado como [`ChangeReasonLogger`] (o
//!   mesmo padrão do `InlineCompletionsFetchLogger` da fonte), e o recorder o descarta junto com ele mesmo, como o
//!   `this._register(...)`. A chave do log estruturado é [`CHANGE_REASON_LOGGER_KEY`];
//! - `IRecordableEditorLogEntry & IDocumentEventDataSetChangeReason` é a struct [`DocumentEventSetChangeReason`]: o
//!   `...reason.metadata` do original é o `metadata` inteiro da origem da edição, e `source`, `time`, `modelUri` e
//!   `modelVersion` são os campos seguintes;
//! - `this._editor instanceof CodeEditorWidget` é sempre verdadeiro: o recorder recebe o `Rc<CodeEditorWidget>`, que é o
//!   único editor que o porte tem (o `ICodeEditor` do original);
//! - `loggingLevel` do original é um observável do nível de log, comparado com `Trace` dentro do `autorun`. O `LogLevel`
//!   da plataforma não tem `StrictEquals` (a regra de coerência de traits impede de implementá-lo daqui), então o observável
//!   é o resultado de `canLog(nível, Trace)`: o `autorun` reexecuta quando esse booleano muda, e o corpo só depende dele;
//! - `Date.now()` é [`date_now`] e `setTimeout(fn, 0)` é [`set_timeout`] com zero milissegundo (o log segue depois do
//!   evento de mudança do conteúdo, para chegar ao host de extensões na ordem certa).

use std::rc::Rc;

use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::base::IObservable;
use je_base::observable::{autorun, observable_from_event};
use je_base::runtime::{date_now, set_timeout};
use je_base::uri::URI;
use je_platform::log::{LogLevel, LoggerOptions, LoggerService, ResourceOrId, can_log};

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::inline_completions::utils::dispose_handle;
use crate::text_model_edit_source::EditSourceMetadata;

/// A chave do `StructuredLogger` do recorder (`editor.inlineSuggest.logChangeReason.commandId`).
pub const CHANGE_REASON_LOGGER_KEY: &str = "editor.inlineSuggest.logChangeReason.commandId";

/// O `sourceId` de `IDocumentEventDataSetChangeReason`.
pub const SET_CHANGE_REASON_SOURCE_ID: &str = "TextModel.setChangeReason";

/// `type?: 'word' | 'line'` de `ITextModelChangeRecorderMetadata`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextModelChangeRecorderType {
    Word,
    Line,
}

/// `ITextModelChangeRecorderMetadata`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextModelChangeRecorderMetadata {
    pub source: Option<String>,
    pub extension_id: Option<String>,
    pub nes: Option<bool>,
    /// `type`.
    pub ty: Option<TextModelChangeRecorderType>,
}

/// `IRecordableEditorLogEntry & IDocumentEventDataSetChangeReason`: o que o log estruturado recebe a cada mudança. O
/// `sourceId` é sempre [`SET_CHANGE_REASON_SOURCE_ID`].
#[derive(Clone, Debug)]
pub struct DocumentEventSetChangeReason {
    /// `...reason.metadata`.
    pub metadata: EditSourceMetadata,
    /// `source`.
    pub source: String,
    /// `time`.
    pub time: f64,
    /// `modelUri`.
    pub model_uri: URI,
    /// `modelVersion`.
    pub model_version: i64,
}

/// O `StructuredLogger<IRecordableEditorLogEntry & IDocumentEventDataSetChangeReason>` do recorder.
pub trait ChangeReasonLogger: IDisposable {
    /// `isEnabled`.
    fn is_enabled(&self) -> IObservable<bool>;
    /// `log(data)`: `false` se o log estruturado está desligado.
    fn log(&self, data: &DocumentEventSetChangeReason) -> bool;
}

/// `TextModelChangeRecorder`.
pub struct TextModelChangeRecorder {
    base: Rc<Disposable>,
}

impl TextModelChangeRecorder {
    pub fn new(editor: &Rc<CodeEditorWidget>, logger_service: &dyn LoggerService, structured_logger: Rc<dyn ChangeReasonLogger>) -> Rc<Self> {
        let base = Disposable::new();
        base.register(dispose_handle(&structured_logger));

        let logger = logger_service.create_logger(
            ResourceOrId::from("textModelChanges"),
            Some(LoggerOptions { hidden: Some(false), name: Some("Text Model Changes Reason".to_owned()), ..LoggerOptions::default() }),
        );

        let can_trace: IObservable<bool> = {
            let logger = Rc::clone(&logger);
            observable_from_event(logger.on_did_change_log_level(), move |_| can_log(logger.get_level(), LogLevel::Trace))
        };

        {
            let (editor, logger) = (Rc::clone(editor), Rc::clone(&logger));
            base.register(autorun(move |reader| {
                if !can_trace.read(reader) {
                    return;
                }

                let (editor_in_listener, logger) = (Rc::clone(&editor), Rc::clone(&logger));
                reader.store().add(editor.on_did_change_model_content().listen(move |e| {
                    if editor_in_listener.get_model().is_some_and(|model| model.uri().scheme().to_string_lossy() == "output") {
                        return;
                    }
                    let reasons = e.detailed_reasons.iter().map(|reason| reason.to_key(usize::MAX, &[])).collect::<Vec<_>>().join(", ");
                    logger.trace(&format!("onDidChangeModelContent: {reasons}"));
                }));
            }));
        }

        {
            let editor = Rc::clone(editor);
            base.register(autorun(move |reader| {
                if !structured_logger.is_enabled().read(reader) {
                    return;
                }

                let (editor_in_listener, structured_logger) = (Rc::clone(&editor), Rc::clone(&structured_logger));
                reader.store().add(editor.on_did_change_model_content().listen(move |e| {
                    let Some(model) = editor_in_listener.get_model() else {
                        return;
                    };
                    let Some(reason) = e.detailed_reasons.first() else {
                        return;
                    };

                    let data = DocumentEventSetChangeReason {
                        metadata: reason.metadata.clone(),
                        source: reason.source.clone(),
                        time: date_now() as f64,
                        model_uri: model.uri(),
                        model_version: model.get_version_id(),
                    };
                    let structured_logger = Rc::clone(&structured_logger);
                    // To ensure that this reaches the extension host after the content change event.
                    // (Without the setTimeout, I observed this command being called before the content change event arrived)
                    let _ = set_timeout(
                        move || {
                            structured_logger.log(&data);
                        },
                        0,
                    );
                }));
            }));
        }

        Rc::new(Self { base })
    }
}

impl IDisposable for TextModelChangeRecorder {
    fn dispose(&self) {
        self.base.dispose();
    }
}
