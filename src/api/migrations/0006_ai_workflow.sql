CREATE TABLE ai_providers (
 id INTEGER PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('openai','anthropic','ollama')),
 base_url TEXT NOT NULL, model TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0,
 revision INTEGER NOT NULL DEFAULT 1, credential_ciphertext TEXT, tested_revision INTEGER,
 last_test_error TEXT, created_at TEXT NOT NULL
);
CREATE TABLE site_ai_settings (
 site_id INTEGER PRIMARY KEY REFERENCES sites(id) ON DELETE CASCADE, revision INTEGER NOT NULL DEFAULT 1,
 settings_json TEXT NOT NULL, updated_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
 auto_activated_at INTEGER, activation_feedback_id INTEGER, updated_at TEXT NOT NULL
);
CREATE TABLE workflow_jobs (
 id INTEGER PRIMARY KEY, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('analysis','reply','notify_email','notify_webhook')),
 payload_json TEXT NOT NULL, unique_key TEXT NOT NULL UNIQUE,
 state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','running','succeeded','failed','cancelled','delivery_unknown')),
 attempt INTEGER NOT NULL DEFAULT 0, token TEXT, lease_until INTEGER, available_at INTEGER NOT NULL,
 error_code TEXT, created_at TEXT NOT NULL
);
CREATE INDEX workflow_jobs_ready ON workflow_jobs(state,available_at,id);
CREATE TABLE ai_knowledge (
 id INTEGER PRIMARY KEY, entry_id INTEGER, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL DEFAULT 1, title TEXT NOT NULL, body TEXT NOT NULL,
 state TEXT NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','approved','retired')),
 approved_by INTEGER REFERENCES users(id) ON DELETE SET NULL, created_at TEXT NOT NULL
);
CREATE TRIGGER ai_knowledge_immutable BEFORE UPDATE OF title,body,site_id,revision ON ai_knowledge BEGIN SELECT RAISE(ABORT,'knowledge revisions are immutable'); END;
CREATE TABLE ai_templates (
 id INTEGER PRIMARY KEY, entry_id INTEGER, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL DEFAULT 1, name TEXT NOT NULL, subject TEXT NOT NULL, body TEXT NOT NULL,
 knowledge_ids_json TEXT NOT NULL DEFAULT '[]',state TEXT NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','approved','retired')),
 approved_by INTEGER REFERENCES users(id) ON DELETE SET NULL, created_at TEXT NOT NULL
);
CREATE TRIGGER ai_templates_immutable BEFORE UPDATE OF name,subject,body,knowledge_ids_json,site_id,revision ON ai_templates BEGIN SELECT RAISE(ABORT,'template revisions are immutable'); END;
CREATE TABLE ai_rules (
 id INTEGER PRIMARY KEY, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0, field_id TEXT NOT NULL,
 equals_value TEXT NOT NULL, category TEXT NOT NULL, template_id INTEGER NOT NULL REFERENCES ai_templates(id)
);
CREATE TABLE ai_runs (
 id INTEGER PRIMARY KEY, feedback_id INTEGER NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
 site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE, actor_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
 job_id INTEGER REFERENCES workflow_jobs(id) ON DELETE SET NULL, provider_id INTEGER NOT NULL REFERENCES ai_providers(id),
 provider_revision INTEGER NOT NULL, policy_revision INTEGER NOT NULL, source_hash TEXT NOT NULL,
 input_json TEXT NOT NULL, input_hash TEXT NOT NULL, knowledge_ids_json TEXT NOT NULL DEFAULT '[]',
 state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','running','succeeded','failed','cancelled','superseded')),
 output_json TEXT, model TEXT, error_code TEXT, error_message TEXT, usage_json TEXT, duration_ms INTEGER,
 created_at TEXT NOT NULL
);
CREATE INDEX ai_runs_feedback ON ai_runs(feedback_id,id);
CREATE TABLE ai_daily_attempts (site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE, day TEXT NOT NULL,attempts INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(site_id,day));
CREATE TABLE classification_proposals (
 id INTEGER PRIMARY KEY,feedback_id INTEGER NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
 run_id INTEGER NOT NULL REFERENCES ai_runs(id) ON DELETE CASCADE,revision INTEGER NOT NULL DEFAULT 1,
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','accepted','rejected','superseded')),
 category TEXT NOT NULL,tags_json TEXT NOT NULL,urgency TEXT NOT NULL,owner_id INTEGER REFERENCES users(id) ON DELETE SET NULL
);
ALTER TABLE feedback ADD COLUMN category TEXT NOT NULL DEFAULT '';
ALTER TABLE feedback ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE feedback ADD COLUMN urgency TEXT NOT NULL DEFAULT 'normal';
ALTER TABLE feedback ADD COLUMN owner_id INTEGER REFERENCES users(id) ON DELETE SET NULL;
CREATE TABLE reply_drafts (
 id INTEGER PRIMARY KEY,feedback_id INTEGER NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL,origin TEXT NOT NULL CHECK(origin IN ('human','ai')),run_id INTEGER REFERENCES ai_runs(id) ON DELETE SET NULL,
 state TEXT NOT NULL DEFAULT 'draft' CHECK(state IN ('draft','approved','rejected','superseded')),
 recipient TEXT NOT NULL,subject TEXT NOT NULL,body TEXT NOT NULL,content_hash TEXT NOT NULL,
 knowledge_ids_json TEXT NOT NULL DEFAULT '[]',source_hash TEXT NOT NULL,
 created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,created_at TEXT NOT NULL, UNIQUE(feedback_id,revision)
);
CREATE TRIGGER reply_drafts_immutable BEFORE UPDATE OF recipient,subject,body,content_hash,knowledge_ids_json,source_hash,revision ON reply_drafts BEGIN SELECT RAISE(ABORT,'reply revisions are immutable'); END;
CREATE TABLE outbound_replies (
 id INTEGER PRIMARY KEY,feedback_id INTEGER NOT NULL REFERENCES feedback(id) ON DELETE CASCADE,
 site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,job_id INTEGER REFERENCES workflow_jobs(id) ON DELETE SET NULL,
 mode TEXT NOT NULL CHECK(mode IN ('human','auto_template')),state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','sending','sent','failed','cancelled','delivery_unknown')),
 draft_id INTEGER REFERENCES reply_drafts(id),template_id INTEGER REFERENCES ai_templates(id),rule_id INTEGER REFERENCES ai_rules(id),
 actor_id INTEGER REFERENCES users(id) ON DELETE SET NULL,policy_revision INTEGER,provider_revision INTEGER,
 source_hash TEXT NOT NULL,recipient TEXT NOT NULL,subject TEXT NOT NULL,body TEXT NOT NULL,
 knowledge_ids_json TEXT NOT NULL DEFAULT '[]',content_hash TEXT NOT NULL,message_id TEXT NOT NULL UNIQUE,
 idempotency_key TEXT NOT NULL,request_hash TEXT NOT NULL,created_at TEXT NOT NULL,sent_at TEXT,error_code TEXT,
 UNIQUE(site_id,idempotency_key)
);
CREATE UNIQUE INDEX outbound_feedback_active ON outbound_replies(feedback_id) WHERE state IN ('queued','sending','sent','delivery_unknown');
CREATE TABLE auto_reply_decisions (feedback_id INTEGER PRIMARY KEY REFERENCES feedback(id) ON DELETE CASCADE,gate TEXT NOT NULL,explanation TEXT NOT NULL,created_at TEXT NOT NULL);
CREATE TABLE auto_daily_quota (site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,day TEXT NOT NULL,sends INTEGER NOT NULL,PRIMARY KEY(site_id,day));
CREATE TABLE auto_recipient_quota (site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,recipient_hash TEXT NOT NULL,reserved_at INTEGER NOT NULL,PRIMARY KEY(site_id,recipient_hash));
CREATE TRIGGER workflow_audit_append_only_update BEFORE UPDATE OF actor_id,entity_type,entity_id,action,metadata_json,created_at ON workflow_audit BEGIN SELECT RAISE(ABORT,'audit is append-only'); END;
-- Allow FK SET NULL when a site is removed, but prevent application deletion of audit rows.
CREATE TRIGGER workflow_audit_append_only_delete BEFORE DELETE ON workflow_audit BEGIN SELECT RAISE(ABORT,'audit is append-only'); END;

CREATE TRIGGER outbound_content_immutable BEFORE UPDATE OF recipient,subject,body,content_hash,source_hash,knowledge_ids_json,message_id,policy_revision,provider_revision ON outbound_replies BEGIN SELECT RAISE(ABORT,'outbound snapshots are immutable'); END;
