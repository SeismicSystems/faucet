import Redis from "ioredis";
import { WebClient } from "@slack/web-api";
import { getServerSession } from "next-auth/next";
import { authOptions } from "@/pages/api/auth/[...nextauth]";
import { hasClaimed } from "@/pages/api/claim/status";
import type { NextApiRequest, NextApiResponse } from "next";

import { seismicFaucetAbi } from "@/utils/contract";
import {
  Address,
  encodeFunctionData,
  createPublicClient,
  createWalletClient,
  Chain,
  http,
  isAddress,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { mainNetwork } from "@/utils/networks";
import { whitelist, developerList } from "@/utils/whitelist";

const MIN_TWITTER_FOLLOWERS = 50;
const MIN_GITHUB_FOLLOWERS = 10;

// Setup redis and slack clients
const client = new Redis(process.env.REDIS_URL as string);
const slack = new WebClient(process.env.SLACK_ACCESS_TOKEN);
const slackChannel = process.env.SLACK_CHANNEL ?? "";

async function postSlackMessage(message: string): Promise<void> {
  await slack.chat.postMessage({
    channel: slackChannel,
    text: message,
    link_names: true,
  });
}

type UserTier = "whitelist" | "developer" | "regular";

function generateTxData(recipient: string, tier: UserTier): `0x${string}` {
  const functionName = tier === "whitelist" 
    ? "dripWhitelist" 
    : tier === "developer" 
      ? "dripDeveloper" 
      : "drip";
  
  return encodeFunctionData({
    abi: seismicFaucetAbi,
    functionName,
    args: [recipient as Address],
  });
}

async function getNonceForChain(
  chain: Chain,
  operatorAddress: Address,
): Promise<number> {
  const cacheKey = `nonce-${chain.name}`;
  const cachedNonce = await client.get(cacheKey);

  if (cachedNonce !== null) {
    return Number(cachedNonce);
  }

  const publicClient = createPublicClient({
    chain,
    transport: http(),
  });

  return await publicClient.getTransactionCount({ address: operatorAddress });
}

async function processDrip(
  account: ReturnType<typeof privateKeyToAccount>,
  chain: Chain,
  data: `0x${string}`,
  faucetAddress: Address,
): Promise<void> {
  console.log(`[processDrip] Starting drip on chain: ${chain.name}`);
  console.log(`[processDrip] Faucet address: ${faucetAddress}`);
  console.log(`[processDrip] TX data: ${data}`);

  const publicClient = createPublicClient({
    chain,
    transport: http(),
  });

  const walletClient = createWalletClient({
    account,
    chain,
    transport: http(),
  });

  const nonce = await getNonceForChain(chain, account.address);
  const gasPrice = await publicClient.getGasPrice();

  console.log(`[processDrip] Nonce: ${nonce}, Gas price: ${gasPrice}`);

  // Update nonce in redis with 5m TTL
  await client.set(`nonce-${chain.name}`, nonce + 1, "EX", 300);

  try {
    console.log(`[processDrip] Sending transaction...`);
    const txHash = await walletClient.sendTransaction({
      to: faucetAddress,
      data,
      gasPrice: gasPrice * BigInt(2),
      gas: BigInt(500_000),
      nonce,
    });
    console.log(`[processDrip] Transaction sent! Hash: ${txHash}`);
  } catch (e: any) {
    console.error(`[processDrip] ERROR: ${e.message || String(e)}`);
    const errorMsg = `Error dripping for ${chain.name}: ${e.message || String(e)}`;
    await postSlackMessage(errorMsg);

    // Attempt self-heal by clearing nonce
    await client.del(`nonce-${chain.name}`);
    await postSlackMessage(`Attempting self heal for ${chain.name}`);

    throw new Error(`Error when processing drip for ${chain.name}`);
  }
}

export default async (req: NextApiRequest, res: NextApiResponse) => {
  console.log(`[claim/new] === New claim request ===`);
  const session: any = await getServerSession(req, res, authOptions);
  const { address, others }: { address: string; others: boolean } = req.body;

  console.log(`[claim/new] Address: ${address}`);
  console.log(`[claim/new] Session provider: ${session?.provider}`);

  if (!session) {
    console.log(`[claim/new] No session - returning 401`);
    return res.status(401).send({ error: "Not authenticated." });
  }

  // Validate authentication provider
  const userId =
    session.provider === "twitter"
      ? session.twitter_id
      : session.provider === "github"
        ? session.github_id
        : null;

  console.log(`[claim/new] User ID: ${userId}`);

  if (!userId) {
    console.log(`[claim/new] No userId - returning 400`);
    return res.status(400).send({ error: "Invalid authentication." });
  }

  // Determine user tier
  const isWhitelisted = whitelist.includes(userId);
  const isDeveloper = developerList.includes(userId);
  const tier: UserTier = isWhitelisted ? "whitelist" : isDeveloper ? "developer" : "regular";
  console.log(`[claim/new] User tier: ${tier}`);

  // Validate Twitter followers (skip for whitelisted and developer users)
  if (tier === "regular" && session.provider === "twitter") {
    const followerCount = session.twitter_num_followers || 0;
    if (followerCount < MIN_TWITTER_FOLLOWERS) {
      return res.status(403).send({
        error: `Minimum ${MIN_TWITTER_FOLLOWERS} Twitter followers required. You have ${followerCount}.`,
      });
    }
  }

  // Validate GitHub followers (skip for whitelisted and developer users)
  if (tier === "regular" && session.provider === "github") {
    const followerCount = session.github_followers || 0;
    if (followerCount < MIN_GITHUB_FOLLOWERS) {
      return res.status(403).send({
        error: `Minimum ${MIN_GITHUB_FOLLOWERS} GitHub followers required. You have ${followerCount}.`,
      });
    }
  }

  // Validate address
  if (!address || !isAddress(address)) {
    return res.status(400).send({ error: "Invalid address." });
  }

  // Check claim status for non-whitelisted users (developers have 24h cooldown too)
  if (tier !== "whitelist") {
    const claimed = await hasClaimed(userId, mainNetwork.name);
    if (claimed) {
      return res.status(400).send({
        error: "Already claimed in 24h window",
      });
    }
  }

  // Create account from private key
  const account = privateKeyToAccount(
    process.env.FAUCET_PRIVATE_KEY as `0x${string}`,
  );
  const faucetAddress = process.env.FAUCET_ADDRESS as Address;

  console.log(`[claim/new] Operator address: ${account.address}`);
  console.log(`[claim/new] Faucet contract: ${faucetAddress}`);

  // Generate transaction data based on tier
  const functionName = tier === "whitelist" ? "dripWhitelist" : tier === "developer" ? "dripDeveloper" : "drip";
  console.log(`[claim/new] Using function: ${functionName}`);
  const data = generateTxData(address, tier);

  // Process drip on main network
  console.log(`[claim/new] Processing drip on ${mainNetwork.name}...`);
  try {
    await processDrip(account, mainNetwork, data, faucetAddress);
    console.log(`[claim/new] Drip successful!`);
  } catch (e: any) {
    console.error(`[claim/new] Drip failed: ${e.message || String(e)}`);
    // Rate limit non-whitelisted users on error (includes developers)
    if (tier !== "whitelist") {
      await client.set(
        `faucet:${mainNetwork.name}:${userId}`,
        "true",
        "EX",
        900,
      ); // 15 min cooldown
    }

    return res.status(500).send({
      error: "Error claiming, try again in 15 minutes.",
    });
  }

  // Set 24h cooldown for non-whitelisted users (includes developers)
  if (tier !== "whitelist") {
    await client.set(
      `faucet:${mainNetwork.name}:${userId}`,
      "true",
      "EX",
      86400,
    );
  }

  console.log(`[claim/new] ${address} claimed from faucet (tier: ${tier})`);

  return res.status(200).send({ claimed: address, tier });
};
