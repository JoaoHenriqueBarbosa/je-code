//! Porte de `src/vs/workbench/contrib/codeEditor/browser/outline/documentSymbolEditor.ts`: o contrato `IDocumentSymbolEditor`
//! (o que o outline de símbolos lê do editor) e os dois adaptadores, o do editor de código comum
//! ([`code_editor_document_symbol_adapter`]) e o do editor de texto personalizado
//! ([`custom_text_editor_document_symbol_adapter`]).
//!
//! Notas de tradução:
//! - **Eventos.** `onDidChangeModel`, `onDidChangeModelLanguage`, `onDidChangeCursorPosition` e `onDidDispose` entregam
//!   `unknown`/`void` e são `Event<()>` (o evento original por [`Event::signal`]); o conteúdo é o
//!   [`ModelContentChangedEvent`].
//! - **`ICodeEditor`.** É o [`CodeEditorWidget`], que tem os eventos como métodos próprios.
//! - **`ICodeEditorService.openCodeEditor`.** É o `ICodeEditorService` mínimo de `je_editor::opener_service`; o editor de origem é
//!   o `CodeEditorHandle` (o próprio widget).
//! - **`IEditorOptions` com `selection` e `selectionRevealType`.** É o [`TextEditorOptions`] do `opener_service`, que ganhou o
//!   `selection_reveal_type`; o do editor personalizado é o `TextEditorOptions` do workbench.
//! - **`editorService.openEditor(input, SIDE_GROUP)`.** É `open_editor_in(input, Some(PreferredGroup::Side))`.
//! - **`createDecorationsCollection(...)` + `clear()`.** A decoração `document-symbols-outline-range-highlight` vive numa
//!   coleção que o descarte do `preview` limpa.

use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use je_editor::editor_common::ScrollType;
use je_editor::model::{ModelDecorationOptionsInit, ModelDeltaDecoration};
use je_editor::opener_service::{CodeEditorHandle, ICodeEditorService, TextEditorOptions as CodeEditorOptions, TextResourceEditorInput};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::{Selection, SelectionDirection};
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_editor::text_model_events::ModelContentChangedEvent;
use je_platform::opener::editor_types::{EditorOptions, EditorOverride, TextEditorSelection, TextEditorSelectionRevealType};

use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
use crate::contrib::custom_editor::custom_text_editor_navigation::ICustomTextEditorNavigation;
use crate::services::editor::editor_groups_service::PreferredGroup;
use crate::services::editor::editor_service::IEditorService;

/// `IDocumentSymbolEditor`.
pub trait IDocumentSymbolEditor {
    fn on_did_change_model(&self) -> Event<()>;
    fn on_did_change_model_language(&self) -> Event<()>;
    fn on_did_change_model_content(&self) -> Event<ModelContentChangedEvent>;
    fn on_did_change_cursor_position(&self) -> Event<()>;
    fn on_did_dispose(&self) -> Event<()>;
    fn get_model(&self) -> Option<Rc<TextModel>>;
    fn get_position(&self) -> Option<Position>;
    fn reveal(&self, resource: &URI, range: Range, selection: Range, options: &EditorOptions, side_by_side: bool) -> Promise<()>;
    fn preview(&self, range: Range) -> DisposableRef;
    fn capture_view_state(&self) -> DisposableRef;
}

/// O `ITextEditorSelection` de um intervalo.
fn text_editor_selection(range: &Range) -> TextEditorSelection {
    TextEditorSelection {
        start_line_number: f64::from(range.start_line_number),
        start_column: f64::from(range.start_column),
        end_line_number: Some(f64::from(range.end_line_number)),
        end_column: Some(f64::from(range.end_column)),
    }
}

/// O destino do `openCodeEditor` do `codeEditorDocumentSymbolAdapter`.
struct CodeEditorSymbolAdapter {
    editor: Rc<CodeEditorWidget>,
    code_editor_service: Rc<dyn ICodeEditorService>,
}

/// `codeEditorDocumentSymbolAdapter(editor, codeEditorService)`.
pub fn code_editor_document_symbol_adapter(editor: Rc<CodeEditorWidget>, code_editor_service: Rc<dyn ICodeEditorService>) -> Rc<dyn IDocumentSymbolEditor> {
    Rc::new(CodeEditorSymbolAdapter { editor, code_editor_service })
}

impl IDocumentSymbolEditor for CodeEditorSymbolAdapter {
    fn on_did_change_model(&self) -> Event<()> {
        self.editor.on_did_change_model().signal()
    }

    fn on_did_change_model_language(&self) -> Event<()> {
        self.editor.on_did_change_model_language().signal()
    }

    fn on_did_change_model_content(&self) -> Event<ModelContentChangedEvent> {
        self.editor.on_did_change_model_content()
    }

    fn on_did_change_cursor_position(&self) -> Event<()> {
        self.editor.on_did_change_cursor_position().signal()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.editor.on_did_dispose()
    }

    fn get_model(&self) -> Option<Rc<TextModel>> {
        ICodeEditor::get_model(self.editor.as_ref())
    }

    fn get_position(&self) -> Option<Position> {
        ICodeEditor::get_position(self.editor.as_ref())
    }

    fn reveal(&self, resource: &URI, _range: Range, selection: Range, options: &EditorOptions, side_by_side: bool) -> Promise<()> {
        let input = TextResourceEditorInput {
            resource: resource.clone(),
            options: Some(CodeEditorOptions {
                editor: options.clone(),
                selection: Some(text_editor_selection(&selection)),
                selection_reveal_type: Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
            }),
        };
        let source: CodeEditorHandle = self.editor.clone();
        self.code_editor_service.open_code_editor(input, Some(source), Some(side_by_side)).then(|_| ())
    }

    fn preview(&self, range: Range) -> DisposableRef {
        ICodeEditor::reveal_range_in_center_if_outside_viewport(self.editor.as_ref(), range, Some(ScrollType::Smooth));
        let decorations = self.editor.create_decorations_collection(Some(vec![ModelDeltaDecoration {
            range,
            options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                description: "document-symbols-outline-range-highlight".to_owned(),
                class_name: Some("rangeHighlight".to_owned()),
                is_whole_line: true,
                ..ModelDecorationOptionsInit::default()
            }),
        }]));
        to_disposable(move || decorations.clear())
    }

    fn capture_view_state(&self) -> DisposableRef {
        let state = ICodeEditor::save_view_state(self.editor.as_ref());
        let editor = Rc::clone(&self.editor);
        to_disposable(move || {
            if let Some(state) = state {
                ICodeEditor::restore_view_state(editor.as_ref(), Some(&state));
            }
        })
    }
}

/// O destino do `editorService.openEditor` e do `navigation.revealRange` do `customTextEditorDocumentSymbolAdapter`.
struct CustomTextEditorSymbolAdapter {
    navigation: Rc<dyn ICustomTextEditorNavigation>,
    view_type: String,
    editor_service: Rc<dyn IEditorService>,
}

/// `customTextEditorDocumentSymbolAdapter(navigation, viewType, editorService)`.
pub fn custom_text_editor_document_symbol_adapter(
    navigation: Rc<dyn ICustomTextEditorNavigation>,
    view_type: &str,
    editor_service: Rc<dyn IEditorService>,
) -> Rc<dyn IDocumentSymbolEditor> {
    Rc::new(CustomTextEditorSymbolAdapter { navigation, view_type: view_type.to_owned(), editor_service })
}

impl IDocumentSymbolEditor for CustomTextEditorSymbolAdapter {
    fn on_did_change_model(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_change_model_language(&self) -> Event<()> {
        self.navigation.model().on_did_change_language().signal()
    }

    fn on_did_change_model_content(&self) -> Event<ModelContentChangedEvent> {
        self.navigation.model().on_did_change_content()
    }

    fn on_did_change_cursor_position(&self) -> Event<()> {
        self.navigation.on_did_change_selection()
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.navigation.on_did_dispose()
    }

    fn get_model(&self) -> Option<Rc<TextModel>> {
        let model = self.navigation.model();
        if model.is_disposed() { None } else { Some(model) }
    }

    fn get_position(&self) -> Option<Position> {
        self.navigation.selection().map(|selection| Position::new(selection.position_line_number, selection.position_column))
    }

    fn reveal(&self, resource: &URI, range: Range, selection: Range, options: &EditorOptions, side_by_side: bool) -> Promise<()> {
        if side_by_side {
            let input = UntypedEditorInput {
                resource: Some(resource.clone()),
                options: Some(TextEditorOptions {
                    base: EditorOptions { override_: Some(EditorOverride::Id(self.view_type.as_str().into())), ..options.clone() },
                    selection: Some(text_editor_selection(&selection)),
                    selection_reveal_type: Some(TextEditorSelectionRevealType::NearTopIfOutsideViewport),
                    selection_source: None,
                }),
                ..UntypedEditorInput::default()
            };
            self.editor_service.open_editor_in(input, Some(PreferredGroup::Side)).then(|_| ())
        } else {
            self.navigation.reveal_range(
                range,
                Some(Selection::from_range(&selection, SelectionDirection::Ltr)),
                options.preserve_focus == Some(true),
                CancellationToken::None,
            )
        }
    }

    fn preview(&self, range: Range) -> DisposableRef {
        let cancellation = CancellationTokenSource::new(None);
        let _ = self.navigation.reveal_range(range, None, true, cancellation.token());
        to_disposable(move || cancellation.dispose_with(true))
    }

    fn capture_view_state(&self) -> DisposableRef {
        self.navigation.capture_view_state()
    }
}
