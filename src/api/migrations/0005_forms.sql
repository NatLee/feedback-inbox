ALTER TABLE sites ADD COLUMN origin_mode TEXT NOT NULL DEFAULT 'exact' CHECK(origin_mode IN ('legacy_public','exact'));
UPDATE sites SET origin_mode='legacy_public' WHERE json_array_length(allowed_origins)=0;
ALTER TABLE sites ADD COLUMN embed_origins TEXT NOT NULL DEFAULT '[]';
CREATE TABLE form_versions (
 id INTEGER PRIMARY KEY, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 version INTEGER NOT NULL, schema_json TEXT NOT NULL, schema_hash TEXT NOT NULL,
 published_by INTEGER REFERENCES users(id) ON DELETE SET NULL, published_at TEXT NOT NULL,
 retired_at TEXT, UNIQUE(site_id,version)
);
CREATE TRIGGER form_versions_immutable BEFORE UPDATE OF site_id,version,schema_json,schema_hash ON form_versions BEGIN SELECT RAISE(ABORT,'published form versions are immutable'); END;
ALTER TABLE sites ADD COLUMN published_form_version_id INTEGER REFERENCES form_versions(id) ON DELETE SET NULL;
CREATE TABLE form_drafts (
 site_id INTEGER PRIMARY KEY REFERENCES sites(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL DEFAULT 1, schema_json TEXT NOT NULL,
 updated_by INTEGER REFERENCES users(id) ON DELETE SET NULL, updated_at TEXT NOT NULL
);
ALTER TABLE feedback ADD COLUMN form_version_id INTEGER REFERENCES form_versions(id) ON DELETE SET NULL;
ALTER TABLE feedback ADD COLUMN answers_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE feedback ADD COLUMN identity_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE feedback ADD COLUMN submission_mode TEXT NOT NULL DEFAULT 'legacy';
ALTER TABLE feedback ADD COLUMN is_test INTEGER NOT NULL DEFAULT 0;
CREATE TABLE submission_receipts (
 site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE, submission_key TEXT NOT NULL,
 request_hash TEXT NOT NULL, feedback_id INTEGER REFERENCES feedback(id) ON DELETE SET NULL,
 expires_at INTEGER NOT NULL, PRIMARY KEY(site_id,submission_key)
);
CREATE TABLE verification_sessions (
 id INTEGER PRIMARY KEY, site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
 token_hash TEXT NOT NULL UNIQUE, expires_at INTEGER NOT NULL, received INTEGER NOT NULL DEFAULT 0,
 last_received_at TEXT, created_by INTEGER REFERENCES users(id) ON DELETE SET NULL
);
CREATE TABLE identity_keys (site_id INTEGER PRIMARY KEY REFERENCES sites(id) ON DELETE CASCADE, ciphertext TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE identity_nonces (site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,jti TEXT NOT NULL,expires_at INTEGER NOT NULL,PRIMARY KEY(site_id,jti));
CREATE TABLE workflow_audit (
 id INTEGER PRIMARY KEY, site_id INTEGER REFERENCES sites(id) ON DELETE SET NULL,
 actor_id INTEGER, entity_type TEXT NOT NULL, entity_id TEXT NOT NULL,
 action TEXT NOT NULL, metadata_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
);
CREATE INDEX workflow_audit_site ON workflow_audit(site_id,id);
