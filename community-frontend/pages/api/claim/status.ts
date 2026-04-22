import Redis from "ioredis";
import { getSession } from "next-auth/react";
import type { NextApiRequest, NextApiResponse } from "next";
import { mainNetwork } from "@/utils/networks";

// Setup redis client
const client = new Redis(process.env.REDIS_URL as string);

/**
 * Checks if a user has claimed from faucet on a specific network in last 24h
 * @param {string} userId - Twitter or GitHub user ID
 * @param {string} chainName - Network/chain name
 * @returns {Promise<boolean>} claim status
 */
export async function hasClaimed(
  userId: string,
  chainName: string,
): Promise<boolean> {
  const key = `faucet:${chainName}:${userId}`;
  const resp: string | null = await client.get(key);
  return resp ? true : false;
}

export default async (req: NextApiRequest, res: NextApiResponse) => {
  const session: any = await getSession({ req });

  if (session) {
    try {
      // Use provider-specific ID for claim tracking
      const userId =
        session.provider === "twitter"
          ? session.twitter_id
          : session.provider === "github"
            ? session.github_id
            : session.discord_id;

      // Check claim status on the main network
      const claimed = await hasClaimed(userId, mainNetwork.name);

      res.status(200).send({ claimed });
    } catch (error) {
      console.error("Error checking claim status:", error);
      res.status(500).send({ error: "Error checking claim status." });
    }
  } else {
    res.status(401).send({ error: "Not authenticated." });
  }
};
