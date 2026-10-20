#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
CONTRACT_MANIFEST="$ROOT_DIR/contracts/ylds-controller/Cargo.toml"
DEFAULT_WASM="$ROOT_DIR/target/wasm32v1-none/release/ylds_controller.wasm"

NETWORK="testnet"
OWNER_ALIAS=""
CONTRACT_ALIAS=""
WASM_PATH=""
SKIP_BUILD="false"
INCLUSION_FEE=""
INSTRUCTION_LEEWAY=""

usage() {
  cat <<'EOF'
Deploy the YLDS Stellar controller contract.

Usage:
  ./scripts/chains/stellar/deploy-controller.sh [options]

Options:
  --network <testnet|mainnet>   Target network (default: testnet)
  --owner <alias>               Local key alias for controller owner (required)
  --alias <contract_alias>      Optional local alias for deployed controller
  --wasm <path>                 Use prebuilt wasm path instead of default output
  --skip-build                  Skip cargo build step
  --inclusion-fee <stroops>     Optional max inclusion fee for deploy tx
  --instruction-leeway <count>  Optional instruction buffer for deploy simulation
  --help                        Show this help

Notes:
  - Script predeploys the YLDS SAC before deploying the controller.
  - Owner is the initial controller owner and asset issuer used during deployment.
  - Post-deploy SAC admin is the controller.
EOF
}

log() {
  printf '[stellar-controller-deploy] %s\n' "$*" >&2
}

fail() {
  printf '[stellar-controller-deploy] ERROR: %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --network)
        NETWORK="${2:-}"
        shift 2
        ;;
      --owner)
        OWNER_ALIAS="${2:-}"
        shift 2
        ;;
      --alias)
        CONTRACT_ALIAS="${2:-}"
        shift 2
        ;;
      --wasm)
        WASM_PATH="${2:-}"
        shift 2
        ;;
      --skip-build)
        SKIP_BUILD="true"
        shift
        ;;
      --inclusion-fee)
        INCLUSION_FEE="${2:-}"
        shift 2
        ;;
      --instruction-leeway)
        INSTRUCTION_LEEWAY="${2:-}"
        shift 2
        ;;
      --help|-h)
        usage
        exit 0
        ;;
      *)
        fail "unknown option: $1"
        ;;
    esac
  done
}

validate_inputs() {
  case "$NETWORK" in
    testnet|mainnet) ;;
    *) fail "invalid --network '$NETWORK' (expected testnet|mainnet)" ;;
  esac

  [[ -n "$OWNER_ALIAS" ]] || fail "missing --owner"

  if [[ -n "$INCLUSION_FEE" ]] && ! [[ "$INCLUSION_FEE" =~ ^[0-9]+$ ]]; then
    fail "invalid --inclusion-fee '$INCLUSION_FEE' (expected integer stroops)"
  fi

  if [[ -n "$INSTRUCTION_LEEWAY" ]] && ! [[ "$INSTRUCTION_LEEWAY" =~ ^[0-9]+$ ]]; then
    fail "invalid --instruction-leeway '$INSTRUCTION_LEEWAY' (expected integer)"
  fi
}

require_owner_key() {
  stellar keys public-key "$OWNER_ALIAS" >/dev/null 2>&1 || fail "owner alias not found: $OWNER_ALIAS"
}

resolve_wasm_path() {
  if [[ -z "$WASM_PATH" ]]; then
    WASM_PATH="$DEFAULT_WASM"
  fi

  [[ -f "$WASM_PATH" ]] || fail "wasm not found at $WASM_PATH"
}

build_contract() {
	log "building stellar controller wasm"
	stellar contract build --manifest-path "$CONTRACT_MANIFEST"
}

extract_contract_id() {
  local raw="$1"
  local cid
  cid="$(printf '%s\n' "$raw" | grep -Eo 'C[A-Z2-7]{55}' | tail -n 1 || true)"
  [[ -n "$cid" ]] || fail "unable to parse contract id"
  printf '%s' "$cid"
}

extract_address() {
  local raw="$1"
  local address
  address="$(printf '%s\n' "$raw" | grep -Eo '[GC][A-Z2-7]{55}' | tail -n 1 || true)"
  [[ -n "$address" ]] || return 1
  printf '%s' "$address"
}

extract_tx_hash() {
  local raw="$1"
  local tx_hash
  tx_hash="$(printf '%s\n' "$raw" | grep -Eo '[a-f0-9]{64}' | tail -n 1 || true)"
  printf '%s' "$tx_hash"
}

resolve_asset_contract_id() {
  local asset="$1"
  local out

  out="$(stellar contract id asset --asset "$asset" 2>&1)" || fail "unable to resolve SAC id for ${asset}: $out"
  extract_contract_id "$out"
}

verify_sac_admin() {
  local sac_id="$1"
  local owner_address="$2"
  local out
  local admin

  out="$(
    stellar contract invoke \
      --id "$sac_id" \
      --source "$OWNER_ALIAS" \
      --send no \
      -- \
      admin 2>&1
  )" || {
    printf 'unable to verify SAC admin for %s: %s' "$sac_id" "$out" >&2
    return 1
  }

  admin="$(extract_address "$out")" || {
    printf 'unable to parse SAC admin for %s from output: %s' "$sac_id" "$out" >&2
    return 1
  }

  if [[ "$admin" != "$owner_address" ]]; then
    printf 'SAC %s admin is %s, expected %s' "$sac_id" "$admin" "$owner_address" >&2
    return 1
  fi
}

predeploy_sac() {
  local owner_address="$1"
  local asset="YLDS:${owner_address}"
  local expected_sac_id
  local deployed_sac_id
  local out
  local tx_hash
  local -a cmd

  expected_sac_id="$(resolve_asset_contract_id "$asset")"

  cmd=(
    stellar contract asset deploy
    --asset "$asset"
    --source-account "$OWNER_ALIAS"
  )

  if [[ -n "$INCLUSION_FEE" ]]; then
    cmd+=(--inclusion-fee "$INCLUSION_FEE")
  fi

  if [[ -n "$INSTRUCTION_LEEWAY" ]]; then
    cmd+=(--instruction-leeway "$INSTRUCTION_LEEWAY")
  fi

  log "predeploying YLDS SAC for ${asset}"
  out="$("${cmd[@]}" 2>&1)" || {
    if printf '%s\n' "$out" | grep -qi "transaction submission timeout"; then
      tx_hash="$(extract_tx_hash "$out")"
      if [[ -n "$tx_hash" ]]; then
        fail "SAC predeploy timed out after submission (tx hash: $tx_hash). Confirm tx status on your RPC/explorer before rerunning."
      fi
      fail "SAC predeploy timed out after submission. Confirm tx status on your RPC/explorer before rerunning."
    fi

    log "SAC predeploy did not complete for ${asset}; verifying existing SAC ${expected_sac_id}"
    local verify_error
    verify_error="$(verify_sac_admin "$expected_sac_id" "$owner_address" 2>&1)" || fail "SAC predeploy failed for ${asset}: $out. Existing SAC verification also failed: $verify_error"
    printf '%s' "$expected_sac_id"
    return
  }

  deployed_sac_id="$(extract_contract_id "$out")"
  [[ "$deployed_sac_id" == "$expected_sac_id" ]] || fail "deployed SAC $deployed_sac_id does not match expected SAC $expected_sac_id"
  local verify_error
  verify_error="$(verify_sac_admin "$expected_sac_id" "$owner_address" 2>&1)" || fail "$verify_error"
  printf '%s' "$expected_sac_id"
}

deploy_controller() {
  local owner_address="$1"
  local out
  local tx_hash
  local -a cmd

  cmd=(
    stellar contract deploy
    --source "$OWNER_ALIAS"
    --wasm "$WASM_PATH"
  )

  if [[ -n "$INCLUSION_FEE" ]]; then
    cmd+=(--inclusion-fee "$INCLUSION_FEE")
  fi

  if [[ -n "$INSTRUCTION_LEEWAY" ]]; then
    cmd+=(--instruction-leeway "$INSTRUCTION_LEEWAY")
  fi

  if [[ -n "$CONTRACT_ALIAS" ]]; then
    log "deploying controller with alias ${CONTRACT_ALIAS}"
    cmd+=(--alias "$CONTRACT_ALIAS")
  else
    log "deploying controller"
  fi

  out="$("${cmd[@]}" -- --owner "$owner_address" 2>&1)" || fail "controller deploy failed: $out"

  if printf '%s\n' "$out" | grep -qi "transaction submission timeout"; then
    tx_hash="$(extract_tx_hash "$out")"
    if [[ -n "$tx_hash" ]]; then
      fail "controller deploy timed out after submission (tx hash: $tx_hash). Confirm tx status on your RPC/explorer and rerun; use higher inclusion fee if needed (e.g. STELLAR_INCLUSION_FEE=100000)."
    fi
    fail "controller deploy timed out after submission. Confirm tx status on your RPC/explorer and rerun; use higher inclusion fee if needed (e.g. STELLAR_INCLUSION_FEE=100000)."
  fi

  extract_contract_id "$out"
}

resolve_sac_id() {
  local controller_id="$1"
  local out

  log "resolving SAC via controller get_sac"
  out="$(
    stellar contract invoke \
      --id "$controller_id" \
      --source "$OWNER_ALIAS" \
      -- \
      get_sac 2>&1
  )" || fail "get_sac invoke failed: $out"

  extract_contract_id "$out"
}

main() {
  parse_args "$@"
  validate_inputs

  require_cmd stellar
  stellar network use "$NETWORK" >/dev/null
  require_owner_key

  if [[ "$SKIP_BUILD" != "true" ]]; then
    require_cmd cargo
    build_contract
  fi

  resolve_wasm_path

  local owner_address
  owner_address="$(stellar keys public-key "$OWNER_ALIAS")"

  local predeployed_sac_id
  predeployed_sac_id="$(predeploy_sac "$owner_address")"

  local controller_id
  controller_id="$(deploy_controller "$owner_address")"

  local sac_id
  sac_id="$(resolve_sac_id "$controller_id")"
  [[ "$sac_id" == "$predeployed_sac_id" ]] || fail "controller SAC $sac_id does not match predeployed SAC $predeployed_sac_id"

  printf 'STELLAR_NETWORK=%s\n' "$NETWORK"
  printf 'STELLAR_OWNER_ALIAS=%s\n' "$OWNER_ALIAS"
  printf 'STELLAR_OWNER_ADDRESS=%s\n' "$owner_address"
  if [[ -n "$CONTRACT_ALIAS" ]]; then
    printf 'STELLAR_CONTROLLER_ALIAS=%s\n' "$CONTRACT_ALIAS"
  fi
  printf 'STELLAR_CONTROLLER_CONTRACT_ID=%s\n' "$controller_id"
  printf 'STELLAR_SAC_CONTRACT_ID=%s\n' "$sac_id"
}

main "$@"
