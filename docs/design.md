# Feedback Inbox：信件工作桌

## 設計

以赤陶紅、暖紙色、襯線標題與郵戳細節，把表單訊息呈現成值得閱讀的來信。保留固定的工作側欄，總覽改成一封引導信與狀態數字並排；收件匣採信件分頁、寄件人印章與較寬鬆的行距。訊息詳情使用信紙式內容區，備註與通知資訊維持原來的操作流程。

- 字體使用已有的本機 Noto Sans TC，搭配平台的 Iowan Old Style／Baskerville／宋體作閱讀標題；移除未使用的 Inter 依賴。沒有外部字型請求。
- 深色模式保留產品色彩；手機使用選單抽屜、單欄內容與可捲動的資料表。
- 手機收件列移除裝飾印章，讓篩選、批次勾選與訊息預覽保持清楚。總覽的單欄網格明確設定可縮排寬度，避免長訊息撐開頁面。
- 動畫尊重 `prefers-reduced-motion`，保留表單標籤、鍵盤焦點與主題選擇功能。

## 實作與後端檢查

1. 首次設定原先先查使用者數量，再建立帳號。六個同時送出的請求均可能通過檢查，實際測試建立了六名管理員。現在以單一 SQLite `INSERT … SELECT … WHERE NOT EXISTS` 原子寫入，只允許一個成功，其他回覆 `409 Conflict`。驗證與 Argon2 雜湊在寫入前完成。
2. 新增 `concurrent_setup_creates_exactly_one_administrator` 整合測試：修正前失敗（6 個成功），修正後一個成功、五個衝突，資料庫恰有一個帳號。
3. 檢查登入 session、同源保護、輸入驗證、網站成員範圍、匯出與管理員操作，並執行原有整合測試。收件搜尋、狀態、備註、CSV 公式防護、公開表單驗證及來源限制均由原有測試涵蓋。
4. Vite 字串形式的代理設定會改寫 `Host`，導致瀏覽器登入被同源保護拒絕。代理改成明確保留 `Host`，並透過真實瀏覽器登入驗證。表單代理另限定為 `/f` 路徑，避免誤攔 `/favicon.svg`。

## 驗證

- `cargo fmt --check`
- `CARGO_BUILD_JOBS=2 cargo clippy --all-targets -- -D warnings`
- `CARGO_BUILD_JOBS=2 cargo test`：25 個單元測試、15 個整合測試通過。
- `pnpm test`：8 個測試通過。
- `pnpm build`：TypeScript 與 Vite 正式建置通過。
- 原有 `docs/smoke-test.sh` 對另一個空的暫存資料庫通過；未使用現有的使用者資料庫。
- Chromium：1440px 桌面與 390px 手機、淺色／深色、登入、選單與主題切換、主要頁面與額外設定頁面。檢查無頁面 JavaScript 錯誤與水平溢出。
- 實際互動：篩選、搜尋、批次選取、CSV 下載、改狀態、新增／移除測試備註。

## 圖像與公開檢查

`docs/images/cover.svg` 是可編輯的程式碼原生封面。所有 PNG 都由新介面重新擷取，舊版封面與未使用截圖已移除。`docs/capture-screenshots.py` 讀取 `DEMO_URL`、`DEMO_PASSWORD`；帳密不寫入專案。重拍需要 Python Playwright 與 Chromium，使用內建 demo 產生的虛構資料。

檢查目前追蹤檔案與 Git 所有可達歷史 blob（85 個），未找到私鑰、常見雲端／GitHub／Slack token 樣式、追蹤中的 `.env` 或資料庫檔案。範例資料使用保留的 example 網域。這是程式碼、規則掃描與畫面檢查的結果，不代表完整滲透測試。未測試外部 SMTP 或真實第三方 webhook，也未在這輪重新建立 Docker 映像檔。
