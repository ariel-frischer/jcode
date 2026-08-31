use super::{Tool, ToolContext, ToolOutput};
use crate::config::WebSearchEngine;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

pub(crate) mod orchestration;

fn classify_http_status(status: reqwest::StatusCode) -> orchestration::PerEngineOutcomeKind {
    match status {
        reqwest::StatusCode::REQUEST_TIMEOUT => orchestration::PerEngineOutcomeKind::Timeout,
        reqwest::StatusCode::TOO_MANY_REQUESTS
        | reqwest::StatusCode::BAD_GATEWAY
        | reqwest::StatusCode::SERVICE_UNAVAILABLE
        | reqwest::StatusCode::GATEWAY_TIMEOUT => orchestration::PerEngineOutcomeKind::Transient,
        _ => orchestration::PerEngineOutcomeKind::Permanent,
    }
}

fn classify_html_response(
    status: reqwest::StatusCode,
    body: &str,
    results: Vec<SearchResult>,
) -> orchestration::BackendOutcome {
    if !status.is_success() {
        return match classify_http_status(status) {
            orchestration::PerEngineOutcomeKind::Timeout => orchestration::BackendOutcome::Timeout,
            orchestration::PerEngineOutcomeKind::Transient => {
                orchestration::BackendOutcome::Transient
            }
            _ => orchestration::BackendOutcome::Permanent,
        };
    }
    if detect_anti_bot_page(body).is_some() {
        return orchestration::BackendOutcome::Challenge;
    }
    if results.is_empty() {
        orchestration::BackendOutcome::Empty
    } else {
        orchestration::BackendOutcome::Results(results)
    }
}

fn classify_transport_error() -> orchestration::BackendOutcome {
    orchestration::BackendOutcome::Transient
}

/// Web search using DuckDuckGo or Bing (HTML scraping, with optional Bing API)
pub struct WebSearchTool {
    client: reqwest::Client,
    health: Arc<tokio::sync::Mutex<orchestration::EngineHealthMap>>,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: crate::provider::shared_http_client(),
            health: Arc::new(tokio::sync::Mutex::new(Default::default())),
        }
    }
}

#[derive(Deserialize)]
struct WebSearchInput {
    query: String,
    #[serde(default)]
    num_results: Option<usize>,
    #[serde(default)]
    engine: Option<WebSearchEngine>,
    #[serde(default)]
    bing_market: Option<String>,
    #[serde(default)]
    resilience: Option<crate::config::WebSearchPolicyOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Clone, Copy)]
struct BingSearchOptions<'a> {
    market: &'a str,
    configured_api_key: Option<&'a str>,
    api_key_env: &'a str,
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Search the web."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "intent": super::intent_schema_property(),
                "query": {
                    "type": "string",
                    "description": "Search query."
                },
                "num_results": {
                    "type": "integer",
                    "description": "Max results."
                },
                "engine": {
                    "type": "string",
                    "enum": ["duckduckgo", "bing", "searxng"],
                    "description": "Engine. Defaults to duckduckgo; bing uses JCODE_BING_API_KEY, searxng uses JCODE_SEARXNG_URL."
                },
                "bing_market": {
                    "type": "string",
                    "description": "Optional Bing market, e.g. en-US or zh-CN. Defaults to JCODE_BING_MARKET or en-US."
                },
                "resilience": {
                    "type": "object",
                    "description": "Optional non-secret resilient websearch controls. Credentials and endpoints remain configuration-only."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: WebSearchInput = serde_json::from_value(input)?;
        let num_results = params.num_results.unwrap_or(8).min(20);

        let config = crate::config::config();
        if let Some(policy) = config
            .resolve_websearch_policy(params.resilience.as_ref())
            .map_err(|err| anyhow::anyhow!("websearch configuration: {err}"))?
        {
            return self.execute_resilient(&params, policy).await;
        }

        // Keep the legacy branch separate and unchanged when resilience is
        // absent or disabled.
        let mut engines = Vec::new();
        engines.push(params.engine.unwrap_or(config.websearch.engine));
        engines.extend(config.websearch.fallback_engines.iter().copied());
        engines.dedup();

        let market = params
            .bing_market
            .as_deref()
            .unwrap_or(&config.websearch.bing_market);
        let mut last_error = None;
        let mut results = Vec::new();
        for (index, engine) in engines.into_iter().enumerate() {
            let allow_bing_api = index == 0;
            match self
                .search_with_engine(
                    engine,
                    &params.query,
                    num_results,
                    BingSearchOptions {
                        market,
                        configured_api_key: config.websearch.bing_api_key.as_deref(),
                        api_key_env: &config.websearch.bing_api_key_env,
                    },
                    allow_bing_api,
                )
                .await
            {
                Ok(found) => {
                    if !found.is_empty() {
                        results = found;
                        break;
                    }
                }
                Err(err) => last_error = Some(err),
            }
        }

        if results.is_empty()
            && let Some(err) = last_error
        {
            return Err(err);
        }

        if results.is_empty() {
            return Ok(ToolOutput::new(format!(
                "No results found for: {}\n\n\
                 If results are consistently empty on this machine, the default \
                 DuckDuckGo/Bing engines may be blocked here by TLS fingerprinting \
                 or IP reputation (common on Linux/servers). Workarounds:\n\
                 - Point at a SearXNG instance: set `websearch.searxng_url` (or \
                 JCODE_SEARXNG_URL) and use engine \"searxng\".\n\
                 - Or provide a Bing Search API key via JCODE_BING_API_KEY.",
                params.query
            )));
        }

        let mut output = format!("Search results for: {}\n\n", params.query);

        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}**\n   {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet
            ));
        }

        Ok(ToolOutput::new(output))
    }
}

impl WebSearchTool {
    async fn execute_resilient(
        &self,
        params: &WebSearchInput,
        policy: crate::config::ResolvedWebSearchPolicy,
    ) -> Result<ToolOutput> {
        let config = crate::config::config();
        let num_results = params.num_results.unwrap_or(8).min(20);
        let preferred = params.engine.unwrap_or(config.websearch.engine);
        let market = params
            .bing_market
            .as_deref()
            .unwrap_or(&config.websearch.bing_market);
        let mut backend = ResilientHttpBackend {
            tool: self,
            query: &params.query,
            num_results,
            market,
            searxng_url: policy.trusted_searxng_url.as_deref(),
        };
        let execution = orchestration::run_search_with_shared_health(
            &policy,
            preferred,
            self.health.as_ref(),
            Instant::now(),
            &mut backend,
        )
        .await?;

        let diagnostic = policy
            .diagnostics_enabled
            .then(|| orchestration::SearchDiagnosticSummary::from_execution(&policy, &execution));
        let title = orchestration::presentation_title(&execution);
        let activity_summary = diagnostic
            .as_ref()
            .and_then(|_| orchestration::presentation_summary(&execution));

        if let Some(results) = execution.results.as_ref() {
            let mut output = format!("Search results for: {}\n\n", params.query);
            for (index, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. **{}**\n   {}\n   {}\n\n",
                    index + 1,
                    result.title,
                    result.url,
                    result.snippet
                ));
            }
            if let Some(activity_summary) = activity_summary {
                output.push_str(&format!("{activity_summary}\n"));
            }
            let mut tool_output = ToolOutput::new(output);
            tool_output = tool_output.with_title(title);
            if let Some(diagnostic) = diagnostic {
                tool_output = tool_output.with_metadata(diagnostic.metadata());
            }
            return Ok(tool_output);
        }

        let message = match execution.terminal {
            orchestration::SearchTerminalOutcome::NoEligibleEngine => {
                "No eligible websearch engine is configured. Enable an engine or configure a trusted SearXNG instance."
            }
            orchestration::SearchTerminalOutcome::Exhausted => {
                "All eligible websearch engines returned no usable results or failed transiently. Check engine access or try again."
            }
            orchestration::SearchTerminalOutcome::Success => "No results found.",
        };
        let mut output = message.to_string();
        if let Some(activity_summary) = activity_summary {
            output.push('\n');
            output.push_str(&activity_summary);
        }
        let mut tool_output = ToolOutput::new(output);
        tool_output = tool_output.with_title(title);
        if let Some(diagnostic) = diagnostic {
            tool_output = tool_output.with_metadata(diagnostic.metadata());
        }
        Ok(tool_output)
    }

    async fn search_with_engine(
        &self,
        engine: WebSearchEngine,
        query: &str,
        num_results: usize,
        bing: BingSearchOptions<'_>,
        allow_bing_api: bool,
    ) -> Result<Vec<SearchResult>> {
        match engine {
            WebSearchEngine::Duckduckgo => self.search_duckduckgo(query, num_results).await,
            WebSearchEngine::Bing => {
                self.search_bing(query, num_results, bing, allow_bing_api)
                    .await
            }
            WebSearchEngine::Searxng => self.search_searxng(query, num_results).await,
        }
    }

    async fn search_duckduckgo(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>> {
        // DuckDuckGo's HTML endpoint now serves an anti-bot "anomaly" challenge
        // (HTTP 202, no results) for plain GET requests. Submitting the query as
        // a POST form, the same way the real HTML page does, still returns the
        // standard results markup with a 200.
        let response = self
            .client
            .post("https://html.duckduckgo.com/html/")
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&[("q", query), ("kl", "us-en")])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Search failed with status: {}",
                response.status()
            ));
        }

        let body = response.text().await?;
        let results = parse_ddg_results(&body, num_results);
        if results.is_empty()
            && let Some(reason) = detect_anti_bot_page(&body)
        {
            return Err(anyhow::anyhow!(
                "DuckDuckGo served an anti-bot challenge page ({reason}) instead of \
                 results. This is commonly caused by TLS fingerprinting or IP \
                 reputation on Linux. Falling back to another engine if configured."
            ));
        }

        Ok(results)
    }

    async fn search_bing(
        &self,
        query: &str,
        num_results: usize,
        options: BingSearchOptions<'_>,
        allow_api: bool,
    ) -> Result<Vec<SearchResult>> {
        if allow_api {
            if let Some(api_key) = options
                .configured_api_key
                .filter(|key| !key.trim().is_empty())
            {
                return self
                    .search_bing_api(query, num_results, options.market, api_key)
                    .await;
            }
            if let Ok(api_key) = std::env::var(options.api_key_env)
                && !api_key.trim().is_empty()
            {
                return self
                    .search_bing_api(query, num_results, options.market, &api_key)
                    .await;
            }
        }

        self.search_bing_html(query, num_results, options.market)
            .await
    }

    async fn search_bing_api(
        &self,
        query: &str,
        num_results: usize,
        market: &str,
        api_key: &str,
    ) -> Result<Vec<SearchResult>> {
        let response = self
            .client
            .get("https://api.bing.microsoft.com/v7.0/search")
            .query(&[
                ("q", query),
                ("count", &num_results.to_string()),
                ("mkt", market),
            ])
            .header("Ocp-Apim-Subscription-Key", api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Bing API search failed with status: {}",
                response.status()
            ));
        }

        Ok(parse_bing_api_results(response.json().await?, num_results))
    }

    async fn search_bing_html(
        &self,
        query: &str,
        num_results: usize,
        market: &str,
    ) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.bing.com/search?q={}&mkt={}",
            urlencoding::encode(query),
            urlencoding::encode(market)
        );

        let response = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Bing search failed with status: {}",
                response.status()
            ));
        }

        let body = response.text().await?;
        let results = parse_bing_html_results(&body, num_results);
        if results.is_empty()
            && let Some(reason) = detect_anti_bot_page(&body)
        {
            return Err(anyhow::anyhow!(
                "Bing served an anti-bot challenge page ({reason}) instead of results."
            ));
        }

        Ok(results)
    }

    /// Query a user-configured SearXNG instance via its JSON API. SearXNG is a
    /// self-hostable metasearch engine; because the request goes to an instance
    /// the user controls (or a public one they trust), it sidesteps the TLS
    /// fingerprinting / IP-reputation blocks that DuckDuckGo and Bing apply to
    /// scraped requests on some hosts (see issue #270).
    async fn search_searxng(&self, query: &str, num_results: usize) -> Result<Vec<SearchResult>> {
        let config = crate::config::config();
        let base = config
            .websearch
            .searxng_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .map(|u| u.to_string())
            .or_else(|| {
                std::env::var(&config.websearch.searxng_url_env)
                    .ok()
                    .filter(|u| !u.trim().is_empty())
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "SearXNG engine selected but no instance URL configured. Set \
                     `websearch.searxng_url` in your config or the {} environment \
                     variable to a SearXNG base URL (e.g. https://searx.example.org).",
                    config.websearch.searxng_url_env
                )
            })?;

        let endpoint = format!("{}/search", base.trim_end_matches('/'));
        let response = self
            .client
            .get(&endpoint)
            .query(&[("q", query), ("format", "json")])
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "SearXNG search failed with status {} (endpoint: {endpoint}). \
                 Ensure the instance has the JSON format enabled in its settings.",
                response.status()
            ));
        }

        let parsed: SearxngResponse = response.json().await.map_err(|err| {
            anyhow::anyhow!(
                "SearXNG returned a non-JSON response ({err}). The instance may have \
                 the JSON format disabled; enable `formats: [html, json]` in its settings."
            )
        })?;

        Ok(parse_searxng_results(parsed, num_results))
    }

    async fn search_duckduckgo_resilient(
        &self,
        query: &str,
        num_results: usize,
    ) -> orchestration::BackendOutcome {
        let response = match self
            .client
            .post("https://html.duckduckgo.com/html/")
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(&[("q", query), ("kl", "us-en")])
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return classify_transport_error(),
        };
        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(_) => return classify_transport_error(),
        };
        classify_html_response(status, &body, parse_ddg_results(&body, num_results))
    }

    async fn search_bing_html_resilient(
        &self,
        query: &str,
        num_results: usize,
        market: &str,
    ) -> orchestration::BackendOutcome {
        let url = format!(
            "https://www.bing.com/search?q={}&mkt={}",
            urlencoding::encode(query),
            urlencoding::encode(market)
        );
        let response = match self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return classify_transport_error(),
        };
        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(_) => return classify_transport_error(),
        };
        classify_html_response(status, &body, parse_bing_html_results(&body, num_results))
    }

    async fn search_searxng_resilient(
        &self,
        query: &str,
        num_results: usize,
        base: &str,
    ) -> orchestration::BackendOutcome {
        let endpoint = format!("{}/search", base.trim_end_matches('/'));
        let response = match self
            .client
            .get(&endpoint)
            .query(&[("q", query), ("format", "json")])
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36",
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return classify_transport_error(),
        };
        if !response.status().is_success() {
            return match classify_http_status(response.status()) {
                orchestration::PerEngineOutcomeKind::Timeout => {
                    orchestration::BackendOutcome::Timeout
                }
                orchestration::PerEngineOutcomeKind::Transient => {
                    orchestration::BackendOutcome::Transient
                }
                _ => orchestration::BackendOutcome::Permanent,
            };
        }
        let parsed: SearxngResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(_) => return orchestration::BackendOutcome::Permanent,
        };
        let results = parse_searxng_results(parsed, num_results);
        if results.is_empty() {
            orchestration::BackendOutcome::Empty
        } else {
            orchestration::BackendOutcome::Results(results)
        }
    }
}

struct ResilientHttpBackend<'a> {
    tool: &'a WebSearchTool,
    query: &'a str,
    num_results: usize,
    market: &'a str,
    searxng_url: Option<&'a str>,
}

#[async_trait]
impl orchestration::SearchBackend for ResilientHttpBackend<'_> {
    async fn search(
        &mut self,
        engine: WebSearchEngine,
        _attempt: u8,
    ) -> orchestration::BackendOutcome {
        match engine {
            WebSearchEngine::Duckduckgo => {
                self.tool
                    .search_duckduckgo_resilient(self.query, self.num_results)
                    .await
            }
            WebSearchEngine::Bing => {
                self.tool
                    .search_bing_html_resilient(self.query, self.num_results, self.market)
                    .await
            }
            WebSearchEngine::Searxng => match self.searxng_url {
                Some(url) => {
                    self.tool
                        .search_searxng_resilient(self.query, self.num_results, url)
                        .await
                }
                None => orchestration::BackendOutcome::Permanent,
            },
        }
    }
}

/// Map a parsed SearXNG JSON response to `SearchResult`s, dropping entries with
/// empty URLs and capping to `num_results`.
fn parse_searxng_results(response: SearxngResponse, num_results: usize) -> Vec<SearchResult> {
    response
        .results
        .into_iter()
        .filter(|r| !r.url.trim().is_empty())
        .take(num_results)
        .map(|r| SearchResult {
            title: if r.title.trim().is_empty() {
                r.url.clone()
            } else {
                r.title
            },
            url: r.url,
            snippet: r.content.unwrap_or_default(),
        })
        .collect()
}

mod search_regex {
    use regex::Regex;
    use std::sync::OnceLock;

    fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
        match Regex::new(pattern) {
            Ok(regex) => Some(regex),
            Err(err) => {
                crate::logging::warn(&format!(
                    "websearch: failed to compile static regex {label}: {}",
                    err
                ));
                None
            }
        }
    }

    macro_rules! static_regex {
        ($name:ident, $pat:expr_2021) => {
            pub fn $name() -> Option<&'static Regex> {
                static RE: OnceLock<Option<Regex>> = OnceLock::new();
                RE.get_or_init(|| compile_regex($pat, stringify!($name)))
                    .as_ref()
            }
        };
    }

    static_regex!(
        result_link,
        r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#
    );
    static_regex!(
        result_snippet,
        r#"(?s)<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#
    );
    static_regex!(tag, r"<[^>]+>");
    static_regex!(
        bing_result_block,
        r#"(?s)<li[^>]*class="[^"]*\bb_algo\b[^"]*"[^>]*>(.*?)</li>"#
    );
    static_regex!(
        bing_link,
        r#"(?s)<h2[^>]*>\s*<a[^>]*href="([^"]+)"[^>]*>(.*?)</a>\s*</h2>"#
    );
    static_regex!(
        bing_caption,
        r#"(?s)<div[^>]*class="[^"]*\bb_caption\b[^"]*"[^>]*>.*?<p[^>]*>(.*?)</p>"#
    );
}

#[derive(Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngResult>,
}

#[derive(Deserialize)]
struct SearxngResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct BingApiResponse {
    #[serde(rename = "webPages")]
    web_pages: Option<BingWebPages>,
}

#[derive(Deserialize)]
struct BingWebPages {
    value: Vec<BingWebPage>,
}

#[derive(Deserialize)]
struct BingWebPage {
    name: String,
    url: String,
    #[serde(default)]
    snippet: String,
}

fn parse_bing_api_results(response: BingApiResponse, max_results: usize) -> Vec<SearchResult> {
    response
        .web_pages
        .map(|pages| {
            pages
                .value
                .into_iter()
                .take(max_results)
                .map(|page| SearchResult {
                    title: page.name,
                    url: page.url,
                    snippet: page.snippet,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_bing_html_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let (Some(block_re), Some(link_re), Some(caption_re), Some(tag_re)) = (
        search_regex::bing_result_block(),
        search_regex::bing_link(),
        search_regex::bing_caption(),
        search_regex::tag(),
    ) else {
        return results;
    };

    for block in block_re.captures_iter(html) {
        if results.len() >= max_results {
            break;
        }
        let Some(link) = link_re.captures(&block[1]) else {
            continue;
        };
        let url = html_decode(&link[1]);
        if !url.starts_with("http") || url.contains("bing.com") {
            continue;
        }
        let title = html_decode(&tag_re.replace_all(&link[2], ""));
        let snippet = caption_re
            .captures(&block[1])
            .map(|cap| html_decode(&tag_re.replace_all(&cap[1], "")))
            .unwrap_or_default();
        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

fn parse_ddg_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let (Some(result_link), Some(result_snippet), Some(tag)) = (
        search_regex::result_link(),
        search_regex::result_snippet(),
        search_regex::tag(),
    ) else {
        return results;
    };

    let links: Vec<_> = result_link.captures_iter(html).collect();
    let snippets: Vec<_> = result_snippet.captures_iter(html).collect();

    for (i, link_cap) in links.iter().enumerate() {
        if results.len() >= max_results {
            break;
        }

        let url = decode_ddg_url(&link_cap[1]);
        let title = html_decode(&tag.replace_all(&link_cap[2], ""));

        if !url.starts_with("http") || url.contains("duckduckgo.com") {
            continue;
        }

        let snippet = if i < snippets.len() {
            let raw = &snippets[i][1];
            html_decode(&tag.replace_all(raw, ""))
        } else {
            String::new()
        };

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

/// Detect whether an HTML body is an anti-bot/captcha challenge rather than a
/// real results page. DuckDuckGo (and similar) serve these with HTTP 200, so a
/// successful status plus zero parsed results is ambiguous without this check.
///
/// Returns a short human-readable reason when a challenge page is detected.
fn detect_anti_bot_page(html: &str) -> Option<&'static str> {
    let lowered = html.to_ascii_lowercase();
    const MARKERS: &[(&str, &str)] = &[
        ("anomaly-modal", "anomaly challenge"),
        ("anomaly.js", "anomaly challenge"),
        ("dpn=1", "anomaly challenge"),
        ("captcha", "captcha"),
        ("g-recaptcha", "recaptcha"),
        ("are you a robot", "bot check"),
        ("unusual traffic", "bot check"),
        ("verify you are human", "human verification"),
        ("challenge-platform", "cloudflare challenge"),
        ("cf-challenge", "cloudflare challenge"),
    ];
    for (needle, reason) in MARKERS {
        if lowered.contains(needle) {
            return Some(reason);
        }
    }
    None
}

fn decode_ddg_url(url: &str) -> String {
    // DDG wraps URLs like //duckduckgo.com/l/?uddg=ACTUAL_URL&...
    if let Some(uddg_start) = url.find("uddg=") {
        let start = uddg_start + 5;
        let end = url[start..]
            .find('&')
            .map(|i| start + i)
            .unwrap_or(url.len());
        let encoded = &url[start..end];
        urlencoding::decode(encoded)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| encoded.to_string())
    } else {
        url.to_string()
    }
}

fn html_decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
