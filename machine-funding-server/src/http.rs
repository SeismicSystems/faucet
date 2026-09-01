use crate::{
    chain::ChainDriver,
    model::{
        ErrorBody, ErrorEnvelope, FundingAsset, FundingInput, FundingResponse, GasRequest,
        ServiceError, SusdcRequest,
    },
    service::FundingService,
    store::FundingStore,
};
use alloy_primitives::{Address, U256};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    str::FromStr,
    sync::{Arc, OnceLock},
};
use subtle::ConstantTimeEq;

const IDEMPOTENCY_PATTERN: &str = r"^[A-Za-z0-9._:-]{1,128}$";
const REASON_PATTERN: &str = r"^[A-Za-z0-9._:-]{1,64}$";

pub enum Erc20FundingService<S, D> {
    Disabled,
    Unavailable,
    Enabled(Box<FundingService<S, D>>),
}

struct RouterState<S, D> {
    legacy: FundingService<S, D>,
    erc20_usdc: Erc20FundingService<S, D>,
}

pub fn router<S, D>(legacy: FundingService<S, D>) -> Router
where
    S: FundingStore,
    D: ChainDriver,
{
    router_with_erc20(legacy, Erc20FundingService::Disabled)
}

pub fn router_with_erc20<S, D>(
    legacy: FundingService<S, D>,
    erc20_usdc: Erc20FundingService<S, D>,
) -> Router
where
    S: FundingStore,
    D: ChainDriver,
{
    Router::new()
        .route("/api/internal/health", get(liveness))
        .route("/api/internal/readiness", get(readiness::<S, D>))
        .route("/api/internal/transfers", post(transfer::<S, D>))
        .route("/api/internal/gas", post(gas::<S, D>))
        .route(
            "/api/internal/erc20-usdc/transfers",
            post(erc20_usdc_transfer::<S, D>),
        )
        .route(
            "/api/internal/erc20-usdc/readiness",
            get(erc20_usdc_readiness::<S, D>),
        )
        .with_state(Arc::new(RouterState { legacy, erc20_usdc }))
}

async fn transfer<S, D>(
    State(state): State<Arc<RouterState<S, D>>>,
    headers: HeaderMap,
    payload: Result<Json<SusdcRequest>, JsonRejection>,
) -> Result<Json<FundingResponse>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    let service = &state.legacy;
    authorize(&headers, &service.config().token)?;
    let Json(request) = payload.map_err(invalid_json)?;
    let common = validate_common(request.idempotency_key, request.recipient, request.reason)?;
    let amount = parse_amount(&request.amount, service.config().max_susdc_amount)?;
    service
        .execute(FundingInput {
            asset: FundingAsset::Susdc,
            amount,
            ..common
        })
        .await
        .map(Json)
}

async fn gas<S, D>(
    State(state): State<Arc<RouterState<S, D>>>,
    headers: HeaderMap,
    payload: Result<Json<GasRequest>, JsonRejection>,
) -> Result<Json<FundingResponse>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    let service = &state.legacy;
    authorize(&headers, &service.config().token)?;
    let Json(request) = payload.map_err(invalid_json)?;
    let common = validate_common(request.idempotency_key, request.recipient, request.reason)?;
    service
        .execute(FundingInput {
            asset: FundingAsset::SusdcGas,
            amount: service.config().gas_susdc_amount,
            ..common
        })
        .await
        .map(Json)
}

async fn erc20_usdc_transfer<S, D>(
    State(state): State<Arc<RouterState<S, D>>>,
    headers: HeaderMap,
    payload: Result<Json<SusdcRequest>, JsonRejection>,
) -> Result<Json<FundingResponse>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    authorize(&headers, &state.legacy.config().token)?;
    let service = erc20_service(&state)?;
    let maximum = service
        .config()
        .erc20_usdc
        .enabled()
        .map(|config| config.max_amount)
        .ok_or_else(erc20_unavailable)?;
    let Json(request) = payload.map_err(invalid_json)?;
    let common = validate_common(request.idempotency_key, request.recipient, request.reason)?;
    let amount = parse_amount(&request.amount, maximum)?;
    service
        .execute(FundingInput {
            asset: FundingAsset::Erc20Usdc,
            deployment_identity: service
                .config()
                .erc20_usdc
                .enabled()
                .map(|config| config.identity(service.config().chain_id)),
            amount,
            ..common
        })
        .await
        .map(Json)
}

async fn liveness() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn readiness<S, D>(
    State(state): State<Arc<RouterState<S, D>>>,
    headers: HeaderMap,
) -> Result<Json<Health>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    let service = &state.legacy;
    authorize(&headers, &service.config().token)?;
    service.health().await?;
    Ok(Json(Health { status: "ok" }))
}

async fn erc20_usdc_readiness<S, D>(
    State(state): State<Arc<RouterState<S, D>>>,
    headers: HeaderMap,
) -> Result<Json<Health>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    authorize(&headers, &state.legacy.config().token)?;
    erc20_service(&state)?.health().await?;
    Ok(Json(Health { status: "ok" }))
}

fn erc20_service<S, D>(state: &RouterState<S, D>) -> Result<&FundingService<S, D>, ServiceError>
where
    S: FundingStore,
    D: ChainDriver,
{
    match &state.erc20_usdc {
        Erc20FundingService::Enabled(service) => Ok(service),
        Erc20FundingService::Disabled => Err(ServiceError::new(
            404,
            "erc20_usdc_disabled",
            "ERC20 USDC funding is not enabled",
        )),
        Erc20FundingService::Unavailable => Err(erc20_unavailable()),
    }
}

fn erc20_unavailable() -> ServiceError {
    ServiceError::new(
        503,
        "erc20_usdc_unavailable",
        "ERC20 USDC funding is unavailable",
    )
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), ServiceError> {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    if bool::from(provided_hash.as_slice().ct_eq(expected_hash.as_slice())) {
        Ok(())
    } else {
        Err(ServiceError::new(
            401,
            "unauthorized",
            "Invalid bearer token",
        ))
    }
}

fn validate_common(
    idempotency_key: String,
    recipient_text: String,
    reason: String,
) -> Result<FundingInput, ServiceError> {
    static IDEMPOTENCY: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    static REASON: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let idempotency_pattern = compiled_pattern(&IDEMPOTENCY, IDEMPOTENCY_PATTERN)?;
    if !idempotency_pattern.is_match(&idempotency_key) {
        return Err(ServiceError::new(
            400,
            "invalid_idempotency_key",
            "idempotency_key must contain 1-128 letters, numbers, '.', '_', ':', or '-'",
        ));
    }
    let recipient = Address::from_str(&recipient_text).map_err(|_| {
        ServiceError::new(
            400,
            "invalid_recipient",
            "recipient must be a non-zero EVM address",
        )
    })?;
    if recipient == Address::ZERO {
        return Err(ServiceError::new(
            400,
            "invalid_recipient",
            "recipient must be a non-zero EVM address",
        ));
    }
    let reason_pattern = compiled_pattern(&REASON, REASON_PATTERN)?;
    if !reason_pattern.is_match(&reason) {
        return Err(ServiceError::new(
            400,
            "invalid_reason",
            "reason must contain 1-64 letters, numbers, '.', '_', ':', or '-'",
        ));
    }
    Ok(FundingInput {
        asset: FundingAsset::Susdc,
        deployment_identity: None,
        idempotency_key,
        recipient,
        recipient_text: recipient.to_checksum(None),
        amount: U256::ZERO,
        reason,
    })
}

fn compiled_pattern<'a>(
    cell: &'a OnceLock<Result<Regex, regex::Error>>,
    pattern: &str,
) -> Result<&'a Regex, ServiceError> {
    cell.get_or_init(|| Regex::new(pattern))
        .as_ref()
        .map_err(|error| {
            tracing::error!(%error, "machine funding validation pattern is invalid");
            ServiceError::internal()
        })
}

fn parse_amount(value: &str, maximum: U256) -> Result<U256, ServiceError> {
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ServiceError::new(
            400,
            "invalid_amount",
            "amount must be a positive decimal string in 6-decimal base units",
        ));
    }
    let amount = U256::from_str_radix(value, 10).map_err(|_| {
        ServiceError::new(
            400,
            "invalid_amount",
            "amount must be a positive decimal string in 6-decimal base units",
        )
    })?;
    if amount > maximum {
        return Err(ServiceError::new(
            400,
            "amount_exceeds_limit",
            format!("amount exceeds the configured limit of {maximum}"),
        ));
    }
    Ok(amount)
}

fn invalid_json(_: JsonRejection) -> ServiceError {
    ServiceError::new(400, "invalid_request", "Invalid JSON body")
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let retry_after_seconds = self.retry_after_seconds;
        let mut response = (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                    retry_after_seconds,
                    transaction_hash: self.transaction_hash,
                },
            }),
        )
            .into_response();
        if let Some(seconds) = retry_after_seconds {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_compares_fixed_length_hashes() {
        let token = "a-secure-machine-token-with-32-characters";
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert!(authorize(&headers, token).is_ok());
        headers.insert(header::AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert_eq!(authorize(&headers, token).unwrap_err().status, 401);
    }

    #[test]
    fn validates_addresses_and_decimal_base_units() {
        let input = validate_common(
            "order-123".into(),
            "0x00000000000000000000000000000000000000a1".into(),
            "order_payout".into(),
        )
        .unwrap();
        assert_eq!(
            input.recipient_text,
            "0x00000000000000000000000000000000000000A1"
        );
        assert_eq!(
            parse_amount("12500000", U256::from(250_000_000u64)).unwrap(),
            U256::from(12_500_000u64)
        );
        assert!(parse_amount("0", U256::MAX).is_err());
        assert!(parse_amount("01", U256::MAX).is_err());
    }
}
