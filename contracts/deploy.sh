#!/usr/bin/env bash
# deploy.sh — Deploy Elenxis contracts (Halo2Verifier + InferenceVerifier)
#
# Usage:
#   ./deploy.sh [sepolia|base-sepolia|base-mainnet]
#   DRY_RUN=true ./deploy.sh base-sepolia
#
# Required .env vars:
#   PRIVATE_KEY
#   OWNER_ADDRESS
#   SETTLER_ADDRESS
#   ETHERSCAN_API_KEY   (only for sepolia)
#   BASESCAN_API_KEY    (only for base-sepolia / base-mainnet)
# ------------------------------------------------------------------

set -eo pipefail  # removed -u to allow optional env vars

source .env

NETWORK=${1:-base-sepolia}
DRY_RUN=${DRY_RUN:-false}

# Optional vars with safe defaults
PRIVATE_KEY=${PRIVATE_KEY:-}
OWNER_ADDRESS=${OWNER_ADDRESS:-}
SETTLER_ADDRESS=${SETTLER_ADDRESS:-}
HALO2_VERIFIER_ADDRESS=${HALO2_VERIFIER_ADDRESS:-}
ETHERSCAN_API_KEY=${ETHERSCAN_API_KEY:-}
BASESCAN_API_KEY=${BASESCAN_API_KEY:-}

# Validate required vars
if [ -z "$PRIVATE_KEY" ]; then
  echo "❌ PRIVATE_KEY is not set in .env"
  exit 1
fi
if [ -z "$OWNER_ADDRESS" ]; then
  echo "❌ OWNER_ADDRESS is not set in .env"
  exit 1
fi
if [ -z "$SETTLER_ADDRESS" ]; then
  echo "❌ SETTLER_ADDRESS is not set in .env"
  exit 1
fi

case $NETWORK in
  sepolia)
    RPC="https://ethereum-sepolia-rpc.publicnode.com"
    CHAIN_ID=11155111
    VERIFIER_URL="https://api-sepolia.etherscan.io/api"
    API_KEY="$ETHERSCAN_API_KEY"
    EXPLORER="https://sepolia.etherscan.io"
    ;;
  base-sepolia)
    RPC="https://sepolia.base.org"
    CHAIN_ID=84532
    VERIFIER_URL="https://api-sepolia.basescan.org/api"
    API_KEY="$BASESCAN_API_KEY"
    EXPLORER="https://sepolia.basescan.org"
    ;;
  base-mainnet)
    RPC="https://mainnet.base.org"
    CHAIN_ID=8453
    VERIFIER_URL="https://api.basescan.org/api"
    API_KEY="$BASESCAN_API_KEY"
    EXPLORER="https://basescan.org"
    ;;
  *)
    echo "❌ Unknown network: $NETWORK"
    echo "Usage: ./deploy.sh [sepolia|base-sepolia|base-mainnet]"
    exit 1
    ;;
esac

echo "=========================================="
echo " Elenxis — Contract Deployment"
echo " Network : $NETWORK (chain $CHAIN_ID)"
echo " RPC     : $RPC"
echo " Explorer: $EXPLORER"
echo " Owner   : $OWNER_ADDRESS"
echo " Settler : $SETTLER_ADDRESS"
echo " Mode    : $([ "$DRY_RUN" = true ] && echo 'DRY RUN (no broadcast)' || echo 'LIVE')"
echo "=========================================="

# ── Step 1: Deploy Halo2Verifier ─────────────────────────────────────────────
echo ""
echo "▶ Step 1/2 — Deploying Halo2Verifier (Verifier.sol)..."

if [ "$DRY_RUN" = true ]; then
  echo "⚠️  Dry run — would run:"
  echo "   forge create src/Verifier.sol:Halo2Verifier \\"
  echo "     --rpc-url $RPC \\"
  echo "     --chain-id $CHAIN_ID \\"
  echo "     --private-key \$PRIVATE_KEY \\"
  echo "     --verify --etherscan-api-key \$API_KEY"
else
  HALO2_OUTPUT=$(forge create src/Verifier.sol:Halo2Verifier \
    --rpc-url "$RPC" \
    --private-key "$PRIVATE_KEY" \
    --chain-id "$CHAIN_ID" \
    --verify \
    --verifier-url "$VERIFIER_URL" \
    --etherscan-api-key "$API_KEY" \
    -vvv \
    2>&1)

  echo "$HALO2_OUTPUT"

  HALO2_ADDRESS=$(echo "$HALO2_OUTPUT" | grep "Deployed to:" | awk '{print $3}')

  if [ -z "$HALO2_ADDRESS" ]; then
    echo "❌ Failed to extract Halo2Verifier address"
    exit 1
  fi

  echo "✅ Halo2Verifier: $HALO2_ADDRESS"
  echo "   $EXPLORER/address/$HALO2_ADDRESS"

  export HALO2_VERIFIER_ADDRESS="$HALO2_ADDRESS"
fi

# ── Step 2: Deploy InferenceVerifier ─────────────────────────────────────────
echo ""
echo "▶ Step 2/2 — Deploying InferenceVerifier..."
echo "   Halo2Verifier : ${HALO2_VERIFIER_ADDRESS:-<set HALO2_VERIFIER_ADDRESS>}"
echo "   Owner         : $OWNER_ADDRESS"
echo "   Settler       : $SETTLER_ADDRESS"

if [ "$DRY_RUN" = true ]; then
  echo "⚠️  Dry run — would run:"
  echo "   forge script script/Deploy.s.sol:Deploy \\"
  echo "     --rpc-url $RPC \\"
  echo "     --chain-id $CHAIN_ID \\"
  echo "     --broadcast --verify"
  echo ""
  echo "   With env:"
  echo "   HALO2_VERIFIER_ADDRESS=${HALO2_VERIFIER_ADDRESS:-<not set>}"
  echo "   OWNER_ADDRESS=$OWNER_ADDRESS"
  echo "   SETTLER_ADDRESS=$SETTLER_ADDRESS"
else
  forge script script/Deploy.s.sol:Deploy \
    --rpc-url "$RPC" \
    --private-key "$PRIVATE_KEY" \
    --chain-id "$CHAIN_ID" \
    --broadcast \
    --verify \
    --verifier-url "$VERIFIER_URL" \
    --etherscan-api-key "$API_KEY" \
    -vvvv

  echo ""
  echo "✅ InferenceVerifier deployed!"
  echo "📄 Broadcast log: broadcast/Deploy.s.sol/$CHAIN_ID/run-latest.json"
  echo ""
  echo "Update .env with:"
  echo "  SETTLER_RPC_URL=$RPC"
  echo "  INFERENCE_VERIFIER_ADDRESS=<address from broadcast log above>"
fi