#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ProviderChoice {
    Jcode,
    Claude,
    #[value(alias = "claude-api", alias = "anthropic-key", alias = "claude-key")]
    AnthropicApi,
    #[deprecated(
        note = "Claude Code CLI subprocess transport is deprecated; use ProviderChoice::Claude for native Anthropic OAuth/API transport"
    )]
    #[value(alias = "claude-subprocess", hide = true)]
    ClaudeSubprocess,
    Openai,
    #[value(
        alias = "openai-key",
        alias = "openai-apikey",
        alias = "openai-platform"
    )]
    OpenaiApi,
    Openrouter,
    #[value(alias = "orca-router")]
    Orcarouter,
    #[value(alias = "aws-bedrock", alias = "aws_bedrock")]
    Bedrock,
    #[value(alias = "azure-openai", alias = "aoai")]
    Azure,
    #[value(alias = "opencode-zen", alias = "zen")]
    Opencode,
    #[value(alias = "opencodego")]
    OpencodeGo,
    #[value(alias = "z.ai", alias = "z-ai", alias = "zai-coding")]
    Zai,
    #[value(
        alias = "kimi-code",
        alias = "kimi-coding",
        alias = "kimi-coding-plan",
        alias = "kimi-for-coding",
        alias = "moonshot-coding"
    )]
    Kimi,
    #[value(alias = "302.ai")]
    Ai302,
    Baseten,
    #[value(alias = "conifer-api")]
    Conifer,
    Cortecs,
    #[value(alias = "cgc", alias = "comtegra-gpu-cloud")]
    Comtegra,
    Deepseek,
    #[value(alias = "fpt-ai", alias = "fptcloud", alias = "fpt-cloud")]
    Fpt,
    Firmware,
    #[value(alias = "hugging-face", alias = "hf")]
    HuggingFace,
    #[value(alias = "moonshot")]
    MoonshotAi,
    Nebius,
    Scaleway,
    Stackit,
    Groq,
    #[value(alias = "mistralai")]
    Mistral,
    #[value(alias = "pplx")]
    Perplexity,
    #[value(alias = "together", alias = "together-ai")]
    TogetherAi,
    #[value(alias = "deep-infra")]
    Deepinfra,
    #[value(alias = "fireworks-ai", alias = "fireworks.ai")]
    Fireworks,
    #[value(alias = "minimax-ai", alias = "minimaxi")]
    Minimax,
    #[value(alias = "x.ai", alias = "x-ai", alias = "grok")]
    Xai,
    /// Grok Build subscription via the authenticated Grok CLI ACP transport.
    #[value(name = "grok-build")]
    GrokBuild,
    #[value(alias = "nvidia", alias = "nim")]
    NvidiaNim,
    #[value(alias = "xiaomi", alias = "mimo", alias = "xiaomi-mimo-api")]
    XiaomiMimo,
    #[value(
        alias = "meta",
        alias = "muse",
        alias = "muse-spark",
        alias = "meta-model-api",
        alias = "meta-ai"
    )]
    MetaMuse,
    #[value(alias = "celeris-ai", alias = "celeris1", alias = "celeris-1")]
    Celeris,
    #[value(alias = "lm-studio")]
    Lmstudio,
    Ollama,
    Chutes,
    #[value(alias = "cerebrascode", alias = "cerberascode")]
    Cerebras,
    #[value(alias = "belvedir.ai", alias = "belvedir-ai")]
    Belvedir,
    #[value(
        alias = "bailian",
        alias = "aliyun-bailian",
        alias = "coding-plan",
        alias = "alibaba-coding"
    )]
    AlibabaCodingPlan,
    #[value(alias = "compat", alias = "custom")]
    OpenaiCompatible,
    Cursor,
    Copilot,
    Gemini,
    #[value(
        alias = "gemini-key",
        alias = "gemini-apikey",
        alias = "google-ai-studio",
        alias = "ai-studio"
    )]
    GeminiApi,
    Antigravity,
    Google,
    Auto,
}

impl ProviderChoice {
    #[allow(deprecated)]
    pub fn as_arg_value(&self) -> &'static str {
        match self {
            Self::Jcode => "jcode",
            Self::Claude => "claude",
            Self::AnthropicApi => "anthropic-api",
            Self::ClaudeSubprocess => "claude-subprocess",
            Self::Openai => "openai",
            Self::OpenaiApi => "openai-api",
            Self::Openrouter => "openrouter",
            Self::Orcarouter => "orcarouter",
            Self::Bedrock => "bedrock",
            Self::Azure => "azure",
            Self::Opencode => "opencode",
            Self::OpencodeGo => "opencode-go",
            Self::Zai => "zai",
            Self::Kimi => "kimi",
            Self::Ai302 => "302ai",
            Self::Baseten => "baseten",
            Self::Conifer => "conifer",
            Self::Cortecs => "cortecs",
            Self::Comtegra => "comtegra",
            Self::Deepseek => "deepseek",
            Self::Fpt => "fpt",
            Self::Firmware => "firmware",
            Self::HuggingFace => "huggingface",
            Self::MoonshotAi => "moonshotai",
            Self::Nebius => "nebius",
            Self::Scaleway => "scaleway",
            Self::Stackit => "stackit",
            Self::Groq => "groq",
            Self::Mistral => "mistral",
            Self::Perplexity => "perplexity",
            Self::TogetherAi => "togetherai",
            Self::Deepinfra => "deepinfra",
            Self::Fireworks => "fireworks",
            Self::Minimax => "minimax",
            Self::Xai => "xai",
            Self::GrokBuild => "grok-build",
            Self::NvidiaNim => "nvidia-nim",
            Self::XiaomiMimo => "xiaomi-mimo",
            Self::MetaMuse => "meta-muse",
            Self::Celeris => "celeris",
            Self::Lmstudio => "lmstudio",
            Self::Ollama => "ollama",
            Self::Chutes => "chutes",
            Self::Cerebras => "cerebras",
            Self::Belvedir => "belvedir",
            Self::AlibabaCodingPlan => "alibaba-coding-plan",
            Self::OpenaiCompatible => "openai-compatible",
            Self::Cursor => "cursor",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::GeminiApi => "gemini-api",
            Self::Antigravity => "antigravity",
            Self::Google => "google",
            Self::Auto => "auto",
        }
    }
}
