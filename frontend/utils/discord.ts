import axios from "axios";

const DISCORD_BOT_TOKEN = process.env.DISCORD_BOT_TOKEN;
const DISCORD_GUILD_ID = process.env.DISCORD_GUILD_ID;

const DISCORD_API_BASE = "https://discord.com/api/v10";

// In-memory cache for guild roles (5 min TTL)
let cachedRoles: { id: string; name: string }[] | null = null;
let cacheExpiry = 0;
const CACHE_TTL_MS = 5 * 60 * 1000;

const MAGNITUDE_REGEX = /^magnitude\s+(\d+(?:\.\d+)?)/i;
const BYPASS_ROLES = ["administrator", "moderator"];

async function getGuildRoles(): Promise<{ id: string; name: string }[]> {
  if (cachedRoles && Date.now() < cacheExpiry) {
    return cachedRoles;
  }

  const res = await axios.get(
    `${DISCORD_API_BASE}/guilds/${DISCORD_GUILD_ID}/roles`,
    { headers: { Authorization: `Bot ${DISCORD_BOT_TOKEN}` } },
  );

  cachedRoles = res.data.map((r: any) => ({ id: r.id, name: r.name }));
  cacheExpiry = Date.now() + CACHE_TTL_MS;
  return cachedRoles!;
}

/**
 * Returns the highest "magnitude X.0" role value for a Discord user in the guild,
 * or null if the user is not a member of the guild.
 */
export async function getDiscordMagnitude(
  userId: string,
): Promise<number | null> {
  try {
    const [roles, memberRes] = await Promise.all([
      getGuildRoles(),
      axios.get(
        `${DISCORD_API_BASE}/guilds/${DISCORD_GUILD_ID}/members/${userId}`,
        { headers: { Authorization: `Bot ${DISCORD_BOT_TOKEN}` } },
      ),
    ]);

    const memberRoleIds: string[] = memberRes.data.roles;
    const memberRoleIdSet = new Set(memberRoleIds);

    // Allow administrator and moderator roles to bypass magnitude check
    for (const role of roles) {
      if (
        BYPASS_ROLES.includes(role.name.toLowerCase()) &&
        memberRoleIdSet.has(role.id)
      ) {
        return Infinity;
      }
    }

    // Build a map of role ID -> magnitude value for magnitude roles
    const magnitudeByRoleId = new Map<string, number>();
    for (const role of roles) {
      const match = role.name.match(MAGNITUDE_REGEX);
      if (match) {
        magnitudeByRoleId.set(role.id, parseFloat(match[1]));
      }
    }

    // Find the highest magnitude among the member's roles
    let highest = -Infinity;
    for (const roleId of memberRoleIds) {
      const mag = magnitudeByRoleId.get(roleId);
      if (mag !== undefined && mag > highest) {
        highest = mag;
      }
    }

    return highest === -Infinity ? 0 : highest;
  } catch (error: any) {
    // 404 means user is not in the guild
    if (error.response?.status === 404) {
      return null;
    }
    // 10007 = Unknown Member (also means not in guild)
    if (error.response?.data?.code === 10007) {
      return null;
    }
    throw error;
  }
}
