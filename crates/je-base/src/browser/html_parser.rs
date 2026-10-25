//! Parser de HTML em Rust puro, no lugar do `DOMParser.parseFromString(html, 'text/html')` que o
//! `htmlToMarkdown.ts` usa só para obter uma árvore e depois percorrer os nós. Não há DOM de navegador,
//! nem renderização: o resultado é uma árvore de [`Element`] e [`Node::Text`], e [`parse_body`] devolve o
//! `document.body`.
//!
//! O tokenizador segue o do HTML5 (estados de dados, tag, atributo, comentário, doctype, texto bruto e
//! RCDATA, referências de caractere) e o construtor de árvore segue os modos "antes do body" e "no body"
//! do HTML5 para o que importa a uma conversão de texto.
//!
//! Divergências do `DOMParser`:
//!
//! - o algoritmo de adoção de agência (elementos de formatação mal aninhados) e a reconstrução dos
//!   elementos de formatação ativos não existem: uma tag de fim fecha o elemento aberto de mesmo nome
//!   mais próximo (e tudo que estiver dentro dele), como o "qualquer outra tag de fim" do HTML5;
//! - a árvore de tabelas não é reparada (sem `tbody` implícito nem foster parenting); `td`, `th`, `tr`,
//!   `tbody`, `thead` e `tfoot` fecham os irmãos abertos, e o conteúdo fica na ordem do texto;
//! - conteúdo estrangeiro (`svg`, `math`) é tratado como HTML comum;
//! - os atributos de `html` e `body` são descartados (a conversão nunca os lê) e o `head` não é
//!   representado: o que o HTML5 poria nele (`title`, `meta`, `link`, `style`, `script` antes do corpo)
//!   é consumido e descartado;
//! - o modo de compatibilidade é só "doctype `html` presente ou não" (afeta apenas se `table` fecha um
//!   `p` aberto);
//! - a profundidade da árvore é limitada a 512 elementos abertos, como o Chromium: o elemento além do
//!   limite é anexado ao último aberto, sem ficar aberto (isso também mantém a recursão da conversão
//!   limitada);
//! - a tabela de nomes de entidade é a do HTML 4 (ver `html_entities.rs`); as referências numéricas são
//!   completas, com a troca dos pontos 0x80 a 0x9F pelo windows-1252 e o U+FFFD dos inválidos.

use crate::browser::html_entities::{named_entity_with_semicolon, named_entity_without_semicolon};
use crate::char_code as cc;
use crate::js_string::JsString;

/// Limite de elementos abertos (`kMaximumHTMLParserDOMTreeDepth` do Chromium).
const MAX_OPEN_ELEMENTS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// O nome em minúsculas.
    pub name: String,
    pub value: JsString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Text(JsString),
    /// Um nó de comentário (`<!--...-->` ou comentário falso), com o `data` dele. Só existe no corpo; fora do
    /// `textContent` e de `children`.
    Comment(JsString),
    Element(Element),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// O `tagName` em minúsculas (o `el.tagName.toLowerCase()` do original).
    pub tag_name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
}

impl Element {
    fn new(tag_name: &str, attributes: Vec<Attribute>) -> Self {
        Self { tag_name: tag_name.to_owned(), attributes, children: Vec::new() }
    }

    /// `getAttribute(name)`: o primeiro atributo com o nome (um duplicado é descartado no parse).
    pub fn get_attribute(&self, name: &str) -> Option<&JsString> {
        self.attributes.iter().find(|attribute| attribute.name == name).map(|attribute| &attribute.value)
    }

    /// `textContent`: o texto de todos os descendentes, em ordem.
    pub fn text_content(&self) -> JsString {
        let mut out = JsString::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut JsString) {
        for child in &self.children {
            match child {
                Node::Text(text) => out.push_units(text.units()),
                Node::Element(element) => element.collect_text(out),
                Node::Comment(_) => {}
            }
        }
    }

    /// `querySelector(tagName)`: o primeiro descendente com a tag, em ordem de documento.
    pub fn query_selector(&self, tag_name: &str) -> Option<&Element> {
        self.element_children().find_map(|element| if element.tag_name == tag_name { Some(element) } else { element.query_selector(tag_name) })
    }

    /// `children`: só os filhos que são elementos.
    pub fn element_children(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|child| match child {
            Node::Element(element) => Some(element),
            Node::Text(_) | Node::Comment(_) => None,
        })
    }
}

/// `new DOMParser().parseFromString(html, 'text/html').body`.
pub fn parse_body(html: &[u16]) -> Element {
    let mut parser = Parser { units: normalize_newlines(html), pos: 0, stack: vec![Element::new("body", Vec::new())], in_body: false, skip_line_feed: false, no_quirks: false };
    parser.run();
    parser.finish()
}

/// O pré-processamento do fluxo de entrada: CRLF e CR viram LF.
fn normalize_newlines(html: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        if html[i] == cc::CarriageReturn {
            out.push(cc::LineFeed);
            if html.get(i + 1) == Some(&cc::LineFeed) {
                i += 1;
            }
        } else {
            out.push(html[i]);
        }
        i += 1;
    }
    out
}

fn is_ascii_alpha(unit: u16) -> bool {
    (cc::A..=cc::Z).contains(&unit) || (cc::a..=cc::z).contains(&unit)
}

fn is_ascii_alphanumeric(unit: u16) -> bool {
    is_ascii_alpha(unit) || (cc::Digit0..=cc::Digit9).contains(&unit)
}

/// Os espaços do HTML: tab, LF, FF e espaço (o CR já foi normalizado).
fn is_html_space(unit: u16) -> bool {
    matches!(unit, cc::Tab | cc::LineFeed | 0x0C | cc::Space)
}

fn lower_ascii(units: &[u16]) -> String {
    let lowered: Vec<u16> = units.iter().map(|&unit| if (cc::A..=cc::Z).contains(&unit) { unit + 32 } else { unit }).collect();
    String::from_utf16_lossy(&lowered)
}

fn starts_with_ignore_case(units: &[u16], prefix: &str) -> bool {
    let prefix: Vec<u16> = prefix.encode_utf16().collect();
    units.len() >= prefix.len() && lower_ascii(&units[..prefix.len()]).encode_utf16().eq(prefix.iter().copied())
}

fn find_unit(units: &[u16], from: usize, wanted: u16) -> Option<usize> {
    units.get(from..)?.iter().position(|&unit| unit == wanted).map(|offset| from + offset)
}

// ---------------------------------------------------------------------------------------------
// Referências de caractere.

fn numeric_reference_char(value: u32) -> char {
    // Os pontos 0x80 a 0x9F do HTML são os do windows-1252.
    const WINDOWS_1252: [(u32, u32); 27] = [
        (0x80, 0x20AC),
        (0x82, 0x201A),
        (0x83, 0x0192),
        (0x84, 0x201E),
        (0x85, 0x2026),
        (0x86, 0x2020),
        (0x87, 0x2021),
        (0x88, 0x02C6),
        (0x89, 0x2030),
        (0x8A, 0x0160),
        (0x8B, 0x2039),
        (0x8C, 0x0152),
        (0x8E, 0x017D),
        (0x91, 0x2018),
        (0x92, 0x2019),
        (0x93, 0x201C),
        (0x94, 0x201D),
        (0x95, 0x2022),
        (0x96, 0x2013),
        (0x97, 0x2014),
        (0x98, 0x02DC),
        (0x99, 0x2122),
        (0x9A, 0x0161),
        (0x9B, 0x203A),
        (0x9C, 0x0153),
        (0x9E, 0x017E),
        (0x9F, 0x0178),
    ];
    let mapped = WINDOWS_1252.iter().find(|&&(from, _)| from == value).map_or(value, |&(_, to)| to);
    // `char::from_u32` recusa 0 apenas em outros contextos; aqui o 0, os substitutos e o que passa de
    // U+10FFFF viram U+FFFD, como no HTML5.
    if mapped == 0 { '\u{FFFD}' } else { char::from_u32(mapped).unwrap_or('\u{FFFD}') }
}

/// A referência de caractere que começa no `&` de `text[start]`: o caractere e quantas unidades ela ocupa.
fn character_reference_at(text: &[u16], start: usize, in_attribute: bool) -> Option<(char, usize)> {
    if text.get(start + 1) == Some(&cc::Hash) {
        let hex = matches!(text.get(start + 2), Some(&(cc::x | cc::X)));
        let radix: u32 = if hex { 16 } else { 10 };
        let digits_start = start + 2 + usize::from(hex);
        let mut end = digits_start;
        let mut value: u32 = 0;
        while let Some(digit) = text.get(end).and_then(|&unit| char::from_u32(u32::from(unit))).and_then(|c| c.to_digit(radix)) {
            value = value.saturating_mul(radix).saturating_add(digit).min(0x11_0000);
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        if text.get(end) == Some(&cc::Semicolon) {
            end += 1;
        }
        return Some((numeric_reference_char(value), end - start));
    }

    let name_end = text[start + 1..].iter().position(|&unit| !is_ascii_alphanumeric(unit)).map_or(text.len(), |offset| start + 1 + offset);
    // Só ASCII alfanumérico: cada unidade é um caractere.
    let name: String = text[start + 1..name_end].iter().filter_map(|&unit| char::from_u32(u32::from(unit))).collect();
    if name.is_empty() {
        return None;
    }
    if text.get(name_end) == Some(&cc::Semicolon)
        && let Some(code_point) = named_entity_with_semicolon(&name)
    {
        return char::from_u32(code_point).map(|c| (c, name.len() + 2));
    }
    // Sem o ponto e vírgula, vale o maior prefixo que é um nome legado.
    (1..=name.len()).rev().find_map(|length| {
        let code_point = named_entity_without_semicolon(&name[..length])?;
        let next = text.get(start + 1 + length).copied();
        // Num atributo, o nome legado seguido de `=` ou de alfanumérico fica como texto.
        if in_attribute && next.is_some_and(|unit| unit == cc::Equals || is_ascii_alphanumeric(unit)) {
            return None;
        }
        char::from_u32(code_point).map(|c| (c, 1 + length))
    })
}

/// Troca as referências de caractere de um trecho de texto (ou de valor de atributo) pelos caracteres.
fn decode_character_references(text: &[u16], in_attribute: bool) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == cc::Ampersand
            && let Some((decoded, consumed)) = character_reference_at(text, i, in_attribute)
        {
            let mut buffer = [0u16; 2];
            out.extend_from_slice(decoded.encode_utf16(&mut buffer));
            i += consumed;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Classes de elementos do HTML5.

const VOID_ELEMENTS: &[&str] = &["area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr"];

/// Os elementos "especiais" do HTML5.
const SPECIAL_ELEMENTS: &[&str] = &[
    "address", "applet", "area", "article", "aside", "base", "basefont", "bgsound", "blockquote", "body", "br", "button", "caption", "center", "col", "colgroup", "dd",
    "details", "dir", "div", "dl", "dt", "embed", "fieldset", "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head",
    "header", "hgroup", "hr", "html", "iframe", "img", "input", "keygen", "li", "link", "listing", "main", "marquee", "menu", "meta", "nav", "noembed", "noframes",
    "noscript", "object", "ol", "p", "param", "plaintext", "pre", "script", "search", "section", "select", "source", "style", "summary", "table", "tbody", "td",
    "template", "textarea", "tfoot", "th", "thead", "title", "tr", "track", "ul", "wbr", "xmp",
];

/// As tags de início que fecham um `p` aberto.
const CLOSES_PARAGRAPH: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2",
    "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "listing", "main", "menu", "nav", "ol", "p", "plaintext", "pre", "search", "section", "summary", "ul", "xmp",
];

/// As tags de fim que exigem o elemento "no escopo".
const SCOPED_END_TAGS: &[&str] = &[
    "address", "applet", "article", "aside", "blockquote", "button", "center", "details", "dialog", "dir", "div", "dl", "fieldset", "figcaption", "figure", "footer",
    "form", "header", "hgroup", "listing", "main", "marquee", "menu", "nav", "object", "ol", "pre", "search", "section", "summary", "ul",
];

const TABLE_END_TAGS: &[&str] = &["caption", "colgroup", "table", "tbody", "td", "tfoot", "th", "thead", "tr"];

fn is_heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Default,
    Button,
    ListItem,
    Table,
}

impl ScopeKind {
    fn is_boundary(self, tag: &str) -> bool {
        match self {
            Self::Table => matches!(tag, "table" | "template"),
            _ => {
                matches!(tag, "applet" | "caption" | "marquee" | "object" | "table" | "td" | "template" | "th")
                    || (self == Self::Button && tag == "button")
                    || (self == Self::ListItem && matches!(tag, "ol" | "ul"))
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RawTextMode {
    /// `script`, `style`, `xmp`, `iframe`, `noembed`, `noframes`: o texto vai até a tag de fim, sem entidades.
    RawText,
    /// `textarea`, `title`: o texto vai até a tag de fim, com entidades.
    RcData,
    /// `plaintext`: o texto vai até o fim da entrada.
    PlainText,
}

fn raw_text_mode(tag: &str) -> Option<RawTextMode> {
    match tag {
        "script" | "style" | "xmp" | "iframe" | "noembed" | "noframes" => Some(RawTextMode::RawText),
        "textarea" | "title" => Some(RawTextMode::RcData),
        "plaintext" => Some(RawTextMode::PlainText),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------

struct Parser {
    units: Vec<u16>,
    pos: usize,
    /// Os elementos abertos; o `body` é o primeiro e nunca sai.
    stack: Vec<Element>,
    /// Falso até o primeiro conteúdo do corpo (a fase em que `html`, `head` e o conteúdo do `head` vivem).
    in_body: bool,
    /// O LF logo depois de `<pre>`, `<listing>` e `<textarea>` é descartado.
    skip_line_feed: bool,
    no_quirks: bool,
}

impl Parser {
    fn run(&mut self) {
        while self.pos < self.units.len() {
            let skip_line_feed = std::mem::take(&mut self.skip_line_feed);
            if self.units[self.pos] == cc::LessThan {
                if !self.parse_markup() {
                    self.emit_text(vec![cc::LessThan], skip_line_feed);
                    self.pos += 1;
                }
                continue;
            }
            let end = find_unit(&self.units, self.pos, cc::LessThan).unwrap_or(self.units.len());
            let text = decode_character_references(&self.units[self.pos..end], false);
            self.pos = end;
            self.emit_text(text, skip_line_feed);
        }
    }

    fn finish(mut self) -> Element {
        self.pop_to(1);
        self.stack.pop().expect("o body nunca sai da pilha")
    }

    // ----- tokenizador

    /// Trata o `<` em `pos`. Falso quando ele é só texto.
    fn parse_markup(&mut self) -> bool {
        match self.units.get(self.pos + 1).copied() {
            Some(unit) if is_ascii_alpha(unit) => self.parse_tag(false),
            Some(cc::Slash) => match self.units.get(self.pos + 2).copied() {
                Some(unit) if is_ascii_alpha(unit) => self.parse_tag(true),
                Some(cc::GreaterThan) => self.pos += 3,
                None => return false,
                Some(_) => self.bogus_comment(self.pos + 2),
            },
            Some(cc::ExclamationMark) => self.markup_declaration(),
            Some(cc::QuestionMark) => self.bogus_comment(self.pos + 1),
            _ => return false,
        }
        true
    }

    /// Um comentário falso vai até o `>` (inclusive) ou o fim da entrada; o `data` começa em `from`.
    fn bogus_comment(&mut self, from: usize) {
        let close = find_unit(&self.units, from, cc::GreaterThan);
        self.emit_comment(from, close.unwrap_or(self.units.len()));
        self.pos = close.map_or(self.units.len(), |index| index + 1);
    }

    /// Anexa o comentário cujo `data` é `units[start..end]` (só depois de o corpo abrir; antes dele o
    /// comentário pertence ao documento, fora do `body`).
    fn emit_comment(&mut self, start: usize, end: usize) {
        if self.in_body {
            let data = self.units[start..end].iter().map(|&unit| if unit == 0 { 0xFFFD } else { unit }).collect();
            self.append(Node::Comment(JsString::from_units(data)));
        }
    }

    fn markup_declaration(&mut self) {
        let rest = &self.units[self.pos + 2..];
        if rest.starts_with(&[cc::Dash, cc::Dash]) {
            self.comment(self.pos + 4);
        } else if starts_with_ignore_case(rest, "doctype") {
            let end = find_unit(&self.units, self.pos, cc::GreaterThan).unwrap_or(self.units.len());
            let declaration = lower_ascii(&self.units[self.pos + 2..end]);
            if !self.in_body {
                self.no_quirks = declaration.split_ascii_whitespace().eq(["doctype", "html"]);
            }
            self.pos = (end + 1).min(self.units.len());
        } else {
            // inclui `<![CDATA[`, que em conteúdo HTML é um comentário falso
            self.bogus_comment(self.pos + 2);
        }
    }

    /// O comentário que começa em `start` (depois de `<!--`).
    fn comment(&mut self, start: usize) {
        let tail = &self.units[start..];
        // (fim do `data`, posição depois do comentário)
        let (data_end, after) = if tail.starts_with(&[cc::GreaterThan]) {
            (start, start + 1)
        } else if tail.starts_with(&[cc::Dash, cc::GreaterThan]) {
            (start, start + 2)
        } else {
            (start..self.units.len())
                .find_map(|i| {
                    let rest = &self.units[i..];
                    if rest.starts_with(&[cc::Dash, cc::Dash, cc::GreaterThan]) {
                        Some((i, i + 3))
                    } else if rest.starts_with(&[cc::Dash, cc::Dash, cc::ExclamationMark, cc::GreaterThan]) {
                        Some((i, i + 4))
                    } else {
                        None
                    }
                })
                .unwrap_or((self.units.len(), self.units.len()))
        };
        self.emit_comment(start, data_end);
        self.pos = after;
    }

    /// Tag de início ou de fim em `pos`. Uma tag sem `>` antes do fim da entrada é descartada.
    fn parse_tag(&mut self, is_end: bool) {
        let len = self.units.len();
        let name_start = self.pos + if is_end { 2 } else { 1 };
        let mut i = name_start;
        while i < len && !(is_html_space(self.units[i]) || self.units[i] == cc::Slash || self.units[i] == cc::GreaterThan) {
            i += 1;
        }
        let name = lower_ascii(&self.units[name_start..i]);
        let mut attributes: Vec<Attribute> = Vec::new();

        loop {
            while i < len && (is_html_space(self.units[i]) || self.units[i] == cc::Slash) {
                i += 1;
            }
            if i >= len {
                self.pos = len;
                return;
            }
            if self.units[i] == cc::GreaterThan {
                i += 1;
                break;
            }

            // o primeiro caractere é do nome mesmo sendo `=`
            let attribute_start = i;
            i += 1;
            while i < len && !(is_html_space(self.units[i]) || matches!(self.units[i], cc::Slash | cc::GreaterThan | cc::Equals)) {
                i += 1;
            }
            let attribute_name = lower_ascii(&self.units[attribute_start..i]);

            let mut j = i;
            while j < len && is_html_space(self.units[j]) {
                j += 1;
            }
            let mut value = Vec::new();
            if j < len && self.units[j] == cc::Equals {
                j += 1;
                while j < len && is_html_space(self.units[j]) {
                    j += 1;
                }
                if j >= len {
                    self.pos = len;
                    return;
                }
                match self.units[j] {
                    quote @ (cc::DoubleQuote | cc::SingleQuote) => {
                        let Some(close) = find_unit(&self.units, j + 1, quote) else {
                            self.pos = len;
                            return;
                        };
                        value = decode_character_references(&self.units[j + 1..close], true);
                        i = close + 1;
                    }
                    cc::GreaterThan => i = j,
                    _ => {
                        let value_start = j;
                        while j < len && !(is_html_space(self.units[j]) || self.units[j] == cc::GreaterThan) {
                            j += 1;
                        }
                        value = decode_character_references(&self.units[value_start..j], true);
                        i = j;
                    }
                }
            } else {
                i = j;
            }

            if !attributes.iter().any(|attribute| attribute.name == attribute_name) {
                attributes.push(Attribute { name: attribute_name, value: JsString::from_units(value) });
            }
        }

        self.pos = i;
        if is_end {
            self.end_tag(&name);
        } else {
            self.start_tag(name, attributes);
        }
    }

    /// Lê o conteúdo de um elemento de texto bruto, RCDATA ou `plaintext` (que começa em `pos`) e deixa
    /// `pos` na tag de fim.
    fn read_raw_text(&mut self, tag: &str, mode: RawTextMode) -> Vec<u16> {
        let start = self.pos;
        let end = if mode == RawTextMode::PlainText { self.units.len() } else { self.find_end_tag(tag, start).unwrap_or(self.units.len()) };
        self.pos = end;
        let mut text: Vec<u16> = self.units[start..end].iter().map(|&unit| if unit == 0 { 0xFFFD } else { unit }).collect();
        if mode == RawTextMode::RcData {
            text = decode_character_references(&text, false);
        }
        if tag == "textarea" && text.first() == Some(&cc::LineFeed) {
            text.remove(0);
        }
        text
    }

    /// O `</tag` seguido de espaço, `/` ou `>` (sem diferenciar maiúsculas) a partir de `from`.
    fn find_end_tag(&self, tag: &str, from: usize) -> Option<usize> {
        let mut at = from;
        while let Some(index) = find_unit(&self.units, at, cc::LessThan) {
            let name_start = index + 2;
            let name_end = name_start + tag.len();
            if self.units.get(index + 1) == Some(&cc::Slash)
                && name_end < self.units.len()
                && lower_ascii(&self.units[name_start..name_end]) == tag
                && (is_html_space(self.units[name_end]) || matches!(self.units[name_end], cc::Slash | cc::GreaterThan))
            {
                return Some(index);
            }
            at = index + 1;
        }
        None
    }

    // ----- construtor de árvore

    fn emit_text(&mut self, mut text: Vec<u16>, skip_line_feed: bool) {
        text.retain(|&unit| unit != 0);
        if skip_line_feed && text.first() == Some(&cc::LineFeed) {
            text.remove(0);
        }
        if !self.in_body {
            // antes do corpo, o espaço em branco é ignorado e o primeiro texto de verdade abre o corpo
            let skipped = text.iter().position(|&unit| !is_html_space(unit)).unwrap_or(text.len());
            text.drain(..skipped);
            if text.is_empty() {
                return;
            }
            self.in_body = true;
        }
        if !text.is_empty() {
            self.append(Node::Text(JsString::from_units(text)));
        }
    }

    /// Anexa um nó ao elemento aberto mais interno, juntando texto vizinho como o DOM faz.
    fn append(&mut self, node: Node) {
        let children = &mut self.stack.last_mut().expect("o body nunca sai da pilha").children;
        if let Node::Text(text) = &node
            && let Some(Node::Text(previous)) = children.last_mut()
        {
            previous.push_units(text.units());
            return;
        }
        children.push(node);
    }

    fn pop(&mut self) {
        let mut element = self.stack.pop().expect("o body nunca sai da pilha");
        if element.tag_name == "template" {
            // o conteúdo de um `template` vive num fragmento à parte, não nos filhos
            element.children.clear();
        }
        self.append(Node::Element(element));
    }

    /// Fecha os elementos abertos do índice `index` (inclusive) para cima.
    fn pop_to(&mut self, index: usize) {
        while self.stack.len() > index {
            self.pop();
        }
    }

    /// O índice do elemento aberto mais interno que casa, sem passar por uma fronteira do escopo.
    fn index_in_scope(&self, matches: impl Fn(&str) -> bool, scope: ScopeKind) -> Option<usize> {
        for index in (1..self.stack.len()).rev() {
            let tag = self.stack[index].tag_name.as_str();
            if matches(tag) {
                return Some(index);
            }
            if scope.is_boundary(tag) {
                return None;
            }
        }
        None
    }

    fn pop_in_scope(&mut self, matches: impl Fn(&str) -> bool, scope: ScopeKind) {
        if let Some(index) = self.index_in_scope(matches, scope) {
            self.pop_to(index);
        }
    }

    /// Fecha o `p` aberto que está no escopo de botão, se houver.
    fn close_paragraph(&mut self) {
        self.pop_in_scope(|tag| tag == "p", ScopeKind::Button);
    }

    /// O laço de `li`, `dd` e `dt`: fecha o item aberto do mesmo grupo, sem atravessar um elemento
    /// especial que não seja `address`, `div` ou `p`.
    fn close_open_item(&mut self, items: &[&str]) {
        let target = (1..self.stack.len())
            .rev()
            .map(|index| (index, self.stack[index].tag_name.as_str()))
            .find_map(|(index, tag)| {
                if items.contains(&tag) {
                    Some(Some(index))
                } else if SPECIAL_ELEMENTS.contains(&tag) && !matches!(tag, "address" | "div" | "p") {
                    Some(None)
                } else {
                    None
                }
            })
            .flatten();
        if let Some(index) = target {
            self.pop_to(index);
        }
        self.close_paragraph();
    }

    fn insert_element(&mut self, tag: &str, attributes: Vec<Attribute>) {
        let element = Element::new(tag, attributes);
        if VOID_ELEMENTS.contains(&tag) || self.stack.len() >= MAX_OPEN_ELEMENTS {
            self.append(Node::Element(element));
        } else {
            self.stack.push(element);
        }
    }

    fn start_tag(&mut self, name: String, attributes: Vec<Attribute>) {
        // o `image` é um apelido de `img` no corpo
        let tag = if name == "image" { "img" } else { name.as_str() };

        if !self.in_body {
            match tag {
                "html" | "head" | "base" | "basefont" | "bgsound" | "link" | "meta" => return,
                "body" => {
                    self.in_body = true;
                    return;
                }
                "title" | "noframes" | "style" | "script" => {
                    // conteúdo do `head`: consumido e descartado
                    let mode = raw_text_mode(tag).expect("é um elemento de texto bruto");
                    self.read_raw_text(tag, mode);
                    return;
                }
                _ => self.in_body = true,
            }
        }

        match tag {
            "html" | "head" | "body" => return,
            "li" => self.close_open_item(&["li"]),
            "dd" | "dt" => self.close_open_item(&["dd", "dt"]),
            "td" | "th" => self.pop_in_scope(|t| matches!(t, "td" | "th"), ScopeKind::Table),
            "tr" => self.pop_in_scope(|t| t == "tr", ScopeKind::Table),
            "tbody" | "thead" | "tfoot" => self.pop_in_scope(|t| matches!(t, "tbody" | "thead" | "tfoot"), ScopeKind::Table),
            "button" => self.pop_in_scope(|t| t == "button", ScopeKind::Default),
            "a" => self.pop_in_scope(|t| t == "a", ScopeKind::Default),
            "option" | "optgroup" => {
                if self.stack.last().is_some_and(|top| top.tag_name == "option") {
                    self.pop();
                }
                if tag == "optgroup" && self.stack.last().is_some_and(|top| top.tag_name == "optgroup") {
                    self.pop();
                }
            }
            "table" => {
                if self.no_quirks {
                    self.close_paragraph();
                }
            }
            _ => {}
        }
        if CLOSES_PARAGRAPH.contains(&tag) {
            self.close_paragraph();
        }
        if is_heading(tag) && self.stack.last().is_some_and(|top| is_heading(&top.tag_name)) {
            self.pop();
        }
        if matches!(tag, "pre" | "listing") {
            self.skip_line_feed = true;
        }

        self.insert_element(tag, attributes);

        if let Some(mode) = raw_text_mode(tag) {
            let text = self.read_raw_text(tag, mode);
            if !text.is_empty() {
                self.append(Node::Text(JsString::from_units(text)));
            }
        }
    }

    fn end_tag(&mut self, name: &str) {
        if !self.in_body {
            if name != "br" {
                return;
            }
            self.in_body = true;
        }

        match name {
            "body" | "html" | "head" => {}
            // `</br>` é tratada como `<br>`
            "br" => self.insert_element("br", Vec::new()),
            "p" => match self.index_in_scope(|tag| tag == "p", ScopeKind::Button) {
                Some(index) => self.pop_to(index),
                None => self.append(Node::Element(Element::new("p", Vec::new()))),
            },
            "li" => self.pop_in_scope(|tag| tag == "li", ScopeKind::ListItem),
            "dd" | "dt" => self.pop_in_scope(|tag| tag == name, ScopeKind::Default),
            _ if is_heading(name) => self.pop_in_scope(is_heading, ScopeKind::Default),
            "template" => {
                if let Some(index) = (1..self.stack.len()).rev().find(|&index| self.stack[index].tag_name == "template") {
                    self.pop_to(index);
                }
            }
            _ if SCOPED_END_TAGS.contains(&name) => self.pop_in_scope(|tag| tag == name, ScopeKind::Default),
            _ if TABLE_END_TAGS.contains(&name) => self.pop_in_scope(|tag| tag == name, ScopeKind::Table),
            _ => {
                // "qualquer outra tag de fim": o aberto de mesmo nome, sem atravessar um elemento especial
                let target = (1..self.stack.len())
                    .rev()
                    .map(|index| (index, self.stack[index].tag_name.as_str()))
                    .find_map(|(index, tag)| if tag == name { Some(Some(index)) } else if SPECIAL_ELEMENTS.contains(&tag) { Some(None) } else { None })
                    .flatten();
                if let Some(index) = target {
                    self.pop_to(index);
                }
            }
        }
    }
}
