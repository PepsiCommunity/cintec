//! Index operations shared by normal and visual queue edits.
use std::ops::Range;

pub fn span(anchor: Option<usize>, cursor: usize, len: usize) -> Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let cursor = cursor.min(len - 1);
    let anchor = anchor.unwrap_or(cursor).min(len - 1);
    anchor.min(cursor)..anchor.max(cursor) + 1
}
/// Move a contiguous selection one row, preserving its internal order.
pub fn move_block<T>(items: &mut [T], range: Range<usize>, down: bool) -> Range<usize> {
    if range.is_empty() {
        return range;
    }
    if down && range.end < items.len() {
        items[range.start..range.end + 1].rotate_right(1);
        range.start + 1..range.end + 1
    } else if !down && range.start > 0 {
        items[range.start - 1..range.end].rotate_left(1);
        range.start - 1..range.end - 1
    } else {
        range
    }
}
pub struct Removal {
    pub current: Option<usize>,
    pub removed_current: bool,
    pub cursor: usize,
}
pub fn remove_block<T>(items: &mut Vec<T>, range: Range<usize>, current: Option<usize>) -> Removal {
    let removed_current = current.is_some_and(|i| range.contains(&i));
    let count = range.len();
    items.drain(range.clone());
    let cursor = range.start.min(items.len().saturating_sub(1));
    let current = current.and_then(|i| {
        if items.is_empty() {
            None
        } else if removed_current {
            Some(cursor)
        } else if i >= range.end {
            Some(i - count)
        } else {
            Some(i)
        }
    });
    Removal {
        current,
        removed_current,
        cursor,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_ranges_work_in_both_directions_and_on_empty_lists() {
        assert_eq!(span(Some(4), 1, 6), 1..5);
        assert_eq!(span(Some(1), 4, 6), 1..5);
        assert_eq!(span(None, 4, 6), 4..5);
        assert_eq!(span(Some(4), 9, 3), 2..3);
        assert_eq!(span(Some(4), 9, 0), 0..0);
    }
    #[test]
    fn block_moves_keep_order_and_stop_at_boundaries() {
        let mut items = vec![0, 1, 2, 3, 4];
        let range = move_block(&mut items, 1..4, true);
        assert_eq!(
            (items.as_slice(), range.clone()),
            (&[0, 4, 1, 2, 3][..], 2..5)
        );
        assert_eq!(move_block(&mut items, range.clone(), true), range);
        let range = move_block(&mut items, range, false);
        assert_eq!((items.as_slice(), range), (&[0, 1, 2, 3, 4][..], 1..4));
        assert_eq!(move_block(&mut items, 0..3, false), 0..3);
        assert_eq!(move_block(&mut items, 0..5, true), 0..5);
    }
    #[test]
    fn deleting_before_current_preserves_playing_entry() {
        let mut items = vec![0, 1, 2, 3, 4];
        let edit = remove_block(&mut items, 1..3, Some(4));
        assert_eq!(items, vec![0, 3, 4]);
        assert_eq!(edit.current, Some(2));
        assert!(!edit.removed_current);
    }
    #[test]
    fn deleting_current_selects_successor_then_previous_at_end() {
        let mut items = vec![0, 1, 2, 3, 4];
        let edit = remove_block(&mut items, 1..3, Some(2));
        assert!(edit.removed_current);
        assert_eq!(edit.current, Some(1));
        assert_eq!(items[1], 3);
        let edit = remove_block(&mut items, 1..3, Some(1));
        assert_eq!(edit.current, Some(0));
        assert_eq!(items, vec![0]);
        let edit = remove_block(&mut items, 0..1, Some(0));
        assert_eq!(edit.current, None);
        assert_eq!(edit.cursor, 0);
    }
}

#[derive(Debug, PartialEq)]
pub enum AfterEnd {
    Stop,
    Play(usize),
    RepeatCurrent,
}
/// A missing queue index means the sounding track was removed from the queue.
pub fn after_end(current: Option<usize>, len: usize, repeat: &str) -> AfterEnd {
    if repeat == "Track" {
        return AfterEnd::RepeatCurrent;
    }
    if let Some(next) = current.and_then(|i| i.checked_add(1)).filter(|i| *i < len) {
        return AfterEnd::Play(next);
    }
    if repeat == "Playlist" && len > 0 {
        AfterEnd::Play(0)
    } else {
        AfterEnd::Stop
    }
}
#[cfg(test)]
mod eof_tests {
    use super::*;
    #[test]
    fn removed_current_obeys_repeat_and_empty_queue() {
        assert_eq!(after_end(None, 3, "None"), AfterEnd::Stop);
        assert_eq!(after_end(None, 3, "Playlist"), AfterEnd::Play(0));
        assert_eq!(after_end(None, 0, "Playlist"), AfterEnd::Stop);
        assert_eq!(after_end(None, 0, "Track"), AfterEnd::RepeatCurrent);
        assert_eq!(after_end(Some(0), 3, "None"), AfterEnd::Play(1));
        assert_eq!(after_end(Some(2), 3, "None"), AfterEnd::Stop);
    }
}

/// Detach a sounding entry without replacing it with the next queued entry.
pub fn remove_preserving_current<T: Clone>(
    items: &mut Vec<T>,
    range: Range<usize>,
    current: Option<usize>,
    loaded: bool,
) -> (Removal, Option<T>) {
    let detached = current
        .filter(|i| loaded && range.contains(i))
        .and_then(|i| items.get(i))
        .cloned();
    let mut edit = remove_block(items, range, current);
    if detached.is_some() {
        edit.current = None;
    }
    (edit, detached)
}
#[cfg(test)]
mod detach_tests {
    use super::*;
    #[test]
    fn deleting_sounding_entry_or_whole_queue_retains_its_identity() {
        let mut q = vec![(1, "same.flac"), (2, "same.flac"), (3, "other.flac")];
        let (edit, active) = remove_preserving_current(&mut q, 0..2, Some(1), true);
        assert_eq!(active, Some((2, "same.flac")));
        assert_eq!(edit.current, None);
        assert_eq!(q, vec![(3, "other.flac")]);
        let (edit, active) = remove_preserving_current(&mut q, 0..1, Some(0), true);
        assert_eq!(active, Some((3, "other.flac")));
        assert_eq!(edit.current, None);
        assert!(q.is_empty());
    }
    #[test]
    fn deleting_other_entries_and_stopped_entries_does_not_detach() {
        let mut q = vec![0, 1, 2];
        let (edit, active) = remove_preserving_current(&mut q, 0..1, Some(2), true);
        assert_eq!((edit.current, active), (Some(1), None));
        let (edit, active) = remove_preserving_current(&mut q, 0..1, Some(0), false);
        assert_eq!((edit.current, active), (Some(0), None));
    }
}
