//! Porte de `src/vs/workbench/api/common/extHostTextEditors.ts`: o lado da extensão de `vscode.window` para os editores de texto
//! (`activeTextEditor`, `visibleTextEditors`, `showTextDocument`, `createTextEditorDecorationType` e os eventos
//! `onDidChange*TextEditor*`), sobre o `ExtHostDocumentsAndEditors` e o proxy `MainThreadTextEditors`.
//!
//! Divergências:
//! - `ExtHostEditors extends Disposable`: o porte tem os emissores e as duas assinaturas dos eventos de editores do
//!   `ExtHostDocumentsAndEditors`, descartadas em [`ExtHostEditors::dispose`].
//! - Os objetos de evento (`TextEditorSelectionChangeEvent`...) são structs ([`TextEditorSelectionChangeEvent`],
//!   [`TextEditorOptionsChangeEvent`], [`TextEditorVisibleRangesChangeEvent`], [`TextEditorViewColumnChangeEvent`],
//!   [`TextEditorDiffInformationChangeEvent`]). `options: { ...data.options, lineNumbers: ... }` é o
//!   [`ResolvedTextEditorConfiguration`] mais o `line_numbers` já convertido.
//! - `TextDocumentShowOptions | ViewColumn` de `showTextDocument` é [`ShowTextDocumentArgument`]. O `viewColumn` ausente vira a coluna
//!   ativa (`ViewColumn.from(undefined)`).
//! - `TextEditorDiffInformation.isStale` (um getter que consulta o documento a cada leitura) é calculado quando a informação chega, a
//!   partir da versão do documento naquele instante: o tipo do porte guarda o booleano.
//! - Os `throw new Error('unknown text editor')` são `Err` (e `$accept*` do protocolo, que não devolve resultado, só as ignora).
//! - Os `$accept*` do protocolo (`void` e que lançam no original) seguem [`ExtHostEditorsShape`]; o `throw` vira pânico com o mesmo
//!   texto, como o resto dos `ExtHost*` (o RPC o captura e relata).

use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_extensions::scanner::ExtensionDescription;
use je_editor::range::Range as EditorRange;

use super::ext_host_document_data::TextDocument;
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{
    EditorPropertiesChangeData, ExtHostEditorsShape, IMainContext, ITextEditorDiffInformation, MainContext, MainThreadTextEditorsShape, ResolvedTextEditorConfiguration,
    TextDocumentShowOptions, TextEditorPositionData,
};
use super::ext_host_text_editor::{DecorationTypeHandle, TextEditor, TextEditorDecorationType};
use super::ext_host_type_converters::geometry::{Range as RangeConverter, Selection as SelectionConverter};
use super::ext_host_type_converters::{TextEditorLineNumbersStyle, ViewColumn};
use super::ext_host_types::{self as types, Range, Selection, TextEditorChange, TextEditorChangeKind, TextEditorDiffInformation, TextEditorLineRange, TextEditorSelectionChangeKind};

/// `vscode.TextEditorSelectionChangeEvent`.
#[derive(Clone)]
pub struct TextEditorSelectionChangeEvent {
    pub text_editor: TextEditor,
    pub selections: Vec<Selection>,
    pub kind: Option<TextEditorSelectionChangeKind>,
}

/// `vscode.TextEditorOptionsChangeEvent`.
#[derive(Clone)]
pub struct TextEditorOptionsChangeEvent {
    pub text_editor: TextEditor,
    pub options: ResolvedTextEditorConfiguration,
    pub line_numbers: types::TextEditorLineNumbersStyle,
}

/// `vscode.TextEditorVisibleRangesChangeEvent`.
#[derive(Clone)]
pub struct TextEditorVisibleRangesChangeEvent {
    pub text_editor: TextEditor,
    pub visible_ranges: Vec<Range>,
}

/// `vscode.TextEditorViewColumnChangeEvent`.
#[derive(Clone)]
pub struct TextEditorViewColumnChangeEvent {
    pub text_editor: TextEditor,
    pub view_column: f64,
}

/// `vscode.TextEditorDiffInformationChangeEvent`.
#[derive(Clone)]
pub struct TextEditorDiffInformationChangeEvent {
    pub text_editor: TextEditor,
    pub diff_information: Option<Vec<TextEditorDiffInformation>>,
}

/// `vscode.ViewColumn | vscode.TextDocumentShowOptions | undefined`, o segundo argumento de `showTextDocument`.
#[derive(Clone, Debug)]
pub enum ShowTextDocumentArgument {
    Column(f64),
    Options { view_column: Option<f64>, preserve_focus: Option<bool>, selection: Option<Range>, preview: Option<bool> },
    Undefined,
}

/// `ExtHostEditors`.
pub struct ExtHostEditors {
    on_did_change_text_editor_selection: Rc<Emitter<TextEditorSelectionChangeEvent>>,
    on_did_change_text_editor_options: Rc<Emitter<TextEditorOptionsChangeEvent>>,
    on_did_change_text_editor_visible_ranges: Rc<Emitter<TextEditorVisibleRangesChangeEvent>>,
    on_did_change_text_editor_view_column: Rc<Emitter<TextEditorViewColumnChangeEvent>>,
    on_did_change_text_editor_diff_information: Rc<Emitter<TextEditorDiffInformationChangeEvent>>,
    on_did_change_active_text_editor: Rc<Emitter<Option<TextEditor>>>,
    on_did_change_visible_text_editors: Rc<Emitter<Vec<TextEditor>>>,
    proxy: Rc<dyn MainThreadTextEditorsShape>,
    ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    subscriptions: Rc<DisposableStore>,
}

impl ExtHostEditors {
    pub fn new(main_context: &dyn IMainContext, ext_host_documents_and_editors: Rc<ExtHostDocumentsAndEditors>) -> Rc<Self> {
        let this = Rc::new(Self {
            on_did_change_text_editor_selection: Emitter::new(),
            on_did_change_text_editor_options: Emitter::new(),
            on_did_change_text_editor_visible_ranges: Emitter::new(),
            on_did_change_text_editor_view_column: Emitter::new(),
            on_did_change_text_editor_diff_information: Emitter::new(),
            on_did_change_active_text_editor: Emitter::new(),
            on_did_change_visible_text_editors: Emitter::new(),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_TEXT_EDITORS),
            ext_host_documents_and_editors: ext_host_documents_and_editors.clone(),
            subscriptions: DisposableStore::new(),
        });
        let visible = this.on_did_change_visible_text_editors.clone();
        this.subscriptions.add(ext_host_documents_and_editors.on_did_change_visible_text_editors().listen(move |editors| visible.fire(editors.clone())));
        let active = this.on_did_change_active_text_editor.clone();
        this.subscriptions.add(ext_host_documents_and_editors.on_did_change_active_text_editor().listen(move |editor| active.fire(editor.clone())));
        this
    }

    pub fn on_did_change_text_editor_selection(&self) -> Event<TextEditorSelectionChangeEvent> {
        self.on_did_change_text_editor_selection.event()
    }

    pub fn on_did_change_text_editor_options(&self) -> Event<TextEditorOptionsChangeEvent> {
        self.on_did_change_text_editor_options.event()
    }

    pub fn on_did_change_text_editor_visible_ranges(&self) -> Event<TextEditorVisibleRangesChangeEvent> {
        self.on_did_change_text_editor_visible_ranges.event()
    }

    pub fn on_did_change_text_editor_view_column(&self) -> Event<TextEditorViewColumnChangeEvent> {
        self.on_did_change_text_editor_view_column.event()
    }

    pub fn on_did_change_text_editor_diff_information(&self) -> Event<TextEditorDiffInformationChangeEvent> {
        self.on_did_change_text_editor_diff_information.event()
    }

    pub fn on_did_change_active_text_editor(&self) -> Event<Option<TextEditor>> {
        self.on_did_change_active_text_editor.event()
    }

    pub fn on_did_change_visible_text_editors(&self) -> Event<Vec<TextEditor>> {
        self.on_did_change_visible_text_editors.event()
    }

    pub fn get_active_text_editor(&self) -> Option<TextEditor> {
        self.ext_host_documents_and_editors.active_editor()
    }

    pub fn get_visible_text_editors(&self) -> Vec<TextEditor> {
        self.ext_host_documents_and_editors.all_editors().iter().map(|editor| editor.value()).collect()
    }

    /// `showTextDocument(document, columnOrOptions, preserveFocus?)`.
    pub fn show_text_document(&self, document: &TextDocument, column_or_options: ShowTextDocumentArgument, preserve_focus: Option<bool>) -> je_base::promise::Promise<TextEditor> {
        let options = match column_or_options {
            ShowTextDocumentArgument::Column(column) => TextDocumentShowOptions { position: Some(ViewColumn::from(Some(column))), preserve_focus, ..TextDocumentShowOptions::default() },
            ShowTextDocumentArgument::Options { view_column, preserve_focus, selection, preview } => TextDocumentShowOptions {
                position: Some(ViewColumn::from(view_column)),
                preserve_focus,
                selection: selection.map(|selection| RangeConverter::from(&selection)),
                pinned: preview.map(|preview| !preview),
            },
            ShowTextDocumentArgument::Undefined => TextDocumentShowOptions { preserve_focus: Some(false), ..TextDocumentShowOptions::default() },
        };

        let uri = document.uri().clone();
        let (proxy, documents_and_editors) = (self.proxy.clone(), self.ext_host_documents_and_editors.clone());
        je_base::async_::async_fn(async move {
            let editor_id = proxy.rpc_try_show_text_document(&uri, options).await?;
            let editor = editor_id.as_deref().and_then(|id| documents_and_editors.get_editor(id));
            if let Some(editor) = editor {
                return Ok(editor.value());
            }
            // we have no editor... having an id means that we had an editor on the main side and that it isn't the current editor anymore...
            let shown = uri.to_string(false);
            if editor_id.is_some() {
                Err(Error::new(format!("Could NOT open editor for \"{shown}\" because another editor opened in the meantime.")))
            } else {
                Err(Error::new(format!("Could NOT open editor for \"{shown}\".")))
            }
        })
    }

    /// `createTextEditorDecorationType(extension, options)`.
    pub fn create_text_editor_decoration_type(&self, extension: &ExtensionDescription, options: &types::DecorationRenderOptions) -> DecorationTypeHandle {
        TextEditorDecorationType::new(&self.proxy, extension, options).value
    }

    fn editor(&self, id: &str) -> Result<Rc<super::ext_host_text_editor::ExtHostTextEditor>, Error> {
        self.ext_host_documents_and_editors.get_editor(id).ok_or_else(|| Error::new("unknown text editor"))
    }

    fn accept_properties(&self, id: &str, data: EditorPropertiesChangeData) -> Result<(), Error> {
        let text_editor = self.editor(id)?;

        // (1) set all properties
        if let Some(options) = &data.options {
            text_editor.accept_options(options);
        }
        let selections = match &data.selections {
            Some(change) => Some(change.selections.iter().map(SelectionConverter::to).collect::<Result<Vec<_>, _>>()?),
            None => None,
        };
        if let Some(selections) = &selections {
            text_editor.accept_selections(selections.clone());
        }
        let visible_ranges = match &data.visible_ranges {
            Some(ranges) => Some(ranges.iter().filter_map(|range: &EditorRange| RangeConverter::to(range).ok()).collect::<Vec<_>>()),
            None => None,
        };
        if let Some(visible_ranges) = &visible_ranges {
            text_editor.accept_visible_ranges(visible_ranges.clone());
        }

        // (2) fire change events
        if let Some(options) = data.options {
            self.on_did_change_text_editor_options.fire(TextEditorOptionsChangeEvent {
                text_editor: text_editor.value(),
                line_numbers: TextEditorLineNumbersStyle::to(options.line_numbers),
                options,
            });
        }
        if let (Some(change), Some(selections)) = (&data.selections, selections) {
            self.on_did_change_text_editor_selection.fire(TextEditorSelectionChangeEvent {
                text_editor: text_editor.value(),
                selections,
                kind: TextEditorSelectionChangeKind::from_source(change.source.as_deref()),
            });
        }
        if let Some(visible_ranges) = visible_ranges {
            self.on_did_change_text_editor_visible_ranges.fire(TextEditorVisibleRangesChangeEvent { text_editor: text_editor.value(), visible_ranges });
        }
        Ok(())
    }

    fn accept_position_data(&self, data: TextEditorPositionData) -> Result<(), Error> {
        for (id, column) in data {
            let text_editor = self.ext_host_documents_and_editors.get_editor(&id).ok_or_else(|| Error::new("Unknown text editor"))?;
            let view_column = ViewColumn::to(column);
            if text_editor.value().view_column() != Some(view_column) {
                text_editor.accept_view_column(view_column);
                self.on_did_change_text_editor_view_column.fire(TextEditorViewColumnChangeEvent { text_editor: text_editor.value(), view_column });
            }
        }
        Ok(())
    }

    fn accept_diff_information(&self, id: &str, diff_information: Option<Vec<ITextEditorDiffInformation>>) -> Result<(), Error> {
        let text_editor = self.editor(id)?;

        let Some(diff_information) = diff_information else {
            text_editor.accept_diff_information(None);
            self.on_did_change_text_editor_diff_information.fire(TextEditorDiffInformationChangeEvent { text_editor: text_editor.value(), diff_information: None });
            return Ok(());
        };

        let result: Vec<TextEditorDiffInformation> = diff_information
            .into_iter()
            .map(|diff| {
                let changes = diff
                    .changes
                    .iter()
                    .map(|[original_start, original_end, modified_start, modified_end]| {
                        let kind = if original_start == original_end {
                            TextEditorChangeKind::Addition
                        } else if modified_start == modified_end {
                            TextEditorChangeKind::Deletion
                        } else {
                            TextEditorChangeKind::Modification
                        };
                        TextEditorChange {
                            original: TextEditorLineRange { start_line_number: f64::from(*original_start), end_line_number_exclusive: f64::from(*original_end) },
                            modified: TextEditorLineRange { start_line_number: f64::from(*modified_start), end_line_number_exclusive: f64::from(*modified_end) },
                            kind,
                        }
                    })
                    .collect();
                let document_version = diff.document_version as f64;
                let is_stale = self.ext_host_documents_and_editors.get_document(&diff.modified).map(|document| document.document().version()) != Some(document_version);
                TextEditorDiffInformation { document_version, original: diff.original, modified: diff.modified, changes, is_stale }
            })
            .collect();

        text_editor.accept_diff_information(Some(result.clone()));
        self.on_did_change_text_editor_diff_information.fire(TextEditorDiffInformationChangeEvent { text_editor: text_editor.value(), diff_information: Some(result) });
        Ok(())
    }
}

impl IDisposable for ExtHostEditors {
    fn dispose(&self) {
        self.subscriptions.dispose();
        self.on_did_change_text_editor_selection.dispose();
        self.on_did_change_text_editor_options.dispose();
        self.on_did_change_text_editor_visible_ranges.dispose();
        self.on_did_change_text_editor_view_column.dispose();
        self.on_did_change_text_editor_diff_information.dispose();
        self.on_did_change_active_text_editor.dispose();
        self.on_did_change_visible_text_editors.dispose();
    }
}

impl ExtHostEditorsShape for ExtHostEditors {
    fn rpc_accept_editor_properties_changed(&self, id: &str, props: EditorPropertiesChangeData) {
        if let Err(error) = self.accept_properties(id, props) {
            errors::throw(error);
        }
    }

    fn rpc_accept_editor_position_data(&self, data: TextEditorPositionData) {
        if let Err(error) = self.accept_position_data(data) {
            errors::throw(error);
        }
    }

    fn rpc_accept_editor_diff_information(&self, id: &str, diff_information: Option<Vec<ITextEditorDiffInformation>>) {
        if let Err(error) = self.accept_diff_information(id, diff_information) {
            errors::throw(error);
        }
    }
}
