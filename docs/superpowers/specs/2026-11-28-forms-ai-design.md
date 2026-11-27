# Versioned forms and reviewed AI correspondence

Status: implementation design. OpenAI **Platform API keys**, Claude API, and local Ollama are the confirmed provider choices. This document records implementation decisions under the shared product direction established during planning; it does not claim separate user approval of every technical choice. No production data, paid inference, or external customer messages are used for verification.

## Implemented contract resolution

The following implementation decisions supersede differing provisional DTO names or numbers later in this design. The exact supported routes/fields are [the API contract](../../contracts/forms-ai-ui.md); user/operator behavior is [the guide](../../forms-ai.md). This resolution keeps the complete form → analysis → review → human send / controlled template send flow, while making the shipped boundaries explicit.

- Draft save **and publication** advance the optimistic draft revision. Duplicate publication of the same expected revision returns 409. Public validation errors identify the offending field in a banner and preserve safe posted values; no separate per-control error-map contract is exposed.
- Embed POST uses the dedicated `/embed` action, without a client-controlled `_display` switch. Standalone and embed result/error headers stay separate.
- Provider mutation is `PUT`; stopping a provider disables its retained configuration rather than deleting historical references. Returned fields are `has_credential`, `revision`, `tested_revision`, `last_test_error`; no secret suffix is exposed. Stored credential envelope version is inside ciphertext. Synthetic tests prove the selected model's schema behavior; the UI does not claim detailed per-stage latency diagnostics.
- Site AI uses `field_ids`, `include_personal`, categories/tags, `daily_attempt_limit`, `timeout_seconds`, `max_output_tokens`, `generate_on_receipt`, and separate `auto_mode`. Private notes are **always excluded**. Language is inferred; arbitrary prompt/policy editing and explicit language override are outside this release. Approved plain-text knowledge carries reply facts/policies instead.
- Categories and tags each have at most 30 configured strings. Output allows at most 10 tags, urgency `low|normal|high`, 1–20 Unicode-scalar evidence ranges, language ≤40, next action ≤1,000, at most 10 missing-information strings. Risk enum is `security|privacy|legal|billing|abuse|self_harm|uncertain|prompt_injection`. The full machine schema is `ai::analysis::schema()`.
- Up to 50 active knowledge entries and 50 active templates per site, title/name ≤120, body ≤10,000. Each edit inserts an immutable revision and retires its predecessor. The latest **five currently approved** knowledge revisions are included, within the shared 20,000-character cap; no per-run knowledge selector. Historical form values are additionally checked against the **current** publication's field sensitivity/removal so a later restriction fences old feedback too.
- Analysis stores a redacted immutable input, its hash, provider/policy revisions, knowledge IDs, source hash, output/usage and actual status. No extra raw prompt copy. Current run, proposal, draft and outbound state names are those in the API contract, not the earlier illustrative names below.
- Durable table is `workflow_jobs`, kinds `analysis|reply|notify_email|notify_webhook`; jitter 0–2 seconds. Work records show latest 100 jobs. Manual regeneration creates a fresh analysis; there is no arbitrary dead-letter replay UI. A lost pre-transmission reply claim can retry, while `sending` recovers to `delivery_unknown` and never auto-retries. Unknown reconciliation requires an administrator's explanation, saved as an internal note and hashed in the append-only audit.
- One rule matches one exact published select value plus one configured category and one approved template revision. At most 20 rules; multiple matches fall back to review. Template interpolation is restricted to `site_name`, `reference`, `category`. With no knowledge reference, only the exact standard acknowledgement preset is allowed; every other template requires approved knowledge.
- Automatic eligibility also requires no human-origin/rejected draft, no missing information and no high urgency. Daily/recipient reservations are **not refunded** on cancellation/failure. Activation/resumption records a feedback-ID high-water mark as well as time, excluding same-second historical records.
- Activation/pause is one revision-checked site settings save. The UI has a dedicated explicit enable action, separate generation/send controls, template approval and rule/dry-run pages. Missing signing key/consent/rule configuration makes sends ineligible and is shown by the dry run; activation itself does not demand artificial fixture confirmation. No freeform AI output can auto-send.

The original detailed design below records rationale and remains applicable except where this resolution or the exact API contract differs. Verification receipts distinguish source tests, protocol fixtures, real Mailpit, and browser checks; they do not claim live paid model validation.

## Outcome and boundaries

A site administrator publishes a form, shares its link or generated integration, and receives schema-validated feedback. A member can inspect an evidence-backed classification, edit an AI reply draft, and approve its exact recipient and content before sending. Administrators can additionally enable tightly scoped automatic replies using approved templates and deterministic rules. Ordinary intake and the inbox continue working with AI disabled or unavailable.

Retain the current Rust/axum/SQLite backend, React/Tailwind frontend, account and site-membership authority, OAuth/API/MCP contracts, and nginx as the only public Compose service. Public forms remain anonymous collection endpoints; collection keys and browser origins do not authenticate a visitor. Existing `feedback:read` MCP operations remain read-only. No arbitrary tool execution, raw SQL, model-controlled network access, or credential extraction is introduced.

This work includes the complete form, classification, draft, human-send, and automatic-policy implementation. It does not include attachments, a general visual workflow engine, payment/account actions, inbound email threading, public customer accounts, a vector database, or a third-party widget SDK. iframe, HTML, JavaScript, curl, and backend examples are sufficient integration formats for this release.

## Existing code to reuse

- `src/api/src/routes/submit.rs`: public GET/POST/OPTIONS, thank-you page, rate limit and honeypot. Existing notification spawning becomes transactional enqueue.
- `src/api/src/intake.rs`: legacy aliases, base field limits, extra fields and email checks. Keep legacy normalization compatible; new schema validation is an explicit layer.
- `src/api/src/routes/sites.rs`: admin settings, membership, key rotation and test-notification endpoint.
- `src/api/src/routes/feedback.rs`: visibility and transactional processing history. New classification/draft records are separate from workflow status and internal notes.
- `src/api/src/notify.rs`: SMTP construction, HTML escaping, staff notifications and webhook formatting. Visitor replies use a separate message type and outbound record.
- `src/web/src/pages/SiteDetail.tsx`: split the existing snippets/settings sections into focused form/integration components; reuse copy and feedback conventions.
- `src/web/src/pages/FeedbackDetail.tsx`: keep the original letter and internal notes; add suggestions and a distinct reply composer.
- `src/api/src/access_registry.rs`: retain its existing explicit read-only allowlist. No new write scope or outbound-send MCP operation is part of this change.

## Global constraints

- AI is optional; no provider configuration or deployment secret is required for existing form and inbox operations.
- Provider credentials stay server-side; the browser never receives a saved secret, signing key after creation, or provider authorization header.
- Public intake accepts at most 65,536 bytes; no file upload is supported.
- All persisted timestamps use UTC; quota days are UTC and are labeled UTC in the UI.
- Admin/API pages retain `X-Frame-Options: DENY`; framing is enabled only for the dedicated public embed response.
- Existing SQLite data and existing read-only API/MCP authorization must survive migration.
- Generated snippets are deterministic checked templates, never model-generated integration code.
- Automated verification uses disposable databases, local fixture model providers, and local Mailpit only.

## 1. Form schemas and publication

### Schema contract

`FormSchemaV1` is `{schema_version: 1, title, description, submit_label, success_message, fields}`. It is independent from the monotonically increasing **published form version**. Title is 1–60 characters, description/success message at most 1,000, and submit label 1–40. A form has 1–30 fields including built-ins. At least one visible field is required; all-hidden forms are invalid.

Each field is `{id, key, label, type, required, help_text, sensitivity, validation, options?, default_value?}`. Array order is display order. IDs are random 32-character lowercase hex values. Keys are unique ASCII `[a-z][a-z0-9_]{0,39}`, immutable after first publication; custom keys cannot reuse built-in aliases, reserved `extra`, or `_` control names. Labels are 1–100 characters, help text at most 300. Sensitivity is `public | personal | sensitive`; member ID/email/phone default to personal, custom fields default to personal. Hidden context is always labeled visitor-supplied/claimed; never put secrets in it.

Supported types and limits:

| Type | Normalized value and validation |
|---|---|
| `text` | String; default maximum 1,000 characters, configurable 1–2,000. Built-in name/phone/company retain 100/50/200. |
| `textarea` | String; default maximum 2,000, configurable 1–10,000. Built-in message retains 10,000. |
| `email` | String, maximum 254, same server validation used for contact email. |
| `number` | Finite JSON number between -1e12 and 1e12; optional inclusive min/max, optionally integer-only. HTML text is parsed server-side. |
| `select` | One stable option value; 1–50 options, value 1–80 characters, label 1–100. Unknown values fail. |
| `checkbox` | Boolean; missing HTML value means false; required means true. |
| `hidden` | String, maximum 500; optional public default. Required/length checks still run server-side. |

`name`, `email`, `phone`, `company`, `message` keep their fixed types and project into current feedback columns. Administrators may omit optional built-ins, reorder them or change labels/help/required flags; custom `member_id` is a text field with a preset label. Values not supplied are null except checkbox=false. No regex authoring, nested object fields, arbitrary HTML, file fields or runtime scripts are accepted.

### Storage and compatibility

Add `form_drafts(site_id PK, revision, schema_json, updated_by, updated_at)` and `form_versions(id PK, site_id, version, schema_json, schema_hash, published_by, published_at, retired_at, UNIQUE(site_id,version))`; `sites.published_form_version_id` points to the active version. Publication validates and inserts the immutable snapshot, updates the active pointer and legacy `required_fields` mirror in one transaction. Draft updates use `expected_revision`, returning 409 on a stale editor. The active version cannot be retired; an older version can be retired explicitly.

Migration creates version 1 for each site from current five fields and required flags. Existing feedback is untouched and displayed as `legacy` without claiming a historically accurate schema. New feedback stores `form_version_id`, `answers_json` (typed values keyed by stable field ID), `identity_json`, and `submission_key`; the existing columns/extra remain available to old readers and export. The accepted immutable form version supplies historical labels and ordering. New schema fields project into `extra` for compatibility, while the typed answer record remains authoritative.

New snippets submit `_form_version=<integer>`. An explicit version accepts against that version until retired; retirement returns 410 with `code: form_version_retired` and a hosted-form link. Unversioned legacy requests validate against the active version and retain legacy unknown-extra handling; mark them `legacy_unversioned`. This preserves the current meaning of changing required fields. Versioned integrations reject unknown non-control fields. Legacy extras retain existing 30-field/100-name/2,000-value limits and are excluded from AI unless mapped to a published field. Old `PATCH /api/sites/:id` calls supplying `required_fields` atomically publish an updated version with those built-in required flags; they cannot invent custom fields.

New schema renderer and snippet generator consume the same normalized schema. A phone-required form must include a phone control in every generated example. HTML escape labels, values, attributes and endpoint strings. Render field-level errors and submitted safe values on HTML validation errors; never repopulate the honeypot or identity token. Do not include values or tokens in URLs.

### Authenticated form API

| Method/path | Contract |
|---|---|
| `GET /api/sites/:id/form` | Visible site members get published schema/version and safe integration data; admins also get draft/revision/version history. |
| `PUT /api/sites/:id/form/draft` | Admin; `{expected_revision, schema}` → updated draft. |
| `POST /api/sites/:id/form/publish` | Admin; `{expected_revision}` → published immutable version. |
| `POST /api/sites/:id/form/versions/:version/retire` | Admin; reject active version; audit retirement. |
| `GET /api/sites/:id/integrations?format=html|fetch|curl|node|iframe` | Visible site member; active version only; generated code contains no private credentials. |

All mutating endpoints keep the existing same-origin guard and role checks. Add typed API errors `{error, code, fields?}` while retaining `error` for old clients.

## 2. Intake, origins and embedding

Keep `GET/POST/OPTIONS /f/:key`, add `GET /f/:key/embed` and `GET /f/:key/embed/thanks`. `GET /f/:key` continues to be a standalone shareable page. No frontend authentication is required for these paths. Public forms use the same cream paper/vermilion typography and dark-mode treatment as the app, with a minimal receipt/thank-you state.

`allowed_origins` remains the browser POST allowlist, maximum 20 normalized exact http(s) origins. Add `origin_mode: legacy_public|exact`: existing empty lists migrate to visibly labeled `legacy_public`; existing nonempty lists and every new site use `exact`. Exact mode with an empty list permits only the hosted same-origin form, not arbitrary websites. A legacy site may explicitly move to exact mode; new sites cannot enable legacy_public. Exact configured public origin is always accepted for the hosted form. Origin `null` is rejected in exact mode; requests without Origin retain CLI/server compatibility. Origin is never proof of identity. Browser JSON examples explicitly use `credentials: "omit"`, `Content-Type: application/json`, `Accept: application/json`, and a bounded retry only when an idempotency key is present. Never prescribe `mode: no-cors` or credentialed wildcard access.

Refactor public error handling so every **allowed-origin** response, including malformed JSON, validation, rate limit, inactive form, size error and internal error, receives consistent CORS headers; unknown keys may return `Access-Control-Allow-Origin: *` without credentials so a non-secret missing-form error is readable. Disallowed origins remain denied. OPTIONS supports `POST, OPTIONS` and `Content-Type, Accept, Idempotency-Key`, `Vary: Origin`, max-age 600 seconds. Do not add CORS to authenticated admin routes.

Add separate `embed_origins` (0–20 exact origins). Empty means embedding disabled, not wildcard. Only `/f/:key/embed` and its result pages omit X-Frame-Options and set CSP `frame-ancestors 'self' <validated origins>`. All other pages remain DENY. Use `form-action 'self'`, `base-uri 'none'`, `default-src 'none'`, explicit style policy; the first iframe implementation needs no scripts, popup permissions, or postMessage. Its copied markup has a title, width 100%, height 760, loading=lazy and a fallback/open-in-new-tab link. iframe submissions carry `_display=embed`, remain within the frame, and use the embed thank-you page rather than redirecting an enclosing customer website. CSP is also applied to HTML validation/closed/not-found result pages in the embed flow; distinguish this response context explicitly rather than by untrusted Referer. Authenticated pages are never embedded.

Idempotency: `_submission_id` or `Idempotency-Key` is an opaque 16–80 ASCII token. Unique `(site_id, submission_key)` is retained 7 days; same key plus canonical request hash returns the same success without another feedback/job; different payload returns 409. Bound storage and cleanup are required. Hosted rendering creates a one-use random submission ID hidden control. Repeated honeypot requests continue returning success without persistence. A token is scoped to a site, never globally. Rate limit is enforced before first acceptance; recognized identical retries can reuse the stored result.

Integration UI explains same-origin, browser allowlisted, iframe and backend modes; shows canonical public URL, version, origin configuration and real receipt time. A verification session is admin-created, expires in 10 minutes, has a random token and accepts at most 5 marked synthetic submissions. Test rows/jobs never send email/webhooks or trigger AI/auto-replies, are labeled in the inbox, excluded from ordinary stats/CSV, and are purged within 24 hours. The checker reports actual receipt and browser response success separately; a server preflight does not claim full browser verification.

### Verified member identity

Keep claimed `member_id` visible as claimed. Add an optional per-site identity-signing secret, generated/rotated by an admin and displayed once. Store it encrypted with the same deployment secret facility as provider keys. A Node backend example signs a base64url payload with HMAC-SHA256: `{v:1, site_id, sub, email?, email_verified?, iat, exp, jti}`. `sub` is 1–128 characters, TTL at most 600 seconds, recommended 300, clock skew 60 seconds; jti is 16–80 characters and unique per site for the validity window. Token scope includes site ID, version and expiry. A valid assertion can establish verified member ID and a verified contact email only when the trusted backend explicitly attests `email_verified:true`; browser hidden fields cannot do this.

`POST /api/sites/:id/identity-key/rotate` is admin-only and returns the key once. `_identity_token` is accepted only in POST body, never URL. Verify signature in constant time and reject conflicting posted contact email. Consume jti atomically with intake; identical idempotent retries remain successful. Rotate invalidates old tokens. Neither the key nor token appears in logs, public schema, integrations endpoint, feedback detail or AI input. Persist only normalized verified claims/provenance. The Node example reads the secret from the integrator's server environment.

## 3. Reusable provider boundary and configuration

Create local crate `crates/ai-kit`, initially package `ai-kit` version 0.1.0, `publish=false`, Rust edition 2021. It is a reusable transport/protocol boundary for future private dashboards: **no FI tables, SQLx, auth/session types, domain taxonomies, prompts, mail, policy, or RAG retrieval**. Application-specific data minimization happens before calling it; response semantic validation happens after it returns.

Public contract:

```rust
ProviderKind = OpenAi | Anthropic | Ollama;
ProviderConfig { kind, base_url: Url, model: String }
CompletionRequest { contract_version: u16, system: String, input: Value,
    output_schema: Value, schema_name: String, max_output_tokens: u32,
    timeout_ms: u64, request_id: String }
CompletionResult { output: Value, resolved_model: String,
    provider_request_id: Option<String>, usage: Option<TokenUsage>, duration_ms: u64 }
TokenUsage { input_tokens: Option<u64>, output_tokens: Option<u64> }
AiErrorKind = Authentication | InvalidRequest | UnsupportedCapability | RateLimited
    | Timeout | Network | ProviderUnavailable | Refused | Incomplete | InvalidOutput;
AiError { kind, retry_after_ms: Option<u64>, safe_message: String }
```

`AiClient::new(config, credential: Option<SecretString>) -> Result<AiClient, AiError>` and `async complete(&self, request: CompletionRequest) -> Result<CompletionResult, AiError>`. `SecretString` has redacted Debug and is never serializable. `capabilities()` reports adapter-supported features, while a real synthetic test establishes the selected model's actual ability. Implement transports for OpenAI `/v1/responses` with `text.format`, Claude `/v1/messages` with `output_config.format`, and Ollama `/api/chat` with `format` and `stream:false`. Responses/stop reasons/refusal/incomplete states are handled explicitly. Application schema validation still runs; JSON syntax alone is insufficient. No automatic provider fallback or hidden paid call.

Use an explicit shared JSON Schema subset: objects with `additionalProperties:false`, required properties, strings/numbers/booleans/null, arrays and enum. Apply domain length/numeric checks locally so provider schema differences do not weaken validation. Bound serialized provider request to 128 KiB and response to 1 MiB. Do not log full provider errors/bodies.

FI stores `ai_providers(id, name, kind, base_url, model, secret_ciphertext, secret_version, enabled, config_revision, test_status, test_revision, tested_at, created_by, updated_at)`. Secret persistence uses AES-256-GCM with random nonce and associated data containing provider ID/purpose; `FI_AI_SECRET_KEY` is a base64-encoded 32-byte deployment key. Secret replacement is a distinct action; omitted secret preserves it, explicit clear disables calls requiring it. APIs return only `has_secret` and a non-sensitive suffix, never ciphertext. Missing/wrong key makes encrypted configurations unavailable with a clear admin error; it must not stop normal app startup or drop data. Rotation/re-encryption is a documented maintenance command with backup and validation.

Remote defaults are fixed `https://api.openai.com` and `https://api.anthropic.com`; arbitrary remote endpoints are not accepted in this release. Ollama origins must exactly match deployment `FI_AI_ALLOWED_OLLAMA_ORIGINS` (default `http://127.0.0.1:11434`); Docker host/container addresses are explicit operator additions. Reject URL userinfo, query, fragment and non-root base paths; never follow redirects. The deployment allowlist deliberately authorizes private local destinations, unlike arbitrary user-provided URLs. No metadata service origin can be added from the app UI.

Provider admin API: `GET/POST /api/ai/providers`, `PATCH/DELETE /api/ai/providers/:id`, `POST /api/ai/providers/:id/test`. GET is admin-only. Test uses a synthetic 128-token schema completion, 15-second timeout, no feedback content, one attempt, and reports connection/auth/model/schema capability/latency. Changing kind/model/base URL/secret invalidates previous test. Deletion is blocked while a site policy references the provider; disable first when stopping work. Connection tests count toward provider usage but never call a fallback provider.

Defaults: one worker process, global AI concurrency 2 (deployment maximum 8), timeout 60 seconds (site range 5–120), output limit 1,000 tokens (range 128–4,096), allowed input at most 20,000 Unicode characters. Oversized input fails visibly rather than truncating evidence. Per-site maximum 100 generation attempts/UTC day by default (admin range 1–1,000); attempts including retries reserve a slot atomically. Provider-reported usage is stored or marked unknown; do not invent cost or token usage. Rate/budget exhaustion stops only AI processing and leaves intake available.

## 4. Classification, evidence, knowledge and drafts

Site AI settings are versioned separately from the form: `{revision, enabled, provider_id, generate_on_receipt, allowed_field_ids, include_internal_notes:false, taxonomy, reply_language, policy_text, limits}`. Defaults are disabled, manual generation, no selected fields, no internal notes. An admin explicitly selects the message and any public fields before enabling. Personal fields require an additional explicit choice; sensitive fields, raw IP/UA/signing claims/tokens are never sent to cloud providers. Preserve the same minimization contract for local Ollama. Legacy extras are not implicitly included.

Before inference, replace email addresses, phone-like contact strings, and known configured personal values in selected text with stable per-run placeholders; this is data minimization, not a guarantee to detect every personal fact in prose. Do not send placeholder mappings. Input documents and all user-authored text are untrusted quoted data under a fixed system instruction. Disable tools. Private notes remain excluded unless the site explicitly enables them; UI discloses which fields are sent and to which provider.

Taxonomy: 1–20 admin-defined categories with stable keys plus mandatory `other`; 0–50 allowed tags; proposed owner must be an active member of the same site. Model result schema is `FeedbackAnalysisV1`: summary ≤1,000 chars, language BCP47-like tag ≤35, category key, ≤5 tags, urgency `low|normal|high|urgent`, suggested_owner_id nullable, next_action ≤500, missing_information ≤5 strings, risk_flags subset of `refund|account_change|identity|legal|medical|financial|unsupported_commitment|insufficient_evidence|prompt_injection|other`, evidence ≤12 items `{source_id, start, end, quote}` and draft `{subject, body, knowledge_refs}` or null. No confidence field authorizes sending.

Evidence offsets are Unicode scalar positions into the exact **redacted** source passed to the model. Every quote must match its range and source ID, and the source must still belong to this site/version. Invalid offsets, category, tag, owner or knowledge references produce `invalid_output`, not a silently repaired fact. UI maps a redacted excerpt back only where safe and clearly labels masked text. Do not display hidden chain-of-thought; show short source-supported suggestions.

Add `knowledge_entries` and immutable `knowledge_versions` with title 1–100, plain text ≤10,000, site ID, version, status `draft|approved|retired`, approver and approval time. Up to 50 active entries/site, maximum 5 explicitly selected entries and 20,000 total input characters/run. No crawler, embeddings or model browsing in this slice. Draft responses may cite approved knowledge-version IDs and feedback excerpts; unsupported facts or missing knowledge require review and may yield a clarifying-question draft. Retiring/changing knowledge invalidates affected pending approvals/automatic candidates.

`ai_runs` records feedback revision, form version, provider/config/model, prompt version (`feedback-analysis-v1`), output schema version, redaction-policy revision, input hash, source descriptors, result, usage and status. Recheck current principal/site authority and every policy/config/source/knowledge revision immediately before provider egress and before accepting the result; changed input becomes superseded and never leaks a formerly authorized source. Do not persist a second full raw prompt copy. Classification proposals are records separate from `feedback.status`; `POST /api/feedback/:id/classification/accept` with expected proposal revision applies category/tags/urgency/owner in a transaction and records human overrides. Existing workflow statuses continue unchanged.

`POST /api/feedback/:id/ai-runs` enqueues `analyze_and_draft`; `GET .../ai-runs` returns visible run state; `POST /api/ai-runs/:id/cancel` cancels queued work. Members may use these only for visible sites, within enabled site policy and budgets. Generation on receipt uses the same queue and authority/policy checks, never a separate path. Existing `feedback:read` MCP output does not expand by accident; new web fields use explicitly checked DTOs.

## 5. Durable work and delivery

Add `jobs(id, kind, payload_json, state, unique_key, attempts, max_attempts, available_at, lease_owner, lease_expires_at, last_error_code, created_at, updated_at)`. States are `queued|running|succeeded|failed|cancelled|delivery_unknown`; kinds initially `staff_notification|ai_analysis|visitor_reply`. Unique keys enforce one task per submission/run/reply revision and destination. Intake and initial staff-notification/AI enqueue commit atomically. Staff notification failures remain separate from customer reply results.

The worker claims a bounded batch using a SQLite transaction and conditional lease update; no transaction is held across network I/O. Each claim has an unpredictable attempt token; heartbeat/completion writes compare state, owner and token so a cancelled/stale worker cannot commit a result. Cancellation and the final transition to sending serialize in SQLite: cancellation winning prevents transmission; a send already claimed for transmission is explicitly in-flight and cannot be recalled. Default poll is 1 second, lease 180 seconds, heartbeat 30 seconds, maximum attempts 3. Retry known pre-delivery transient failures/429/5xx with 5s then 30s backoff and bounded jitter (0–1s); honor Retry-After up to 300 seconds. Auth, invalid request, unsupported capability, invalid output, refusal and incomplete responses are terminal unless a user explicitly regenerates. Every attempt is recorded; the admin dead-letter view can retry only eligible failures, creating an audited new attempt. Restart reclaims safe jobs, not ambiguous mail sends.

Create `reply_drafts` with immutable revisions and `outbound_replies` with fixed feedback/site/recipient/subject/body/content hash/approval identity/policy revision/idempotency key/status. Public-facing draft content is separate from notes. Human approval binds exact recipient, content, draft revision and supporting knowledge. Editing creates a new revision and invalidates earlier approval. Subject ≤200 characters and no controls/newlines; body ≤10,000; only plain text plus server-escaped HTML is sent. Recipient is one validated feedback contact address or its verified identity address; the model cannot propose extra recipients/CC/BCC. Changing the contact for sending is a separate human edit and requires fresh approval.

States are exposed separately rather than conflating all records: AI run `queued→generating→needs_review|failed|cancelled`; draft `needs_review→approved|rejected|superseded`; outbound `queued_for_send→sending→sent|failed|delivery_unknown|cancelled`. Approval and enqueue are one transaction. `POST /api/replies/:id/approve-and-send` requires `{expected_revision, content_hash, idempotency_key}` and a visible site member. Repetition returns the same outbound record. Worker rechecks actor active/membership, exact approval hash, current site policy, non-spam/non-deleted feedback, non-test flag, and knowledge validity immediately before sending; stale authority cancels with a visible reason. No missing SMTP configuration can appear as successful delivery.

Extend Mailer with `send_reply(&ReplyMessage) -> SendOutcome`, sharing transport construction without reusing Notice formatting or staff destinations. Each message gets a stable Message-ID and `Auto-Submitted: auto-replied`. SMTP accepted result is `sent`, meaning accepted by SMTP, not delivered/read. SMTP outcome after DATA timeout, process death with a reply in sending, or acceptance followed by result-write failure is `delivery_unknown`; never automatically resend. A UI reconciliation action can mark known sent or create a new manually approved send attempt, acknowledging possible duplicate delivery. A timeout before any message delivery can safely retry. Delivery implementations must distinguish these stages conservatively.

Append-only `workflow_audit` records actor or system policy, site/entity IDs, action, revision/hash, provider/policy/knowledge versions, result code and timestamp; no credentials or duplicate content body. Site/feedback deletion cancels pending jobs and removes content-bearing run/draft/reply data under existing delete semantics; minimal non-content audit events retain tombstone IDs without contact data. Human/system actions are visually distinct.

## 6. Actual automatic-reply policy

Automatic **generation** and automatic **sending** are separate switches. Default mode is `review_required`. Deployment switch `FI_AI_AUTO_SEND_ENABLED=false` is a global emergency gate; admin UI explains that operator enablement is needed. Once enabled, a site admin may configure `auto_template` mode. Freeform model drafts always require human review in this release; automatic sends use an approved template rendered deterministically.

An automatic rule is `{id, enabled, form_field_id, allowed_values, analysis_categories, template_version_id, priority}`. It must match an explicit published `select` field with 1–10 exact values; no regex, arbitrary code or full-text keyword match. The selected category must also be in its 1–10 approved category keys, but model classification alone can never match a rule. Evaluate at most 20 rules/site, ordered by priority then ID; multiple matching rules are a review fallback rather than an arbitrary choice. Keep `other` outside auto categories.

Templates have immutable versions, type `receipt|approved_information`, plain-text subject/body, explicit approved knowledge-version references, and a human admin approval record. Allowed interpolation is fixed `site_name`, `feedback_reference`, `received_date` and `contact_name` (plain escaped text); free-form feedback/message/custom text cannot be interpolated. Subject/body use the same reply limits. Receipt templates may acknowledge intake without a factual promise of action or timing; approved-information templates require at least one approved knowledge version. Template editing produces a draft and invalidates rule approval until reapproved. Configuration activation shows the rendered sample, exact matching form values, provider, recipient requirement, quotas and responsible admin, and records the activation revision.

All gates must pass at enqueue and again before send:

1. Global and site switches are enabled; site is active; policy/template/knowledge revisions still match.
2. Feedback is new, not spam/test/deleted, less than 24 hours old, and has no prior customer reply or pending automatic reply.
3. A trusted signed backend assertion attests the exact recipient email as verified; claimed member ID or typed email alone is insufficient. Anonymous hosted submissions remain eligible for reviewed drafts.
4. The form explicitly includes a checked `reply_requested` checkbox. No reply is sent merely because an email address exists.
5. Exactly one deterministic rule matches; a completed valid analysis has the allowed category and no risk flags. Refunds, identity/account changes, legal/medical/financial issues, unsupported commitments, insufficient evidence, and injection suspicions route to review.
6. The exact template/knowledge versions were human-approved and the policy's approving admin is still active. No model-generated freeform text enters the outgoing message.
7. Atomic quotas allow it: default 20 automatic replies/site/UTC day, configurable 1–200; one per normalized recipient/site/rolling 24 hours; one per feedback. Failed known-pre-send attempts release the reserved daily slot; sent/unknown retain it. Denied gates explain the review reason, not discard the feedback.

`GET/PUT /api/sites/:id/reply-policy` is admin-only; PUT takes expected revision. `POST .../reply-policy/activate` requires a current successful provider test, valid template/knowledge/rule preview, SMTP configured, identity key, reply_requested checkbox and explicit sample confirmation; activation does not send existing feedback. `POST .../reply-policy/pause` atomically disables and cancels queued auto-sends. In-flight SMTP cannot be recalled; UI states this accurately. Resuming starts only on future submissions. Provider/SMTP outage, quota exhaustion, stale revision or any uncertain evidence falls back to review. Admin settings include a synthetic dry-run that reports each gate and never delivers.

This is a usable, bounded automatic-reply feature, not a promise of a later generic agent. The recipient trust requirement and deterministic template restriction are product limits shown in setup and documentation.

## 7. Interface and acceptance

Site Detail tabs: **表單**, **發布與嵌入**, **AI 協助**, **通知與負責人**. Form editing uses ordered field rows and a paper preview, keyboard reorder, explicit draft/published labels and stale-version resolution. Integration tabs distinguish sharing/iframe/HTML/JavaScript/backend without exposing implementation jargon to ordinary visitors.

Admin settings add provider cards with masked credential, model, last actual test, availability and limits; policy editor shows selected export fields and testable rules. Feedback keeps original content primary, source-backed suggestion stamps to the side, reply composer below, and internal notes clearly separate. Inbox adds category/urgency and **待審草稿** filters. A review queue shows source excerpts, recipients, knowledge versions, fallback reason, edit/reject/approve-and-send controls and real delivery status. Support desktop/mobile, light/dark, keyboard operation and reduced motion using existing tokens/components.

Acceptance runs on disposable legacy DB copies and a real two-origin browser fixture: migration preservation; stale publish conflicts; historical schema rendering; required custom fields; snippet parity; public/embedded HTML success/errors; exact-origin JSON and denial; null/no-Origin handling; idempotent retries; signed identity replay/cross-site/expiry; secrets absent from UI/logs; three fixture-provider transports; schema/refusal/timeout/minimization/usage; job restart and quota races; stale draft approval; current membership; staff versus visitor delivery; local Mailpit human and auto-template delivery; duplicate and ambiguous SMTP cases; pause in queue; no automation on test submissions; safe fallback for every auto gate. Existing auth/registration/access/MCP tests remain mandatory.

## Primary references

- [OpenAI structured model outputs](https://developers.openai.com/api/docs/guides/structured-outputs): Responses structured output and refusal/incomplete handling.
- [Claude structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs): `output_config.format` and schema limitations.
- [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs): local `/api/chat` schema format; cloud support is not assumed.
- [OpenAI authentication](https://developers.openai.com/api/reference/overview#authentication): API keys are distinct from ChatGPT/Codex login sessions.

Provider models and capability claims are tested against the selected deployment configuration; no fixed model name or availability claim is baked into this design.
