//! Os mnemônicos de rótulo (`&&`) de `menu.ts`: as regexes `MENU_MNEMONIC_REGEX` e
//! `MENU_ESCAPED_MNEMONIC_REGEX`, `cleanMnemonic` e a quebra de um rótulo em trechos (antes, letra
//! sublinhada, depois) que o menu e a barra de menus montam no DOM.
//!
//! As regexes são escritas à mão sobre `char` (o original usa unidades UTF-16): a diferença só aparece num
//! mnemônico fora do plano básico, que o `[^\s&]` do JavaScript partiria ao meio.

/// `\s` do JavaScript.
pub fn is_js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

/// `String#trim` do JavaScript.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// `[^\s&]`.
fn is_mnemonic_char(c: char) -> bool {
    !is_js_space(c) && c != '&'
}

/// O casamento de `MENU_MNEMONIC_REGEX` (`/\(&([^\s&])\)|(^|[^&])&([^\s&])/`), com índices em `char`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MnemonicMatch {
    pub index: usize,
    pub len: usize,
    /// O grupo 1: a letra de `(&F)`.
    pub group1: Option<char>,
    /// O grupo 2: o caractere antes do `&` (vazio no começo do texto).
    pub group2: Option<char>,
    /// O grupo 3: a letra depois do `&`.
    pub group3: Option<char>,
}

impl MnemonicMatch {
    /// `(!!matches[1] ? matches[1] : matches[3])`.
    pub fn letter(&self) -> char {
        self.group1.or(self.group3).expect("todo casamento tem a letra")
    }
}

/// `MENU_MNEMONIC_REGEX.exec(label)`.
pub fn exec_menu_mnemonic(label: &str) -> Option<MnemonicMatch> {
    let chars: Vec<char> = label.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == '('
            && chars.get(i + 1) == Some(&'&')
            && let Some(&letter) = chars.get(i + 2).filter(|c| is_mnemonic_char(**c))
            && chars.get(i + 3) == Some(&')')
        {
            return Some(MnemonicMatch { index: i, len: 4, group1: Some(letter), group2: None, group3: None });
        }
        // `(^|[^&])&([^\s&])`: primeiro o `^` (só no começo), depois um caractere que não é `&`.
        if i == 0
            && chars[0] == '&'
            && let Some(&letter) = chars.get(1).filter(|c| is_mnemonic_char(**c))
        {
            return Some(MnemonicMatch { index: 0, len: 2, group1: None, group2: None, group3: Some(letter) });
        }
        if chars[i] != '&'
            && chars.get(i + 1) == Some(&'&')
            && let Some(&letter) = chars.get(i + 2).filter(|c| is_mnemonic_char(**c))
        {
            return Some(MnemonicMatch { index: i, len: 3, group1: None, group2: Some(chars[i]), group3: Some(letter) });
        }
    }
    None
}

/// O mnemônico de um rótulo, em minúsculas (`this.mnemonic = (...).toLocaleLowerCase()`).
pub fn mnemonic_of(label: &str) -> Option<String> {
    exec_menu_mnemonic(label).map(|m| m.letter().to_lowercase().collect())
}

/// `cleanMnemonic(label)`.
pub fn clean_mnemonic(label: &str) -> String {
    let Some(matches) = exec_menu_mnemonic(label) else { return label.to_owned() };
    let chars: Vec<char> = label.chars().collect();
    let mut out: String = chars[..matches.index].iter().collect();
    // `mnemonicInText ? '$2$3' : ''`
    if matches.group1.is_none() {
        out.extend(matches.group2);
        out.extend(matches.group3);
    }
    out.extend(&chars[matches.index + matches.len..]);
    js_trim(&out).to_owned()
}

/// `strings.escape(text)`: `&`, `<` e `>` viram entidades.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `str.replace(/&amp;&amp;/g, '&amp;')`.
pub fn replace_double_escapes(text: &str) -> String {
    text.replace("&amp;&amp;", "&amp;")
}

/// `str.replace(/&&/g, '&')`.
pub fn unescape_double_ampersands(text: &str) -> String {
    text.replace("&&", "&")
}

/// O casamento de `MENU_ESCAPED_MNEMONIC_REGEX` (`/(&amp;)?(&amp;)([^\s&])/g`).
struct EscapedMatch {
    index: usize,
    len: usize,
    group1: bool,
    group3: char,
}

fn exec_escaped_mnemonic(chars: &[char], from: usize) -> Option<EscapedMatch> {
    const AMP: [char; 5] = ['&', 'a', 'm', 'p', ';'];
    let at = |i: usize| chars.get(i..i + 5) == Some(&AMP[..]);
    for i in from..chars.len() {
        if at(i) && at(i + 5) && chars.get(i + 10).is_some_and(|c| is_mnemonic_char(*c)) {
            return Some(EscapedMatch { index: i, len: 11, group1: true, group3: chars[i + 10] });
        }
        if at(i) && chars.get(i + 5).is_some_and(|c| is_mnemonic_char(*c)) {
            return Some(EscapedMatch { index: i, len: 6, group1: false, group3: chars[i + 5] });
        }
    }
    None
}

/// Um rótulo com mnemônico dividido como `updateLabel` o monta: o texto antes, a letra (o `<u>` do menu ou o
/// `<mnemonic>` da barra) e o texto depois.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelPieces {
    /// `textContent = ...`: um trecho só.
    Plain(String),
    Mnemonic { before: String, letter: String, after: String },
}

impl LabelPieces {
    /// O `textContent` do rótulo montado.
    pub fn text_content(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Mnemonic { before, letter, after } => format!("{before}{letter}{after}"),
        }
    }
}

/// O trecho de `updateLabel` (do menu e da barra) que escapa o rótulo, acha o primeiro mnemônico que não
/// seja `&&` e monta os trechos. `escaped` é o rótulo já passado por [`escape_html`].
pub fn split_escaped_label(escaped: &str) -> LabelPieces {
    let chars: Vec<char> = escaped.chars().collect();
    // O `lastIndex` é global: o laço continua de onde o casamento anterior terminou.
    let mut from = 0;
    let mut found = exec_escaped_mnemonic(&chars, from);
    // Sem lookbehind negativo: se casou o `(&amp;)?` (um `&&`), pula.
    while let Some(m) = found.as_ref().filter(|m| m.group1) {
        from = m.index + m.len;
        found = exec_escaped_mnemonic(&chars, from);
    }
    match found {
        Some(m) => {
            let before: String = chars[..m.index].iter().collect();
            let after: String = chars[m.index + m.len..].iter().collect();
            LabelPieces::Mnemonic {
                before: replace_double_escapes(&before).trim_start_matches(' ').to_owned(),
                letter: m.group3.to_string(),
                after: replace_double_escapes(&after).trim_end_matches(' ').to_owned(),
            }
        }
        None => LabelPieces::Plain(js_trim(&replace_double_escapes(escaped)).to_owned()),
    }
}

/// O rótulo do item de menu depois de `stripIcons` (`updateLabel` do `BaseMenuActionViewItem`): o texto de
/// `aria-label` e os trechos. `enable_mnemonics` falso apaga os mnemônicos antes.
pub fn menu_item_label(stripped: &str, enable_mnemonics: bool) -> (String, LabelPieces, Option<String>) {
    let clean = clean_mnemonic(stripped);
    let label = if enable_mnemonics { stripped.to_owned() } else { clean.clone() };
    let aria_label = unescape_double_ampersands(&clean);
    match exec_menu_mnemonic(&label) {
        Some(matches) => {
            let mnemonic = matches.letter().to_lowercase().collect::<String>();
            (aria_label, split_escaped_label(&escape_html(&label)), Some(mnemonic))
        }
        None => (aria_label, LabelPieces::Plain(js_trim(&unescape_double_ampersands(&label)).to_owned()), None),
    }
}
