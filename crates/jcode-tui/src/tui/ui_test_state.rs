use super::*;

pub(super) fn clear() {
    *body_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = BodyCacheState::default();
    *full_prep_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = FullPrepCacheState::default();
    set_last_max_scroll(0);
    set_pinned_pane_total_lines(0);
    set_last_diff_pane_effective_scroll(0);
    set_last_diff_pane_max_scroll(0);
    set_last_total_wrapped_lines(0);
    set_last_resolved_chat_scroll(0);
    TEST_TAIL_FOLLOW_SNAP_PENDING.with(|cell| cell.set(false));
    update_user_prompt_positions(&[]);
    // Flicker events recorded by sibling tests add a "⚠ flicker detected"
    // notification line to subsequent renders, shifting every layout-sensitive
    // assertion (click mapping, snapshot rows).
    frame_metrics::clear_flicker_frame_history_for_tests();
    TEST_LAST_LAYOUT.with(|snapshot| {
        *snapshot.borrow_mut() = None;
    });
    TEST_LAST_STATUS_AREA.with(|snapshot| {
        *snapshot.borrow_mut() = None;
    });
    set_visible_copy_targets(Vec::new());
    clear_copy_viewport_snapshot();

    TEST_PROMPT_VIEWPORT_STATE.with(|state| {
        *state.borrow_mut() = PromptViewportState::default();
    });
}
