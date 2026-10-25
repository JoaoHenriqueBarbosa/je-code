//! O resolvedor de estilo dos fixtures de componentes: o que o `getComputedStyle` do navegador devolve ao original para as
//! folhas de estilo que os fixtures instalam (`adoptedStyleSheets` e `<style>`), sobre a árvore nativa da `Ui`.
//!
//! O CSS aqui é **especificação executada só no teste**: o texto vem do mesmo gerador do original
//! (`generateColorThemeCSS`, `generateTokensCSSForColorMap`) e dos arquivos `.css` do VS Code, e este módulo o interpreta
//! para conferir o estilo resolvido de cada nó. Nada disto está no caminho de pintura (os widgets nativos pintam o valor
//! resolvido do tema, sem folha de estilo).
//!
//! O que existe, e só o que os casos dos fixtures leem:
//! - **Seletores.** Lista separada por vírgula de seletores com combinador de descendente, cada compound com classes
//!   (`.a.b`), `*`, `:scope` e `:where(...)` (que não soma especificidade). Tags, atributos, `>` e pseudo-elementos não
//!   existem na árvore nativa e dão erro de análise.
//! - **`@scope`.** Com prelúdio (`@scope (seletor) { ... }`) a regra vale para os nós dentro de um elemento que casa o
//!   prelúdio; sem prelúdio (`@scope { ... }`) vale dentro do elemento onde a folha foi anexada (o pai do `<style>`).
//!   Seletores relativos ganham o `:scope ` implícito (o nó mais à esquerda é descendente estrito da raiz do escopo, sem
//!   somar especificidade), e `:scope` explícito casa a própria raiz.
//! - **Cascata.** `!important`, depois estilo em linha, depois especificidade (número de classes), depois a ordem (folhas do
//!   documento, folhas adotadas na ordem de instalação, regras, declarações). A proximidade de escopo (`@scope`) não é
//!   usada: nenhum caso depende dela.
//! - **Propriedades.** Cada declaração vira longhands (`border`, `background`, `font`, `text-decoration`); as
//!   propriedades personalizadas (`--x`) herdam e `var(--x, fallback)` é substituído na hora de calcular. Herdam `color`,
//!   `font-style` e `font-weight`; as demais não.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use je_base::lifecycle::{DisposableStore, to_disposable};
use je_theme::color::Color;
use je_theme::color::format::css::format_rgb;
use je_ui::{NodeId, Role, Tree, Ui};

// ---------------------------------------------------------------------------------------------------------------------
// Seletores
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone)]
enum Simple {
    Universal,
    Class(String),
    Scope,
    Where(Vec<Complex>),
}

#[derive(Clone, Default)]
struct Compound(Vec<Simple>);

/// Os compounds ligados por combinador de descendente; o último é o sujeito.
#[derive(Clone, Default)]
struct Complex(Vec<Compound>);

#[derive(Clone)]
enum Scope {
    /// `@scope (seletores)`.
    Selectors(Vec<Complex>),
    /// `@scope { ... }` de uma folha anexada ao elemento.
    Element(NodeId),
}

/// Uma declaração `nome: valor`.
#[derive(Clone)]
struct Declaration {
    name: String,
    value: String,
    important: bool,
}

struct Rule {
    scope: Option<Rc<Scope>>,
    selectors: Vec<Complex>,
    declarations: Vec<Declaration>,
}

/// Uma folha de estilo analisada (o `CSSStyleSheet`).
pub struct Stylesheet {
    text: String,
    rules: Vec<Rule>,
}

impl Stylesheet {
    /// `replaceSync(css)`. `scope_root` é o elemento ao qual a folha está anexada (o pai do `<style>`), que um `@scope`
    /// sem prelúdio usa como raiz.
    pub fn parse(css: &str, scope_root: Option<NodeId>) -> Result<Stylesheet, String> {
        let mut rules = Vec::new();
        parse_rules(&strip_comments(css), None, scope_root, &mut rules)?;
        Ok(Stylesheet { text: css.to_owned(), rules })
    }

    /// O texto da folha (o `textContent` do `<style>`).
    pub fn text(&self) -> &str {
        &self.text
    }
}

fn strip_comments(css: &str) -> String {
    let mut output = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        output.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => rest = "",
        }
    }
    output.push_str(rest);
    output
}

/// Divide no separador, ignorando o que está entre parênteses; descarta as partes vazias e apara as demais.
fn split_top_level(text: &str, is_separator: impl Fn(char) -> bool) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for c in text.chars() {
        if c == '(' {
            depth += 1;
            current.push(c);
        } else if c == ')' {
            depth -= 1;
            current.push(c);
        } else if depth == 0 && is_separator(c) {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    parts.into_iter().map(|part| part.trim().to_owned()).filter(|part| !part.is_empty()).collect()
}

/// O texto entre o `(` em `open` e o `)` que o fecha, e o índice depois dele.
fn read_parenthesized(chars: &[char], open: usize) -> Result<(String, usize), String> {
    if chars.get(open) != Some(&'(') {
        return Err("esperava '('".to_owned());
    }
    let mut depth = 0;
    let mut inner = String::new();
    for (offset, &c) in chars[open..].iter().enumerate() {
        if c == '(' {
            depth += 1;
            if depth > 1 {
                inner.push(c);
            }
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Ok((inner, open + offset + 1));
            }
            inner.push(c);
        } else {
            inner.push(c);
        }
    }
    Err("parêntese sem fechamento".to_owned())
}

fn read_ident(chars: &[char], start: usize) -> (String, usize) {
    let mut index = start;
    let mut ident = String::new();
    while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '-' || chars[index] == '_') {
        ident.push(chars[index]);
        index += 1;
    }
    (ident, index)
}

fn parse_selector_list(text: &str) -> Result<Vec<Complex>, String> {
    split_top_level(text, |c| c == ',').iter().map(|selector| parse_complex(selector)).collect()
}

fn parse_complex(text: &str) -> Result<Complex, String> {
    let compounds: Result<Vec<Compound>, String> = split_top_level(text, char::is_whitespace).iter().map(|part| parse_compound(part)).collect();
    let compounds = compounds?;
    if compounds.is_empty() {
        return Err(format!("seletor vazio: {text}"));
    }
    Ok(Complex(compounds))
}

fn parse_compound(text: &str) -> Result<Compound, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    let mut simples = Vec::new();
    while index < chars.len() {
        match chars[index] {
            '*' => {
                simples.push(Simple::Universal);
                index += 1;
            }
            '.' => {
                let (name, next) = read_ident(&chars, index + 1);
                simples.push(Simple::Class(name));
                index = next;
            }
            ':' => {
                let (name, next) = read_ident(&chars, index + 1);
                index = next;
                match name.as_str() {
                    "scope" => simples.push(Simple::Scope),
                    "where" => {
                        let (inner, after) = read_parenthesized(&chars, index)?;
                        simples.push(Simple::Where(parse_selector_list(&inner)?));
                        index = after;
                    }
                    other => return Err(format!("pseudo-classe sem suporte: :{other} em {text}")),
                }
            }
            other => return Err(format!("seletor sem suporte: '{other}' em {text}")),
        }
    }
    Ok(Compound(simples))
}

fn parse_declarations(body: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    for entry in split_top_level(body, |c| c == ';') {
        let Some((name, value)) = entry.split_once(':') else { continue };
        let name = name.trim();
        let name = if name.starts_with("--") { name.to_owned() } else { name.to_ascii_lowercase() };
        let value = value.trim();
        let (value, important) = match value.strip_suffix("!important") {
            Some(stripped) => (stripped.trim(), true),
            None => (value, false),
        };
        declarations.push(Declaration { name, value: value.to_owned(), important });
    }
    declarations
}

fn parse_rules(css: &str, scope: Option<Rc<Scope>>, scope_root: Option<NodeId>, output: &mut Vec<Rule>) -> Result<(), String> {
    let chars: Vec<char> = css.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let mut prelude = String::new();
        let mut opened = false;
        while index < chars.len() {
            let c = chars[index];
            index += 1;
            if c == '{' {
                opened = true;
                break;
            }
            prelude.push(c);
        }
        let prelude = prelude.trim().to_owned();
        if !opened {
            if prelude.is_empty() {
                break;
            }
            return Err(format!("regra sem corpo: {prelude}"));
        }
        let mut depth = 1;
        let mut body = String::new();
        while index < chars.len() {
            let c = chars[index];
            index += 1;
            if c == '{' {
                depth += 1;
                body.push(c);
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                body.push(c);
            } else {
                body.push(c);
            }
        }
        if depth != 0 {
            return Err(format!("chave sem fechamento na regra: {prelude}"));
        }
        if let Some(at_scope) = prelude.strip_prefix("@scope") {
            let at_scope = at_scope.trim();
            let new_scope = if at_scope.is_empty() {
                Scope::Element(scope_root.ok_or_else(|| "@scope sem prelúdio exige o elemento onde a folha está anexada".to_owned())?)
            } else {
                let prelude_chars: Vec<char> = at_scope.chars().collect();
                let (inner, _) = read_parenthesized(&prelude_chars, 0)?;
                Scope::Selectors(parse_selector_list(&inner)?)
            };
            parse_rules(&body, Some(Rc::new(new_scope)), scope_root, output)?;
        } else if prelude.starts_with('@') {
            return Err(format!("at-rule sem suporte: {prelude}"));
        } else {
            output.push(Rule { scope: scope.clone(), selectors: parse_selector_list(&prelude)?, declarations: parse_declarations(&body) });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Casamento
// ---------------------------------------------------------------------------------------------------------------------

struct MatchContext<'a> {
    tree: &'a Tree,
    /// O elemento que `:scope` casa.
    scope: Option<NodeId>,
}

fn matches_simple(simple: &Simple, node: NodeId, context: &MatchContext) -> bool {
    match simple {
        Simple::Universal => true,
        Simple::Class(name) => context.tree.has_class(node, name),
        Simple::Scope => context.scope == Some(node),
        Simple::Where(list) => list.iter().any(|complex| matches_complex(complex, node, context, None)),
    }
}

fn matches_compound(compound: &Compound, node: NodeId, context: &MatchContext) -> bool {
    compound.0.iter().all(|simple| matches_simple(simple, node, context))
}

/// `limit`: a raiz do `@scope`, da qual o nó mais à esquerda tem de ser descendente estrito (o `:scope ` implícito).
fn matches_complex(complex: &Complex, node: NodeId, context: &MatchContext, limit: Option<NodeId>) -> bool {
    match_chain(&complex.0, complex.0.len(), node, context, limit)
}

/// Casa `compounds[count - 1]` em `node` e os anteriores nos antepassados.
fn match_chain(compounds: &[Compound], count: usize, node: NodeId, context: &MatchContext, limit: Option<NodeId>) -> bool {
    if count == 0 {
        return true;
    }
    if !matches_compound(&compounds[count - 1], node, context) {
        return false;
    }
    if count == 1 {
        return match limit {
            Some(root) => root != node && context.tree.is_ancestor(root, node),
            None => true,
        };
    }
    let mut ancestor = context.tree.parent(node);
    while let Some(current) = ancestor {
        if match_chain(compounds, count - 1, current, context, limit) {
            return true;
        }
        ancestor = context.tree.parent(current);
    }
    false
}

fn mentions_scope(complex: &Complex) -> bool {
    complex.0.iter().any(|compound| compound.0.iter().any(|simple| matches!(simple, Simple::Scope)))
}

/// A especificidade que importa aqui: o número de classes e de `:scope` explícitos (`:where` e `*` valem zero).
fn specificity(complex: &Complex) -> u32 {
    complex
        .0
        .iter()
        .flat_map(|compound| compound.0.iter())
        .map(|simple| match simple {
            Simple::Class(_) | Simple::Scope => 1,
            Simple::Universal | Simple::Where(_) => 0,
        })
        .sum()
}

fn rule_matches(rule: &Rule, selector: &Complex, node: NodeId, tree: &Tree) -> bool {
    let limit_for = |root: NodeId| if mentions_scope(selector) { None } else { Some(root) };
    match rule.scope.as_deref() {
        None => matches_complex(selector, node, &MatchContext { tree, scope: None }, None),
        Some(Scope::Element(root)) => {
            if *root != node && !tree.is_ancestor(*root, node) {
                return false;
            }
            matches_complex(selector, node, &MatchContext { tree, scope: Some(*root) }, limit_for(*root))
        }
        Some(Scope::Selectors(scope_selectors)) => {
            let plain = MatchContext { tree, scope: None };
            let mut candidate = Some(node);
            while let Some(root) = candidate {
                if scope_selectors.iter().any(|scope_selector| matches_complex(scope_selector, root, &plain, None)) {
                    let context = MatchContext { tree, scope: Some(root) };
                    if matches_complex(selector, node, &context, limit_for(root)) {
                        return true;
                    }
                }
                candidate = tree.parent(root);
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Valores
// ---------------------------------------------------------------------------------------------------------------------

fn starts_with_at(chars: &[char], index: usize, pattern: &str) -> bool {
    pattern.chars().enumerate().all(|(offset, expected)| chars.get(index + offset) == Some(&expected))
}

/// Substitui os `var(--nome, fallback)`. `None` é o valor inválido em tempo de cálculo (variável indefinida sem fallback).
fn resolve_vars(value: &str, custom: &HashMap<String, String>) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if starts_with_at(&chars, index, "var(") {
            let (inner, after) = read_parenthesized(&chars, index + 3).ok()?;
            let parts = split_first_top_level_comma(&inner);
            let name = parts.0.trim().to_owned();
            let replacement = match custom.get(&name) {
                Some(found) => found.clone(),
                None => match parts.1 {
                    Some(fallback) => resolve_vars(fallback.trim(), custom)?,
                    None => return None,
                },
            };
            output.push_str(&replacement);
            index = after;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    Some(output)
}

/// Divide na primeira vírgula fora de parênteses.
fn split_first_top_level_comma(text: &str) -> (String, Option<String>) {
    let mut depth = 0i32;
    for (offset, c) in text.char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
        } else if c == ',' && depth == 0 {
            return (text[..offset].to_owned(), Some(text[offset + 1..].to_owned()));
        }
    }
    (text.to_owned(), None)
}

/// A cor de um valor CSS (`#rgb[a]`, `rgb()`, `rgba()`, `black`, `white`, `transparent`, `currentcolor`).
pub fn parse_css_color(text: &str, current_color: &Color) -> Option<Color> {
    let lowered = text.trim().to_ascii_lowercase();
    if lowered.starts_with('#') {
        return Some(Color::from_hex(&lowered));
    }
    match lowered.as_str() {
        "black" => return Some(Color::from_hex("#000000")),
        "white" => return Some(Color::from_hex("#ffffff")),
        "transparent" => return Some(Color::from_hex("#00000000")),
        "currentcolor" => return Some(current_color.clone()),
        _ => {}
    }
    let arguments = lowered.strip_prefix("rgba(").or_else(|| lowered.strip_prefix("rgb("))?.strip_suffix(')')?;
    let numbers: Vec<f64> = arguments
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_end_matches('%').parse::<f64>())
        .collect::<Result<Vec<f64>, _>>()
        .ok()?;
    if numbers.len() < 3 {
        return None;
    }
    let alpha = numbers.get(3).copied().unwrap_or(1.0);
    let byte = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    Some(Color::from_hex(&format!("#{:02x}{:02x}{:02x}{:02x}", byte(numbers[0]), byte(numbers[1]), byte(numbers[2]), byte(alpha * 255.0))))
}

const BORDER_STYLES: [&str; 9] = ["none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset"];

fn is_length(token: &str) -> bool {
    token.ends_with("px") || token.parse::<f64>().is_ok() || matches!(token, "thin" | "medium" | "thick")
}

/// Os longhands de uma declaração: `border`, `background`, `font` e `text-decoration` são expandidos; o resto passa direto.
fn expand_declaration(name: &str, value: &str) -> Vec<(String, String)> {
    let value = value.trim();
    let keyword = matches!(value, "inherit" | "initial");
    match name {
        "border" => {
            if keyword {
                return ["border-width", "border-style", "border-color"].iter().map(|longhand| ((*longhand).to_owned(), value.to_owned())).collect();
            }
            let (mut width, mut style, mut color) = ("medium".to_owned(), "none".to_owned(), "currentcolor".to_owned());
            for token in split_top_level(value, char::is_whitespace) {
                if BORDER_STYLES.contains(&token.as_str()) {
                    style = token;
                } else if is_length(&token) {
                    width = token;
                } else {
                    color = token;
                }
            }
            vec![("border-width".to_owned(), width), ("border-style".to_owned(), style), ("border-color".to_owned(), color)]
        }
        "background" => vec![("background-color".to_owned(), value.to_owned())],
        "font" => {
            if keyword {
                return vec![("font-style".to_owned(), value.to_owned()), ("font-weight".to_owned(), value.to_owned())];
            }
            let tokens = split_top_level(value, char::is_whitespace);
            let style = if tokens.iter().any(|token| token == "italic") { "italic" } else { "normal" };
            let weight = tokens
                .iter()
                .find(|token| token.as_str() == "bold" || token.parse::<u32>().is_ok_and(|number| (100..=900).contains(&number) && number % 100 == 0))
                .cloned()
                .unwrap_or_else(|| "normal".to_owned());
            vec![("font-style".to_owned(), style.to_owned()), ("font-weight".to_owned(), weight)]
        }
        "text-decoration" => {
            if keyword {
                return vec![("text-decoration-line".to_owned(), value.to_owned())];
            }
            let lines: Vec<String> = split_top_level(value, char::is_whitespace).into_iter().filter(|token| matches!(token.as_str(), "underline" | "line-through" | "overline")).collect();
            vec![("text-decoration-line".to_owned(), if lines.is_empty() { "none".to_owned() } else { lines.join(" ") })]
        }
        _ => vec![(name.to_owned(), value.to_owned())],
    }
}

const INHERITED: [&str; 3] = ["color", "font-style", "font-weight"];

/// O resultado do `getComputedStyle`: as propriedades personalizadas e os longhands calculados.
#[derive(Clone, Default)]
pub struct ComputedStyle {
    custom: HashMap<String, String>,
    values: HashMap<String, String>,
}

impl ComputedStyle {
    /// O `color` calculado (o `black` do navegador quando nada o define).
    fn current_color(&self) -> Color {
        let black = Color::from_hex("#000000");
        self.values.get("color").and_then(|value| parse_css_color(value, &black)).unwrap_or(black)
    }

    fn color_of(&self, property: &str) -> Option<Color> {
        let current = self.current_color();
        self.values.get(property).and_then(|value| parse_css_color(value, &current))
    }

    /// `style.color`.
    pub fn color(&self) -> String {
        format_rgb(&self.current_color())
    }

    /// `style.backgroundColor`.
    pub fn background_color(&self) -> String {
        self.color_of("background-color").map_or_else(|| "rgba(0, 0, 0, 0)".to_owned(), |color| format_rgb(&color))
    }

    /// `style.border`: largura, estilo e cor, quando as quatro bordas são iguais (o único caso que os fixtures leem).
    pub fn border(&self) -> String {
        let style = self.values.get("border-style").map_or("none", String::as_str);
        let width = if matches!(style, "none" | "hidden") {
            "0px".to_owned()
        } else {
            match self.values.get("border-width").map(String::as_str) {
                None | Some("medium") => "3px".to_owned(),
                Some("thin") => "1px".to_owned(),
                Some("thick") => "5px".to_owned(),
                Some(other) => other.to_owned(),
            }
        };
        let color = self.color_of("border-color").unwrap_or_else(|| self.current_color());
        format!("{width} {style} {}", format_rgb(&color))
    }

    /// `style.fontStyle`.
    pub fn font_style(&self) -> String {
        self.values.get("font-style").cloned().unwrap_or_else(|| "normal".to_owned())
    }

    /// `style.fontWeight`: as palavras-chave viram o número (`normal` é `400`, `bold` é `700`).
    pub fn font_weight(&self) -> String {
        match self.values.get("font-weight").map(String::as_str) {
            None | Some("normal") => "400".to_owned(),
            Some("bold" | "bolder") => "700".to_owned(),
            Some(other) => other.to_owned(),
        }
    }

    /// `style.textDecorationLine`.
    pub fn text_decoration_line(&self) -> String {
        self.values.get("text-decoration-line").cloned().unwrap_or_else(|| "none".to_owned())
    }

    /// `getPropertyValue('--nome')`.
    pub fn custom_property(&self, name: &str) -> Option<&str> {
        self.custom.get(name).map(String::as_str)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Documento e elementos
// ---------------------------------------------------------------------------------------------------------------------

/// A posição de uma declaração na cascata: `!important`, em linha, especificidade, folha, regra e declaração.
type CascadeKey = (bool, bool, u32, usize, usize, usize);

fn consider(winners: &mut HashMap<String, (CascadeKey, String)>, name: String, value: String, key: CascadeKey) {
    let replace = match winners.get(&name) {
        Some((existing, _)) => *existing <= key,
        None => true,
    };
    if replace {
        winners.insert(name, (key, value));
    }
}

struct DocumentSheet {
    id: u64,
    sheet: Rc<Stylesheet>,
    /// O elemento que contém o `<style>` (`None` para uma folha de documento sem elemento nosso).
    host: Option<NodeId>,
}

/// O documento do fixture: a árvore nativa, as folhas de estilo do documento e as adotadas, e o estilo em linha.
pub struct FixtureDocument {
    ui: Ui,
    document_sheets: RefCell<Vec<DocumentSheet>>,
    adopted_sheets: RefCell<Vec<Rc<Stylesheet>>>,
    inline_styles: RefCell<HashMap<NodeId, Vec<Declaration>>>,
    next_id: Cell<u64>,
}

impl FixtureDocument {
    pub fn new() -> Rc<FixtureDocument> {
        Rc::new(FixtureDocument {
            ui: Ui::new(),
            document_sheets: RefCell::new(Vec::new()),
            adopted_sheets: RefCell::new(Vec::new()),
            inline_styles: RefCell::new(HashMap::new()),
            next_id: Cell::new(0),
        })
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// `document.body`.
    pub fn body(self: &Rc<Self>) -> FixtureElement {
        FixtureElement { document: self.clone(), node: self.ui.root() }
    }

    /// Uma folha do documento (um `<link>` ou `<style>` do `index.html`): vem antes das folhas adotadas.
    pub fn add_document_style_sheet(&self, sheet: Rc<Stylesheet>) {
        let id = self.take_id();
        self.document_sheets.borrow_mut().push(DocumentSheet { id, sheet, host: None });
    }

    /// `document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]`.
    pub fn adopt_style_sheet(&self, sheet: Rc<Stylesheet>) {
        self.adopted_sheets.borrow_mut().push(sheet);
    }

    fn take_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    /// As folhas na ordem da cascata: as do documento, depois as adotadas.
    fn sheets_in_cascade_order(&self) -> Vec<Rc<Stylesheet>> {
        let mut sheets: Vec<Rc<Stylesheet>> = self.document_sheets.borrow().iter().map(|entry| entry.sheet.clone()).collect();
        sheets.extend(self.adopted_sheets.borrow().iter().cloned());
        sheets
    }

    /// `getComputedStyle(element)`.
    pub fn computed_style(&self, node: NodeId) -> ComputedStyle {
        let sheets = self.sheets_in_cascade_order();
        let tree = self.ui.tree();
        self.compute(&sheets, &tree, node)
    }

    fn compute(&self, sheets: &[Rc<Stylesheet>], tree: &Tree, node: NodeId) -> ComputedStyle {
        let parent = tree.parent(node).map(|parent| self.compute(sheets, tree, parent));
        let mut style = ComputedStyle::default();
        if let Some(parent) = &parent {
            style.custom = parent.custom.clone();
            for name in INHERITED {
                if let Some(value) = parent.values.get(name) {
                    style.values.insert(name.to_owned(), value.clone());
                }
            }
        }
        let cascaded = self.cascade(sheets, tree, node);

        // As propriedades personalizadas primeiro, para os `var()` das demais.
        let own_custom: Vec<(String, String)> = cascaded.iter().filter(|(name, _)| name.starts_with("--")).map(|(name, value)| (name.clone(), value.clone())).collect();
        let mut environment = style.custom.clone();
        for (name, value) in &own_custom {
            environment.insert(name.clone(), value.clone());
        }
        for (name, value) in &own_custom {
            match resolve_vars(value, &environment) {
                Some(resolved) => {
                    style.custom.insert(name.clone(), resolved);
                }
                None => {
                    style.custom.remove(name);
                }
            }
        }

        for (name, value) in &cascaded {
            if name.starts_with("--") {
                continue;
            }
            let Some(resolved) = resolve_vars(value, &style.custom) else { continue };
            match resolved.trim() {
                "inherit" => match parent.as_ref().and_then(|parent| parent.values.get(name)) {
                    Some(inherited) => {
                        style.values.insert(name.clone(), inherited.clone());
                    }
                    None => {
                        style.values.remove(name);
                    }
                },
                "initial" => {
                    style.values.remove(name);
                }
                other => {
                    style.values.insert(name.clone(), other.to_owned());
                }
            }
        }
        style
    }

    /// A declaração vencedora de cada propriedade no nó (já em longhands).
    fn cascade(&self, sheets: &[Rc<Stylesheet>], tree: &Tree, node: NodeId) -> BTreeMap<String, String> {
        let mut winners: HashMap<String, (CascadeKey, String)> = HashMap::new();
        for (sheet_index, sheet) in sheets.iter().enumerate() {
            for (rule_index, rule) in sheet.rules.iter().enumerate() {
                let best = rule.selectors.iter().filter(|selector| rule_matches(rule, selector, node, tree)).map(specificity).max();
                let Some(best) = best else { continue };
                for (declaration_index, declaration) in rule.declarations.iter().enumerate() {
                    for (name, value) in expand_declaration(&declaration.name, &declaration.value) {
                        consider(&mut winners, name, value, (declaration.important, false, best, sheet_index, rule_index, declaration_index));
                    }
                }
            }
        }
        if let Some(inline) = self.inline_styles.borrow().get(&node) {
            for (declaration_index, declaration) in inline.iter().enumerate() {
                for (name, value) in expand_declaration(&declaration.name, &declaration.value) {
                    consider(&mut winners, name, value, (declaration.important, true, 0, 0, 0, declaration_index));
                }
            }
        }
        winners.into_iter().map(|(name, (_, value))| (name, value)).collect()
    }
}

/// Um elemento do documento do fixture: o `HTMLElement` do original.
#[derive(Clone)]
pub struct FixtureElement {
    document: Rc<FixtureDocument>,
    node: NodeId,
}

impl FixtureElement {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn document(&self) -> &Rc<FixtureDocument> {
        &self.document
    }

    /// `parent.appendChild($('.a.b'))`: um elemento novo, filho deste, com as classes (separadas por espaço).
    pub fn append_child(&self, class_name: &str) -> FixtureElement {
        let node = {
            let mut tree = self.document.ui.tree_mut();
            let node = tree.create(self.node, Role::Generic);
            tree.set_class_name(node, class_name);
            node
        };
        FixtureElement { document: self.document.clone(), node }
    }

    /// `classList.add(...classNames)`.
    pub fn add_classes(&self, class_names: &[&str]) {
        let mut tree = self.document.ui.tree_mut();
        let mut classes: Vec<String> = tree.classes(self.node).to_vec();
        for class_name in class_names {
            if !classes.iter().any(|existing| existing == class_name) {
                classes.push((*class_name).to_owned());
            }
        }
        tree.set_class_name(self.node, &classes.join(" "));
    }

    /// `element.remove()`.
    pub fn remove(&self) {
        self.document.ui.remove(self.node);
    }

    /// `element.style.setProperty(name, value)` (ou `element.style.x = value`, com `x` em kebab-case): o valor em linha
    /// substitui o anterior da mesma propriedade.
    pub fn set_style_property(&self, name: &str, value: &str) {
        let mut inline = self.document.inline_styles.borrow_mut();
        let declarations = inline.entry(self.node).or_default();
        match declarations.iter_mut().find(|declaration| declaration.name == name) {
            Some(existing) => existing.value = value.to_owned(),
            None => declarations.push(Declaration { name: name.to_owned(), value: value.to_owned(), important: false }),
        }
    }

    /// `getComputedStyle(element)`.
    pub fn computed_style(&self) -> ComputedStyle {
        self.document.computed_style(self.node)
    }

    /// `createStyleSheet(container, undefined, disposables)` com o `textContent` já definido: um `<style>` neste elemento,
    /// removido quando o `store` é descartado. Um `@scope` sem prelúdio vale dentro deste elemento.
    pub fn append_style_element(&self, css: &str, store: &DisposableStore) -> Result<(), String> {
        let sheet = Rc::new(Stylesheet::parse(css, Some(self.node))?);
        let id = self.document.take_id();
        self.document.document_sheets.borrow_mut().push(DocumentSheet { id, sheet, host: Some(self.node) });
        let document = self.document.clone();
        store.add(to_disposable(move || document.document_sheets.borrow_mut().retain(|entry| entry.id != id)));
        Ok(())
    }

    /// `element.querySelector('style')?.textContent`.
    pub fn style_element_text(&self) -> Option<String> {
        self.document.document_sheets.borrow().iter().find(|entry| entry.host == Some(self.node)).map(|entry| entry.sheet.text().to_owned())
    }
}
