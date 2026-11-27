# Forms and AI UI contract

All endpoints below are under `/api`, return JSON and use existing cookie/same-origin protection. Errors use the existing `error` string plus optional stable `code`. Admin-only writes are identified below; members can view their assigned sites and review individual feedback. The interface must keep ordinary internal notes separate from customer replies. No saved secret is returned.

## Existing forms integration (implemented)

- `GET /sites/:id/identity-key` (admin): `{configured,encryption_ready,example}`.
- `POST /sites/:id/identity-key/rotate` (admin): `{secret,algorithm,ttl_seconds,example}`. Display secret once in current component state with copy control; never localStorage/URL. Rotation immediately invalidates old tokens. Missing deployment encryption returns 503.
- `POST /sites/:id/verification-sessions` (admin): `{id,token,expires_at,max_receipts:5}`. Token belongs only in `_test_token` POST body. 10 minute lifetime. Display a generated integration example with the currently published fields plus this control; synthetic submissions do not notify or trigger AI.
- `GET /sites/:id/verification-sessions/:session`: `{id,expires_at,received,last_received_at}`. Poll while visible/unexpired, display actual server receipts. This does not prove CORS succeeded in another browser.
- `GET /feedback/:id` now includes `form:{version,schema}|null`, `answers:{[fieldId]:value}`, `identity:{subject?,email?,email_verified?,verified_at?}`, `submission_mode`, `is_test`. Render labels from stored form schema, never current draft. Ordinary contact email is unverified unless identity.email_verified is true. `GET /feedback?test_only=true` explicitly lists synthetic rows; default hides them.

## Providers (admin)

`GET /ai/providers` → `{items:Provider[],encryption_ready,allowed_ollama_origins,auto_send_enabled,smtp_ready}`.
`Provider={id,name,kind:'openai'|'anthropic'|'ollama',base_url,model,enabled,revision,has_credential,tested_revision:number|null,last_test_error:string|null}`.
`POST /ai/providers` body `{name,kind,base_url,model,enabled:false,credential?:string}`.
`PUT /ai/providers/:id` body same plus `expected_revision`; omitted credential preserves existing, empty clears. Kind cannot change. Edit invalidates connection test.
`POST /ai/providers/:id/test` body `{expected_revision}` → updated Provider. Uses synthetic data only; explicit click can incur provider charge. Do not auto-test or auto-enable. OpenAI label “OpenAI Platform API Key”. OpenAI/Anthropic origins fixed; Ollama must be deployment allowlisted.

## Site AI configuration (admin writes)

`GET /sites/:id/ai` → `{settings,providers,fields,members,capabilities}`.
`settings={revision,enabled:false,provider_id:number|null,generate_on_receipt:false,field_ids:string[],include_personal:false,categories:string[],tags:string[],daily_attempt_limit:100,timeout_seconds:60,max_output_tokens:1000,auto_mode:'off'|'auto_template',auto_daily_limit:20,auto_activated_at:number|null}`.
`fields` is published schema fields. `members` is visible staff `{id,display_name}`. `capabilities={encryption_ready,smtp_ready,auto_send_enabled}`.
`PUT /sites/:id/ai` body settings plus `expected_revision` (ignore read-only revision/auto_activated_at); returns settings. AI default disabled, field allowlist empty. Sensitive fields cannot be selected. Personal fields require explicit include_personal. Daily limits UTC. Auto mode is separate from generation, requires deployment kill switch, SMTP and a tested provider; activation applies only to future submissions. Pause cancels queued auto replies; cannot recall transmission already started.

## Approved knowledge and templates

`GET /sites/:id/ai/knowledge` → `{items:Knowledge[]}`.
`Knowledge={id,revision,title,body,state:'draft'|'approved'|'retired',created_at}`.
`POST /sites/:id/ai/knowledge` body `{title,body}` → Knowledge.
`PUT /sites/:id/ai/knowledge/:item` body `{expected_revision,title,body}` → new immutable revision, state draft.
`POST /sites/:id/ai/knowledge/:item/approve` / `.../retire` body `{expected_revision}` → Knowledge.
All writes admin. Editing invalidates approval; show approved reference version.

`GET /sites/:id/ai/templates` → `{items:Template[]}`.
`Template={id,revision,name,subject,body,state:'draft'|'approved'|'retired',knowledge_ids:number[]}`.
Create/update/approve/retire paths mirror knowledge. Template body accepts only `{site_name}`, `{reference}`, `{category}` placeholders. Never interpolate visitor text. knowledge_ids reference approved knowledge revision IDs. A zero-reference acknowledgement must use subject `[{site_name}] 已收到您的訊息` and body `我們已收到您的訊息（編號 {reference}），工作人員會盡快回覆。`; offer a one-click preset. All other templates must cite approved knowledge.

`GET /sites/:id/ai/rules` → `{items:Rule[]}`.
`Rule={id,name,enabled,field_id,equals:string,category,template_id:number}` where template_id references one approved template revision.
`POST /sites/:id/ai/rules` or `PUT .../:rule` accepts Rule fields. DELETE removes rule. Admin only. Match is deterministic published select field exact value AND validated analysis category. Multiple matches fall back to review. Auto requires verified identity email, true reply_requested checkbox, new feedback <24h, no risk flags, approved versions, no previous reply, quota and policy checks. Freeform model drafts always require human approval.
`POST /sites/:id/ai/auto/dry-run` body `{feedback_id}` → `{eligible:boolean,gate:string,explanation:string}`; never sends.

## Feedback analysis, proposals and reply workflow

`GET /feedback/:id/ai` → `{runs,proposal,draft,outbound:Outbound[],auto_decision}`. Poll when queued/running/sending.
`Run={id,state:'queued'|'running'|'succeeded'|'failed'|'cancelled'|'superseded',error_code?,error_message?,provider_id,model?,created_at,output?:Analysis}`.
`Analysis={summary,language,category,tags:string[],urgency:'low'|'normal'|'high',owner_id:number|null,next_action,missing_information:string[],risk_flags:string[],evidence:[{field_id,start,end,quote}],draft:{subject,body,knowledge_ids:number[]}|null}`. Evidence indices are Unicode scalar offsets into the redacted source; render quote/source identity, do not reconstruct from unredacted text.
`proposal={id,state:'pending'|'accepted'|'rejected'|'superseded',revision,category,tags,urgency,owner_id}`.
`POST /feedback/:id/ai/analyze` body `{}` → `{run_id,job_id}`.
`POST /ai/runs/:run/cancel` body `{}` → `{ok:true}`.
`POST /feedback/:id/ai/classification` body `{expected_revision,decision:'accept'|'reject',category?,tags?,urgency?,owner_id?}` → proposal. Accept applies classification only; it never changes inbox workflow status.

`Draft={id,revision,state:'draft'|'rejected'|'approved'|'superseded',recipient,subject,body,content_hash,knowledge_ids:number[],origin:'human'|'ai',created_at}`.
`POST /feedback/:id/ai/draft` body `{recipient,subject,body,knowledge_ids:[]}` → Draft; works without AI provider for human replies.
`PUT /feedback/:id/ai/draft` body same + `{expected_revision}` → next Draft. Editing invalidates approval. Default recipient from contact but always visible and editable. Subject <=200 chars, body <=10,000, one email recipient only.
`POST /feedback/:id/ai/draft/reject` body `{expected_revision}` → Draft.
`POST /feedback/:id/ai/send` body `{expected_revision,content_hash,idempotency_key}` → Outbound. A clear explicit “核准並寄出” action previews exact recipient and saved content. Double clicks reuse idempotency key. No send by opening view.
`Outbound={id,state:'queued'|'sending'|'sent'|'failed'|'cancelled'|'delivery_unknown',mode:'human'|'auto_template',recipient,subject,body,created_at,sent_at?,error_code?,message_id}`.
`POST /ai/outbound/:id/cancel` body `{}` → Outbound. Only queued may be cancelled atomically. Unknown must show “寄送結果不明；請先到郵件服務確認，系統不會自動重寄”. No blind retry control.
`POST /ai/outbound/:id/reconcile` admin body `{resolution:'confirmed_sent'|'confirmed_not_sent',note}` → Outbound; explicit audit record, no automatic send.
`GET /ai/review` → `{items:[{feedback_id,site_id,site_name,sender,created_at,draft_state,run_state,outbound_state,fallback_reason}]}` site-scoped queue. Link actual feedback detail. No bulk send.

## Work records and filters

`GET /sites/:id/ai/jobs` returns the latest 100 safe job records `{id,kind,state,attempt,error_code,created_at}` for a visible site; no job payload or credentials. Inbox query adds `category`, `urgency`, `review_only`, `test_only`; ordinary mode excludes test rows. Publication advances the draft revision as well as the immutable version.

## UI ownership and visual direction

Frontend worker owns only `src/web/src` plus frontend tests and docs/screenshots. Backend worker owns Rust/migrations/operations. Add provider controls to existing Settings, AI tab to SiteDetail, analysis + distinct customer reply section to FeedbackDetail, review queue route/navigation. Reuse cream paper/vermilion editorial correspondence styling, calm stamped state labels, clear exact recipient, no decorative futuristic AI styling. Responsive, accessible labels, keyboard focus and meaningful empty/error states. Add tests for provider secret retention, explicit enable/test, member-only view, draft edits invalidating approval, unknown delivery display, actual synthetic receipt polling and no auto-send on render.
