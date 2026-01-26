export const seismicFaucetAbi = [
  // Events
  {
    type: "event",
    name: "FaucetDripped",
    inputs: [
      {
        name: "recipient",
        type: "address",
        indexed: true,
        internalType: "address",
      },
    ],
    anonymous: false,
  },
  {
    type: "event",
    name: "FaucetDrained",
    inputs: [
      {
        name: "recipient",
        type: "address",
        indexed: true,
        internalType: "address",
      },
    ],
    anonymous: false,
  },
  {
    type: "event",
    name: "OperatorUpdated",
    inputs: [
      {
        name: "operator",
        type: "address",
        indexed: true,
        internalType: "address",
      },
      {
        name: "status",
        type: "bool",
        indexed: false,
        internalType: "bool",
      },
    ],
    anonymous: false,
  },
  {
    type: "event",
    name: "SuperOperatorUpdated",
    inputs: [
      {
        name: "operator",
        type: "address",
        indexed: true,
        internalType: "address",
      },
      {
        name: "status",
        type: "bool",
        indexed: false,
        internalType: "bool",
      },
    ],
    anonymous: false,
  },
  // Read Functions
  {
    type: "function",
    name: "ETH_AMOUNT",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256", internalType: "uint256" }],
  },
  {
    type: "function",
    name: "DEVELOPER_ETH_AMOUNT",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256", internalType: "uint256" }],
  },
  {
    type: "function",
    name: "WHITELIST_ETH_AMOUNT",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "uint256", internalType: "uint256" }],
  },
  {
    type: "function",
    name: "approvedOperators",
    stateMutability: "view",
    inputs: [{ name: "", type: "address", internalType: "address" }],
    outputs: [{ name: "", type: "bool", internalType: "bool" }],
  },
  {
    type: "function",
    name: "superOperators",
    stateMutability: "view",
    inputs: [{ name: "", type: "address", internalType: "address" }],
    outputs: [{ name: "", type: "bool", internalType: "bool" }],
  },
  {
    type: "function",
    name: "availableDrips",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "ethDrips", type: "uint256", internalType: "uint256" }],
  },
  // Write Functions
  {
    type: "function",
    name: "drip",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_recipient",
        type: "address",
        internalType: "address",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "dripDeveloper",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_recipient",
        type: "address",
        internalType: "address",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "dripWhitelist",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_recipient",
        type: "address",
        internalType: "address",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "drain",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_recipient",
        type: "address",
        internalType: "address",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "updateApprovedOperator",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_operator",
        type: "address",
        internalType: "address",
      },
      {
        name: "_status",
        type: "bool",
        internalType: "bool",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "updateSuperOperator",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_operator",
        type: "address",
        internalType: "address",
      },
      {
        name: "_status",
        type: "bool",
        internalType: "bool",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "updateDripAmount",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_ethAmount",
        type: "uint256",
        internalType: "uint256",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "updateDeveloperDripAmount",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_ethAmount",
        type: "uint256",
        internalType: "uint256",
      },
    ],
    outputs: [],
  },
  {
    type: "function",
    name: "updateWhitelistDripAmount",
    stateMutability: "nonpayable",
    inputs: [
      {
        name: "_ethAmount",
        type: "uint256",
        internalType: "uint256",
      },
    ],
    outputs: [],
  },
  // Receive Function
  {
    type: "receive",
    stateMutability: "payable",
  },
] as const;
