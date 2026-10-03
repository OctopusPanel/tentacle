use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::Value;
use tower::ServiceExt;

use tentacle::api::auth::{Claims, TokenValidator};
use tentacle::api::create_router;
use tentacle::config::Config;
use tentacle::state::AppState;

#[test]
fn test_jwt_and_hmac_token_validation() {
    let secret = "test-secret-key-1234567890-test-key";

    // 1. Valid JWT
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = Claims {
        sub: "user-123".to_string(),
        exp: (now + 3600) as usize,
        iss: Some("octopus-panel".to_string()),
        aud: Some("tentacle-node".to_string()),
        server_id: Some("srv-abc-123".to_string()),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("failed to encode jwt");

    let decoded = TokenValidator::verify_jwt(&token, secret).expect("failed to decode jwt");
    assert_eq!(decoded.sub, "user-123");
    assert_eq!(decoded.server_id, Some("srv-abc-123".to_string()));

    // 2. Expired JWT
    let expired_claims = Claims {
        sub: "user-123".to_string(),
        exp: (now - 3600) as usize,
        iss: Some("octopus-panel".to_string()),
        aud: None,
        server_id: None,
    };
    let expired_token = encode(
        &Header::default(),
        &expired_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();
    assert!(TokenValidator::verify_jwt(&expired_token, secret).is_err());

    // 3. HMAC signature verification
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let payload = b"{\"action\":\"start\",\"server_id\":\"123\"}";
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload);
    let signature = hex::encode(mac.finalize().into_bytes());

    assert!(TokenValidator::verify_hmac(payload, &signature, secret).is_ok());
    assert!(TokenValidator::verify_hmac(b"altered payload", &signature, secret).is_err());
}

#[tokio::test]
async fn test_api_public_health_and_protected_routes() {
    let mut config = Config::default();
    config.auth.panel_secret = "secret-panel-key".to_string();

    let state = AppState::new(config).expect("failed to init state");
    let app = create_router(state);

    // 1. Health check (public)
    let req = Request::builder()
        .uri("/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 2. Protected route without auth (should return 403 Forbidden)
    let req = Request::builder()
        .uri("/api/system")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // 3. Protected route with valid bearer token
    let req = Request::builder()
        .uri("/api/system")
        .method("GET")
        .header(header::AUTHORIZATION, "Bearer secret-panel-key")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["node_id"], "node-local-01");
}
