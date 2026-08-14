//! Configuration file support for jcode
//!
//! Config is loaded from `~/.jcode/config.toml` (or `$JCODE_HOME/config.toml`)
//! Environment variables override config file settings.

pub use jcode_config_types::{
    AgentsConfig, AmbientConfig, AuthConfig, AutoJudgeConfig, AutoReviewConfig, CompactionConfig,
    CompactionMode, CrossProviderFailoverMode, DiagramDisplayMode, DiagramPanePosition,
    DiffDisplayMode, DisplayConfig, FeatureConfig, GatewayConfig, HandoffConfig,
    HandoffProfileConfig, HookCommands, HooksConfig, KeybindingsConfig, LatexRenderingMode,
    LaunchHotkeyEntry, LaunchHotkeysConfig, MarkdownSpacingMode, NamedProviderAuth,
    NamedProviderConfig, NamedProviderModelConfig, NamedProviderType, NativeScrollbarConfig,
    NotificationsConfig, OverscrollStatusMode, PowerConfig, ProviderConfig, ReasoningDisplayMode,
    RunSafetyConfig, SafetyConfig, SessionLibrarianConfig, SessionPickerResumeAction,
    SessionProfileConfig, SkillsMode, SponsorsConfig, SwarmRolePolicy, SwarmSpawnMode,
    SwarmStripLayout, TerminalConfig, UpdateChannel, WebSearchConfig, WebSearchEngine,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

const CONFIG_CACHE_CHECK_INTERVAL: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_millis(500)
};

const CONFIG_ENV_KEYS: &[&str] = &[
    "HOME",
    "JCODE_ACP_PROFILE",
    "JCODE_ACP_TOOL_PROFILE",
    "JCODE_ACTIVE_SESSIONS_MANAGER",
    "JCODE_EXTERNAL_SESSIONS",
    "JCODE_AMBIENT_ENABLED",
    "JCODE_AMBIENT_MAX_INTERVAL",
    "JCODE_AMBIENT_MIN_INTERVAL",
    "JCODE_AMBIENT_MODEL",
    "JCODE_AMBIENT_PROACTIVE",
    "JCODE_AMBIENT_PROVIDER",
    "JCODE_AMBIENT_VISIBLE",
    "JCODE_ANIMATION_FPS",
    "JCODE_AUTO_POKE",
    "JCODE_RUN_MAX_TURNS",
    "JCODE_RUN_MAX_TOOL_STEPS",
    "JCODE_RUN_TOKEN_BUDGET",
    "JCODE_RUN_DEADLINE",
    "JCODE_AUTOJUDGE_ENABLED",
    "JCODE_AUTOJUDGE_MODEL",
    "JCODE_AUTOREVIEW_ENABLED",
    "JCODE_AUTOREVIEW_MODEL",
    "JCODE_AUTO_POKE",
    "JCODE_AUTO_SERVER_RELOAD",
    "JCODE_BING_API_KEY",
    "JCODE_BING_API_KEY_ENV",
    "JCODE_BING_MARKET",
    "JCODE_CENTERED_TOGGLE_KEY",
    "JCODE_CHAT_NATIVE_SCROLLBAR",
    "JCODE_COMPACT_NOTIFICATIONS",
    "JCODE_COPY_BADGE_ALT_LABEL",
    "JCODE_COPY_SELECTION_TOGGLE_KEY",
    "JCODE_COPILOT_PREMIUM",
    "JCODE_CROSS_PROVIDER_FAILOVER",
    "JCODE_DEBUG_SOCKET",
    "JCODE_DEFAULT_REASONING_DISPLAY",
    "JCODE_DICTATION_COMMAND",
    "JCODE_DICTATION_KEY",
    "JCODE_DICTATION_MODE",
    "JCODE_DICTATION_TIMEOUT_SECS",
    "JCODE_DIFF_LINE_WRAP",
    "JCODE_DIFF_MODE",
    "JCODE_DIFF_MODE_CYCLE_KEY",
    "JCODE_DIAGRAM_PANE_TOGGLE_KEY",
    "JCODE_DISABLE_BASE_TOOLS",
    "JCODE_DISABLED_ANIMATIONS",
    "JCODE_DISABLED_TOOLS",
    "JCODE_DISCORD_BOT_TOKEN",
    "JCODE_DISCORD_BOT_USER_ID",
    "JCODE_DISCORD_CHANNEL_ID",
    "JCODE_DISCORD_REPLY_ENABLED",
    "JCODE_DISPLAY_CENTERED",
    "JCODE_EFFORT_DECREASE_KEY",
    "JCODE_EFFORT_INCREASE_KEY",
    "JCODE_EMAIL_REPLY_ENABLED",
    "JCODE_EMAIL_TO",
    "JCODE_FOCUS_HOOK",
    "JCODE_GATEWAY_BIND_ADDR",
    "JCODE_GATEWAY_ENABLED",
    "JCODE_GATEWAY_PORT",
    "JCODE_HOME",
    "JCODE_HOOK_PRE_TOOL",
    "JCODE_HOOK_PRE_TOOL_TOOLS",
    "JCODE_HOOK_PRE_TOOL_TIMEOUT_MS",
    "JCODE_HOOK_POST_TOOL",
    "JCODE_HOOK_POST_TOOL_TOOLS",
    "JCODE_HOOK_SESSION_END",
    "JCODE_HOOK_SESSION_START",
    "JCODE_HOOK_TURN_END",
    "JCODE_HOOK_TURN_START",
    "JCODE_IDLE_ANIMATION",
    "JCODE_IMAP_HOST",
    "JCODE_INFO_WIDGET_TOGGLE_KEY",
    "JCODE_JADE_RELAY_API_BASE",
    "JCODE_JADE_RELAY_ENABLED",
    "JCODE_JADE_RELAY_LAUNCH_ENABLED",
    "JCODE_JADE_RELAY_LAUNCH_WORKING_DIR",
    "JCODE_JADE_RELAY_REPLY_ENABLED",
    "JCODE_JADE_RELAY_SESSION_ID",
    "JCODE_JADE_RELAY_TOKEN",
    "JCODE_JADE_RELAY_TOKEN_ID",
    "JCODE_JADE_RELAY_USER_ID",
    "JCODE_KV_CACHE_MISS_NOTICES",
    "JCODE_LATEX_RENDERING",
    "JCODE_MARKDOWN_SPACING",
    "JCODE_MEMORY_EMBEDDING_BACKEND",
    "JCODE_MEMORY_EMBEDDING_BASE_URL",
    "JCODE_MEMORY_EMBEDDING_DIM",
    "JCODE_MEMORY_EMBEDDING_MODEL",
    "JCODE_MEMORY_ENABLED",
    "JCODE_ENABLE_MERMAID",
    "JCODE_MEMORY_MODEL",
    "JCODE_MEMORY_REASONING_EFFORT",
    "JCODE_MEMORY_SIDECAR_ENABLED",
    "JCODE_PERSIST_MEMORY_INJECTIONS",
    "JCODE_MESSAGE_TIMESTAMPS",
    "JCODE_MODEL",
    "JCODE_MODEL_SWITCH_KEY",
    "JCODE_MODEL_SWITCH_PREV_KEY",
    "JCODE_OPEN_MODEL_PICKER_KEY",
    "JCODE_MOUSE_CAPTURE",
    "JCODE_NEW_TERMINAL_KEY",
    "JCODE_NO_EMOJI",
    "JCODE_NTFY_SERVER",
    "JCODE_NTFY_TOPIC",
    "JCODE_OPENAI_NATIVE_COMPACTION_MODE",
    "JCODE_OPENAI_NATIVE_COMPACTION_THRESHOLD_TOKENS",
    "JCODE_OPENAI_REASONING_EFFORT",
    "JCODE_OPENAI_SERVICE_TIER",
    "JCODE_OPENAI_TRANSPORT",
    "JCODE_ANTHROPIC_REASONING_EFFORT",
    "JCODE_PRESERVE_REASONING_CONTEXT",
    "JCODE_PERFORMANCE",
    "JCODE_PIN_IMAGES",
    "JCODE_PIN_TODOS",
    "JCODE_PREVENT_SLEEP_WHILE_STREAMING",
    "JCODE_PROVIDER",
    "JCODE_PROMPT_ENTRY_ANIMATION",
    "JCODE_QUEUE_MODE",
    "JCODE_REASONING_DISPLAY",
    "JCODE_REDRAW_FPS",
    "JCODE_SAME_PROVIDER_ACCOUNT_FAILOVER",
    "JCODE_SCROLL_BOOKMARK_KEY",
    "JCODE_SCROLL_DOWN_FALLBACK_KEY",
    "JCODE_SCROLL_DOWN_KEY",
    "JCODE_SCROLL_PAGE_DOWN_KEY",
    "JCODE_SCROLL_PAGE_UP_KEY",
    "JCODE_SCROLL_PROMPT_DOWN_KEY",
    "JCODE_SCROLL_PROMPT_UP_KEY",
    "JCODE_SCROLL_UP_FALLBACK_KEY",
    "JCODE_SCROLL_UP_KEY",
    "JCODE_SEARXNG_URL",
    "JCODE_SESSION_LIBRARIAN_DEADLINE_SECONDS",
    "JCODE_SESSION_LIBRARIAN_MAX_COST_USD",
    "JCODE_SESSION_LIBRARIAN_MAX_INPUT_TOKENS",
    "JCODE_SESSION_LIBRARIAN_MAX_OUTPUT_TOKENS",
    "JCODE_SESSION_LIBRARIAN_MAX_REQUESTS",
    "JCODE_SESSION_LIBRARIAN_MODEL",
    "JCODE_SESSION_LIBRARIAN_PROVIDER",
    "JCODE_SESSION_LIBRARIAN_REASONING_EFFORT",
    "JCODE_SHOW_AGENTGREP_OUTPUT",
    "JCODE_SHOW_DIFFS",
    "JCODE_SHOW_THINKING",
    "JCODE_SIDE_PANEL_TOGGLE_KEY",
    "JCODE_SIDE_PANEL_NATIVE_SCROLLBAR",
    "JCODE_SMTP_PASSWORD",
    "JCODE_SPAWN_HOOK",
    "JCODE_STREAM_IDLE_TIMEOUT_SECS",
    "JCODE_SWARM_ENABLED",
    "JCODE_SWARM_MODEL",
    "JCODE_SWARM_MAX_CONCURRENT_AGENTS",
    "JCODE_SWARM_SPAWN_MODE",
    "JCODE_SWARM_STRIP_LAYOUT",
    "JCODE_TELEGRAM_BOT_TOKEN",
    "JCODE_TELEGRAM_CHAT_ID",
    "JCODE_TELEGRAM_REPLY_ENABLED",
    "JCODE_TOOL_CALL_DETAILS",
    "JCODE_TOOL_PROFILE",
    "JCODE_TOOLS",
    "JCODE_TRUSTED_EXTERNAL_AUTH_SOURCES",
    "JCODE_TYPING_SCROLL_LOCK_TOGGLE_KEY",
    "JCODE_UPDATE_CHANNEL",
    "JCODE_WEBSEARCH_ENGINE",
    "JCODE_WEBSEARCH_FALLBACK_ENGINES",
    "JCODE_WORKSPACE_DOWN_KEY",
    "JCODE_WORKSPACE_LEFT_KEY",
    "JCODE_WORKSPACE_RIGHT_KEY",
    "JCODE_WORKSPACE_UP_KEY",
    "XDG_CONFIG_HOME",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigCacheFingerprint {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: Option<u64>,
    env: Vec<(String, String)>,
}

impl ConfigCacheFingerprint {
    fn current() -> Self {
        let path = Config::path();
        let metadata = path.as_ref().and_then(|path| std::fs::metadata(path).ok());
        Self {
            path,
            modified: metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok()),
            len: metadata.as_ref().map(std::fs::Metadata::len),
            env: config_env_fingerprint(),
        }
    }
}

struct ConfigCache {
    config: &'static Config,
    fingerprint: ConfigCacheFingerprint,
    last_checked: Instant,
    force_reload: bool,
}

static CONFIG_CACHE: LazyLock<RwLock<ConfigCache>> = LazyLock::new(|| {
    let config = leak_config(Config::load());
    // Fingerprint after the load: applying env overrides may set env vars
    // (e.g. copilot_premium -> JCODE_COPILOT_PREMIUM), and fingerprinting
    // first would guarantee a spurious full reload on the next check.
    let fingerprint = ConfigCacheFingerprint::current();
    // Seed the global context-limit cache from named provider configs on first
    // load so every codepath (TUI info widget, compaction budget, model
    // switching) sees user-configured `context_window` values from the start.
    // Read from the loaded config directly to avoid recursing into config(),
    // which would deadlock on the still-initializing CONFIG_CACHE.
    populate_context_limits_from_config_ref(config);
    RwLock::new(ConfigCache {
        config,
        fingerprint,
        last_checked: Instant::now(),
        force_reload: false,
    })
});

fn leak_config(config: Config) -> &'static Config {
    Box::leak(Box::new(config))
}

/// Seed the global context-limit cache from a config reference directly.
///
/// Used during CONFIG_CACHE initialization (where calling config() would
/// deadlock) and shares its logic with
/// `crate::provider::populate_context_limits_from_config`.
fn populate_context_limits_from_config_ref(cfg: &Config) {
    crate::provider::populate_context_limits_from_config_value(cfg);
}

/// Get the global config instance.
///
/// The returned reference is backed by a reloadable process cache. Calls check
/// the config file path/metadata and relevant environment overrides on a short
/// throttle, not every frame. When those inputs change, the next checked call
/// reloads config.toml and invalidates dependent auth/model caches. Older
/// references remain valid for the duration of any in-flight operation.
pub fn config() -> &'static Config {
    let now = Instant::now();
    if let Ok(cache) = CONFIG_CACHE.read()
        && !cache.force_reload
        && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
    {
        return cache.config;
    }

    let mut reload_reason = None;
    let config = {
        let mut cache = CONFIG_CACHE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let now = Instant::now();
        if !cache.force_reload
            && now.duration_since(cache.last_checked) < CONFIG_CACHE_CHECK_INTERVAL
        {
            return cache.config;
        }

        let fingerprint = ConfigCacheFingerprint::current();
        cache.last_checked = now;
        if cache.force_reload || cache.fingerprint != fingerprint {
            reload_reason = Some(describe_config_reload(
                cache.force_reload,
                &cache.fingerprint,
                &fingerprint,
            ));
            cache.config = leak_config(Config::load());
            // Loading applies env overrides that can themselves set env vars
            // (e.g. copilot_premium propagates config -> JCODE_COPILOT_PREMIUM).
            // Re-fingerprint after the load so those self-inflicted env changes
            // don't trigger a guaranteed second reload on the next check.
            cache.fingerprint = ConfigCacheFingerprint::current();
            cache.force_reload = false;
        }
        cache.config
    };

    if let Some(reason) = reload_reason {
        crate::logging::info(&format!("CONFIG_RELOAD {}", reason));
        // A config reload can change config-derived system prompt sections
        // (feature toggles, sponsors, ...), which legitimately invalidates the
        // KV cache prefix of warm sessions. Document it so a subsequent
        // harness-attributed cache miss is surfaced with this cause instead of
        // as an unexplained prompt mutation.
        crate::cache_invalidation::record("config reload", &reason);
        notify_config_reloaded();
        // Re-seed the global context-limit cache so user edits to named
        // provider `context_window` values take effect without a restart.
        crate::provider::populate_context_limits_from_config();
    }

    config
}

fn describe_config_reload(
    forced: bool,
    previous: &ConfigCacheFingerprint,
    next: &ConfigCacheFingerprint,
) -> String {
    let mut parts = Vec::new();
    if forced {
        parts.push("forced=true".to_string());
    }
    if previous.path != next.path {
        parts.push(format!(
            "path={:?}->{:?}",
            previous.path.as_ref().map(|p| p.display().to_string()),
            next.path.as_ref().map(|p| p.display().to_string())
        ));
    }
    if previous.modified != next.modified {
        parts.push("modified_changed=true".to_string());
    }
    if previous.len != next.len {
        parts.push(format!("len={:?}->{:?}", previous.len, next.len));
    }
    let env_changes = describe_env_changes(&previous.env, &next.env);
    if !env_changes.is_empty() {
        parts.push(format!("env=[{}]", env_changes.join(", ")));
    }
    if parts.is_empty() {
        "unchanged".to_string()
    } else {
        parts.join(" ")
    }
}

fn describe_env_changes(previous: &[(String, String)], next: &[(String, String)]) -> Vec<String> {
    let previous_map: BTreeMap<&str, &str> = previous
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let next_map: BTreeMap<&str, &str> = next
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let keys: BTreeSet<&str> = previous_map
        .keys()
        .chain(next_map.keys())
        .copied()
        .collect();

    keys.into_iter()
        .filter_map(|key| match (previous_map.get(key), next_map.get(key)) {
            (Some(previous), Some(next)) if previous != next => Some(format!(
                "{}:changed({}->{})",
                key,
                env_value_fingerprint(previous),
                env_value_fingerprint(next)
            )),
            (None, Some(next)) => Some(format!("{}:added({})", key, env_value_fingerprint(next))),
            (Some(previous), None) => Some(format!(
                "{}:removed({})",
                key,
                env_value_fingerprint(previous)
            )),
            _ => None,
        })
        .collect()
}

fn env_value_fingerprint(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("len:{} hash:{:016x}", value.len(), hasher.finish())
}

fn config_env_fingerprint() -> Vec<(String, String)> {
    let mut values = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_string_lossy().to_string();
            if CONFIG_ENV_KEYS.contains(&key.as_str()) {
                Some((key, value.to_string_lossy().to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    values
}

pub fn invalidate_config_cache() {
    let mut cache = CONFIG_CACHE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.force_reload = true;
    drop(cache);
    notify_config_reloaded();
}

fn notify_config_reloaded() {
    CONFIG_RELOAD_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for listener in CONFIG_RELOAD_LISTENERS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
    {
        listener();
    }
}

/// Monotonic counter bumped every time the config cache reloads.
///
/// Callers that snapshot config-derived state (e.g. the TUI's parsed
/// keybindings) can poll this cheaply and re-derive their snapshot when the
/// generation changes, giving instant hot-reload of config edits without a
/// restart.
static CONFIG_RELOAD_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Current config reload generation. Increments after every cache reload.
pub fn config_reload_generation() -> u64 {
    CONFIG_RELOAD_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Listeners invoked after the config cache reloads.
///
/// Config is a foundational module, so instead of reaching up into higher-level
/// subsystems (auth cache, event bus) on reload, those subsystems register a
/// reaction here at startup. This keeps config free of upward dependencies and
/// breaks the config -> auth / config -> bus cycle edges.
/// Type of a config reload listener callback.
type ConfigReloadListener = fn();

static CONFIG_RELOAD_LISTENERS: LazyLock<RwLock<Vec<ConfigReloadListener>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Register a callback to run after the config cache reloads.
///
/// Callbacks must be cheap and non-blocking; they run on whichever thread
/// triggers the reload. Intended to be called once per subsystem during
/// process startup.
pub fn on_config_reloaded(listener: fn()) {
    CONFIG_RELOAD_LISTENERS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(listener);
}

pub const LIBRARIAN_MAX_RECEIPT_BYTES: usize = 1024;
pub const LIBRARIAN_MAX_ITEM_TOKENS: u32 = 768;
pub const LIBRARIAN_MAX_NORMALIZED_FILE_TOKENS: u32 = 1200;
pub const LIBRARIAN_MAX_TOOL_CATEGORY_TOKENS: u32 = 2000;

const DEFAULT_LIBRARIAN_PROVIDER: &str = "openai-oauth";
const DEFAULT_LIBRARIAN_MODEL: &str = "gpt-5.6-luna";
const DEFAULT_LIBRARIAN_REASONING_EFFORT: &str = "xhigh";
const DEFAULT_LIBRARIAN_MAX_INPUT_TOKENS: &str = "12000";
const DEFAULT_LIBRARIAN_MAX_OUTPUT_TOKENS: &str = "2500";
const DEFAULT_LIBRARIAN_MAX_REQUESTS: &str = "1";
const DEFAULT_LIBRARIAN_MAX_COST_USD: &str = "0.50";
const DEFAULT_LIBRARIAN_DEADLINE_SECONDS: &str = "120";

/// Per-invocation librarian settings. Explicit `Some("")` values are retained
/// so a malformed high-precedence override cannot silently fall through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibrarianInvocationOverrides {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub max_input_tokens: Option<String>,
    pub max_output_tokens: Option<String>,
    pub max_requests: Option<String>,
    pub max_cost_usd: Option<String>,
    pub deadline_seconds: Option<String>,
}

/// Credential-free route identity resolved independently from the active session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianRouteIdentity {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianBudgets {
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub max_requests: u32,
    pub max_cost_micros: u64,
    pub deadline_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianAdmissionCaps {
    pub max_receipt_bytes: usize,
    pub max_item_tokens: u32,
    pub max_normalized_file_tokens: u32,
    pub max_tool_category_tokens: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLibrarianConfig {
    pub route: LibrarianRouteIdentity,
    pub budgets: LibrarianBudgets,
    pub admission_caps: LibrarianAdmissionCaps,
}

/// Non-secret provider-boundary facts used to fail closed before transmission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarianRouteValidation {
    pub supported: bool,
    pub authentication_available: bool,
    pub worst_case_cost_micros: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibrarianConfigError {
    InvalidRouteField {
        field: &'static str,
        value: String,
    },
    InvalidBudget {
        field: &'static str,
        value: String,
    },
    UnsupportedRoute {
        provider: String,
        model: String,
    },
    MissingAuthentication {
        provider: String,
    },
    UnknownPricing {
        provider: String,
        model: String,
    },
    UnsafeCost {
        approved_micros: u64,
        required_micros: u64,
    },
}

impl std::fmt::Display for LibrarianConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRouteField { field, value } => {
                write!(
                    formatter,
                    "invalid session librarian {field} value {value:?}"
                )
            }
            Self::InvalidBudget { field, value } => write!(
                formatter,
                "invalid session librarian {field} budget {value:?}; use a positive finite value"
            ),
            Self::UnsupportedRoute { provider, model } => write!(
                formatter,
                "session librarian route {provider}/{model} is unsupported"
            ),
            Self::MissingAuthentication { provider } => write!(
                formatter,
                "session librarian route {provider} has no available authentication"
            ),
            Self::UnknownPricing { provider, model } => write!(
                formatter,
                "session librarian route {provider}/{model} has no verified pricing metadata"
            ),
            Self::UnsafeCost {
                approved_micros,
                required_micros,
            } => write!(
                formatter,
                "session librarian worst-case cost {required_micros} micros USD exceeds the approved {approved_micros} micros USD"
            ),
        }
    }
}

impl std::error::Error for LibrarianConfigError {}

/// Resolve librarian settings through invocation, already-applied environment,
/// persisted config, and built-in defaults, then fail closed on provider support,
/// authentication, pricing, and approved worst-case cost.
pub fn resolve_librarian_config<F>(
    config: &Config,
    invocation: &LibrarianInvocationOverrides,
    _active_route: &LibrarianRouteIdentity,
    validate_route: F,
) -> Result<ResolvedLibrarianConfig, LibrarianConfigError>
where
    F: FnOnce(&LibrarianRouteIdentity) -> LibrarianRouteValidation,
{
    let persisted = &config.session_librarian;
    let route = LibrarianRouteIdentity {
        provider: resolve_route_field(
            "provider",
            invocation.provider.as_deref(),
            persisted.provider.as_deref(),
            DEFAULT_LIBRARIAN_PROVIDER,
        )?,
        model: resolve_route_field(
            "model",
            invocation.model.as_deref(),
            persisted.model.as_deref(),
            DEFAULT_LIBRARIAN_MODEL,
        )?,
        reasoning_effort: resolve_route_field(
            "reasoning_effort",
            invocation.reasoning_effort.as_deref(),
            persisted.reasoning_effort.as_deref(),
            DEFAULT_LIBRARIAN_REASONING_EFFORT,
        )?,
    };

    let budgets = LibrarianBudgets {
        max_input_tokens: parse_positive_u32(
            "max_input_tokens",
            resolve_value(
                invocation.max_input_tokens.as_deref(),
                persisted.max_input_tokens.as_deref(),
                DEFAULT_LIBRARIAN_MAX_INPUT_TOKENS,
            ),
        )?,
        max_output_tokens: parse_positive_u32(
            "max_output_tokens",
            resolve_value(
                invocation.max_output_tokens.as_deref(),
                persisted.max_output_tokens.as_deref(),
                DEFAULT_LIBRARIAN_MAX_OUTPUT_TOKENS,
            ),
        )?,
        max_requests: parse_max_requests(resolve_value(
            invocation.max_requests.as_deref(),
            persisted.max_requests.as_deref(),
            DEFAULT_LIBRARIAN_MAX_REQUESTS,
        ))?,
        max_cost_micros: parse_usd_micros(resolve_value(
            invocation.max_cost_usd.as_deref(),
            persisted.max_cost_usd.as_deref(),
            DEFAULT_LIBRARIAN_MAX_COST_USD,
        ))?,
        deadline_seconds: parse_positive_u64(
            "deadline_seconds",
            resolve_value(
                invocation.deadline_seconds.as_deref(),
                persisted.deadline_seconds.as_deref(),
                DEFAULT_LIBRARIAN_DEADLINE_SECONDS,
            ),
        )?,
    };

    let validation = validate_route(&route);
    if !validation.supported {
        return Err(LibrarianConfigError::UnsupportedRoute {
            provider: route.provider.clone(),
            model: route.model.clone(),
        });
    }
    if !validation.authentication_available {
        return Err(LibrarianConfigError::MissingAuthentication {
            provider: route.provider.clone(),
        });
    }
    let required_micros =
        validation
            .worst_case_cost_micros
            .ok_or_else(|| LibrarianConfigError::UnknownPricing {
                provider: route.provider.clone(),
                model: route.model.clone(),
            })?;
    if required_micros > budgets.max_cost_micros {
        return Err(LibrarianConfigError::UnsafeCost {
            approved_micros: budgets.max_cost_micros,
            required_micros,
        });
    }

    let global_input_cap = budgets.max_input_tokens;
    Ok(ResolvedLibrarianConfig {
        route,
        budgets,
        admission_caps: LibrarianAdmissionCaps {
            max_receipt_bytes: LIBRARIAN_MAX_RECEIPT_BYTES,
            max_item_tokens: LIBRARIAN_MAX_ITEM_TOKENS.min(global_input_cap),
            max_normalized_file_tokens: LIBRARIAN_MAX_NORMALIZED_FILE_TOKENS.min(global_input_cap),
            max_tool_category_tokens: LIBRARIAN_MAX_TOOL_CATEGORY_TOKENS.min(global_input_cap),
        },
    })
}

fn resolve_value<'a>(
    invocation: Option<&'a str>,
    persisted: Option<&'a str>,
    default: &'static str,
) -> &'a str {
    invocation.or(persisted).unwrap_or(default)
}

fn resolve_route_field(
    field: &'static str,
    invocation: Option<&str>,
    persisted: Option<&str>,
    default: &'static str,
) -> Result<String, LibrarianConfigError> {
    let value = resolve_value(invocation, persisted, default);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(LibrarianConfigError::InvalidRouteField {
            field,
            value: value.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

fn parse_positive_u32(field: &'static str, value: &str) -> Result<u32, LibrarianConfigError> {
    match value.trim().parse::<u32>() {
        Ok(parsed) if parsed > 0 => Ok(parsed),
        _ => Err(LibrarianConfigError::InvalidBudget {
            field,
            value: value.to_string(),
        }),
    }
}

fn parse_positive_u64(field: &'static str, value: &str) -> Result<u64, LibrarianConfigError> {
    match value.trim().parse::<u64>() {
        Ok(parsed) if parsed > 0 => Ok(parsed),
        _ => Err(LibrarianConfigError::InvalidBudget {
            field,
            value: value.to_string(),
        }),
    }
}

fn parse_max_requests(value: &str) -> Result<u32, LibrarianConfigError> {
    let parsed = parse_positive_u32("max_requests", value)?;
    if parsed != 1 {
        return Err(LibrarianConfigError::InvalidBudget {
            field: "max_requests",
            value: value.to_string(),
        });
    }
    Ok(parsed)
}

fn parse_usd_micros(value: &str) -> Result<u64, LibrarianConfigError> {
    let invalid = || LibrarianConfigError::InvalidBudget {
        field: "max_cost_usd",
        value: value.to_string(),
    };
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.starts_with('+') {
        return Err(invalid());
    }
    let mut parts = trimmed.split('.');
    let whole = parts.next().ok_or_else(&invalid)?;
    let fraction = parts.next();
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    let fraction = fraction.unwrap_or("");
    if fraction.len() > 6 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let whole_micros = whole
        .parse::<u64>()
        .map_err(|_| invalid())?
        .checked_mul(1_000_000)
        .ok_or_else(&invalid)?;
    let fraction_micros = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .map_err(|_| invalid())?
            .checked_mul(10_u64.pow(6 - fraction.len() as u32))
            .ok_or_else(&invalid)?
    };
    let micros = whole_micros
        .checked_add(fraction_micros)
        .ok_or_else(&invalid)?;
    if micros == 0 {
        return Err(invalid());
    }
    Ok(micros)
}

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Keybinding configuration
    pub keybindings: KeybindingsConfig,

    /// External dictation / speech-to-text integration
    pub dictation: DictationConfig,

    /// Display/UI configuration
    pub display: DisplayConfig,

    /// Feature toggles
    pub features: FeatureConfig,

    /// Web search tool configuration
    pub websearch: WebSearchConfig,

    /// Built-in tool exposure configuration
    pub tools: ToolConfig,

    /// Agent Client Protocol adapter configuration
    pub acp: AcpConfig,

    /// Auth trust / consent configuration
    pub auth: AuthConfig,

    /// Provider configuration
    pub provider: ProviderConfig,

    /// Named provider profiles, keyed by profile name.
    ///
    /// Example:
    /// [providers.my-gateway]
    /// type = "openai-compatible"
    /// base_url = "https://llm.example.com/v1"
    /// api_key_env = "MY_GATEWAY_API_KEY"
    pub providers: BTreeMap<String, NamedProviderConfig>,

    /// Named session profiles, keyed by exact profile name.
    ///
    /// The map is omitted when empty so configurations that do not use
    /// profiles retain their existing serialized shape.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, SessionProfileConfig>,

    /// Fresh-session handoff policy. Named profiles may override individual
    /// fields without mutating this global baseline.
    pub handoff: HandoffConfig,

    /// Independent route and hard budgets for the manually invoked session librarian.
    #[serde(default, skip_serializing_if = "SessionLibrarianConfig::is_empty")]
    pub session_librarian: SessionLibrarianConfig,

    /// Agent-specific model defaults
    pub agents: AgentsConfig,

    /// Terminal window/pane spawning configuration
    pub terminal: TerminalConfig,

    /// Lifecycle hooks (external commands at turn/session/tool boundaries)
    pub hooks: HooksConfig,

    /// Ambient mode configuration
    pub ambient: AmbientConfig,

    /// Safety / notification configuration
    pub safety: SafetyConfig,

    /// Optional bounds for unattended `jcode run` invocations.
    pub run_safety: RunSafetyConfig,

    /// Desktop notifications for interactive sessions (e.g. turn completion)
    pub notifications: NotificationsConfig,

    /// WebSocket gateway configuration (for iOS/web clients)
    pub gateway: GatewayConfig,

    /// Compaction configuration
    pub compaction: CompactionConfig,

    /// Power-management configuration (prevent sleep while streaming)
    pub power: PowerConfig,

    /// Auto-review configuration
    pub autoreview: AutoReviewConfig,

    /// Auto-judge configuration
    pub autojudge: AutoJudgeConfig,

    /// Partner discovery configuration. Skipped when it matches the shipped
    /// default so saving config never bakes today's default into the file (see
    /// [`sponsors_is_default`]).
    #[serde(skip_serializing_if = "sponsors_is_default")]
    pub sponsors: SponsorsConfig,

    /// Global "launch a new jcode" hotkeys (macOS). Baked once by auto-import.
    pub launch_hotkeys: LaunchHotkeysConfig,
}

/// Agent Client Protocol adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpConfig {
    /// Client compatibility profile: "standard" (default), "extended", or "full".
    pub profile: String,
    /// Tool profile to request when `jcode acp` starts a daemon itself.
    pub tool_profile: String,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            profile: "standard".to_string(),
            tool_profile: "acp".to_string(),
        }
    }
}

/// Controls which tools are sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolConfig {
    /// Tool profile: "full" (default), "acp", "minimal"/"lite", or "none".
    pub profile: String,
    /// Explicit allow-list. When set, only these tools are exposed.
    /// Use "*" or "all" to expose all tools without an allow-list.
    pub enabled: Vec<String>,
    /// Tools to remove after applying profile/enabled.
    pub disabled: Vec<String>,
    /// Disable all built-in tools unless `enabled` is provided.
    pub disable_base_tools: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSelection {
    pub allowed_tools: Option<HashSet<String>>,
    pub disabled_tools: HashSet<String>,
}

impl ToolConfig {
    pub fn selection(&self) -> ToolSelection {
        let mut allowed_tools = self.base_allowed_tools();
        let disabled_tools: HashSet<String> = self
            .disabled
            .iter()
            .map(|name| normalize_tool_name(name))
            .filter(|name| !name.is_empty())
            .collect();

        if let Some(allowed) = allowed_tools.as_mut() {
            for name in &disabled_tools {
                allowed.remove(name);
            }
        }

        ToolSelection {
            allowed_tools,
            disabled_tools,
        }
    }

    pub fn allowed_tools(&self) -> Option<HashSet<String>> {
        self.selection().allowed_tools
    }

    pub fn apply_to_allowed_set(&self, allowed: &mut HashSet<String>) {
        let selection = self.selection();
        if let Some(global_allowed) = selection.allowed_tools {
            allowed.retain(|name| global_allowed.contains(name));
        }
        for disabled in selection.disabled_tools {
            allowed.remove(&disabled);
        }
    }

    fn base_allowed_tools(&self) -> Option<HashSet<String>> {
        let (explicit, enables_all_tools) = self.normalized_enabled_tools();

        let profile = self.profile.trim().to_ascii_lowercase();
        if enables_all_tools {
            None
        } else if !explicit.is_empty() {
            Some(explicit)
        } else if self.disable_base_tools || matches!(profile.as_str(), "none" | "off" | "disabled")
        {
            Some(HashSet::new())
        } else if matches!(profile.as_str(), "acp") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                    "batch",
                    "mcp",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else if matches!(profile.as_str(), "minimal" | "lite" | "small") {
            Some(
                [
                    "bash",
                    "read",
                    "write",
                    "edit",
                    "multiedit",
                    "apply_patch",
                    "patch",
                    "agentgrep",
                    "ls",
                ]
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            )
        } else {
            None
        }
    }

    fn normalized_enabled_tools(&self) -> (HashSet<String>, bool) {
        let mut enabled = HashSet::new();
        let mut enables_all_tools = false;

        for name in &self.enabled {
            let normalized = normalize_tool_name(name);
            if normalized.is_empty() {
                continue;
            }
            if normalized == "*" || normalized.eq_ignore_ascii_case("all") {
                enables_all_tools = true;
            } else {
                enabled.insert(normalized);
            }
        }

        (enabled, enables_all_tools)
    }
}

fn normalize_tool_name(name: &str) -> String {
    let trimmed = name.trim().trim_matches('"');
    jcode_tool_types::resolve_tool_name(trimmed).to_string()
}

/// External dictation / speech-to-text integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    /// Shell command to run. Must print the transcript to stdout.
    pub command: String,
    /// How to apply the resulting transcript.
    pub mode: crate::protocol::TranscriptMode,
    /// Optional in-app hotkey to trigger dictation.
    pub key: String,
    /// Maximum time to wait for the command to finish (0 = no timeout).
    pub timeout_secs: u64,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            mode: crate::protocol::TranscriptMode::Send,
            key: "off".to_string(),
            timeout_secs: 90,
        }
    }
}

pub mod change_report;
mod config_file;
mod default_file;
mod display_summary;
mod env_overrides;
pub mod session_profile;

pub use session_profile::{
    FieldSource, ProfileInspectionResult, ProfileRestoreStatus, ProviderModelReasoningSnapshot,
    ResolvedProfileSnapshot, ResolvedSessionProfile, SessionPromptOverlay,
    SessionPromptOverlaySnapshot, SkillPolicy, ToolPolicySnapshot,
    active_environment_provider_profile,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "config_color_tests.rs"]
mod color_tests;

/// Whether integration discovery settings carry no information beyond the shipped
/// default, so `[sponsors]` can be left out of written config files.
///
/// Discovery originally shipped opt-in with `enabled = false`, and because
/// config saves serialize the whole struct, any save during that window froze
/// the old default into the user's file and permanently disabled discovery even
/// after the default flipped. Omitting default sections prevents a repeat.
fn sponsors_is_default(sponsors: &SponsorsConfig) -> bool {
    sponsors.enabled && is_default_discovery_endpoint(&sponsors.endpoint)
}

/// Endpoints that only ever came from a shipped default, never a user choice.
fn is_default_discovery_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint.trim_end_matches('/'),
        "https://api.jcode.sh/v1/discovery" | "https://api.solosystems.dev/v1/discovery"
    )
}
