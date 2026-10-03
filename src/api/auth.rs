use axum::extract::Request;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::config::Config;
use crate::error::TentacleError;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iss: Option<String>,
    pub aud: Option<String>,
    pub server_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthQuery {
    pub token: Option<String>,
}

pub struct TokenValidator;

impl TokenValidator {
    pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, TentacleError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;
        validation.validate_aud = false;

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .map_err(|e| TentacleError::Auth(format!("Invalid JWT token: {}", e)))?;

        Ok(token_data.claims)
    }

    pub fn verify_hmac(payload: &[u8], signature: &str, secret: &str) -> Result<(), TentacleError> {
        let expected_bytes = hex::decode(signature)
            .map_err(|_| TentacleError::Auth("Invalid hex signature format".to_string()))?;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| TentacleError::Auth(format!("HMAC key initialization error: {}", e)))?;

        mac.update(payload);
        mac.verify_slice(&expected_bytes)
            .map_err(|_| TentacleError::Auth("HMAC signature verification failed".to_string()))?;

        Ok(())
    }

    pub fn extract_token(req: &Request) -> Option<String> {
        // 1. Authorization: Bearer <token>
        if let Some(auth_header) = req.headers().get(AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    return Some(token.trim().to_string());
                }
            }
        }

        // 2. Query param ?token=...
        if let Some(query) = req.uri().query() {
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    if k == "token" {
                        return Some(v.to_string());
                    }
                }
            }
        }

        None
    }
}

pub async fn auth_middleware(
    config: &Config,
    req: Request,
    next: Next,
) -> Result<Response, TentacleError> {
    let token = TokenValidator::extract_token(&req)
        .ok_or_else(|| TentacleError::Auth("Missing authorization header or token".to_string()))?;

    // Accept raw panel secret or valid JWT token
    if token != config.auth.panel_secret {
        let _ = TokenValidator::verify_jwt(&token, &config.auth.panel_secret)?;
    }

    Ok(next.run(req).await)
}
