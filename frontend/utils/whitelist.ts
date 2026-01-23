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

// Chainlink team GitHub IDs
const CHAINLINK_TEAM = [
  "2430254",    // Todor Karaivanov (@tkaraivanov)
];

// Twitter IDs (separate since they're a different provider)
const TWITTER_WHITELIST = [
  "1311531128201916417", // Ameya Deshmukh (@0xameya)
];

export const whitelist = [
  ...SEISMIC_TEAM,
  ...BROOKWELL_TEAM,
  ...CHAINLINK_TEAM,
  ...TWITTER_WHITELIST,
];
