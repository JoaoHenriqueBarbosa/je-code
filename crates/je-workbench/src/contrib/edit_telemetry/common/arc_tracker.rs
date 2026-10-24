//! Porte de `src/vs/workbench/contrib/editTelemetry/common/arcTracker.ts`.

use je_base::edit_arc_tracker::{EditArcTracker, IArcTextEdit, IArcTextReplacement, IEditArcLineCountInfo, IEditArcValues};
use je_editor::edits::edit::{BaseEdit, BaseReplacement};
use je_editor::edits::string_edit::{BaseStringEdit, BaseStringReplacement};
use je_editor::text::abstract_text::AbstractText;

/// The ARC (accepted and retained characters) counts how many characters inserted by the initial suggestion (trackedEdit)
/// stay unmodified after a certain amount of time after acceptance.
pub struct ArcTracker {
    tracker: EditArcTracker,
}

impl ArcTracker {
    pub fn new<E: BaseStringEdit>(value_before_tracked_edit: &dyn AbstractText, tracked_edit: &E) -> Self
    where
        E::Replacement: BaseStringReplacement,
    {
        Self { tracker: EditArcTracker::new(&value_before_tracked_edit.get_value(), &to_arc_text_edit(tracked_edit)) }
    }

    pub fn get_original_character_count(&self) -> usize {
        self.tracker.get_original_character_count()
    }

    /// edit must apply to _updatedTrackedEdit.apply(_valueBeforeTrackedEdit)
    pub fn handle_edits<E: BaseStringEdit>(&mut self, edit: &E)
    where
        E::Replacement: BaseStringReplacement,
    {
        self.tracker.handle_edits(&to_arc_text_edit(edit));
    }

    pub fn get_accepted_restrained_characters_count(&self) -> usize {
        self.tracker.get_accepted_restrained_characters_count()
    }

    pub fn get_line_count_info(&self) -> IEditArcLineCountInfo {
        self.tracker.get_line_count_info()
    }

    /// `getValues()` (`unknown` no original).
    pub fn get_values(&self) -> IEditArcValues {
        self.tracker.get_values()
    }
}

fn to_arc_text_edit<E: BaseStringEdit>(edit: &E) -> IArcTextEdit
where
    E::Replacement: BaseStringReplacement,
{
    IArcTextEdit {
        replacements: edit
            .replacements()
            .iter()
            .map(|replacement| {
                let range = replacement.replace_range();
                IArcTextReplacement { start: range.start as usize, end_exclusive: range.end_exclusive as usize, text: replacement.new_text().clone() }
            })
            .collect(),
    }
}
