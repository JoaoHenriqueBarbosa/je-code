//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/compressedVirtualizedScrollLayout.ts`: o layout, em
//! coordenadas completas e comprimidas, de uma lista virtualizada de itens com altura própria.
//!
//! Decisões de tradução:
//! - Os números do original são `number`; o `OffsetRange` do porte é de `i32`, e por isso as alturas, o espaçamento, as
//!   folgas e as posições de rolagem do layout também são `i32` (pixels inteiros).
//! - `LayoutRevision` (`number & { _brand }`) é o newtype [`LayoutRevision`], criado por [`as_layout_revision`].
//! - Os parâmetros opcionais de `ICompressedVirtualizedScrollLayoutInput` são `Option`.
//! - O `BugIndicatingError` lançado é o `throw` de [`errors::throw`] com [`Error::bug_indicating`].

use je_base::errors::{self, Error};

use crate::offset_range::OffsetRange;

/// `CompressedVirtualizedItemVisibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressedVirtualizedItemVisibility {
    Before,
    Visible,
    After,
}

/// `LayoutRevision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayoutRevision(pub i32);

impl std::fmt::Display for LayoutRevision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `ICompressedVirtualizedItemLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICompressedVirtualizedItemLayout {
    /// Complete item range in outer scroll coordinates.
    pub content_range: OffsetRange,
    /// Item range in the compressed rendered coordinate system.
    pub rendered_range: OffsetRange,
    /// Height removed from the rendered row and represented by item-local scrolling.
    pub max_scroll_offset: i32,
    /// Item-local scroll coordinates:
    ///
    /// ```text
    /// 0 ---------------------------- maxScrollOffset
    /// item begins                    item end is aligned
    /// ```
    pub scroll_offset: i32,
    pub visibility: CompressedVirtualizedItemVisibility,
}

/// `ICompressedVirtualizedScrollLayout`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICompressedVirtualizedScrollLayout {
    pub revision: LayoutRevision,
    /// Clamped position in the complete outer scroll coordinates.
    pub scroll_top: i32,
    /// Complete height of the logical item content, excluding transient scroll slack.
    pub logical_scroll_height: i32,
    /// Complete height of all items and inter-item gaps.
    pub scroll_height: i32,
    pub leading_scroll_slack: i32,
    pub trailing_scroll_slack: i32,
    /// Height occupied by the compressed item rows and inter-item gaps.
    pub rendered_height: i32,
    pub content_viewport: OffsetRange,
    pub rendered_viewport: OffsetRange,
    /// Full-content height above the viewport represented by item-local scrolling instead of DOM movement.
    pub hidden_content_height_above_viewport: i32,
    pub items: Vec<ICompressedVirtualizedItemLayout>,
}

/// `ICompressedVirtualizedScrollLayoutInput`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ICompressedVirtualizedScrollLayoutInput<'a> {
    pub revision: Option<LayoutRevision>,
    pub scroll_top: i32,
    pub viewport_height: i32,
    pub item_gap: i32,
    pub item_heights: &'a [i32],
    pub leading_scroll_slack: Option<i32>,
    pub trailing_scroll_slack: Option<i32>,
}

/// `ILogicalPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ILogicalPosition {
    pub revision: LayoutRevision,
    pub offset: i32,
}

/// `ISizeEdit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ISizeEdit {
    pub old_range: OffsetRange,
    pub new_range: OffsetRange,
}

/// `IAnchoredSizeEditBatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAnchoredSizeEditBatch {
    pub from_revision: LayoutRevision,
    pub to_revision: LayoutRevision,
    pub edits: Vec<ISizeEdit>,
    pub anchor: ILogicalPosition,
}

/// `asLayoutRevision(value)`.
pub fn as_layout_revision(value: i32) -> LayoutRevision {
    LayoutRevision(value)
}

fn throw_bug(message: &str) -> ! {
    errors::throw(Error::bug_indicating(Some(message)))
}

/// Computes the complete and compressed vertical layout of a virtualized list.
pub fn compute_compressed_virtualized_scroll_layout(input: &ICompressedVirtualizedScrollLayoutInput) -> ICompressedVirtualizedScrollLayout {
    assert_non_negative("viewportHeight", input.viewport_height);
    let leading_scroll_slack = input.leading_scroll_slack.unwrap_or(0);
    let trailing_scroll_slack = input.trailing_scroll_slack.unwrap_or(0);
    assert_non_negative("leadingScrollSlack", leading_scroll_slack);
    assert_non_negative("trailingScrollSlack", trailing_scroll_slack);
    let logical_scroll_height = compute_compressed_virtualized_scroll_height(input.item_heights, input.item_gap);
    let scroll_height = leading_scroll_slack + logical_scroll_height + trailing_scroll_slack;
    let max_scroll_top = (scroll_height - input.viewport_height).max(0);
    let scroll_top = input.scroll_top.min(max_scroll_top).max(0);
    let content_viewport = OffsetRange::of_start_and_length(scroll_top - leading_scroll_slack, input.viewport_height);

    let mut content_top = 0;
    let mut rendered_top = leading_scroll_slack;
    let mut hidden_content_height_above_viewport = 0;
    let mut items: Vec<ICompressedVirtualizedItemLayout> = Vec::new();

    for (index, &full_height) in input.item_heights.iter().enumerate() {
        let rendered_height = full_height.min(input.viewport_height);
        let max_scroll_offset = full_height - rendered_height;
        let content_range = OffsetRange::of_start_and_length(content_top, full_height);
        let rendered_range = OffsetRange::of_start_and_length(rendered_top, rendered_height);

        let (visibility, scroll_offset) = if content_range.is_before(content_viewport) {
            (CompressedVirtualizedItemVisibility::Before, max_scroll_offset)
        } else if content_range.is_after(content_viewport) {
            (CompressedVirtualizedItemVisibility::After, 0)
        } else {
            (CompressedVirtualizedItemVisibility::Visible, (content_viewport.start - content_range.start).min(max_scroll_offset).max(0))
        };

        hidden_content_height_above_viewport += scroll_offset;
        items.push(ICompressedVirtualizedItemLayout { content_range, rendered_range, max_scroll_offset, scroll_offset, visibility });

        if index < input.item_heights.len() - 1 {
            content_top += full_height + input.item_gap;
            rendered_top += rendered_height + input.item_gap;
        } else {
            content_top += full_height;
            rendered_top += rendered_height;
        }
    }

    let rendered_scroll_top = scroll_top - hidden_content_height_above_viewport;
    ICompressedVirtualizedScrollLayout {
        revision: input.revision.unwrap_or_else(|| as_layout_revision(0)),
        scroll_top,
        logical_scroll_height,
        scroll_height,
        leading_scroll_slack,
        trailing_scroll_slack,
        rendered_height: rendered_top + trailing_scroll_slack,
        content_viewport,
        rendered_viewport: OffsetRange::of_start_and_length(rendered_scroll_top, input.viewport_height),
        hidden_content_height_above_viewport,
        items,
    }
}

/// `computeItemRanges(itemHeights, itemGap)`.
pub fn compute_item_ranges(item_heights: &[i32], item_gap: i32) -> Vec<OffsetRange> {
    assert_non_negative("itemGap", item_gap);
    let mut ranges: Vec<OffsetRange> = Vec::new();
    let mut offset = 0;
    for (index, &height) in item_heights.iter().enumerate() {
        assert_non_negative(&format!("itemHeights[{index}]"), height);
        ranges.push(OffsetRange::of_start_and_length(offset, height));
        offset += height + if index < item_heights.len() - 1 { item_gap } else { 0 };
    }
    ranges
}

/// `createAnchoredSizeEditBatch(...)`.
pub fn create_anchored_size_edit_batch(
    from_revision: LayoutRevision,
    to_revision: LayoutRevision,
    old_item_heights: &[i32],
    new_item_heights: &[i32],
    old_item_gap: i32,
    new_item_gap: i32,
    anchor_offset: i32,
) -> IAnchoredSizeEditBatch {
    if old_item_heights.len() != new_item_heights.len() {
        throw_bug("Size edits require stable item identity and ordering");
    }
    let old_ranges = compute_item_ranges(old_item_heights, old_item_gap);
    let new_ranges = compute_item_ranges(new_item_heights, new_item_gap);
    let mut edits: Vec<ISizeEdit> = Vec::new();
    for (old_range, new_range) in old_ranges.iter().zip(&new_ranges) {
        if !old_range.equals(*new_range) {
            edits.push(ISizeEdit { old_range: *old_range, new_range: *new_range });
        }
    }
    IAnchoredSizeEditBatch { from_revision, to_revision, edits, anchor: ILogicalPosition { revision: from_revision, offset: anchor_offset } }
}

/// `mapLogicalPosition(position, edit)`.
pub fn map_logical_position(position: ILogicalPosition, edit: &IAnchoredSizeEditBatch) -> ILogicalPosition {
    if position.revision != edit.from_revision {
        throw_bug(&format!("Cannot map layout revision {} through revision {}", position.revision, edit.from_revision));
    }
    let mut mapped_offset = position.offset;
    for size_edit in &edit.edits {
        if position.offset < size_edit.old_range.start {
            break;
        }
        if size_edit.old_range.contains(position.offset) {
            let relative_offset = position.offset - size_edit.old_range.start;
            mapped_offset = if relative_offset < size_edit.new_range.length() { size_edit.new_range.start + relative_offset } else { size_edit.new_range.start };
            return ILogicalPosition { revision: edit.to_revision, offset: mapped_offset };
        }
        mapped_offset = position.offset + size_edit.new_range.end_exclusive - size_edit.old_range.end_exclusive;
    }
    ILogicalPosition { revision: edit.to_revision, offset: mapped_offset }
}

/// `computeCompressedVirtualizedScrollHeight(itemHeights, itemGap)`.
pub fn compute_compressed_virtualized_scroll_height(item_heights: &[i32], item_gap: i32) -> i32 {
    assert_non_negative("itemGap", item_gap);
    for (i, &height) in item_heights.iter().enumerate() {
        assert_non_negative(&format!("itemHeights[{i}]"), height);
    }
    if item_heights.is_empty() {
        return 0;
    }
    item_heights.iter().sum::<i32>() + item_gap * (item_heights.len() as i32 - 1)
}

fn assert_non_negative(name: &str, value: i32) {
    if value < 0 {
        throw_bug(&format!("{name} must be a finite non-negative number, got {value}"));
    }
}
