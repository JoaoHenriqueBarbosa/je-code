//! O alicerce da decisão de arquitetura do extension host (`wip/notes/exthost-plan.md`, seção "Decisão de
//! arquitetura"): como os valores da API `vscode` cruzam a fronteira entre o Rust e o código das extensões.
//!
//! No original TypeScript uma classe da API é um objeto com protótipo, e o host decide por `instanceof`
//! (`value instanceof extHostTypes.Position`). Aqui o valor que cruza é um [`Dynamic::Object`] com a propriedade
//! reservada `$$class`, que guarda o nome da classe: [`class_of`] lê o nome e [`is_instance_of`] é o
//! `instanceof` (com a herança do original: uma `Selection` é também um `Range`). A ponte com o zjsc, quando
//! existir, cria o objeto JavaScript com o protótipo do registro de classes e preserva `$$class`.
//!
//! Cada tipo de [`super::ext_host_types`] que cruza como valor implementa [`ApiValue`]: `to_dynamic` monta o
//! objeto com as propriedades públicas da classe (as que a extensão enxerga) e `from_dynamic` é o caminho de
//! volta, que confere a classe (`instanceof`) e valida as propriedades como os construtores do original.
//!
//! Também mora aqui o `revive` de `marshalling.ts` sobre [`Dynamic`] ([`revive`]), que o original aplica a
//! `any`, e as pontes de bytes ([`bytes_of`], [`vs_buffer_to_dynamic`]), porque `VSBuffer` e `ArrayBuffer`
//! não são variantes de [`Dynamic`] (o `Uint8Array` é [`Dynamic::TypedArray`]).
//!
//! Divergências:
//! - `$$class` é uma propriedade comum do objeto (o [`Dynamic`] não tem propriedades não enumeráveis), então
//!   um `JSON.stringify` do valor a inclui. As classes de [`CLASSES`] são as únicas que [`class_of`]
//!   reconhece: um nome fora da tabela lê como ausente.
//! - `from_dynamic` exige a classe (`instanceof`). O duck typing do original (`Position.isPosition`,
//!   `Range.isRange`, `URI.isUri` sobre objetos com a forma certa) não existe: o que vem da extensão já
//!   chega marcado pela ponte, e o que vem do protocolo é DTO (ver `ext_host_type_converters`).
//! - O `Uri` cruza como objeto de classe `Uri` com `$mid: 1` e os cinco componentes (sempre presentes, como
//!   as propriedades do `URI`); os caches `fsPath`, `external` e `_sep` do `toJSON` só entram em [`revive`],
//!   que os lê.
//! - `ArrayBuffer` cruza como objeto de classe `ArrayBuffer` com `bytes` (um `Uint8Array`), e `VSBuffer` como
//!   objeto de classe `VSBuffer` com `buffer` (um `Uint8Array`).
//! - `SnippetString` perde o contador interno de tabstops (`_tabstop`), que a extensão não vê; um
//!   `from_dynamic` recomeça em 1.
//! - `Command` é objeto literal no original, sem classe: [`Command::to_dynamic`] não grava `$$class`.
//! - `TreeItem` está em [`CLASSES`] só para o `instanceof TreeItem` de `TreeItem.isTreeItem`: o host de árvores lê
//!   o item que o provedor devolve como [`Dynamic`] (a interface `vscode.TreeItem`, com ou sem a classe), então
//!   `TreeItem` não implementa [`ApiValue`].

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::html_content::{IsTrusted, MarkdownStringTrustedOptions};
use je_base::js_string::JsString;
use je_base::marshalling_ids::MarshalledId;
use je_base::path::Platform;
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::{UriComponents, UriState, URI};

use super::ext_host_types::{
    CallHierarchyItem, Color, Command, Diagnostic, DiagnosticCode, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, EndOfLine, Location,
    MarkdownString, NotebookRange, Position, Range, RangeOrPosition, Selection, SnippetString, SymbolKind, SymbolTag, TextEdit, TypeHierarchyItem,
};

/// A propriedade reservada que guarda o nome da classe.
pub const CLASS_KEY: &str = "$$class";

/// Os nomes de classe que [`class_of`] reconhece.
pub const CLASSES: &[&str] = &[
    "Position",
    "Range",
    "Selection",
    "Location",
    "TextEdit",
    "Uri",
    "ThemeColor",
    "ThemeIcon",
    "MarkdownString",
    "Diagnostic",
    "DiagnosticRelatedInformation",
    "SnippetString",
    "Color",
    "NotebookRange",
    "CallHierarchyItem",
    "TypeHierarchyItem",
    "VSBuffer",
    "ArrayBuffer",
    "TreeItem",
    "TimelineItem",
];

/// `class extends`: a classe da qual `class` herda, se houver.
fn parent_of(class: &str) -> Option<&'static str> {
    match class {
        "Selection" => Some("Range"),
        _ => None,
    }
}

/// Grava `$$class` em `obj` (se for um objeto) e o devolve.
pub fn tag(obj: Dynamic, class: &str) -> Dynamic {
    if let Dynamic::Object(object) = &obj {
        object.set(&JsString::from(CLASS_KEY), Dynamic::string(class));
    }
    obj
}

/// O nome da classe de um valor marcado com [`tag`], se for uma das [`CLASSES`].
pub fn class_of(value: &Dynamic) -> Option<&'static str> {
    let Dynamic::Object(object) = value else {
        return None;
    };
    let Some(Dynamic::String(name)) = object.get(&JsString::from(CLASS_KEY)) else {
        return None;
    };
    CLASSES.iter().copied().find(|class| name == *class)
}

/// `value instanceof Class`, com a herança de [`parent_of`] (uma `Selection` é um `Range`).
pub fn is_instance_of(value: &Dynamic, class: &str) -> bool {
    let mut current = class_of(value);
    while let Some(name) = current {
        if name == class {
            return true;
        }
        current = parent_of(name);
    }
    false
}

// ---- leitura de propriedades ------------------------------------------------------------------------

fn require_class(value: &Dynamic, class: &str) -> Result<(), Error> {
    if is_instance_of(value, class) { Ok(()) } else { Err(Error::type_error(format!("Expected an instance of {class}"))) }
}

fn number(value: &Dynamic, class: &str, key: &str) -> Result<f64, Error> {
    match value.get_str(key) {
        Dynamic::Number(n) => Ok(n),
        _ => Err(Error::type_error(format!("{class}.{key} must be a number"))),
    }
}

fn optional_number(value: &Dynamic, class: &str, key: &str) -> Result<Option<f64>, Error> {
    match value.get_str(key) {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        Dynamic::Number(n) => Ok(Some(n)),
        _ => Err(Error::type_error(format!("{class}.{key} must be a number"))),
    }
}

fn text(value: &Dynamic, class: &str, key: &str) -> Result<String, Error> {
    match value.get_str(key) {
        Dynamic::String(s) => Ok(s.to_string_lossy()),
        _ => Err(Error::type_error(format!("{class}.{key} must be a string"))),
    }
}

fn optional_text(value: &Dynamic, class: &str, key: &str) -> Result<Option<String>, Error> {
    match value.get_str(key) {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        Dynamic::String(s) => Ok(Some(s.to_string_lossy())),
        _ => Err(Error::type_error(format!("{class}.{key} must be a string"))),
    }
}

fn optional_bool(value: &Dynamic, class: &str, key: &str) -> Result<Option<bool>, Error> {
    match value.get_str(key) {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        Dynamic::Bool(flag) => Ok(Some(flag)),
        _ => Err(Error::type_error(format!("{class}.{key} must be a boolean"))),
    }
}

/// `value[key]` quando definido (nem `undefined` nem `null`).
fn defined(value: &Dynamic, key: &str) -> Option<Dynamic> {
    let item = value.get_str(key);
    (!item.is_nullish()).then_some(item)
}

/// O objeto só com as propriedades definidas (uma propriedade `undefined` não existe).
fn object_of(entries: Vec<(&str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
}

/// O valor numérico inteiro de um `enum` da API.
fn enum_value(value: f64) -> Option<i32> {
    (value.fract() == 0.0 && value.abs() <= f64::from(i32::MAX)).then_some(value as i32)
}

// ---- o contrato -------------------------------------------------------------------------------------

/// Um tipo da API que cruza a fronteira como valor.
pub trait ApiValue: Sized {
    /// O nome da classe em `$$class`.
    const CLASS: &'static str;

    /// O valor com as propriedades da classe e `$$class`.
    fn to_dynamic(&self) -> Dynamic;

    /// O caminho de volta: `TypeError` se o valor não é desta classe ou se uma propriedade tem o tipo errado,
    /// e o erro do construtor do original (`illegalArgument`) se o valor é inválido.
    fn from_dynamic(value: &Dynamic) -> Result<Self, Error>;
}

impl ApiValue for Position {
    const CLASS: &'static str = "Position";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("line", Dynamic::Number(self.line())), ("character", Dynamic::Number(self.character()))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<Position, Error> {
        require_class(value, Self::CLASS)?;
        Position::new(number(value, Self::CLASS, "line")?, number(value, Self::CLASS, "character")?)
    }
}

impl ApiValue for Range {
    const CLASS: &'static str = "Range";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("start", self.start().to_dynamic()), ("end", self.end().to_dynamic())]), Self::CLASS)
    }

    /// Aceita também uma `Selection` (herança): o `Range` dela.
    fn from_dynamic(value: &Dynamic) -> Result<Range, Error> {
        require_class(value, Self::CLASS)?;
        Ok(Range::new(Position::from_dynamic(&value.get_str("start"))?, Position::from_dynamic(&value.get_str("end"))?))
    }
}

impl ApiValue for Selection {
    const CLASS: &'static str = "Selection";

    fn to_dynamic(&self) -> Dynamic {
        tag(
            Dynamic::object([
                ("start", self.start().to_dynamic()),
                ("end", self.end().to_dynamic()),
                ("anchor", self.anchor().to_dynamic()),
                ("active", self.active().to_dynamic()),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<Selection, Error> {
        require_class(value, Self::CLASS)?;
        Ok(Selection::new(Position::from_dynamic(&value.get_str("anchor"))?, Position::from_dynamic(&value.get_str("active"))?))
    }
}

impl ApiValue for Location {
    const CLASS: &'static str = "Location";

    fn to_dynamic(&self) -> Dynamic {
        tag(object_of(vec![("uri", self.uri.as_ref().map(ApiValue::to_dynamic)), ("range", self.range.as_ref().map(ApiValue::to_dynamic))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<Location, Error> {
        require_class(value, Self::CLASS)?;
        let uri = defined(value, "uri").map(|uri| URI::from_dynamic(&uri)).transpose()?;
        let range = defined(value, "range").map(|range| Range::from_dynamic(&range)).transpose()?;
        Ok(Location::new(uri, range.map(RangeOrPosition::Range)))
    }
}

impl ApiValue for TextEdit {
    const CLASS: &'static str = "TextEdit";

    fn to_dynamic(&self) -> Dynamic {
        tag(
            object_of(vec![
                ("range", Some(self.range().to_dynamic())),
                ("newText", Some(Dynamic::string(self.new_text()))),
                ("newEol", self.new_eol().map(|eol| Dynamic::from(eol.value()))),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<TextEdit, Error> {
        require_class(value, Self::CLASS)?;
        let mut edit = TextEdit::new(Range::from_dynamic(&value.get_str("range"))?, optional_text(value, Self::CLASS, "newText")?.as_deref());
        if let Some(eol) = optional_number(value, Self::CLASS, "newEol")? {
            let eol = enum_value(eol).and_then(EndOfLine::from_value).ok_or_else(|| Error::type_error("TextEdit.newEol must be an EndOfLine"))?;
            edit.set_new_eol(Some(eol));
        }
        Ok(edit)
    }
}

/// Os componentes de um objeto `Uri` ou `$mid: Uri` (a forma de `URI#toJSON`), com os caches.
fn uri_state_of(value: &Dynamic) -> Result<UriState, Error> {
    let component = |key: &str| -> Result<Option<JsString>, Error> {
        match value.get_str(key) {
            Dynamic::Undefined | Dynamic::Null => Ok(None),
            Dynamic::String(s) if s.is_empty() => Ok(None),
            Dynamic::String(s) => Ok(Some(s)),
            _ => Err(Error::type_error(format!("Uri.{key} must be a string"))),
        }
    };
    Ok(UriState {
        mid: Some(MarshalledId::Uri as i32),
        external: component("external")?,
        fs_path: component("fsPath")?,
        sep: matches!(value.get_str("_sep"), Dynamic::Number(n) if n == 1.0).then_some(1),
        components: UriComponents {
            scheme: component("scheme")?,
            authority: component("authority")?,
            path: component("path")?,
            query: component("query")?,
            fragment: component("fragment")?,
        },
    })
}

/// A plataforma do `URI.revive` do processo.
fn host_platform() -> Platform {
    if cfg!(windows) { Platform::Win32 } else { Platform::Posix }
}

impl ApiValue for URI {
    const CLASS: &'static str = "Uri";

    fn to_dynamic(&self) -> Dynamic {
        tag(
            Dynamic::object([
                ("$mid", Dynamic::from(MarshalledId::Uri as i32)),
                ("scheme", Dynamic::String(self.scheme().clone())),
                ("authority", Dynamic::String(self.authority().clone())),
                ("path", Dynamic::String(self.path().clone())),
                ("query", Dynamic::String(self.query().clone())),
                ("fragment", Dynamic::String(self.fragment().clone())),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<URI, Error> {
        require_class(value, Self::CLASS)?;
        Ok(URI::revive(&uri_state_of(value)?, host_platform()))
    }
}

impl ApiValue for ThemeColor {
    const CLASS: &'static str = "ThemeColor";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("id", Dynamic::string(&self.id))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<ThemeColor, Error> {
        require_class(value, Self::CLASS)?;
        Ok(ThemeColor::new(text(value, Self::CLASS, "id")?))
    }
}

impl ApiValue for ThemeIcon {
    const CLASS: &'static str = "ThemeIcon";

    fn to_dynamic(&self) -> Dynamic {
        tag(object_of(vec![("id", Some(Dynamic::string(&self.id))), ("color", self.color.as_ref().map(ApiValue::to_dynamic))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<ThemeIcon, Error> {
        require_class(value, Self::CLASS)?;
        let color = defined(value, "color").map(|color| ThemeColor::from_dynamic(&color)).transpose()?;
        Ok(ThemeIcon { id: text(value, Self::CLASS, "id")?, color })
    }
}

impl ApiValue for MarkdownString {
    const CLASS: &'static str = "MarkdownString";

    fn to_dynamic(&self) -> Dynamic {
        let is_trusted = self.is_trusted.as_ref().map(|trusted| match trusted {
            IsTrusted::Bool(flag) => Dynamic::Bool(*flag),
            IsTrusted::Options(options) => {
                Dynamic::object([("enabledCommands", Dynamic::array(options.enabled_commands.iter().map(|command| Dynamic::string(command))))])
            }
        });
        tag(
            object_of(vec![
                ("value", Some(Dynamic::String(self.value.clone()))),
                ("isTrusted", is_trusted),
                ("supportThemeIcons", self.support_theme_icons.map(Dynamic::Bool)),
                ("supportHtml", self.support_html.map(Dynamic::Bool)),
                ("supportAlertSyntax", self.support_alert_syntax.map(Dynamic::Bool)),
                ("baseUri", self.base_uri.as_ref().map(ApiValue::to_dynamic)),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<MarkdownString, Error> {
        require_class(value, Self::CLASS)?;
        let Dynamic::String(markdown) = value.get_str("value") else {
            return Err(Error::type_error("MarkdownString.value must be a string"));
        };
        let mut result = MarkdownString::new(None, optional_bool(value, Self::CLASS, "supportThemeIcons")?.unwrap_or(false));
        result.value = markdown;
        result.is_trusted = match value.get_str("isTrusted") {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::Bool(flag) => Some(IsTrusted::Bool(flag)),
            options @ Dynamic::Object(_) => {
                let Dynamic::Array(commands) = options.get_str("enabledCommands") else {
                    return Err(Error::type_error("MarkdownString.isTrusted.enabledCommands must be an array"));
                };
                let enabled_commands = commands
                    .to_vec()
                    .into_iter()
                    .map(|command| match command {
                        Dynamic::String(command) => Ok(command.to_string_lossy()),
                        _ => Err(Error::type_error("MarkdownString.isTrusted.enabledCommands must hold strings")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Some(IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands }))
            }
            _ => return Err(Error::type_error("MarkdownString.isTrusted must be a boolean or an object")),
        };
        result.support_html = optional_bool(value, Self::CLASS, "supportHtml")?;
        result.support_alert_syntax = optional_bool(value, Self::CLASS, "supportAlertSyntax")?;
        result.base_uri = defined(value, "baseUri").map(|uri| URI::from_dynamic(&uri)).transpose()?;
        Ok(result)
    }
}

impl ApiValue for DiagnosticRelatedInformation {
    const CLASS: &'static str = "DiagnosticRelatedInformation";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("location", self.location.to_dynamic()), ("message", Dynamic::string(&self.message))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<DiagnosticRelatedInformation, Error> {
        require_class(value, Self::CLASS)?;
        Ok(DiagnosticRelatedInformation::new(Location::from_dynamic(&value.get_str("location"))?, text(value, Self::CLASS, "message")?))
    }
}

impl ApiValue for Diagnostic {
    const CLASS: &'static str = "Diagnostic";

    fn to_dynamic(&self) -> Dynamic {
        let code = self.code.as_ref().map(|code| match code {
            DiagnosticCode::Text(code) => Dynamic::string(code),
            DiagnosticCode::Number(code) => Dynamic::Number(*code),
        });
        tag(
            object_of(vec![
                ("range", Some(self.range.to_dynamic())),
                ("message", Some(Dynamic::string(&self.message))),
                ("severity", Some(Dynamic::from(self.severity.value()))),
                ("source", self.source.as_deref().map(Dynamic::string)),
                ("code", code),
                (
                    "relatedInformation",
                    self.related_information.as_ref().map(|items| Dynamic::array(items.iter().map(ApiValue::to_dynamic))),
                ),
                ("tags", self.tags.as_ref().map(|tags| Dynamic::array(tags.iter().map(|tag| Dynamic::from(tag.value()))))),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<Diagnostic, Error> {
        require_class(value, Self::CLASS)?;
        let severity = enum_value(number(value, Self::CLASS, "severity")?)
            .and_then(DiagnosticSeverity::from_value)
            .ok_or_else(|| Error::type_error("Diagnostic.severity must be a DiagnosticSeverity"))?;
        let mut diagnostic = Diagnostic::new(Range::from_dynamic(&value.get_str("range"))?, &text(value, Self::CLASS, "message")?, Some(severity))?;
        diagnostic.source = optional_text(value, Self::CLASS, "source")?;
        diagnostic.code = match value.get_str("code") {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::String(code) => Some(DiagnosticCode::Text(code.to_string_lossy())),
            Dynamic::Number(code) => Some(DiagnosticCode::Number(code)),
            _ => return Err(Error::type_error("Diagnostic.code must be a string or a number")),
        };
        diagnostic.related_information = match value.get_str("relatedInformation") {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::Array(items) => Some(items.to_vec().iter().map(DiagnosticRelatedInformation::from_dynamic).collect::<Result<Vec<_>, _>>()?),
            _ => return Err(Error::type_error("Diagnostic.relatedInformation must be an array")),
        };
        diagnostic.tags = match value.get_str("tags") {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::Array(items) => Some(
                items
                    .to_vec()
                    .iter()
                    .map(|item| match item {
                        Dynamic::Number(n) => enum_value(*n).and_then(DiagnosticTag::from_value).ok_or_else(|| Error::type_error("Diagnostic.tags must hold DiagnosticTag values")),
                        _ => Err(Error::type_error("Diagnostic.tags must hold numbers")),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err(Error::type_error("Diagnostic.tags must be an array")),
        };
        Ok(diagnostic)
    }
}

impl ApiValue for SnippetString {
    const CLASS: &'static str = "SnippetString";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("value", Dynamic::string(&self.value))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<SnippetString, Error> {
        require_class(value, Self::CLASS)?;
        Ok(SnippetString::new(Some(&text(value, Self::CLASS, "value")?)))
    }
}

impl ApiValue for Color {
    const CLASS: &'static str = "Color";

    fn to_dynamic(&self) -> Dynamic {
        tag(
            Dynamic::object([
                ("red", Dynamic::Number(self.red)),
                ("green", Dynamic::Number(self.green)),
                ("blue", Dynamic::Number(self.blue)),
                ("alpha", Dynamic::Number(self.alpha)),
            ]),
            Self::CLASS,
        )
    }

    fn from_dynamic(value: &Dynamic) -> Result<Color, Error> {
        require_class(value, Self::CLASS)?;
        Ok(Color::new(
            number(value, Self::CLASS, "red")?,
            number(value, Self::CLASS, "green")?,
            number(value, Self::CLASS, "blue")?,
            number(value, Self::CLASS, "alpha")?,
        ))
    }
}

impl ApiValue for NotebookRange {
    const CLASS: &'static str = "NotebookRange";

    fn to_dynamic(&self) -> Dynamic {
        tag(Dynamic::object([("start", Dynamic::Number(self.start())), ("end", Dynamic::Number(self.end()))]), Self::CLASS)
    }

    fn from_dynamic(value: &Dynamic) -> Result<NotebookRange, Error> {
        require_class(value, Self::CLASS)?;
        NotebookRange::new(number(value, Self::CLASS, "start")?, number(value, Self::CLASS, "end")?)
    }
}

/// `CallHierarchyItem` e `TypeHierarchyItem`: o mesmo formato, as mesmas propriedades (`_sessionId` e `_itemId`
/// são as propriedades de nome privado do original, que o host lê).
macro_rules! hierarchy_item_value {
    ($ty:ident) => {
        impl ApiValue for $ty {
            const CLASS: &'static str = stringify!($ty);

            fn to_dynamic(&self) -> Dynamic {
                tag(
                    object_of(vec![
                        ("_sessionId", self.session_id.as_deref().map(Dynamic::string)),
                        ("_itemId", self.item_id.as_deref().map(Dynamic::string)),
                        ("kind", Some(Dynamic::from(self.kind.value()))),
                        ("tags", self.tags.as_ref().map(|tags| Dynamic::array(tags.iter().map(|tag| Dynamic::from(tag.value()))))),
                        ("name", Some(Dynamic::string(&self.name))),
                        ("detail", self.detail.as_deref().map(Dynamic::string)),
                        ("uri", Some(self.uri.to_dynamic())),
                        ("range", Some(self.range.to_dynamic())),
                        ("selectionRange", Some(self.selection_range.to_dynamic())),
                    ]),
                    Self::CLASS,
                )
            }

            fn from_dynamic(value: &Dynamic) -> Result<$ty, Error> {
                require_class(value, Self::CLASS)?;
                let kind = enum_value(number(value, Self::CLASS, "kind")?)
                    .and_then(SymbolKind::from_value)
                    .ok_or_else(|| Error::type_error(concat!(stringify!($ty), ".kind must be a SymbolKind")))?;
                let mut item = $ty::new(
                    kind,
                    &text(value, Self::CLASS, "name")?,
                    "",
                    URI::from_dynamic(&value.get_str("uri"))?,
                    Range::from_dynamic(&value.get_str("range"))?,
                    Range::from_dynamic(&value.get_str("selectionRange"))?,
                );
                item.detail = optional_text(value, Self::CLASS, "detail")?;
                item.session_id = optional_text(value, Self::CLASS, "_sessionId")?;
                item.item_id = optional_text(value, Self::CLASS, "_itemId")?;
                item.tags = match value.get_str("tags") {
                    Dynamic::Undefined | Dynamic::Null => None,
                    Dynamic::Array(tags) => Some(
                        tags.to_vec()
                            .iter()
                            .map(|tag| match tag {
                                Dynamic::Number(n) => enum_value(*n)
                                    .and_then(SymbolTag::from_value)
                                    .ok_or_else(|| Error::type_error(concat!(stringify!($ty), ".tags must hold SymbolTag values"))),
                                _ => Err(Error::type_error(concat!(stringify!($ty), ".tags must hold numbers"))),
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    _ => return Err(Error::type_error(concat!(stringify!($ty), ".tags must be an array"))),
                };
                Ok(item)
            }
        }
    };
}

hierarchy_item_value!(CallHierarchyItem);
hierarchy_item_value!(TypeHierarchyItem);

impl Command {
    /// O objeto literal de `vscode.Command` (`{ title, command, tooltip?, arguments? }`), sem `$$class`.
    pub fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("title", Some(Dynamic::string(&self.title))),
            ("command", Some(Dynamic::string(&self.command))),
            ("tooltip", self.tooltip.as_deref().map(Dynamic::string)),
            ("arguments", self.arguments.as_ref().map(|arguments| Dynamic::array(arguments.iter().cloned()))),
        ])
    }

    /// O caminho de volta. Um `title` ausente lê como `''` (o `ICommandDto` de um comando de API pode não ter).
    pub fn from_dynamic(value: &Dynamic) -> Result<Command, Error> {
        if value.type_of() != "object" || value.is_null() {
            return Err(Error::type_error("Command must be an object"));
        }
        let arguments = match value.get_str("arguments") {
            Dynamic::Undefined | Dynamic::Null => None,
            Dynamic::Array(items) => Some(items.to_vec()),
            _ => return Err(Error::type_error("Command.arguments must be an array")),
        };
        Ok(Command {
            title: optional_text(value, "Command", "title")?.unwrap_or_default(),
            command: text(value, "Command", "command")?,
            tooltip: optional_text(value, "Command", "tooltip")?,
            arguments,
        })
    }
}

// ---- bytes ------------------------------------------------------------------------------------------

/// `new VSBuffer(...)` como valor: objeto de classe `VSBuffer` com `buffer` (um `Uint8Array` com os bytes).
pub fn vs_buffer_to_dynamic(buffer: &VSBuffer) -> Dynamic {
    tag(Dynamic::object([("buffer", Dynamic::typed_array(buffer.to_vec()))]), "VSBuffer")
}

/// `new ArrayBuffer(...)` como valor: objeto de classe `ArrayBuffer` com `bytes` (um `Uint8Array`).
pub fn array_buffer_to_dynamic(bytes: Vec<u8>) -> Dynamic {
    tag(Dynamic::object([("bytes", Dynamic::typed_array(bytes))]), "ArrayBuffer")
}

/// O `VSBuffer` de um valor de classe `VSBuffer`.
pub fn vs_buffer_from_dynamic(value: &Dynamic) -> Result<VSBuffer, Error> {
    require_class(value, "VSBuffer")?;
    match value.get_str("buffer") {
        Dynamic::TypedArray(bytes) => Ok(VSBuffer::wrap(bytes.to_vec())),
        _ => Err(Error::type_error("VSBuffer.buffer must be a Uint8Array")),
    }
}

/// Os bytes de um `Uint8Array`, de um `ArrayBuffer` ou de um `VSBuffer`; `None` para qualquer outro valor.
pub fn bytes_of(value: &Dynamic) -> Option<Vec<u8>> {
    match value {
        Dynamic::TypedArray(bytes) => Some(bytes.to_vec()),
        Dynamic::Object(_) => match (class_of(value), value.get_str("bytes"), value.get_str("buffer")) {
            (Some("ArrayBuffer"), Dynamic::TypedArray(bytes), _) | (Some("VSBuffer"), _, Dynamic::TypedArray(bytes)) => Some(bytes.to_vec()),
            _ => None,
        },
        _ => None,
    }
}

// ---- revive -----------------------------------------------------------------------------------------

/// `new Date(source)` do `revive`: um número de milissegundos ou uma string ISO 8601; qualquer outra coisa é `NaN`.
fn date_from(source: &Dynamic) -> f64 {
    match source {
        Dynamic::Number(n) if n.is_finite() && n.abs() <= 8.64e15 => n.trunc(),
        Dynamic::String(s) => s.to_string_lossy().parse::<jiff::Timestamp>().map_or(f64::NAN, |timestamp| timestamp.as_millisecond() as f64),
        _ => f64::NAN,
    }
}

/// `revive` de `marshalling.ts`: troca, em profundidade e no próprio valor, os objetos `$mid` pelo que
/// representam (`Uri` marcado com `$$class`, `RegExp`, `Date`); `VSBuffer` e `Uint8Array` passam como estão.
pub fn revive(obj: &Dynamic) -> Dynamic {
    revive_at(obj, 0)
}

fn revive_at(obj: &Dynamic, depth: usize) -> Dynamic {
    let falsy = match obj {
        Dynamic::Undefined | Dynamic::Null => true,
        Dynamic::Bool(flag) => !flag,
        Dynamic::Number(n) => *n == 0.0 || n.is_nan(),
        Dynamic::String(s) => s.is_empty(),
        _ => false,
    };
    if falsy || depth > 200 {
        return obj.clone();
    }
    match obj {
        Dynamic::Array(items) => {
            for (index, item) in items.to_vec().iter().enumerate() {
                items.set(index, revive_at(item, depth + 1));
            }
            obj.clone()
        }
        Dynamic::Object(object) => {
            if let Dynamic::Number(mid) = obj.get_str("$mid") {
                match MarshalledId::from_i32(mid as i32).filter(|id| f64::from(*id as i32) == mid) {
                    Some(MarshalledId::Uri) => {
                        // um objeto `$mid` malformado fica como está, como o `URI.revive` do original tolera
                        if let Ok(state) = uri_state_of(obj) {
                            return URI::revive(&state, host_platform()).to_dynamic();
                        }
                    }
                    Some(MarshalledId::Regexp) => {
                        let part = |key: &str| match obj.get_str(key) {
                            Dynamic::String(s) => s.to_string_lossy(),
                            _ => String::new(),
                        };
                        return Dynamic::regexp(&part("source"), &part("flags"));
                    }
                    Some(MarshalledId::Date) => return Dynamic::date(date_from(&obj.get_str("source"))),
                    _ => {}
                }
            }
            if class_of(obj) == Some("VSBuffer") {
                return obj.clone();
            }
            for (key, item) in object.entries() {
                object.set(&key, revive_at(&item, depth + 1));
            }
            obj.clone()
        }
        other => other.clone(),
    }
}
