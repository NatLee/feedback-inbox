//! HTTP API（規格見 `docs/api.md`）、收表單的公開網址，和前端靜態檔。

mod ai;
mod auth;
mod feedback;
mod forms;
pub mod sites;
mod stats;
mod submit;
mod users;

use axum::{
    extract::State,
    http::{header, HeaderValue},
    middleware,
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use crate::{auth::origin_guard, error::AppError, state::AppState};

fn api_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/ai/providers",
            get(ai::provider_list).post(ai::provider_create),
        )
        .route(
            "/ai/providers/{id}",
            axum::routing::put(ai::provider_update),
        )
        .route("/ai/providers/{id}/test", post(ai::provider_test))
        .route(
            "/sites/{id}/ai",
            get(ai::site_settings).put(ai::site_settings_update),
        )
        .route(
            "/sites/{id}/ai/{kind}",
            get(ai::content_list).post(ai::content_create),
        )
        .route(
            "/sites/{id}/ai/{kind}/{item}",
            axum::routing::put(ai::content_update),
        )
        .route(
            "/sites/{id}/ai/{kind}/{item}/{action}",
            post(ai::content_transition),
        )
        .route("/feedback/{id}/ai", get(ai::feedback_ai))
        .route("/feedback/{id}/ai/analyze", post(ai::analyze))
        .route("/feedback/{id}/ai/classification", post(ai::classification))
        .route("/ai/runs/{id}/cancel", post(ai::cancel_run))
        .route(
            "/feedback/{id}/ai/draft",
            post(ai::draft_create).put(ai::draft_update),
        )
        .route("/feedback/{id}/ai/draft/reject", post(ai::draft_reject))
        .route("/feedback/{id}/ai/send", post(ai::reply_send))
        .route("/ai/outbound/{id}/cancel", post(ai::outbound_cancel))
        .route("/ai/outbound/{id}/reconcile", post(ai::outbound_reconcile))
        .route("/ai/review", get(ai::review))
        .route(
            "/sites/{id}/ai/rules",
            get(ai::rules_list).post(ai::rule_create),
        )
        .route(
            "/sites/{id}/ai/rules/{item}",
            axum::routing::put(ai::rule_update).delete(ai::rule_remove),
        )
        .route("/sites/{id}/ai/auto/dry-run", post(ai::auto_dry_run))
        .route("/sites/{id}/ai/jobs", get(ai::jobs_list))
        .route("/health", get(|| async { "ok" }))
        .route("/meta", get(meta))
        .route("/auth/state", get(auth::state))
        .route("/auth/setup", post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", patch(auth::update_me))
        .route("/auth/password", post(auth::change_password))
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", patch(users::update))
        .route("/sites", get(sites::list).post(sites::create))
        .route(
            "/sites/{id}",
            get(sites::detail)
                .patch(sites::update)
                .delete(sites::remove),
        )
        .route("/sites/{id}/identity-key", get(forms::identity_info))
        .route(
            "/sites/{id}/identity-key/rotate",
            post(forms::rotate_identity),
        )
        .route(
            "/sites/{id}/verification-sessions",
            post(forms::create_verification),
        )
        .route(
            "/sites/{id}/verification-sessions/{session}",
            get(forms::verification_status),
        )
        .route("/sites/{id}/integrations", get(forms::integrations))
        .route("/sites/{id}/form", get(forms::detail))
        .route("/sites/{id}/form/draft", axum::routing::put(forms::draft))
        .route("/sites/{id}/form/publish", post(forms::publish))
        .route(
            "/sites/{id}/form/versions/{version}/retire",
            post(forms::retire),
        )
        .route("/sites/{id}/rotate-key", post(sites::rotate_key))
        .route("/sites/{id}/test", post(sites::test_notification))
        .route("/feedback", get(feedback::list))
        .route("/feedback/export", get(feedback::export))
        .route("/feedback/bulk", post(feedback::bulk))
        .route(
            "/feedback/{id}",
            get(feedback::detail)
                .patch(feedback::update)
                .delete(feedback::remove),
        )
        .route("/feedback/{id}/notes", post(feedback::add_note))
        .route("/notes/{id}", delete(feedback::remove_note))
        .route("/stats", get(stats::overview))
        .fallback(|| async { AppError::not_found("找不到這個 API") })
}

/// Fixed domain reuse path: no public tracing, no access routes, no network.
pub fn domain_router(state: AppState) -> Router {
    Router::new().nest("/api", api_routes()).with_state(state)
}

pub fn router(state: AppState) -> Router {
    let api = api_routes();

    // 前端是單頁應用：找不到的路徑一律回 index.html，交給前端路由。
    let static_dir = state.config.static_dir.clone();
    let spa = ServeDir::new(&static_dir).fallback(ServeFile::new(static_dir.join("index.html")));

    Router::new()
        .nest("/api", api)
        // 同源檢查只套在 API 上
        .route_layer(middleware::from_fn_with_state(state.clone(), origin_guard))
        // 收表單的網址本來就是給別的網站用的，所以放在同源檢查之後才加
        .route(
            "/f/{key}",
            get(submit::form)
                .post(submit::submit)
                .options(submit::preflight),
        )
        .route("/f/{key}/thanks", get(submit::thanks))
        .route(
            "/f/{key}/embed",
            get(submit::form)
                .post(submit::submit)
                .options(submit::preflight),
        )
        .route("/f/{key}/embed/thanks", get(submit::thanks))
        .fallback_service(spa)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(middleware::from_fn(frame_guard))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ))
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<axum::body::Body>| {
                let route = request
                    .extensions()
                    .get::<axum::extract::MatchedPath>()
                    .map(|path| path.as_str())
                    .unwrap_or("static");
                tracing::info_span!("http",method=%request.method(),route)
            },
        ))
        .with_state(state)
}

async fn meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "smtp_configured": state.mailer.is_some(),
        "public_url": state.config.public_url,
    }))
}

async fn frame_guard(
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if response.extensions().get::<submit::Frameable>().is_none() {
        response
            .headers_mut()
            .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    response
}
