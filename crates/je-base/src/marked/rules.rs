//! As gramáticas de bloco e de linha do `marked` (v14.0.0), montadas pela mesma sequência de
//! `edit(...).replace(...)` do original, para que cada regex tenha o mesmo fonte que no JavaScript.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::Options;
use super::re::Re;

/// O `edit()` do `marked`: parte de um fonte e troca nomes por fontes de outras regexes.
struct Edit {
    source: String,
    flags: &'static str,
}

fn edit(source: &str, flags: &'static str) -> Edit {
    Edit {
        source: source.to_owned(),
        flags,
    }
}

thread_local! {
    static CARET: Re = Re::new(r"(^|[^\[])\^", "g");
}

/// `valSource.replace(caret, '$1')`: tira os `^` que não abrem uma classe negada.
fn strip_carets(value: &str) -> String {
    CARET.with(|caret| {
        let input: Vec<u16> = value.encode_utf16().collect();
        let out = caret.replace_all(&input, |c| crate::JsString::from(c.g(1).unwrap_or_default()));
        out.to_string_lossy()
    })
}

/// `GetSubstitution` de `String.prototype.replace` para um padrão em texto: só `$$`, `$&`, `` $` `` e `$'`
/// têm significado (sem grupos de captura, `$1` fica literal).
fn expand(replacement: &str, matched: &str, before: &str, after: &str) -> String {
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('&') => {
                chars.next();
                out.push_str(matched);
            }
            Some('`') => {
                chars.next();
                out.push_str(before);
            }
            Some('\'') => {
                chars.next();
                out.push_str(after);
            }
            _ => out.push('$'),
        }
    }
    out
}

impl Edit {
    /// `.replace('name', val)`: só a primeira ocorrência do nome.
    fn replace(mut self, name: &str, value: &str) -> Edit {
        let value = strip_carets(value);
        if let Some(at) = self.source.find(name) {
            let (before, rest) = self.source.split_at(at);
            let after = &rest[name.len()..];
            let inserted = expand(&value, name, before, after);
            self.source = format!("{before}{inserted}{after}");
        }
        self
    }

    /// `.replace(/name/g, val)` com um nome literal: todas as ocorrências.
    fn replace_all(mut self, name: &str, value: &str) -> Edit {
        let value = strip_carets(value);
        let mut out = String::with_capacity(self.source.len());
        let mut rest = self.source.as_str();
        while let Some(at) = rest.find(name) {
            let (before, tail) = rest.split_at(at);
            let after = &tail[name.len()..];
            out.push_str(before);
            // O `before` e o `after` do `$\`` e do `$'` são os do texto inteiro, não do pedaço.
            let whole_before = &self.source[..self.source.len() - rest.len() + at];
            out.push_str(&expand(&value, name, whole_before, after));
            rest = after;
        }
        out.push_str(rest);
        self.source = out;
        self
    }

    fn get(self) -> String {
        self.source
    }

    fn flags(&self) -> &'static str {
        self.flags
    }
}

// Fontes de regex em texto (o `.source` das regexes literais do original).
const NEWLINE: &str = r"^(?: *(?:\n|$))+";
const BLOCK_CODE: &str = r"^( {4}[^\n]+(?:\n(?: *(?:\n|$))*)?)+";
const FENCES: &str =
    r"^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)";
const HR: &str = r"^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)";
const HEADING: &str = r"^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)";
const BULLET: &str = r"(?:[*+-]|\d{1,9}[.)])";
const PARAGRAPH_SRC: &str = r"^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table| +\n)[^\n]+)*)";
const BLOCK_TEXT: &str = r"^[^\n]+";
const BLOCK_LABEL: &str = r"(?!\s*\])(?:\\.|[^\[\]\\])+";
const TAG: &str = "address|article|aside|base|basefont|blockquote|body|caption\
    |center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption\
    |figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe\
    |legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option\
    |p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title\
    |tr|track|ul";
const COMMENT: &str = r"<!--(?:-?>|[\s\S]*?(?:-->|$))";

const PUNCTUATION_CLASS: &str = r"\p{P}\p{S}";

/// Regras de bloco. `None` é o `noopTest` do original (`exec` que devolve `null`).
pub(crate) struct BlockRules {
    pub(crate) blockquote: Re,
    pub(crate) code: Re,
    pub(crate) def: Re,
    pub(crate) fences: Option<Re>,
    pub(crate) heading: Re,
    pub(crate) hr: Re,
    pub(crate) html: Re,
    pub(crate) lheading: Re,
    pub(crate) list: Re,
    pub(crate) newline: Re,
    pub(crate) paragraph: Re,
    pub(crate) table: Option<Re>,
    pub(crate) text: Re,
}

/// Regras de linha.
pub(crate) struct InlineRules {
    pub(crate) backpedal: Option<Re>,
    pub(crate) any_punctuation: Re,
    pub(crate) autolink: Re,
    pub(crate) block_skip: Re,
    pub(crate) br: Re,
    pub(crate) code: Re,
    pub(crate) del: Option<Re>,
    pub(crate) em_strong_ldelim: Re,
    pub(crate) em_strong_rdelim_ast: Re,
    pub(crate) em_strong_rdelim_und: Re,
    pub(crate) escape: Re,
    pub(crate) link: Re,
    pub(crate) nolink: Re,
    pub(crate) punctuation: Re,
    pub(crate) reflink: Re,
    pub(crate) reflink_search: Re,
    pub(crate) tag: Re,
    pub(crate) text: Re,
    pub(crate) url: Option<Re>,
}

pub(crate) struct Rules {
    pub(crate) block: BlockRules,
    pub(crate) inline: InlineRules,
}

fn anchored(source: &str, flags: &str) -> Re {
    Re::anchored(source, flags)
}

fn lheading_source() -> String {
    edit(
        r"^(?!bull |blockCode|fences|blockquote|heading|html)((?:.|\n(?!\s*?\n|bull |blockCode|fences|blockquote|heading|html))+?)\n {0,3}(=+|-+) *(?:\n+|$)",
        "",
    )
    .replace_all("bull", BULLET)
    .replace_all("blockCode", r" {4}")
    .replace_all("fences", r" {0,3}(?:`{3,}|~{3,})")
    .replace_all("blockquote", r" {0,3}>")
    .replace_all("heading", r" {0,3}#{1,6}")
    .replace_all("html", r" {0,3}<[^\n>]+>\n")
    .get()
}

fn def_source() -> String {
    edit(
        r"^ {0,3}\[(label)\]: *(?:\n *)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n *)?| *\n *)(title))? *(?:\n+|$)",
        "",
    )
    .replace("label", BLOCK_LABEL)
    .replace(
        "title",
        r#"(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))"#,
    )
    .get()
}

fn list_source() -> String {
    edit(r"^( {0,3}bull)([ \t][^\n]+?)?(?:\n|$)", "")
        .replace_all("bull", BULLET)
        .get()
}

fn html_source() -> (String, &'static str) {
    let e = edit(
        &[
            "^ {0,3}(?:",
            r"<(script|pre|style|textarea)[\s>][\s\S]*?(?:</\1>[^\n]*\n+|$)",
            r"|comment[^\n]*(\n+|$)",
            r"|<\?[\s\S]*?(?:\?>\n*|$)",
            r"|<![A-Z][\s\S]*?(?:>\n*|$)",
            r"|<!\[CDATA\[[\s\S]*?(?:\]\]>\n*|$)",
            r"|</?(tag)(?: +|\n|/?>)[\s\S]*?(?:(?:\n *)+\n|$)",
            r"|<(?!script|pre|style|textarea)([a-z][\w-]*)(?:attribute)*? */?>(?=[ \t]*(?:\n|$))[\s\S]*?(?:(?:\n *)+\n|$)",
            r"|</(?!script|pre|style|textarea)[a-z][\w-]*\s*>(?=[ \t]*(?:\n|$))[\s\S]*?(?:(?:\n *)+\n|$)",
            ")",
        ]
        .concat(),
        "i",
    )
    .replace("comment", COMMENT)
    .replace("tag", TAG)
    .replace("attribute", r#" +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?"#);
    let flags = e.flags();
    (e.get(), flags)
}

fn paragraph_source(table: Option<&str>) -> String {
    let e = edit(PARAGRAPH_SRC, "")
        .replace("hr", HR)
        .replace("heading", r" {0,3}#{1,6}(?:\s|$)")
        .replace("|lheading", "");
    let e = match table {
        None => e.replace("|table", ""),
        Some(table) => e.replace("table", table),
    };
    e.replace("blockquote", " {0,3}>")
        .replace("fences", r" {0,3}(?:`{3,}(?=[^`\n]*\n)|~{3,})[^\n]*\n")
        .replace("list", r" {0,3}(?:[*+-]|1[.)]) ")
        .replace("html", r"</?(?:tag)(?: +|\n|/?>)|<(?:script|pre|style|textarea|!--)")
        .replace("tag", TAG)
        .get()
}

fn blockquote_source(paragraph: &str) -> String {
    edit(r"^( {0,3}> ?(paragraph|[^\n]*)(?:\n|$))+", "")
        .replace("paragraph", paragraph)
        .get()
}

fn gfm_table_source() -> String {
    edit(
        &[
            r"^ *([^\n ].*)\n",
            r" {0,3}((?:\| *)?:?-+:? *(?:\| *:?-+:? *)*(?:\| *)?)",
            r"(?:\n((?:(?! *\n|hr|heading|blockquote|code|fences|list|html).*(?:\n|$))*)\n*|$)",
        ]
        .concat(),
        "",
    )
    .replace("hr", HR)
    .replace("heading", r" {0,3}#{1,6}(?:\s|$)")
    .replace("blockquote", " {0,3}>")
    .replace("code", r" {4}[^\n]")
    .replace("fences", r" {0,3}(?:`{3,}(?=[^`\n]*\n)|~{3,})[^\n]*\n")
    .replace("list", r" {0,3}(?:[*+-]|1[.)]) ")
    .replace("html", r"</?(?:tag)(?: +|\n|/?>)|<(?:script|pre|style|textarea|!--)")
    .replace("tag", TAG)
    .get()
}

fn pedantic_html_source() -> String {
    edit(
        &[
            "^ *(?:comment *(?:\\n|\\s*$)",
            r"|<(tag)[\s\S]+?</\1> *(?:\n{2,}|\s*$)",
            r#"|<tag(?:"[^"]*"|'[^']*'|\s[^'"/>\s]*)*?/?> *(?:\n{2,}|\s*$))"#,
        ]
        .concat(),
        "",
    )
    .replace("comment", COMMENT)
    .replace_all(
        "tag",
        &[
            "(?!(?:",
            "a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd|sub",
            "|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)",
            r"\b)\w+(?!:|[^\w\s@]*@)\b",
        ]
        .concat(),
    )
    .get()
}

fn pedantic_paragraph_source(lheading: &str) -> String {
    edit(PARAGRAPH_SRC, "")
        .replace("hr", HR)
        .replace("heading", " *#{1,6} *[^\n]")
        .replace("lheading", lheading)
        .replace("|table", "")
        .replace("blockquote", " {0,3}>")
        .replace("|fences", "")
        .replace("|list", "")
        .replace("|html", "")
        .replace("|tag", "")
        .get()
}

fn build_block(gfm: bool, pedantic: bool) -> BlockRules {
    let paragraph_normal = paragraph_source(None);
    let lheading_normal = lheading_source();
    let gfm_table = gfm_table_source();

    // blockNormal, com `table` e `paragraph` trocados conforme o modo.
    let (html, html_flags) = html_source();
    let mut html_re = (html, html_flags);
    let mut def = def_source();
    let mut heading = HEADING.to_owned();
    let mut fences = Some(FENCES.to_owned());
    let mut lheading = lheading_normal.clone();
    let mut table = None;
    let mut paragraph = paragraph_normal;

    if pedantic {
        html_re = (pedantic_html_source(), "");
        def = r#"^ *\[([^\]]+)\]: *<?([^\s>]+)>?(?: +(["(][^\n]+[")]))? *(?:\n+|$)"#.to_owned();
        heading = r"^(#{1,6})(.*)(?:\n+|$)".to_owned();
        fences = None;
        lheading = r"^(.+?)\n {0,3}(=+|-+) *(?:\n+|$)".to_owned();
        paragraph = pedantic_paragraph_source(&lheading_normal);
    } else if gfm {
        table = Some(gfm_table.clone());
        paragraph = paragraph_source(Some(&gfm_table));
    }
    // O `blockquote` usa o `paragraph` do modo normal (`blockNormal.blockquote` é calculado antes da
    // troca de `paragraph` nos modos gfm e pedantic).
    let blockquote = blockquote_source(&paragraph_source(None));

    BlockRules {
        blockquote: anchored(&blockquote, ""),
        code: anchored(BLOCK_CODE, ""),
        def: anchored(&def, ""),
        fences: fences.map(|f| anchored(&f, "")),
        heading: anchored(&heading, ""),
        hr: anchored(HR, ""),
        html: anchored(&html_re.0, html_re.1),
        lheading: anchored(&lheading, ""),
        list: anchored(&list_source(), ""),
        newline: anchored(NEWLINE, ""),
        paragraph: anchored(&paragraph, ""),
        table: table.map(|t| anchored(&t, "")),
        text: anchored(BLOCK_TEXT, ""),
    }
}

const ESCAPE: &str = r##"^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])"##;
const INLINE_CODE: &str = r"^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)";
const BR: &str = r"^( {2,}|\\)\n(?!\s*$)";
const INLINE_TEXT: &str = r"^(`+|[^`])(?:(?= {2,}\n)|[\s\S]*?(?:(?=[\\<!\[`*_]|\b_|$)|[^ ](?= {2,}\n)))";
const BLOCK_SKIP: &str = r"\[[^[\]]*?\]\([^\(\)]*?\)|`[^`]*?`|<[^<>]*?>";
const INLINE_LABEL: &str = r"(?:\[(?:\\.|[^\[\]\\])*\]|\\.|`[^`]*`|[^\[\]\\`])*?";
const GFM_TEXT: &str = r"^([`~]+|[^`~])(?:(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|https?:\/\/|ftp:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)))";

fn em_strong_rdelim_ast() -> String {
    edit(
        &[
            r"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)",
            r"|[^*]+(?=[^*])",
            r"|(?!\*)[punct](\*+)(?=[\s]|$)",
            r"|[^punct\s](\*+)(?!\*)(?=[punct\s]|$)",
            r"|(?!\*)[punct\s](\*+)(?=[^punct\s])",
            r"|[\s](\*+)(?!\*)(?=[punct])",
            r"|(?!\*)[punct](\*+)(?!\*)(?=[punct])",
            r"|[^punct\s](\*+)(?=[^punct\s])",
        ]
        .concat(),
        "gu",
    )
    .replace_all("punct", PUNCTUATION_CLASS)
    .get()
}

fn em_strong_rdelim_und() -> String {
    edit(
        &[
            r"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)",
            r"|[^_]+(?=[^_])",
            r"|(?!_)[punct](_+)(?=[\s]|$)",
            r"|[^punct\s](_+)(?!_)(?=[punct\s]|$)",
            r"|(?!_)[punct\s](_+)(?=[^punct\s])",
            r"|[\s](_+)(?!_)(?=[punct])",
            r"|(?!_)[punct](_+)(?!_)(?=[punct])",
        ]
        .concat(),
        "gu",
    )
    .replace_all("punct", PUNCTUATION_CLASS)
    .get()
}

fn autolink_source() -> String {
    edit(r"^<(scheme:[^\s\x00-\x1f<>]*|email)>", "")
        .replace("scheme", r"[a-zA-Z][a-zA-Z0-9+.-]{1,31}")
        .replace(
            "email",
            r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])",
        )
        .get()
}

fn tag_source() -> String {
    let inline_comment = edit(COMMENT, "").replace("(?:-->|$)", "-->").get();
    edit(
        &[
            "^comment",
            r"|^</[a-zA-Z][\w:-]*\s*>",
            r"|^<[a-zA-Z][\w-]*(?:attribute)*?\s*/?>",
            r"|^<\?[\s\S]*?\?>",
            r"|^<![a-zA-Z]+\s[\s\S]*?>",
            r"|^<!\[CDATA\[[\s\S]*?\]\]>",
        ]
        .concat(),
        "",
    )
    .replace("comment", &inline_comment)
    .replace(
        "attribute",
        r#"\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?"#,
    )
    .get()
}

fn link_source() -> String {
    edit(r"^!?\[(label)\]\(\s*(href)(?:\s+(title))?\s*\)", "")
        .replace("label", INLINE_LABEL)
        .replace("href", r"<(?:\\.|[^\n<>\\])+>|[^\s\x00-\x1f]*")
        .replace(
            "title",
            r#""(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)"#,
        )
        .get()
}

fn reflink_source() -> String {
    edit(r"^!?\[(label)\]\[(ref)\]", "")
        .replace("label", INLINE_LABEL)
        .replace("ref", BLOCK_LABEL)
        .get()
}

fn nolink_source() -> String {
    edit(r"^!?\[(ref)\](?:\[\])?", "").replace("ref", BLOCK_LABEL).get()
}

fn build_inline(gfm: bool, breaks: bool, pedantic: bool) -> InlineRules {
    let punctuation = edit(r"^((?![*_])[\spunctuation])", "u")
        .replace_all("punctuation", PUNCTUATION_CLASS)
        .get();
    let em_strong_ldelim = edit(
        r"^(?:\*+(?:((?!\*)[punct])|[^\s*]))|^_+(?:((?!_)[punct])|([^\s_]))",
        "u",
    )
    .replace_all("punct", PUNCTUATION_CLASS)
    .get();
    let any_punctuation = edit(r"\\([punct])", "gu").replace_all("punct", PUNCTUATION_CLASS).get();

    let mut link = link_source();
    let mut reflink = reflink_source();
    let nolink = nolink_source();
    if pedantic {
        link = edit(r"^!?\[(label)\]\((.*?)\)", "")
            .replace("label", INLINE_LABEL)
            .get();
        reflink = edit(r"^!?\[(label)\]\s*\[([^\]]*)\]", "")
            .replace("label", INLINE_LABEL)
            .get();
    }
    // `reflinkSearch` vem de `inlineNormal`: o modo pedantic troca `link` e `reflink`, mas não este.
    let reflink_search = edit("reflink|nolink(?!\\()", "g")
        .replace("reflink", &reflink_source())
        .replace("nolink", &nolink)
        .get();

    let mut escape = ESCAPE.to_owned();
    let mut url = None;
    let mut backpedal = None;
    let mut del = None;
    let mut text = INLINE_TEXT.to_owned();
    let mut br = BR.to_owned();
    if !pedantic && gfm {
        escape = edit(ESCAPE, "").replace("])", "~|])").get();
        url = Some(
            edit(r"^((?:ftp|https?):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^email", "i")
                .replace(
                    "email",
                    r"[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])",
                )
                .get(),
        );
        backpedal = Some(r#"(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+"#.to_owned());
        del = Some(r"^(~~?)(?=[^\s~])([\s\S]*?[^\s~])\1(?=[^~]|$)".to_owned());
        text = GFM_TEXT.to_owned();
        if breaks {
            br = edit(BR, "").replace("{2,}", "*").get();
            text = edit(GFM_TEXT, "")
                .replace("\\b_", "\\b_| {2,}\\n")
                .replace_all("{2,}", "*")
                .get();
        }
    }

    InlineRules {
        backpedal: backpedal.map(|s| Re::new(&s, "")),
        any_punctuation: Re::new(&any_punctuation, "gu"),
        autolink: anchored(&autolink_source(), ""),
        block_skip: Re::new(BLOCK_SKIP, "g"),
        br: anchored(&br, ""),
        code: anchored(INLINE_CODE, ""),
        del: del.map(|s| anchored(&s, "")),
        em_strong_ldelim: anchored(&em_strong_ldelim, "u"),
        em_strong_rdelim_ast: Re::new(&em_strong_rdelim_ast(), "gu"),
        em_strong_rdelim_und: Re::new(&em_strong_rdelim_und(), "gu"),
        escape: anchored(&escape, ""),
        link: anchored(&link, ""),
        nolink: anchored(&nolink, ""),
        punctuation: anchored(&punctuation, "u"),
        reflink: anchored(&reflink, ""),
        reflink_search: Re::new(&reflink_search, "g"),
        tag: anchored(&tag_source(), ""),
        text: anchored(&text, ""),
        url: url.map(|s| anchored(&s, "i")),
    }
}

impl Rules {
    fn build(options: &Options) -> Rules {
        Rules {
            block: build_block(options.gfm, options.pedantic),
            inline: build_inline(options.gfm, options.breaks, options.pedantic),
        }
    }

    /// As regras do modo das opções, compiladas uma vez por thread.
    pub(crate) fn for_options(options: &Options) -> Rc<Rules> {
        thread_local! {
            static CACHE: RefCell<HashMap<(bool, bool, bool), Rc<Rules>>> = RefCell::new(HashMap::new());
        }
        CACHE.with(|cache| {
            let key = (options.gfm, options.breaks, options.pedantic);
            Rc::clone(
                cache
                    .borrow_mut()
                    .entry(key)
                    .or_insert_with(|| Rc::new(Rules::build(options))),
            )
        })
    }

    /// Nome, fonte e flags de cada regex, na ordem fixa que o golden usa.
    pub(crate) fn sources(&self) -> Vec<(&'static str, String, String)> {
        fn one(out: &mut Vec<(&'static str, String, String)>, name: &'static str, re: &Re) {
            out.push((name, re.source.clone(), re.flags.clone()));
        }
        fn opt(out: &mut Vec<(&'static str, String, String)>, name: &'static str, re: &Option<Re>) {
            match re {
                Some(re) => one(out, name, re),
                None => out.push((name, String::new(), "noop".to_owned())),
            }
        }
        let (b, i) = (&self.block, &self.inline);
        let mut out = Vec::new();
        one(&mut out, "block.blockquote", &b.blockquote);
        one(&mut out, "block.code", &b.code);
        one(&mut out, "block.def", &b.def);
        opt(&mut out, "block.fences", &b.fences);
        one(&mut out, "block.heading", &b.heading);
        one(&mut out, "block.hr", &b.hr);
        one(&mut out, "block.html", &b.html);
        one(&mut out, "block.lheading", &b.lheading);
        one(&mut out, "block.list", &b.list);
        one(&mut out, "block.newline", &b.newline);
        one(&mut out, "block.paragraph", &b.paragraph);
        opt(&mut out, "block.table", &b.table);
        one(&mut out, "block.text", &b.text);
        opt(&mut out, "inline._backpedal", &i.backpedal);
        one(&mut out, "inline.anyPunctuation", &i.any_punctuation);
        one(&mut out, "inline.autolink", &i.autolink);
        one(&mut out, "inline.blockSkip", &i.block_skip);
        one(&mut out, "inline.br", &i.br);
        one(&mut out, "inline.code", &i.code);
        opt(&mut out, "inline.del", &i.del);
        one(&mut out, "inline.emStrongLDelim", &i.em_strong_ldelim);
        one(&mut out, "inline.emStrongRDelimAst", &i.em_strong_rdelim_ast);
        one(&mut out, "inline.emStrongRDelimUnd", &i.em_strong_rdelim_und);
        one(&mut out, "inline.escape", &i.escape);
        one(&mut out, "inline.link", &i.link);
        one(&mut out, "inline.nolink", &i.nolink);
        one(&mut out, "inline.punctuation", &i.punctuation);
        one(&mut out, "inline.reflink", &i.reflink);
        one(&mut out, "inline.reflinkSearch", &i.reflink_search);
        one(&mut out, "inline.tag", &i.tag);
        one(&mut out, "inline.text", &i.text);
        opt(&mut out, "inline.url", &i.url);
        out
    }
}
