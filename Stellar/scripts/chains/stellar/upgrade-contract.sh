#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
CONTRACT_MANIFEST="$ROOT_DIR/contracts/ylds-controller/Cargo.toml"
DEFAULT_WASM="$ROOT_DIR/target/wasm32v1-none/release/ylds_controller.wasm"

NETWORK="testnet"
RPC_URL=""
NETWORK_PASSPHRASE=""
OWNER_ALIAS=""
CONTRACT_ID="${STELLAR_CONTROLLER_CONTRACT_ID:-}"
WASM_PATH=""
SKIP_BUILD="false"

usage() {
  cat <<'EOF'
Upgrade deployed YLDS Stellar controller contract.

Usage:
  ./scripts/chains/stellar/upgrade-contract.sh [options]

Options:
  --network <testnet|futurenet|mainnet>   Target network (default: testnet)
  --rpc-url <url>                         Soroban RPC URL (overrides network default)
  --network-passphrase <text>             Network passphrase (overrides network default)
  --owner <alias>                         Local key alias for controller owner (required)
  --contract-id <id>                      Contract id to upgrade (or STELLAR_CONTROLLER_CONTRACT_ID)
  --wasm <path>                           Use prebuilt wasm path instead of default output
  --skip-build                            Skip cargo build step
  --help                                  Show this help

Output:
  Prints env-style key/value lines describing the upgrade.
EOF
}

log() {
  printf '[stellar-upgrade] %s\n' "$*" >&2
}

fail() {
  printf '[stellar-upgrade] ERROR: %s\n' "$*" >&2
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
      --rpc-url)
        RPC_URL="${2:-}"
        shift 2
        ;;
      --network-passphrase)
        NETWORK_PASSPHRASE="${2:-}"
        shift 2
        ;;
      --owner)
        OWNER_ALIAS="${2:-}"
        shift 2
        ;;
      --contract-id)
        CONTRACT_ID="${2:-}"
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

set_network_defaults() {
  case "$NETWORK" in
    testnet)
      [[ -n "$RPC_URL" ]] || RPC_URL="https://soroban-testnet.stellar.org"
      [[ -n "$NETWORK_PASSPHRASE" ]] || NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
      ;;
    futurenet)
      [[ -n "$RPC_URL" ]] || RPC_URL="https://rpc-futurenet.stellar.org"
      [[ -n "$NETWORK_PASSPHRASE" ]] || NETWORK_PASSPHRASE="Test SDF Future Network ; October 2022"
      ;;
    mainnet)
      [[ -n "$RPC_URL" ]] || RPC_URL="https://mainnet.sorobanrpc.com"
      [[ -n "$NETWORK_PASSPHRASE" ]] || NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
      ;;
    *)
      fail "invalid --network '$NETWORK' (expected testnet|futurenet|mainnet)"
      ;;
  esac
}

validate_inputs() {
  [[ -n "$OWNER_ALIAS" ]] || fail "missing --owner"
  [[ -n "$CONTRACT_ID" ]] || fail "missing contract id (set --contract-id or STELLAR_CONTROLLER_CONTRACT_ID)"
}

require_owner_key() {
  stellar keys public-key "$OWNER_ALIAS" >/dev/null 2>&1 || fail "owner alias not found: $OWNER_ALIAS"
}

build_contract() {
	log "building stellar contract wasm"
	stellar contract build --manifest-path "$CONTRACT_MANIFEST"
}

resolve_wasm_path() {
  if [[ -z "$WASM_PATH" ]]; then
    WASM_PATH="$DEFAULT_WASM"
  fi

  [[ -f "$WASM_PATH" ]] || fail "wasm not found at $WASM_PATH"
}

extract_wasm_hash() {
  local raw="$1"
  local hash
  hash="$(printf '%s\n' "$raw" | grep -Eo '[a-f0-9]{64}' | tail -n 1 || true)"
  [[ -n "$hash" ]] || fail "unable to parse wasm hash from upload output"
  printf '%s' "$hash"
}

upload_wasm() {
  log "uploading new contract wasm"

  local out
  out="$(
    stellar contract upload \
      --quiet \
      --wasm "$WASM_PATH" \
      --source "$OWNER_ALIAS" \
      --rpc-url "$RPC_URL" \
      --network-passphrase "$NETWORK_PASSPHRASE"
  )"

  extract_wasm_hash "$out"
}

invoke_upgrade() {
  local wasm_hash="$1"
  log "invoking upgrade on contract"

  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$OWNER_ALIAS" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    upgrade \
    --new_wasm_hash "$wasm_hash" >/dev/null
}

validate_contract() {
  log "validating upgraded contract"
  stellar contract invoke \
    --id "$CONTRACT_ID" \
    --source "$OWNER_ALIAS" \
    --rpc-url "$RPC_URL" \
    --network-passphrase "$NETWORK_PASSPHRASE" \
    -- \
    version >/dev/null
}

main() {
  parse_args "$@"
  set_network_defaults
  validate_inputs

  require_cmd stellar
  require_owner_key
  if [[ "$SKIP_BUILD" != "true" ]]; then
    require_cmd cargo
    build_contract
  fi

  resolve_wasm_path

  local wasm_hash
  wasm_hash="$(upload_wasm)"
  invoke_upgrade "$wasm_hash"
  validate_contract

  printf 'STELLAR_NETWORK=%s\n' "$NETWORK"
  printf 'STELLAR_RPC=%s\n' "$RPC_URL"
  printf 'STELLAR_NETWORK_PASSPHRASE=%s\n' "$NETWORK_PASSPHRASE"
  printf 'STELLAR_CONTROLLER_CONTRACT_ID=%s\n' "$CONTRACT_ID"
  printf 'STELLAR_UPGRADE_WASM=%s\n' "$WASM_PATH"
  printf 'STELLAR_UPGRADE_WASM_HASH=%s\n' "$wasm_hash"
}

main "$@"
