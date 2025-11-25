import styles from "@/styles/Layout.module.scss"; // Styles
import { default as HTMLHead } from "next/head"; // Meta

// Page layout
export default function Layout({
  children,
}: {
  children: (JSX.Element | null)[];
}) {
  return (
    <div className={styles.layout}>
      {/* Meta + Head */}
      <Head />

      {/* Header */}
      <Header />

      {/* Layout sizer */}
      <div className={styles.layout__content}>{children}</div>

      {/* Footer */}
      <Footer />
    </div>
  );
}

// Header
function Header() {
  return (
    <div className={styles.layout__header}>
      <div className={styles.header__content}>
        <div></div> {/* Empty div for spacing */}
        <a
          href="https://github.com/SeismicSystems/faucet"
          target="_blank"
          rel="noopener noreferrer"
          className={styles.github__link}
        >
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="currentColor"
            style={{ marginRight: "8px" }}
          >
            <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
          </svg>
          Source Code
        </a>
      </div>
    </div>
  );
}

// Head + Meta
function Head() {
  return (
    <HTMLHead>
      {/* Google Fonts */}
      <link rel="preconnect" href="https://fonts.googleapis.com" />
      <link
        rel="preconnect"
        href="https://fonts.gstatic.com"
        crossOrigin="anonymous"
      />
      <link
        href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;700&display=swap"
        rel="stylesheet"
      />

      {/* Favicon */}
      <link rel="shortcut icon" href="/favicon.ico" />

      {/* Primary Meta Tags */}
      <title>Seismic Faucet | Bootstrap your testnet/devnet wallet</title>
      <meta
        name="title"
        content="Seismic Faucet | Bootstrap your testnet/devnet wallet"
      />
      <meta
        name="description"
        content="MultiFaucet funds a wallet with ETH, wETH, DAI, and NFTs across 8 testnet networks, at once."
      />

      {/* OG + Facebook */}
      <meta property="og:type" content="website" />
      <meta property="og:url" content="https://faucet.seismic.systems" />
      <meta
        property="og:title"
        content="Seismic Faucet | Bootstrap your testnet wallet"
      />
      <meta
        property="og:description"
        content="MultiFaucet funds a wallet with ETH, wETH, DAI, and NFTs across 8 testnet networks, at once."
      />
      <meta
        property="og:image"
        content="https://faucet.seismic.systems/meta.png"
      />

      {/* Twitter */}
      <meta property="twitter:card" content="summary_large_image" />
      <meta property="twitter:url" content="https://faucet.seismic.systems" />
      <meta
        property="twitter:title"
        content="Seismic Faucet | Bootstrap your testnet wallet"
      />
      <meta
        property="twitter:description"
        content="MultiFaucet funds a wallet with ETH, wETH, DAI, and NFTs across 8 testnet networks, at once."
      />
      <meta
        property="twitter:image"
        content="https://faucet.seismic.systems/meta.png"
      />
    </HTMLHead>
  );
}

// Footer
function Footer() {
  return (
    <div className={styles.layout__footer}>
      {/* Credits */}
      <p style={{ fontSize: "14px", color: "#666" }}>
        We thank{" "}
        <a
          href="https://github.com/Anish-Agnihotri"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: "#0070f3", textDecoration: "none" }}
        >
          Anish Agnihotri
        </a>{" "}
        and{" "}
        <a
          href="https://paradigm.xyz"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: "#0070f3", textDecoration: "none" }}
        >
          Paradigm
        </a>{" "}
        for the original{" "}
        <a
          href="https://faucet.paradigm.xyz"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: "#0070f3", textDecoration: "none" }}
        >
          MultiFaucet
        </a>{" "}
        that this project is based on, and for their contributions to the OSS
        blockchain ecosystem.
      </p>

      {/* Disclaimer */}
      <p style={{ marginTop: "20px" }}>
        These smart contracts are being provided as is. No guarantee,
        representation or warranty is being made, express or implied, as to the
        safety or correctness of the user interface or the smart contracts. They
        have not been audited and as such there can be no assurance they will
        work as intended, and users may experience delays, failures, errors,
        omissions or loss of transmitted information. Seismic is not liable for
        any of the foregoing. Users should proceed with caution and use at their
        own risk.
      </p>
    </div>
  );
}
