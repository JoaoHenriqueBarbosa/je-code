//! Os documentos e os editores de texto no `vscode`: em `vscode.window`, `activeTextEditor`, `visibleTextEditors`,
//! `onDidChangeActiveTextEditor`, `onDidChangeVisibleTextEditors`, `onDidChangeTextEditorSelection`, `onDidChangeTextEditorOptions`,
//! `onDidChangeTextEditorVisibleRanges`, `onDidChangeTextEditorViewColumn` e `showTextDocument`; em `vscode.workspace`,
//! `textDocuments`, `openTextDocument`, `onDidOpenTextDocument`, `onDidCloseTextDocument`, `onDidChangeTextDocument` e
//! `onDidSaveTextDocument`; e as classes `TextDocument`, `TextEditor` e `TextEditorEdit` (a que o `callback` de `TextEditor#edit` recebe).
//!
//! É a ponte para o `ExtHostDocumentsAndEditors`, o `ExtHostDocuments` e o `ExtHostEditors` portados (`extHostDocuments.ts`,
//! `extHostTextEditors.ts`, `extHostTextEditor.ts`, `extHostDocumentData.ts`). Os três só existem quando o binder registrou os clientes
//! `MainThreadDocuments` e `MainThreadTextEditors` (o `MainThreadDocumentsAndEditors` registra os dois).
//!
//! `window.createTextEditorDecorationType`, `TextEditor#setDecorations` e `TextEditor#insertSnippet` também estão aqui.
//!
//! Falta (o serviço do host tem, falta só a ponte): `TextEditor#options` com `cursorStyle` e `lineNumbers`, `diffInformation`,
//! `workspace.onWillSaveTextDocument`, `registerTextDocumentContentProvider` (`workspace.notebookDocuments` e os membros de notebook
//! estão em `notebooks`).
//!
//! Divergências:
//! - O motor entrega objeto comum por cópia e só função, promessa e instância de classe nativa por identidade, então `TextDocument`,
//!   `TextEditor` e `TextEditorEdit` são instâncias de classes nativas que carregam só um `_id`, com acessores no protótipo (como o
//!   `StatusBarItem`). O `_id` do documento é um número da [`TextObjects`], e o do editor é o id do `ExtHostTextEditor`. A mesma
//!   `TextDocument` volta com o mesmo `_id` (a leitura de qualquer propriedade vê o mesmo estado vivo), mas cada leitura de
//!   `activeTextEditor` ou `textDocuments` cria um objeto JavaScript novo: `a.document === b.document` falha, comparar por
//!   `document.uri.toString()` funciona.
//! - A [`TextObjects`] guarda cada documento e editor que já atravessou a fronteira e não os solta: um objeto que a extensão guardou
//!   depois de o documento fechar continua legível (`isClosed` vale `true`), como no original, ao custo de a memória do espelho do
//!   documento só ser liberada com o host.
//! - `TextEditor#options` devolve um objeto copiado com `tabSize`, `indentSize` e `insertSpaces`: `editor.options.tabSize = 2` não tem
//!   efeito, e `editor.options = { tabSize: 2 }` (o `assign` do original) tem.
//! - `createTextEditorDecorationType` devolve um objeto com `key` e `dispose` (a classe `Disposable`); a extensão chamadora é `je.exthost`.
//!   `insertSnippet` e `setDecorations` aceitam um literal `{ value }` onde o original pede a `SnippetString`, e o `setDecorations` sem
//!   lista de intervalos lança em vez de falhar adiante.
//! - `TextEditor#edit`: se o `callback` lança, a exceção sobe por `edit` (como no original), mas as edições que ele já coletou seguem
//!   para o editor, porque o porte aplica no mesmo passo em que chama o `callback`.
//! - `TextDocument#getWordRangeAtPosition` não aceita o argumento `regexp` (o motor entrega a `RegExp` como objeto opaco, e o
//!   `JsRegExp` do Rust não se constrói a partir dele): passá-lo lança. Sem `regexp` vale a definição de palavra do idioma.
//! - `showTextDocument` com uma `Uri` abre o documento antes (`workspace.openTextDocument`), como o `extHost.api.impl.ts`; o aviso
//!   de depreciação de `vscode-remote` sem autoridade não existe.
//! - A extensão chamadora é sempre `je.exthost` (o `vscode` é um só para todas), então `openTextDocument` não registra a origem no log.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::event::Event;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_workbench::api::common::ext_host_document_data::{ExtHostDocumentLine, LineOrPosition, TextDocument};
use je_workbench::api::common::ext_host_documents::{ExtHostDocuments, TextDocumentChangeEvent};
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_editors::{
    ExtHostEditors, ShowTextDocumentArgument, TextEditorOptionsChangeEvent, TextEditorSelectionChangeEvent, TextEditorViewColumnChangeEvent,
    TextEditorVisibleRangesChangeEvent,
};
use je_workbench::api::common::ext_host_protocol::{CreateDocumentOptions, OpenDocumentOptions, SnippetOptions, UndoStopOptions};
use je_workbench::api::common::ext_host_text_editor::{
    DecorationTypeHandle, EditLocation, RangeOrSelection, SnippetLocation, TextEditor, TextEditorEdit, TextEditorOptionsInput,
};
use je_workbench::api::common::ext_host_types::{
    ColorValue, DecorationInstanceRenderOptions, DecorationOptions, DecorationRangeBehavior, DecorationRanges, DecorationRenderOptions, EndOfLine, HoverMessage,
    PathOrUri, RangeOrPosition, Selection, SnippetString, TextDocumentChangeReason, TextEditorRevealType, TextEditorSelectionChangeKind,
    ThemableDecorationAttachmentRenderOptions, ThemableDecorationInstanceRenderOptions, ThemableDecorationRenderOptions,
};
use je_editor::model::OverviewRulerLane;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::promise_to_js;
use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::language_types::marked_string_from_js;
use crate::api::languages::host_extension;
use crate::api::types::{position_data, range_data, selection_data, to_position, to_range, to_selection};
use crate::api::window::theme_color_of;
use crate::convert::{class_name, field, js_to_dynamic, js_to_error, opt_bool, opt_number, opt_string, string_arg, uri_from_js, uri_to_js};

/// Os serviços do host que a ponte usa.
#[derive(Clone)]
pub struct TextEditorServices {
    pub documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    pub documents: Rc<ExtHostDocuments>,
    pub editors: Rc<ExtHostEditors>,
}

/// Os serviços e a tabela de objetos, que `window` e `workspace` compartilham.
#[derive(Clone)]
pub struct TextEditorApi {
    pub services: TextEditorServices,
    pub objects: Rc<TextObjects>,
}

/// Os documentos, os editores e os construtores de edição que o JavaScript segura, por id (os objetos carregam só o id).
#[derive(Default)]
pub struct TextObjects {
    next: Cell<u64>,
    documents: RefCell<HashMap<u64, TextDocument>>,
    documents_by_uri: RefCell<HashMap<String, Vec<u64>>>,
    editors: RefCell<HashMap<String, TextEditor>>,
    edits: RefCell<HashMap<u64, Rc<TextEditorEdit>>>,
    finished_edits: RefCell<HashSet<u64>>,
    decoration_types: RefCell<HashMap<String, DecorationTypeHandle>>,
}

fn instance(class: &str, id: JsData) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), id)])
}

impl TextObjects {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn next_id(&self) -> u64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        id
    }

    /// O `vscode.TextDocument` do documento: o mesmo documento volta com o mesmo `_id`.
    pub fn document_to_js(&self, document: &TextDocument) -> JsData {
        let key = document.uri().to_string(false).to_string_lossy();
        let known = {
            let documents = self.documents.borrow();
            self.documents_by_uri
                .borrow()
                .get(&key)
                .and_then(|ids| ids.iter().copied().find(|id| documents.get(id).is_some_and(|candidate| TextDocument::ptr_eq(candidate, document))))
        };
        let id = known.unwrap_or_else(|| {
            let id = self.next_id();
            self.documents.borrow_mut().insert(id, document.clone());
            self.documents_by_uri.borrow_mut().entry(key).or_default().push(id);
            id
        });
        instance("TextDocument", JsData::Num(id as f64))
    }

    pub(crate) fn document_of(&self, value: &JsData) -> Result<TextDocument, String> {
        match field(value, "_id") {
            Some(JsData::Num(id)) => self.documents.borrow().get(&(*id as u64)).cloned().ok_or_else(|| "TextDocument: documento desconhecido".to_owned()),
            _ => Err("esperava um TextDocument".to_owned()),
        }
    }

    /// O `vscode.TextEditor` do editor. Cada chamada guarda o editor atual do id (um editor refeito com o mesmo id é o mesmo editor
    /// e o mesmo documento).
    pub fn editor_to_js(&self, editor: &TextEditor) -> JsData {
        self.editors.borrow_mut().insert(editor.id().to_owned(), editor.clone());
        instance("TextEditor", JsData::Str(editor.id().to_owned()))
    }

    fn editor_of(&self, value: &JsData) -> Result<TextEditor, String> {
        match field(value, "_id") {
            Some(JsData::Str(id)) => self.editors.borrow().get(id).cloned().ok_or_else(|| "TextEditor: editor desconhecido".to_owned()),
            _ => Err("esperava um TextEditor".to_owned()),
        }
    }

    fn register_edit(&self, edit: Rc<TextEditorEdit>) -> JsData {
        let id = self.next_id();
        self.edits.borrow_mut().insert(id, edit);
        instance("TextEditorEdit", JsData::Num(id as f64))
    }

    /// O `callback` terminou: o construtor deixa de valer (`Edit is only valid while callback runs`).
    fn finish_edit(&self, edit: &JsData) {
        if let Some(JsData::Num(id)) = field(edit, "_id") {
            let id = *id as u64;
            self.edits.borrow_mut().remove(&id);
            self.finished_edits.borrow_mut().insert(id);
        }
    }

    fn edit_of(&self, value: &JsData) -> Result<Rc<TextEditorEdit>, String> {
        let Some(JsData::Num(id)) = field(value, "_id") else { return Err("esperava um TextEditorEdit".to_owned()) };
        let id = *id as u64;
        if let Some(edit) = self.edits.borrow().get(&id) {
            return Ok(edit.clone());
        }
        if self.finished_edits.borrow().contains(&id) {
            return Err("Edit is only valid while callback runs".to_owned());
        }
        Err("TextEditorEdit: construtor desconhecido".to_owned())
    }

    fn decoration_type_of(&self, value: &JsData) -> Result<DecorationTypeHandle, String> {
        match field(value, "key") {
            Some(JsData::Str(key)) => self.decoration_types.borrow().get(key).cloned().ok_or_else(|| "TextEditorDecorationType: tipo de decoração desconhecido".to_owned()),
            _ => Err("esperava um TextEditorDecorationType".to_owned()),
        }
    }
}

// --- decorações e snippets

fn color_value(value: Option<&JsData>) -> Result<Option<ColorValue>, String> {
    match value {
        Some(JsData::Str(text)) => Ok(Some(ColorValue::Text(text.clone()))),
        Some(color @ JsData::Object(_)) => Ok(Some(ColorValue::Theme(theme_color_of(color)?))),
        _ => Ok(None),
    }
}

fn path_or_uri(value: Option<&JsData>) -> Result<Option<PathOrUri>, String> {
    match value {
        Some(JsData::Str(path)) => Ok(Some(PathOrUri::Path(path.clone()))),
        Some(uri @ JsData::Object(_)) => Ok(Some(PathOrUri::Uri(uri_from_js(uri)?))),
        _ => Ok(None),
    }
}

/// O objeto de `name` em `value`, convertido por `convert`; ausente ou não objeto vale `None`.
fn nested<T>(value: &JsData, name: &str, convert: impl Fn(&JsData) -> Result<T, String>) -> Result<Option<T>, String> {
    match field(value, name) {
        Some(inner @ JsData::Object(_)) => convert(inner).map(Some),
        _ => Ok(None),
    }
}

fn attachment_options(value: &JsData) -> Result<ThemableDecorationAttachmentRenderOptions, String> {
    let text = |name: &str| opt_string(field(value, name));
    let color = |name: &str| color_value(field(value, name));
    Ok(ThemableDecorationAttachmentRenderOptions {
        content_text: text("contentText"),
        content_icon_path: path_or_uri(field(value, "contentIconPath"))?,
        border: text("border"),
        border_color: color("borderColor")?,
        font_style: text("fontStyle"),
        font_weight: text("fontWeight"),
        text_decoration: text("textDecoration"),
        color: color("color")?,
        background_color: color("backgroundColor")?,
        margin: text("margin"),
        width: text("width"),
        height: text("height"),
    })
}

fn themable_options(value: &JsData) -> Result<ThemableDecorationRenderOptions, String> {
    let text = |name: &str| opt_string(field(value, name));
    let color = |name: &str| color_value(field(value, name));
    Ok(ThemableDecorationRenderOptions {
        background_color: color("backgroundColor")?,
        outline: text("outline"),
        outline_color: color("outlineColor")?,
        outline_style: text("outlineStyle"),
        outline_width: text("outlineWidth"),
        border: text("border"),
        border_color: color("borderColor")?,
        border_radius: text("borderRadius"),
        border_spacing: text("borderSpacing"),
        border_style: text("borderStyle"),
        border_width: text("borderWidth"),
        font_style: text("fontStyle"),
        font_weight: text("fontWeight"),
        text_decoration: text("textDecoration"),
        cursor: text("cursor"),
        color: color("color")?,
        opacity: text("opacity"),
        letter_spacing: text("letterSpacing"),
        gutter_icon_path: path_or_uri(field(value, "gutterIconPath"))?,
        gutter_icon_size: text("gutterIconSize"),
        overview_ruler_color: color("overviewRulerColor")?,
        before: nested(value, "before", attachment_options)?,
        after: nested(value, "after", attachment_options)?,
    })
}

fn overview_ruler_lane(value: Option<&JsData>) -> Option<OverviewRulerLane> {
    match opt_number(value)? as i64 {
        1 => Some(OverviewRulerLane::Left),
        2 => Some(OverviewRulerLane::Center),
        4 => Some(OverviewRulerLane::Right),
        7 => Some(OverviewRulerLane::Full),
        _ => None,
    }
}

/// `DecorationRenderOptions`: as opções comuns, `isWholeLine`, `rangeBehavior`, `overviewRulerLane`, `light` e `dark`.
fn decoration_render_options(value: &JsData) -> Result<DecorationRenderOptions, String> {
    Ok(DecorationRenderOptions {
        base: themable_options(value)?,
        is_whole_line: opt_bool(field(value, "isWholeLine")),
        range_behavior: opt_number(field(value, "rangeBehavior")).and_then(|behavior| DecorationRangeBehavior::from_value(behavior as i32)),
        overview_ruler_lane: overview_ruler_lane(field(value, "overviewRulerLane")),
        light: nested(value, "light", themable_options)?,
        dark: nested(value, "dark", themable_options)?,
    })
}

fn instance_themable_options(value: &JsData) -> Result<ThemableDecorationInstanceRenderOptions, String> {
    Ok(ThemableDecorationInstanceRenderOptions { before: nested(value, "before", attachment_options)?, after: nested(value, "after", attachment_options)? })
}

fn decoration_instance_options(value: &JsData) -> Result<DecorationInstanceRenderOptions, String> {
    Ok(DecorationInstanceRenderOptions {
        base: instance_themable_options(value)?,
        light: nested(value, "light", instance_themable_options)?,
        dark: nested(value, "dark", instance_themable_options)?,
    })
}

fn hover_message(value: Option<&JsData>) -> Result<Option<HoverMessage>, String> {
    let marked = |item: &JsData| marked_string_from_js(item).map_err(|error| error.message);
    match value {
        Some(JsData::Array(items)) => items.iter().map(marked).collect::<Result<Vec<_>, _>>().map(|items| Some(HoverMessage::Many(items))),
        Some(JsData::Undefined | JsData::Null) | None => Ok(None),
        Some(item) => marked(item).map(|item| Some(HoverMessage::One(item))),
    }
}

/// `DecorationOptions`: `range`, `hoverMessage` e `renderOptions`.
fn decoration_options(value: &JsData) -> Result<DecorationOptions, String> {
    Ok(DecorationOptions {
        range: to_range(field(value, "range").ok_or_else(|| "DecorationOptions.range é obrigatório".to_owned())?)?,
        hover_message: hover_message(field(value, "hoverMessage"))?,
        render_options: nested(value, "renderOptions", decoration_instance_options)?,
    })
}

/// `Range[] | DecorationOptions[]`: a lista é de `DecorationOptions` quando algum item tem `range`.
fn decoration_ranges(value: Option<&JsData>) -> Result<DecorationRanges, String> {
    let Some(JsData::Array(items)) = value else { return Err("setDecorations: a lista de intervalos é obrigatória".to_owned()) };
    if items.iter().any(|item| field(item, "range").is_some()) {
        items.iter().map(decoration_options).collect::<Result<Vec<_>, _>>().map(DecorationRanges::Options)
    } else {
        items.iter().map(to_range).collect::<Result<Vec<_>, _>>().map(DecorationRanges::Ranges)
    }
}

/// `Range | Position` pelo formato do valor (o `Range` tem `start`).
fn range_or_position(value: &JsData) -> Result<RangeOrPosition, String> {
    if field(value, "start").is_some() {
        Ok(RangeOrPosition::Range(to_range(value)?))
    } else {
        Ok(RangeOrPosition::Position(to_position(value)?))
    }
}

/// `Position | Range | readonly Position[] | readonly Range[] | undefined`: o `location` de `insertSnippet`.
fn snippet_location(value: Option<&JsData>) -> Result<Option<SnippetLocation>, String> {
    match value {
        None | Some(JsData::Undefined | JsData::Null) => Ok(None),
        Some(JsData::Array(items)) => items.iter().map(range_or_position).collect::<Result<Vec<_>, _>>().map(|items| Some(SnippetLocation::Many(items))),
        Some(item) => Ok(Some(match range_or_position(item)? {
            RangeOrPosition::Range(range) => SnippetLocation::Range(range),
            RangeOrPosition::Position(position) => SnippetLocation::Position(position),
        })),
    }
}

/// O padrão `{ undoStopBefore: true, undoStopAfter: true }` só vale sem objeto: um objeto parcial deixa o campo ausente `undefined`.
fn snippet_options(value: Option<&JsData>) -> SnippetOptions {
    let Some(options @ JsData::Object(_)) = value else { return SnippetOptions::default() };
    SnippetOptions {
        undo_stop_before: opt_bool(field(options, "undoStopBefore")).unwrap_or(false),
        undo_stop_after: opt_bool(field(options, "undoStopAfter")).unwrap_or(false),
        keep_whitespace: opt_bool(field(options, "keepWhitespace")),
    }
}

// --- TextDocument

fn document_getter(objects: &Rc<TextObjects>, name: &str, read: impl Fn(&TextDocument) -> JsData + 'static) -> (String, MethodCallback) {
    let objects = objects.clone();
    (name.to_owned(), Rc::new(move |this, _| Ok(read(&objects.document_of(this)?))))
}

fn document_method(objects: &Rc<TextObjects>, name: &str, length: u32, body: impl Fn(&TextDocument, &[JsData]) -> Result<JsData, String> + 'static) -> (String, u32, MethodCallback) {
    let objects = objects.clone();
    (name.to_owned(), length, Rc::new(move |this, args| body(&objects.document_of(this)?, args)))
}

fn line_to_js(line: &ExtHostDocumentLine) -> JsData {
    JsData::Object(vec![
        ("lineNumber".to_owned(), JsData::Num(line.line_number())),
        ("text".to_owned(), JsData::Str(line.text().to_string_lossy())),
        ("range".to_owned(), range_data(line.range())),
        ("rangeIncludingLineBreak".to_owned(), range_data(line.range_including_line_break())),
        ("firstNonWhitespaceCharacterIndex".to_owned(), JsData::Num(line.first_non_whitespace_character_index())),
        ("isEmptyOrWhitespace".to_owned(), JsData::Bool(line.is_empty_or_whitespace())),
    ])
}

fn require_arg<'a>(args: &'a [JsData], index: usize, what: &str) -> Result<&'a JsData, String> {
    args.get(index).ok_or_else(|| format!("{what} é obrigatório"))
}

fn define_document_class(runner: &NodeRunner, objects: &Rc<TextObjects>) {
    let getters = vec![
        document_getter(objects, "uri", |document| uri_to_js(document.uri())),
        document_getter(objects, "fileName", |document| JsData::Str(document.file_name().to_string_lossy())),
        document_getter(objects, "isUntitled", |document| JsData::Bool(document.is_untitled())),
        document_getter(objects, "languageId", |document| JsData::Str(document.language_id())),
        document_getter(objects, "version", |document| JsData::Num(document.version())),
        document_getter(objects, "isDirty", |document| JsData::Bool(document.is_dirty())),
        document_getter(objects, "isClosed", |document| JsData::Bool(document.is_closed())),
        document_getter(objects, "eol", |document| JsData::Num(if document.eol() == EndOfLine::LF { 1.0 } else { 2.0 })),
        document_getter(objects, "lineCount", |document| JsData::Num(document.line_count())),
        document_getter(objects, "encoding", |document| JsData::Str(document.encoding())),
    ];
    let save_runner = runner.clone();
    let methods = vec![
        document_method(objects, "save", 0, move |document, _| Ok(promise_to_js(&save_runner, document.save(), |saved| JsData::Bool(*saved)))),
        document_method(objects, "getText", 1, |document, args| {
            let range = match args.first() {
                Some(value @ JsData::Object(_)) => Some(to_range(value)?),
                _ => None,
            };
            Ok(JsData::Str(document.get_text(range.as_ref()).to_string_lossy()))
        }),
        document_method(objects, "lineAt", 1, |document, args| {
            let argument = match require_arg(args, 0, "lineAt: a linha ou a posição")? {
                JsData::Num(line) => LineOrPosition::Line(*line),
                position => LineOrPosition::Position(to_position(position)?),
            };
            document.line_at(argument).map(|line| line_to_js(&line)).map_err(|error| error.message)
        }),
        document_method(objects, "offsetAt", 1, |document, args| Ok(JsData::Num(document.offset_at(&to_position(require_arg(args, 0, "offsetAt: a posição")?)?)))),
        document_method(objects, "positionAt", 1, |document, args| match require_arg(args, 0, "positionAt: o deslocamento")? {
            JsData::Num(offset) => Ok(position_data(document.position_at(*offset))),
            _ => Err("positionAt: o deslocamento precisa ser número".to_owned()),
        }),
        document_method(objects, "validateRange", 1, |document, args| Ok(range_data(document.validate_range(&to_range(require_arg(args, 0, "validateRange: o intervalo")?)?)))),
        document_method(objects, "validatePosition", 1, |document, args| {
            Ok(position_data(document.validate_position(&to_position(require_arg(args, 0, "validatePosition: a posição")?)?)))
        }),
        document_method(objects, "getWordRangeAtPosition", 2, |document, args| {
            let position = to_position(require_arg(args, 0, "getWordRangeAtPosition: a posição")?)?;
            if !matches!(args.get(1), None | Some(JsData::Undefined) | Some(JsData::Null)) {
                return Err("getWordRangeAtPosition: o argumento regexp ainda não é suportado pelo host".to_owned());
            }
            let range = document.get_word_range_at_position(&position, None).map_err(|error| error.message)?;
            Ok(range.map_or(JsData::Undefined, range_data))
        }),
    ];
    runner.define_class(ClassSpec {
        name: "TextDocument".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("TextDocument não tem construtor público".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
}

// --- TextEditorEdit

fn edit_method(objects: &Rc<TextObjects>, name: &str, length: u32, body: impl Fn(&TextEditorEdit, &[JsData]) -> Result<(), String> + 'static) -> (String, u32, MethodCallback) {
    let objects = objects.clone();
    (
        name.to_owned(),
        length,
        Rc::new(move |this, args| {
            body(&*objects.edit_of(this)?, args)?;
            Ok(JsData::Undefined)
        }),
    )
}

/// `Position | Range | Selection`, pelo formato do valor (a `Selection` tem `anchor`, o `Range` tem `start`).
fn edit_location(value: &JsData) -> Result<EditLocation, String> {
    if field(value, "anchor").is_some() {
        Ok(EditLocation::Selection(to_selection(value)?))
    } else if field(value, "start").is_some() {
        Ok(EditLocation::Range(to_range(value)?))
    } else {
        Ok(EditLocation::Position(to_position(value)?))
    }
}

fn range_or_selection(value: &JsData) -> Result<RangeOrSelection, String> {
    if field(value, "anchor").is_some() {
        Ok(RangeOrSelection::Selection(to_selection(value)?))
    } else {
        Ok(RangeOrSelection::Range(to_range(value)?))
    }
}

fn define_edit_class(runner: &NodeRunner, objects: &Rc<TextObjects>) {
    let methods = vec![
        edit_method(objects, "replace", 2, |edit, args| {
            let location = edit_location(require_arg(args, 0, "replace: o local")?)?;
            edit.replace(location, &string_arg(args, 1, "replace: o texto")?).map_err(|error| error.message)
        }),
        edit_method(objects, "insert", 2, |edit, args| {
            let position = to_position(require_arg(args, 0, "insert: a posição")?)?;
            edit.insert(position, &string_arg(args, 1, "insert: o texto")?).map_err(|error| error.message)
        }),
        edit_method(objects, "delete", 1, |edit, args| {
            let location = range_or_selection(require_arg(args, 0, "delete: o local")?)?;
            edit.delete(location).map_err(|error| error.message)
        }),
        edit_method(objects, "setEndOfLine", 1, |edit, args| {
            let end_of_line = match args.first() {
                Some(JsData::Num(value)) if *value == 1.0 => EndOfLine::LF,
                Some(JsData::Num(value)) if *value == 2.0 => EndOfLine::CRLF,
                _ => return Err("Illegal argument: endOfLine".to_owned()),
            };
            edit.set_end_of_line(end_of_line).map_err(|error| error.message)
        }),
    ];
    runner.define_class(ClassSpec {
        name: "TextEditorEdit".to_owned(),
        length: 0,
        construct: Rc::new(|_| Err("TextEditorEdit não tem construtor público: ele chega ao callback de TextEditor#edit".to_owned())),
        methods,
        getters: vec![],
        statics: vec![],
        parent: None,
    });
}

// --- TextEditor

fn editor_getter(objects: &Rc<TextObjects>, name: &str, read: impl Fn(&TextEditor) -> JsData + 'static) -> (String, MethodCallback) {
    let objects = objects.clone();
    (name.to_owned(), Rc::new(move |this, _| Ok(read(&objects.editor_of(this)?))))
}

fn editor_setter(objects: &Rc<TextObjects>, name: &str, write: impl Fn(&TextEditor, &JsData) -> Result<(), String> + 'static) -> (String, MethodCallback) {
    let objects = objects.clone();
    (
        name.to_owned(),
        Rc::new(move |this, args| {
            write(&objects.editor_of(this)?, require_arg(args, 0, "o valor")?)?;
            Ok(JsData::Undefined)
        }),
    )
}

fn editor_method(objects: &Rc<TextObjects>, name: &str, length: u32, body: impl Fn(&TextEditor, &[JsData]) -> Result<JsData, String> + 'static) -> (String, u32, MethodCallback) {
    let objects = objects.clone();
    (name.to_owned(), length, Rc::new(move |this, args| body(&objects.editor_of(this)?, args)))
}

fn options_to_js(editor: &TextEditor) -> JsData {
    let options = editor.options();
    JsData::Object(vec![
        ("tabSize".to_owned(), JsData::Num(options.tab_size())),
        ("indentSize".to_owned(), JsData::Num(options.indent_size())),
        ("insertSpaces".to_owned(), JsData::Bool(options.insert_spaces())),
    ])
}

fn selections_of(value: &JsData) -> Result<Vec<Selection>, String> {
    match value {
        JsData::Array(items) => items.iter().map(to_selection).collect(),
        _ => Err("TextEditor#selections precisa ser uma lista de Selection".to_owned()),
    }
}

fn reveal_type_of(value: Option<&JsData>) -> Option<TextEditorRevealType> {
    match opt_number(value)? as i64 {
        0 => Some(TextEditorRevealType::Default),
        1 => Some(TextEditorRevealType::InCenter),
        2 => Some(TextEditorRevealType::InCenterIfOutsideViewport),
        3 => Some(TextEditorRevealType::AtTop),
        _ => None,
    }
}

fn define_editor_class(runner: &NodeRunner, objects: &Rc<TextObjects>) {
    let document_objects = objects.clone();
    let getters = vec![
        editor_getter(objects, "document", move |editor| document_objects.document_to_js(&editor.document())),
        editor_getter(objects, "selection", |editor| editor.selection().map_or(JsData::Undefined, selection_data)),
        editor_getter(objects, "selections", |editor| JsData::Array(editor.selections().into_iter().map(selection_data).collect())),
        editor_getter(objects, "visibleRanges", |editor| JsData::Array(editor.visible_ranges().into_iter().map(range_data).collect())),
        editor_getter(objects, "viewColumn", |editor| editor.view_column().map_or(JsData::Undefined, JsData::Num)),
        editor_getter(objects, "options", options_to_js),
    ];
    let setters = vec![
        editor_setter(objects, "selection", |editor, value| {
            let _ = editor.set_selection(to_selection(value)?);
            Ok(())
        }),
        editor_setter(objects, "selections", |editor, value| {
            let _ = editor.set_selections(selections_of(value)?);
            Ok(())
        }),
        editor_setter(objects, "options", |editor, value| {
            let assign = |name: &str| field(value, name).map(js_to_dynamic);
            editor.set_options(&TextEditorOptionsInput {
                tab_size: assign("tabSize"),
                indent_size: assign("indentSize"),
                insert_spaces: assign("insertSpaces"),
                ..TextEditorOptionsInput::default()
            });
            Ok(())
        }),
    ];

    let (edit_runner, edit_objects) = (runner.clone(), objects.clone());
    let (snippet_runner, decoration_objects) = (runner.clone(), objects.clone());
    let methods = vec![
        editor_method(objects, "insertSnippet", 3, move |editor, args| {
            let snippet = match require_arg(args, 0, "insertSnippet: o snippet")? {
                snippet @ JsData::Object(_) => SnippetString::new(Some(&string_arg(std::slice::from_ref(field(snippet, "value").unwrap_or(&JsData::Undefined)), 0, "SnippetString.value")?)),
                _ => return Err("insertSnippet: esperava um SnippetString".to_owned()),
            };
            let inserted = editor.insert_snippet(&snippet, snippet_location(args.get(1))?, Some(snippet_options(args.get(2))));
            Ok(promise_to_js(&snippet_runner, inserted, |inserted| JsData::Bool(*inserted)))
        }),
        editor_method(objects, "setDecorations", 2, move |editor, args| {
            let decoration_type = decoration_objects.decoration_type_of(require_arg(args, 0, "setDecorations: o tipo de decoração")?)?;
            editor.set_decorations(&decoration_type, &decoration_ranges(args.get(1))?);
            Ok(JsData::Undefined)
        }),
        editor_method(objects, "edit", 2, move |editor, args| {
            let Some(JsData::Handle(callback)) = args.first().cloned() else { return Err("TextEditor#edit: o callback precisa ser função".to_owned()) };
            let options = match args.get(1) {
                Some(options @ JsData::Object(_)) => Some(UndoStopOptions {
                    undo_stop_before: opt_bool(field(options, "undoStopBefore")).unwrap_or(false),
                    undo_stop_after: opt_bool(field(options, "undoStopAfter")).unwrap_or(false),
                }),
                _ => None,
            };
            let thrown: Rc<RefCell<Option<String>>> = Rc::default();
            let promise = editor.edit(
                {
                    let (runner, objects, thrown) = (edit_runner.clone(), edit_objects.clone(), thrown.clone());
                    move |builder| {
                        let edit = objects.register_edit(builder);
                        let outcome = runner.call_data_throwing(&callback, std::slice::from_ref(&edit));
                        objects.finish_edit(&edit);
                        if let Err(reason) = outcome {
                            *thrown.borrow_mut() = Some(js_to_error(&reason).message);
                        }
                    }
                },
                options,
            );
            if let Some(message) = thrown.borrow_mut().take() {
                return Err(message);
            }
            Ok(promise_to_js(&edit_runner, promise, |applied| JsData::Bool(*applied)))
        }),
        editor_method(objects, "revealRange", 2, |editor, args| {
            let range = to_range(require_arg(args, 0, "revealRange: o intervalo")?)?;
            editor.reveal_range(&range, reveal_type_of(args.get(1)));
            Ok(JsData::Undefined)
        }),
        editor_method(objects, "show", 1, |editor, args| {
            editor.show(opt_number(args.first()));
            Ok(JsData::Undefined)
        }),
        editor_method(objects, "hide", 0, |editor, _| {
            editor.hide();
            Ok(JsData::Undefined)
        }),
    ];
    runner.define_accessor_class(
        ClassSpec {
            name: "TextEditor".to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("TextEditor não tem construtor público: use window.showTextDocument".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
}

/// As classes `TextDocument`, `TextEditor` e `TextEditorEdit` (nenhuma é exportada em `vscode`: são interfaces no `vscode.d.ts`, e o
/// protótipo existe para os objetos que o host entrega).
pub fn define_classes(runner: &NodeRunner, objects: &Rc<TextObjects>) {
    define_document_class(runner, objects);
    define_edit_class(runner, objects);
    define_editor_class(runner, objects);
}

// --- eventos

/// Um acessor de evento (`vscode.window.onDidChangeActiveTextEditor`...): cada leitura cria a função de evento nova sobre `event()`.
fn event_getter<T: 'static>(
    runner: &NodeRunner,
    table: &Rc<DisposableTable>,
    name: &'static str,
    event: impl Fn() -> Event<T> + 'static,
    to_js: impl Fn(&T) -> JsData + 'static,
) -> (String, MethodCallback) {
    let (runner, table) = (runner.clone(), table.clone());
    let to_js = Rc::new(to_js);
    (
        name.to_owned(),
        Rc::new(move |_, _| {
            let to_js = to_js.clone();
            Ok(event_function(&runner, &table, name, event(), move |value| to_js(value)))
        }),
    )
}

fn editor_event(objects: &TextObjects, editor: &TextEditor, extra: Vec<(&str, JsData)>) -> JsData {
    let mut fields = vec![("textEditor".to_owned(), objects.editor_to_js(editor))];
    fields.extend(extra.into_iter().map(|(name, value)| (name.to_owned(), value)));
    JsData::Object(fields)
}

fn change_kind_value(kind: Option<TextEditorSelectionChangeKind>) -> JsData {
    match kind {
        Some(TextEditorSelectionChangeKind::Keyboard) => JsData::Num(1.0),
        Some(TextEditorSelectionChangeKind::Mouse) => JsData::Num(2.0),
        Some(TextEditorSelectionChangeKind::Command) => JsData::Num(3.0),
        None => JsData::Undefined,
    }
}

fn change_reason_value(reason: Option<TextDocumentChangeReason>) -> JsData {
    match reason {
        Some(TextDocumentChangeReason::Undo) => JsData::Num(1.0),
        Some(TextDocumentChangeReason::Redo) => JsData::Num(2.0),
        None => JsData::Undefined,
    }
}

fn document_change_to_js(objects: &TextObjects, event: &TextDocumentChangeEvent) -> JsData {
    let changes = event
        .content_changes
        .iter()
        .map(|change| {
            JsData::Object(vec![
                ("range".to_owned(), range_data(change.range)),
                ("rangeOffset".to_owned(), JsData::Num(change.range_offset)),
                ("rangeLength".to_owned(), JsData::Num(change.range_length)),
                ("text".to_owned(), JsData::Str(change.text.to_string_lossy())),
            ])
        })
        .collect();
    let mut fields = vec![("document".to_owned(), objects.document_to_js(&event.document)), ("contentChanges".to_owned(), JsData::Array(changes))];
    if let reason @ JsData::Num(_) = change_reason_value(event.reason) {
        fields.push(("reason".to_owned(), reason));
    }
    JsData::Object(fields)
}

// --- window

type Methods = Vec<(String, u32, MethodCallback)>;
type Getters = Vec<(String, MethodCallback)>;

fn show_argument(value: Option<&JsData>) -> Result<ShowTextDocumentArgument, String> {
    match value {
        Some(JsData::Num(column)) => Ok(ShowTextDocumentArgument::Column(*column)),
        Some(options @ JsData::Object(_)) => {
            let selection = match field(options, "selection") {
                Some(selection @ JsData::Object(_)) => Some(to_range(selection)?),
                _ => None,
            };
            Ok(ShowTextDocumentArgument::Options {
                view_column: opt_number(field(options, "viewColumn")),
                preserve_focus: opt_bool(field(options, "preserveFocus")),
                selection,
                preview: opt_bool(field(options, "preview")),
            })
        }
        _ => Ok(ShowTextDocumentArgument::Undefined),
    }
}

/// `showTextDocument(documentOrUri, columnOrOptions?, preserveFocus?)`.
fn show_text_document(runner: &NodeRunner, api: &TextEditorApi, args: &[JsData]) -> Result<JsData, String> {
    let target = require_arg(args, 0, "showTextDocument: o documento ou a URI")?;
    let column_or_options = show_argument(args.get(1))?;
    let preserve_focus = opt_bool(args.get(2));
    let editors = api.services.editors.clone();
    let shown: Promise<TextEditor> = if class_name(target) == Some("Uri") {
        let uri = uri_from_js(target)?;
        api.services
            .documents
            .ensure_document_data(&uri, None)
            .then_maybe(move |data| MaybePromise::Promise(editors.show_text_document(&data.document(), column_or_options, preserve_focus)))
    } else {
        editors.show_text_document(&api.objects.document_of(target)?, column_or_options, preserve_focus)
    };
    let objects = api.objects.clone();
    Ok(promise_to_js(runner, shown, move |editor| objects.editor_to_js(editor)))
}

/// Os membros de `vscode.window` desta ponte.
pub fn window_members(runner: &NodeRunner, api: &TextEditorApi, table: &Rc<DisposableTable>) -> (Methods, Getters) {
    let services = &api.services;
    let objects = &api.objects;
    let mut getters: Getters = Vec::new();

    {
        let (documents_and_editors, objects) = (services.documents_and_editors.clone(), objects.clone());
        getters.push((
            "activeTextEditor".to_owned(),
            Rc::new(move |_, _| Ok(documents_and_editors.active_editor().map_or(JsData::Undefined, |editor| objects.editor_to_js(&editor)))),
        ));
    }
    {
        let (editors, objects) = (services.editors.clone(), objects.clone());
        getters.push((
            "visibleTextEditors".to_owned(),
            Rc::new(move |_, _| Ok(JsData::Array(editors.get_visible_text_editors().iter().map(|editor| objects.editor_to_js(editor)).collect()))),
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeActiveTextEditor",
            move || editors.on_did_change_active_text_editor(),
            move |editor: &Option<TextEditor>| editor.as_ref().map_or(JsData::Undefined, |editor| event_objects.editor_to_js(editor)),
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeVisibleTextEditors",
            move || editors.on_did_change_visible_text_editors(),
            move |visible: &Vec<TextEditor>| JsData::Array(visible.iter().map(|editor| event_objects.editor_to_js(editor)).collect()),
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeTextEditorSelection",
            move || editors.on_did_change_text_editor_selection(),
            move |event: &TextEditorSelectionChangeEvent| {
                let selections = JsData::Array(event.selections.iter().copied().map(selection_data).collect());
                editor_event(&event_objects, &event.text_editor, vec![("selections", selections), ("kind", change_kind_value(event.kind))])
            },
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeTextEditorOptions",
            move || editors.on_did_change_text_editor_options(),
            move |event: &TextEditorOptionsChangeEvent| {
                // `options` do evento é o que o editor guarda neste instante (o `accept_options` já rodou antes de o evento disparar)
                editor_event(&event_objects, &event.text_editor, vec![("options", options_to_js(&event.text_editor))])
            },
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeTextEditorVisibleRanges",
            move || editors.on_did_change_text_editor_visible_ranges(),
            move |event: &TextEditorVisibleRangesChangeEvent| {
                let ranges = JsData::Array(event.visible_ranges.iter().copied().map(range_data).collect());
                editor_event(&event_objects, &event.text_editor, vec![("visibleRanges", ranges)])
            },
        ));
    }
    {
        let (editors, event_objects) = (services.editors.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeTextEditorViewColumn",
            move || editors.on_did_change_text_editor_view_column(),
            move |event: &TextEditorViewColumnChangeEvent| editor_event(&event_objects, &event.text_editor, vec![("viewColumn", JsData::Num(event.view_column))]),
        ));
    }

    let (show_runner, show_api) = (runner.clone(), api.clone());
    let (decoration_editors, decoration_objects, decoration_table) = (services.editors.clone(), objects.clone(), table.clone());
    let methods: Methods = vec![
        ("showTextDocument".to_owned(), 3, Rc::new(move |_, args| show_text_document(&show_runner, &show_api, args))),
        (
            "createTextEditorDecorationType".to_owned(),
            1,
            Rc::new(move |_, args| {
                let options = match args.first() {
                    Some(options @ JsData::Object(_)) => decoration_render_options(options)?,
                    _ => DecorationRenderOptions::default(),
                };
                let handle = decoration_editors.create_text_editor_decoration_type(&host_extension(), &options);
                let key = handle.key().to_owned();
                decoration_objects.decoration_types.borrow_mut().insert(key.clone(), handle.clone());
                let id = decoration_table.add(Rc::new(handle));
                Ok(JsData::Object(vec![
                    (CLASS_KEY.to_owned(), JsData::Str("Disposable".to_owned())),
                    ("_id".to_owned(), JsData::Num(id as f64)),
                    ("key".to_owned(), JsData::Str(key)),
                ]))
            }),
        ),
    ];
    (methods, getters)
}

// --- workspace

/// `openTextDocument(uriOrFileNameOrOptions?, options?)`.
fn open_text_document(runner: &NodeRunner, api: &TextEditorApi, args: &[JsData]) -> Result<JsData, String> {
    let documents = api.services.documents.clone();
    // `options = options ?? uriOrFileNameOrOptions`
    let options_source = match args.get(1) {
        None | Some(JsData::Undefined) | Some(JsData::Null) => args.first(),
        other => other,
    };
    let encoding = options_source.and_then(|options| opt_string(field(options, "encoding")));
    let open_options = encoding.clone().map(|encoding| OpenDocumentOptions { encoding: Some(encoding), ..Default::default() });

    let uri: Promise<URI> = match args.first() {
        Some(JsData::Str(path)) => Promise::resolved(URI::file(path.clone(), je_platform::resources::host_platform())),
        Some(uri @ JsData::Object(_)) if class_name(uri) == Some("Uri") => Promise::resolved(uri_from_js(uri)?),
        None | Some(JsData::Undefined) | Some(JsData::Null) | Some(JsData::Object(_)) => {
            let create = options_source.filter(|options| matches!(options, JsData::Object(_))).map(|options| CreateDocumentOptions {
                language: opt_string(field(options, "language")),
                content: opt_string(field(options, "content")),
                encoding: encoding.clone(),
            });
            documents.create_document_data(create)
        }
        Some(_) => return Err("illegal argument - uriOrFileNameOrOptions".to_owned()),
    };
    let opened = uri.then_maybe(move |uri| {
        MaybePromise::Promise(documents.ensure_document_data(&uri, open_options).then(|data| data.document()))
    });
    let objects = api.objects.clone();
    Ok(promise_to_js(runner, opened, move |document| objects.document_to_js(document)))
}

/// Os membros de `vscode.workspace` desta ponte.
pub fn workspace_members(runner: &NodeRunner, api: &TextEditorApi, table: &Rc<DisposableTable>) -> (Methods, Getters) {
    let documents = &api.services.documents;
    let objects = &api.objects;
    let mut getters: Getters = Vec::new();

    {
        let (documents, objects) = (documents.clone(), objects.clone());
        getters.push((
            "textDocuments".to_owned(),
            Rc::new(move |_, _| Ok(JsData::Array(documents.get_all_document_data().iter().map(|data| objects.document_to_js(&data.document())).collect()))),
        ));
    }
    let events: [(&'static str, fn(&ExtHostDocuments) -> Event<TextDocument>); 3] = [
        ("onDidOpenTextDocument", ExtHostDocuments::on_did_add_document),
        ("onDidCloseTextDocument", ExtHostDocuments::on_did_remove_document),
        ("onDidSaveTextDocument", ExtHostDocuments::on_did_save_document),
    ];
    for (name, pick) in events {
        let (documents, objects) = (documents.clone(), objects.clone());
        getters.push(event_getter(runner, table, name, move || pick(&documents), move |document: &TextDocument| objects.document_to_js(document)));
    }
    {
        let (documents, objects) = (documents.clone(), objects.clone());
        getters.push(event_getter(
            runner,
            table,
            "onDidChangeTextDocument",
            move || documents.on_did_change_document(),
            move |event: &TextDocumentChangeEvent| document_change_to_js(&objects, event),
        ));
    }

    let (open_runner, open_api) = (runner.clone(), api.clone());
    let methods: Methods = vec![("openTextDocument".to_owned(), 2, Rc::new(move |_, args| open_text_document(&open_runner, &open_api, args)))];
    (methods, getters)
}
