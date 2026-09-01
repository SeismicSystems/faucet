use crate::model::Erc20DeploymentIdentity;
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
const MINIMUM_GAS_SUSDC_AMOUNT: u64 = 100_000;
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
    pub gas_susdc_amount: U256,
    pub global_susdc_budget: U256,
    pub global_gas_susdc_budget: U256,
    pub erc20_usdc: Erc20UsdcActivation,
    pub rate_limit: u64,
    pub rate_window: Duration,
    pub confirmations: u64,
    pub receipt_timeout: Duration,
    pub lock_wait: Duration,
    pub request_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Erc20UsdcConfig {
    pub token_address: Address,
    pub faucet_address: Address,
    pub private_key: String,
    pub funding_address: Address,
    pub reserve_address: Address,
    pub max_amount: U256,
    pub global_budget: U256,
    pub rate_limit: u64,
    pub rate_window: Duration,
}

impl Erc20UsdcConfig {
    pub fn identity(&self, chain_id: u64) -> Erc20DeploymentIdentity {
        Erc20DeploymentIdentity {
            chain_id,
            token_address: self.token_address.to_checksum(None),
            faucet_address: self.faucet_address.to_checksum(None),
            operator_address: self.funding_address.to_checksum(None),
            reserve_address: self.reserve_address.to_checksum(None),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Erc20UsdcActivation {
    Disabled,
    Invalid(String),
    Enabled(Erc20UsdcConfig),
}

impl Erc20UsdcActivation {
    pub fn enabled(&self) -> Option<&Erc20UsdcConfig> {
        match self {
            Self::Enabled(config) => Some(config),
            Self::Disabled | Self::Invalid(_) => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{0} is invalid")]
    Invalid(&'static str),
    #[error("{0} must be at least {1} characters")]
    TooShort(&'static str, usize),
    #[error("{0} must be at least {1}")]
    BelowMinimum(&'static str, u64),
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
        validate_private_key(&private_key, "INTERNAL_FUNDING_PRIVATE_KEY")?;

        let max_susdc_amount = parse_u256(
            required("INTERNAL_FUNDING_MAX_SUSDC_AMOUNT")?,
            "INTERNAL_FUNDING_MAX_SUSDC_AMOUNT",
        )?;
        let gas_susdc_amount = parse_u256(
            required("INTERNAL_FUNDING_GAS_SUSDC_AMOUNT")?,
            "INTERNAL_FUNDING_GAS_SUSDC_AMOUNT",
        )?;
        if gas_susdc_amount < U256::from(MINIMUM_GAS_SUSDC_AMOUNT) {
            return Err(ConfigError::BelowMinimum(
                "INTERNAL_FUNDING_GAS_SUSDC_AMOUNT",
                MINIMUM_GAS_SUSDC_AMOUNT,
            ));
        }
        let global_susdc_budget = parse_u256(
            required("INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET")?,
            "INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET",
        )?;
        let global_gas_susdc_budget = parse_u256(
            required("INTERNAL_FUNDING_GLOBAL_GAS_SUSDC_BUDGET")?,
            "INTERNAL_FUNDING_GLOBAL_GAS_SUSDC_BUDGET",
        )?;
        if max_susdc_amount > global_susdc_budget || gas_susdc_amount > global_gas_susdc_budget {
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

        let chain_id = required("INTERNAL_FUNDING_CHAIN_ID")?
            .parse()
            .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_CHAIN_ID"))?;
        let faucet_address = Address::from_str(&required("FAUCET_ADDRESS")?)
            .map_err(|_| ConfigError::Invalid("FAUCET_ADDRESS"))?;
        let funding_address = Address::from_str(&required("INTERNAL_FUNDING_ADDRESS")?)
            .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_ADDRESS"))?;
        let erc20_usdc =
            match parse_erc20_usdc_config(&lookup, chain_id, faucet_address, funding_address) {
                Ok(Some(config)) => Erc20UsdcActivation::Enabled(config),
                Ok(None) => Erc20UsdcActivation::Disabled,
                Err(error) => Erc20UsdcActivation::Invalid(error.to_string()),
            };

        Ok(Self {
            bind_addr,
            redis_url: required("REDIS_URL")?,
            rpc_url: required("RPC_URL")?,
            chain_id,
            token,
            private_key,
            funding_address,
            faucet_address,
            max_susdc_amount,
            gas_susdc_amount,
            global_susdc_budget,
            global_gas_susdc_budget,
            erc20_usdc,
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

fn parse_erc20_usdc_config(
    lookup: &impl Fn(&str) -> Option<String>,
    chain_id: u64,
    susdc_faucet_address: Address,
    susdc_funding_address: Address,
) -> Result<Option<Erc20UsdcConfig>, ConfigError> {
    let enabled = match lookup("INTERNAL_FUNDING_ERC20_USDC_ENABLED").as_deref() {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => {
            return Err(ConfigError::Invalid("INTERNAL_FUNDING_ERC20_USDC_ENABLED"));
        }
    };
    if !enabled {
        return Ok(None);
    }

    let required = |name: &'static str| lookup(name).ok_or(ConfigError::Missing(name));
    let token_address = Address::from_str(&required("ERC20_USDC_TOKEN_ADDRESS")?)
        .map_err(|_| ConfigError::Invalid("ERC20_USDC_TOKEN_ADDRESS"))?;
    let faucet_address = Address::from_str(&required("ERC20_USDC_FAUCET_ADDRESS")?)
        .map_err(|_| ConfigError::Invalid("ERC20_USDC_FAUCET_ADDRESS"))?;
    let private_key = required("INTERNAL_FUNDING_ERC20_USDC_PRIVATE_KEY")?;
    validate_private_key(&private_key, "INTERNAL_FUNDING_ERC20_USDC_PRIVATE_KEY")?;
    let funding_address = Address::from_str(&required("INTERNAL_FUNDING_ERC20_USDC_ADDRESS")?)
        .map_err(|_| ConfigError::Invalid("INTERNAL_FUNDING_ERC20_USDC_ADDRESS"))?;
    let reserve_address = Address::from_str(&required("ERC20_USDC_RESERVE_ADDRESS")?)
        .map_err(|_| ConfigError::Invalid("ERC20_USDC_RESERVE_ADDRESS"))?;
    if chain_id == 0
        || token_address == Address::ZERO
        || faucet_address == Address::ZERO
        || token_address == faucet_address
        || faucet_address == susdc_faucet_address
        || funding_address == Address::ZERO
        || reserve_address == Address::ZERO
        || funding_address == susdc_funding_address
        || reserve_address == funding_address
    {
        return Err(ConfigError::Invalid("ERC20_USDC_FAUCET_ADDRESS"));
    }
    let max_amount = parse_u256(
        required("INTERNAL_FUNDING_MAX_ERC20_USDC_AMOUNT")?,
        "INTERNAL_FUNDING_MAX_ERC20_USDC_AMOUNT",
    )?;
    let global_budget = parse_u256(
        required("INTERNAL_FUNDING_GLOBAL_ERC20_USDC_BUDGET")?,
        "INTERNAL_FUNDING_GLOBAL_ERC20_USDC_BUDGET",
    )?;
    if max_amount > global_budget {
        return Err(ConfigError::BudgetBelowTransfer);
    }
    let rate_limit = parse_u64(
        lookup("INTERNAL_FUNDING_ERC20_USDC_RATE_LIMIT"),
        DEFAULT_RATE_LIMIT,
        "INTERNAL_FUNDING_ERC20_USDC_RATE_LIMIT",
    )?;
    let rate_window = Duration::from_secs(parse_u64(
        lookup("INTERNAL_FUNDING_ERC20_USDC_RATE_WINDOW_SECONDS"),
        DEFAULT_RATE_WINDOW_SECONDS,
        "INTERNAL_FUNDING_ERC20_USDC_RATE_WINDOW_SECONDS",
    )?);

    Ok(Some(Erc20UsdcConfig {
        token_address,
        faucet_address,
        private_key,
        funding_address,
        reserve_address,
        max_amount,
        global_budget,
        rate_limit,
        rate_window,
    }))
}

fn validate_private_key(value: &str, name: &'static str) -> Result<(), ConfigError> {
    if value.len() != 66 || !value.starts_with("0x") || hex::decode(&value[2..]).is_err() {
        return Err(ConfigError::Invalid(name));
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
                "INTERNAL_FUNDING_GAS_SUSDC_AMOUNT",
                MINIMUM_GAS_SUSDC_AMOUNT.to_string(),
            ),
            ("INTERNAL_FUNDING_GLOBAL_SUSDC_BUDGET", "1000000000".into()),
            (
                "INTERNAL_FUNDING_GLOBAL_GAS_SUSDC_BUDGET",
                "10000000".into(),
            ),
        ])
    }

    fn enable_erc20_usdc(values: &mut HashMap<&'static str, String>) {
        values.insert("INTERNAL_FUNDING_ERC20_USDC_ENABLED", "true".into());
        values.insert(
            "ERC20_USDC_TOKEN_ADDRESS",
            "0x0000000000000000000000000000000000000010".into(),
        );
        values.insert(
            "ERC20_USDC_FAUCET_ADDRESS",
            "0x0000000000000000000000000000000000000020".into(),
        );
        values.insert(
            "INTERNAL_FUNDING_ERC20_USDC_PRIVATE_KEY",
            format!("0x{}", "2".repeat(64)),
        );
        values.insert(
            "INTERNAL_FUNDING_ERC20_USDC_ADDRESS",
            "0x1563915e194D8CfBA1943570603F7606A3115508".into(),
        );
        values.insert(
            "ERC20_USDC_RESERVE_ADDRESS",
            "0x0000000000000000000000000000000000000030".into(),
        );
        values.insert("INTERNAL_FUNDING_MAX_ERC20_USDC_AMOUNT", "250000000".into());
        values.insert(
            "INTERNAL_FUNDING_GLOBAL_ERC20_USDC_BUDGET",
            "1000000000".into(),
        );
    }

    #[test]
    fn loads_defaults_and_required_limits() {
        let values = valid_env();
        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:3002".parse().unwrap());
        assert_eq!(config.rate_limit, 10);
        assert_eq!(config.receipt_timeout, Duration::from_secs(20));
        assert_eq!(config.request_timeout, Duration::from_secs(45));
        assert_eq!(config.erc20_usdc, Erc20UsdcActivation::Disabled);
    }

    #[test]
    fn loads_erc20_usdc_only_when_explicitly_enabled() {
        let mut values = valid_env();
        enable_erc20_usdc(&mut values);

        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        let erc20 = config.erc20_usdc.enabled().unwrap();
        assert_eq!(erc20.max_amount, U256::from(250_000_000u64));
        assert_eq!(erc20.rate_limit, DEFAULT_RATE_LIMIT);
        assert_eq!(erc20.rate_window, Duration::from_secs(3_600));
        assert_eq!(
            erc20.identity(config.chain_id).scope(),
            concat!(
                "erc20_usdc:5124:",
                "0x0000000000000000000000000000000000000010:",
                "0x0000000000000000000000000000000000000020:",
                "0x1563915e194d8cfba1943570603f7606a3115508:",
                "0x0000000000000000000000000000000000000030"
            )
        );
    }

    #[test]
    fn ignores_erc20_usdc_values_until_enabled() {
        let mut values = valid_env();
        values.insert("ERC20_USDC_TOKEN_ADDRESS", "invalid".into());
        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert_eq!(config.erc20_usdc, Erc20UsdcActivation::Disabled);
    }

    #[test]
    fn invalid_erc20_usdc_config_does_not_prevent_legacy_startup() {
        let mut values = valid_env();
        values.insert("INTERNAL_FUNDING_ERC20_USDC_ENABLED", "true".into());
        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert!(matches!(config.erc20_usdc, Erc20UsdcActivation::Invalid(_)));

        enable_erc20_usdc(&mut values);
        values.insert("INTERNAL_FUNDING_MAX_ERC20_USDC_AMOUNT", "250000001".into());
        values.insert(
            "INTERNAL_FUNDING_GLOBAL_ERC20_USDC_BUDGET",
            "250000000".into(),
        );
        let config = Config::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert_eq!(
            config.erc20_usdc,
            Erc20UsdcActivation::Invalid(ConfigError::BudgetBelowTransfer.to_string())
        );
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
    fn rejects_gas_drips_too_small_for_a_signed_read() {
        let mut values = valid_env();
        values.insert("INTERNAL_FUNDING_GAS_SUSDC_AMOUNT", "99999".into());
        assert!(matches!(
            Config::from_lookup(|key| values.get(key).cloned()),
            Err(ConfigError::BelowMinimum(
                "INTERNAL_FUNDING_GAS_SUSDC_AMOUNT",
                MINIMUM_GAS_SUSDC_AMOUNT
            ))
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
