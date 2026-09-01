import { seismicFaucetAbi as communityAbi } from "../community-frontend/utils/contract";
import { seismicFaucetAbi as frontendAbi } from "../frontend/utils/contract";

const artifactPath = new URL(
  "../contracts/out/SeismicFaucet.sol/SeismicFaucet.json",
  import.meta.url,
);
const artifactDisplayPath =
  "contracts/out/SeismicFaucet.sol/SeismicFaucet.json";

type AbiItem = Record<string, unknown> & { type?: string };

type AbiFunction = AbiItem & {
  type: "function";
  name: string;
  inputs?: Array<{ type: string }>;
};

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

async function artifactAbi(relativePath: string): Promise<AbiItem[]> {
  const path = new URL(`../${relativePath}`, import.meta.url);
  const file = Bun.file(path);
  if (!(await file.exists())) {
    throw new Error(`Missing contract artifact ${relativePath}; run sforge build first`);
  }
  const value = await file.json();
  if (!value || !Array.isArray(value.abi)) {
    throw new Error(`Missing contract ABI in ${relativePath}; run sforge build first`);
  }
  return value.abi;
}

function functionSignatures(abi: readonly AbiItem[]): Set<string> {
  return new Set(
    abi
      .filter((item): item is AbiFunction => item.type === "function")
      .map(
        (item) =>
          `${item.name}(${(item.inputs ?? []).map((input) => input.type).join(",")})`,
      ),
  );
}

function assertFunctions(
  name: string,
  abi: readonly AbiItem[],
  required: readonly string[],
): void {
  const signatures = functionSignatures(abi);
  const missing = required.filter((signature) => !signatures.has(signature));
  if (missing.length === 0) {
    return;
  }
  console.error(`${name} ABI is missing required functions:`, missing.join(", "));
  process.exitCode = 1;
}

const erc20UsdcAbi = await artifactAbi(
  "contracts/out/TestnetUSDC.sol/TestnetUSDC.json",
);
assertFunctions("TestnetUSDC", erc20UsdcAbi, [
  "allowance(address,address)",
  "approve(address,uint256)",
  "balanceOf(address)",
  "decimals()",
  "mint(address,uint256)",
  "owner()",
  "transfer(address,uint256)",
  "transferFrom(address,address,uint256)",
]);

const erc20FaucetAbi = await artifactAbi(
  "contracts/out/ERC20USDCFaucet.sol/ERC20USDCFaucet.json",
);
assertFunctions("ERC20USDCFaucet", erc20FaucetAbi, [
  "MAX_EXACT_TRANSFER_AMOUNT()",
  "machineOperators(address)",
  "superOperators(address)",
  "transferExact(address,uint256)",
  "updateSuperOperator(address,bool)",
  "usdc()",
]);
