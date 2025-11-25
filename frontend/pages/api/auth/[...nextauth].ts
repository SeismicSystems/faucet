import NextAuth, { Account, Session, User, NextAuthOptions } from "next-auth"; // Next auth
import Twitter from "next-auth/providers/twitter"; // Twitter provider
import GitHub from "next-auth/providers/github"; // GitHub provider
import { JWT } from "next-auth/jwt";

// Extend session type to include custom fields
declare module "next-auth" {
  interface Session {
    provider?: string;
    twitter_id?: string;
    twitter_handle?: string;
    twitter_num_tweets?: number;
    twitter_num_followers?: number;
    twitter_created_at?: string;
    github_id?: string;
    github_username?: string;
    github_public_repos?: number;
    github_followers?: number;
    github_created_at?: string;
  }
}

// Extend JWT type to include custom fields
declare module "next-auth/jwt" {
  interface JWT {
    provider?: string;
    twitter_id?: string;
    twitter_handle?: string;
    twitter_num_tweets?: number;
    twitter_num_followers?: number;
    twitter_created_at?: string;
    github_id?: string;
    github_username?: string;
    github_public_repos?: number;
    github_followers?: number;
    github_created_at?: string;
  }
}

export const authOptions: NextAuthOptions = {
  providers: [
    // Twitter OAuth provider (OAuth 2.0)
    Twitter({
      clientId: process.env.TWITTER_CLIENT_ID as string,
      clientSecret: process.env.TWITTER_CLIENT_SECRET as string,
      version: "2.0",
    }),
    // GitHub OAuth provider
    GitHub({
      clientId: process.env.GITHUB_CLIENT_ID as string,
      clientSecret: process.env.GITHUB_CLIENT_SECRET as string,
    }),
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
        if (account?.provider === "twitter") {
          // Attach Twitter parameters
          const twitterProfile = profile as any;
          token.provider = "twitter";
          token.twitter_id = account?.providerAccountId;
          token.twitter_handle = twitterProfile.screen_name;
          token.twitter_num_tweets = twitterProfile.statuses_count;
          token.twitter_num_followers = twitterProfile.followers_count;
          token.twitter_created_at = twitterProfile.created_at;
        } else if (account?.provider === "github") {
          // Attach GitHub parameters - use profile.id which matches the GitHub API user ID
          const githubProfile = profile as any;
          token.provider = "github";
          token.github_id = githubProfile.id?.toString(); // This should be "74180822"
          token.github_username = githubProfile.login;
          token.github_public_repos = githubProfile.public_repos;
          token.github_followers = githubProfile.followers;
          token.github_created_at = githubProfile.created_at;
        }
      }

      // Resolve JWT
      return Promise.resolve(token);
    },
    // On session retrieval
    session: async ({ session, token }) => {
      // Attach provider info from token to session
      session.provider = token.provider;

      if (token.provider === "twitter") {
        // Attach Twitter params from JWT to session
        session.twitter_id = token.twitter_id;
        session.twitter_handle = token.twitter_handle;
        session.twitter_num_tweets = token.twitter_num_tweets;
        session.twitter_num_followers = token.twitter_num_followers;
        session.twitter_created_at = token.twitter_created_at;
      } else if (token.provider === "github") {
        // Attach GitHub params from JWT to session
        session.github_id = token.github_id;
        session.github_username = token.github_username;
        session.github_public_repos = token.github_public_repos;
        session.github_followers = token.github_followers;
        session.github_created_at = token.github_created_at;
      }

      // Return session
      return session;
    },
  },
};

export default NextAuth(authOptions);
