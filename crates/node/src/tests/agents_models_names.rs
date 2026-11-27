use super::*;

#[test]
fn flsh_ids_read_as_names_with_their_provider() {
    let cases = [
        (
            "mistral:deepseek-v4-1-flash-1017",
            "mistral",
            "DeepSeek V4.1 Flash 1017",
        ),
        (
            "openrouter:deepseek/deepseek-v4.1-flash-1017",
            "openrouter",
            "DeepSeek V4.1 Flash 1017",
        ),
        (
            "openrouter:deepseek/deepseek-v4.1-flash-1017:batch",
            "openrouter",
            "DeepSeek V4.1 Flash 1017 (Batch)",
        ),
        (
            "venice:deepseek-v4-1-flash-1017-ultrafast",
            "venice",
            "DeepSeek V4.1 Flash 1017 Ultrafast",
        ),
        (
            "openrouter:anthropic/claude-opus-4.5",
            "openrouter",
            "Claude Opus 4.5",
        ),
        (
            "venice:claude-opus-4-8-fast",
            "venice",
            "Claude Opus 4.8 Fast",
        ),
        ("mistral:glm-5-2", "mistral", "GLM 5.2"),
        ("mistral:codestral-2508", "mistral", "Codestral 2508"),
        (
            "mistral:ministral-14b-2512",
            "mistral",
            "Ministral 14B 2512",
        ),
        (
            "mistral:mistral-code-fim-latest",
            "mistral",
            "Mistral Code FIM Latest",
        ),
        (
            "openrouter:qwen/qwen3-coder-flash-preview",
            "openrouter",
            "Qwen3 Coder Flash Preview",
        ),
        (
            "openrouter:deepseek/deepseek-chat-v3-0324",
            "openrouter",
            "DeepSeek Chat V3 0324",
        ),
        (
            "openrouter:deepseek/deepseek-r1-0528",
            "openrouter",
            "DeepSeek R1 0528",
        ),
        (
            "openrouter:qwen/qwen3-235b-a22b-2507",
            "openrouter",
            "Qwen3 235B A22B 2507",
        ),
        ("openrouter:openai/gpt-4o-mini", "openrouter", "GPT 4o Mini"),
        (
            "openrouter:openai/gpt-oss-120b",
            "openrouter",
            "GPT OSS 120B",
        ),
        ("openrouter:openai/o3-mini", "openrouter", "o3 Mini"),
        (
            "openrouter:x-ai/grok-4.1-fast",
            "openrouter",
            "Grok 4.1 Fast",
        ),
        ("openrouter:moonshotai/kimi-k2-5", "openrouter", "Kimi K2.5"),
        (
            "openrouter:meta-llama/llama-3.1-8b-instruct",
            "openrouter",
            "Llama 3.1 8B Instruct",
        ),
    ];

    for (id, provider, name) in cases {
        assert_eq!(
            flsh_name(id),
            (Some(provider.to_owned()), name.to_owned()),
            "{id}"
        );
    }
}

#[test]
fn ids_without_a_provider_keep_no_provider() {
    assert_eq!(
        flsh_name("gpt-5-1-codex"),
        (None, "GPT 5.1 Codex".to_owned())
    );
    assert_eq!(
        flsh_name("claude-opus-4-1-20250805"),
        (None, "Claude Opus 4.1 20250805".to_owned())
    );
}

#[test]
fn odd_ids_stay_readable() {
    assert_eq!(display_name("---"), "---");
    assert_eq!(display_name("chroma"), "Chroma");
    assert_eq!(display_name("grok-4.1-fast"), "Grok 4.1 Fast");
    assert_eq!(display_name("labs-leanstral-1-5-1"), "Labs Leanstral 1.5.1");
    assert_eq!(display_name("mistral-large-4-0"), "Mistral Large 4.0");
}

#[test]
fn dates_stay_dates() {
    assert_eq!(
        display_name("openai/gpt-4o-2024-08-06"),
        "GPT 4o 2024-08-06"
    );
    assert_eq!(
        display_name("qwen/qwen3.5-plus-02-15"),
        "Qwen3.5 Plus 02-15"
    );
    assert_eq!(display_name("zai-glm-5"), "Z.ai GLM 5");
    assert_eq!(
        display_name("voxtral-mini-tts-2603"),
        "Voxtral Mini TTS 2603"
    );
}
