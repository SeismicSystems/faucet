import { sanvil, createSeismicDevnet } from "seismic-viem";
import type { Chain } from "viem";

const isDevelopment = process.env.NODE_ENV === "development";

const seismicTestnet0: Chain = createSeismicDevnet({
  nodeHost: "testnet-0.seismictest.net",
});

// Main network - the primary faucet network
export const mainNetwork: Chain = isDevelopment ? sanvil : seismicTestnet0;
