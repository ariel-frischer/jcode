use super::*;

/// Get the last known user prompt line positions (from the most recent render frame).
/// Returns positions as wrapped line indices from the top of content.
pub fn last_user_prompt_positions() -> Vec<usize> {
    #[cfg(test)]
    {
        return TEST_LAST_USER_PROMPT_POSITIONS.with(|v| v.borrow().clone());
    }
    #[cfg(not(test))]
    {
        match LAST_USER_PROMPT_POSITIONS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
        {
            Ok(positions) => positions.clone(),
            Err(error) => {
                crate::logging::warn(&format!(
                    "Failed to read user prompt positions from poisoned lock: {error}"
                ));
                Vec::new()
            }
        }
    }
}

pub(super) fn update_user_prompt_positions(positions: &[usize]) {
    #[cfg(test)]
    {
        TEST_LAST_USER_PROMPT_POSITIONS.with(|v| {
            let mut v = v.borrow_mut();
            v.clear();
            v.extend_from_slice(positions);
        });
        return;
    }
    #[cfg(not(test))]
    {
        let mutex = LAST_USER_PROMPT_POSITIONS.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(mut v) = mutex.lock() {
            v.clear();
            v.extend_from_slice(positions);
        }
    }
}

pub(super) fn hash_text_for_cache(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    std::hash::Hasher::finish(&hasher)
}

pub(crate) fn chat_link_target_from_screen(column: u16, row: u16) -> Option<(String, usize)> {
    let point = copy_point_from_screen(column, row)?;
    if point.pane != crate::tui::CopySelectionPane::Chat {
        return None;
    }
    let snapshot = copy_snapshot_for_pane(point.pane)?;
    let target = link_target_from_snapshot(&snapshot, point)?;
    let prepared = match &snapshot.data {
        CopyViewportData::ChatFrame { prepared } => prepared,
        CopyViewportData::Dense { .. } => return None,
    };
    Some((target, prepared.message_index_at_line(point.abs_line)?))
}
