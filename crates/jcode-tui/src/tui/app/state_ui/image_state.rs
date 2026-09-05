impl App {
    /// Drop the cached inline-image signature so the next prepared frame
    /// recomputes it. Needed when the image set changes without a
    /// display-messages mutation (e.g. a live SidePaneImages event).
    pub(super) fn invalidate_side_pane_images_signature(&mut self) {
        self.side_pane_images_signature_cache.set(None);
    }
    /// Drop rendered inline images and every cache keyed by their contents.
    /// Use this when the entire transcript is discarded.
    pub(crate) fn clear_inline_image_state(&mut self) {
        self.remote_side_pane_images.clear();
        self.invalidate_side_pane_images_signature();
        self.expanded_images.clear();
        self.expanded_images_version = self.expanded_images_version.wrapping_add(1);
    }
}
