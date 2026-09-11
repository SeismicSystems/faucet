use crate::model::{FundingRecord, ReservationResult, ServiceError};
use async_trait::async_trait;
use redis::{
    aio::{ConnectionManager, ConnectionManagerConfig},
    AsyncCommands, Script,
};
use std::time::Duration;

const REDIS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
const REDIS_CONNECTION_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_REVERTED_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end
redis.call('SET', KEYS[3], ARGV[1], 'NX')
redis.call('SET', KEYS[1], ARGV[2])
redis.call('LREM', KEYS[2], 0, KEYS[1])
redis.call('RPUSH', KEYS[2], KEYS[1])
return 1
"#;

const RESERVE_AND_ENQUEUE_SCRIPT: &str = r#"
if redis.call('EXISTS', KEYS[1]) == 1 then return {0, 0} end
local recipientCount = tonumber(redis.call('GET', KEYS[3]) or '0')
if recipientCount >= tonumber(ARGV[3]) then return {-1, redis.call('TTL', KEYS[3])} end
local globalExisted = redis.call('EXISTS', KEYS[4])
redis.call('INCRBY', KEYS[4], ARGV[4])
local globalAmount = redis.call('GET', KEYS[4])
local function greaterThan(left, right)
  left = string.gsub(left, '^0+', '')
  right = string.gsub(right, '^0+', '')
  if string.len(left) ~= string.len(right) then return string.len(left) > string.len(right) end
  return left > right
end
if greaterThan(globalAmount, ARGV[5]) then
  redis.call('DECRBY', KEYS[4], ARGV[4])
  if globalExisted == 0 then redis.call('DEL', KEYS[4]) end
  return {-2, redis.call('TTL', KEYS[4])}
end
local newRecipientCount = redis.call('INCR', KEYS[3])
if newRecipientCount == 1 then redis.call('EXPIRE', KEYS[3], ARGV[2]) end
if globalExisted == 0 then redis.call('EXPIRE', KEYS[4], ARGV[2]) end
redis.call('SET', KEYS[1], ARGV[1])
redis.call('RPUSH', KEYS[2], KEYS[1])
return {1, 0}
"#;
const POP_QUEUE_HEAD_SCRIPT: &str = r#"
if redis.call('LINDEX', KEYS[1], 0) == ARGV[1] then
  redis.call('LPOP', KEYS[1])
  return 1
end
return 0
"#;
const RELEASE_LOCK_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) end
return 0
"#;
const RENEW_LOCK_SCRIPT: &str = r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('PEXPIRE', KEYS[1], ARGV[2]) end
return 0
"#;

#[derive(Clone)]
pub struct RedisStore {
    connection: ConnectionManager,
}

pub struct Reservation<'a> {
    pub record_key: &'a str,
    pub queue_key: &'a str,
    pub recipient_rate_key: &'a str,
    pub global_budget_key: &'a str,
    pub record: &'a FundingRecord,
    pub amount: &'a str,
    pub global_budget: &'a str,
    pub recipient_limit: u64,
    pub window_seconds: u64,
}

#[async_trait]
pub trait FundingStore: Clone + Send + Sync + 'static {
    async fn reserve(
        &self,
        reservation: Reservation<'_>,
    ) -> Result<ReservationResult, ServiceError>;
    async fn get_record(&self, key: &str) -> Result<Option<FundingRecord>, ServiceError>;
    async fn set_record(&self, key: &str, record: &FundingRecord) -> Result<(), ServiceError>;
    async fn retry_reverted(
        &self,
        key: &str,
        queue: &str,
        failed: &FundingRecord,
    ) -> Result<(), ServiceError>;
    async fn queue_head(&self, key: &str) -> Result<Option<String>, ServiceError>;
    async fn pop_queue_head(&self, key: &str, expected: &str) -> Result<(), ServiceError>;
    async fn acquire_lock(&self, key: &str, token: &str, ttl_ms: u64)
        -> Result<bool, ServiceError>;
    async fn release_lock(&self, key: &str, token: &str) -> Result<(), ServiceError>;
    async fn renew_lock(&self, key: &str, token: &str, ttl_ms: u64) -> Result<bool, ServiceError>;
    async fn ping(&self) -> Result<(), ServiceError>;
    async fn verify_durability(&self) -> Result<(), ServiceError>;
}

impl RedisStore {
    pub async fn connect(url: &str) -> Result<Self, ServiceError> {
        let client = redis::Client::open(url).map_err(store_error)?;
        let manager_config = ConnectionManagerConfig::new()
            .set_number_of_retries(1)
            .set_response_timeout(REDIS_RESPONSE_TIMEOUT)
            .set_connection_timeout(REDIS_CONNECTION_TIMEOUT);
        let connection = ConnectionManager::new_with_config(client, manager_config)
            .await
            .map_err(store_error)?;
        Ok(Self { connection })
    }
}

#[async_trait]
impl FundingStore for RedisStore {
    async fn reserve(
        &self,
        reservation: Reservation<'_>,
    ) -> Result<ReservationResult, ServiceError> {
        let record = serde_json::to_string(reservation.record).map_err(store_error)?;
        let result: (i64, i64) = Script::new(RESERVE_AND_ENQUEUE_SCRIPT)
            .key(reservation.record_key)
            .key(reservation.queue_key)
            .key(reservation.recipient_rate_key)
            .key(reservation.global_budget_key)
            .arg(record)
            .arg(reservation.window_seconds)
            .arg(reservation.recipient_limit)
            .arg(reservation.amount)
            .arg(reservation.global_budget)
            .invoke_async(&mut self.connection.clone())
            .await
            .map_err(store_error)?;
        let retry = result.1.max(1) as u64;
        match result.0 {
            1 => Ok(ReservationResult::Reserved),
            0 => Ok(ReservationResult::Existing),
            -1 => Ok(ReservationResult::RecipientRateLimited(retry)),
            -2 => Ok(ReservationResult::GlobalBudgetExceeded(retry)),
            _ => Err(store_error("unexpected reservation result")),
        }
    }

    async fn get_record(&self, key: &str) -> Result<Option<FundingRecord>, ServiceError> {
        let value: Option<String> = self
            .connection
            .clone()
            .get(key)
            .await
            .map_err(store_error)?;
        value
            .map(|raw| serde_json::from_str(&raw).map_err(store_error))
            .transpose()
    }

    async fn set_record(&self, key: &str, record: &FundingRecord) -> Result<(), ServiceError> {
        let value = serde_json::to_string(record).map_err(store_error)?;
        self.connection
            .clone()
            .set::<_, _, ()>(key, value)
            .await
            .map_err(store_error)
    }

    async fn retry_reverted(
        &self,
        key: &str,
        queue: &str,
        failed: &FundingRecord,
    ) -> Result<(), ServiceError> {
        let FundingRecord::Failed {
            fingerprint,
            input,
            transaction,
            code,
            ..
        } = failed
        else {
            return Err(ServiceError::internal());
        };
        if code != "transaction_reverted" {
            return Err(ServiceError::internal());
        }
        let queued = FundingRecord::Queued {
            fingerprint: fingerprint.clone(),
            input: input.clone(),
        };
        let changed: bool = Script::new(RETRY_REVERTED_SCRIPT)
            .key(key)
            .key(queue)
            .key(format!("{key}:reverted:{}", transaction.hash))
            .arg(serde_json::to_string(failed).map_err(store_error)?)
            .arg(serde_json::to_string(&queued).map_err(store_error)?)
            .invoke_async(&mut self.connection.clone())
            .await
            .map_err(store_error)?;
        if !changed {
            return Err(ServiceError::new(
                409,
                "retry_state_changed",
                "Funding state changed before retry",
            )
            .retry(1));
        }
        Ok(())
    }

    async fn queue_head(&self, key: &str) -> Result<Option<String>, ServiceError> {
        self.connection
            .clone()
            .lindex(key, 0)
            .await
            .map_err(store_error)
    }

    async fn pop_queue_head(&self, key: &str, expected: &str) -> Result<(), ServiceError> {
        Script::new(POP_QUEUE_HEAD_SCRIPT)
            .key(key)
            .arg(expected)
            .invoke_async::<i64>(&mut self.connection.clone())
            .await
            .map(|_| ())
            .map_err(store_error)
    }

    async fn acquire_lock(
        &self,
        key: &str,
        token: &str,
        ttl_ms: u64,
    ) -> Result<bool, ServiceError> {
        let result: Option<String> = redis::cmd("SET")
            .arg(key)
            .arg(token)
            .arg("PX")
            .arg(ttl_ms)
            .arg("NX")
            .query_async(&mut self.connection.clone())
            .await
            .map_err(store_error)?;
        Ok(result.as_deref() == Some("OK"))
    }

    async fn release_lock(&self, key: &str, token: &str) -> Result<(), ServiceError> {
        Script::new(RELEASE_LOCK_SCRIPT)
            .key(key)
            .arg(token)
            .invoke_async::<i64>(&mut self.connection.clone())
            .await
            .map(|_| ())
            .map_err(store_error)
    }

    async fn renew_lock(&self, key: &str, token: &str, ttl_ms: u64) -> Result<bool, ServiceError> {
        Script::new(RENEW_LOCK_SCRIPT)
            .key(key)
            .arg(token)
            .arg(ttl_ms)
            .invoke_async::<i64>(&mut self.connection.clone())
            .await
            .map(|renewed| renewed == 1)
            .map_err(store_error)
    }

    async fn ping(&self) -> Result<(), ServiceError> {
        redis::cmd("PING")
            .query_async::<String>(&mut self.connection.clone())
            .await
            .map(|_| ())
            .map_err(store_error)
    }

    async fn verify_durability(&self) -> Result<(), ServiceError> {
        let append_only = redis_config_value(&self.connection, "appendonly").await?;
        let append_fsync = redis_config_value(&self.connection, "appendfsync").await?;
        if append_only != "yes" || append_fsync != "always" {
            tracing::error!(%append_only, %append_fsync, "Redis durability policy is unsafe for machine funding");
            return Err(ServiceError::new(
                503,
                "unsafe_state_configuration",
                "Machine funding requires Redis appendonly=yes and appendfsync=always",
            ));
        }
        Ok(())
    }
}

async fn redis_config_value(
    connection: &ConnectionManager,
    name: &str,
) -> Result<String, ServiceError> {
    let values: Vec<String> = redis::cmd("CONFIG")
        .arg("GET")
        .arg(name)
        .query_async(&mut connection.clone())
        .await
        .map_err(store_error)?;
    values
        .get(1)
        .cloned()
        .ok_or_else(store_error_missing_config)
}

fn store_error_missing_config() -> ServiceError {
    store_error("Redis CONFIG GET returned no value")
}

fn store_error(error: impl std::fmt::Display) -> ServiceError {
    tracing::error!(%error, "Redis funding state operation failed");
    ServiceError::new(
        503,
        "state_unavailable",
        "Machine funding state is unavailable",
    )
    .retry(1)
}
