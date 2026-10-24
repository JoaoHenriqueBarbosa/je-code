//! Referências de caractere nomeadas do HTML, para o [`crate::browser::html_parser`].
//!
//! A tabela completa do HTML5 tem 2231 nomes. Aqui entram os do HTML 4 (os 252 nomes que páginas e
//! editores de verdade emitem: Latin-1, símbolos, letras gregas, pontuação tipográfica e setas) mais
//! `apos` e as variantes em maiúsculas legadas (`AMP`, `LT`, `GT`, `QUOT`, `COPY`, `REG`). Um nome que
//! não está aqui fica como texto literal (`&nome;`), o que é o tratamento do HTML para um nome
//! desconhecido; a diferença para um navegador só aparece nos nomes do HTML5 que o HTML 4 não tinha.
//! As referências numéricas (`&#60;`, `&#x3C;`) não passam por aqui e são completas.
//!
//! Os nomes do HTML 4 mapeiam para o mesmo ponto de código no HTML5, exceto `lang` e `rang`, que o HTML5
//! moveu para U+27E8 e U+27E9.

/// Os nomes de U+00A0 a U+00FF, na ordem do ponto de código.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2",
    "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc",
    "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig", "agrave", "aacute", "acirc",
    "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve",
    "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Os nomes legados de ASCII, que o HTML aceita também sem o ponto e vírgula.
const ASCII_LEGACY: &[(&str, u32)] =
    &[("amp", 38), ("AMP", 38), ("lt", 60), ("LT", 60), ("gt", 62), ("GT", 62), ("quot", 34), ("QUOT", 34), ("COPY", 169), ("REG", 174)];

/// Os demais nomes do HTML 4 (e `apos`), que exigem o ponto e vírgula.
const SYMBOLS: &[(&str, u32)] = &[
    ("apos", 39),
    ("OElig", 338),
    ("oelig", 339),
    ("Scaron", 352),
    ("scaron", 353),
    ("Yuml", 376),
    ("fnof", 402),
    ("circ", 710),
    ("tilde", 732),
    ("Alpha", 913),
    ("Beta", 914),
    ("Gamma", 915),
    ("Delta", 916),
    ("Epsilon", 917),
    ("Zeta", 918),
    ("Eta", 919),
    ("Theta", 920),
    ("Iota", 921),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Nu", 925),
    ("Xi", 926),
    ("Omicron", 927),
    ("Pi", 928),
    ("Rho", 929),
    ("Sigma", 931),
    ("Tau", 932),
    ("Upsilon", 933),
    ("Phi", 934),
    ("Chi", 935),
    ("Psi", 936),
    ("Omega", 937),
    ("alpha", 945),
    ("beta", 946),
    ("gamma", 947),
    ("delta", 948),
    ("epsilon", 949),
    ("zeta", 950),
    ("eta", 951),
    ("theta", 952),
    ("iota", 953),
    ("kappa", 954),
    ("lambda", 955),
    ("mu", 956),
    ("nu", 957),
    ("xi", 958),
    ("omicron", 959),
    ("pi", 960),
    ("rho", 961),
    ("sigmaf", 962),
    ("sigma", 963),
    ("tau", 964),
    ("upsilon", 965),
    ("phi", 966),
    ("chi", 967),
    ("psi", 968),
    ("omega", 969),
    ("thetasym", 977),
    ("upsih", 978),
    ("piv", 982),
    ("ensp", 8194),
    ("emsp", 8195),
    ("thinsp", 8201),
    ("zwnj", 8204),
    ("zwj", 8205),
    ("lrm", 8206),
    ("rlm", 8207),
    ("ndash", 8211),
    ("mdash", 8212),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("ldquo", 8220),
    ("rdquo", 8221),
    ("bdquo", 8222),
    ("dagger", 8224),
    ("Dagger", 8225),
    ("bull", 8226),
    ("hellip", 8230),
    ("permil", 8240),
    ("prime", 8242),
    ("Prime", 8243),
    ("lsaquo", 8249),
    ("rsaquo", 8250),
    ("oline", 8254),
    ("frasl", 8260),
    ("euro", 8364),
    ("image", 8465),
    ("weierp", 8472),
    ("real", 8476),
    ("trade", 8482),
    ("alefsym", 8501),
    ("larr", 8592),
    ("uarr", 8593),
    ("rarr", 8594),
    ("darr", 8595),
    ("harr", 8596),
    ("crarr", 8629),
    ("lArr", 8656),
    ("uArr", 8657),
    ("rArr", 8658),
    ("dArr", 8659),
    ("hArr", 8660),
    ("forall", 8704),
    ("part", 8706),
    ("exist", 8707),
    ("empty", 8709),
    ("nabla", 8711),
    ("isin", 8712),
    ("notin", 8713),
    ("ni", 8715),
    ("prod", 8719),
    ("sum", 8721),
    ("minus", 8722),
    ("lowast", 8727),
    ("radic", 8730),
    ("prop", 8733),
    ("infin", 8734),
    ("ang", 8736),
    ("and", 8743),
    ("or", 8744),
    ("cap", 8745),
    ("cup", 8746),
    ("int", 8747),
    ("there4", 8756),
    ("sim", 8764),
    ("cong", 8773),
    ("asymp", 8776),
    ("ne", 8800),
    ("equiv", 8801),
    ("le", 8804),
    ("ge", 8805),
    ("sub", 8834),
    ("sup", 8835),
    ("nsub", 8836),
    ("sube", 8838),
    ("supe", 8839),
    ("oplus", 8853),
    ("otimes", 8855),
    ("perp", 8869),
    ("sdot", 8901),
    ("lceil", 8968),
    ("rceil", 8969),
    ("lfloor", 8970),
    ("rfloor", 8971),
    ("lang", 10216),
    ("rang", 10217),
    ("loz", 9674),
    ("spades", 9824),
    ("clubs", 9827),
    ("hearts", 9829),
    ("diams", 9830),
];

fn latin1(name: &str) -> Option<u32> {
    let index = LATIN1.iter().position(|&candidate| candidate == name)?;
    u32::try_from(index).ok().map(|index| 160 + index)
}

fn ascii_legacy(name: &str) -> Option<u32> {
    ASCII_LEGACY.iter().find(|(candidate, _)| *candidate == name).map(|&(_, code_point)| code_point)
}

/// O ponto de código de `&name;` (com o ponto e vírgula), ou `None` se o nome não está na tabela.
pub fn named_entity_with_semicolon(name: &str) -> Option<u32> {
    ascii_legacy(name).or_else(|| latin1(name)).or_else(|| SYMBOLS.iter().find(|(candidate, _)| *candidate == name).map(|&(_, code_point)| code_point))
}

/// O ponto de código de `&name` sem o ponto e vírgula: só os nomes legados (Latin-1 e `amp`, `lt`, `gt`,
/// `quot` e as variantes em maiúsculas) valem assim.
pub fn named_entity_without_semicolon(name: &str) -> Option<u32> {
    ascii_legacy(name).or_else(|| latin1(name))
}
