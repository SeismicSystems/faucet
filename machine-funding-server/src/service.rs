use crate::{
    chain::ChainDriver,
    config::Config,
    model::{
        ChainResult, FundingAsset, FundingInput, FundingRecord, FundingResponse, PersistedInput,
        ReservationResult, ServiceError,
    },
    store::{FundingStore, Reservation},
};
use alloy_primitives::U256;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::{
    sync::oneshot,
    time::{sleep, timeout, Instant},
};
use uuid::Uuid;

const LOCK_TTL_BUFFER: Duration = Duration::from_secs(60);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone)]
pub struct FundingService<S, D> {
    store: S,
    driver: D,
    config: Config,
}

struct OperatorLockGuard<S: FundingStore> {
    store: S,
    key: String,
    token: String,
    armed: bool,
}

impl<S: FundingStore> OperatorLockGuard<S> {
    fn new(store: S, key: String, token: String) -> Self {
        Self {
            store,
            key,
            token,
            armed: true,
        }
    }

    async fn release(mut self) {
        self.armed = false;
        if let Err(error) = self.store.release_lock(&self.key, &self.token).await {
            tracing::error!(%error, "failed to release machine funding operator lock");
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }

    fn key(&self) -> &str {
        &self.key
    }

    fn token(&self) -> &str {
        &self.token
    }
}

impl<S: FundingStore> Drop for OperatorLockGuard<S> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let store = self.store.clone();
        let key = self.key.clone();
        let token = self.token.clone();
        tokio::spawn(async move {
            if let Err(error) = store.release_lock(&key, &token).await {
                tracing::error!(%error, "failed to release cancelled machine funding operator lock");
            }
        });
    }
}

impl<S, D> FundingService<S, D>
where
    S: FundingStore,
    D: ChainDriver,
{
    pub fn new(store: S, driver: D, config: Config) -> Self {
        Self {
            store,
            driver,
            config,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn health(&self) -> Result<(), ServiceError> {
        tokio::try_join!(self.store.ping(), self.driver.health()).map(|_| ())
    }

    pub async fn execute(&self, input: FundingInput) -> Result<FundingResponse, ServiceError> {
        match timeout(self.config.request_timeout, self.execute_inner(input)).await {
            Ok(result) => result,
            Err(_) => Err(ServiceError::new(
                504,
                "request_timeout",
                "Machine funding exceeded its request deadline and can be retried unchanged",
            )
            .retry(1)),
        }
    }

    async fn execute_inner(&self, input: FundingInput) -> Result<FundingResponse, ServiceError> {
        let record_key = record_key(&input.idempotency_key);
        let fingerprint = fingerprint(&input);
        let queue_key = queue_key(self.driver.operator_key());
        let queued = FundingRecord::Queued {
            fingerprint: fingerprint.clone(),
            input: PersistedInput::from(&input),
        };
        let amount = input.amount.to_string();
        let budget = self.global_budget(input.asset).to_string();
        let recipient_rate_key = recipient_rate_key(&input);
        let global_budget_key = global_budget_key(input.asset);
        let reservation = self
            .store
            .reserve(Reservation {
                record_key: &record_key,
                queue_key: &queue_key,
                recipient_rate_key: &recipient_rate_key,
                global_budget_key: &global_budget_key,
                record: &queued,
                amount: &amount,
                global_budget: &budget,
                recipient_limit: self.config.rate_limit,
                window_seconds: self.config.rate_window.as_secs(),
            })
            .await?;
        match reservation {
            ReservationResult::RecipientRateLimited(seconds) => {
                return Err(ServiceError::new(
                    429,
                    "rate_limited",
                    "Recipient funding rate limit exceeded",
                )
                .retry(seconds));
            }
            ReservationResult::GlobalBudgetExceeded(seconds) => {
                return Err(ServiceError::new(
                    429,
                    "global_budget_exceeded",
                    "Global funding budget exceeded",
                )
                .retry(seconds));
            }
            ReservationResult::Reserved | ReservationResult::Existing => {}
        }

        let existing = self
            .store
            .get_record(&record_key)
            .await?
            .ok_or_else(ServiceError::internal)?;
        if existing.fingerprint() != fingerprint {
            return Err(ServiceError::new(
                409,
                "idempotency_conflict",
                "idempotency_key was already used for a different request",
            ));
        }
        if let Some(response) = terminal_result(&existing, true)? {
            return Ok(response);
        }

        let lock_guard = self
            .acquire_lock(lock_key(self.driver.operator_key()))
            .await?;
        let result = self
            .execute_with_lease(
                lock_guard.key(),
                lock_guard.token(),
                &record_key,
                &queue_key,
                &fingerprint,
            )
            .await;
        lock_guard.release().await;
        result
    }

    async fn execute_with_lease(
        &self,
        lock_key: &str,
        lock_token: &str,
        record_key: &str,
        queue_key: &str,
        fingerprint: &str,
    ) -> Result<FundingResponse, ServiceError> {
        let ttl = self.config.receipt_timeout + LOCK_TTL_BUFFER;
        let renewal_interval = ttl / 3;
        let store = self.store.clone();
        let lease_key = lock_key.to_owned();
        let lease_token = lock_token.to_owned();
        let (stop_sender, mut stop_receiver) = oneshot::channel();
        let (failure_sender, mut failure_receiver) = oneshot::channel();
        let lease = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sleep(renewal_interval) => {
                        match store.renew_lock(&lease_key, &lease_token, ttl.as_millis() as u64).await {
                            Ok(true) => {}
                            Ok(false) => {
                                let _ = failure_sender.send(ServiceError::new(
                                    503,
                                    "operator_lease_lost",
                                    "Machine funding operator lease was lost",
                                ).retry(1));
                                return;
                            }
                            Err(error) => {
                                let _ = failure_sender.send(error);
                                return;
                            }
                        }
                    }
                    _ = &mut stop_receiver => return,
                }
            }
        });
        let operation = self.execute_locked(record_key, queue_key, fingerprint);
        tokio::pin!(operation);
        let result = tokio::select! {
            result = &mut operation => result,
            lease_failure = &mut failure_receiver => {
                Err(lease_failure.unwrap_or_else(|_| ServiceError::new(
                    503,
                    "operator_lease_lost",
                    "Machine funding operator lease was lost",
                ).retry(1)))
            }
        };
        let _ = stop_sender.send(());
        let _ = lease.await;
        result
    }

    async fn acquire_lock(&self, key: String) -> Result<OperatorLockGuard<S>, ServiceError> {
        let deadline = Instant::now() + self.config.lock_wait;
        let ttl = self.config.receipt_timeout + LOCK_TTL_BUFFER;
        loop {
            let guard =
                OperatorLockGuard::new(self.store.clone(), key.clone(), Uuid::new_v4().to_string());
            if self
                .store
                .acquire_lock(guard.key(), guard.token(), ttl.as_millis() as u64)
                .await?
            {
                return Ok(guard);
            }
            guard.disarm();
            if Instant::now() >= deadline {
                return Err(ServiceError::new(
                    409,
                    "operator_busy",
                    "The machine funding operator is processing another transaction",
                )
                .retry(1));
            }
            sleep(LOCK_RETRY_INTERVAL).await;
        }
    }

    async fn execute_locked(
        &self,
        request_record_key: &str,
        queue: &str,
        fingerprint: &str,
    ) -> Result<FundingResponse, ServiceError> {
        let current = self
            .store
            .get_record(request_record_key)
            .await?
            .ok_or_else(ServiceError::internal)?;
        if current.fingerprint() != fingerprint {
            return Err(ServiceError::new(
                409,
                "idempotency_conflict",
                "idempotency_key was already used for a different request",
            ));
        }
        if let Some(response) = terminal_result(&current, true)? {
            return Ok(response);
        }

        let head = self
            .store
            .queue_head(queue)
            .await?
            .ok_or_else(ServiceError::internal)?;
        let processed = self.process_head(queue, &head).await?;
        if head != request_record_key {
            return Err(ServiceError::new(
                409,
                "request_queued",
                "The request is queued behind another machine funding transaction",
            )
            .retry(1));
        }
        terminal_result(&processed, false)?.ok_or_else(ServiceError::internal)
    }

    async fn process_head(
        &self,
        queue: &str,
        record_key: &str,
    ) -> Result<FundingRecord, ServiceError> {
        let mut record = match self.store.get_record(record_key).await? {
            Some(record) => record,
            None => {
                self.store.pop_queue_head(queue, record_key).await?;
                return Err(ServiceError::internal());
            }
        };
        if matches!(
            record,
            FundingRecord::Completed { .. } | FundingRecord::Failed { .. }
        ) {
            self.store.pop_queue_head(queue, record_key).await?;
            return Ok(record);
        }

        if let FundingRecord::Queued { fingerprint, input } = record {
            let nonce = self.driver.pending_nonce().await?;
            let transaction = self
                .driver
                .prepare(&FundingInput::try_from(&input)?, nonce)
                .await?;
            record = FundingRecord::Prepared {
                fingerprint,
                input,
                transaction,
            };
            self.store.set_record(record_key, &record).await?;
        }

        let (fingerprint, input, transaction) = match &record {
            FundingRecord::Prepared {
                fingerprint,
                input,
                transaction,
            } => (fingerprint.clone(), input.clone(), transaction.clone()),
            _ => return Err(ServiceError::internal()),
        };
        let result = self.driver.broadcast_and_confirm(&transaction).await?;
        let terminal = match result {
            ChainResult::Pending => {
                return Err(ServiceError::new(
                    504,
                    "transaction_pending",
                    "The signed transaction is pending confirmation and will be retried unchanged",
                )
                .retry(1)
                .transaction(transaction.hash));
            }
            ChainResult::Reverted => FundingRecord::Failed {
                fingerprint,
                input,
                transaction: transaction.clone(),
                code: "transaction_reverted".into(),
                message: "The funding transaction reverted".into(),
                transaction_hash: transaction.hash,
            },
            ChainResult::Rejected(message) => FundingRecord::Failed {
                fingerprint,
                input,
                transaction: transaction.clone(),
                code: "transaction_rejected".into(),
                message,
                transaction_hash: transaction.hash,
            },
            ChainResult::Success => {
                let response = FundingResponse {
                    idempotency_key: input.idempotency_key.clone(),
                    recipient: input.recipient.clone(),
                    amount: input.amount.clone(),
                    transaction_hash: transaction.hash.clone(),
                    replayed: false,
                };
                FundingRecord::Completed {
                    fingerprint,
                    input,
                    transaction,
                    response,
                }
            }
        };
        self.store.set_record(record_key, &terminal).await?;
        self.store.pop_queue_head(queue, record_key).await?;
        Ok(terminal)
    }

    fn global_budget(&self, asset: FundingAsset) -> U256 {
        match asset {
            FundingAsset::Susdc => self.config.global_susdc_budget,
            FundingAsset::Native => self.config.global_gas_budget_wei,
        }
    }
}

fn terminal_result(
    record: &FundingRecord,
    replayed: bool,
) -> Result<Option<FundingResponse>, ServiceError> {
    match record {
        FundingRecord::Completed { response, .. } => {
            let mut response = response.clone();
            response.replayed = replayed;
            Ok(Some(response))
        }
        FundingRecord::Failed {
            code,
            message,
            transaction_hash,
            ..
        } => Err(ServiceError::new(422, code, message).transaction(transaction_hash)),
        FundingRecord::Queued { .. } | FundingRecord::Prepared { .. } => Ok(None),
    }
}

fn record_key(idempotency_key: &str) -> String {
    format!("machine-funding:idempotency:{idempotency_key}")
}

fn queue_key(operator_key: &str) -> String {
    format!("machine-funding:queue:{operator_key}")
}

fn lock_key(operator_key: &str) -> String {
    format!("machine-funding:lock:{operator_key}")
}

fn recipient_rate_key(input: &FundingInput) -> String {
    format!(
        "machine-funding:rate:{}:{}",
        input.asset.key(),
        input.recipient_text.to_ascii_lowercase()
    )
}

fn global_budget_key(asset: FundingAsset) -> String {
    format!("machine-funding:budget:{}", asset.key())
}

fn fingerprint(input: &FundingInput) -> String {
    let value = format!(
        "{}\n{}\n{}\n{}",
        input.asset.key(),
        input.recipient_text.to_ascii_lowercase(),
        input.amount,
        input.reason
    );
    hex::encode(Sha256::digest(value.as_bytes()))
}
