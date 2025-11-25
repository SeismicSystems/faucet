import Redis from "ioredis";
import { WebClient } from "@slack/web-api";
import { 
  seismicDevnet1, 
  seismicDevnet2, 
  sanvil, 
  seismicTestnet 
} from "seismic-viem";
import { getSession } from "next-auth/client";
import { hasClaimed } from "pages/api/claim/status";
import type { NextApiRequest, NextApiResponse } from "next";

import { seismicFaucetAbi } from "utils/contract";
import {
  Address,
  encodeFunctionData,
  PublicClient,
  WalletClient,
  createPublicClient,
  createWalletClient,
  Chain,
  http,
  isAddress,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { mainnet } from "viem/chains";

const AMEYA_TWITTER_ID = "1311531128201916417";
const AMEYA_GITHUB_ID = "74180822";
const CHRISTIAN_GITHUB_ID = "1449882";

const isDevelopment = process.env.NODE_ENV === "development";

// Setup whitelist
const twitterWhitelist: string[] = [AMEYA_TWITTER_ID];
const githubWhitelist: string[] = [AMEYA_GITHUB_ID, CHRISTIAN_GITHUB_ID];
const whitelist: string[] = [...twitterWhitelist, ...githubWhitelist];

// Setup redis client
const client = new Redis(process.env.REDIS_URL);

// Setup slack client
const slack = new WebClient(process.env.SLACK_ACCESS_TOKEN);
const slackChannel: string = process.env.SLACK_CHANNEL ?? "";

/**
 * Post message to slack channel
 */
async function postSlackMessage(message: string): Promise<void> {
  await slack.chat.postMessage({
    channel: slackChannel,
    text: message,
    link_names: true,
  });
}

// Network configuration
interface NetworkConfig {
  chain: Chain;
}

const mainNetworkConfigs: Record<number, NetworkConfig> = isDevelopment
  ? {
      [sanvil.id]: { chain: sanvil },
    }
  : {
      [seismicTestnet.id]: { chain: seismicTestnet },
    };

const secondaryNetworkConfigs: Record<number, NetworkConfig> = isDevelopment
  ? {}
  : {
      [seismicDevnet1.id]: { chain: seismicDevnet1 },
      [seismicDevnet2.id]: { chain: seismicDevnet2 },
    };

/**
 * Generate encoded transaction data for drip
 */
function generateTxData(recipient: string): `0x${string}` {
  return encodeFunctionData({
    abi: seismicFaucetAbi,
    functionName: "drip",
    args: [recipient as Address],
  });
}

/**
 * Get public client by chain ID
 */
function getPublicClientByChainId(chainId: number): PublicClient {
  const allConfigs = { ...mainNetworkConfigs, ...secondaryNetworkConfigs };
  const config = allConfigs[chainId];
  
  if (!config) {
    throw new Error(`No configuration found for chain ID ${chainId}`);
  }

  return createPublicClient({
    chain: config.chain,
    transport: http(config.chain.rpcUrls.default.http[0]),
  });
}

/**
 * Get wallet client by chain ID
 */
function getWalletClientByChainId(
  chainId: number,
  account: ReturnType<typeof privateKeyToAccount>
): WalletClient {
  const allConfigs = { ...mainNetworkConfigs, ...secondaryNetworkConfigs };
  const config = allConfigs[chainId];
  
  if (!config) {
    throw new Error(`No configuration found for chain ID ${chainId}`);
  }

  return createWalletClient({
    account,
    chain: config.chain,
    transport: http(config.chain.rpcUrls.default.http[0]),
  });
}

/**
 * Get nonce by chain ID (cache first)
 */
async function getNonceByChainId(chainId: number): Promise<number> {
  const redisNonce = await client.get(`nonce-${chainId}`);

  if (redisNonce == null) {
    const publicClient = getPublicClientByChainId(chainId);
    return await publicClient.getTransactionCount({
      address: process.env.NEXT_PUBLIC_OPERATOR_ADDRESS as Address,
    });
  }

  return Number(redisNonce);
}

/**
 * Process drip transaction for a network
 */
async function processDrip(
  account: ReturnType<typeof privateKeyToAccount>,
  chainId: number,
  data: `0x${string}`
): Promise<void> {
  const publicClient = getPublicClientByChainId(chainId);
  const walletClient = getWalletClientByChainId(chainId, account);
  
  const nonce = await getNonceByChainId(chainId);
  const gasPrice = await publicClient.getGasPrice();

  // Update nonce in redis with 5m TTL
  await client.set(`nonce-${chainId}`, nonce + 1, "EX", 300);

  try {
    await walletClient.sendTransaction({
      account,
      to: process.env.FAUCET_ADDRESS as Address,
      data,
      gasPrice: gasPrice * BigInt(2),
      gas: BigInt(500_000),
      nonce,
      chain: walletClient.chain,
    });
  } catch (e: any) {
    await postSlackMessage(
      `@ameya Error dripping for chain ${chainId}: ${e.message || String(e)}`
    );

    // Delete nonce key to attempt self-heal
    const delStatus = await client.del(`nonce-${chainId}`);
    await postSlackMessage(`Attempting self heal: ${delStatus}`);

    throw new Error(`Error when processing drip for chain ${chainId}`);
  }
}

export default async (req: NextApiRequest, res: NextApiResponse) => {
  const session: any = await getSession({ req });
  const { address, others }: { address: string; others: boolean } = req.body;

  if (!session) {
    return res.status(401).send({ error: "Not authenticated." });
  }

  // Anti-bot measures
  if (session.provider === "twitter") {
    if (!session.twitter_id) {
      return res.status(400).send({ error: "Invalid Twitter account." });
    }
  } else if (session.provider === "github") {
    if (!session.github_id) {
      return res.status(400).send({ error: "Invalid GitHub account." });
    }
  } else {
    return res.status(400).send({ error: "Unsupported authentication provider." });
  }

  if (!address || !isAddress(address)) {
    return res.status(400).send({ error: "Invalid address." });
  }

  let addr: string = address;

  // Handle ENS resolution
  if (address.toLowerCase().includes(".eth")) {
    const mainnetClient = createPublicClient({
      chain: mainnet,
      transport: http(`https://eth-mainnet.alchemyapi.io/v2/${process.env.ALCHEMY_API_KEY}`),
    });

    const resolvedAddress = await mainnetClient.getEnsAddress({ 
      name: address as `${string}.eth` 
    });

    if (!resolvedAddress) {
      return res.status(400).send({ error: "Invalid ENS name. No reverse record." });
    }

    addr = resolvedAddress;
  }

  const userId = session.provider === "twitter" ? session.twitter_id : session.github_id;
  const isWhitelisted = whitelist.includes(userId);

  if (!isWhitelisted) {
    const claimed: boolean = await hasClaimed(userId);
    if (claimed) {
      return res.status(400).send({ error: "Already claimed in 24h window" });
    }
  }

  // Create account from private key
  const account = privateKeyToAccount(process.env.OPERATOR_PRIVATE_KEY as `0x${string}`);

  // Generate transaction data
  const data = generateTxData(addr);

  // Determine which networks to claim on
  const claimNetworkConfigs = others
    ? { ...mainNetworkConfigs, ...secondaryNetworkConfigs }
    : mainNetworkConfigs;

  // Process drip for each network
  for (const chainId of Object.keys(claimNetworkConfigs)) {
    try {
      await processDrip(account, Number(chainId), data);
    } catch (e) {
      // If not whitelisted, force user to wait 15 minutes
      if (!isWhitelisted) {
        await client.set(userId, "true", "EX", 900);
      }

      return res
        .status(500)
        .send({ error: "Error fully claiming, try again in 15 minutes." });
    }
  }

  // Update claim status for non-whitelisted users
  if (!isWhitelisted) {
    await client.set(userId, "true", "EX", 86400);
  }

  if (isWhitelisted) {
    console.log(`${address} claimed from faucet`);
  }

  return res.status(200).send({ claimed: address });
};