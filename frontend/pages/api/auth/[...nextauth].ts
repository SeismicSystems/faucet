import NextAuth, { Account, Session, User, NextAuthOptions } from "next-auth"; // Next auth
import GitHub from "next-auth/providers/github"; // GitHub provider
import Discord from "next-auth/providers/discord"; // Discord provider
import { JWT } from "next-auth/jwt";

// Extend session type to include custom fields
declare module "next-auth" {
  interface Session {
    provider?: string;
    github_id?: string;
    github_username?: string;
    github_public_repos?: number;
    github_followers?: number;
    github_created_at?: string;
    discord_id?: string;
    discord_username?: string;
    discord_verified?: boolean;
  }
}

// Extend JWT type to include custom fields
declare module "next-auth/jwt" {
  interface JWT {
    provider?: string;
    github_id?: string;
    github_username?: string;
    github_public_repos?: number;
    github_followers?: number;
    github_created_at?: string;
    discord_id?: string;
    discord_username?: string;
    discord_verified?: boolean;
  }
}

export const authOptions: NextAuthOptions = {
  providers: [
    // GitHub OAuth provider
    GitHub({
      clientId: process.env.GITHUB_CLIENT_ID as string,
      clientSecret: process.env.GITHUB_CLIENT_SECRET as string,
    }),
    // // Discord OAuth provider
    // Discord({
    //   clientId: process.env.DISCORD_CLIENT_ID as string,
    //   clientSecret: process.env.DISCORD_CLIENT_SECRET as string,
    // }),
  ],
  // Custom page:
  pages: {
    // On error, throw to home
    error: "/",
  },
  // Use JWT
  session: {
    strategy: "jwt" as const,
    // 30 day expiry
    maxAge: 30 * 24 * 60 * 60,
    // Refresh JWT on each login
    updateAge: 0,
  },
  // Secret for JWT signing and encryption
  secret: process.env.NEXTAUTH_SECRET || process.env.NEXTAUTH_JWT_SECRET,
  callbacks: {
    // On signin + signout
    jwt: async ({ token, user, account, profile }) => {
      // Check if user is signing in (versus logging out)
      const isSignIn = user ? true : false;

      // If signing in
      if (isSignIn && profile) {
        if (account?.provider === "github") {
          // Attach GitHub parameters - use profile.id which matches the GitHub API user ID
          const githubProfile = profile as any;
          token.provider = "github";
          token.github_id = githubProfile.id?.toString(); // This should be "74180822"
          token.github_username = githubProfile.login;
          token.github_public_repos = githubProfile.public_repos;
          token.github_followers = githubProfile.followers;
          token.github_created_at = githubProfile.created_at;
        } else if (account?.provider === "discord") {
          // Attach Discord parameters
          const discordProfile = profile as any;
          token.provider = "discord";
          token.discord_id = account?.providerAccountId;
          token.discord_username = discordProfile.username;
          token.discord_verified = discordProfile.verified;
        }
      }

      // Resolve JWT
      return Promise.resolve(token);
    },
    // On session retrieval
    session: async ({ session, token }) => {
      // Attach provider info from token to session
      session.provider = token.provider;

      if (token.provider === "github") {
        // Attach GitHub params from JWT to session
        session.github_id = token.github_id;
        session.github_username = token.github_username;
        session.github_public_repos = token.github_public_repos;
        session.github_followers = token.github_followers;
        session.github_created_at = token.github_created_at;
      } else if (token.provider === "discord") {
        // Attach Discord params from JWT to session
        session.discord_id = token.discord_id;
        session.discord_username = token.discord_username;
        session.discord_verified = token.discord_verified;
      }

      // Return session
      return session;
    },
  },
};

export default NextAuth(authOptions);
