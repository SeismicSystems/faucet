import NextAuth from "next-auth"; // Next auth
import Providers from "next-auth/providers"; // Twitter provider

export default NextAuth({
  providers: [
    // Twitter OAuth provider
    Providers.Twitter({
      clientId: process.env.TWITTER_CLIENT_ID,
      clientSecret: process.env.TWITTER_CLIENT_SECRET,
    }),
    // GitHub OAuth provider
    Providers.GitHub({
      clientId: process.env.GITHUB_CLIENT_ID,
      clientSecret: process.env.GITHUB_CLIENT_SECRET,
    }),
  ],
  // Custom page:
  pages: {
    // On error, throw to home
    error: "/",
  },
  // Use JWT
  session: {
    jwt: true,
    // 30 day expiry
    maxAge: 30 * 24 * 60 * 60,
    // Refresh JWT on each login
    updateAge: 0,
  },
  jwt: {
    // JWT secret
    secret: process.env.NEXTAUTH_JWT_SECRET,
  },
  callbacks: {
    // On signin + signout
    jwt: async (token, user, account, profile) => {
      // Check if user is signing in (versus logging out)
      const isSignIn = user ? true : false;

      // If signing in
      if (isSignIn) {
        if (account?.provider === "twitter") {
          // Attach Twitter parameters
          token.provider = "twitter";
          token.twitter_id = account?.id;
          token.twitter_handle = profile?.screen_name;
          token.twitter_num_tweets = profile?.statuses_count;
          token.twitter_num_followers = profile?.followers_count;
          token.twitter_created_at = profile?.created_at;
        } else if (account?.provider === "github") {
          // Attach GitHub parameters - use profile.id which matches the GitHub API user ID
          token.provider = "github";
          token.github_id = profile?.id?.toString(); // This should be "74180822"
          token.github_username = profile?.login;
          token.github_public_repos = profile?.public_repos;
          token.github_followers = profile?.followers;
          token.github_created_at = profile?.created_at;
        }
      }

      // Resolve JWT
      return Promise.resolve(token);
    },
    // On session retrieval
    session: async (session, user) => {
      // Attach provider info
      session.provider = user.provider;

      if (user.provider === "twitter") {
        // Attach Twitter params from JWT to session
        session.twitter_id = user.twitter_id;
        session.twitter_handle = user.twitter_handle;
        session.twitter_num_tweets = user.twitter_num_tweets;
        session.twitter_num_followers = user.twitter_num_followers;
        session.twitter_created_at = user.twitter_created_at;
      } else if (user.provider === "github") {
        // Attach GitHub params from JWT to session
        session.github_id = user.github_id;
        session.github_username = user.github_username;
        session.github_public_repos = user.github_public_repos;
        session.github_followers = user.github_followers;
        session.github_created_at = user.github_created_at;
      }

      // Resolve session
      return Promise.resolve(session);
    },
  },
});
