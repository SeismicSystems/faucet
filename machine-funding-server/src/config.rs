use alloy_primitives::{Address, U256};
use std::{env, net::SocketAddr, str::FromStr, time::Duration};
use thiserror::Error;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3002";
const DEFAULT_RATE_LIMIT: u64 = 10;
const DEFAULT_RATE_WINDOW_SECONDS: u64 = 3_600;
const DEFAULT_CONFIRMATIONS: u64 = 1;
const DEFAULT_RECEIPT_TIMEOUT_MS: u64 = 20_000;
const DEFAULT_LOCK_WAIT_MS: u64 = 5_000;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 45_000;
pub const MAX_REQUEST_TIMEOUT_MS: u64 = 45_000;
const MINIMUM_TOKEN_LENGTH: usize = 32;
const MAX_REDIS_INTEGER: u64 = i64::MAX as u64;

#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub redis_url: String,
    pub rpc_url: String,
    pub chain_id: u64,
    pub token: String,
    pub private_key: String,
    pub funding_address: Address,
    pub faucet_address: Address,
    pub max_susdc_amount: U256,
    pub gas_amount_wei: U256,
    pub global_susdc_budget: U256,
    pub global_gas_budget_wei: U256,
    pub rate_limit: u64,
    pub rate_window: Duration,
    pub confirmations: u64,
    pub receipt_timeout: Duration,
    pub lock_wait: Duration,
    pub request_timeout: Duration,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} is invalid")]
    Invalid(&'static str),
    #[error("{0} must be at least {1} characters")]
    TooShort(&'static str, usize),
    #[error("single-transfer amount exceeds its global budget")]
    BudgetBelowTransfer,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |name: &'static str| lookup(name).ok_or(ConfigError::Missing(name));
        let token = required("INTERNAL_FUNDING_TOKEN")?;
        if token.len() < MINIMUM_TOKEN_LENGTH {
            return Err(ConfigError::TooShort(
                "INTERNAL_FUNDING_TOKEN",
                MINIMUM_TOKEN_LENGTH,
            ));
        }
        let private_key = required("INTERNAL_FUNDING_PRIVATE_KEY")?;
        validate_private_key(&private_key)?;

        let max_susdc_amount = parse_u256(
            required("INTERNAL_FUNDING_MAX_SUSDC_AMOUNT")?,
            "INTERNAL_FUNDING_MAX_SUSDC_AMOUNT",
        )?;
        let gas_amount_wei = parse_u256(
            required("INTERNAL_FUNDING_GAS_AMOUNT_WEI")?,
            "INTERNAL_FUNDING_GAS_AMOUNT_WEI",
        )?;
        let global_susdc_budget = parse_u256(
            required("INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET")?,
            "INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET",
        )?;
        let global_gas_budget_wei = parse_u256(
            required("INTERNAL_FUNDING_GLOBAL_GAS_BUDGET_WEI")?,
            "INTERNAL_FUNDING_GLOBAL_GAS_BUDGET_WEI",
        )?;
        if max_susdc_amount > global_susdc_budget || gas_amount_wei > global_gas_budget_wei {
            return Err(ConfigError::BudgetBelowTransfer);
        }

        let bind_addr: SocketAddr = lookup("INTERNAL_FUNDING_BIND_ADDR")
            .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned())
            .parse()
            .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_BIND_ADDR"))?;
        if !bind_addr.ip().is_loopback() {
            return Err(ConfigError::Invalid("INTERNAL_FUNDING_BIND_ADDR"));
        }
        let request_timeout_ms = parse_u64(
            lookup("INTERNAL_FUNDING_REQUEST_TIMEOUT_MS"),
            DEFAULT_REQUEST_TIMEOUT_MS,
            "INTERNAL_FUNDING_REQUEST_TIMEOUT_MS",
        )?;
        if request_timeout_ms > MAX_REQUEST_TIMEOUT_MS {
            return Err(ConfigError::Invalid("INTERNAL_FUNDING_REQUEST_TIMEOUT_MS"));
        }

        Ok(Self {
            bind_addr,
            redis_url: required("REDIS_URL")?,
            rpc_url: required("RPC_URL")?,
            chain_id: required("INTERNAL_FUNDING_CHAIN_ID")?
                .parse()
                .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_CHAIN_ID"))?,
            token,
            private_key,
            funding_address: Address::from_str(&required("INTERNAL_FUNDING_ADDRESS")?)
                .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_ADDRESS"))?,
            faucet_address: Address::from_str(&required("FAUCET_ADDRESS")?)
                .map_err(|_| ConfigError::Invalid("FAUCET_ADDRESS"))?,
            max_susdc_amount,
            gas_amount_wei,
            global_susdc_budget,
            global_gas_budget_wei,
            rate_limit: parse_u64(
                lookup("INTERNAL_FUNDING_RATE_LIMIT"),
                DEFAULT_RATE_LIMIT,
                "INTERNAL_FUNDING_RATE_LIMIT",
            )?,
            rate_window: Duration::from_secs(parse_u64(
                lookup("INTERNAL_FUNDING_RATE_WINDOW_SECONDS"),
                DEFAULT_RATE_WINDOW_SECONDS,
                "INTERNAL_FUNDING_RATE_WINDOW_SECONDS",
            )?),
            confirmations: parse_u64(
                lookup("INTERNAL_FUNDING_CONFIRMATIONS"),
                DEFAULT_CONFIRMATIONS,
                "INTERNAL_FUNDING_CONFIRMATIONS",
            )?,
            receipt_timeout: Duration::from_millis(parse_u64(
                lookup("INTERNAL_FUNDING_RECEIPT_TIMEOUT_MS"),
                DEFAULT_RECEIPT_TIMEOUT_MS,
                "INTERNAL_FUNDING_RECEIPT_TIMEOUT_MS",
            )?),
            lock_wait: Duration::from_millis(parse_u64(
                lookup("INTERNAL_FUNDING_LOCK_WAIT_MS"),
                DEFAULT_LOCK_WAIT_MS,
                "INTERNAL_FUNDING_LOCK_WAIT_MS",
            )?),
            request_timeout: Duration::from_millis(request_timeout_ms),
        })
    }
}

fn validate_private_key(value: &str) -> Result<(), ConfigError> {
    if value.len() != 66 || !value.starts_with("0x") || hex::decode(&value[2..]).is_err() {
        return Err(ConfigError::Invalid("INTERNAL_FUNDING_PRIVATE_KEY"));
    }
    Ok(())
}

fn parse_u256(value: String, name: &'static str) -> Result<U256, ConfigError> {
    let parsed = U256::from_str_radix(&value, 10).map_err(|_| ConfigError::Invalid(name))?;
    if parsed == U256::ZERO || parsed > U256::from(MAX_REDIS_INTEGER) {
        return Err(ConfigError::Invalid(name));
    }
    Ok(parsed)
}

fn parse_u64(value: Option<String>, default: u64, name: &'static str) -> Result<u64, ConfigError> {
    let parsed = value
        .map(|raw| raw.parse().map_err(|_| ConfigError::Invalid(name)))
        .transpose()?
        .unwrap_or(default);
    if parsed == 0 {
        return Err(ConfigError::Invalid(name));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const _: () = assert!(DEFAULT_REQUEST_TIMEOUT_MS < 50_000);

    fn valid_env() -> HashMap<&'static str, String> {
        HashMap::from([
            ("REDIS_URL", "redis://127.0.0.1/".into()),
            ("RPC_URL", "http://127.0.0.1:8545".into()),
            ("INTERNAL_FUNDING_CHAIN_ID", "5124".into()),
            ("INTERNAL_FUNDING_TOKEN", "x".repeat(32)),
            (
                "INTERNAL_FUNDING_PRIVATE_KEY",
                format!("0x{}", "1".repeat(64)),
            ),
            (
                "INTERNAL_FUNDING_ADDRESS",
                "0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A".into(),
            ),
            (
                "FAUCET_ADDRESS",
                "0x0000000000000000000000000000000000000001".into(),
            ),
            ("INTERNAL_FUNDING_MAX_SUSDC_AMOUNT", "250000000".into()),
            (
                "INTERNAL_FUNDING_GAS_AMOUNT_WEI",
                "10000000000000000".into(),
            ),
            ("INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET", "1000000000".into()),
            (
                "INTERNAL_FUNDING_GLOBAL_GAS_BUDGET_WEI",
                "1000000000000000000".into(),
            ),
        ])
    }

    #[test]
    fn loads_defaults_and_required_limits() {
        let values = valid_env();
        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:3002".parse().unwrap());
        assert_eq!(config.rate_limit, 10);
        assert_eq!(config.receipt_timeout, Duration::from_secs(20));
        assert_eq!(config.request_timeout, Duration::from_secs(45));
    }

    #[test]
    fn rejects_weak_token_and_oversized_transfer() {
        let mut values = valid_env();
        values.insert("INTERNAL_FUNDING_TOKEN", "short".into());
        assert!(Config::from_lookup(|key| values.get(key).cloned()).is_err());
        values.insert("INTERNAL_FUNDING_TOKEN", "x".repeat(32));
        values.insert("INTERNAL_FUNDING_MAX_SUSDC_AMOUNT", "1000000001".into());
        assert!(matches!(
            Config::from_lookup(|key| values.get(key).cloned()),
            Err(ConfigError::BudgetBelowTransfer)
        ));
    }

    #[test]
    fn rejects_non_loopback_bind_and_oversized_request_timeout() {
        let mut values = valid_env();
        values.insert("INTERNAL_FUNDING_BIND_ADDR", "0.0.0.0:3002".into());
        assert!(matches!(
            Config::from_lookup(|key| values.get(key).cloned()),
            Err(ConfigError::Invalid("INTERNAL_FUNDING_BIND_ADDR"))
        ));

        values.insert("INTERNAL_FUNDING_BIND_ADDR", "[::1]:3002".into());
        values.insert("INTERNAL_FUNDING_REQUEST_TIMEOUT_MS", "45001".into());
        assert!(matches!(
            Config::from_lookup(|key| values.get(key).cloned()),
            Err(ConfigError::Invalid("INTERNAL_FUNDING_REQUEST_TIMEOUT_MS"))
        ));
    }

    #[test]
    fn timeout_envelope_keeps_proxy_and_client_margin() {
        let nginx = include_str!("../../deploy/nginx.conf");
        assert!(nginx.contains("proxy_read_timeout 50s"));
    }
}
