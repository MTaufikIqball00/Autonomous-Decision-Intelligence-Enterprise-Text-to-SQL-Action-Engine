use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use dta_backend::{
    api::build_router,
    auth::JwtService,
    config::AppConfig,
    crypto::SecretCipher,
    db::app_db::{bootstrap_app_database, connect_app_pool},
    state::AppState,
};

async fn test_app() -> (axum::Router, AppState) {
    let config = AppConfig::from_env().expect("AppConfig from .env");
    let cipher = SecretCipher::from_hex(&config.master_key_hex).expect("cipher");
    let app_pool = connect_app_pool(&config).await.expect("connect app db");
    let _ = bootstrap_app_database(&app_pool, &config, &cipher)
        .await
        .expect("bootstrap");
    let jwt = JwtService::new(&config.jwt_secret);
    let state = AppState::new(config, app_pool, jwt, cipher);
    let router = build_router(state.clone());
    (router, state)
}

async fn login_as(app: &axum::Router, email: &str, password: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": email, "password": password }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "login failed for {email}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn rbac_credential_protection_saved_artifacts_and_audit_logs_enforced() {
    let (app, state) = test_app().await;

    // 1. AES-256-GCM credential encryption roundtrip
    let enc = state.cipher.encrypt("secret-db-pass").expect("encrypt");
    assert!(!enc.contains("secret-db-pass"));
    assert_eq!(state.cipher.decrypt(&enc).expect("decrypt"), "secret-db-pass");

    // 2. Invalid login returns 401
    let bad_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"email": "analyst@enterprise.local", "password": "wrong"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_login.status(), StatusCode::UNAUTHORIZED);

    let admin_tok = login_as(&app, "admin@enterprise.local", "AdminPass!2026").await;
    let analyst_tok = login_as(&app, "analyst@enterprise.local", "AnalystPass!2026").await;
    let biz_tok = login_as(&app, "business@enterprise.local", "BusinessPass!2026").await;
    let auditor_tok = login_as(&app, "auditor@enterprise.local", "AuditorPass!2026").await;

    // 3. Connections & Providers NEVER expose plaintext or encrypted passwords/keys
    let conns_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/connections")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conns_resp.status(), StatusCode::OK);
    let conns_body = String::from_utf8(
        conns_resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(!conns_body.contains(&state.config.readonly_db_password));
    assert!(!conns_body.contains("encrypted_password"));

    // 4. Sensitive column tax_id is hidden from analyst in /schema and blocked in SQL execution
    let schema_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/schema")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(schema_resp.status(), StatusCode::OK);
    let schema_body = String::from_utf8(
        schema_resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(!schema_body.contains("tax_id"));
    assert!(!schema_body.contains("restricted_hr"));

    // 5. Business user is blocked from finance schema in /schema AND blocked from CSV export & audit logs
    let biz_schema = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/schema")
                .header(header::AUTHORIZATION, format!("Bearer {biz_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let biz_schema_body = String::from_utf8(
        biz_schema.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(!biz_schema_body.contains("department_expenses"));

    let biz_audit_denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/audit")
                .header(header::AUTHORIZATION, format!("Bearer {biz_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(biz_audit_denied.status(), StatusCode::FORBIDDEN);

    let biz_export_denied = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queries/export")
                .header(header::AUTHORIZATION, format!("Bearer {biz_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"query_id": uuid::Uuid::new_v4()}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(biz_export_denied.status(), StatusCode::FORBIDDEN);

    // 6. Analyst runs a real query -> exports CSV -> saves query -> creates dashboard
    let ask_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queries/ask")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "question": "Show the top ten products by profit.",
                        "clarify_ambiguity": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ask_resp.status(), StatusCode::OK);
    let ask_bytes = ask_resp.into_body().collect().await.unwrap().to_bytes();
    let ask_json: Value = serde_json::from_slice(&ask_bytes).unwrap();
    let qid = ask_json["query_id"].as_str().unwrap();

    let csv_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queries/export")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"query_id": qid}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(csv_resp.status(), StatusCode::OK);
    let csv_text = String::from_utf8(
        csv_resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(csv_text.contains("total_profit"));

    // Save query & clean up
    let save_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/saved-queries")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "query_execution_id": qid,
                        "title": "Integration Test Saved Query",
                        "question": "Top 10 profit",
                        "sql_text": ask_json["generated_sql"],
                        "is_shared": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(save_resp.status(), StatusCode::OK);
    let saved_json: Value = serde_json::from_slice(
        &save_resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    let saved_id = saved_json["id"].as_str().unwrap();

    // Cross-user private saved query deletion denied to business user!
    let del_denied = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/saved-queries/{saved_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {biz_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_denied.status(), StatusCode::NOT_FOUND);

    // Owner deletes cleanly
    let del_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/saved-queries/{saved_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_ok.status(), StatusCode::OK);

    // 7. Auditor can inspect audit logs; Admin can refresh schema
    let audit_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/audit")
                .header(header::AUTHORIZATION, format!("Bearer {auditor_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_resp.status(), StatusCode::OK);
    let audit_text = String::from_utf8(
        audit_resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(audit_text.contains("data_export"));
    assert!(audit_text.contains("sql_validation"));

    let refresh_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/schema/refresh")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refresh_resp.status(), StatusCode::OK);

    // 8. Manual execute-validated persists query_executions and works with CSV export
    let exec_manual = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queries/execute-validated")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "sql": "SELECT region_code, region_name FROM analytics.regions ORDER BY region_code",
                        "question": "Manual region lookup"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exec_manual.status(), StatusCode::OK);
    let manual_json: Value = serde_json::from_slice(
        &exec_manual.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    let manual_qid = manual_json["query_id"].as_str().unwrap();

    let manual_export = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/queries/export")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "query_id": manual_qid }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manual_export.status(), StatusCode::OK);

    // 9. HttpOnly cookie auth works for /auth/me and logout clears cookie
    let login_cookie_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "analyst@enterprise.local", "password": "AnalystPass!2026" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_cookie_resp.status(), StatusCode::OK);
    let set_cookie = login_cookie_resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(set_cookie.contains("dta_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let cookie_kv = set_cookie.split(';').next().unwrap().to_string();

    let me_via_cookie = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .header(header::COOKIE, cookie_kv)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me_via_cookie.status(), StatusCode::OK);

    let logout_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout_resp.status(), StatusCode::OK);
    let logout_cookie = logout_resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(logout_cookie.contains("Max-Age=0"));

    // 10. Rate limiter returns 429 Too Many Requests with Retry-After when exceeded
    let mut rl_state = state.clone();
    rl_state.config.login_rate_limit_per_min = 1;
    let rl_app = build_router(rl_state);
    let _first = rl_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "ratelimit@acme.local", "password": "wrong" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let second = rl_app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "ratelimit@acme.local", "password": "wrong" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(second.headers().contains_key(header::RETRY_AFTER));

    // 11. P1 Dashboard widget append + live AST-validated refresh + Admin user update guard
    let create_dash = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/dashboards")
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "title": "P1 Live Dashboard",
                        "description": "Live widget refresh test",
                        "is_shared": true,
                        "widgets": []
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_dash.status(), StatusCode::OK);
    let dash_json: Value = serde_json::from_slice(
        &create_dash.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    let dash_id = dash_json["id"].as_str().unwrap().to_string();

    let add_widget = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/dashboards/{dash_id}/widgets"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "widget": {
                            "title": "Regions Live",
                            "question": "List regions",
                            "sql_text": "SELECT region_code, region_name FROM analytics.regions ORDER BY region_code",
                            "chart_type": "bar",
                            "columns": [],
                            "rows": []
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add_widget.status(), StatusCode::OK);

    let refresh_dash = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/dashboards/{dash_id}/refresh"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refresh_dash.status(), StatusCode::OK);
    let refreshed_json: Value = serde_json::from_slice(
        &refresh_dash.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert!(!refreshed_json["widgets"][0]["rows"].as_array().unwrap().is_empty());
    assert!(refreshed_json["widgets"][0]["chart_spec"]["chart_suitable"].is_boolean());

    // Delete test dashboard cleanly
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/dashboards/{dash_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Non-admin cannot update users via PUT /admin/users/{id}
    let users_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/users")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(users_resp.status(), StatusCode::OK);
    let users_json: Value = serde_json::from_slice(
        &users_resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    let biz_user_id = users_json
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["email"] == "business@enterprise.local")
        .unwrap()["id"]
        .as_str()
        .unwrap();

    let non_admin_put = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/admin/users/{biz_user_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "is_active": false }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_admin_put.status(), StatusCode::FORBIDDEN);
}
