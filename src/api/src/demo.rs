//! 範例資料：`feedback-inbox demo` 在空的資料庫裡建立幾個帳號、網站和訊息，
//! 用來試用或截圖。內容都是虛構的，時間以執行當下往回推。

use anyhow::{anyhow, bail, Result};
use chrono::{Duration, Utc};

use crate::{auth, db, intake::Extra, state::AppState};

pub const MEMBER_USERNAMES: [&str; 2] = ["amy", "ben"];

/// 帳號、顯示名稱、信箱、角色
const USERS: [(&str, &str, &str, &str); 3] = [
    ("demo", "林小安", "demo@example.com", "admin"),
    ("amy", "陳雅婷", "amy@example.com", "member"),
    ("ben", "張志豪", "", "member"),
];

struct SiteSeed {
    name: &'static str,
    url: &'static str,
    key: &'static str,
    origins: &'static [&'static str],
    required: &'static [&'static str],
    webhook: &'static str,
    /// 負責的人（`USERS` 的位置）
    members: &'static [usize],
}

const SITES: [SiteSeed; 3] = [
    SiteSeed {
        name: "官方網站",
        url: "https://www.example.com",
        key: "demo0000official",
        origins: &["https://www.example.com"],
        required: &["name", "email", "message"],
        webhook: "https://hooks.example.com/feedback",
        members: &[0, 1],
    },
    SiteSeed {
        name: "產品文件",
        url: "https://docs.example.com",
        key: "demo0000docs0000",
        origins: &[],
        required: &["message"],
        webhook: "",
        members: &[0, 2],
    },
    SiteSeed {
        name: "活動報名頁",
        url: "https://events.example.com/2026",
        key: "demo0000events00",
        origins: &["https://events.example.com"],
        required: &["name", "email", "phone"],
        webhook: "",
        members: &[1, 2],
    },
];

const PEOPLE: [(&str, &str); 14] = [
    ("王小明", "ming"),
    ("李佳穎", "chiaying"),
    ("黃柏翰", "bohan"),
    ("吳淑芬", "shufen"),
    ("蔡宗翰", "tsunghan"),
    ("許雅雯", "yawen"),
    ("鄭家豪", "chiahao"),
    ("謝佩珊", "peishan"),
    ("周育廷", "yuting"),
    ("賴思妤", "szuyu"),
    ("Alex Morgan", "alex.morgan"),
    ("Jamie Rivera", "jamie.rivera"),
    ("洪建宏", "chienhung"),
    ("邱怡君", "yichun"),
];

const COMPANIES: [&str; 9] = [
    "範例科技",
    "晨星設計工作室",
    "北極光數位",
    "",
    "青田書店",
    "山嵐咖啡",
    "",
    "遠帆物流",
    "Example Labs",
];

/// 訊息，和表單上多出來的欄位
type MessageSeed = (&'static str, &'static [(&'static str, &'static str)]);

/// 每個網站會收到的訊息
const MESSAGES: [&[MessageSeed]; 3] = [
    &[
        (
            "想了解企業方案的報價，我們大約 30 個人會用。\n方便的話請提供年繳的價格。",
            &[("想了解的方案", "企業版"), ("人數", "30")],
        ),
        (
            "請問有提供試用嗎？想先讓團隊評估一下。",
            &[("想了解的方案", "團隊版")],
        ),
        ("發票抬頭打錯了，可以重開嗎？訂單編號是 A-10293。", &[]),
        (
            "網站的價格頁在手機上版面會跑掉，按鈕被擋住按不到。",
            &[("裝置", "iPhone / Safari")],
        ),
        ("我們是非營利組織，請問有教育或公益優惠嗎？", &[]),
        (
            "想約個時間線上展示，下週二或週四下午都可以。",
            &[("想了解的方案", "企業版"), ("偏好的時間", "週二、週四下午")],
        ),
        ("忘記密碼的信一直沒收到，垃圾信件匣也找過了。", &[]),
        (
            "想請問資料存放的地點，以及有沒有通過相關的資安認證？",
            &[("人數", "200")],
        ),
        ("你們的服務很好用，謝謝！希望之後可以加上深色模式。", &[]),
        (
            "想洽談經銷合作，請問該聯絡哪一位？",
            &[("合作類型", "經銷")],
        ),
    ],
    &[
        (
            "「快速開始」第三步的指令少了一個參數，照著做會出錯。",
            &[("頁面", "快速開始")],
        ),
        (
            "API 文件裡分頁的範例回傳的欄位和實際的不一樣。",
            &[("頁面", "API 參考")],
        ),
        ("希望可以補上 Docker Compose 的部署範例。", &[]),
        (
            "搜尋找不到「webhook」相關的頁面，但目錄裡明明有。",
            &[("頁面", "搜尋")],
        ),
        ("升級說明寫得很清楚，照著做十分鐘就完成了，謝謝。", &[]),
        ("範例程式碼可以加上複製按鈕嗎？", &[]),
        (
            "英文版的安裝頁面連結是壞的（404）。",
            &[("頁面", "Installation")],
        ),
    ],
    &[
        (
            "報名兩位，都吃素。請問現場有停車位嗎？",
            &[("人數", "2"), ("餐點", "素食")],
        ),
        (
            "報名一位。想確認活動會不會錄影？",
            &[("人數", "1"), ("餐點", "葷食")],
        ),
        (
            "公司想團體報名 8 位，可以開一張發票嗎？",
            &[("人數", "8"), ("餐點", "葷食")],
        ),
        (
            "原本報名了但當天臨時有事，請問可以轉讓名額嗎？",
            &[("人數", "1")],
        ),
        ("請問下午場還有名額嗎？", &[("場次", "下午場")]),
        (
            "報名三位，其中一位需要無障礙座位。",
            &[("人數", "3"), ("餐點", "葷食"), ("其他需求", "無障礙座位")],
        ),
    ],
];

const SPAM: [&str; 3] = [
    "Boost your website traffic today!!! Visit https://spam.example.net now",
    "您好，我們提供網站排名優化服務，保證三天上首頁，請回信。",
    "CONGRATULATIONS you have been selected, click https://spam.example.net/win",
];

const NOTES: [&str; 6] = [
    "已經回信，等對方確認。",
    "打過電話了，約好週四下午再聊。",
    "轉給業務同事跟進。",
    "已經修正，下次發版會上線。",
    "和對方確認過需求，報價單今天寄出。",
    "問題重現了，開了內部的工單追蹤。",
];

const USER_AGENTS: [&str; 3] = [
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
];

/// 每次執行都產生一樣的資料。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

pub struct Summary {
    pub users: usize,
    pub sites: usize,
    pub feedback: usize,
}

pub async fn seed(state: &AppState, password: &str) -> Result<Summary> {
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    if existing > 0 {
        bail!("資料庫裡已經有帳號了；範例資料只能建在空的資料庫裡");
    }
    let now = Utc::now();

    let mut user_ids = Vec::new();
    for (username, display_name, email, role) in USERS {
        let id = auth::create_user(&state.db, username, display_name, email, password, role)
            .await
            .map_err(|e| anyhow!("建立帳號 {username} 失敗：{e:?}"))?;
        user_ids.push(id);
    }

    let mut site_ids = Vec::new();
    for (i, site) in SITES.iter().enumerate() {
        let id = sqlx::query(
            "INSERT INTO sites (name, url, key, allowed_origins, required_fields, webhook_url, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(site.name)
        .bind(site.url)
        .bind(site.key)
        .bind(serde_json::to_string(site.origins)?)
        .bind(serde_json::to_string(site.required)?)
        .bind(site.webhook)
        .bind(db::format_time(now - Duration::days(90 - i as i64 * 20)))
        .execute(&state.db)
        .await?
        .last_insert_rowid();
        for member in site.members {
            sqlx::query("INSERT INTO site_members (site_id, user_id) VALUES (?, ?)")
                .bind(id)
                .bind(user_ids[*member])
                .execute(&state.db)
                .await?;
        }
        site_ids.push(id);
    }

    let mut rng = Lcg(20260105);
    // 越近的越多：離現在幾個小時
    let mut ages: Vec<i64> = (0..44)
        .map(|i| {
            let day = match i {
                0..=7 => rng.next(2),
                8..=19 => 2 + rng.next(6),
                _ => 8 + rng.next(21),
            };
            (day * 24 + 1 + rng.next(22)) as i64
        })
        .collect();
    ages.sort_unstable_by(|a, b| b.cmp(a));

    let mut tx = state.db.begin().await?;
    let mut count = 0;
    for (i, age) in ages.into_iter().enumerate() {
        let created = now - Duration::hours(age) - Duration::minutes(rng.next(60) as i64);
        let spam = i % 11 == 5;
        // 官方網站最多，其次是文件
        let site = match rng.next(10) {
            0..=4 => 0,
            5..=7 => 1,
            _ => 2,
        };
        let (person, mailbox) = PEOPLE[rng.next(PEOPLE.len())];
        let company = COMPANIES[rng.next(COMPANIES.len())];
        let (message, extras) = MESSAGES[site][rng.next(MESSAGES[site].len())];
        let status = if spam {
            "spam"
        } else if age < 30 {
            "new"
        } else if age < 24 * 5 {
            ["new", "in_progress", "in_progress", "done"][rng.next(4)]
        } else {
            ["done", "done", "done", "done", "in_progress"][rng.next(5)]
        };
        let (name, email, phone, company, message, extra) = if spam {
            (
                "SEO Expert",
                "promo@spam.example.net".to_owned(),
                String::new(),
                "",
                SPAM[rng.next(SPAM.len())],
                Vec::new(),
            )
        } else {
            let extra: Vec<Extra> = extras
                .iter()
                .map(|(name, value)| Extra {
                    name: name.to_string(),
                    value: value.to_string(),
                })
                .collect();
            // 文件的表單只要求訊息，有些人不留名字
            let anonymous = site == 1 && rng.next(3) == 0;
            (
                if anonymous { "" } else { person },
                if anonymous {
                    String::new()
                } else {
                    format!("{mailbox}@example.org")
                },
                if site == 1 || rng.next(3) == 0 {
                    String::new()
                } else {
                    format!("0900-000-{:03}", 100 + rng.next(900))
                },
                if site == 1 { "" } else { company },
                message,
                extra,
            )
        };
        let updated = if status == "new" || spam {
            created
        } else {
            created + Duration::hours(2 + rng.next(20) as i64)
        }
        .min(now);

        let id = sqlx::query(
            "INSERT INTO feedback (site_id, name, email, phone, company, message, extra, status,
                ip, user_agent, page_url, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(site_ids[site])
        .bind(name)
        .bind(email)
        .bind(phone)
        .bind(company)
        .bind(message)
        .bind(serde_json::to_string(&extra)?)
        .bind(status)
        // 文件保留給範例用的網段
        .bind(format!("203.0.113.{}", 1 + rng.next(250)))
        .bind(USER_AGENTS[rng.next(USER_AGENTS.len())])
        .bind(format!("{}/contact", SITES[site].url))
        .bind(db::format_time(created))
        .bind(db::format_time(updated))
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        count += 1;

        // 通知的紀錄：有信箱的負責人各一封，官方網站另外送 webhook
        let delivered = db::format_time(created + Duration::seconds(2));
        for member in SITES[site].members {
            let email = USERS[*member].2;
            if email.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO deliveries (feedback_id, channel, target, ok, detail, created_at)
                 VALUES (?, 'email', ?, 1, '', ?)",
            )
            .bind(id)
            .bind(email)
            .bind(&delivered)
            .execute(&mut *tx)
            .await?;
        }
        if !SITES[site].webhook.is_empty() {
            let failed = i % 9 == 4;
            sqlx::query(
                "INSERT INTO deliveries (feedback_id, channel, target, ok, detail, created_at)
                 VALUES (?, 'webhook', 'https://hooks.example.com/…', ?, ?, ?)",
            )
            .bind(id)
            .bind(!failed)
            .bind(if failed {
                "對方回應 503 Service Unavailable"
            } else {
                ""
            })
            .bind(&delivered)
            .execute(&mut *tx)
            .await?;
        }

        // 處理紀錄
        let handler = user_ids[SITES[site].members[rng.next(SITES[site].members.len())]];
        let log = |kind: &'static str, body: String, at: chrono::DateTime<Utc>| {
            (kind, body, db::format_time(at.min(now)))
        };
        let mut entries = Vec::new();
        match status {
            "spam" => entries.push(log("status", "spam".into(), created + Duration::hours(1))),
            "in_progress" => {
                entries.push(log("status", "in_progress".into(), updated));
                if rng.next(2) == 0 {
                    let note = NOTES[rng.next(NOTES.len())];
                    entries.push(log("note", note.into(), updated + Duration::minutes(5)));
                }
            }
            "done" => {
                let started = created + Duration::hours(1);
                entries.push(log("status", "in_progress".into(), started));
                let note = NOTES[rng.next(NOTES.len())];
                entries.push(log("note", note.into(), started + Duration::minutes(20)));
                entries.push(log("status", "done".into(), updated));
            }
            _ => {}
        }
        for (kind, body, at) in entries {
            sqlx::query(
                "INSERT INTO notes (feedback_id, user_id, kind, body, created_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(handler)
            .bind(kind)
            .bind(body)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;

    Ok(Summary {
        users: USERS.len(),
        sites: SITES.len(),
        feedback: count,
    })
}
