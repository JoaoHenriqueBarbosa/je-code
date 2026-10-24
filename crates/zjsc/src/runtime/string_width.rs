//! Porte de `Bun.stringWidth` (`stringWidth.cpp` e `ANSIHelpers.h` do bun): a largura visível em colunas
//! de um texto UTF-16, ciente de sequências ANSI, de clusters de grafema (ZWJ, bandeiras, keycaps, seletores
//! de variação) e do East Asian Width, usada pelo `console.table`.
//!
//! Os núcleos SIMD do bun (`highway_*`) são reproduzidos em Rust escalar com o mesmo resultado, inclusive
//! o caminho em bloco para texto que forma o próprio cluster (`bulk_unit_width`): o primeiro codepoint
//! do bloco não consulta a quebra de grafema, só descarrega o cluster pendente, então um `Prepend` logo
//! antes dele não o gruda.
//!
//! As sequências ANSI são as de `ansi.rs` (o `ANSIHelpers.h`, que o `Bun.stripANSI` também usa). O modo
//! `perCodePoint` (o do node) mede cada codepoint pela tabela tirada do ICU do bun no oráculo
//! (`string_width_per_code_point.rs`).

use super::ansi::{consume_ansi, is_escape_character};
use super::string_width_per_code_point::RUNS;
use super::string_width_tables::{GRAPHEME_BREAK_STAGE_1, GRAPHEME_BREAK_STAGE_2, GRAPHEME_BREAK_STAGE_3};

const FUSED_CLASS_MASK: u8 = 0x1F;
const FUSED_WIDTH_SHIFT: u8 = 5;
const FUSED_WIDTH_MASK: u8 = 0x3;
const FUSED_WIDTH_AMBIGUOUS: u8 = 3;
const FUSED_EMOJI_BIT: u8 = 0x80;

/// A classificação empacotada de `cp` (classe de quebra, largura e Emoji) pela tabela de três estágios.
pub fn fused_classify(cp: u32) -> u8 {
    let high = (cp >> 8) as usize;
    let low = (cp & 0xFF) as usize;
    let stage2_index = GRAPHEME_BREAK_STAGE_1[high] as usize + low;
    GRAPHEME_BREAK_STAGE_3[GRAPHEME_BREAK_STAGE_2[stage2_index] as usize]
}

/// A largura em colunas a partir do byte empacotado.
fn width_from_fused(packed: u8, ambiguous_as_wide: bool) -> u8 {
    let width = (packed >> FUSED_WIDTH_SHIFT) & FUSED_WIDTH_MASK;
    if width == FUSED_WIDTH_AMBIGUOUS {
        return if ambiguous_as_wide { 2 } else { 1 };
    }
    width
}

/// A largura em colunas de um codepoint (`visibleCodepointWidth`).
pub fn visible_codepoint_width(cp: u32, ambiguous_as_wide: bool) -> u8 {
    width_from_fused(fused_classify(cp), ambiguous_as_wide)
}

/// A propriedade Emoji do codepoint (`isEmojiPresentation`).
pub fn is_emoji_presentation(cp: u32) -> bool {
    fused_classify(cp) & FUSED_EMOJI_BIT != 0
}

// Ordinais de `GraphemeBreakClass`; devem casar com o estágio 3.
const OTHER: u8 = 0;
const PREPEND: u8 = 1;
const REGIONAL_INDICATOR: u8 = 2;
const SPACING_MARK: u8 = 3;
const L: u8 = 4;
const V: u8 = 5;
const T: u8 = 6;
const LV: u8 = 7;
const LVT: u8 = 8;
const ZWJ: u8 = 9;
const ZWNJ: u8 = 10;
const EXTENDED_PICTOGRAPHIC: u8 = 11;
const EMOJI_MODIFIER_BASE: u8 = 12;
const EMOJI_MODIFIER: u8 = 13;
const INDIC_EXTEND: u8 = 14;
const INDIC_LINKER: u8 = 15;
const INDIC_CONSONANT: u8 = 16;

/// O estado carregado entre chamadas sequenciais da quebra de grafema.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakState {
    Default,
    RegionalIndicator,
    ExtendedPictographic,
    IndicConsonant,
    IndicLinker,
}

fn class_from_fused(packed: u8) -> u8 {
    packed & FUSED_CLASS_MASK
}

fn is_indic_extend(gb: u8) -> bool {
    gb == INDIC_EXTEND || gb == ZWJ
}

fn is_extend(gb: u8) -> bool {
    gb == ZWNJ || gb == INDIC_EXTEND || gb == INDIC_LINKER
}

fn is_extended_pictographic(gb: u8) -> bool {
    gb == EXTENDED_PICTOGRAPHIC || gb == EMOJI_MODIFIER_BASE
}

/// O algoritmo de quebra de grafema do uucode (`computeGraphemeBreakNoControl`), com GB9c. `true` quando há
/// quebra entre as duas classes.
fn compute_grapheme_break(gb1: u8, gb2: u8, state: &mut BreakState) -> bool {
    match *state {
        BreakState::RegionalIndicator => {
            if gb1 != REGIONAL_INDICATOR || gb2 != REGIONAL_INDICATOR {
                *state = BreakState::Default;
            }
        }
        BreakState::ExtendedPictographic => {
            let expected = |gb: u8| {
                gb == INDIC_EXTEND
                    || gb == INDIC_LINKER
                    || gb == ZWNJ
                    || gb == ZWJ
                    || gb == EXTENDED_PICTOGRAPHIC
                    || gb == EMOJI_MODIFIER_BASE
                    || gb == EMOJI_MODIFIER
            };
            if !expected(gb1) {
                *state = BreakState::Default;
            }
            if !expected(gb2) {
                *state = BreakState::Default;
            }
        }
        BreakState::IndicConsonant | BreakState::IndicLinker => {
            let expected = |gb: u8| gb == INDIC_CONSONANT || gb == INDIC_LINKER || gb == INDIC_EXTEND || gb == ZWJ;
            if !expected(gb1) {
                *state = BreakState::Default;
            }
            if !expected(gb2) {
                *state = BreakState::Default;
            }
        }
        BreakState::Default => {}
    }

    // GB6, GB7, GB8: sequências de hangul.
    if gb1 == L && (gb2 == L || gb2 == V || gb2 == LV || gb2 == LVT) {
        return false;
    }
    if (gb1 == LV || gb1 == V) && (gb2 == V || gb2 == T) {
        return false;
    }
    if (gb1 == LVT || gb1 == T) && gb2 == T {
        return false;
    }

    // GB9a: SpacingMark. GB9b: Prepend.
    if gb2 == SPACING_MARK {
        return false;
    }
    if gb1 == PREPEND {
        return false;
    }

    // GB9c: Indic Conjunct Break.
    if gb1 == INDIC_CONSONANT {
        if is_indic_extend(gb2) {
            *state = BreakState::IndicConsonant;
            return false;
        }
        if gb2 == INDIC_LINKER {
            *state = BreakState::IndicLinker;
            return false;
        }
    } else if *state == BreakState::IndicConsonant {
        if gb2 == INDIC_LINKER {
            *state = BreakState::IndicLinker;
            return false;
        }
        if is_indic_extend(gb2) {
            return false;
        }
        *state = BreakState::Default;
    } else if *state == BreakState::IndicLinker {
        if gb2 == INDIC_LINKER || is_indic_extend(gb2) {
            return false;
        }
        if gb2 == INDIC_CONSONANT {
            *state = BreakState::Default;
            return false;
        }
        *state = BreakState::Default;
    }

    // GB11: sequência ZWJ de emoji e sequência com modificador.
    if is_extended_pictographic(gb1) {
        if is_extend(gb2) || gb2 == ZWJ {
            *state = BreakState::ExtendedPictographic;
            return false;
        }
        if gb1 == EMOJI_MODIFIER_BASE && gb2 == EMOJI_MODIFIER {
            *state = BreakState::ExtendedPictographic;
            return false;
        }
    } else if *state == BreakState::ExtendedPictographic {
        if (is_extend(gb1) || gb1 == EMOJI_MODIFIER) && (is_extend(gb2) || gb2 == ZWJ) {
            return false;
        }
        if gb1 == ZWJ && is_extended_pictographic(gb2) {
            *state = BreakState::Default;
            return false;
        }
        *state = BreakState::Default;
    }

    // GB12 e GB13: indicadores regionais.
    if gb1 == REGIONAL_INDICATOR && gb2 == REGIONAL_INDICATOR {
        if *state == BreakState::Default {
            *state = BreakState::RegionalIndicator;
            return false;
        }
        *state = BreakState::Default;
        return true;
    }

    // GB9: x (Extend | ZWJ).
    if is_extend(gb2) || gb2 == ZWJ {
        return false;
    }

    // GB999.
    true
}

fn is_regional_indicator(cp: u32) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&cp)
}

fn is_skin_tone_modifier(cp: u32) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&cp)
}

/// Acumula os codepoints de um cluster e decide a largura dele (`GraphemeState`).
#[derive(Default, Clone, Copy)]
struct GraphemeState {
    first_cp: u32,
    non_emoji_width: u16,
    base_width: u8,
    count: u8,
    emoji_base: bool,
    keycap: bool,
    regional_indicator: bool,
    skin_tone: bool,
    zwj: bool,
    vs15: bool,
    vs16: bool,
}

impl GraphemeState {
    fn reset(&mut self, cp: u32, packed: u8, ambiguous_as_wide: bool) {
        if cp < 0x80 {
            let w = u8::from((0x20..0x7F).contains(&cp));
            *self = GraphemeState { first_cp: cp, count: 1, base_width: w, non_emoji_width: u16::from(w), ..GraphemeState::default() };
            return;
        }
        let w = width_from_fused(packed, ambiguous_as_wide);
        *self = GraphemeState {
            first_cp: cp,
            count: 1,
            base_width: w,
            non_emoji_width: u16::from(w),
            emoji_base: packed & FUSED_EMOJI_BIT != 0,
            keycap: cp == 0x20E3,
            regional_indicator: is_regional_indicator(cp),
            skin_tone: is_skin_tone_modifier(cp),
            zwj: cp == 0x200D,
            vs15: false,
            vs16: false,
        };
    }

    fn add(&mut self, cp: u32, packed: u8, ambiguous_as_wide: bool) {
        if self.count < u8::MAX {
            self.count += 1;
        }
        self.keycap = self.keycap || cp == 0x20E3;
        self.regional_indicator = self.regional_indicator || is_regional_indicator(cp);
        self.skin_tone = self.skin_tone || is_skin_tone_modifier(cp);
        self.zwj = self.zwj || cp == 0x200D;
        self.vs15 = self.vs15 || cp == 0xFE0E;
        self.vs16 = self.vs16 || cp == 0xFE0F;
        let new_width = u32::from(self.non_emoji_width) + u32::from(width_from_fused(packed, ambiguous_as_wide));
        self.non_emoji_width = new_width.min(1023) as u16;
    }

    fn width(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        if self.regional_indicator && self.count >= 2 {
            return 2;
        }
        if self.keycap {
            return 2;
        }
        if self.regional_indicator {
            return 1;
        }
        if self.emoji_base && (self.skin_tone || self.zwj) {
            return 2;
        }
        if self.vs15 || self.vs16 {
            if self.base_width == 2 || (self.vs16 && (self.emoji_base || self.first_cp == 0xA9 || self.first_cp == 0xAE)) {
                return 2;
            }
            return usize::from(self.base_width);
        }
        usize::from(self.non_emoji_width)
    }
}

/// A largura fixa de uma unidade UTF-16 que sempre forma o próprio cluster (`ClassifyBulkUTF16Unit`):
/// ASCII imprimível, a maior parte do latino, grego e cirílico (1) e kana, CJK, sílabas hangul e formas
/// de largura plena (2). `None` encerra o run em bloco.
fn bulk_unit_width(u: u16) -> Option<u8> {
    match u {
        0x20..=0x7E | 0xA0..=0x2FF | 0x370..=0x482 | 0x48A..=0x52F if !matches!(u, 0xA9 | 0xAD | 0xAE) => Some(1),
        0x3041..=0x3096 | 0x309B..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7A3 | 0xFF01..=0xFF60 => Some(2),
        _ => None,
    }
}

/// O acumulador de largura para runs de UTF-16 sem sequências de escape (`UTF16WidthAccumulator`).
struct WidthAccumulator {
    len: usize,
    grapheme: GraphemeState,
    /// A classe do último codepoint visível; bytes de escape não participam da quebra.
    prev_class: u8,
    break_state: BreakState,
    has_prev_visible: bool,
    ambiguous_as_wide: bool,
}

impl WidthAccumulator {
    fn new(ambiguous_as_wide: bool) -> Self {
        WidthAccumulator { len: 0, grapheme: GraphemeState::default(), prev_class: OTHER, break_state: BreakState::Default, has_prev_visible: false, ambiguous_as_wide }
    }

    fn add_codepoint(&mut self, cp: u32) {
        let packed = fused_classify(cp);
        let class = class_from_fused(packed);
        if !self.has_prev_visible {
            self.grapheme.reset(cp, packed, self.ambiguous_as_wide);
        } else if compute_grapheme_break(self.prev_class, class, &mut self.break_state) {
            self.len += self.grapheme.width();
            self.grapheme.reset(cp, packed, self.ambiguous_as_wide);
        } else {
            self.grapheme.add(cp, packed, self.ambiguous_as_wide);
        }
        self.has_prev_visible = true;
        self.prev_class = class;
    }

    /// Semeia o cluster com o último codepoint de um run em bloco, descarregando o cluster pendente. A
    /// largura dele não entra aqui: uma marca combinante logo depois ainda se junta a ele.
    fn seed_from_bulk_run(&mut self, cp: u32, packed: u8) {
        if self.grapheme.count > 0 {
            self.len += self.grapheme.width();
        }
        self.grapheme.reset(cp, packed, self.ambiguous_as_wide);
        self.has_prev_visible = true;
        self.prev_class = class_from_fused(packed);
        self.break_state = BreakState::Default;
    }

    /// Consome texto até o fim de `input` ou, com `stop_at_escape`, até o primeiro introdutor de escape.
    /// Devolve as unidades consumidas.
    fn add_run(&mut self, input: &[u16], stop_at_escape: bool) -> usize {
        let mut pos = 0;
        loop {
            if pos >= input.len() || (stop_at_escape && is_escape_character(input[pos])) {
                break;
            }
            // O caminho em bloco (`highway_visible_utf16_width`): o prefixo de unidades que são sempre o
            // próprio cluster e têm largura fixa. Só vale com ambíguos estreitos, e o primeiro olhar
            // descarta controle e surrogate.
            if !self.ambiguous_as_wide && input[pos] >= 0x20 && !(0xD800..0xE000).contains(&input[pos]) {
                let mut bulk_width = 0usize;
                let consumed = input[pos..].iter().map_while(|&u| bulk_unit_width(u)).inspect(|&w| bulk_width += usize::from(w)).count();
                if consumed > 0 {
                    let last_cp = u32::from(input[pos + consumed - 1]);
                    let last_packed = fused_classify(last_cp);
                    self.seed_from_bulk_run(last_cp, last_packed);
                    self.len += bulk_width - usize::from(width_from_fused(last_packed, self.ambiguous_as_wide));
                    pos += consumed;
                    continue;
                }
            }
            // O run ASCII, limitado pelo próximo ESC quando `stop_at_escape`.
            let mut idx = 0;
            if input[pos] <= 0x7F {
                let mut bound = input.len() - pos;
                if stop_at_escape
                    && let Some(esc) = input[pos..].iter().position(|&u| u == 0x1B)
                {
                    bound = esc;
                }
                idx = input[pos..pos + bound].iter().position(|&u| u > 0x7F).unwrap_or(bound);
            }
            if idx > 0 {
                let last_cp = u32::from(input[pos + idx - 1]);
                let last_packed = fused_classify(last_cp);
                self.seed_from_bulk_run(last_cp, last_packed);
                self.len += input[pos..pos + idx - 1].iter().filter(|&&u| (0x20..0x7F).contains(&u)).count();
                pos += idx;
                continue;
            }
            // Um codepoint não ASCII; surrogates soltos têm largura zero e não mexem no estado.
            let unit = input[pos];
            if (0xD800..0xDC00).contains(&unit) {
                match input.get(pos + 1) {
                    Some(&next) if (0xDC00..0xE000).contains(&next) => {
                        let cp = 0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(next) - 0xDC00);
                        pos += 2;
                        self.add_codepoint(cp);
                    }
                    _ => pos += 1,
                }
            } else if (0xDC00..0xE000).contains(&unit) {
                pos += 1;
            } else {
                pos += 1;
                self.add_codepoint(u32::from(unit));
            }
        }
        pos
    }

    fn finish(&self) -> usize {
        self.len + self.grapheme.width()
    }
}

/// A largura visível de `units` com clusters de grafema. Com `count_ansi` falso, sequências de escape não
/// contam (`visibleUTF16Width` com `excludeAnsiColors`); com verdadeiro, os bytes delas contam como
/// codepoints comuns.
pub fn visible_width(units: &[u16], ambiguous_as_wide: bool, count_ansi: bool) -> usize {
    let mut accumulator = WidthAccumulator::new(ambiguous_as_wide);
    if count_ansi {
        accumulator.add_run(units, false);
        return accumulator.finish();
    }
    let mut pos = 0;
    while pos < units.len() {
        if is_escape_character(units[pos]) {
            let next = consume_ansi(units, pos);
            pos = if next == pos { pos + 1 } else { next };
            continue;
        }
        pos += accumulator.add_run(&units[pos..], true);
    }
    accumulator.finish()
}

/// `perCodePointColumnWidth` (o `GetColumnWidth` do node sobre o ICU do bun), pela medição no oráculo.
fn per_code_point_column_width(cp: u32, ambiguous_as_wide: bool) -> usize {
    let (_, narrow, wide) = RUNS[RUNS.partition_point(|run| run.0 <= cp) - 1];
    usize::from(if ambiguous_as_wide { wide } else { narrow })
}

/// `perCodePointUTF16Width`: cada codepoint pela própria largura, sem grafemas (uma sequência de emoji com ZWJ conta
/// cada membro); o surrogate solto conta como ele mesmo. Com `count_ansi` falso as sequências de escape não contam.
pub fn per_code_point_width(units: &[u16], ambiguous_as_wide: bool, count_ansi: bool) -> usize {
    let mut width = 0;
    let mut pos = 0;
    while pos < units.len() {
        let unit = units[pos];
        if !count_ansi && is_escape_character(unit) {
            pos = consume_ansi(units, pos);
            continue;
        }
        let mut cp = u32::from(unit);
        pos += 1;
        if (0xD800..0xDC00).contains(&unit)
            && let Some(&next) = units.get(pos).filter(|next| (0xDC00..0xE000).contains(*next))
        {
            cp = 0x10000 + ((cp - 0xD800) << 10) + (u32::from(next) - 0xDC00);
            pos += 1;
        }
        width += per_code_point_column_width(cp, ambiguous_as_wide);
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(text: &str) -> usize {
        let units: Vec<u16> = text.encode_utf16().collect();
        visible_width(&units, false, false)
    }

    fn width_counting_ansi(text: &str) -> usize {
        let units: Vec<u16> = text.encode_utf16().collect();
        visible_width(&units, false, true)
    }

    #[test]
    fn ascii_and_cjk() {
        assert_eq!(width(""), 0);
        assert_eq!(width("hello"), 5);
        assert_eq!(width("中文"), 4);
        assert_eq!(width("a中b"), 4);
        assert_eq!(width("한"), 2);
    }

    #[test]
    fn emoji_clusters() {
        assert_eq!(width("\u{1F468}\u{200D}\u{1F469}"), 2);
        assert_eq!(width("\u{2764}\u{FE0F}"), 2);
        assert_eq!(width("\u{1F44D}\u{1F3FD}"), 2);
        assert_eq!(width("\u{1F1E7}\u{1F1F7}"), 2);
    }

    #[test]
    fn combining_and_zero_width() {
        assert_eq!(width("e\u{0301}"), 1);
        assert_eq!(width("\u{200B}"), 0);
        assert_eq!(width("\u{1112}\u{1161}\u{11AB}"), 2);
    }

    #[test]
    fn controls_and_ansi() {
        assert_eq!(width("a\u{1}b"), 2);
        assert_eq!(width("\u{1b}[31mab\u{1b}[0m"), 2);
        assert_eq!(width_counting_ansi("\u{1b}[31mab\u{1b}[0m"), 9);
    }

    #[test]
    fn prepend_before_bulk_blocks() {
        // Valores medidos com `Bun.stringWidth` do bun 1.4.2. O bloco não gruda no `Prepend` anterior.
        let cases: &[(&str, usize)] = &[
            ("\u{600}中", 2),
            ("\u{600}中中", 4),
            ("x\u{600}中y\u{600}中中\u{301}", 8),
            ("\u{600}한한", 4),
            ("\u{600}é", 1),
            ("\u{600}éé", 2),
            ("x\u{600}éy\u{600}éé\u{301}", 5),
            ("\u{600}😀", 2),
            ("\u{600}😀😀", 4),
            ("\u{600}ああ", 4),
            ("\u{600}ＡＡ", 4),
            ("\u{600}αα", 2),
            ("\u{110BD}中", 3),
            ("\u{110BD}중", 3),
            ("\u{110BD}한한", 5),
            ("\u{110BD}é", 2),
            ("\u{110BD}😀", 3),
            ("\u{110BD}😀😀", 5),
            ("x\u{110BD}😀y\u{110BD}😀😀\u{301}", 10),
            ("\u{110BD}aa", 3),
            ("\u{D4E}中", 3),
            ("\u{D4E}한", 3),
            ("\u{D4E}éé", 3),
            ("\u{D4E}😀", 3),
            ("\u{D4E}ああ", 5),
            ("x\u{D4E}ay\u{D4E}aa\u{301}", 7),
            (
                "a中한é\u{600}中中한한éé\u{D4E}ああ\u{110BD}😀中bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{600}中中中中中中中中中中中中中中中中中中中中\u{600}😀\u{600}한",
                110,
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(width(text), *expected, "{text:?}");
        }
    }

    #[test]
    fn codepoint_tables() {
        assert_eq!(visible_codepoint_width(0x41, false), 1);
        assert_eq!(visible_codepoint_width(0x1B, false), 0);
        assert_eq!(visible_codepoint_width(0x4E2D, false), 2);
        assert_eq!(visible_codepoint_width(0xA7, false), 1);
        assert_eq!(visible_codepoint_width(0xA7, true), 2);
        assert!(is_emoji_presentation(0x1F600));
    }
}
