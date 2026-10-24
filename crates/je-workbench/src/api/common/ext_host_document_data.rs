//! Porte de `src/vs/workbench/api/common/extHostDocumentData.ts`: o documento de texto que a extensão
//! enxerga (`vscode.TextDocument`) e o espelho do modelo que o alimenta.
//!
//! Divergências:
//! - `ExtHostDocumentData extends MirrorTextModel` vira composição: o espelho (o `MirrorTextModel` do
//!   `je-editor`, cujos membros `protected` o original lê) fica atrás de [`ExtHostDocumentData::mirror`] e
//!   [`ExtHostDocumentData::mirror_mut`] (é por eles que se chama `onEvents`, `getText()` e `version`).
//! - O objeto `document` (um literal congelado com getters que apontam para os campos privados) é o
//!   [`TextDocument`]: um `Rc` para o mesmo estado que o [`ExtHostDocumentData`] guarda. Os métodos privados
//!   (`_save`, `_lineAt`, `_offsetAt`...) são os métodos do [`TextDocument`], sem camada de repasse, e os
//!   campos de escrita proibida (`uri`, `isDirty`, `lineCount`...) não têm setter: tentar escrevê-los, que
//!   o original rejeita em tempo de execução, é erro de compilação. A identidade do objeto (`_document`
//!   guardado) é a do estado, comparável com [`TextDocument::ptr_eq`].
//! - Os números da API são [`f64`] (`version`, `lineCount`, `offsetAt`, `positionAt`), as posições e os
//!   intervalos são os valores de [`super::ext_host_types`], e o texto é [`JsString`], porque a API é em
//!   unidades UTF-16 e um texto de extensão pode carregar substitutos isolados.
//! - `_strictInstanceofChecks` (o parâmetro de reuso do construtor) e as verificações de tipo de
//!   `_validateRange`, `_validatePosition` e `_lineAt` (`instanceof`, `isPosition`, `typeof`) não existem:
//!   os tipos garantem o argumento. Pela mesma razão `lineAt` recebe um [`LineOrPosition`].
//! - `IExtHostDocumentSaveDelegate.$trySaveDocument(uri: UriComponents)` é `rpc_try_save_document(&URI)`
//!   (o prefixo `$` vira `rpc_`, ver `ext_host_protocol`, e a conversão para componentes é da camada de RPC).
//! - O mapa global `_languageId2WordDefinition` é `thread_local!`, porque o [`JsRegExp`] não é `Sync`.
//! - `dispose()` é o método próprio de [`ExtHostDocumentData`] (não implementa `IDisposable`): o original
//!   não chama o `dispose` do espelho, então as linhas continuam legíveis depois de fechado.
//! - `getWordRangeAtPosition` e `lineAt` lançam; aqui devolvem [`Result`]. O `ok(!this._isDisposed)` dos
//!   `_accept*` e do `dispose` é uma asserção que lança (pânico com [`Error`]), como no original.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::assert::ok;
use je_base::errors::Error;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::{is_js_whitespace, substring};
use je_base::network::schemas;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::strings::reg_exp_leads_to_endless_loop;
use je_base::uri::URI;
use je_editor::mirror_text_model::MirrorTextModel;
use je_editor::model::Eol;
use je_editor::word_helper::{ensure_valid_word_definition, get_word_at_text};

use super::ext_host_types::{EndOfLine, Position, Range};

thread_local! {
    static LANGUAGE_ID_TO_WORD_DEFINITION: RefCell<HashMap<String, Rc<JsRegExp>>> = RefCell::new(HashMap::new());
}

/// `setWordDefinitionFor`: sem definição, remove a do idioma.
pub fn set_word_definition_for(language_id: &str, word_definition: Option<Rc<JsRegExp>>) {
    LANGUAGE_ID_TO_WORD_DEFINITION.with(|definitions| {
        let mut definitions = definitions.borrow_mut();
        match word_definition {
            None => {
                definitions.remove(language_id);
            }
            Some(word_definition) => {
                definitions.insert(language_id.to_owned(), word_definition);
            }
        }
    });
}

fn get_word_definition_for(language_id: &str) -> Option<Rc<JsRegExp>> {
    LANGUAGE_ID_TO_WORD_DEFINITION.with(|definitions| definitions.borrow().get(language_id).cloned())
}

/// `IExtHostDocumentSaveDelegate`.
pub trait IExtHostDocumentSaveDelegate {
    fn rpc_try_save_document(&self, uri: &URI) -> Promise<bool>;
}

/// `number | vscode.Position`: o argumento de `lineAt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineOrPosition {
    Line(f64),
    Position(Position),
}

impl From<f64> for LineOrPosition {
    fn from(line: f64) -> Self {
        LineOrPosition::Line(line)
    }
}

impl From<Position> for LineOrPosition {
    fn from(position: Position) -> Self {
        LineOrPosition::Position(position)
    }
}

/// O estado do documento, dividido entre o [`ExtHostDocumentData`] (que o atualiza) e o [`TextDocument`]
/// (que a extensão lê).
struct DocumentState {
    proxy: Rc<dyn IExtHostDocumentSaveDelegate>,
    uri: URI,
    mirror: RefCell<MirrorTextModel>,
    language_id: RefCell<String>,
    is_dirty: Cell<bool>,
    encoding: RefCell<String>,
    is_disposed: Cell<bool>,
}

/// `ExtHostDocumentData`.
pub struct ExtHostDocumentData {
    document: TextDocument,
}

impl ExtHostDocumentData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proxy: Rc<dyn IExtHostDocumentSaveDelegate>,
        uri: URI,
        lines: Vec<JsString>,
        eol: Eol,
        version_id: i64,
        language_id: &str,
        is_dirty: bool,
        encoding: &str,
    ) -> ExtHostDocumentData {
        let mirror = MirrorTextModel::new(Some(uri.clone()), lines, eol, version_id);
        ExtHostDocumentData {
            document: TextDocument(Rc::new(DocumentState {
                proxy,
                uri,
                mirror: RefCell::new(mirror),
                language_id: RefCell::new(language_id.to_owned()),
                is_dirty: Cell::new(is_dirty),
                encoding: RefCell::new(encoding.to_owned()),
                is_disposed: Cell::new(false),
            })),
        }
    }

    /// O espelho do modelo (`MirrorTextModel`): `getText()`, `version`, `lines`...
    pub fn mirror(&self) -> Ref<'_, MirrorTextModel> {
        self.document.0.mirror.borrow()
    }

    /// O espelho para aplicar eventos (`onEvents`).
    pub fn mirror_mut(&self) -> RefMut<'_, MirrorTextModel> {
        self.document.0.mirror.borrow_mut()
    }

    /// Não descarta de verdade, para que as extensões ainda leiam o documento: só marca como fechado e
    /// limpo (algumas operações, como salvar, passam a falhar).
    pub fn dispose(&self) {
        let state = &self.document.0;
        ok(&!state.is_disposed.get(), None);
        state.is_disposed.set(true);
        state.is_dirty.set(false);
    }

    pub fn equal_lines(&self, lines: &[JsString]) -> bool {
        self.mirror().lines() == lines
    }

    /// `document`: o `vscode.TextDocument` que a extensão enxerga.
    pub fn document(&self) -> TextDocument {
        self.document.clone()
    }

    pub fn accept_language_id(&self, new_language_id: &str) {
        let state = &self.document.0;
        ok(&!state.is_disposed.get(), None);
        *state.language_id.borrow_mut() = new_language_id.to_owned();
    }

    pub fn accept_is_dirty(&self, is_dirty: bool) {
        let state = &self.document.0;
        ok(&!state.is_disposed.get(), None);
        state.is_dirty.set(is_dirty);
    }

    pub fn accept_encoding(&self, encoding: &str) {
        let state = &self.document.0;
        ok(&!state.is_disposed.get(), None);
        *state.encoding.borrow_mut() = encoding.to_owned();
    }
}

/// `vscode.TextDocument`.
#[derive(Clone)]
pub struct TextDocument(Rc<DocumentState>);

impl TextDocument {
    /// O `===` do original: o mesmo documento.
    pub fn ptr_eq(a: &TextDocument, b: &TextDocument) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    pub fn uri(&self) -> &URI {
        &self.0.uri
    }

    /// `fileName`: o `fsPath` da `uri`.
    pub fn file_name(&self) -> JsString {
        self.0.uri.fs_path(Platform::detect().path_flavor())
    }

    pub fn is_untitled(&self) -> bool {
        self.0.uri.scheme() == schemas::UNTITLED
    }

    pub fn language_id(&self) -> String {
        self.0.language_id.borrow().clone()
    }

    pub fn version(&self) -> f64 {
        self.0.mirror.borrow().version() as f64
    }

    pub fn is_closed(&self) -> bool {
        self.0.is_disposed.get()
    }

    pub fn is_dirty(&self) -> bool {
        self.0.is_dirty.get()
    }

    pub fn encoding(&self) -> String {
        self.0.encoding.borrow().clone()
    }

    pub fn eol(&self) -> EndOfLine {
        match self.0.mirror.borrow().eol() {
            Eol::Lf => EndOfLine::LF,
            Eol::CrLf => EndOfLine::CRLF,
        }
    }

    pub fn line_count(&self) -> f64 {
        self.0.mirror.borrow().lines().len() as f64
    }

    pub fn save(&self) -> Promise<bool> {
        if self.0.is_disposed.get() {
            return Promise::rejected(Error::new("Document has been closed"));
        }
        self.0.proxy.rpc_try_save_document(&self.0.uri)
    }

    /// `getText(range?)`: o texto todo, ou o do intervalo (validado contra o documento).
    pub fn get_text(&self, range: Option<&Range>) -> JsString {
        let Some(range) = range else {
            return self.0.mirror.borrow().get_text();
        };
        let range = self.validate_range(range);

        if range.is_empty() {
            return JsString::new();
        }

        let mirror = self.0.mirror.borrow();
        let lines = mirror.lines();
        let (start, end) = (range.start(), range.end());
        let (start_line, end_line) = (start.line() as usize, end.line() as usize);
        let units_of = |line: usize, from: f64, to: f64| substring(lines[line].units(), from as i64, to as i64);

        if range.is_single_line() {
            return JsString::from(units_of(start_line, start.character(), end.character()));
        }

        let mut result: Vec<&[u16]> = Vec::with_capacity(end_line - start_line + 1);
        result.push(units_of(start_line, start.character(), f64::MAX));
        result.extend(lines[start_line + 1..end_line].iter().map(JsString::units));
        result.push(units_of(end_line, 0.0, end.character()));
        JsString::from_units(result.join(mirror.eol().as_units()))
    }

    /// `lineAt(lineOrPosition)`: lança `Illegal value for `line`` com uma linha fora do documento ou
    /// fracionária.
    pub fn line_at(&self, line_or_position: LineOrPosition) -> Result<ExtHostDocumentLine, Error> {
        let line = match line_or_position {
            LineOrPosition::Line(line) => line,
            LineOrPosition::Position(position) => position.line(),
        };

        let mirror = self.0.mirror.borrow();
        let lines = mirror.lines();
        // escrito como a negação do que é válido, para que `NaN` também seja recusado
        if !(line >= 0.0 && line < lines.len() as f64 && line.floor() == line) {
            return Err(Error::new("Illegal value for `line`"));
        }

        let index = line as usize;
        Ok(ExtHostDocumentLine { line, text: lines[index].clone(), is_last_line: index == lines.len() - 1 })
    }

    pub fn offset_at(&self, position: &Position) -> f64 {
        let position = self.validate_position(position);
        let mut mirror = self.0.mirror.borrow_mut();
        let prefix_sum = mirror.ensure_line_starts().get_prefix_sum(position.line() as i64 - 1);
        f64::from(prefix_sum) + position.character()
    }

    pub fn position_at(&self, offset: f64) -> Position {
        let offset = offset.floor().max(0.0);

        let mut mirror = self.0.mirror.borrow_mut();
        // a conversão de `f64` para `i64` satura, como o `Number.MAX_VALUE` do original passa do fim
        let out = mirror.ensure_line_starts().get_index_of(offset as i64);
        let line_length = mirror.lines()[out.index].len() as i64;

        // garante uma posição válida
        Position::new(out.index as f64, out.remainder.min(line_length) as f64).expect("linha e caractere são não negativos")
    }

    // ---- range math

    pub fn validate_range(&self, range: &Range) -> Range {
        let start = self.validate_position(&range.start());
        let end = self.validate_position(&range.end());

        if start == range.start() && end == range.end() {
            return *range;
        }
        Range::new(start, end)
    }

    /// Leva a posição para dentro do documento. Uma linha negativa não existe (o [`Position`] já recusa).
    pub fn validate_position(&self, position: &Position) -> Position {
        let mirror = self.0.mirror.borrow();
        let lines = mirror.lines();
        let origin = Position::new(0.0, 0.0).expect("a origem é válida");
        if lines.is_empty() {
            return origin;
        }

        let (mut line, mut character) = (position.line(), position.character());
        let mut has_changed = false;

        if line >= lines.len() as f64 {
            line = (lines.len() - 1) as f64;
            character = lines[line as usize].len() as f64;
            has_changed = true;
        } else {
            let max_character = lines[line as usize].len() as f64;
            if character > max_character {
                character = max_character;
                has_changed = true;
            }
        }

        if !has_changed {
            return *position;
        }
        Position::new(line, character).expect("linha e caractere são não negativos")
    }

    /// `getWordRangeAtPosition(position, regexp?)`: sem `regexp`, vale a definição de palavra do idioma;
    /// uma que casa a cadeia vazia lança.
    pub fn get_word_range_at_position(&self, position: &Position, regexp: Option<&Rc<JsRegExp>>) -> Result<Option<Range>, Error> {
        let position = self.validate_position(position);

        let regexp = match regexp {
            // usa o padrão quando nenhum regexp foi dado
            None => get_word_definition_for(&self.0.language_id.borrow()),
            // um regexp ruim é recusado
            Some(regexp) if reg_exp_leads_to_endless_loop(regexp) => {
                return Err(Error::new(format!(
                    "[getWordRangeAtPosition]: ignoring custom regexp '{}' because it matches the empty string.",
                    JsString::from(regexp.source())
                )));
            }
            Some(regexp) => Some(Rc::clone(regexp)),
        };

        let mirror = self.0.mirror.borrow();
        let word_at_text = get_word_at_text(
            position.character() as i32 + 1,
            &ensure_valid_word_definition(regexp.as_ref()),
            mirror.lines()[position.line() as usize].units(),
            0,
            None,
        );

        Ok(word_at_text.map(|word| {
            let line = position.line();
            Range::from_coordinates(line, f64::from(word.start_column - 1), line, f64::from(word.end_column - 1)).expect("colunas não negativas")
        }))
    }

    /// `[Symbol.for('debug.description')]()`.
    pub fn debug_description(&self) -> String {
        format!("TextDocument({})", self.0.uri.to_string(false))
    }
}

/// `ExtHostDocumentLine`: um instantâneo de uma linha (não acompanha as edições seguintes).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtHostDocumentLine {
    line: f64,
    text: JsString,
    is_last_line: bool,
}

impl ExtHostDocumentLine {
    pub fn new(line: f64, text: JsString, is_last_line: bool) -> ExtHostDocumentLine {
        ExtHostDocumentLine { line, text, is_last_line }
    }

    pub fn line_number(&self) -> f64 {
        self.line
    }

    pub fn text(&self) -> &JsString {
        &self.text
    }

    pub fn range(&self) -> Range {
        Range::from_coordinates(self.line, 0.0, self.line, self.text.len() as f64).expect("linha e coluna não negativas")
    }

    pub fn range_including_line_break(&self) -> Range {
        if self.is_last_line {
            return self.range();
        }
        Range::from_coordinates(self.line, 0.0, self.line + 1.0, 0.0).expect("linha e coluna não negativas")
    }

    /// O comprimento do espaço em branco do começo (`/^(\s*)/`).
    pub fn first_non_whitespace_character_index(&self) -> f64 {
        // o original anota que o nome da API deveria ser `leadingWhitespaceLength`
        self.text.units().iter().take_while(|unit| is_js_whitespace(**unit)).count() as f64
    }

    pub fn is_empty_or_whitespace(&self) -> bool {
        self.first_non_whitespace_character_index() == self.text.len() as f64
    }
}
