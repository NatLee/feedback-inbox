# ai-kit

A small Rust transport crate for structured JSON completion with OpenAI Responses, Anthropic Messages and local Ollama. It contains no application tables, credentials at rest, permissions, domain prompts, retrieval, job scheduling or sending authority.

`AiClient::new(ProviderConfig, Option<SecretString>)` creates its own redirect-disabled HTTP client. `complete(CompletionRequest)` makes exactly one bounded request and returns typed output/usage or a sanitized `AiError`. Provider configuration kinds serialize as `openai`, `anthropic`, `ollama`.

The host must authorize the configured destination, principal and data before calling; minimize/redact input; enforce concurrency, spending and retry limits; and recheck source/policy versions before accepting output. `capabilities()` describes the adapter, not a guarantee about every selected model. Use an actual synthetic structured-output request to verify a model. No fallback provider is called.

Limits: contract version 1, maximum serialized request 128 KiB, response 1 MiB, timeout 100–120,000 ms, 1–32,768 requested output tokens. Hosts can impose lower limits. Output schema is locally validated with network/file schema resolution disabled; external `$ref` is rejected. Token usage remains `None` if the provider does not report it. HTTP authorization and provider bodies are absent from errors; `SecretString` debug output is redacted and it cannot be serialized.

## Verification

```sh
CARGO_BUILD_JOBS=2 CARGO_PROFILE_TEST_DEBUG=0 cargo test --manifest-path crates/ai-kit/Cargo.toml
cargo clippy --manifest-path crates/ai-kit/Cargo.toml --all-targets -- -D warnings
```

Tests call local HTTP fixture servers. No credentials, paid provider calls or model downloads are required. They verify wire payloads, schema enforcement, reported usage, refusals/incomplete output, timeouts/errors, size limits, secret redaction and redirect rejection.

Provider references: [OpenAI](https://developers.openai.com/api/docs/guides/structured-outputs), [Anthropic](https://platform.claude.com/docs/en/build-with-claude/structured-outputs), [Ollama](https://docs.ollama.com/capabilities/structured-outputs).
