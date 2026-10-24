//! Gerado por `scripts/gen-url-character-class-table.py` a partir de
//! `upstream/WTF/wtf/URLParser.cpp` (`URLCharacterClass`, linha 53, e `characterClassTable`,
//! linha 65). Não editar à mão.

/// `enum URLCharacterClass` (URLParser.cpp 53).
pub const USER_INFO_ENCODE: u8 = 0x1;
pub const PATH_ENCODE: u8 = 0x2;
pub const FORBIDDEN_HOST: u8 = 0x4;
pub const FORBIDDEN_DOMAIN: u8 = 0x8;
pub const QUERY_ENCODE: u8 = 0x10;
pub const SLASH_QUESTION_OR_HASH: u8 = 0x20;
pub const VALID_SCHEME: u8 = 0x40;

/// `characterClassTable` (URLParser.cpp 65), indexada pelo ponto de código (0 a 255).
pub static CHARACTER_CLASS_TABLE: [u8; 256] = [
    0x1F, // 0x0: UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x1B, // 0x1: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x2: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x3: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x4: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x5: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x6: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x7: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x8: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1F, // 0x9: UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x1F, // 0xA: UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x1B, // 0xB: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0xC: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1F, // 0xD: UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x1B, // 0xE: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0xF: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x10: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x11: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x12: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x13: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x14: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x15: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x16: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x17: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x18: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x19: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1A: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1B: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1C: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1D: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1E: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1B, // 0x1F: UserInfoEncode | PathEncode | QueryEncode | ForbiddenDomain
    0x1F, // ' ': UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x00, // '!': 0
    0x13, // '"': UserInfoEncode | PathEncode | QueryEncode
    0x3F, // '#': UserInfoEncode | PathEncode | QueryEncode | SlashQuestionOrHash | ForbiddenHost | ForbiddenDomain
    0x00, // '$': 0
    0x08, // '%': ForbiddenDomain
    0x00, // '&': 0
    0x00, // '\'': 0
    0x00, // '(': 0
    0x00, // ')': 0
    0x00, // '*': 0
    0x40, // '+': ValidScheme
    0x00, // ',': 0
    0x40, // '-': ValidScheme
    0x40, // '.': ValidScheme
    0x2D, // '/': UserInfoEncode | SlashQuestionOrHash | ForbiddenHost | ForbiddenDomain
    0x40, // '0': ValidScheme
    0x40, // '1': ValidScheme
    0x40, // '2': ValidScheme
    0x40, // '3': ValidScheme
    0x40, // '4': ValidScheme
    0x40, // '5': ValidScheme
    0x40, // '6': ValidScheme
    0x40, // '7': ValidScheme
    0x40, // '8': ValidScheme
    0x40, // '9': ValidScheme
    0x0D, // ':': UserInfoEncode | ForbiddenHost | ForbiddenDomain
    0x01, // ';': UserInfoEncode
    0x1F, // '<': UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x01, // '=': UserInfoEncode
    0x1F, // '>': UserInfoEncode | PathEncode | QueryEncode | ForbiddenHost | ForbiddenDomain
    0x2F, // '?': UserInfoEncode | PathEncode | SlashQuestionOrHash | ForbiddenHost | ForbiddenDomain
    0x0D, // '@': UserInfoEncode | ForbiddenHost | ForbiddenDomain
    0x40, // 'A': ValidScheme
    0x40, // 'B': ValidScheme
    0x40, // 'C': ValidScheme
    0x40, // 'D': ValidScheme
    0x40, // 'E': ValidScheme
    0x40, // 'F': ValidScheme
    0x40, // 'G': ValidScheme
    0x40, // 'H': ValidScheme
    0x40, // 'I': ValidScheme
    0x40, // 'J': ValidScheme
    0x40, // 'K': ValidScheme
    0x40, // 'L': ValidScheme
    0x40, // 'M': ValidScheme
    0x40, // 'N': ValidScheme
    0x40, // 'O': ValidScheme
    0x40, // 'P': ValidScheme
    0x40, // 'Q': ValidScheme
    0x40, // 'R': ValidScheme
    0x40, // 'S': ValidScheme
    0x40, // 'T': ValidScheme
    0x40, // 'U': ValidScheme
    0x40, // 'V': ValidScheme
    0x40, // 'W': ValidScheme
    0x40, // 'X': ValidScheme
    0x40, // 'Y': ValidScheme
    0x40, // 'Z': ValidScheme
    0x0D, // '[': UserInfoEncode | ForbiddenHost | ForbiddenDomain
    0x2D, // '\\': UserInfoEncode | SlashQuestionOrHash | ForbiddenHost | ForbiddenDomain
    0x0D, // ']': UserInfoEncode | ForbiddenHost | ForbiddenDomain
    0x0F, // '^': UserInfoEncode | PathEncode | ForbiddenHost | ForbiddenDomain
    0x00, // '_': 0
    0x03, // '`': UserInfoEncode | PathEncode
    0x40, // 'a': ValidScheme
    0x40, // 'b': ValidScheme
    0x40, // 'c': ValidScheme
    0x40, // 'd': ValidScheme
    0x40, // 'e': ValidScheme
    0x40, // 'f': ValidScheme
    0x40, // 'g': ValidScheme
    0x40, // 'h': ValidScheme
    0x40, // 'i': ValidScheme
    0x40, // 'j': ValidScheme
    0x40, // 'k': ValidScheme
    0x40, // 'l': ValidScheme
    0x40, // 'm': ValidScheme
    0x40, // 'n': ValidScheme
    0x40, // 'o': ValidScheme
    0x40, // 'p': ValidScheme
    0x40, // 'q': ValidScheme
    0x40, // 'r': ValidScheme
    0x40, // 's': ValidScheme
    0x40, // 't': ValidScheme
    0x40, // 'u': ValidScheme
    0x40, // 'v': ValidScheme
    0x40, // 'w': ValidScheme
    0x40, // 'x': ValidScheme
    0x40, // 'y': ValidScheme
    0x40, // 'z': ValidScheme
    0x03, // '{': UserInfoEncode | PathEncode
    0x0D, // '|': UserInfoEncode | ForbiddenHost | ForbiddenDomain
    0x03, // '}': UserInfoEncode | PathEncode
    0x00, // '~': 0
    0x18, // 0x7F: QueryEncode | ForbiddenDomain
    0x10, // 0x80: QueryEncode
    0x10, // 0x81: QueryEncode
    0x10, // 0x82: QueryEncode
    0x10, // 0x83: QueryEncode
    0x10, // 0x84: QueryEncode
    0x10, // 0x85: QueryEncode
    0x10, // 0x86: QueryEncode
    0x10, // 0x87: QueryEncode
    0x10, // 0x88: QueryEncode
    0x10, // 0x89: QueryEncode
    0x10, // 0x8A: QueryEncode
    0x10, // 0x8B: QueryEncode
    0x10, // 0x8C: QueryEncode
    0x10, // 0x8D: QueryEncode
    0x10, // 0x8E: QueryEncode
    0x10, // 0x8F: QueryEncode
    0x10, // 0x90: QueryEncode
    0x10, // 0x91: QueryEncode
    0x10, // 0x92: QueryEncode
    0x10, // 0x93: QueryEncode
    0x10, // 0x94: QueryEncode
    0x10, // 0x95: QueryEncode
    0x10, // 0x96: QueryEncode
    0x10, // 0x97: QueryEncode
    0x10, // 0x98: QueryEncode
    0x10, // 0x99: QueryEncode
    0x10, // 0x9A: QueryEncode
    0x10, // 0x9B: QueryEncode
    0x10, // 0x9C: QueryEncode
    0x10, // 0x9D: QueryEncode
    0x10, // 0x9E: QueryEncode
    0x10, // 0x9F: QueryEncode
    0x10, // 0xA0: QueryEncode
    0x10, // 0xA1: QueryEncode
    0x10, // 0xA2: QueryEncode
    0x10, // 0xA3: QueryEncode
    0x10, // 0xA4: QueryEncode
    0x10, // 0xA5: QueryEncode
    0x10, // 0xA6: QueryEncode
    0x10, // 0xA7: QueryEncode
    0x10, // 0xA8: QueryEncode
    0x10, // 0xA9: QueryEncode
    0x10, // 0xAA: QueryEncode
    0x10, // 0xAB: QueryEncode
    0x10, // 0xAC: QueryEncode
    0x10, // 0xAD: QueryEncode
    0x10, // 0xAE: QueryEncode
    0x10, // 0xAF: QueryEncode
    0x10, // 0xB0: QueryEncode
    0x10, // 0xB1: QueryEncode
    0x10, // 0xB2: QueryEncode
    0x10, // 0xB3: QueryEncode
    0x10, // 0xB4: QueryEncode
    0x10, // 0xB5: QueryEncode
    0x10, // 0xB6: QueryEncode
    0x10, // 0xB7: QueryEncode
    0x10, // 0xB8: QueryEncode
    0x10, // 0xB9: QueryEncode
    0x10, // 0xBA: QueryEncode
    0x10, // 0xBB: QueryEncode
    0x10, // 0xBC: QueryEncode
    0x10, // 0xBD: QueryEncode
    0x10, // 0xBE: QueryEncode
    0x10, // 0xBF: QueryEncode
    0x10, // 0xC0: QueryEncode
    0x10, // 0xC1: QueryEncode
    0x10, // 0xC2: QueryEncode
    0x10, // 0xC3: QueryEncode
    0x10, // 0xC4: QueryEncode
    0x10, // 0xC5: QueryEncode
    0x10, // 0xC6: QueryEncode
    0x10, // 0xC7: QueryEncode
    0x10, // 0xC8: QueryEncode
    0x10, // 0xC9: QueryEncode
    0x10, // 0xCA: QueryEncode
    0x10, // 0xCB: QueryEncode
    0x10, // 0xCC: QueryEncode
    0x10, // 0xCD: QueryEncode
    0x10, // 0xCE: QueryEncode
    0x10, // 0xCF: QueryEncode
    0x10, // 0xD0: QueryEncode
    0x10, // 0xD1: QueryEncode
    0x10, // 0xD2: QueryEncode
    0x10, // 0xD3: QueryEncode
    0x10, // 0xD4: QueryEncode
    0x10, // 0xD5: QueryEncode
    0x10, // 0xD6: QueryEncode
    0x10, // 0xD7: QueryEncode
    0x10, // 0xD8: QueryEncode
    0x10, // 0xD9: QueryEncode
    0x10, // 0xDA: QueryEncode
    0x10, // 0xDB: QueryEncode
    0x10, // 0xDC: QueryEncode
    0x10, // 0xDD: QueryEncode
    0x10, // 0xDE: QueryEncode
    0x10, // 0xDF: QueryEncode
    0x10, // 0xE0: QueryEncode
    0x10, // 0xE1: QueryEncode
    0x10, // 0xE2: QueryEncode
    0x10, // 0xE3: QueryEncode
    0x10, // 0xE4: QueryEncode
    0x10, // 0xE5: QueryEncode
    0x10, // 0xE6: QueryEncode
    0x10, // 0xE7: QueryEncode
    0x10, // 0xE8: QueryEncode
    0x10, // 0xE9: QueryEncode
    0x10, // 0xEA: QueryEncode
    0x10, // 0xEB: QueryEncode
    0x10, // 0xEC: QueryEncode
    0x10, // 0xED: QueryEncode
    0x10, // 0xEE: QueryEncode
    0x10, // 0xEF: QueryEncode
    0x10, // 0xF0: QueryEncode
    0x10, // 0xF1: QueryEncode
    0x10, // 0xF2: QueryEncode
    0x10, // 0xF3: QueryEncode
    0x10, // 0xF4: QueryEncode
    0x10, // 0xF5: QueryEncode
    0x10, // 0xF6: QueryEncode
    0x10, // 0xF7: QueryEncode
    0x10, // 0xF8: QueryEncode
    0x10, // 0xF9: QueryEncode
    0x10, // 0xFA: QueryEncode
    0x10, // 0xFB: QueryEncode
    0x10, // 0xFC: QueryEncode
    0x10, // 0xFD: QueryEncode
    0x10, // 0xFE: QueryEncode
    0x10, // 0xFF: QueryEncode
];
