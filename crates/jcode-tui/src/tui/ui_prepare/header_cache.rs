use super::*;

/// Header data changes rarely, so cache it independently of the render loop.
pub(super) const HEADER_PREP_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

struct HeaderPrepCacheState {
    signature: u64,
    built_at: Instant,
    prepared: Arc<PreparedMessages>,
}

fn header_prep_cache() -> &'static std::sync::Mutex<Option<HeaderPrepCacheState>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<HeaderPrepCacheState>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Drop the prepared-header cache so the next frame re-probes the disk-backed
/// surfaces (auth inventory, skills, goal badge, update badges).
pub(crate) fn invalidate_header_prep_cache() {
    if let Ok(mut cache) = header_prep_cache().lock() {
        *cache = None;
    }
}

/// Hash of the header inputs that are cheap to read every frame. Anything
/// expensive (disk probes) is intentionally excluded and covered by the TTL.
pub(super) fn header_prep_signature(app: &dyn TuiState, width: u16) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    width.hash(&mut hasher);
    app.provider_model().hash(&mut hasher);
    app.provider_name().hash(&mut hasher);
    app.session_display_name().hash(&mut hasher);
    app.server_display_name().hash(&mut hasher);
    app.server_display_version().hash(&mut hasher);
    app.server_display_icon().hash(&mut hasher);
    app.connection_type().hash(&mut hasher);
    app.upstream_provider().hash(&mut hasher);
    app.is_replay().hash(&mut hasher);
    app.is_remote_mode().hash(&mut hasher);
    app.is_canary().hash(&mut hasher);
    app.server_update_available().hash(&mut hasher);
    app.mcp_servers().hash(&mut hasher);
    app.connected_clients().hash(&mut hasher);
    app.server_sessions().len().hash(&mut hasher);
    app.working_dir().hash(&mut hasher);
    crate::auth::auth_status_generation().hash(&mut hasher);
    if let Some(page) = app.side_panel().focused_page() {
        page.id.hash(&mut hasher);
        page.title.hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn prepare_header_cached(app: &dyn TuiState, width: u16) -> Arc<PreparedMessages> {
    let build = || {
        let (mut all_header_lines, secondary_lines) = header::build_header_sections(app, width);
        all_header_lines.extend(secondary_lines);
        Arc::new(wrap_lines(all_header_lines, &[], &[], &[], width))
    };

    if cfg!(test) {
        return build();
    }

    let signature = header_prep_signature(app, width);
    if let Ok(cache) = header_prep_cache().lock()
        && let Some(state) = cache.as_ref()
        && state.signature == signature
        && state.built_at.elapsed() < HEADER_PREP_CACHE_TTL
    {
        return state.prepared.clone();
    }

    let prepared = build();
    if let Ok(mut cache) = header_prep_cache().lock() {
        *cache = Some(HeaderPrepCacheState {
            signature,
            built_at: Instant::now(),
            prepared: prepared.clone(),
        });
    }
    prepared
}
