#!/usr/bin/env bash

set -euo pipefail

NETWORK="testnet"
ISSUER_ALIAS="issuer"

usage() {
  cat <<'EOF'
Create a Stellar issuer account and set issuer flags.

Usage:
  ./scripts/chains/stellar/create-issuer.sh [options]

Options:
  --network <testnet|mainnet>   Target network (default: testnet)
  --issuer <alias>              Local key alias (default: issuer)
  --help                        Show this help

Notes:
  - Testnet: auto-funds issuer via Friendbot.
  - Mainnet: account must already be funded with XLM before flags can be set.
  - Sets issuer flags: authorization-required + revocable + clawback-enabled.
EOF
}

log() {
  printf '[create-issuer] %s\n' "$*" >&2
}

fail() {
  printf '[create-issuer] ERROR: %s\n' "$*" >&2
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
      --issuer)
        ISSUER_ALIAS="${2:-}"
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

validate_network() {
  case "$NETWORK" in
    testnet|mainnet) ;;
    *) fail "invalid --network '$NETWORK' (expected testnet|mainnet)" ;;
  esac
}

ensure_issuer_key() {
  if stellar keys public-key "$ISSUER_ALIAS" >/dev/null 2>&1; then
    log "using existing key alias: $ISSUER_ALIAS"
  else
    log "generating key alias: $ISSUER_ALIAS"
    stellar keys generate "$ISSUER_ALIAS" >/dev/null
  fi
}

fund_testnet_if_needed() {
  if [[ "$NETWORK" != "testnet" ]]; then
    return
  fi

  log "funding issuer on testnet via Friendbot"
  if ! stellar keys fund "$ISSUER_ALIAS" >/dev/null 2>&1; then
    log "friendbot funding skipped (account likely already funded)"
  fi
}

set_issuer_flags() {
  log "setting issuer flags: authorization-required + revocable + clawback-enabled"
  local out
  if ! out="$(stellar tx new set-options --source-account "$ISSUER_ALIAS" --set-required --set-revocable --set-clawback-enabled)"; then
    if [[ "$NETWORK" == "mainnet" ]]; then
      fail "failed to set flags on mainnet. Fund issuer with XLM first, then rerun."
    fi
    fail "failed to set issuer flags"
  fi
}

main() {
  parse_args "$@"
  validate_network

  require_cmd stellar
  stellar network use "$NETWORK" >/dev/null

  ensure_issuer_key
  fund_testnet_if_needed

  local issuer_address
  issuer_address="$(stellar keys public-key "$ISSUER_ALIAS")"

  stellar keys use "$ISSUER_ALIAS" >/dev/null
  set_issuer_flags

  printf 'STELLAR_NETWORK=%s\n' "$NETWORK"
  printf 'STELLAR_ISSUER_ALIAS=%s\n' "$ISSUER_ALIAS"
  printf 'STELLAR_ISSUER_ADDRESS=%s\n' "$issuer_address"
  printf 'STELLAR_ISSUER_FLAGS=required,revocable,clawback-enabled\n'

  if [[ "$NETWORK" == "mainnet" ]]; then
    printf 'NOTE=Mainnet issuer must remain funded for future transactions and fees.\n'
  fi
}

main "$@"
