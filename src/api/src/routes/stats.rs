//! 首頁的統計。

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::QueryBuilder;

use super::feedback::{self, push_visible, STATUSES};
use crate::{auth::CurrentUser, db, error::AppResult, state::AppState};

const DAYS: i64 = 30;

#[derive(Deserialize)]
pub struct Params {
    /// 比 UTC 快幾分鐘（台北是 480）；每天的數字照這個時區切
    tz_offset: Option<i32>,
}

pub async fn overview(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(params): Query<Params>,
) -> AppResult<Json<Value>> {
    let tz_offset = i64::from(params.tz_offset.unwrap_or(0).clamp(-14 * 60, 14 * 60));

    let mut qb = QueryBuilder::new("SELECT f.status, COUNT(*) FROM feedback f WHERE 1 = 1");
    push_visible(&mut qb, &user);
    qb.push(" GROUP BY f.status");
    let counts: HashMap<String, i64> = qb
        .build_query_as::<(String, i64)>()
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect();
    let by_status: serde_json::Map<String, Value> = STATUSES
        .iter()
        .map(|s| {
            (
                s.to_string(),
                json!(counts.get(*s).copied().unwrap_or_default()),
            )
        })
        .collect();

    // 最近 30 天每天幾則（不算垃圾訊息），照使用者的時區切日
    let now = Utc::now();
    let mut open_age = serde_json::Map::new();
    for age in ["fresh", "waiting", "overdue"] {
        let mut qb = QueryBuilder::new(
            "SELECT COUNT(*) FROM feedback f WHERE f.status IN ('new', 'in_progress')",
        );
        push_visible(&mut qb, &user);
        feedback::push_age(&mut qb, age, now)?;
        let count: i64 = qb.build_query_scalar().fetch_one(&state.db).await?;
        open_age.insert(age.to_owned(), json!(count));
    }
    let today = (now + Duration::minutes(tz_offset)).date_naive();
    let first_day = today - Duration::days(DAYS - 1);
    let since = first_day.and_hms_opt(0, 0, 0).unwrap().and_utc() - Duration::minutes(tz_offset);
    let mut qb = QueryBuilder::new("SELECT strftime('%Y-%m-%d', datetime(f.created_at, ");
    qb.push_bind(format!("{tz_offset:+} minutes"))
        .push(")) AS day, COUNT(*) FROM feedback f WHERE f.status != 'spam' AND f.created_at >= ")
        .push_bind(db::format_time(since));
    push_visible(&mut qb, &user);
    qb.push(" GROUP BY day");
    let per_day: HashMap<String, i64> = qb
        .build_query_as::<(String, i64)>()
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect();
    let daily: Vec<Value> = (0..DAYS)
        .map(|i| {
            let date = (first_day + Duration::days(i))
                .format("%Y-%m-%d")
                .to_string();
            let count = per_day.get(&date).copied().unwrap_or_default();
            json!({ "date": date, "count": count })
        })
        .collect();
    let sum = |range: std::ops::Range<usize>| -> i64 {
        daily[range]
            .iter()
            .filter_map(|d| d["count"].as_i64())
            .sum()
    };
    let days = DAYS as usize;

    let mut qb = QueryBuilder::new(
        "SELECT s.id, s.name, s.active,
            (SELECT COUNT(*) FROM feedback f WHERE f.site_id = s.id AND f.status != 'spam'),
            (SELECT COUNT(*) FROM feedback f
              WHERE f.site_id = s.id AND f.status IN ('new', 'in_progress')),
            (SELECT MAX(f.created_at) FROM feedback f WHERE f.site_id = s.id AND f.status != 'spam')
         FROM sites s WHERE 1 = 1",
    );
    if !user.is_admin() {
        qb.push(" AND s.id IN (SELECT site_id FROM site_members WHERE user_id = ")
            .push_bind(user.id)
            .push(")");
    }
    qb.push(" ORDER BY s.name");
    let sites: Vec<Value> = qb
        .build_query_as::<(i64, String, bool, i64, i64, Option<String>)>()
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|(id, name, active, total, unhandled, last_at)| {
            json!({
                "id": id,
                "name": name,
                "active": active,
                "total": total,
                "unhandled": unhandled,
                "last_at": last_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "by_status": by_status,
        "open_age": open_age,
        "last_7_days": sum(days - 7..days),
        "previous_7_days": sum(days - 14..days - 7),
        "daily": daily,
        "sites": sites,
        "recent": feedback::recent(&state, &user, 6).await?,
    })))
}
