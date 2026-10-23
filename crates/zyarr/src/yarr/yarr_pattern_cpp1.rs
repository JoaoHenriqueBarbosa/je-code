//! Porte de `yarr/YarrPattern.cpp`, primeira fatia (linhas 1 a 768): o `CharacterClassConstructor`,
//! do construtor até `subtractionStrings`. O restante da classe (`latin1Op`, `latin1Invert`,
//! `nonLatin1OpSorted`, `nonLatin1Invert`, `coalesceTables`, `computeCharacterWidths`,
//! `anyCharacter`, `isUnionSetOp`) vem na fatia seguinte, no mesmo `impl`.
//!
//! As funções de membro do C++ que recebem um `Vector&` que é, ao mesmo tempo, membro do próprio
//! construtor (`addSorted(m_matches8, ch)`, `addSortedRange(m_ranges8, ...)`) viram funções livres que
//! recebem os campos por empréstimo disjunto (`add_sorted_to`, `add_sorted_range_to`), para o
//! empréstimo único do Rust refletir o aliasing do C++ sem cópia.

use crate::wtf::ascii_ctype::{is_ascii, is_ascii_alpha, to_ascii_lower, to_ascii_upper};
use crate::yarr::yarr_canonicalize::UCS2CanonicalizationType::{
    CanonicalizeAlternatingAligned, CanonicalizeAlternatingUnaligned, CanonicalizeRangeHi,
    CanonicalizeRangeLo, CanonicalizeSet, CanonicalizeUnique,
};
use crate::yarr::yarr_canonicalize::{
    canonical_character_set_info, canonical_range_info_for, get_canonical_pair, CanonicalMode,
    CanonicalizationRange,
};
use crate::yarr::yarr_parser::CharacterClassSetOp;
use crate::yarr::yarr_flags::FlagSet;
use crate::yarr::yarr_pattern::{
    AlternativeId, ByteTable, CharacterClass, CharacterClassId, CharacterClassWidths, CharacterRange,
    CompileMode, DisjunctionId, MatchDirection, PatternAlternative, PatternDisjunction, PatternTerm,
    PatternTermType, YarrPattern, UCHAR_MAX_VALUE,
};
use crate::wtf::text::wtf_string::String;
use crate::yarr::yarr_parser::Delegate;
use std::collections::HashMap;

/// `isLatin1(char32_t)`.
pub(crate) fn is_latin1(ch: u32) -> bool {
    ch <= 0xff
}

/// `U_IS_BMP(c)` do ICU.
fn u_is_bmp(ch: u32) -> bool {
    ch <= 0xffff
}

/// `CharacterClassConstructor::compareUTF32Strings`.
pub fn compare_utf32_strings(a: &[u32], b: &[u32]) -> i32 {
    // Longer strings before shorter.
    if a.len() > b.len() {
        return -1;
    }

    if a.len() < b.len() {
        return 1;
    }

    // Lexically sort for same length strings.
    for i in 0..a.len() {
        if a[i] != b[i] {
            return if a[i] < b[i] { -1 } else { 1 };
        }
    }

    0
}

/// O `char32_t` entra no vetor de 8 bits ou no de 32 bits conforme `isLatin1` (os lambdas `addChar`
/// e `addCh` do C++).
fn push_by_width(matches8: &mut Vec<u32>, matches32: &mut Vec<u32>, ch: u32) {
    if is_latin1(ch) {
        matches8.push(ch);
    } else {
        matches32.push(ch);
    }
}

/// `mergeRangesFrom(ranges, index)`.
fn merge_ranges_from(ranges: &mut Vec<CharacterRange>, index: usize) {
    let next = index + 1;

    // each iteration of the loop we will either remove something from the list, or break out of the loop.
    while next < ranges.len() {
        if ranges[next].begin <= (ranges[index].end + 1) {
            // the next entry now overlaps / concatenates with this one.
            ranges[index].end = ranges[index].end.max(ranges[next].end);
            ranges.remove(next);
        } else {
            break;
        }
    }
}

/// `addSortedRange(Vector<CharacterRange>& ranges, lo, hi)`. `widths` é o `m_characterWidths`.
fn add_sorted_range_to(ranges: &mut Vec<CharacterRange>, widths: &mut CharacterClassWidths, lo: u32, hi: u32) {
    if u_is_bmp(lo) {
        *widths |= CharacterClassWidths::HasBMPChars;
    }
    if !u_is_bmp(hi) {
        *widths |= CharacterClassWidths::HasNonBMPChars;
    }

    let iter = ranges.partition_point(|range| (range.end as u64) + 1 < lo as u64);
    if iter == ranges.len() {
        // CharacterRange comes after all existing ranges.
        ranges.push(CharacterRange::new(lo, hi));
        return;
    }

    // does the new range fall before the current position in the array
    if hi < ranges[iter].begin {
        // Concatenate appending ranges.
        if hi == ranges[iter].begin.wrapping_sub(1) {
            ranges[iter].begin = lo;
            return;
        }
        ranges.insert(iter, CharacterRange::new(lo, hi));
        return;
    }

    // If the new range start at or before the end of the last range, then the overlap (if it starts one after the
    // end of the last range they concatenate, which is just as good.
    // found an intersect! we'll replace this entry in the array.
    ranges[iter].begin = ranges[iter].begin.min(lo);
    ranges[iter].end = ranges[iter].end.max(hi);
    merge_ranges_from(ranges, iter);
}

/// `addSorted(Vector<char32_t>& matches, char32_t ch)`. As faixas em que um vizinho se funde são as
/// do próprio construtor (`m_ranges8`/`m_ranges32`, escolhidas por `isLatin1(ch)`), mesmo quando
/// `matches` é o vetor de outra classe (o `inverted` de `appendInverted`).
fn add_sorted_to(
    matches: &mut Vec<u32>,
    ranges8: &mut Vec<CharacterRange>,
    ranges32: &mut Vec<CharacterRange>,
    widths: &mut CharacterClassWidths,
    ch: u32,
) {
    let mut pos: usize = 0;
    let mut range: usize = matches.len();

    *widths |= if u_is_bmp(ch) {
        CharacterClassWidths::HasBMPChars
    } else {
        CharacterClassWidths::HasNonBMPChars
    };

    // binary chop, find position to insert char.
    while range != 0 {
        let index = range >> 1;

        let val: i32 = (matches[pos + index] as i32).wrapping_sub(ch as i32);
        if val == 0 {
            return;
        } else if val > 0 {
            if val == 1 {
                let mut lo = ch;
                let hi = ch.wrapping_add(1);
                matches.remove(pos + index);
                if pos + index > 0 && matches[pos + index - 1] == ch.wrapping_sub(1) {
                    lo = ch.wrapping_sub(1);
                    matches.remove(pos + index - 1);
                }
                let target = if is_latin1(ch) { ranges8 } else { ranges32 };
                add_sorted_range_to(target, widths, lo, hi);
                return;
            }
            range = index;
        } else {
            if val == -1 {
                let lo = ch.wrapping_sub(1);
                let mut hi = ch;
                matches.remove(pos + index);
                if pos + index + 1 < matches.len() && matches[pos + index + 1] == ch.wrapping_add(1) {
                    hi = ch.wrapping_add(1);
                    matches.remove(pos + index + 1);
                }
                let target = if is_latin1(ch) { ranges8 } else { ranges32 };
                add_sorted_range_to(target, widths, lo, hi);
                return;
            }
            pos += index + 1;
            range -= index + 1;
        }
    }

    if pos == matches.len() {
        matches.push(ch);
    } else {
        matches.insert(pos, ch);
    }
}

/// `class CharacterClassConstructor`.
pub struct CharacterClassConstructor {
    pub is_case_insensitive: bool,
    pub any_character: bool,
    pub may_contain_strings: bool,
    pub inverted_strings: bool,
    pub compile_mode: CompileMode,
    pub character_widths: CharacterClassWidths,
    pub canonical_mode: CanonicalMode,

    pub strings: Vec<Vec<u32>>,
    pub matches8: Vec<u32>,
    pub ranges8: Vec<CharacterRange>,
    pub matches32: Vec<u32>,
    pub ranges32: Vec<CharacterRange>,
    pub set_op: CharacterClassSetOp,
}

impl CharacterClassConstructor {
    pub fn new(is_case_insensitive: bool, compile_mode: CompileMode) -> Self {
        CharacterClassConstructor {
            is_case_insensitive,
            any_character: false,
            may_contain_strings: false,
            inverted_strings: false,
            compile_mode,
            character_widths: CharacterClassWidths::Unknown,
            canonical_mode: if compile_mode == CompileMode::Legacy {
                CanonicalMode::UCS2
            } else {
                CanonicalMode::Unicode
            },
            strings: Vec::new(),
            matches8: Vec::new(),
            ranges8: Vec::new(),
            matches32: Vec::new(),
            ranges32: Vec::new(),
            set_op: CharacterClassSetOp::Default,
        }
    }

    pub fn reset(&mut self) {
        self.strings.clear();
        self.matches8.clear();
        self.ranges8.clear();
        self.matches32.clear();
        self.ranges32.clear();
        self.set_op = CharacterClassSetOp::Default;
        self.any_character = false;
        self.may_contain_strings = false;
        self.inverted_strings = false;
        self.character_widths = CharacterClassWidths::Unknown;
    }

    pub fn combining_set_op(&mut self, set_op: CharacterClassSetOp) {
        self.set_op = set_op;
    }

    pub fn append(&mut self, other: &CharacterClass) {
        if self.set_op != CharacterClassSetOp::Default {
            self.perform_set_op_with(other);
            return;
        }

        if !other.strings.is_empty() {
            self.perform_set_op_with_strings(&other.strings); // a union here: keeps m_strings sorted and repeat-free, as the set operations expect
        }
        for &m in &other.matches8 {
            self.add_sorted_in(true, m);
        }
        for range in &other.ranges8 {
            self.add_sorted_range_in(true, range.begin, range.end);
        }
        for &m in &other.matches32 {
            self.add_sorted_in(false, m);
        }
        for range in &other.ranges32 {
            self.add_sorted_range_in(false, range.begin, range.end);
        }
    }

    /// O lambda `addSortedMatchOrRange` do `appendInverted`. `dest` é a classe `inverted` (o
    /// `destMatches`/`destRanges` que não são membros do construtor) ou `None` para os membros.
    fn add_sorted_match_or_range(&mut self, latin1: bool, dest: Option<&mut CharacterClass>, lo: u32, hi_plus_one: u32) {
        if lo >= hi_plus_one {
            return;
        }

        if lo + 1 == hi_plus_one {
            match dest {
                None => self.add_sorted_in(latin1, lo),
                Some(inverted) => {
                    let matches = if latin1 { &mut inverted.matches8 } else { &mut inverted.matches32 };
                    add_sorted_to(matches, &mut self.ranges8, &mut self.ranges32, &mut self.character_widths, lo);
                }
            }
        } else {
            match dest {
                None => self.add_sorted_range_in(latin1, lo, hi_plus_one - 1),
                Some(inverted) => {
                    let ranges = if latin1 { &mut inverted.ranges8 } else { &mut inverted.ranges32 };
                    add_sorted_range_to(ranges, &mut self.character_widths, lo, hi_plus_one - 1);
                }
            }
        }
    }

    /// O lambda `addSortedInverted(min, max, srcMatches, srcRanges, destMatches, destRanges)`;
    /// `latin1` diz se o destino é o par de 8 bits ou o de 32 bits.
    fn add_sorted_inverted(
        &mut self,
        min: u32,
        max: u32,
        src_matches: &[u32],
        src_ranges: &[CharacterRange],
        latin1: bool,
        mut dest: Option<&mut CharacterClass>,
    ) {
        let mut lo = min;
        let mut matches_index: usize = 0;
        let mut ranges_index: usize = 0;
        let mut matches_remaining = matches_index < src_matches.len();
        let mut ranges_remaining = ranges_index < src_ranges.len();

        if !matches_remaining && !ranges_remaining {
            self.add_sorted_match_or_range(latin1, dest, min, max + 1);
            return;
        }

        while matches_remaining || ranges_remaining {
            let hi_plus_one: u32;
            let next_lo: u32;

            if matches_remaining && (!ranges_remaining || src_matches[matches_index] < src_ranges[ranges_index].begin) {
                hi_plus_one = src_matches[matches_index];
                next_lo = hi_plus_one + 1;
                matches_index += 1;
                matches_remaining = matches_index < src_matches.len();
            } else {
                hi_plus_one = src_ranges[ranges_index].begin;
                next_lo = src_ranges[ranges_index].end + 1;
                ranges_index += 1;
                ranges_remaining = ranges_index < src_ranges.len();
            }

            self.add_sorted_match_or_range(latin1, dest.as_deref_mut(), lo, hi_plus_one);

            lo = next_lo;
        }

        self.add_sorted_match_or_range(latin1, dest, lo, max + 1);
    }

    pub fn append_inverted(&mut self, other: &CharacterClass) {
        if other.has_strings() {
            self.may_contain_strings = true;
            self.inverted_strings = true;
        }

        if self.set_op != CharacterClassSetOp::Default {
            // The complement has no strings, so the pending operation has to be applied to the accumulated
            // strings as well (an intersection drops them all, a subtraction keeps them): materialize it and
            // go through append(), which applies m_setOp to both the characters and the strings.
            let mut inverted = CharacterClass::new();
            self.add_sorted_inverted(0, 0xff, &other.matches8, &other.ranges8, true, Some(&mut inverted));
            self.add_sorted_inverted(0x100, UCHAR_MAX_VALUE, &other.matches32, &other.ranges32, false, Some(&mut inverted));
            self.append(&inverted);
            return;
        }

        self.add_sorted_inverted(0, 0xff, &other.matches8, &other.ranges8, true, None);
        self.add_sorted_inverted(0x100, UCHAR_MAX_VALUE, &other.matches32, &other.ranges32, false, None);
    }

    pub fn put_char(&mut self, ch: u32) {
        if !self.is_union_set_op() {
            return self.put_char_non_union(ch);
        }

        if !self.is_case_insensitive {
            self.add_sorted(ch);
            return;
        }

        if self.canonical_mode == CanonicalMode::UCS2 && is_ascii(ch) {
            // Handle ASCII cases.
            if is_ascii_alpha(ch) {
                self.add_sorted_in(true, to_ascii_upper(ch));
                self.add_sorted_in(true, to_ascii_lower(ch));
            } else {
                self.add_sorted_in(true, ch);
            }
            return;
        }

        // Add multiple matches, if necessary.
        let info = canonical_range_info_for(ch, self.canonical_mode);
        if info.type_ == CanonicalizeUnique {
            self.add_sorted(ch);
        } else {
            self.put_unicode_ignore_case(ch, info);
        }
    }

    pub fn put_char_non_union(&mut self, ch: u32) {
        let mut matches8: Vec<u32> = Vec::new();
        let mut matches32: Vec<u32> = Vec::new();
        let empty_ranges: Vec<CharacterRange> = Vec::new();

        if self.set_op == CharacterClassSetOp::Intersection {
            self.strings.clear();
        }

        if !self.is_case_insensitive {
            push_by_width(&mut matches8, &mut matches32, ch);
            self.perform_set_op_with_matches(&matches8, &empty_ranges, &matches32, &empty_ranges);
            return;
        }

        if self.canonical_mode == CanonicalMode::UCS2 && is_ascii(ch) {
            // Handle ASCII cases.
            if is_ascii_alpha(ch) {
                push_by_width(&mut matches8, &mut matches32, to_ascii_upper(ch));
                push_by_width(&mut matches8, &mut matches32, to_ascii_lower(ch));
            } else {
                push_by_width(&mut matches8, &mut matches32, ch);
            }
            self.perform_set_op_with_matches(&matches8, &empty_ranges, &matches32, &empty_ranges);
            return;
        }

        // Add multiple matches, if necessary.
        let info = canonical_range_info_for(ch, self.canonical_mode);
        if info.type_ == CanonicalizeUnique {
            push_by_width(&mut matches8, &mut matches32, ch);
        } else if info.type_ == CanonicalizeSet {
            for &member in canonical_character_set_info(info.value, self.canonical_mode) {
                if member == 0 {
                    break;
                }
                push_by_width(&mut matches8, &mut matches32, member);
            }
        } else {
            let canonical_char = get_canonical_pair(info, ch);
            push_by_width(&mut matches8, &mut matches32, ch.min(canonical_char));
            push_by_width(&mut matches8, &mut matches32, ch.max(canonical_char));
        }

        self.perform_set_op_with_matches(&matches8, &empty_ranges, &matches32, &empty_ranges);
    }

    pub fn put_unicode_ignore_case(&mut self, ch: u32, info: &CanonicalizationRange) {
        if info.type_ == CanonicalizeSet {
            for &member in canonical_character_set_info(info.value, self.canonical_mode) {
                if member == 0 {
                    break;
                }
                self.add_sorted(member);
            }
        } else {
            self.add_sorted(ch);
            self.add_sorted(get_canonical_pair(info, ch));
        }
    }

    pub fn put_range(&mut self, mut lo: u32, hi: u32) {
        // The ASCII case-folding fast path is only valid in UCS2 canonical mode. In Unicode
        // canonical mode, U+212A and U+017F canonicalize into ASCII 'k' and 's', so ASCII
        // ranges must go through the canonicalization table below.
        if !self.is_case_insensitive || self.canonical_mode == CanonicalMode::UCS2 {
            // This is ASCII-case-folding fast path. So intentionally using isASCII (not isLatin1).
            if is_ascii(lo) {
                let ascii_lo = lo;
                let ascii_hi = hi.min(0x7f);
                self.add_sorted_range(lo, ascii_hi);

                if self.is_case_insensitive {
                    if ascii_lo <= 'Z' as u32 && ascii_hi >= 'A' as u32 {
                        self.add_sorted_range(
                            ascii_lo.max('A' as u32) + ('a' as u32 - 'A' as u32),
                            ascii_hi.min('Z' as u32) + ('a' as u32 - 'A' as u32),
                        );
                    }
                    if ascii_lo <= 'z' as u32 && ascii_hi >= 'a' as u32 {
                        self.add_sorted_range(
                            ascii_lo.max('a' as u32) - ('a' as u32 - 'A' as u32),
                            ascii_hi.min('z' as u32) - ('a' as u32 - 'A' as u32),
                        );
                    }
                }
            }
            if is_ascii(hi) {
                return;
            }

            lo = lo.max(0x80);
        }
        self.add_sorted_range(lo, hi);

        if !self.is_case_insensitive {
            return;
        }

        let mut info = canonical_range_info_for(lo, self.canonical_mode);
        loop {
            // Handle the range [lo .. end]
            let end = info.end.min(hi);

            match info.type_ {
                CanonicalizeUnique => {
                    // Nothing to do - no canonical equivalents.
                }
                CanonicalizeSet => {
                    for &member in canonical_character_set_info(info.value, self.canonical_mode) {
                        // `char16_t ch`: o valor é truncado para 16 bits antes do teste do terminador.
                        let ch = member as u16 as u32;
                        if ch == 0 {
                            break;
                        }
                        self.add_sorted(ch);
                    }
                }
                CanonicalizeRangeLo => {
                    self.add_sorted_range(lo + info.value, end + info.value);
                }
                CanonicalizeRangeHi => {
                    self.add_sorted_range(lo - info.value, end - info.value);
                }
                CanonicalizeAlternatingAligned => {
                    // Use addSortedRange since there is likely an abutting range to combine with.
                    if lo & 1 != 0 {
                        self.add_sorted_range(lo - 1, lo - 1);
                    }
                    if end & 1 == 0 {
                        self.add_sorted_range(end + 1, end + 1);
                    }
                }
                CanonicalizeAlternatingUnaligned => {
                    // Use addSortedRange since there is likely an abutting range to combine with.
                    if lo & 1 == 0 {
                        self.add_sorted_range(lo - 1, lo - 1);
                    }
                    if end & 1 != 0 {
                        self.add_sorted_range(end + 1, end + 1);
                    }
                }
            }

            if hi == end {
                return;
            }

            // `++info; lo = info->begin;`: a tabela é contígua, então o elemento seguinte é o que
            // contém `info.end + 1`.
            info = canonical_range_info_for(info.end + 1, self.canonical_mode);
            lo = info.begin;
        }
    }

    pub fn atom_class_string_disjunction(&mut self, disjunction_strings: &mut Vec<Vec<u32>>) {
        let mut utf32_strings: Vec<Vec<u32>> = Vec::new();
        let mut matches8: Vec<u32> = Vec::new();
        let mut matches32: Vec<u32> = Vec::new();
        let empty_ranges: Vec<CharacterRange> = Vec::new();

        Self::sort(disjunction_strings);
        disjunction_strings.dedup(); // \q{ab|ab} is the set {"ab"}; the set-op merges assume no repeats

        for string in disjunction_strings.iter() {
            if string.len() == 1 {
                let ch = string[0];
                if !self.is_case_insensitive {
                    push_by_width(&mut matches8, &mut matches32, ch);
                    continue;
                }

                // Add multiple matches, if necessary.
                let info = canonical_range_info_for(ch, self.canonical_mode);
                if info.type_ == CanonicalizeUnique {
                    push_by_width(&mut matches8, &mut matches32, ch);
                } else if info.type_ == CanonicalizeSet {
                    for &member in canonical_character_set_info(info.value, self.canonical_mode) {
                        if member == 0 {
                            break;
                        }
                        push_by_width(&mut matches8, &mut matches32, member);
                    }
                } else {
                    push_by_width(&mut matches8, &mut matches32, ch);
                    push_by_width(&mut matches8, &mut matches32, get_canonical_pair(info, ch));
                }
                continue;
            }

            utf32_strings.push(string.clone());
        }

        self.perform_set_op_with_strings(&utf32_strings);
        self.perform_set_op_with_matches(&matches8, &empty_ranges, &matches32, &empty_ranges);
    }

    pub fn invert_matches(&mut self) {
        if !self.strings.is_empty() {
            self.inverted_strings = true;
        }

        self.latin1_invert();
        self.non_latin1_invert();
    }

    /// `performSetOpWith(CharacterClassConstructor*)`.
    pub fn perform_set_op_with_constructor(&mut self, rhs: &CharacterClassConstructor) {
        self.perform_set_op_with_parts(&rhs.strings, &rhs.matches8, &rhs.ranges8, &rhs.matches32, &rhs.ranges32);
    }

    /// `performSetOpWith(const CharacterClass*)`.
    pub fn perform_set_op_with(&mut self, rhs: &CharacterClass) {
        self.perform_set_op_with_parts(&rhs.strings, &rhs.matches8, &rhs.ranges8, &rhs.matches32, &rhs.ranges32);
    }

    /// O corpo comum das duas sobrecargas de `performSetOpWith`.
    fn perform_set_op_with_parts(
        &mut self,
        strings: &[Vec<u32>],
        matches8: &[u32],
        ranges8: &[CharacterRange],
        matches32: &[u32],
        ranges32: &[CharacterRange],
    ) {
        self.perform_set_op_with_strings(strings);
        self.perform_set_op_with_matches(matches8, ranges8, matches32, ranges32);
    }

    pub fn perform_set_op_with_strings(&mut self, utf32_strings: &[Vec<u32>]) {
        if self.compile_mode != CompileMode::UnicodeSets {
            return;
        }

        match self.set_op {
            CharacterClassSetOp::Default | CharacterClassSetOp::Union => {
                self.union_strings(utf32_strings);
            }
            CharacterClassSetOp::Intersection => {
                self.intersection_strings(utf32_strings);
            }
            CharacterClassSetOp::Subtraction => {
                self.subtraction_strings(utf32_strings);
            }
        }
    }

    pub fn perform_set_op_with_matches(
        &mut self,
        rhs_matches8: &[u32],
        rhs_ranges8: &[CharacterRange],
        rhs_matches32: &[u32],
        rhs_ranges32: &[CharacterRange],
    ) {
        if self.compile_mode != CompileMode::UnicodeSets {
            return;
        }

        self.latin1_op(rhs_matches8, rhs_ranges8);
        // Sort the incoming non-Latin-1 matches, since Unicode case folding canonicalization may cause
        // characters to be added to rhsMatches32 out of code point order.
        let mut rhs_sorted_matches32 = rhs_matches32.to_vec();
        rhs_sorted_matches32.sort();

        self.non_latin1_op_sorted(&rhs_sorted_matches32, rhs_ranges32);
    }

    pub fn has_inverted_strings(&self) -> bool {
        self.inverted_strings
    }

    pub fn sort(utf32_strings: &mut Vec<Vec<u32>>) {
        utf32_strings.sort_by(|a, b| compare_utf32_strings(a, b).cmp(&0));
    }

    pub fn char_class(&mut self) -> CharacterClass {
        self.coalesce_tables();

        if !self.strings.is_empty() {
            Self::sort(&mut self.strings);
        }

        let mut character_class = CharacterClass::new();

        character_class.strings = std::mem::take(&mut self.strings);
        character_class.matches8 = std::mem::take(&mut self.matches8);
        character_class.ranges8 = std::mem::take(&mut self.ranges8);
        character_class.matches32 = std::mem::take(&mut self.matches32);
        character_class.ranges32 = std::mem::take(&mut self.ranges32);
        character_class.any_character = self.any_character();
        character_class.character_widths = Self::compute_character_widths(&character_class);

        Self::build_latin1_table_if_beneficial(&mut character_class);

        self.any_character = false;
        self.character_widths = CharacterClassWidths::Unknown;

        character_class
    }

    pub fn set_is_case_insensitive(&mut self, ignore_case: bool) {
        self.is_case_insensitive = ignore_case;
    }

    pub fn build_latin1_table_if_beneficial(character_class: &mut CharacterClass) {
        if !character_class.strings.is_empty() || character_class.any_character || character_class.table.is_some() {
            return;
        }

        let matches = &character_class.matches8;
        let ranges = &character_class.ranges8;

        // A single range is already one subtract + one compare in the JIT, so a table would not help.
        let entry_count = matches.len() + ranges.len();
        if entry_count < 2 {
            return;
        }

        let mut low = if !ranges.is_empty() { ranges[0].begin } else { matches[0] };
        let mut high = if !ranges.is_empty() { ranges[ranges.len() - 1].end } else { matches[matches.len() - 1] };
        if !matches.is_empty() {
            low = low.min(matches[0]);
            high = high.max(matches[matches.len() - 1]);
        }
        const BIT_TEST_FOOTPRINT: u32 = 64;
        if high - low < BIT_TEST_FOOTPRINT {
            return;
        }

        let mut table = Box::new(ByteTable::new());
        for &m in matches {
            table.data[m as usize] = 1;
        }
        for range in ranges {
            for ch in range.begin..=range.end {
                table.data[ch as usize] = 1;
            }
        }
        character_class.latin1_table = Some(table);
    }

    /// `addSorted(char32_t ch)`.
    fn add_sorted(&mut self, ch: u32) {
        self.add_sorted_in(is_latin1(ch), ch);
    }

    /// `addSorted(m_matches8 ou m_matches32, ch)`, conforme `latin1`.
    fn add_sorted_in(&mut self, latin1: bool, ch: u32) {
        let matches = if latin1 { &mut self.matches8 } else { &mut self.matches32 };
        add_sorted_to(matches, &mut self.ranges8, &mut self.ranges32, &mut self.character_widths, ch);
    }

    /// `addSortedRange(m_ranges8 ou m_ranges32, lo, hi)`, conforme `latin1`.
    fn add_sorted_range_in(&mut self, latin1: bool, lo: u32, hi: u32) {
        let ranges = if latin1 { &mut self.ranges8 } else { &mut self.ranges32 };
        add_sorted_range_to(ranges, &mut self.character_widths, lo, hi);
    }

    /// `addSortedRange(char32_t lo, char32_t hi)`.
    fn add_sorted_range(&mut self, mut lo: u32, hi: u32) {
        if lo == hi {
            self.add_sorted(lo);
            return;
        }

        if is_latin1(lo) {
            let latin1_hi = hi.min(0xff);
            if lo == latin1_hi {
                self.add_sorted_in(true, lo);
            } else {
                self.add_sorted_range_in(true, lo, latin1_hi);
            }

            if is_latin1(hi) {
                return;
            }
            lo = 0x100;
            if lo == hi {
                self.add_sorted_in(false, hi);
                return;
            }
        }
        self.add_sorted_range_in(false, lo, hi);
    }

    pub fn union_strings(&mut self, rhs_strings: &[Vec<u32>]) {
        // result should include strings in either the LHS or RHS
        self.merge_strings(rhs_strings, true, true, true);
    }

    pub fn intersection_strings(&mut self, rhs_strings: &[Vec<u32>]) {
        // result should include strings that are in both the LHS and RHS.
        self.merge_strings(rhs_strings, false, false, true);
    }

    pub fn subtraction_strings(&mut self, rhs_strings: &[Vec<u32>]) {
        // result should include strings in LHS that are not in RHS.
        self.merge_strings(rhs_strings, true, false, false);
    }

    /// O laço de fusão comum de `unionStrings`, `intersectionStrings` e `subtractionStrings`: as duas
    /// listas estão ordenadas por `compareUTF32Strings`; `keep_lhs_only`, `keep_rhs_only` e
    /// `keep_both` dizem o que fica de cada região (só na esquerda, só na direita, nas duas). Sobras
    /// de uma lista esgotada seguem a mesma regra da região a que pertencem.
    fn merge_strings(&mut self, rhs_strings: &[Vec<u32>], keep_lhs_only: bool, keep_rhs_only: bool, keep_both: bool) {
        let mut result: Vec<Vec<u32>> = Vec::new();
        let mut lhs_index: usize = 0;
        let mut rhs_index: usize = 0;

        while lhs_index < self.strings.len() && rhs_index < rhs_strings.len() {
            let lhs_string = &self.strings[lhs_index];
            let rhs_string = &rhs_strings[rhs_index];

            let str_compare = compare_utf32_strings(lhs_string, rhs_string);
            if str_compare == 0 {
                if keep_both {
                    result.push(lhs_string.clone());
                }
                lhs_index += 1;
                rhs_index += 1;
            } else if str_compare < 0 {
                if keep_lhs_only {
                    result.push(lhs_string.clone());
                }
                lhs_index += 1;
            } else {
                if keep_rhs_only {
                    result.push(rhs_string.clone());
                }
                rhs_index += 1;
            }
        }

        // One of LHS or RHS has been exhausted, add the remaining strings.
        if keep_lhs_only {
            while lhs_index < self.strings.len() {
                result.push(self.strings[lhs_index].clone());
                lhs_index += 1;
            }
        }

        if keep_rhs_only {
            while rhs_index < rhs_strings.len() {
                result.push(rhs_strings[rhs_index].clone());
                rhs_index += 1;
            }
        }

        self.strings = result;
        self.may_contain_strings = !self.strings.is_empty();
    }
}

include!("yarr_pattern_cpp2.rs");
include!("yarr_pattern_cpp3.rs");
include!("yarr_pattern_cpp4.rs");
include!("yarr_pattern_cpp5.rs");
include!("yarr_pattern_cpp6.rs");
