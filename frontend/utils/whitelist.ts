/*
 * WHITELIST (Trusted - 250 SUSDC, no cooldown)
 */

// Seismic team GitHub IDs
const SEISMIC_TEAM = [
  "74180822", // Ameya Deshmukh (@ameya-deshmukh)
  "1449882", // Christian Drappi (@cdrappi)
  "25928722", // Matthias Wright (@matthias-wright)
  "71679972", // Dalton Coder (@daltoncoder)
  "30359739", // Lyron Co Ting Keh (@lyronctk)
  "57149625", // Matt Haines (@mHaines9219)
  "9342524", // Sam Laferriere (@samlaf)
  "67980579", // Henry Baldwin (@HenryMBaldwin)
];

// Brookwell team GitHub IDs
const BROOKWELL_TEAM = [
  "8132955", // Ravi Riley (@raviriley)
  "51090093", // Rohan Patra (@rohan-patra)
];

// Chainlink team GitHub IDs
const CHAINLINK_TEAM = [
  "2430254", // Todor Karaivanov (@tkaraivanov)
  "164586642", // Felix Medina (@femedmad)
];

// Pimlico team GitHub IDs
const PIMLICO_TEAM = [
  "97399882", // mous (@mouseless0x)
];

// Ankr team Github IDs
const ANKR_TEAM = [
  "24973480", // Finn (@guiltylotus)
  "165103466", // Felip (@fr-automator)
];

// Fireblocks team GitHub IDs
const FIREBLOCKS_TEAM = [
  "207824728", // Tomer Shoham (@tomer-shoham)
];

// Anchorage team GitHub IDs
const ANCHORAGE_TEAM = [
  "272382493", // Rodrigo Nascimento (@rodrigonascimento-anchorlabs)
];

// Den team GitHub IDs
const DEN_TEAM = [
  "8534926", // Jonah Erlich (@jierlich)
];

// MoonPay team GitHub IDs
const MOONPAY_TEAM = [
  "42893075", // Leonard Kulms (@leonardkulms)
  "26767653", // abcalphabet (@abcalphabet)
];

/*
 * DEVELOPERS (50 SUSDC, 24h cooldown)
 */
const DEVELOPERS = [
  "158461935", // Dave Thompson (@Davethompson01)
];

// Discord IDs for whitelist
const DISCORD_WHITELIST: string[] = [];

// Discord IDs for developers
const DISCORD_DEVELOPERS: string[] = [];

/*
 * EXPORTS
 */

// Whitelist: trusted users (250 SUSDC, no cooldown)
export const whitelist = [
  ...SEISMIC_TEAM,
  ...BROOKWELL_TEAM,
  ...CHAINLINK_TEAM,
  ...PIMLICO_TEAM,
  ...ANKR_TEAM,
  ...FIREBLOCKS_TEAM,
  ...ANCHORAGE_TEAM,
  ...DEN_TEAM,
  ...MOONPAY_TEAM,
  ...DISCORD_WHITELIST,
];

// Developers: elevated access (50 SUSDC, 24h cooldown)
export const developerList = [...DEVELOPERS, ...DISCORD_DEVELOPERS];
