# API

兩組網址：

- `/f/<key>`：收表單的公開網址，不用登入，別的網站可以直接送過來。
- `/api/...`：管理介面用的 API，要先登入（HttpOnly cookie `fi_session`）。

錯誤一律是 `{"error": "給人看的訊息"}` 加上對應的 HTTP 狀態碼。時間都是 UTC 的 RFC 3339（`2026-01-05T08:30:00Z`）。

## 收表單

### `POST /f/<key>`

`Content-Type` 可以是 `application/json` 或 `application/x-www-form-urlencoded`（一般的 HTML 表單），內容最多 64 KB。

| 欄位 | 說明 |
|---|---|
| `name`、`email`、`phone`、`company`、`message` | 認得的欄位。哪幾個必填在網站設定裡選（預設 `name`、`email`、`message`）；`email` 有填就會檢查格式 |
| `_gotcha` | 藏起來給機器人填的欄位。有值就當成垃圾訊息：照樣回成功，但不存、不通知 |
| 其他 | 最多 30 個，原樣留在訊息裡（名稱最多 100 字、值最多 2000 字）|

回應：

- 用 JSON 送，或 `Accept` 裡有 `application/json`：`200 {"ok": true}`，失敗是 `{"error": "..."}`。
- 一般的 HTML 表單：`303` 轉到網站設定的「送出後轉去的網址」，沒設就轉到內建的感謝頁 `/f/<key>/thanks`；失敗時回一頁說明。

| 狀態碼 | 什麼時候 |
|---|---|
| `400` | 少了必填欄位、信箱格式不對、內容讀不懂 |
| `403` | 網站設了「允許的來源」而 `Origin` 不在裡面；或這張表單已經停收 |
| `404` | 沒有這個 key（換過表單網址之後，舊的也是 404）|
| `413` / `415` | 內容太長／不是上面兩種 `Content-Type` |
| `429` | 同一個 IP 送得太頻繁（`FI_RATE_LIMIT`，預設一分鐘 10 次）|

跨站：沒設「允許的來源」時回 `Access-Control-Allow-Origin: *`；有設的話只對名單裡的來源回它自己。`OPTIONS /f/<key>` 處理 preflight。沒有 `Origin` 標頭的請求（伺服器之間、命令列）不受來源限制。

```bash
curl -H 'content-type: application/json' \
  -d '{"name":"王小明","email":"ming@example.org","message":"想了解報價","方案":"企業版"}' \
  https://inbox.example.com/f/<key>
```

### `GET /f/<key>`

一張現成的表單頁，照網站設定的必填欄位標示。沒有自己的網頁時可以直接把這個連結給人填。

## Webhook

網站設了 webhook 網址的話，每一則新訊息會 `POST` 一份 JSON 過去（15 秒沒回應算失敗，結果記在訊息的「通知」裡，不會重送）。

網址是 Slack（`hooks.slack.com`）或 Discord（`discord.com/api/webhooks`）的 incoming webhook 時，自動改用它們的格式。其他網址收到的是：

```json
{
  "event": "feedback.created",
  "site": "官方網站",
  "feedback": {
    "id": 42,
    "name": "王小明",
    "email": "ming@example.org",
    "phone": "",
    "company": "",
    "message": "想了解報價",
    "extra": [{ "name": "方案", "value": "企業版" }],
    "created_at": "2026-01-05T08:30:00Z"
  },
  "url": "https://inbox.example.com/inbox/42"
}
```

`url` 要有設 `FI_PUBLIC_URL` 才有，否則是 `null`。

## 管理 API

除了 `health`、`meta`、`auth/state`、`auth/setup`、`auth/login` 之外都要登入，沒登入回 `401`。會改資料的請求如果帶了 `Origin` 而且不是本站，回 `403`。

「管理員」欄打勾的只有 `admin` 能用。成員（`member`）只看得到自己負責的網站和那些網站的訊息。

### 基本

| 方法 | 路徑 | 說明 |
|---|---|---|
| `GET` | `/api/health` | 回 `ok` |
| `GET` | `/api/meta` | `{version, smtp_configured, public_url}` |

### 登入和帳號

| 方法 | 路徑 | 管理員 | 說明 |
|---|---|---|---|
| `GET` | `/api/auth/state` | | `{setup_required, user}`；`setup_required` 是還沒有任何帳號 |
| `POST` | `/api/auth/setup` | | 建立第一個管理員並登入：`{username, password, display_name?, email?}`。已經有帳號之後回 `409` |
| `POST` | `/api/auth/login` | | `{username, password}`，回使用者並設定 cookie。同一個帳號連續失敗太多次會暫時鎖住 |
| `POST` | `/api/auth/logout` | | |
| `PATCH` | `/api/auth/me` | | `{display_name?, email?}`；`email` 是收通知信的信箱 |
| `POST` | `/api/auth/password` | | `{current_password, new_password}`，其他裝置上的登入會一起登出 |
| `GET` | `/api/users` | ✓ | 所有帳號，含 `disabled`、`site_count` |
| `POST` | `/api/users` | ✓ | `{username, password, role, display_name?, email?}`，`role` 是 `admin` 或 `member` |
| `PATCH` | `/api/users/<id>` | ✓ | `{display_name?, email?, role?, disabled?, password?}`；不能改自己的角色或停用自己 |

密碼至少 8 個字元，用 Argon2 雜湊。

### 網站

| 方法 | 路徑 | 管理員 | 說明 |
|---|---|---|---|
| `GET` | `/api/sites` | | 看得到的網站，含 `total`、`unhandled`、`members` |
| `POST` | `/api/sites` | ✓ | 至少要 `name`，其他欄位同下 |
| `GET` | `/api/sites/<id>` | | |
| `PATCH` | `/api/sites/<id>` | ✓ | 只改有給的欄位 |
| `DELETE` | `/api/sites/<id>` | ✓ | 連同底下的訊息一起刪 |
| `POST` | `/api/sites/<id>/rotate-key` | ✓ | 換一個表單網址，舊的立刻失效 |
| `POST` | `/api/sites/<id>/test` | ✓ | 用存好的設定送一則測試通知，回 `{outcomes, smtp_configured, no_recipients}` |

可以改的欄位：

| 欄位 | 說明 |
|---|---|
| `name` | 1 到 60 個字元，不能重複 |
| `url` | 網站的網址，可以空著 |
| `required_fields` | `name`、`email`、`phone`、`company`、`message` 裡的幾個 |
| `allowed_origins` | 只收這些來源送來的表單，例如 `["https://www.example.com"]`；空的就不限制。最多 20 個 |
| `redirect_url` | HTML 表單送出之後轉去哪裡 |
| `active` | `false` 就停收 |
| `member_ids` | 負責這個網站的帳號 |
| `notify_email` | 有新訊息時要不要寄信給負責人 |
| `webhook_url` | 只有管理員讀得到；其他人只會看到 `has_webhook` |

### 訊息

| 方法 | 路徑 | 管理員 | 說明 |
|---|---|---|---|
| `GET` | `/api/feedback` | | 清單，回 `{items, total, page, per_page}` |
| `GET` | `/api/feedback/export` | | 同樣的篩選條件，匯出 CSV（最多 50,000 列）。多一個 `tz_offset`（比 UTC 快幾分鐘，台北是 `480`）決定時間怎麼顯示 |
| `POST` | `/api/feedback/bulk` | | `{ids, status}`，一次最多 200 則，回 `{updated}` |
| `GET` | `/api/feedback/<id>` | | 完整內容，含 `extra`、`notes`、`deliveries`（通知有沒有送出去）|
| `PATCH` | `/api/feedback/<id>` | | `{status}`，會在處理紀錄裡留一筆 |
| `DELETE` | `/api/feedback/<id>` | ✓ | |
| `POST` | `/api/feedback/<id>/notes` | | `{body}`，最多 2000 字 |
| `DELETE` | `/api/notes/<id>` | | 自己寫的備註，或管理員 |

清單的查詢參數：

| 參數 | 說明 |
|---|---|
| `site_id` | 只看某個網站 |
| `status` | `new`、`in_progress`、`done`、`spam`；`open` 是未處理加處理中；`all` 是全部。沒給的話是垃圾訊息以外的全部 |
| `age` | 收件年齡：`fresh` 未滿 24 小時；`waiting` 已滿 24 小時、未滿 7 天；`overdue` 已滿 7 天。可與 `status` 等條件交集，不代表 SLA 逾時 |
| `sort` | `newest`（預設）或 `oldest`，清單與 CSV 使用相同排序 |
| `q` | 在姓名、信箱、電話、公司、訊息裡找 |
| `from`、`to` | 時間範圍（RFC 3339），含 `from`、不含 `to` |
| `page`、`per_page` | `per_page` 最多 100 |

### 統計

`GET /api/stats?tz_offset=480` 回總覽頁用的數字：`by_status`、`open_age`（`fresh`／`waiting`／`overdue` 三個互斥區間，只計 new 與 in_progress）、`last_7_days`、`previous_7_days`、`daily`（最近 30 天每天幾則，不含垃圾訊息）、`sites`、`recent`。
