#!/usr/bin/env bash
# 對一個剛啟動、資料庫是空的 Feedback Inbox 跑一輪：建立管理員、建網站、送表單、處理訊息、匯出。
# 用法：bash docs/smoke-test.sh http://127.0.0.1:8787
set -euo pipefail
base=${1:-http://127.0.0.1:8787}
jar=$(mktemp)
# 每次跑都用新的隨機密碼，不放任何固定密碼在 repo 裡
pw=$(openssl rand -hex 16)
json=(-H 'content-type: application/json')
status() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

for _ in $(seq 1 30); do
  curl -fsS "$base/api/health" >/dev/null 2>&1 && break
  sleep 1
done
curl -fsS "$base/api/meta" | grep -q '"smtp_configured":false'
curl -fsS "$base/" | grep -q '<div id="root">'
curl -fsS "$base/inbox/1" | grep -q '<div id="root">'

# 還沒有帳號：要先建立管理員；沒登入的 API 回 401
curl -fsS "$base/api/auth/state" | grep -q '"setup_required":true'
test "$(status "$base/api/feedback")" = 401
curl -fsS -c "$jar" -D "$jar.headers" "${json[@]}" \
  -d "{\"username\":\"admin\",\"password\":\"$pw\"}" "$base/api/auth/setup" >/dev/null
grep -qi 'set-cookie: fi_session=.*HttpOnly' "$jar.headers"
# 只能建立一次
code=$(status "${json[@]}" -d "{\"username\":\"other\",\"password\":\"$pw\"}" "$base/api/auth/setup")
test "$code" = 409

# 建一個網站，只收 www.example.com 送來的表單
site=$(curl -fsS -b "$jar" "${json[@]}" \
  -d '{"name":"官方網站","url":"https://www.example.com","allowed_origins":["https://www.example.com"]}' "$base/api/sites")
key=$(printf '%s' "$site" | sed -n 's/.*"key":"\([^"]*\)".*/\1/p')
test -n "$key"
form="$base/f/$key"

# 內建的表單頁
curl -fsS "$form" | grep -q '官方網站'
# 用 JSON 送：成功、其他欄位照樣留著
curl -fsS "${json[@]}" -H 'Origin: https://www.example.com' \
  -d '{"name":"王小明","email":"ming@example.org","message":"想了解報價","方案":"企業版"}' "$form" | grep -q '"ok":true'
# 一般的 HTML 表單：送完轉去感謝頁
test "$(status -H 'Origin: https://www.example.com' --data-urlencode 'name=李小華' \
  --data-urlencode 'email=hua@example.org' --data-urlencode 'message=網站打不開' "$form")" = 303
curl -fsS "$form/thanks" >/dev/null
# 少了必填欄位、別的網站送來的、找不到的表單
test "$(status "${json[@]}" -H 'Origin: https://www.example.com' -d '{"name":"沒留信箱"}' "$form")" = 400
test "$(status "${json[@]}" -H 'Origin: https://evil.example' -d '{"name":"x","email":"x@example.org","message":"x"}' "$form")" = 403
test "$(status "${json[@]}" -d '{}' "$base/f/nosuchformkey0000")" = 404
# 機器人填了藏起來的欄位：回成功，但不存
curl -fsS "${json[@]}" -H 'Origin: https://www.example.com' \
  -d '{"name":"bot","email":"bot@example.org","message":"buy now","_gotcha":"1"}' "$form" | grep -q '"ok":true'

# 收件匣裡有兩則
curl -fsS -b "$jar" "$base/api/feedback" | grep -q '"total":2'
curl -fsS -b "$jar" "$base/api/feedback?q=報價" | grep -q '"total":1'
curl -fsS -b "$jar" "$base/api/feedback/1" | grep -q '"name":"方案","value":"企業版"'
# 改狀態、寫備註、整批改
curl -fsS -b "$jar" -X PATCH "${json[@]}" -d '{"status":"in_progress"}' "$base/api/feedback/1" | grep -q '"status":"in_progress"'
curl -fsS -b "$jar" "${json[@]}" -d '{"body":"已經回信給對方"}' "$base/api/feedback/1/notes" | grep -q '已經回信給對方'
curl -fsS -b "$jar" "${json[@]}" -d '{"ids":[1,2],"status":"done"}' "$base/api/feedback/bulk" | grep -q '"updated":2'
curl -fsS -b "$jar" "$base/api/stats" | grep -q '"done":2'
test "$(curl -fsS -b "$jar" "$base/api/feedback/export" | wc -l)" -ge 3

# 換表單網址之後，舊的就不能用了
curl -fsS -b "$jar" -X POST "${json[@]}" -d '{}' "$base/api/sites/1/rotate-key" >/dev/null
test "$(status "${json[@]}" -d '{}' "$form")" = 404

# 別的網站送來的管理請求被擋下
test "$(status -b "$jar" -H 'Origin: https://evil.example' "${json[@]}" -d '{"name":"x"}' "$base/api/sites")" = 403

# 登出後 session 失效
curl -fsS -b "$jar" -X POST "${json[@]}" -d '{}' "$base/api/auth/logout" >/dev/null
test "$(status -b "$jar" "$base/api/feedback")" = 401
rm -f "$jar" "$jar.headers"
echo "smoke test ok"
