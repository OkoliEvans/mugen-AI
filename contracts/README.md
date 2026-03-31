# Mugen — On-Chain Contracts

Settlement layer for the Mugen verifiable inference network.
Two contracts work together to bridge ZK proof verification from Ethereum to StarkNet.

---

## Architecture

```
Rust Gateway
     │
     ▼
InferenceBridge.sol          (Eth Sepolia)
  ├── KZG pairing check via Halo2Verifier.sol
  └── IStarknetMessaging.sendMessageToL2()
                │
                │  L1 → L2 message (~1–3 min)
                ▼
InferenceVerifier.cairo      (StarkNet Sepolia)
  └── #[l1_handler] consume_inference_result()
        ├── validates L1 sender whitelist
        ├── replay protection
        └── writes InferenceRecord to storage
```

**The KZG pairing check runs entirely on Ethereum.** StarkNet does zero cryptographic
work — trust comes from StarkNet core only delivering messages from the whitelisted
`InferenceBridge.sol` address.

---

## Deployed Addresses

| Contract | Network | Address |
|---|---|---|
| `Halo2Verifier.sol` | Eth Sepolia | `0x7bcf4980868bA06A38AC561904aE6BDEd9Ee46D2` |
| `InferenceVerifier.sol` | Eth Sepolia | `0x37c5c1E314d2d895Dce71d2fbDBB49DDA74c8699` |
| `InferenceBridge.sol` | Eth Sepolia | `0x820fa9edB1DD0f248A4a8FB44693505417656480` |
| `InferenceVerifier.cairo` | StarkNet Sepolia | `0x048e54ece6691ca3f76246895cc1ac7c073a9377e02518aeed908618eb5ec7ca` |

---

## Directory Structure

```
contracts/
├── evm/
│   ├── src/
│   │   ├── InferenceVerifier.sol   — model registry + proof attestation
│   │   ├── InferenceBridge.sol     — KZG verifier + L1→L2 relay
│   │   └── AggregatedVerifier.sol  — aggregated proof settlement (Phase 2)
│   ├── script/
│   │   └── Deploy.s.sol            — Foundry deploy scripts
│   ├── test/
│   └── foundry.toml
└── starknet_l2/
    ├── src/
    │   ├── lib.cairo
    │   └── inference_verifier.cairo — L1→L2 settlement consumer
    ├── tests/
    │   └── test_inference_verifier.cairo
    └── Scarb.toml
```

---

## EVM Contracts

### `InferenceVerifier.sol`

Registry and attestation layer wrapping the EZKL Halo2 circuit verifier.

**Key functions:**

| Function | Access | Description |
|---|---|---|
| `registerModel(modelId, ipfsCid, inputShapeHash)` | `onlyOwner` | Register a model after IPFS pin |
| `submitProof(proof, instances, modelId, inputHash, outputHash)` | whitelisted settler | Verify and attest an inference |
| `isVerified(outputHash)` | public | Check if an output hash is attested |
| `getAttestation(outputHash)` | public | Fetch full attestation record |
| `getModel(modelId)` | public | Fetch model registration record |
| `computeModelId(name, version)` | public pure | Derive `keccak256(name \|\| version)` |
| `setSettler(settler, approved)` | `onlyOwner` | Manage settler whitelist |
| `upgradeVerifier(newVerifier)` | `onlyOwner` | Replace Halo2 verifier circuit |

**Security:** `Ownable2Step`, `Pausable`, `ReentrancyGuard`. Attestations are permanent — no delete path.

### `InferenceBridge.sol`

Used exclusively for StarkNet settlement. Verifies the KZG proof on L1 then relays
the result to `InferenceVerifier.cairo` via `IStarknetMessaging.sendMessageToL2()`.

**Key functions:**

| Function | Access | Description |
|---|---|---|
| `verifyAndBridge(proof, publicInputs, inferenceId, modelHash)` | public payable | KZG check + L1→L2 relay |
| `isVerified(inferenceId)` | public | Replay guard check |

`msg.value` is forwarded to StarkNet core as the L1→L2 messaging fee.
Configure via `SETTLER_STARKNET_BRIDGE_FEE_WEI` (default: 0.03 ETH on Sepolia).

---

## Cairo Contract

### `InferenceVerifier.cairo`

StarkNet settlement consumer. Called by StarkNet core when an L1→L2 message
arrives from `InferenceBridge.sol`.

**Entry points:**

| Function | Type | Description |
|---|---|---|
| `consume_inference_result` | `#[l1_handler]` | Receives and records L1 settlement |
| `is_inference_verified` | view | Check if inference_id is settled |
| `get_inference_record` | view | Fetch full `InferenceRecord` |
| `add_l1_verifier` | external (owner) | Whitelist an L1 bridge address |
| `remove_l1_verifier` | external (owner) | Remove an L1 bridge address |

**Payload layout** (must match `InferenceBridge.sol`):
```
payload[0] = inference_id   (bytes32 → felt252)
payload[1] = model_hash     (bytes32 → felt252)
payload[2] = timestamp      (uint256 → u64)
payload[3] = submitter      (address → felt252)
```

---

## Deployment

### Prerequisites

```bash
# EVM
curl -L https://foundry.paradigm.xyz | bash && foundryup

# StarkNet
curl --proto '=https' --tlsv1.2 -sSf https://docs.swmansion.com/scarb/install.sh | sh
# sncast ships with Starknet Foundry
curl -L https://raw.githubusercontent.com/foundry-rs/starknet-foundry/master/scripts/install.sh | sh
```

### Deploy EVM (Eth Sepolia)

```bash
cd evm

# 1. Deploy InferenceVerifier + InferenceBridge
forge script script/Deploy.s.sol:Deploy \
  --sig "deployInference()" \
  --rpc-url $RPC_URL \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --verify \
  --etherscan-api-key $ETHERSCAN_API_KEY

# 2. Optionally register a model immediately
MODEL_NAME=tiny_mlp_v1 \
MODEL_VERSION=0.1.0 \
MODEL_IPFS_CID=QmfC82kKWio31Zstf7Q1FhFoUJCREjg8VV3pcHb4kgydaU \
MODEL_INPUT_SHAPE=$(cast abi-encode 'f(uint256[])' '[1,4]' | cut -c3-) \
INFERENCE_VERIFIER_ADDRESS=0x37c5c1E314d2d895Dce71d2fbDBB49DDA74c8699 \
forge script script/Deploy.s.sol:Deploy \
  --sig "registerModel()" \
  --rpc-url $RPC_URL \
  --private-key $PRIVATE_KEY \
  --broadcast
```

### Deploy StarkNet

```bash
cd starknet_l2
scarb build

# Declare the contract class
sncast --account deployer declare \
  --contract-name InferenceVerifier \
  --url https://starknet-sepolia.public.blastapi.io/rpc/v0_7

# Deploy with constructor args: owner, initial_l1_verifier (InferenceBridge address as felt252)
sncast --account deployer deploy \
  --class-hash <CLASS_HASH_FROM_DECLARE> \
  --constructor-calldata <OWNER_ADDRESS> <INFERENCE_BRIDGE_ADDRESS_AS_FELT252> \
  --url https://starknet-sepolia.public.blastapi.io/rpc/v0_7
```

After deploying the Cairo contract, whitelist the bridge:

```bash
sncast --account deployer invoke \
  --contract-address <CAIRO_CONTRACT_ADDRESS> \
  --function add_l1_verifier \
  --calldata <INFERENCE_BRIDGE_ADDRESS_AS_FELT252> \
  --url https://starknet-sepolia.public.blastapi.io/rpc/v0_7
```

### Run Cairo tests

```bash
cd starknet_l2
snforge test
```

---

## Required env vars

```bash
# EVM deploy
RPC_URL=https://eth-sepolia.g.alchemy.com/v2/<key>
PRIVATE_KEY=0x...
OWNER_ADDRESS=0x...
SETTLER_ADDRESS=0x...
HALO2_VERIFIER_ADDRESS=0x7bcf4980868bA06A38AC561904aE6BDEd9Ee46D2
ETHERSCAN_API_KEY=...

# StarkNet bridge (deployInference)
STARKNET_CORE_ADDRESS=0xde29d060D45901Fb19ED6C6e959EB22d8626708e
CAIRO_DEST=<InferenceVerifier.cairo address as uint256>
CAIRO_SELECTOR=<keccak selector for consume_inference_result>
```