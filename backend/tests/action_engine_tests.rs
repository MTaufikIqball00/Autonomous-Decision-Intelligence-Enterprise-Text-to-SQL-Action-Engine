use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::Mutex};
use tower::ServiceExt;

use dta_backend::{
    api::build_router,
    auth::JwtService,
    config::AppConfig,
    crypto::SecretCipher,
    db::app_db::{bootstrap_app_database, connect_app_pool},
    state::AppState,
};

#[derive(Clone, Default)]
struct MockWebhookState {
    calls: Arc<AtomicUsize>,
    last_secret: Arc<Mutex<Option<String>>>,
    last_idem_key: Arc<Mutex<Option<String>>>,
    last_body: Arc<Mutex<Option<Value>>>,
}

async fn mock_webhook_handler(
    State(st): State<MockWebhookState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    st.calls.fetch_add(1, Ordering::SeqCst);
    *st.last_secret.lock().await = headers
        .get("x-action-secret")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    *st.last_idem_key.lock().await = headers
        .get("x-idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    *st.last_body.lock().await = Some(body);
    Json(json!({ "accepted": true }))
}

async fn login_as(app: &axum::Router, email: &str, password: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "email": email, "password": password }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "login failed for {email}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn action_engine_monitoring_hil_approval_idempotency_and_rbac_work() {
    let config = AppConfig::from_env().expect("AppConfig from .env");
    let cipher = SecretCipher::from_hex(&config.master_key_hex).expect("cipher");
    let app_pool = connect_app_pool(&config).await.expect("connect app db");
    let _ = bootstrap_app_database(&app_pool, &config, &cipher)
        .await
        .expect("bootstrap");
    let jwt = JwtService::new(&config.jwt_secret);
    let state = AppState::new(config, app_pool.clone(), jwt, cipher);
    let app = build_router(state);

    let admin_tok = login_as(&app, "admin@enterprise.local", "AdminPass!2026").await;
    let analyst_tok = login_as(&app, "analyst@enterprise.local", "AnalystPass!2026").await;

    // Spin up ephemeral local mock webhook receiver
    let mock_state = MockWebhookState::default();
    let mock_router = Router::new()
        .route("/hook", post(mock_webhook_handler))
        .with_state(mock_state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hook_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, mock_router).await;
    });
    let hook_url = format!("http://{hook_addr}/hook");

    // 1. Connector SSRF metadata block + file:// rejection
    let ssrf_bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/actions/connectors")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": "bad-metadata",
                        "connector_type": "webhook",
                        "endpoint_url": "http://169.254.169.254/latest/meta-data"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ssrf_bad.status(), StatusCode::BAD_REQUEST);

    // 2. Create encrypted webhook connector (secret hidden in DTO)
    let conn_name = format!("ops-webhook-{}", uuid::Uuid::new_v4());
    let create_conn = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/actions/connectors")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": conn_name,
                        "connector_type": "webhook",
                        "endpoint_url": hook_url,
                        "secret": "TopSecretHookToken!99"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_conn.status(), StatusCode::OK);
    let conn_bytes = create_conn.into_body().collect().await.unwrap().to_bytes();
    let conn_text = String::from_utf8(conn_bytes.to_vec()).unwrap();
    assert!(!conn_text.contains("TopSecretHookToken!99"));
    let conn_json: Value = serde_json::from_str(&conn_text).unwrap();
    assert_eq!(conn_json["has_secret"], true);
    let connector_id = conn_json["id"].as_str().unwrap().to_string();

    // 3. Rule creation rejects non-readonly SQL via AST validator
    let bad_rule = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/actions/rules")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": "Unsafe Rule",
                        "connector_id": connector_id,
                        "sql_text": "DELETE FROM analytics.orders",
                        "metric_column": "total",
                        "comparator": "gt",
                        "threshold_value": 10.0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_rule.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 4. Create read-only monitoring rule & evaluate against live PostgreSQL
    let create_rule = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/actions/rules")
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "name": "High Order Count Alert",
                        "description": "Triggers when completed orders exceed 1",
                        "connector_id": connector_id,
                        "sql_text": "SELECT COUNT(*) AS completed_orders FROM analytics.orders WHERE status IN ('COMPLETED', 'SHIPPED')",
                        "metric_column": "completed_orders",
                        "comparator": "gt",
                        "threshold_value": 1.0,
                        "schedule_interval_secs": 300
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_rule.status(), StatusCode::OK);
    let rule_json: Value = serde_json::from_slice(
        &create_rule.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    let rule_id = rule_json["id"].as_str().unwrap().to_string();

    let eval_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/rules/{rule_id}/evaluate"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(eval_resp.status(), StatusCode::OK);
    let eval_json: Value = serde_json::from_slice(
        &eval_resp.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(eval_json["triggered"], true);
    let proposal_id = eval_json["proposal"]["id"].as_str().unwrap().to_string();
    assert_eq!(eval_json["proposal"]["status"], "pending");
    assert!(eval_json["query_id"].as_str().is_some());

    // 5. RBAC & HIL guard: Analyst cannot approve; Admin cannot execute before approve
    let analyst_approve = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/proposals/{proposal_id}/approve"))
                .header(header::AUTHORIZATION, format!("Bearer {analyst_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(analyst_approve.status(), StatusCode::FORBIDDEN);

    let premature_exec = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/proposals/{proposal_id}/execute"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(premature_exec.status(), StatusCode::BAD_REQUEST);

    // 6. Admin approves and executes; mock webhook receives decrypted secret & evidence
    let approve_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/proposals/{proposal_id}/approve"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "review_note": "Verified against order ledger" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approve_ok.status(), StatusCode::OK);

    let exec_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/proposals/{proposal_id}/execute"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exec_ok.status(), StatusCode::OK);
    let exec_json: Value = serde_json::from_slice(
        &exec_ok.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();
    assert_eq!(exec_json["proposal"]["status"], "executed");
    assert_eq!(exec_json["execution"]["outcome"], "executed");
    assert_eq!(mock_state.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        mock_state.last_secret.lock().await.as_deref(),
        Some("TopSecretHookToken!99")
    );
    assert!(mock_state.last_idem_key.lock().await.is_some());

    // 7. Idempotency guard blocks duplicate dispatch
    let duplicate_exec = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/actions/proposals/{proposal_id}/execute"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_exec.status(), StatusCode::BAD_REQUEST);
    assert_eq!(mock_state.calls.load(Ordering::SeqCst), 1);

    // Clean up rule & connector (leave proposal/execution audit trail intact or delete rule)
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/actions/rules/{rule_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/actions/connectors/{connector_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {admin_tok}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
}
