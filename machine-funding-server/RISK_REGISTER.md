# Machine Funding Risk Register

## Status

This design is accepted for a testnet staging MVP. It is not suitable for assets
with real-world value without revisiting the deferred controls below.

Review this register before:

- funding assets acquire real-world value;
- the service is exposed beyond the staging orchestration machine;
- sandbox access becomes broadly self-service;
- the faucet or orchestration deployment topology changes;
- the operator wallet or contract balance is materially increased.

## Architecture Assumptions

The staging decision relies on all of these remaining true:

- nginx admits `/api/internal/*` only from the orchestration staging machine's
  static `/32` egress address;
- the Rust service binds only to `127.0.0.1`;
- every funding request carries a randomly generated bearer token with at least
  256 bits of entropy;
- the token and funding private key are stored in a mode-`0600` environment file
  owned by the `ubuntu` account that runs the faucet services;
- the machine operator is not a faucet super operator;
- the operator wallet has zero native balance and holds only the sUSDC gas
  reserve needed for staging;
- sUSDC is a test asset with no real-world value;
- Redis durability remains enabled and its data is not reset independently of
  the mock-provider database without an explicit replay assessment.

The source-IP restriction and bearer token are independent checks against an
ordinary internet caller. They are not independent after the orchestration host
is compromised because that host supplies both the allowed source address and
the token.

## Existing Controls

- nginx source-IP allowlist before proxying to the service;
- HTTPS for the public network hop;
- loopback-only Rust listener, rejected at configuration time otherwise;
- bearer-token verification using a constant-time comparison of fixed-length
  hashes;
- minimum bearer-token length validation;
- nonzero EVM recipient validation and bounded request fields;
- per-transfer sUSDC ceiling and fixed sUSDC gas-reserve amount;
- per-recipient request limit;
- separate global sUSDC payout and gas-reserve budgets per configured window;
- atomic Redis reservation, idempotency records, and operator serialization;
- raw signed transaction persistence before broadcast;
- receipt confirmation before returning success;
- dedicated non-super machine operator enforced at startup and on-chain;
- shared loopback-only Redis instance with AOF persistence and `appendfsync
  always`; machine-funding keys use a dedicated logical database.

## Risk Summary

| Risk | Likelihood | Impact | Current containment |
| --- | --- | --- | --- |
| Direct unauthenticated internet call | Low | Medium | Source-IP gate and bearer token |
| Weak or leaked bearer token | Low to medium | Medium | Source-IP gate and rolling global budgets |
| Authorized sandbox farming | Medium to high | Medium | Per-recipient and global budgets only |
| Orchestration host compromise | Low | High | API budgets and limited operator balances |
| Faucet host or operator-key compromise | Low | High | Operator is non-super; per-transfer contract ceiling |
| Queue or Redis exhaustion | Medium after credential compromise or client bug | Medium to high | Global amount budget and serialized processing |
| RPC compromise or malfunction | Low | Medium | Local signing, chain-id check, exact response validation |
| Redis loss or stale restore | Low | Medium | AOF durability; no independent durable idempotency ledger |
| Configuration drift | Medium | Medium to high | Loopback validation and nginx configuration test during deployment |

## Detailed Risks

### Authorized Sandbox Farming

The public sandbox can indirectly cause the legitimate mock-provider machine to
request funding. A user can generate new destination addresses, accounts,
customers, orders, or organizations to evade controls keyed only by recipient.
The global budget bounds aggregate spend but permits one actor to starve other
testers.

Systematic prevention belongs at the authenticated orchestration boundary, not
in an address allowlist. The future control should meter a stable sandbox
principal, preferably the authenticated organization plus a stable user or
verified-domain identity:

- one gas grant per virtual account;
- maximum funded virtual accounts per principal and time window;
- sUSDC amount and order-count budgets per principal;
- idempotent quota reservations keyed to the account or order operation;
- a maximum share of the global pool available to one principal;
- limits on sandbox organizations per verified user or domain.

Per-organization quotas alone do not solve Sybil creation when identities and
organizations are free. Stronger prevention requires a scarce admission factor
such as an invitation, verified work identity, manual approval, or economic
cost. For staging, global containment and fair per-principal quotas are the
proportionate future controls.

### Arbitrary Authenticated Recipients

After passing the network and bearer gates, the caller can name any nonzero EVM
address. This is required for external crypto payouts, so a static recipient
allowlist would break the intended flow. Compromise is instead contained by
principal, purpose, amount, count, and global budgets.

### Token Replay and Rotation

The bearer token has no request signature, expiry, nonce, or audience beyond the
endpoint. A captured token remains replayable until manually rotated. One token
currently authorizes both sUSDC payout and gas-reserve operations.

The token must be generated randomly rather than merely satisfying the minimum
length. It must not appear in shell history, process logs, supervisor config,
repository files, or request logging. Rotation currently requires coordinated
replacement and restart on both machines; there is no two-token overlap window.

### Source-IP Assumptions

The nginx gate authenticates a network origin, not a process. It becomes weaker
if the egress address is shared by other hosts or a large NAT pool. If the
staging address is released and later reassigned while nginx retains it, the new
owner reaches the bearer-token gate.

The current configuration intentionally uses nginx's direct peer address. A
future CDN or load balancer must not replace this with an untrusted forwarded-IP
header. Any deployment topology change requires retesting the allowlist from
both allowed and denied origins.

### Orchestration Pivot and SSRF

A process compromise on orchestration staging inherits the allowed source IP. An
SSRF primitive is useful to an attacker only if it can also attach the bearer
header or retrieve the token, but a general host compromise satisfies both
conditions. No HTTP authentication scheme prevents abuse by the fully
compromised authorized caller.

### Amount-Based Budget Evasion

The global budget meters transferred amount, not request count. A caller can
submit many unique one-base-unit sUSDC requests without quickly consuming the
amount budget. Unique recipients also evade the per-recipient count limit.

This can produce a large queue and persistent idempotency records even when the
economic amount is negligible. A global request-count budget and maximum queue
depth are the primary deferred controls.

### Queue and Lock Amplification

Requests reserve budget and enter the queue before obtaining the operator lock.
Concurrent waiters poll Redis every 25 milliseconds until the lock-wait deadline.
A burst from the trusted client, an application retry bug, or a compromised
caller can amplify one incoming request into many Redis operations.

The caller also retries selected failures up to five times. Stable idempotency
prevents duplicate transfers, but retries still contribute HTTP, Redis, and
queue load.

### Persistent Redis Growth

Idempotency records intentionally have no TTL so an old retry cannot re-fund an
operation. They include the prepared raw transaction and remain in the shared,
always-synced AOF. Long-running or abusive traffic can exhaust disk, increase
restart time, or affect the other faucet services using Redis.

A retention policy must account for the maximum business retry horizon before
records are expired or compacted. A naive TTL can turn a sufficiently late retry
into a duplicate transfer.

### Fixed-Window Bursts

Recipient and global budgets use fixed expiry windows beginning with the first
reservation. A caller can consume a full budget just before expiry and another
full budget immediately after expiry. Operational capacity must tolerate up to
roughly twice a nominal window budget around a boundary.

### Budget Starvation on Failed Transactions

Budget is reserved before signing and broadcast. A rejected or reverted
transaction remains a terminal idempotency record and does not refund its
reservation. This is fail-closed for funds but permits repeated failures to
consume the window budget and deny service to legitimate testers.

### Redis Loss, Rollback, or Split Reset

Redis is the funding idempotency ledger. Losing it, restoring a stale snapshot,
or resetting it without the mock-provider database can allow a previously
completed business operation to be submitted as new and funded again.

The opposite reset order also matters: resetting the mock-provider database but
retaining Redis can cause reused logical identifiers to replay old results or
conflict permanently with different request fingerprints.

### Operator-Key Bypass

The funding private key is available to the network-facing Rust process. Anyone
who obtains it can bypass nginx, bearer authentication, Redis idempotency, and
all API budgets.

The contract restricts the signer to `transferExact`, rejects a super operator,
and caps each transfer, but it does not enforce a cumulative machine-operator
allowance. A stolen key can drain the contract's sUSDC through repeated bounded
transfers and can directly transfer its own sUSDC gas reserve.

For staging, this is accepted because the token is test-only and the operator's
sUSDC reserve must remain small. Before assets have value, add a cumulative on-chain
allowance or a separately funded machine vault and move signing out of the HTTP
process or into managed key custody.

### RPC Manipulation

Transactions are signed locally, so the RPC cannot change the recipient, value,
or calldata. The service checks chain ID, contract code, operator role, returned
transaction hash, and receipts.

The RPC-provided gas price is doubled without a configured ceiling. A malicious
or faulty RPC can propose an excessive gas price and consume the operator's
sUSDC gas reserve. A configured maximum gas price is a deferred control. A
malicious RPC can also delay, omit, or lie about chain state, causing availability
or local-state inconsistencies even though it cannot forge a different signed
transfer.

### Raw Signed Transactions at Rest

Redis stores prepared raw transactions before broadcast to guarantee exact-byte
replay. Anyone who reads Redis can rebroadcast the already-authorized transfer,
but cannot change its recipient or amount. Chain ID and nonce constrain replay;
state exposure still reveals request and transaction metadata.

### Dependency and Process Compromise

Rust reduces memory-safety risk but the service still parses network input and
depends on nginx, Axum, Redis, Alloy, TLS, and operating-system components. An
RCE in the service process exposes both bearer-token material and the funding
private key because signing is in-process.

### Availability and Fairness

The global budget is a loss ceiling, not a fairness mechanism. One authorized
actor can consume it before other testers. Likewise, a poisoned queue can keep
legitimate work behind low-value requests. Per-principal quotas and a bounded or
fair queue are separate future controls.

## Base Sepolia Funding

The Base Sepolia routes (`/api/internal/base/gas`, `/api/internal/base/erc20-usdc/transfers`,
`/api/internal/base/readiness`) reuse every existing control: the same nginx
source-IP gate and bearer token, the same Redis reservation, idempotency, and
operator serialization, and per-asset budgets and rate windows. What differs:

- **The signer is the reserve.** There is no faucet contract on Base, so the
  reserve key holds both the native ETH gas reserve and the ERC20 USDC supply
  and transfers them directly. A stolen Base key drains both balances outright;
  the containment is the small, explicitly funded reserve and the dedicated key.
  `INTERNAL_FUNDING_BASE_PRIVATE_KEY` is refused when it matches a Seismic
  funding key unless `INTERNAL_FUNDING_BASE_ALLOW_SHARED_KEY=true` is set with
  explicit approval.
- **Native gas is real value on other networks.** Base Sepolia ETH has no
  real-world value, but the same key format on Base mainnet would. The chain id
  is verified at connect time and on every readiness probe, and every Base
  ledger key is scoped to `chain id + token + reserve`, so a mispointed RPC
  fails closed rather than replaying a testnet ledger against another network.
- **Reserve depletion is a diagnostic, not a surprise.** Startup preflight and
  `/api/internal/base/readiness` compare both balances to configured floors
  (`INTERNAL_FUNDING_BASE_ETH_RESERVE_FLOOR`,
  `INTERNAL_FUNDING_BASE_ERC20_USDC_RESERVE_FLOOR`); readiness answers
  `503 reserve_low` below either floor so an alert fires before a drip fails
  on-chain with `insufficient funds`, which is otherwise a terminal `422`.
- **EIP-1559 fees are estimated, not capped.** The estimate is doubled for
  headroom without a configured ceiling, the same deferred control as the
  Seismic gas price.

## Deferred Hardening

### Staging Hardening

- Add a global request-count budget independent of transferred amount.
- Add an atomic maximum queue depth.
- Add nginx request-body, request-rate, and connection limits.
- Add a configured maximum RPC gas price.
- Define idempotency retention and AOF compaction procedures.
- Add per-principal wallet-count and payout-amount quotas in orchestration.
- Add alerts for denied authentication, budget exhaustion, queue depth, Redis
  memory/disk use, operator balance, and unusual transfer velocity.
- Document coordinated bearer-token rotation and emergency revocation.

### Required Before Valuable Assets

- Add a cumulative on-chain allowance or separately funded machine vault.
- Separate request handling from signing or use managed key custody.
- Replace the shared bearer-only machine identity with mTLS, a private network,
  or equivalent workload identity if the network boundary expands.
- Add a durable funding audit ledger outside the Redis operational store.
- Add per-principal entitlement and anti-Sybil controls at sandbox admission.
- Add automated circuit breaking and tested key/operator revocation.

## Staging Validation

Before each deployment:

- verify nginx configuration and confirm the allowlist is a single intended
  egress address;
- confirm denied-origin requests never reach the Rust service;
- confirm allowed-origin requests without or with an incorrect token return
  `401`;
- confirm valid retries replay the same transaction and do not double-fund;
- verify transfer, recipient, and global limits with production-like values;
- verify Redis AOF durability and confirm it remains bound to loopback;
- confirm the operator is a machine operator and not a super operator;
- confirm the operator's native balance is zero and minimize its sUSDC gas reserve;
- confirm logs do not include the bearer token, funding private key, or complete
  authorization header;
- exercise emergency token rotation and on-chain operator revocation.

## Incident Response

If abuse or credential exposure is suspected:

1. Disable the machine operator on-chain using a super operator.
2. Remove or block the internal nginx route.
3. Stop the machine-funding service without deleting Redis state.
4. Rotate the bearer token and funding key.
5. Inspect Redis idempotency records, queue state, AOF, service logs, and on-chain
   transfers before restoring service.
6. Re-enable funding with reduced budgets and balances until the cause is known.
