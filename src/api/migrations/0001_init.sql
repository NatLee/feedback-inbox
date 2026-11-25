CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT    NOT NULL,
    -- 收通知信的信箱；空字串表示不收
    email         TEXT    NOT NULL DEFAULT '',
    password_hash TEXT    NOT NULL,
    role          TEXT    NOT NULL DEFAULT 'member' CHECK (role IN ('admin', 'member')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT    NOT NULL
);

-- 只存 session token 的 SHA-256
CREATE TABLE sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);

-- 一個網站（或一張表單）一筆；表單送到 /f/<key>
CREATE TABLE sites (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL UNIQUE,
    url             TEXT    NOT NULL DEFAULT '',
    key             TEXT    NOT NULL UNIQUE,
    -- JSON 陣列：允許從哪些 origin 用瀏覽器送出；空陣列是不限制
    allowed_origins TEXT    NOT NULL DEFAULT '[]',
    -- JSON 陣列：name / email / phone / company / message 裡哪些必填
    required_fields TEXT    NOT NULL DEFAULT '["name","email","message"]',
    -- 用 HTML 表單送出之後要轉去的網址；空字串是用內建的感謝頁
    redirect_url    TEXT    NOT NULL DEFAULT '',
    webhook_url     TEXT    NOT NULL DEFAULT '',
    notify_email    INTEGER NOT NULL DEFAULT 1,
    active          INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT    NOT NULL
);

-- 誰負責哪個網站：看得到它的訊息，也會收到通知信
CREATE TABLE site_members (
    site_id INTEGER NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (site_id, user_id)
);
CREATE INDEX site_members_user ON site_members (user_id);

CREATE TABLE feedback (
    id         INTEGER PRIMARY KEY,
    site_id    INTEGER NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL DEFAULT '',
    email      TEXT    NOT NULL DEFAULT '',
    phone      TEXT    NOT NULL DEFAULT '',
    company    TEXT    NOT NULL DEFAULT '',
    message    TEXT    NOT NULL DEFAULT '',
    -- JSON 陣列：[{"name": "...", "value": "..."}]，表單上其他的欄位
    extra      TEXT    NOT NULL DEFAULT '[]',
    status     TEXT    NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'in_progress', 'done', 'spam')),
    ip         TEXT    NOT NULL DEFAULT '',
    user_agent TEXT    NOT NULL DEFAULT '',
    -- 表單所在的頁面（Referer）
    page_url   TEXT    NOT NULL DEFAULT '',
    created_at TEXT    NOT NULL,
    updated_at TEXT    NOT NULL
);
CREATE INDEX feedback_site ON feedback (site_id, created_at);
CREATE INDEX feedback_status ON feedback (status);
CREATE INDEX feedback_created ON feedback (created_at);

-- 處理紀錄：kind = note 是內部備註，status 是狀態變更
CREATE TABLE notes (
    id          INTEGER PRIMARY KEY,
    feedback_id INTEGER NOT NULL REFERENCES feedback (id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users (id),
    kind        TEXT    NOT NULL DEFAULT 'note' CHECK (kind IN ('note', 'status')),
    body        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL
);
CREATE INDEX notes_feedback ON notes (feedback_id);

-- 每一次通知的結果
CREATE TABLE deliveries (
    id          INTEGER PRIMARY KEY,
    feedback_id INTEGER NOT NULL REFERENCES feedback (id) ON DELETE CASCADE,
    channel     TEXT    NOT NULL CHECK (channel IN ('email', 'webhook')),
    target      TEXT    NOT NULL,
    ok          INTEGER NOT NULL,
    detail      TEXT    NOT NULL DEFAULT '',
    created_at  TEXT    NOT NULL
);
CREATE INDEX deliveries_feedback ON deliveries (feedback_id);
