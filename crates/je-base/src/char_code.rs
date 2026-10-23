//! Porte de `src/vs/base/common/charCode.ts`: os códigos de caractere usados pelo VS Code.
//!
//! Gerado de `charCode.ts` (tag 1.143.1); os comentários de documentação vêm de lá.

#![allow(non_upper_case_globals)]

pub const Null: u16 = 0;
/// The `\b` character.
pub const Backspace: u16 = 8;
/// The `\t` character.
pub const Tab: u16 = 9;
/// The `\n` character.
pub const LineFeed: u16 = 10;
/// The `\r` character.
pub const CarriageReturn: u16 = 13;
pub const Space: u16 = 32;
/// The `!` character.
pub const ExclamationMark: u16 = 33;
/// The `"` character.
pub const DoubleQuote: u16 = 34;
/// The `#` character.
pub const Hash: u16 = 35;
/// The `$` character.
pub const DollarSign: u16 = 36;
/// The `%` character.
pub const PercentSign: u16 = 37;
/// The `&` character.
pub const Ampersand: u16 = 38;
/// The `'` character.
pub const SingleQuote: u16 = 39;
/// The `(` character.
pub const OpenParen: u16 = 40;
/// The `)` character.
pub const CloseParen: u16 = 41;
/// The `*` character.
pub const Asterisk: u16 = 42;
/// The `+` character.
pub const Plus: u16 = 43;
/// The `,` character.
pub const Comma: u16 = 44;
/// The `-` character.
pub const Dash: u16 = 45;
/// The `.` character.
pub const Period: u16 = 46;
/// The `/` character.
pub const Slash: u16 = 47;
pub const Digit0: u16 = 48;
pub const Digit1: u16 = 49;
pub const Digit2: u16 = 50;
pub const Digit3: u16 = 51;
pub const Digit4: u16 = 52;
pub const Digit5: u16 = 53;
pub const Digit6: u16 = 54;
pub const Digit7: u16 = 55;
pub const Digit8: u16 = 56;
pub const Digit9: u16 = 57;
/// The `:` character.
pub const Colon: u16 = 58;
/// The `;` character.
pub const Semicolon: u16 = 59;
/// The `<` character.
pub const LessThan: u16 = 60;
/// The `=` character.
pub const Equals: u16 = 61;
/// The `>` character.
pub const GreaterThan: u16 = 62;
/// The `?` character.
pub const QuestionMark: u16 = 63;
/// The `@` character.
pub const AtSign: u16 = 64;
pub const A: u16 = 65;
pub const B: u16 = 66;
pub const C: u16 = 67;
pub const D: u16 = 68;
pub const E: u16 = 69;
pub const F: u16 = 70;
pub const G: u16 = 71;
pub const H: u16 = 72;
pub const I: u16 = 73;
pub const J: u16 = 74;
pub const K: u16 = 75;
pub const L: u16 = 76;
pub const M: u16 = 77;
pub const N: u16 = 78;
pub const O: u16 = 79;
pub const P: u16 = 80;
pub const Q: u16 = 81;
pub const R: u16 = 82;
pub const S: u16 = 83;
pub const T: u16 = 84;
pub const U: u16 = 85;
pub const V: u16 = 86;
pub const W: u16 = 87;
pub const X: u16 = 88;
pub const Y: u16 = 89;
pub const Z: u16 = 90;
/// The `[` character.
pub const OpenSquareBracket: u16 = 91;
/// The `\` character.
pub const Backslash: u16 = 92;
/// The `]` character.
pub const CloseSquareBracket: u16 = 93;
/// The `^` character.
pub const Caret: u16 = 94;
/// The `_` character.
pub const Underline: u16 = 95;
/// The ``(`)`` character.
pub const BackTick: u16 = 96;
pub const a: u16 = 97;
pub const b: u16 = 98;
pub const c: u16 = 99;
pub const d: u16 = 100;
pub const e: u16 = 101;
pub const f: u16 = 102;
pub const g: u16 = 103;
pub const h: u16 = 104;
pub const i: u16 = 105;
pub const j: u16 = 106;
pub const k: u16 = 107;
pub const l: u16 = 108;
pub const m: u16 = 109;
pub const n: u16 = 110;
pub const o: u16 = 111;
pub const p: u16 = 112;
pub const q: u16 = 113;
pub const r: u16 = 114;
pub const s: u16 = 115;
pub const t: u16 = 116;
pub const u: u16 = 117;
pub const v: u16 = 118;
pub const w: u16 = 119;
pub const x: u16 = 120;
pub const y: u16 = 121;
pub const z: u16 = 122;
/// The `{` character.
pub const OpenCurlyBrace: u16 = 123;
/// The `|` character.
pub const Pipe: u16 = 124;
/// The `}` character.
pub const CloseCurlyBrace: u16 = 125;
/// The `~` character.
pub const Tilde: u16 = 126;
/// The &nbsp; (no-break space) character.
/// Unicode Character 'NO-BREAK SPACE' (U+00A0)
pub const NoBreakSpace: u16 = 160;
/// U+0300 Combining Grave Accent
pub const U_Combining_Grave_Accent: u16 = 0x0300;
/// U+0301 Combining Acute Accent
pub const U_Combining_Acute_Accent: u16 = 0x0301;
/// U+0302 Combining Circumflex Accent
pub const U_Combining_Circumflex_Accent: u16 = 0x0302;
/// U+0303 Combining Tilde
pub const U_Combining_Tilde: u16 = 0x0303;
/// U+0304 Combining Macron
pub const U_Combining_Macron: u16 = 0x0304;
/// U+0305 Combining Overline
pub const U_Combining_Overline: u16 = 0x0305;
/// U+0306 Combining Breve
pub const U_Combining_Breve: u16 = 0x0306;
/// U+0307 Combining Dot Above
pub const U_Combining_Dot_Above: u16 = 0x0307;
/// U+0308 Combining Diaeresis
pub const U_Combining_Diaeresis: u16 = 0x0308;
/// U+0309 Combining Hook Above
pub const U_Combining_Hook_Above: u16 = 0x0309;
/// U+030A Combining Ring Above
pub const U_Combining_Ring_Above: u16 = 0x030A;
/// U+030B Combining Double Acute Accent
pub const U_Combining_Double_Acute_Accent: u16 = 0x030B;
/// U+030C Combining Caron
pub const U_Combining_Caron: u16 = 0x030C;
/// U+030D Combining Vertical Line Above
pub const U_Combining_Vertical_Line_Above: u16 = 0x030D;
/// U+030E Combining Double Vertical Line Above
pub const U_Combining_Double_Vertical_Line_Above: u16 = 0x030E;
/// U+030F Combining Double Grave Accent
pub const U_Combining_Double_Grave_Accent: u16 = 0x030F;
/// U+0310 Combining Candrabindu
pub const U_Combining_Candrabindu: u16 = 0x0310;
/// U+0311 Combining Inverted Breve
pub const U_Combining_Inverted_Breve: u16 = 0x0311;
/// U+0312 Combining Turned Comma Above
pub const U_Combining_Turned_Comma_Above: u16 = 0x0312;
/// U+0313 Combining Comma Above
pub const U_Combining_Comma_Above: u16 = 0x0313;
/// U+0314 Combining Reversed Comma Above
pub const U_Combining_Reversed_Comma_Above: u16 = 0x0314;
/// U+0315 Combining Comma Above Right
pub const U_Combining_Comma_Above_Right: u16 = 0x0315;
/// U+0316 Combining Grave Accent Below
pub const U_Combining_Grave_Accent_Below: u16 = 0x0316;
/// U+0317 Combining Acute Accent Below
pub const U_Combining_Acute_Accent_Below: u16 = 0x0317;
/// U+0318 Combining Left Tack Below
pub const U_Combining_Left_Tack_Below: u16 = 0x0318;
/// U+0319 Combining Right Tack Below
pub const U_Combining_Right_Tack_Below: u16 = 0x0319;
/// U+031A Combining Left Angle Above
pub const U_Combining_Left_Angle_Above: u16 = 0x031A;
/// U+031B Combining Horn
pub const U_Combining_Horn: u16 = 0x031B;
/// U+031C Combining Left Half Ring Below
pub const U_Combining_Left_Half_Ring_Below: u16 = 0x031C;
/// U+031D Combining Up Tack Below
pub const U_Combining_Up_Tack_Below: u16 = 0x031D;
/// U+031E Combining Down Tack Below
pub const U_Combining_Down_Tack_Below: u16 = 0x031E;
/// U+031F Combining Plus Sign Below
pub const U_Combining_Plus_Sign_Below: u16 = 0x031F;
/// U+0320 Combining Minus Sign Below
pub const U_Combining_Minus_Sign_Below: u16 = 0x0320;
/// U+0321 Combining Palatalized Hook Below
pub const U_Combining_Palatalized_Hook_Below: u16 = 0x0321;
/// U+0322 Combining Retroflex Hook Below
pub const U_Combining_Retroflex_Hook_Below: u16 = 0x0322;
/// U+0323 Combining Dot Below
pub const U_Combining_Dot_Below: u16 = 0x0323;
/// U+0324 Combining Diaeresis Below
pub const U_Combining_Diaeresis_Below: u16 = 0x0324;
/// U+0325 Combining Ring Below
pub const U_Combining_Ring_Below: u16 = 0x0325;
/// U+0326 Combining Comma Below
pub const U_Combining_Comma_Below: u16 = 0x0326;
/// U+0327 Combining Cedilla
pub const U_Combining_Cedilla: u16 = 0x0327;
/// U+0328 Combining Ogonek
pub const U_Combining_Ogonek: u16 = 0x0328;
/// U+0329 Combining Vertical Line Below
pub const U_Combining_Vertical_Line_Below: u16 = 0x0329;
/// U+032A Combining Bridge Below
pub const U_Combining_Bridge_Below: u16 = 0x032A;
/// U+032B Combining Inverted Double Arch Below
pub const U_Combining_Inverted_Double_Arch_Below: u16 = 0x032B;
/// U+032C Combining Caron Below
pub const U_Combining_Caron_Below: u16 = 0x032C;
/// U+032D Combining Circumflex Accent Below
pub const U_Combining_Circumflex_Accent_Below: u16 = 0x032D;
/// U+032E Combining Breve Below
pub const U_Combining_Breve_Below: u16 = 0x032E;
/// U+032F Combining Inverted Breve Below
pub const U_Combining_Inverted_Breve_Below: u16 = 0x032F;
/// U+0330 Combining Tilde Below
pub const U_Combining_Tilde_Below: u16 = 0x0330;
/// U+0331 Combining Macron Below
pub const U_Combining_Macron_Below: u16 = 0x0331;
/// U+0332 Combining Low Line
pub const U_Combining_Low_Line: u16 = 0x0332;
/// U+0333 Combining Double Low Line
pub const U_Combining_Double_Low_Line: u16 = 0x0333;
/// U+0334 Combining Tilde Overlay
pub const U_Combining_Tilde_Overlay: u16 = 0x0334;
/// U+0335 Combining Short Stroke Overlay
pub const U_Combining_Short_Stroke_Overlay: u16 = 0x0335;
/// U+0336 Combining Long Stroke Overlay
pub const U_Combining_Long_Stroke_Overlay: u16 = 0x0336;
/// U+0337 Combining Short Solidus Overlay
pub const U_Combining_Short_Solidus_Overlay: u16 = 0x0337;
/// U+0338 Combining Long Solidus Overlay
pub const U_Combining_Long_Solidus_Overlay: u16 = 0x0338;
/// U+0339 Combining Right Half Ring Below
pub const U_Combining_Right_Half_Ring_Below: u16 = 0x0339;
/// U+033A Combining Inverted Bridge Below
pub const U_Combining_Inverted_Bridge_Below: u16 = 0x033A;
/// U+033B Combining Square Below
pub const U_Combining_Square_Below: u16 = 0x033B;
/// U+033C Combining Seagull Below
pub const U_Combining_Seagull_Below: u16 = 0x033C;
/// U+033D Combining X Above
pub const U_Combining_X_Above: u16 = 0x033D;
/// U+033E Combining Vertical Tilde
pub const U_Combining_Vertical_Tilde: u16 = 0x033E;
/// U+033F Combining Double Overline
pub const U_Combining_Double_Overline: u16 = 0x033F;
/// U+0340 Combining Grave Tone Mark
pub const U_Combining_Grave_Tone_Mark: u16 = 0x0340;
/// U+0341 Combining Acute Tone Mark
pub const U_Combining_Acute_Tone_Mark: u16 = 0x0341;
/// U+0342 Combining Greek Perispomeni
pub const U_Combining_Greek_Perispomeni: u16 = 0x0342;
/// U+0343 Combining Greek Koronis
pub const U_Combining_Greek_Koronis: u16 = 0x0343;
/// U+0344 Combining Greek Dialytika Tonos
pub const U_Combining_Greek_Dialytika_Tonos: u16 = 0x0344;
/// U+0345 Combining Greek Ypogegrammeni
pub const U_Combining_Greek_Ypogegrammeni: u16 = 0x0345;
/// U+0346 Combining Bridge Above
pub const U_Combining_Bridge_Above: u16 = 0x0346;
/// U+0347 Combining Equals Sign Below
pub const U_Combining_Equals_Sign_Below: u16 = 0x0347;
/// U+0348 Combining Double Vertical Line Below
pub const U_Combining_Double_Vertical_Line_Below: u16 = 0x0348;
/// U+0349 Combining Left Angle Below
pub const U_Combining_Left_Angle_Below: u16 = 0x0349;
/// U+034A Combining Not Tilde Above
pub const U_Combining_Not_Tilde_Above: u16 = 0x034A;
/// U+034B Combining Homothetic Above
pub const U_Combining_Homothetic_Above: u16 = 0x034B;
/// U+034C Combining Almost Equal To Above
pub const U_Combining_Almost_Equal_To_Above: u16 = 0x034C;
/// U+034D Combining Left Right Arrow Below
pub const U_Combining_Left_Right_Arrow_Below: u16 = 0x034D;
/// U+034E Combining Upwards Arrow Below
pub const U_Combining_Upwards_Arrow_Below: u16 = 0x034E;
/// U+034F Combining Grapheme Joiner
pub const U_Combining_Grapheme_Joiner: u16 = 0x034F;
/// U+0350 Combining Right Arrowhead Above
pub const U_Combining_Right_Arrowhead_Above: u16 = 0x0350;
/// U+0351 Combining Left Half Ring Above
pub const U_Combining_Left_Half_Ring_Above: u16 = 0x0351;
/// U+0352 Combining Fermata
pub const U_Combining_Fermata: u16 = 0x0352;
/// U+0353 Combining X Below
pub const U_Combining_X_Below: u16 = 0x0353;
/// U+0354 Combining Left Arrowhead Below
pub const U_Combining_Left_Arrowhead_Below: u16 = 0x0354;
/// U+0355 Combining Right Arrowhead Below
pub const U_Combining_Right_Arrowhead_Below: u16 = 0x0355;
/// U+0356 Combining Right Arrowhead And Up Arrowhead Below
pub const U_Combining_Right_Arrowhead_And_Up_Arrowhead_Below: u16 = 0x0356;
/// U+0357 Combining Right Half Ring Above
pub const U_Combining_Right_Half_Ring_Above: u16 = 0x0357;
/// U+0358 Combining Dot Above Right
pub const U_Combining_Dot_Above_Right: u16 = 0x0358;
/// U+0359 Combining Asterisk Below
pub const U_Combining_Asterisk_Below: u16 = 0x0359;
/// U+035A Combining Double Ring Below
pub const U_Combining_Double_Ring_Below: u16 = 0x035A;
/// U+035B Combining Zigzag Above
pub const U_Combining_Zigzag_Above: u16 = 0x035B;
/// U+035C Combining Double Breve Below
pub const U_Combining_Double_Breve_Below: u16 = 0x035C;
/// U+035D Combining Double Breve
pub const U_Combining_Double_Breve: u16 = 0x035D;
/// U+035E Combining Double Macron
pub const U_Combining_Double_Macron: u16 = 0x035E;
/// U+035F Combining Double Macron Below
pub const U_Combining_Double_Macron_Below: u16 = 0x035F;
/// U+0360 Combining Double Tilde
pub const U_Combining_Double_Tilde: u16 = 0x0360;
/// U+0361 Combining Double Inverted Breve
pub const U_Combining_Double_Inverted_Breve: u16 = 0x0361;
/// U+0362 Combining Double Rightwards Arrow Below
pub const U_Combining_Double_Rightwards_Arrow_Below: u16 = 0x0362;
/// U+0363 Combining Latin Small Letter A
pub const U_Combining_Latin_Small_Letter_A: u16 = 0x0363;
/// U+0364 Combining Latin Small Letter E
pub const U_Combining_Latin_Small_Letter_E: u16 = 0x0364;
/// U+0365 Combining Latin Small Letter I
pub const U_Combining_Latin_Small_Letter_I: u16 = 0x0365;
/// U+0366 Combining Latin Small Letter O
pub const U_Combining_Latin_Small_Letter_O: u16 = 0x0366;
/// U+0367 Combining Latin Small Letter U
pub const U_Combining_Latin_Small_Letter_U: u16 = 0x0367;
/// U+0368 Combining Latin Small Letter C
pub const U_Combining_Latin_Small_Letter_C: u16 = 0x0368;
/// U+0369 Combining Latin Small Letter D
pub const U_Combining_Latin_Small_Letter_D: u16 = 0x0369;
/// U+036A Combining Latin Small Letter H
pub const U_Combining_Latin_Small_Letter_H: u16 = 0x036A;
/// U+036B Combining Latin Small Letter M
pub const U_Combining_Latin_Small_Letter_M: u16 = 0x036B;
/// U+036C Combining Latin Small Letter R
pub const U_Combining_Latin_Small_Letter_R: u16 = 0x036C;
/// U+036D Combining Latin Small Letter T
pub const U_Combining_Latin_Small_Letter_T: u16 = 0x036D;
/// U+036E Combining Latin Small Letter V
pub const U_Combining_Latin_Small_Letter_V: u16 = 0x036E;
/// U+036F Combining Latin Small Letter X
pub const U_Combining_Latin_Small_Letter_X: u16 = 0x036F;
/// Unicode Character 'LINE SEPARATOR' (U+2028)
/// http://www.fileformat.info/info/unicode/char/2028/index.htm
pub const LINE_SEPARATOR: u16 = 0x2028;
/// Unicode Character 'PARAGRAPH SEPARATOR' (U+2029)
/// http://www.fileformat.info/info/unicode/char/2029/index.htm
pub const PARAGRAPH_SEPARATOR: u16 = 0x2029;
/// Unicode Character 'NEXT LINE' (U+0085)
/// http://www.fileformat.info/info/unicode/char/0085/index.htm
pub const NEXT_LINE: u16 = 0x0085;
/// http://www.fileformat.info/info/unicode/category/Sk/list.htm
/// U+005E CIRCUMFLEX
pub const U_CIRCUMFLEX: u16 = 0x005E;
/// U+0060 GRAVE ACCENT
pub const U_GRAVE_ACCENT: u16 = 0x0060;
/// U+00A8 DIAERESIS
pub const U_DIAERESIS: u16 = 0x00A8;
/// U+00AF MACRON
pub const U_MACRON: u16 = 0x00AF;
/// U+00B4 ACUTE ACCENT
pub const U_ACUTE_ACCENT: u16 = 0x00B4;
/// U+00B8 CEDILLA
pub const U_CEDILLA: u16 = 0x00B8;
/// U+02C2 MODIFIER LETTER LEFT ARROWHEAD
pub const U_MODIFIER_LETTER_LEFT_ARROWHEAD: u16 = 0x02C2;
/// U+02C3 MODIFIER LETTER RIGHT ARROWHEAD
pub const U_MODIFIER_LETTER_RIGHT_ARROWHEAD: u16 = 0x02C3;
/// U+02C4 MODIFIER LETTER UP ARROWHEAD
pub const U_MODIFIER_LETTER_UP_ARROWHEAD: u16 = 0x02C4;
/// U+02C5 MODIFIER LETTER DOWN ARROWHEAD
pub const U_MODIFIER_LETTER_DOWN_ARROWHEAD: u16 = 0x02C5;
/// U+02D2 MODIFIER LETTER CENTRED RIGHT HALF RING
pub const U_MODIFIER_LETTER_CENTRED_RIGHT_HALF_RING: u16 = 0x02D2;
/// U+02D3 MODIFIER LETTER CENTRED LEFT HALF RING
pub const U_MODIFIER_LETTER_CENTRED_LEFT_HALF_RING: u16 = 0x02D3;
/// U+02D4 MODIFIER LETTER UP TACK
pub const U_MODIFIER_LETTER_UP_TACK: u16 = 0x02D4;
/// U+02D5 MODIFIER LETTER DOWN TACK
pub const U_MODIFIER_LETTER_DOWN_TACK: u16 = 0x02D5;
/// U+02D6 MODIFIER LETTER PLUS SIGN
pub const U_MODIFIER_LETTER_PLUS_SIGN: u16 = 0x02D6;
/// U+02D7 MODIFIER LETTER MINUS SIGN
pub const U_MODIFIER_LETTER_MINUS_SIGN: u16 = 0x02D7;
/// U+02D8 BREVE
pub const U_BREVE: u16 = 0x02D8;
/// U+02D9 DOT ABOVE
pub const U_DOT_ABOVE: u16 = 0x02D9;
/// U+02DA RING ABOVE
pub const U_RING_ABOVE: u16 = 0x02DA;
/// U+02DB OGONEK
pub const U_OGONEK: u16 = 0x02DB;
/// U+02DC SMALL TILDE
pub const U_SMALL_TILDE: u16 = 0x02DC;
/// U+02DD DOUBLE ACUTE ACCENT
pub const U_DOUBLE_ACUTE_ACCENT: u16 = 0x02DD;
/// U+02DE MODIFIER LETTER RHOTIC HOOK
pub const U_MODIFIER_LETTER_RHOTIC_HOOK: u16 = 0x02DE;
/// U+02DF MODIFIER LETTER CROSS ACCENT
pub const U_MODIFIER_LETTER_CROSS_ACCENT: u16 = 0x02DF;
/// U+02E5 MODIFIER LETTER EXTRA-HIGH TONE BAR
pub const U_MODIFIER_LETTER_EXTRA_HIGH_TONE_BAR: u16 = 0x02E5;
/// U+02E6 MODIFIER LETTER HIGH TONE BAR
pub const U_MODIFIER_LETTER_HIGH_TONE_BAR: u16 = 0x02E6;
/// U+02E7 MODIFIER LETTER MID TONE BAR
pub const U_MODIFIER_LETTER_MID_TONE_BAR: u16 = 0x02E7;
/// U+02E8 MODIFIER LETTER LOW TONE BAR
pub const U_MODIFIER_LETTER_LOW_TONE_BAR: u16 = 0x02E8;
/// U+02E9 MODIFIER LETTER EXTRA-LOW TONE BAR
pub const U_MODIFIER_LETTER_EXTRA_LOW_TONE_BAR: u16 = 0x02E9;
/// U+02EA MODIFIER LETTER YIN DEPARTING TONE MARK
pub const U_MODIFIER_LETTER_YIN_DEPARTING_TONE_MARK: u16 = 0x02EA;
/// U+02EB MODIFIER LETTER YANG DEPARTING TONE MARK
pub const U_MODIFIER_LETTER_YANG_DEPARTING_TONE_MARK: u16 = 0x02EB;
/// U+02ED MODIFIER LETTER UNASPIRATED
pub const U_MODIFIER_LETTER_UNASPIRATED: u16 = 0x02ED;
/// U+02EF MODIFIER LETTER LOW DOWN ARROWHEAD
pub const U_MODIFIER_LETTER_LOW_DOWN_ARROWHEAD: u16 = 0x02EF;
/// U+02F0 MODIFIER LETTER LOW UP ARROWHEAD
pub const U_MODIFIER_LETTER_LOW_UP_ARROWHEAD: u16 = 0x02F0;
/// U+02F1 MODIFIER LETTER LOW LEFT ARROWHEAD
pub const U_MODIFIER_LETTER_LOW_LEFT_ARROWHEAD: u16 = 0x02F1;
/// U+02F2 MODIFIER LETTER LOW RIGHT ARROWHEAD
pub const U_MODIFIER_LETTER_LOW_RIGHT_ARROWHEAD: u16 = 0x02F2;
/// U+02F3 MODIFIER LETTER LOW RING
pub const U_MODIFIER_LETTER_LOW_RING: u16 = 0x02F3;
/// U+02F4 MODIFIER LETTER MIDDLE GRAVE ACCENT
pub const U_MODIFIER_LETTER_MIDDLE_GRAVE_ACCENT: u16 = 0x02F4;
/// U+02F5 MODIFIER LETTER MIDDLE DOUBLE GRAVE ACCENT
pub const U_MODIFIER_LETTER_MIDDLE_DOUBLE_GRAVE_ACCENT: u16 = 0x02F5;
/// U+02F6 MODIFIER LETTER MIDDLE DOUBLE ACUTE ACCENT
pub const U_MODIFIER_LETTER_MIDDLE_DOUBLE_ACUTE_ACCENT: u16 = 0x02F6;
/// U+02F7 MODIFIER LETTER LOW TILDE
pub const U_MODIFIER_LETTER_LOW_TILDE: u16 = 0x02F7;
/// U+02F8 MODIFIER LETTER RAISED COLON
pub const U_MODIFIER_LETTER_RAISED_COLON: u16 = 0x02F8;
/// U+02F9 MODIFIER LETTER BEGIN HIGH TONE
pub const U_MODIFIER_LETTER_BEGIN_HIGH_TONE: u16 = 0x02F9;
/// U+02FA MODIFIER LETTER END HIGH TONE
pub const U_MODIFIER_LETTER_END_HIGH_TONE: u16 = 0x02FA;
/// U+02FB MODIFIER LETTER BEGIN LOW TONE
pub const U_MODIFIER_LETTER_BEGIN_LOW_TONE: u16 = 0x02FB;
/// U+02FC MODIFIER LETTER END LOW TONE
pub const U_MODIFIER_LETTER_END_LOW_TONE: u16 = 0x02FC;
/// U+02FD MODIFIER LETTER SHELF
pub const U_MODIFIER_LETTER_SHELF: u16 = 0x02FD;
/// U+02FE MODIFIER LETTER OPEN SHELF
pub const U_MODIFIER_LETTER_OPEN_SHELF: u16 = 0x02FE;
/// U+02FF MODIFIER LETTER LOW LEFT ARROW
pub const U_MODIFIER_LETTER_LOW_LEFT_ARROW: u16 = 0x02FF;
/// U+0375 GREEK LOWER NUMERAL SIGN
pub const U_GREEK_LOWER_NUMERAL_SIGN: u16 = 0x0375;
/// U+0384 GREEK TONOS
pub const U_GREEK_TONOS: u16 = 0x0384;
/// U+0385 GREEK DIALYTIKA TONOS
pub const U_GREEK_DIALYTIKA_TONOS: u16 = 0x0385;
/// U+1FBD GREEK KORONIS
pub const U_GREEK_KORONIS: u16 = 0x1FBD;
/// U+1FBF GREEK PSILI
pub const U_GREEK_PSILI: u16 = 0x1FBF;
/// U+1FC0 GREEK PERISPOMENI
pub const U_GREEK_PERISPOMENI: u16 = 0x1FC0;
/// U+1FC1 GREEK DIALYTIKA AND PERISPOMENI
pub const U_GREEK_DIALYTIKA_AND_PERISPOMENI: u16 = 0x1FC1;
/// U+1FCD GREEK PSILI AND VARIA
pub const U_GREEK_PSILI_AND_VARIA: u16 = 0x1FCD;
/// U+1FCE GREEK PSILI AND OXIA
pub const U_GREEK_PSILI_AND_OXIA: u16 = 0x1FCE;
/// U+1FCF GREEK PSILI AND PERISPOMENI
pub const U_GREEK_PSILI_AND_PERISPOMENI: u16 = 0x1FCF;
/// U+1FDD GREEK DASIA AND VARIA
pub const U_GREEK_DASIA_AND_VARIA: u16 = 0x1FDD;
/// U+1FDE GREEK DASIA AND OXIA
pub const U_GREEK_DASIA_AND_OXIA: u16 = 0x1FDE;
/// U+1FDF GREEK DASIA AND PERISPOMENI
pub const U_GREEK_DASIA_AND_PERISPOMENI: u16 = 0x1FDF;
/// U+1FED GREEK DIALYTIKA AND VARIA
pub const U_GREEK_DIALYTIKA_AND_VARIA: u16 = 0x1FED;
/// U+1FEE GREEK DIALYTIKA AND OXIA
pub const U_GREEK_DIALYTIKA_AND_OXIA: u16 = 0x1FEE;
/// U+1FEF GREEK VARIA
pub const U_GREEK_VARIA: u16 = 0x1FEF;
/// U+1FFD GREEK OXIA
pub const U_GREEK_OXIA: u16 = 0x1FFD;
/// U+1FFE GREEK DASIA
pub const U_GREEK_DASIA: u16 = 0x1FFE;
/// U+3002 IDEOGRAPHIC FULL STOP
pub const U_IDEOGRAPHIC_FULL_STOP: u16 = 0x3002;
/// U+300C LEFT CORNER BRACKET
pub const U_LEFT_CORNER_BRACKET: u16 = 0x300C;
/// U+300D RIGHT CORNER BRACKET
pub const U_RIGHT_CORNER_BRACKET: u16 = 0x300D;
/// U+3010 LEFT BLACK LENTICULAR BRACKET
pub const U_LEFT_BLACK_LENTICULAR_BRACKET: u16 = 0x3010;
/// U+3011 RIGHT BLACK LENTICULAR BRACKET
pub const U_RIGHT_BLACK_LENTICULAR_BRACKET: u16 = 0x3011;
/// Unicode Character 'OVERLINE'
pub const U_OVERLINE: u16 = 0x203E;
/// UTF-8 BOM
/// Unicode Character 'ZERO WIDTH NO-BREAK SPACE' (U+FEFF)
/// http://www.fileformat.info/info/unicode/char/feff/index.htm
pub const UTF8_BOM: u16 = 65279;
/// U+FF1B FULLWIDTH SEMICOLON
pub const U_FULLWIDTH_SEMICOLON: u16 = 0xFF1B;
/// U+FF0C FULLWIDTH COMMA
pub const U_FULLWIDTH_COMMA: u16 = 0xFF0C;
