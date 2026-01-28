/*
 * WHITELIST (Trusted - 10 ETH, no cooldown)
 */

// Seismic team GitHub IDs
const SEISMIC_TEAM = [
  "74180822",   // Ameya Deshmukh (@ameya-deshmukh)
  "1449882",    // Christian Drappi (@cdrappi)
  "25928722",   // Matthias Wright (@matthias-wright)
  "71679972",   // Dalton Coder (@daltoncoder)
  "30359739",   // Lyron Co Ting Keh (@lyronctk)
  "57149625",   // Matt Haines (@mHaines9219)
  "9342524",    // Sam Laferriere (@samlaf)
];

// Brookwell team GitHub IDs
const BROOKWELL_TEAM = [
  "8132955",    // Ravi Riley (@raviriley)
  "51090093",   // Rohan Patra (@rohan-patra)
];


// Twitter IDs for whitelist
const TWITTER_WHITELIST = [
  "1311531128201916417", // Ameya Deshmukh (@0xameya)
];

// Chainlink team GitHub IDs
const CHAINLINK_TEAM = [
  "2430254",    // Todor Karaivanov (@tkaraivanov)
  "164586642",  // Felix Medina (@femedmad)
];

// Pimlico team GitHub IDs
const PIMLICO_TEAM = [
  "97399882",   // mous (@mouseless0x)
];

// Ankr team Github IDs
const ANKR_TEAM = [
  "24973480", // Finn (@guiltylotus)
  "165103466" // Felip (@fr-automator)

];

/*
 * DEVELOPERS (2 ETH, 24h cooldown)
 */
const DEVELOPERS = [
  "158461935", // Dave Thompson (@Davethompson01)
];


// Twitter IDs for developers
const TWITTER_DEVELOPERS: string[] = [];

/*
 * EXPORTS
 */

// Whitelist: trusted users (10 ETH, no cooldown)
export const whitelist = [
  ...SEISMIC_TEAM,
  ...BROOKWELL_TEAM,
  ...CHAINLINK_TEAM,
  ...PIMLICO_TEAM,
  ...ANKR_TEAM,
  ...TWITTER_WHITELIST,
];

// Developers: elevated access (2 ETH, 24h cooldown)
export const developerList = [
  ...DEVELOPERS,
];
