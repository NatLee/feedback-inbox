<p align="center">
  <img src="docs/images/cover.svg" alt="Feedback Inbox：每一封來信，都值得回應" width="100%">
</p>

<p align="center">
  <a href="https://github.com/NatLee/feedback-inbox/actions/workflows/ci.yml"><img src="https://github.com/NatLee/feedback-inbox/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/backend-Rust%20%2B%20axum-b7410e" alt="Rust + axum">
  <img src="https://img.shields.io/badge/frontend-React%20%2B%20Tailwind-b5412b" alt="React + Tailwind">
  <img src="https://img.shields.io/badge/docker-one%20image-2496ed" alt="Docker">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT">
</p>

網站上的「聯絡我們」表單，送出之後通常是寄一封信到某個共用信箱，然後就沒有然後了：誰看過、誰回了、哪幾則還沒人理，都不知道。

Feedback Inbox 是這些表單的後端和收件匣。把表單的 `action` 指到它給的網址，訊息就會進來；每個網站可以指定負責人，有新訊息時寄信或送到 Slack / Discord；每一則訊息有狀態和處理紀錄，總覽頁看得到還有幾則沒處理。

一個 Docker 映像檔就是全部：Rust 後端、React 前端、SQLite 資料庫。

## 三步處理一封來信

以下三張圖是在隔離的內建 demo 執行個體中，連續操作同一封「產品文件」來信（`#00001`）；不是示意頁面。封面是獨立的信紙與信封插畫，操作圖則來自真實介面。

### 1. 從等最久的信封開始

在總覽以鍵盤開啟「已滿 7 天」信封，收件匣會保留「待處理／已滿 7 天／最舊來信優先」。畫面第一封就是下一步要閱讀的訊息。

![步驟一：打開逾七天信封，收件匣依最舊來信排序](docs/images/workflow-01.png)

### 2. 打開同一封來信

點第一封訊息，閱讀原文與來源，並確認信紙上的編號 `#00001`。右側通知是內建 demo 的歷史紀錄；這段操作沒有送出通知。

![步驟二：閱讀產品文件來信 #00001 與既有處理紀錄](docs/images/workflow-02.png)

### 3. 完成分類，留下接手資訊

把狀態改成「已完成」，再填入只供同事閱讀的備註。畫面同時顯示新狀態、狀態變更紀錄與備註；腳本也向 API 確認資料已儲存，拍完後恢復原狀與時間戳記，移除本次新增的紀錄。

![步驟三：同一封來信已完成，團隊備註確實儲存](docs/images/workflow-03.png)

[觀看實際選信、狀態與備註動態（WebM）](docs/images/feature-motion.webm)。選取定位線以 160 ms 回饋；閱讀、已儲存狀態與處理紀錄以 180 ms 銜接。系統啟用「減少動態」時立即呈現最後狀態。影片只記錄已登入的隔離 demo，不含登入畫面。

重拍方式、資料還原與驗證範圍見 [操作驗證](docs/workflow-verification.md)；執行 [capture-workflow.py](docs/capture-workflow.py) 時，帳密由環境變數提供。

新版來源樣式、桌面／手機檢查與錄影驗證見 [主題動態驗證](docs/themed-motion-verification.md)。

## 功能

- **一個網址就能收表單**：一般的 HTML 表單和 `fetch` 送的 JSON 都收，不用在自己的網站寫後端。也有一張現成的表單頁，沒有網頁時直接把連結給人填。
- **多個網站**：每個網站有自己的表單網址、必填欄位、負責人和通知方式。成員只看得到自己負責的網站。
- **通知**：寄信給負責人（直接按「回覆」就是回給留言的人），以及 webhook。Slack 和 Discord 的 webhook 網址會自動用它們的格式，其他網址收到完整的 JSON。每一則通知有沒有送出去都查得到。
- **處理流程**：未處理 → 處理中 → 已完成，或標成垃圾訊息。可以寫備註給同事看，誰在什麼時候改了狀態都留著。
- **擋垃圾訊息**：藏起來的欄位（honeypot）、同一個 IP 的次數限制、限定只收哪些網站送來的表單。
- **來信工作台**：狀態登錄、信封式等待分流、30 天來信日曆、網站處理負載。點圖表直接進入對應來信；等待最久的信件可優先處理。
- **搜尋、篩選、整批處理、CSV 匯出**：網站、狀態、日期與收到多久的篩選可一起使用，CSV 保留同樣的條件與排序。
- **帳號**：第一個打開網頁的人建立管理員，沒有預設帳號，也沒有預設密碼。密碼用 Argon2 雜湊，登入是 HttpOnly cookie。
- 淺色和深色模式。

## 畫面與工作流程

以朱紅、奶油紙色、信封與收件郵戳構成「回饋處理編輯台」，固定的收件側欄讓團隊在總覽、收件匣和網站設定間移動。所有數字來自 API；截圖使用專案內建的虛構 demo 資料。

### 今日工作桌

總覽將待處理來信分成未滿 24 小時、1–7 天內、已滿 7 天三個可拆閱的信封。點選後以最舊來信優先；等待時間從收件時間計算，不是回覆速度或 SLA。收件郵戳顯示待處理封數，旁邊的狀態登錄可分別開啟未處理、處理中與已完成來信。

30 天日曆按瀏覽器時區切日，點任一日期會帶入收件匣的起訖日期。網站負載依待處理封數排序，可只顯示仍有待處理訊息的網站。

### 閱讀與接續對話

信件原文放在帶收件日期與編號的信紙上，處理紀錄另列為團隊工作區。內容頁保留聯絡方式、狀態選單、備註、通知結果及時間紀錄。列表提供狀態分頁、搜尋、日期篩選、整批處理及 CSV 匯出。

### 網站與成員

每個網站分別設定表單欄位、來源限制、負責人與通知方式。

<details>
<summary>其他功能、登入、深色與手機畫面</summary>

![完整來信工作台](docs/images/screenshot-overview-light.png)

![收件匣與相同條件的 CSV 匯出](docs/images/screenshot-inbox-light.png)
![訊息與團隊備註](docs/images/screenshot-detail-light.png)
![網站設定](docs/images/screenshot-site-light.png)

![登入](docs/images/screenshot-login-light.png)
![深色工作桌](docs/images/screenshot-overview-dark.png)
<img src="docs/images/screenshot-mobile.png" alt="手機來信工作台" width="390">

</details>

## 快速開始

```bash
git clone https://github.com/NatLee/feedback-inbox.git
cd feedback-inbox
docker compose up -d --build
```

打開 <http://127.0.0.1:8787>，第一次會請你建立管理員帳號。接著：

1. 到「網站」新增一個網站，會拿到一個表單網址。
2. 把它貼進表單的 `action`：

   ```html
   <form action="http://127.0.0.1:8787/f/你的表單代碼" method="post">
     <input name="name" placeholder="姓名" required>
     <input name="email" type="email" placeholder="電子信箱" required>
     <textarea name="message" placeholder="想說的話" required></textarea>
     <!-- 人看不到、機器人會填的欄位：有填就當成垃圾訊息 -->
     <input name="_gotcha" style="display:none" tabindex="-1" autocomplete="off">
     <button>送出</button>
   </form>
   ```

   認得的欄位是 `name`、`email`、`phone`、`company`、`message`，其他欄位會原樣留在訊息裡。用 `fetch` 送 JSON 也可以，網站設定頁有寫法。
3. 送一則試試，它會出現在收件匣。

想先看有資料的樣子，可以在**空的**資料庫裡建立範例帳號、網站和訊息（密碼會印在畫面上）：

```bash
docker compose run --rm feedback-inbox feedback-inbox demo
```

忘記密碼的時候：

```bash
docker compose exec feedback-inbox feedback-inbox reset-password <帳號>
```

### 通知信

寄信需要一個 SMTP 服務，在 `.env` 裡設定（[.env.example](.env.example) 有範例）；沒設定就不寄信，webhook 不受影響。收信的是網站的負責人，信箱在各自的「設定」裡填。

還沒有 SMTP 的話，可以先用內附的假信箱看信長什麼樣子：

```bash
cat >> .env <<'ENV'
FI_SMTP_HOST=mailpit
FI_SMTP_PORT=1025
FI_SMTP_TLS=none
FI_SMTP_FROM=Feedback Inbox <inbox@example.com>
ENV
docker compose --profile mail up -d
```

寄出去的信都會出現在 <http://localhost:8025>，不會真的寄到任何人的信箱。網站設定頁的「送測試通知」可以馬上試。

### 放到網路上

前面放一個有 HTTPS 的反向代理（Caddy、nginx…），然後設定：

- `FI_PUBLIC_URL`：外部看到的網址。表單網址和通知信裡的連結用它組，是 https 時登入 cookie 會加 `Secure`。
- `FI_TRUST_PROXY=true`：訪客的 IP 才會是對的（次數限制是照 IP 算的）。
- 每個網站的「允許的來源」填上自己的網址，別的網站就不能拿你的表單網址來送。

## 設定

都是環境變數，全部可以不設。用 docker compose 時寫在同一個資料夾的 `.env`。

| 變數 | 預設 | 說明 |
|---|---|---|
| `FI_PORT` | `8787` | docker compose 對外的 port |
| `FI_PUBLIC_URL` | | 外部看到的網址，例如 `https://inbox.example.com` |
| `FI_COOKIE_SECURE` | 看 `FI_PUBLIC_URL` | `true` / `false`，強制指定 cookie 要不要加 `Secure` |
| `FI_TRUST_PROXY` | `false` | 前面有反向代理時設 `true`，訪客的 IP 改看 `X-Forwarded-For` |
| `FI_RATE_LIMIT` | `10` | 同一個 IP 一分鐘最多送幾次表單，`0` 是不限制 |
| `FI_SMTP_HOST` | | SMTP 伺服器。沒設就不寄通知信 |
| `FI_SMTP_PORT` | 看 `FI_SMTP_TLS` | `starttls` 是 587、`tls` 是 465、`none` 是 25 |
| `FI_SMTP_USERNAME`、`FI_SMTP_PASSWORD` | | SMTP 的帳號密碼 |
| `FI_SMTP_FROM` | | 寄件人，例如 `Feedback Inbox <inbox@example.com>`。設了 `FI_SMTP_HOST` 就要設 |
| `FI_SMTP_TLS` | `starttls` | `starttls`、`tls` 或 `none` |
| `FI_BIND` | `0.0.0.0:8787` | 伺服器聽的位址 |
| `FI_DATA_DIR` | `data`（映像檔裡是 `/app/data`） | 放 SQLite 資料庫的資料夾 |
| `FI_DATABASE_PATH` | `<FI_DATA_DIR>/feedback-inbox.db` | 資料庫檔案 |
| `FI_STATIC_DIR` | `src/web/dist`（映像檔裡是 `/app/web`） | 前端檔案的位置 |

資料都在 `feedback-inbox-data` 這個 volume 的 SQLite 檔案裡，備份那個檔案就好。

## 開發

需要 Rust（stable）、Node 24、pnpm。開發固定使用 `http://127.0.0.1:5173`；後端的 `ACCESS_PUBLIC_URL` 必須與前端相同。若另設 `FI_PUBLIC_URL`，也必須同 origin。

```bash
# 後端：http://127.0.0.1:8787
cd src/api && ACCESS_PUBLIC_URL=http://127.0.0.1:5173 cargo run

# 前端：http://127.0.0.1:5173，/api 和 /f 會轉給後端
cd src/web && pnpm install && pnpm dev --host 127.0.0.1 --port 5173 --strictPort
```

```bash
cd src/api && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd src/web && pnpm test && pnpm build
bash docs/smoke-test.sh http://127.0.0.1:8787   # 對一個空的執行個體跑一輪
```

API 和 webhook 的格式在 [docs/api.md](docs/api.md)。

## 設計與驗證

[產業介面設計與驗證](docs/industry-design.md) 說明這次的專用元件與真實資料互動；[視覺分析設計與驗證](docs/visual-insights.md) 說明新工作台的資料定義與測試；[原版型紀錄](docs/design.md) 說明版型、響應式、後端修正及驗證範圍。封面是可編輯的 [SVG](docs/images/cover.svg)；應用畫面由 [擷取腳本](docs/capture-screenshots.py) 對隔離的 demo 執行個體重拍。

## 授權

[MIT](LICENSE) © Nat Lee

## 帳號綁定、OAuth、API 與 MCP

原生「連線與存取」可綁定 GitHub／Google、建立與撤銷金鑰、管理應用授權及核對真正用量。`feedback:read` 可讀取既有站台權限內的來信、聯絡資料與內部備註。 設定與協定見 [存取指南](docs/access.md)，本機真實操作見 [錄影](docs/images/access-workflow.webm) 與 [稽核截圖](docs/images/access-03-usage.png)。


## 首次登入與建立管理員

第一次開啟空白安裝時，首頁、登入頁和需要登入的頁面都會自動導向 `/first-login`。輸入 3–32 個英文字母、數字或 `. _ -` 的帳號、選填顯示名稱，以及 8–200 個字元的密碼與確認密碼，按「建立管理員」後即以新管理員身份進入工作空間。密碼不一致時不會送出請求。

這個頁面只用來建立第一位管理員。有帳號的安裝會改顯示 `/login`；其他成員由管理員在既有設定頁新增。若另一位管理員已先完成設定，舊頁面會自動轉成登入。透過 OAuth 登入的合法本站授權流程會在建立帳號後接續進行。

Docker 對外網址請維持 `FI_PUBLIC_URL` 與 `ACCESS_PUBLIC_URL` 同源。操作與實際瀏覽器驗證見 [首次登入驗證紀錄](docs/first-login-verification.md)。

![首次登入頁](docs/images/first-login-1440-light.png)
