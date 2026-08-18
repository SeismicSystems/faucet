import Redis, { RedisOptions } from "ioredis";

// Fail fast when redis is unreachable. ioredis defaults have no command
// timeout and an unbounded offline queue, so a black-holed connection
// (packets dropped, not refused) holds every in-flight API request in
// memory indefinitely. Bounded timeouts turn a redis outage into fast
// 500s instead of unbounded memory growth.
export const redisOptions: RedisOptions = {
  connectTimeout: 3_000,
  commandTimeout: 3_000,
  maxRetriesPerRequest: 2,
  enableOfflineQueue: false,
};

export function createRedisClient(url: string): Redis {
  return new Redis(url, redisOptions);
}

export const redis = createRedisClient(process.env.REDIS_URL as string);
