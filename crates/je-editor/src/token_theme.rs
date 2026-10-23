//! Porte de `src/vs/editor/common/languages/supports/tokenization.ts`: o tema de tokens (`TokenTheme`), o
//! mapa de cores (`ColorMap`), a árvore de regras por escopo (`ThemeTrieElement`) e a geração das classes CSS
//! de token (`generateTokensCSSForColorMap`, `generateTokensCSSForFontMap`).
//!
//! Correspondência de nomes: `_match` é [`TokenTheme::match_rule`] e `match(languageId, token)` é
//! [`TokenTheme::match_token`] (`match` é palavra reservada). O `ITokenThemeRule` do original, uma
//! interface com campos opcionais, é a struct [`ITokenThemeRule`] com construtor encadeado.
//!
//! Divergências: o `throw` de `ColorMap.getId` (cor inválida) vira `Result` com [`IllegalTokenColorError`];
//! o array esparso de cores (`_id2color`, com o índice 0 vazio) vira `Vec<Option<Color>>`; os mapas do JS viram
//! `HashMap` (a comparação do `deepStrictEqual` entre `Map` também ignora a ordem).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;

use je_theme::color::Color;

use crate::encoded_token_attributes::{color_id, font_style, metadata_consts, standard_token_type};

/// `ITokenThemeRule`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ITokenThemeRule {
    pub token: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub font_style: Option<String>,
}

impl ITokenThemeRule {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            ..Self::default()
        }
    }

    pub fn with_foreground(mut self, foreground: &str) -> Self {
        self.foreground = Some(foreground.to_owned());
        self
    }

    pub fn with_background(mut self, background: &str) -> Self {
        self.background = Some(background.to_owned());
        self
    }

    pub fn with_font_style(mut self, font_style: &str) -> Self {
        self.font_style = Some(font_style.to_owned());
        self
    }
}

/// `ParsedTokenThemeRule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedTokenThemeRule {
    pub token: String,
    pub index: i32,
    /// `FontStyle::NOT_SET` (-1) se não definido; senão uma máscara `or` de `FontStyle`.
    pub font_style: i32,
    pub foreground: Option<String>,
    pub background: Option<String>,
}

impl ParsedTokenThemeRule {
    pub fn new(
        token: &str,
        index: i32,
        font_style: i32,
        foreground: Option<&str>,
        background: Option<&str>,
    ) -> Self {
        Self {
            token: token.to_owned(),
            index,
            font_style,
            foreground: foreground.map(str::to_owned),
            background: background.map(str::to_owned),
        }
    }
}

/// `parseTokenTheme`: interpreta um tema cru como regras.
pub fn parse_token_theme(source: &[ITokenThemeRule]) -> Vec<ParsedTokenThemeRule> {
    source
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let mut parsed_font_style = font_style::NOT_SET;
            if let Some(entry_font_style) = &entry.font_style {
                let mut flags = font_style::NONE;
                for segment in entry_font_style.split(' ') {
                    match segment {
                        "italic" => flags |= font_style::ITALIC,
                        "bold" => flags |= font_style::BOLD,
                        "underline" => flags |= font_style::UNDERLINE,
                        "strikethrough" => flags |= font_style::STRIKETHROUGH,
                        _ => {}
                    }
                }
                parsed_font_style = flags as i32;
            }
            ParsedTokenThemeRule {
                token: entry.token.clone(),
                index: i as i32,
                font_style: parsed_font_style,
                foreground: entry.foreground.clone(),
                background: entry.background.clone(),
            }
        })
        .collect()
}

/// O `Error('Illegal value for token color: ...')` de `ColorMap.getId`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IllegalTokenColorError(pub String);

impl fmt::Display for IllegalTokenColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Illegal value for token color: {}", self.0)
    }
}

impl std::error::Error for IllegalTokenColorError {}

/// `resolveParsedTokenThemeRules`: resolve as regras (a herança).
fn resolve_parsed_token_theme_rules(
    mut parsed_theme_rules: Vec<ParsedTokenThemeRule>,
    custom_token_colors: &[String],
) -> Result<TokenTheme, IllegalTokenColorError> {
    // Ordena lexicograficamente e, em seguida, pelo índice (a ordenação do JS e a do Rust são estáveis).
    parsed_theme_rules.sort_by(|a, b| {
        strcmp(&a.token, &b.token)
            .cmp(&0)
            .then_with(|| a.index.cmp(&b.index))
    });

    // Determina os padrões.
    let mut default_font_style = font_style::NONE as i32;
    let mut default_foreground = String::from("000000");
    let mut default_background = String::from("ffffff");
    let mut first_rule = 0;
    while first_rule < parsed_theme_rules.len() && parsed_theme_rules[first_rule].token.is_empty() {
        let incoming_defaults = &parsed_theme_rules[first_rule];
        if incoming_defaults.font_style != font_style::NOT_SET {
            default_font_style = incoming_defaults.font_style;
        }
        if let Some(foreground) = &incoming_defaults.foreground {
            default_foreground.clone_from(foreground);
        }
        if let Some(background) = &incoming_defaults.background {
            default_background.clone_from(background);
        }
        first_rule += 1;
    }
    let mut color_map = ColorMap::new();

    // Começa pelas cores de token dos temas de token customizados.
    for color in custom_token_colors {
        color_map.get_id(Some(color))?;
    }

    let foreground_color_id = color_map.get_id(Some(&default_foreground))?;
    let background_color_id = color_map.get_id(Some(&default_background))?;

    let defaults = ThemeTrieElementRule::new(
        default_font_style,
        foreground_color_id,
        background_color_id,
    );
    let mut root = ThemeTrieElement::new(defaults);
    for rule in &parsed_theme_rules[first_rule..] {
        let foreground = color_map.get_id(rule.foreground.as_deref())?;
        let background = color_map.get_id(rule.background.as_deref())?;
        root.insert(&rule.token, rule.font_style, foreground, background);
    }

    Ok(TokenTheme::new(color_map, root))
}

/// O `colorRegExp` do original, `^#?([0-9A-Fa-f]{6})([0-9A-Fa-f]{2})?$`: devolve o grupo 1 (os seis dígitos).
fn match_color(color: &str) -> Option<&str> {
    let digits = color.strip_prefix('#').unwrap_or(color);
    let bytes = digits.as_bytes();
    if (bytes.len() == 6 || bytes.len() == 8) && bytes.iter().all(u8::is_ascii_hexdigit) {
        Some(&digits[..6])
    } else {
        None
    }
}

/// `ColorMap`.
#[derive(Clone, Debug, Default)]
pub struct ColorMap {
    last_color_id: u32,
    /// O índice 0 fica vazio (`None`), como o buraco do array esparso do original.
    id2color: Vec<Option<Color>>,
    color2id: HashMap<String, u32>,
}

impl ColorMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getId`: o id da cor (0 para `None`); a cor ganha um id novo na primeira vez que aparece. O alfa
    /// (os dois dígitos finais) é descartado.
    pub fn get_id(&mut self, color: Option<&str>) -> Result<u32, IllegalTokenColorError> {
        let Some(color) = color else {
            return Ok(0);
        };
        let Some(digits) = match_color(color) else {
            return Err(IllegalTokenColorError(color.to_owned()));
        };
        let color = digits.to_uppercase();
        if let Some(&value) = self.color2id.get(&color) {
            return Ok(value);
        }
        self.last_color_id += 1;
        let value = self.last_color_id;
        let parsed = Color::from_hex(&format!("#{color}"));
        self.color2id.insert(color, value);
        if self.id2color.len() <= value as usize {
            self.id2color.resize(value as usize + 1, None);
        }
        self.id2color[value as usize] = Some(parsed);
        Ok(value)
    }

    pub fn get_color_map(&self) -> Vec<Option<Color>> {
        self.id2color.clone()
    }
}

/// `TokenTheme`.
#[derive(Debug)]
pub struct TokenTheme {
    color_map: ColorMap,
    root: ThemeTrieElement,
    cache: RefCell<HashMap<String, u32>>,
}

impl TokenTheme {
    pub fn create_from_raw_token_theme(
        source: &[ITokenThemeRule],
        custom_token_colors: &[String],
    ) -> Result<TokenTheme, IllegalTokenColorError> {
        Self::create_from_parsed_token_theme(parse_token_theme(source), custom_token_colors)
    }

    pub fn create_from_parsed_token_theme(
        source: Vec<ParsedTokenThemeRule>,
        custom_token_colors: &[String],
    ) -> Result<TokenTheme, IllegalTokenColorError> {
        resolve_parsed_token_theme_rules(source, custom_token_colors)
    }

    pub fn new(color_map: ColorMap, root: ThemeTrieElement) -> Self {
        Self {
            color_map,
            root,
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn get_color_map(&self) -> Vec<Option<Color>> {
        self.color_map.get_color_map()
    }

    /// `getThemeTrieElement`: usado nos testes.
    pub fn get_theme_trie_element(&self) -> ExternalThemeTrieElement {
        self.root.to_external_theme_trie_element()
    }

    /// `_match`.
    pub fn match_rule(&self, token: &str) -> &ThemeTrieElementRule {
        self.root.match_rule(token)
    }

    /// `match`: o metadado do token com a linguagem.
    pub fn match_token(&self, language_id: u32, token: &str) -> u32 {
        // O cache guarda o metadado sem os bits da linguagem.
        let cached = self.cache.borrow().get(token).copied();
        let result = match cached {
            Some(result) => result,
            None => {
                let rule = self.match_rule(token);
                let standard_token = to_standard_token_type(token);
                let result = rule.metadata | (standard_token << metadata_consts::TOKEN_TYPE_OFFSET);
                self.cache.borrow_mut().insert(token.to_owned(), result);
                result
            }
        };

        result | (language_id << metadata_consts::LANGUAGEID_OFFSET)
    }
}

/// Se `byte` é um caractere de palavra do JavaScript (`\w`: ASCII, letra, dígito ou `_`).
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `toStandardTokenType`: a primeira ocorrência, como palavra inteira, de `comment`, `string`, `regex` ou
/// `regexp` (`\b(comment|string|regex|regexp)\b`), com as alternativas tentadas nesta ordem.
pub fn to_standard_token_type(token_type: &str) -> u32 {
    const ALTERNATIVES: [(&[u8], u32); 4] = [
        (b"comment", standard_token_type::COMMENT),
        (b"string", standard_token_type::STRING),
        (b"regex", standard_token_type::REG_EX),
        (b"regexp", standard_token_type::REG_EX),
    ];
    let bytes = token_type.as_bytes();
    for start in 0..bytes.len() {
        if start > 0 && is_word_byte(bytes[start - 1]) {
            continue;
        }
        for (word, standard_type) in ALTERNATIVES {
            let end = start + word.len();
            if bytes[start..].starts_with(word) && bytes.get(end).is_none_or(|&b| !is_word_byte(b)) {
                return standard_type;
            }
        }
    }
    standard_token_type::OTHER
}

/// `strcmp`: compara por unidades de código UTF-16, como o `<` do JavaScript. Devolve -1, 0 ou 1.
pub fn strcmp(a: &str, b: &str) -> i32 {
    match a.encode_utf16().cmp(b.encode_utf16()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `ThemeTrieElementRule`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeTrieElementRule {
    font_style: i32,
    foreground: u32,
    background: u32,
    pub metadata: u32,
}

impl ThemeTrieElementRule {
    pub fn new(font_style: i32, foreground: u32, background: u32) -> Self {
        Self {
            font_style,
            foreground,
            background,
            metadata: Self::compute_metadata(font_style, foreground, background),
        }
    }

    fn compute_metadata(font_style: i32, foreground: u32, background: u32) -> u32 {
        ((font_style << metadata_consts::FONT_STYLE_OFFSET) as u32)
            | (foreground << metadata_consts::FOREGROUND_OFFSET)
            | (background << metadata_consts::BACKGROUND_OFFSET)
    }

    pub fn accept_overwrite(&mut self, font_style: i32, foreground: u32, background: u32) {
        if font_style != font_style::NOT_SET {
            self.font_style = font_style;
        }
        if foreground != color_id::NONE {
            self.foreground = foreground;
        }
        if background != color_id::NONE {
            self.background = background;
        }
        self.metadata = Self::compute_metadata(self.font_style, self.foreground, self.background);
    }
}

/// `ExternalThemeTrieElement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalThemeTrieElement {
    pub main_rule: ThemeTrieElementRule,
    pub children: HashMap<String, ExternalThemeTrieElement>,
}

impl ExternalThemeTrieElement {
    pub fn new(main_rule: ThemeTrieElementRule) -> Self {
        Self {
            main_rule,
            children: HashMap::new(),
        }
    }

    /// O construtor do original com os filhos dados como objeto.
    pub fn with_children<K: Into<String>>(
        main_rule: ThemeTrieElementRule,
        children: impl IntoIterator<Item = (K, ExternalThemeTrieElement)>,
    ) -> Self {
        Self {
            main_rule,
            children: children
                .into_iter()
                .map(|(key, element)| (key.into(), element))
                .collect(),
        }
    }
}

/// Separa `token` no primeiro ponto: a cabeça e o resto (vazio se não há ponto).
fn split_head(token: &str) -> (&str, &str) {
    token.split_once('.').unwrap_or((token, ""))
}

/// `ThemeTrieElement`.
#[derive(Clone, Debug)]
pub struct ThemeTrieElement {
    main_rule: ThemeTrieElementRule,
    children: HashMap<String, ThemeTrieElement>,
}

impl ThemeTrieElement {
    pub fn new(main_rule: ThemeTrieElementRule) -> Self {
        Self {
            main_rule,
            children: HashMap::new(),
        }
    }

    /// `toExternalThemeTrieElement`: usado nos testes.
    pub fn to_external_theme_trie_element(&self) -> ExternalThemeTrieElement {
        ExternalThemeTrieElement {
            main_rule: self.main_rule.clone(),
            children: self
                .children
                .iter()
                .map(|(index, element)| (index.clone(), element.to_external_theme_trie_element()))
                .collect(),
        }
    }

    /// `match`.
    pub fn match_rule(&self, token: &str) -> &ThemeTrieElementRule {
        if token.is_empty() {
            return &self.main_rule;
        }

        let (head, tail) = split_head(token);
        match self.children.get(head) {
            Some(child) => child.match_rule(tail),
            None => &self.main_rule,
        }
    }

    pub fn insert(&mut self, token: &str, font_style: i32, foreground: u32, background: u32) {
        if token.is_empty() {
            // Mescla na regra principal.
            self.main_rule
                .accept_overwrite(font_style, foreground, background);
            return;
        }

        let (head, tail) = split_head(token);
        let main_rule = &self.main_rule;
        let child = self
            .children
            .entry(head.to_owned())
            .or_insert_with(|| ThemeTrieElement::new(main_rule.clone()));
        child.insert(tail, font_style, foreground, background);
    }
}

/// `generateTokensCSSForColorMap`. Um índice sem cor (o 0 do mapa do `TokenTheme`) seria `undefined` no
/// `${color}` do original, por isso sai como `undefined`.
pub fn generate_tokens_css_for_color_map(color_map: &[Option<Color>]) -> String {
    let mut rules: Vec<String> = Vec::new();
    if color_map.len() >= 2 {
        // `rules[i] = ...` a partir de 1 deixa o índice 0 vazio, que o `join` escreve como texto vazio.
        rules.push(String::new());
    }
    for (i, color) in color_map.iter().enumerate().skip(1) {
        let color = color
            .as_ref()
            .map_or_else(|| String::from("undefined"), ToString::to_string);
        rules.push(format!(".mtk{i} {{ color: {color}; }}"));
    }
    rules.push(".mtki { font-style: italic; }".to_owned());
    rules.push(".mtkb { font-weight: bold; }".to_owned());
    rules.push(
        ".mtku { text-decoration: underline; text-underline-position: under; }".to_owned(),
    );
    rules.push(".mtks { text-decoration: line-through; }".to_owned());
    rules.push(
        ".mtks.mtku { text-decoration: underline line-through; text-underline-position: under; }"
            .to_owned(),
    );
    rules.join("\n")
}

/// `IFontTokenOptions` (de `platform/theme/common/themeService.ts`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IFontTokenOptions {
    pub font_family: Option<String>,
    pub font_size_multiplier: Option<f64>,
    pub line_height_multiplier: Option<f64>,
}

/// `generateTokensCSSForFontMap`. Os números saem com o `Display` do Rust, que coincide com o
/// `Number.prototype.toString` do JavaScript fora de expoentes (abaixo de 1e-6 ou a partir de 1e21).
pub fn generate_tokens_css_for_font_map(font_map: &[IFontTokenOptions]) -> String {
    let mut rules: Vec<String> = Vec::new();
    let mut fonts: HashSet<String> = HashSet::new();
    for font in font_map.iter().skip(1) {
        let font_family = font.font_family.as_deref().filter(|f| !f.is_empty());
        // Falsy no JS: ausente, 0 ou NaN.
        let size_multiplier = font
            .font_size_multiplier
            .filter(|m| *m != 0.0 && !m.is_nan());
        if font_family.is_none() && size_multiplier.is_none() {
            continue;
        }
        let class_name = class_name_for_font_token_decorations(
            font_family.unwrap_or(""),
            size_multiplier.unwrap_or(0.0),
        );
        if !fonts.insert(class_name.clone()) {
            continue;
        }
        let mut rule = format!(".{class_name} {{");
        if let Some(font_family) = font_family {
            rule.push_str(&format!("font-family: {font_family};"));
        }
        if let Some(multiplier) = size_multiplier {
            rule.push_str(&format!(
                "font-size: calc(var(--editor-font-size)*{multiplier});"
            ));
        }
        rule.push('}');
        rules.push(rule);
    }
    rules.join("\n")
}

/// `classNameForFontTokenDecorations`.
pub fn class_name_for_font_token_decorations(font_family: &str, font_size: f64) -> String {
    let safe_font_family = sanitize_font_family_for_class_name(font_family);
    clean_class_name(&format!("font-decoration-{safe_font_family}-{font_size}"))
}

/// Os espaços do `String.prototype.trim` do JavaScript (`WhiteSpace` e `LineTerminator`).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{000B}'
            | '\u{000C}'
            | '\r'
            | ' '
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

fn sanitize_font_family_for_class_name(font_family: &str) -> String {
    let lowered = font_family.to_lowercase();
    let normalized = lowered.trim_matches(is_js_whitespace);
    if normalized.is_empty() {
        return "default".to_owned();
    }
    clean_class_name(normalized)
}

/// `className.replace(/[^a-z0-9_-]/gi, '-')`: o JavaScript troca por unidade UTF-16, então um caractere
/// fora do plano básico vira dois hífens.
fn clean_class_name(class_name: &str) -> String {
    let mut cleaned = String::with_capacity(class_name.len());
    for c in class_name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            cleaned.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                cleaned.push('-');
            }
        }
    }
    cleaned
}
