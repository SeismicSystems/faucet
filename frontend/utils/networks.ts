import { sanvil, seismicDevnet2, seismicTestnet, seismicTestnetGcp1, createSeismicGcpTestnet } from "seismic-viem";
import type { Chain } from "viem";

const seismicTestnetGcp0 = createSeismicGcpTestnet(0);

const isDevelopment = process.env.NODE_ENV === "development";

// Main network - the primary faucet network
export const mainNetwork: Chain = isDevelopment ? sanvil : seismicTestnetGcp0;
