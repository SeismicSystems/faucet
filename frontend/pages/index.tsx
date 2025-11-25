import axios from "axios"; // Requests
import Image from "next/image"; // Image
import { isAddress } from "viem";
import { toast } from "react-toastify"; // Toast notifications
import Layout from "components/Layout"; // Layout wrapper
import { useRouter } from "next/router"; // Router
import styles from "styles/Home.module.scss"; // Styles
import { ReactElement, useState } from "react"; // Local state + types
import { hasClaimed } from "pages/api/claim/status"; // Claim status
import { signIn, getSession, signOut } from "next-auth/client"; // Auth

export default function Home({
  session,
  claimed: initialClaimed,
}: {
  session: any;
  claimed: boolean;
}) {
  // Collect prefilled address
  const {
    query: { addr },
  } = useRouter();
  // Fill prefilled address
  const prefilledAddress: string = addr && typeof addr === "string" ? addr : "";

  // Claim address
  const [address, setAddress] = useState<string>(prefilledAddress);
  // Claimed status
  const [claimed, setClaimed] = useState<boolean>(initialClaimed);
  // First claim
  const [firstClaim, setFirstClaim] = useState<boolean>(false);
  // Loading status
  const [loading, setLoading] = useState<boolean>(false);

  /**
   * Processes a claim to the faucet
   */
  const processClaim = async () => {
    // Toggle loading
    setLoading(true);

    try {
      // Post new claim with recipient address
      await axios.post("/api/claim/new", { address });
      // Toast if success + toggle claimed
      toast.success("Tokens dispersed—check balances shortly!");
      setClaimed(true);
      setFirstClaim(true);
    } catch (error: any) {
      // If error, toast error message
      toast.error(error.response.data.error);
    }

    // Toggle loading
    setLoading(false);
  };

  return (
    <Layout>
      {/* CTA + description */}
      <div className={styles.home__cta}>
        <div>
          <a
            href="https://seismic.systems"
            target="_blank"
            rel="noopener noreferrer"
          >
            <Image
              src="/seismiclogo.png"
              alt="Seismic Logo"
              width={200}
              height={58}
            />
          </a>
        </div>
        <h1>Bootstrap your testnet/devnet wallet</h1>
        <span>This faucet funds your Seismic testnet/devnet wallet.</span>
      </div>

      {/* Claim from facuet card */}
      <div className={styles.home__card}>
        {/* Card title */}
        <div className={styles.home__card_title}>
          <h3>Request Tokens</h3>
        </div>

        {/* Card content */}
        <div className={styles.home__card_content}>
          {!session ? (
            // If user is unauthenticated:
            <div className={styles.content__unauthenticated}>
              {/* Reasoning for OAuth */}
              <p>
                To prevent faucet botting, you must sign in with Twitter or
                GitHub. We request read-only access to verify your account.
              </p>

              {/* Sign in buttons */}
              <div
                style={{
                  display: "flex",
                  gap: "10px",
                  flexDirection: "column",
                }}
              >
                <button
                  className={styles.button__main}
                  onClick={() => signIn("twitter")}
                >
                  Sign In with Twitter
                </button>
                <button
                  className={styles.button__main}
                  onClick={() => signIn("github")}
                >
                  Sign In with GitHub
                </button>
              </div>
            </div>
          ) : (
            // If user is authenticated:
            <div className={styles.content__authenticated}>
              {claimed ? (
                // If user has already claimed once in 24h
                <div className={styles.content__claimed}>
                  <p>
                    {firstClaim
                      ? "You have successfully claimed tokens. You can request again in 24 hours."
                      : "You have already claimed tokens today. Please try again in 24 hours."}
                  </p>

                  <input
                    type="text"
                    placeholder="0x478669bb3846d79f2ff511ce99eaee8f85554476"
                    disabled
                  />
                  <button className={styles.button__main} disabled>
                    Tokens Already Claimed
                  </button>
                </div>
              ) : (
                // If user has not claimed in 24h
                <div className={styles.content__unclaimed}>
                  {/* Claim description */}
                  <p>
                    Enter your Seismic testnet/devnet address to receive tokens:
                  </p>

                  {/* Address input */}
                  <input
                    type="text"
                    placeholder="0x478669bb3846d79f2ff511ce99eaee8f85554476"
                    value={address}
                    onChange={(e) => setAddress(e.target.value)}
                  />

                  {isAddress(address) ? (
                    // If address is valid, allow claiming
                    <button
                      className={styles.button__main}
                      onClick={processClaim}
                      disabled={loading}
                    >
                      {!loading ? "Claim" : "Claiming..."}
                    </button>
                  ) : (
                    // Else, force fix
                    <button className={styles.button__main} disabled>
                      {address === ""
                        ? "Enter Valid Address"
                        : "Invalid Address"}
                    </button>
                  )}
                </div>
              )}

              {/* General among claimed or unclaimed, allow signing out */}
              <div className={styles.content__twitter}>
                <button onClick={() => signOut()}>
                  Sign out @
                  {session.provider === "twitter"
                    ? session.twitter_handle
                    : session.github_username}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Faucet details card */}
      <div className={styles.home__card}>
        {/* Card title */}
        <div className={styles.home__card_title}>
          <h3>Faucet Details</h3>
        </div>

        {/* General information */}
        <div>
          <div className={styles.home__card_content_section}>
            <h4>General Information</h4>
            <p>
              Sign in with Twitter or GitHub to claim ETH from the faucet. No
              minimum requirements - just need a valid account.
            </p>
            <p className={styles.home__card_content_section_lh}>
              The faucet drips ETH on your configured testnet. Each claim gives
              you 1 ETH.
            </p>
            <p>You can claim from the faucet once every 24 hours.</p>
          </div>
        </div>
      </div>
    </Layout>
  );
}

export async function getServerSideProps(context: any) {
  // Collect session
  const session: any = await getSession(context);

  if (!session) {
    return {
      props: {
        session,
        claimed: false,
      },
    };
  }

  const userId =
    session.provider === "twitter" ? session.twitter_id : session.github_id;

  // Check if user is whitelisted (same as backend)
  const AMEYA_GITHUB_ID = "74180822";
  const CHRISTIAN_GITHUB_ID = "1449882";
  const AMEYA_TWITTER_ID = "1311531128201916417";

  const whitelist = [AMEYA_GITHUB_ID, CHRISTIAN_GITHUB_ID, AMEYA_TWITTER_ID];
  const isWhitelisted = whitelist.includes(userId);

  return {
    props: {
      session,
      // If whitelisted, always show as not claimed (can claim anytime)
      claimed: isWhitelisted ? false : await hasClaimed(userId),
    },
  };
}
