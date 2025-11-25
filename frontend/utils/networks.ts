import { sanvil, seismicDevnet2, seismicTestnet } from "seismic-viem";
import type { Chain } from "viem";

const isDevelopment = process.env.NODE_ENV === "development";

// Main network - the primary faucet network
export const mainNetwork: Chain = isDevelopment ? sanvil : seismicDevnet2;
