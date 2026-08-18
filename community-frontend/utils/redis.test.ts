import { test, expect } from "bun:test";

process.env.REDIS_URL = "redis://127.0.0.1:1";

// Black-holed connection: accepts TCP, never replies. This is what a dead
// network looks like to ioredis — connect succeeds at the TCP layer (or
// hangs), commands never get a response. With default ioredis options a
// command issued here hangs forever, pinning its API request in memory.
test(
  "redis commands reject quickly against a black-holed connection",
  async () => {
    const blackhole = Bun.listen({
      hostname: "127.0.0.1",
      port: 0,
      socket: { data() {}, open() {} },
    });

    const { createRedisClient } = await import("./redis");
    const client = createRedisClient(`redis://127.0.0.1:${blackhole.port}`);

    const started = Date.now();
    expect(client.get("any-key")).rejects.toThrow();
    await client.get("any-key").catch(() => {});
    expect(Date.now() - started).toBeLessThan(8_000);

    client.disconnect();
    blackhole.stop(true);
  },
  { timeout: 15_000 },
);
