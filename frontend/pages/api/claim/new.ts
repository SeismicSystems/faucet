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

const AMEYA_TWITTER_ID = "1311531128201916417";
const AMEYA_GITHUB_ID = "74180822";
const CHRISTIAN_GITHUB_ID = "1449882";

const whitelist = [AMEYA_TWITTER_ID, AMEYA_GITHUB_ID, CHRISTIAN_GITHUB_ID];

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

function generateTxData(recipient: string): `0x${string}` {
  return encodeFunctionData({
    abi: seismicFaucetAbi,
    functionName: "drip",
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

  // Update nonce in redis with 5m TTL
  await client.set(`nonce-${chain.name}`, nonce + 1, "EX", 300);

  try {
    await walletClient.sendTransaction({
      to: faucetAddress,
      data,
      gasPrice: gasPrice * BigInt(2),
      gas: BigInt(500_000),
      nonce,
    });
  } catch (e: any) {
    const errorMsg = `Error dripping for ${chain.name}: ${e.message || String(e)}`;
    await postSlackMessage(errorMsg);

    // Attempt self-heal by clearing nonce
    await client.del(`nonce-${chain.name}`);
    await postSlackMessage(`Attempting self heal for ${chain.name}`);

    throw new Error(`Error when processing drip for ${chain.name}`);
  }
}

export default async (req: NextApiRequest, res: NextApiResponse) => {
  const session: any = await getServerSession(req, res, authOptions);
  const { address, others }: { address: string; others: boolean } = req.body;

  if (!session) {
    return res.status(401).send({ error: "Not authenticated." });
  }

  // Validate authentication provider
  const userId =
    session.provider === "twitter"
      ? session.twitter_id
      : session.provider === "github"
        ? session.github_id
        : null;

  if (!userId) {
    return res.status(400).send({ error: "Invalid authentication." });
  }

  // Validate Twitter followers
  if (session.provider === "twitter") {
    const followerCount = session.twitter_num_followers || 0;
    if (followerCount < MIN_TWITTER_FOLLOWERS) {
      return res.status(403).send({
        error: `Minimum ${MIN_TWITTER_FOLLOWERS} Twitter followers required. You have ${followerCount}.`,
      });
    }
  }

  // Validate GitHub followers
  if (session.provider === "github") {
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

  const isWhitelisted = whitelist.includes(userId);

  // Check claim status for non-whitelisted users
  if (!isWhitelisted) {
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

  // Generate transaction data
  const data = generateTxData(address);

  // Process drip on main network
  try {
    await processDrip(account, mainNetwork, data, faucetAddress);
  } catch (e) {
    // Rate limit non-whitelisted users on error
    if (!isWhitelisted) {
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

  // Set 24h cooldown for non-whitelisted users
  if (!isWhitelisted) {
    await client.set(
      `faucet:${mainNetwork.name}:${userId}`,
      "true",
      "EX",
      86400,
    );
  }

  if (isWhitelisted) {
    console.log(`${address} claimed from faucet`);
  }

  return res.status(200).send({ claimed: address });
};
