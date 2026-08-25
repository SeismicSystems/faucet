import { seismicFaucetAbi as communityAbi } from "../community-frontend/utils/contract";
import { seismicFaucetAbi as frontendAbi } from "../frontend/utils/contract";

const artifactPath = new URL(
  "../contracts/out/SeismicFaucet.sol/SeismicFaucet.json",
  import.meta.url,
);
const artifactDisplayPath =
  "contracts/out/SeismicFaucet.sol/SeismicFaucet.json";

type AbiItem = Record<string, unknown> & { type?: string };

function canonicalize(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, child]) => [key, canonicalize(child)]),
    );
  }
  return value;
}

function normalizedAbi(abi: readonly AbiItem[]): string[] {
  return abi
    .filter((item) => item.type !== "constructor")
    .map((item) => JSON.stringify(canonicalize(item)))
    .sort();
}

function assertCurrentAbi(
  name: string,
  actual: readonly AbiItem[],
  expected: readonly AbiItem[],
): void {
  const normalizedActual = normalizedAbi(actual);
  const normalizedExpected = normalizedAbi(expected);
  if (JSON.stringify(normalizedActual) === JSON.stringify(normalizedExpected)) {
    return;
  }

  const actualItems = new Set(normalizedActual);
  const expectedItems = new Set(normalizedExpected);
  const missing = normalizedExpected.filter((item) => !actualItems.has(item));
  const stale = normalizedActual.filter((item) => !expectedItems.has(item));
  console.error(`${name} ABI does not match ${artifactDisplayPath}`);
  if (missing.length > 0) {
    console.error("Missing ABI entries:", missing.join("\n"));
  }
  if (stale.length > 0) {
    console.error("Stale ABI entries:", stale.join("\n"));
  }
  process.exitCode = 1;
}

const artifactFile = Bun.file(artifactPath);
if (!(await artifactFile.exists())) {
  throw new Error(
    `Missing contract artifact ${artifactDisplayPath}; run sforge build first`,
  );
}
const artifact = await artifactFile.json();
if (!artifact || !Array.isArray(artifact.abi)) {
  throw new Error(
    `Missing contract ABI in ${artifactDisplayPath}; run sforge build first`,
  );
}

assertCurrentAbi("frontend", frontendAbi, artifact.abi);
assertCurrentAbi("community-frontend", communityAbi, artifact.abi);
