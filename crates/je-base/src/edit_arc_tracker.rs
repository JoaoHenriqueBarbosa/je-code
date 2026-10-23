//! Porte de `src/vs/base/common/editArcTracker.ts`: quantos caracteres de uma edição inicial sobrevivem às
//! edições seguintes (o "ARC", accepted and retained characters).
//!
//! Os deslocamentos são em unidades UTF-16, como no original (`text.length`, `substring`). Os erros
//! (`BugIndicatingError`) são lançados com [`errors::throw`].

use crate::errors::{self, Error};
use crate::js_string::{JsString, substring};
use crate::strings::{common_prefix_length, common_suffix_length, split_lines};

/// `IArcTextReplacement`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IArcTextReplacement {
    pub start: usize,
    pub end_exclusive: usize,
    pub text: JsString,
}

/// `IArcTextEdit`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IArcTextEdit {
    pub replacements: Vec<IArcTextReplacement>,
}

/// `IEditArcLineCountInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IEditArcLineCountInfo {
    pub deleted_line_counts: usize,
    pub inserted_line_counts: usize,
}

/// `IEditArcValues`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IEditArcValues {
    pub arc: usize,
    pub deleted_line_counts: usize,
    pub inserted_line_counts: usize,
}

#[derive(Clone, Debug)]
struct Tracked {
    start: usize,
    end_exclusive: usize,
    text: Vec<u16>,
    is_tracked: bool,
}

impl Tracked {
    fn delta(&self) -> i64 {
        self.text.len() as i64 - (self.end_exclusive - self.start) as i64
    }
}

fn bug(message: &str) -> ! {
    errors::throw(Error::bug_indicating(Some(message)))
}

/// `EditArcTracker`: tracks how many characters from an initial edit survive later edits.
pub struct EditArcTracker {
    value_before_tracked_edit: Vec<u16>,
    tracked_edit: Vec<Tracked>,
    updated_tracked_edit: Vec<Tracked>,
}

impl EditArcTracker {
    pub fn new(value_before_tracked_edit: &JsString, tracked_edit: &IArcTextEdit) -> Self {
        let value = value_before_tracked_edit.units().to_vec();
        let tracked_edit = normalize_replacements(remove_common_text(&tracked_edit.replacements, &value, true));
        Self { value_before_tracked_edit: value, updated_tracked_edit: tracked_edit.clone(), tracked_edit }
    }

    pub fn get_original_character_count(&self) -> usize {
        self.tracked_edit.iter().map(|r| r.text.len()).sum()
    }

    pub fn handle_edits(&mut self, edit: &IArcTextEdit) {
        let untracked_edit = normalize_replacements(
            edit.replacements.iter().map(|r| Tracked { start: r.start, end_exclusive: r.end_exclusive, text: r.text.units().to_vec(), is_tracked: false }).collect(),
        );
        self.updated_tracked_edit = compose(&self.updated_tracked_edit, &untracked_edit);
    }

    pub fn get_accepted_restrained_characters_count(&self) -> usize {
        self.updated_tracked_edit.iter().filter(|r| r.is_tracked).map(|r| r.text.len()).sum()
    }

    pub fn get_line_count_info(&self) -> IEditArcLineCountInfo {
        let tracked: Vec<Tracked> = self.updated_tracked_edit.iter().filter(|r| r.is_tracked).cloned().collect();
        get_line_count_info(&tracked, &self.value_before_tracked_edit)
    }

    pub fn get_values(&self) -> IEditArcValues {
        let lines = self.get_line_count_info();
        IEditArcValues { arc: self.get_accepted_restrained_characters_count(), deleted_line_counts: lines.deleted_line_counts, inserted_line_counts: lines.inserted_line_counts }
    }
}

fn remove_common_text(replacements: &[IArcTextReplacement], original_text: &[u16], is_tracked: bool) -> Vec<Tracked> {
    replacements
        .iter()
        .map(|replacement| {
            let old_text = substring(original_text, replacement.start as i64, replacement.end_exclusive as i64);
            let new_text = replacement.text.units();
            let prefix_length = common_prefix_length(old_text, new_text);
            let suffix_length = (old_text.len() - prefix_length).min(new_text.len() - prefix_length).min(common_suffix_length(old_text, new_text));
            Tracked {
                start: replacement.start + prefix_length,
                end_exclusive: replacement.end_exclusive - suffix_length,
                text: new_text[prefix_length..new_text.len() - suffix_length].to_vec(),
                is_tracked,
            }
        })
        .collect()
}

fn normalize_replacements(replacements: Vec<Tracked>) -> Vec<Tracked> {
    let mut result: Vec<Tracked> = Vec::new();
    let mut previous: Option<Tracked> = None;

    for replacement in replacements {
        if replacement.start > replacement.end_exclusive {
            bug("Edit replacement start must not be after its end");
        }
        if previous.as_ref().is_some_and(|p| replacement.start < p.end_exclusive) {
            bug("Edit replacements must be sorted and disjoint");
        }
        if replacement.start == replacement.end_exclusive && replacement.text.is_empty() {
            continue;
        }
        if let Some(prev) = &previous
            && prev.end_exclusive == replacement.start
            && prev.is_tracked == replacement.is_tracked
        {
            let mut text = prev.text.clone();
            text.extend_from_slice(&replacement.text);
            previous = Some(Tracked { start: prev.start, end_exclusive: replacement.end_exclusive, text, is_tracked: prev.is_tracked });
            continue;
        }
        if let Some(prev) = previous.take() {
            result.push(prev);
        }
        previous = Some(replacement);
    }

    if let Some(prev) = previous {
        result.push(prev);
    }
    result
}

/// `start + offset` com o deslocamento possivelmente negativo (o original soma números sem sinal).
fn shifted(start: usize, offset: i64) -> i64 {
    start as i64 + offset
}

fn compose(first: &[Tracked], second: &[Tracked]) -> Vec<Tracked> {
    if first.is_empty() {
        return second.to_vec();
    }
    if second.is_empty() {
        return first.to_vec();
    }

    let mut first_queue: std::collections::VecDeque<Tracked> = first.iter().cloned().collect();
    let mut result: Vec<Tracked> = Vec::new();
    let mut first_to_second_offset: i64 = 0;

    for second_replacement in second {
        while let Some(first_replacement) = first_queue.front() {
            if shifted(first_replacement.start, first_to_second_offset) + first_replacement.text.len() as i64 >= second_replacement.start as i64 {
                break;
            }
            let first_replacement = first_queue.pop_front().expect("conferido no laço");
            first_to_second_offset += first_replacement.delta();
            result.push(first_replacement);
        }

        let offset_before_intersections = first_to_second_offset;
        let mut first_intersecting: Option<Tracked> = None;
        let mut last_intersecting: Option<Tracked> = None;

        while let Some(first_replacement) = first_queue.front() {
            if shifted(first_replacement.start, first_to_second_offset) > second_replacement.end_exclusive as i64 {
                break;
            }
            let first_replacement = first_queue.pop_front().expect("conferido no laço");
            if first_intersecting.is_none() {
                first_intersecting = Some(first_replacement.clone());
            }
            first_to_second_offset += first_replacement.delta();
            last_intersecting = Some(first_replacement);
        }

        let Some(first_intersecting) = first_intersecting else {
            result.push(delta_replacement(second_replacement, -first_to_second_offset));
            continue;
        };

        let replace_start = (first_intersecting.start as i64).min(second_replacement.start as i64 - offset_before_intersections);
        let prefix_length = second_replacement.start as i64 - (first_intersecting.start as i64 + offset_before_intersections);
        if prefix_length > 0 {
            result.push(slice_replacement(&first_intersecting, replace_start, replace_start, 0, prefix_length as usize));
        }

        let Some(last_intersecting) = last_intersecting else {
            bug("Missing intersecting ARC edit");
        };
        let suffix_length = last_intersecting.end_exclusive as i64 + first_to_second_offset - second_replacement.end_exclusive as i64;
        if suffix_length > 0 {
            let suffix = slice_replacement(
                &last_intersecting,
                last_intersecting.end_exclusive as i64,
                last_intersecting.end_exclusive as i64,
                last_intersecting.text.len() - suffix_length as usize,
                last_intersecting.text.len(),
            );
            first_to_second_offset -= suffix.delta();
            first_queue.push_front(suffix);
        }

        result.push(slice_replacement(second_replacement, replace_start, second_replacement.end_exclusive as i64 - first_to_second_offset, 0, second_replacement.text.len()));
    }

    result.extend(first_queue);
    normalize_replacements(result)
}

fn delta_replacement(replacement: &Tracked, offset: i64) -> Tracked {
    Tracked { start: (replacement.start as i64 + offset) as usize, end_exclusive: (replacement.end_exclusive as i64 + offset) as usize, ..replacement.clone() }
}

fn slice_replacement(replacement: &Tracked, start: i64, end_exclusive: i64, text_start: usize, text_end_exclusive: usize) -> Tracked {
    Tracked { start: start as usize, end_exclusive: end_exclusive as usize, text: substring(&replacement.text, text_start as i64, text_end_exclusive as i64).to_vec(), is_tracked: replacement.is_tracked }
}

#[derive(Clone, Copy)]
struct Position {
    line_number: usize,
    column: usize,
}

struct TextReplacement {
    start: Position,
    end: Position,
    start_offset: usize,
    end_offset: usize,
    text: Vec<u16>,
}

fn get_line_count_info(replacements: &[Tracked], initial_text: &[u16]) -> IEditArcLineCountInfo {
    if replacements.is_empty() {
        return IEditArcLineCountInfo { deleted_line_counts: 0, inserted_line_counts: 0 };
    }

    let line_offsets = LineOffsets::new(initial_text);
    let text_replacements: Vec<TextReplacement> = replacements
        .iter()
        .map(|r| TextReplacement { start: line_offsets.get_position(r.start), end: line_offsets.get_position(r.end_exclusive), start_offset: r.start, end_offset: r.end_exclusive, text: r.text.clone() })
        .collect();

    let mut deleted_line_counts = 0usize;
    let mut inserted_line_counts = 0usize;
    let mut current: Vec<&TextReplacement> = Vec::new();
    for (i, replacement) in text_replacements.iter().enumerate() {
        let next = text_replacements.get(i + 1);
        current.push(replacement);
        if next.is_some_and(|n| n.start.line_number == replacement.end.line_number) {
            continue;
        }

        let joined = join_text_replacements(&current, initial_text);
        current.clear();
        let mut new_lines: Vec<Vec<u16>> = split_lines(&joined.text).into_iter().map(<[u16]>::to_vec).collect();
        let mut start_line_number = joined.start.line_number;
        let mut end_line_number_exclusive = joined.end.line_number + 1;

        let first_line_prefix = substring(initial_text, line_offsets.get_line_start(joined.start.line_number) as i64, joined.start_offset as i64).to_vec();
        let mut first = first_line_prefix.clone();
        first.extend_from_slice(&new_lines[0]);
        new_lines[0] = first;
        let last_line_suffix = substring(initial_text, joined.end_offset as i64, line_offsets.get_line_end(joined.end.line_number) as i64).to_vec();
        new_lines.last_mut().expect("split_lines devolve ao menos uma linha").extend_from_slice(&last_line_suffix);

        let starts_at_line_end = joined.start.column == line_offsets.get_line_length(joined.start.line_number) + 1;
        let ends_at_line_start = joined.end.column == 1;
        if starts_at_line_end && new_lines[0].len() == first_line_prefix.len() {
            start_line_number += 1;
            new_lines.remove(0);
        }
        if !new_lines.is_empty() && start_line_number < end_line_number_exclusive && ends_at_line_start && new_lines[new_lines.len() - 1].len() == last_line_suffix.len() {
            end_line_number_exclusive -= 1;
            new_lines.pop();
        }

        deleted_line_counts += end_line_number_exclusive - start_line_number;
        inserted_line_counts += new_lines.len();
    }

    IEditArcLineCountInfo { deleted_line_counts, inserted_line_counts }
}

fn join_text_replacements(replacements: &[&TextReplacement], initial_text: &[u16]) -> TextReplacement {
    let first = replacements[0];
    let last = replacements[replacements.len() - 1];
    let mut text = first.text.clone();
    for i in 1..replacements.len() {
        text.extend_from_slice(substring(initial_text, replacements[i - 1].end_offset as i64, replacements[i].start_offset as i64));
        text.extend_from_slice(&replacements[i].text);
    }
    TextReplacement { start: first.start, end: last.end, start_offset: first.start_offset, end_offset: last.end_offset, text }
}

struct LineOffsets {
    line_starts: Vec<usize>,
    line_ends: Vec<usize>,
}

impl LineOffsets {
    fn new(text: &[u16]) -> Self {
        let mut line_starts = vec![0];
        let mut line_ends = Vec::new();
        for (i, &unit) in text.iter().enumerate() {
            if unit == 10 {
                line_starts.push(i + 1);
                line_ends.push(if i > 0 && text[i - 1] == 13 { i - 1 } else { i });
            }
        }
        line_ends.push(text.len());
        Self { line_starts, line_ends }
    }

    fn get_position(&self, offset: usize) -> Position {
        let (mut low, mut high) = (0usize, self.line_starts.len() - 1);
        while low < high {
            let middle = (low + high).div_ceil(2);
            if self.line_starts[middle] <= offset {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        Position { line_number: low + 1, column: offset - self.line_starts[low] + 1 }
    }

    fn get_line_start(&self, line_number: usize) -> usize {
        self.line_starts[line_number - 1]
    }

    fn get_line_end(&self, line_number: usize) -> usize {
        self.line_ends[line_number - 1]
    }

    fn get_line_length(&self, line_number: usize) -> usize {
        self.get_line_end(line_number) - self.get_line_start(line_number)
    }
}
