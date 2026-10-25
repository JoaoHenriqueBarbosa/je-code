//! Porte de `src/vs/editor/contrib/editorState/browser/editorState.ts`: `CodeEditorStateFlag`, `EditorState` (o
//! retrato do que o editor mostra, para saber se uma operação assíncrona ainda vale) e
//! `TextModelCancellationTokenSource`.
//!
//! Decisões de tradução:
//! - `EditorState` só lê do editor o modelo (URI e versão), a posição, a seleção e a rolagem. Esse recorte é o trait
//!   [`EditorStateSource`], com uma implementação para todo [`ICodeEditor`]; o teste do original passa um objeto
//!   literal com só esses cinco métodos (`as ICodeEditor`), o que aqui é uma estrutura própria que implementa o
//!   trait. Os campos que o literal deixa `undefined` são `Option` ([`EditorStateModel::version_id`], a rolagem).
//! - `strings.format('{0}#{1}', uri.toString(), version)` imprime `undefined` quando a versão falta; o
//!   [`EditorState`] guarda o texto pronto, como o original.
//! - `Selection.equalsRange` compara só o intervalo; o estado guarda a seleção como [`Range`] (a mesma conversão
//!   implícita do `Selection extends Range`).
//! - `EditorStateCancellationTokenSource` estende [`EditorKeybindingCancellationTokenSource`]: aqui é o [`Deref`] para
//!   ela (e, por ela, para a fonte). Recebe o `range` opcional como `Option<Range>` e ouve os eventos do
//!   [`CodeEditorWidget`] (`onDidChangeCursorPosition`, `onDidChangeCursorSelection`, `onDidScrollChange`,
//!   `onDidChangeModel` e `onDidChangeModelContent`); o `IActiveCodeEditor` do original é o widget com modelo.
//! - `TextModelCancellationTokenSource` estende `CancellationTokenSource`: aqui é o [`Deref`] para a fonte, e o
//!   `dispose` que o original sobrescreve é o [`IDisposable`] desta estrutura.

use std::ops::Deref;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::uri::URI;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::keybinding_cancellation::{EditorCancellationTokens, EditorKeybindingCancellationTokenSource};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// `CodeEditorStateFlag`: máscara de bits.
pub mod code_editor_state_flag {
    pub const VALUE: u32 = 1;
    pub const SELECTION: u32 = 2;
    pub const POSITION: u32 = 4;
    pub const SCROLL: u32 = 8;
}

/// O que `EditorState` lê do `ITextModel`: `uri` e `getVersionId()`. A versão é `Option` porque o objeto literal do
/// teste do original a deixa `undefined`.
#[derive(Clone)]
pub struct EditorStateModel {
    pub uri: URI,
    pub version_id: Option<i64>,
}

/// O recorte do `ICodeEditor` que `EditorState` consulta (ver a nota do módulo).
pub trait EditorStateSource {
    /// `getModel()`.
    fn get_model(&self) -> Option<EditorStateModel>;

    /// `getPosition()`.
    fn get_position(&self) -> Option<Position>;

    /// `getSelection()`.
    fn get_selection(&self) -> Option<Range>;

    /// `getScrollLeft()`.
    fn get_scroll_left(&self) -> Option<f64>;

    /// `getScrollTop()`.
    fn get_scroll_top(&self) -> Option<f64>;
}

impl<E: ICodeEditor + ?Sized> EditorStateSource for E {
    fn get_model(&self) -> Option<EditorStateModel> {
        ICodeEditor::get_model(self).map(|model| EditorStateModel { uri: model.uri(), version_id: Some(model.get_version_id()) })
    }

    fn get_position(&self) -> Option<Position> {
        ICodeEditor::get_position(self)
    }

    fn get_selection(&self) -> Option<Range> {
        ICodeEditor::get_selection(self).map(|selection| *selection)
    }

    fn get_scroll_left(&self) -> Option<f64> {
        Some(ICodeEditor::get_scroll_left(self))
    }

    fn get_scroll_top(&self) -> Option<f64> {
        Some(ICodeEditor::get_scroll_top(self))
    }
}

/// `EditorState`.
pub struct EditorState {
    flags: u32,
    position: Option<Position>,
    selection: Option<Range>,
    model_version_id: Option<String>,
    scroll_left: Option<f64>,
    scroll_top: Option<f64>,
}

impl EditorState {
    pub fn new(editor: &dyn EditorStateSource, flags: u32) -> Self {
        let model_version_id = if flags & code_editor_state_flag::VALUE != 0 {
            editor.get_model().map(|model| {
                let version = model.version_id.map_or_else(|| "undefined".to_owned(), |version| version.to_string());
                format!("{}#{}", model.uri.to_string(false).to_string_lossy(), version)
            })
        } else {
            None
        };
        let position = if flags & code_editor_state_flag::POSITION != 0 { editor.get_position() } else { None };
        let selection = if flags & code_editor_state_flag::SELECTION != 0 { editor.get_selection() } else { None };
        let (scroll_left, scroll_top) = if flags & code_editor_state_flag::SCROLL != 0 {
            (editor.get_scroll_left(), editor.get_scroll_top())
        } else {
            (Some(-1.0), Some(-1.0))
        };
        Self { flags, position, selection, model_version_id, scroll_left, scroll_top }
    }

    /// `_equals(other)`: `other` é sempre um `EditorState` (o `instanceof` do original é o tipo).
    fn equals(&self, other: &EditorState) -> bool {
        self.model_version_id == other.model_version_id
            && self.scroll_left == other.scroll_left
            && self.scroll_top == other.scroll_top
            && self.position == other.position
            && self.selection == other.selection
    }

    pub fn validate(&self, editor: &dyn EditorStateSource) -> bool {
        self.equals(&EditorState::new(editor, self.flags))
    }
}

/// `EditorStateCancellationTokenSource`: uma fonte de cancelamento que cancela quando o editor muda, como expressam as
/// `flags`. Com `range`, mudanças de posição e de seleção dentro dele não cancelam.
pub struct EditorStateCancellationTokenSource {
    base: EditorKeybindingCancellationTokenSource,
    listener: Rc<DisposableStore>,
}

impl EditorStateCancellationTokenSource {
    pub fn new(
        editor: &CodeEditorWidget,
        tokens: &EditorCancellationTokens,
        flags: u32,
        range: Option<Range>,
        parent: Option<&CancellationToken>,
    ) -> Self {
        let base = EditorKeybindingCancellationTokenSource::new(editor, tokens, parent);
        let listener = DisposableStore::new();
        let source: CancellationTokenSource = (*base).clone();

        if flags & code_editor_state_flag::POSITION != 0 {
            let to_cancel = source.clone();
            listener.add(editor.on_did_change_cursor_position().listen(move |e| {
                if !range.is_some_and(|range| range.contains_position(&e.position)) {
                    to_cancel.cancel();
                }
            }));
        }
        if flags & code_editor_state_flag::SELECTION != 0 {
            let to_cancel = source.clone();
            listener.add(editor.on_did_change_cursor_selection().listen(move |e| {
                if !range.is_some_and(|range| range.contains_range(&e.selection)) {
                    to_cancel.cancel();
                }
            }));
        }
        if flags & code_editor_state_flag::SCROLL != 0 {
            let to_cancel = source.clone();
            listener.add(editor.on_did_scroll_change().listen(move |_| to_cancel.cancel()));
        }
        if flags & code_editor_state_flag::VALUE != 0 {
            let to_cancel = source.clone();
            listener.add(editor.on_did_change_model().listen(move |_| to_cancel.cancel()));
            let to_cancel = source.clone();
            listener.add(editor.on_did_change_model_content().listen(move |_| to_cancel.cancel()));
        }
        Self { base, listener }
    }
}

impl Deref for EditorStateCancellationTokenSource {
    type Target = CancellationTokenSource;

    fn deref(&self) -> &CancellationTokenSource {
        &self.base
    }
}

impl IDisposable for EditorStateCancellationTokenSource {
    fn dispose(&self) {
        self.listener.dispose();
        self.base.dispose();
    }
}

/// `TextModelCancellationTokenSource`: uma fonte de cancelamento que cancela quando o conteúdo do modelo muda.
pub struct TextModelCancellationTokenSource {
    source: CancellationTokenSource,
    listener: DisposableRef,
}

impl TextModelCancellationTokenSource {
    pub fn new(model: &TextModel, parent: Option<&CancellationToken>) -> Self {
        let source = CancellationTokenSource::new(parent);
        let to_cancel = source.clone();
        let listener = model.on_did_change_content().listen(move |_| to_cancel.cancel());
        Self { source, listener }
    }
}

impl Deref for TextModelCancellationTokenSource {
    type Target = CancellationTokenSource;

    fn deref(&self) -> &CancellationTokenSource {
        &self.source
    }
}

impl IDisposable for TextModelCancellationTokenSource {
    fn dispose(&self) {
        self.listener.dispose();
        self.source.dispose();
    }
}
