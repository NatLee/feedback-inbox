# 封面與三步操作驗證

2026-11-27，Chrome／Playwright，隔離的內建 demo，1440 × 1050、Asia/Taipei、淺色。前端沿用現有元件，沒有產品行為或權限變更。

## 封面

保留 `docs/images/cover.svg`。以實際 SVG 在 1440 px 與 README 常見的 800 px 寬度檢查：標題、主題圖形及文字可讀，沒有裁切。Inbox 是信紙與信封，CDP 是頁面／互動／身分三條事件航線；封面是品牌插畫，不是產品操作或即時資料，也沒有設定 GitHub Social Preview。

## 重拍

先以專案的 `demo` 指令初始化獨立的本機資料目錄，啟動 API 與前端；不要指向正式或多人共用資料庫。需要 `pip install playwright` 與 Chrome。帳密只由環境變數傳入，不寫進腳本或圖片。

```bash
export DEMO_URL=http://127.0.0.1:15374
export DEMO_USERNAME=demo
# 以自己的隔離 demo 設定 DEMO_PASSWORD 與 DEMO_DATABASE_PATH。
python docs/capture-workflow.py
python -m py_compile docs/capture-workflow.py
git diff --check
```

`DEMO_DATABASE_PATH` 是這個本機 demo 實際使用的 `feedback-inbox.db`，必填。腳本只修改選中的來信。由於狀態稽核紀錄不能透過 API 刪除，腳本在 `finally` 用本機 SQLite 還原原狀態與 `updated_at`，刪除本次新增的狀態紀錄和備註，並比對完整資料列、備註與 API 回傳均等於原值；失敗時也會還原。請勿在重拍時同時讓其他人修改該 demo。

## 實際操作與結果

1. 總覽用鍵盤開啟「已滿 7 天」信封。API 逾期數與收件匣總數相同，介面保留 `status=open`、`age=overdue`、`sort=oldest`，收件日期確實從最舊排起。
2. 開啟列表第一封 `#00001`，核對信紙原文、編號與 API 同一筆資料。圖中的通知結果是 seed 資料的歷史紀錄。
3. 透過真實狀態選單改為「已完成」，透過表單新增團隊備註。API 確認 `done`、狀態稽核紀錄與備註確實存在，且訊息原文與通知紀錄不變。拍完完整還原，沒有呼叫送表單、測試通知或寄信端點。

實跑輸出：`PASS: oldest-envelope drilldown → message #1 → persisted done + internal note; demo restored; desktop/mobile no overflow or page errors.`

三張 PNG 均為 1440 × 1050 完整視窗。實際逐張檢查：列表的相同原文、信紙 `#00001`、完成狀態與內部備註可讀。390 × 844 下檢查總覽、分流收件匣及完成後內容頁，文件沒有水平溢出；同一封信的備註仍可讀。

## 靜態檢查

- 兩次真實流程重跑通過，沒有瀏覽器 page error。
- `python -m py_compile docs/capture-workflow.py` 通過。
- `git diff --check` 通過。
- Pillow `Image.verify()` 成功解碼三張 PNG；README 的本機 Markdown／HTML 圖片與文件連結逐一確認存在；SVG XML 解析與 Chrome 兩種寬度渲染成功。
- 沒有產品原始碼改動，因此未為文件或圖片新增鏡像測試；產品測試與 build 交由既有 PR CI 執行。流程本身由腳本的真實 UI 與 API 斷言驗證。

## 維護決策

沿用合格封面及既有專用元件，沒有為本輪重做品牌。舊的功能截圖移入 README 可收合圖庫，主區改為連續操作故事。實作與本文件由作者先自查，再交由主代理獨立審查後合併。
