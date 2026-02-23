#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="agent-box"
DIST_DIR="${ROOT_DIR}/dist"

usage() {
  cat <<'EOF'
Usage:
  ./dev.sh build
  ./dev.sh pack
  ./dev.sh run [-- <agent-box args...>]
  ./dev.sh all [-- <agent-box args...>]

Commands:
  build   Build release binary
  pack    Build and package release binary into ./dist/
  run     Run the app with cargo (fast dev loop)
  all     build + pack + run
EOF
}

build_release() {
  echo "==> Building release binary"
  cargo build --release
}

pack_release() {
  echo "==> Packing release artifact"
  build_release
  mkdir -p "${DIST_DIR}"

  local os arch stamp artifact_name artifact_path
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  stamp="$(date +%Y%m%d-%H%M%S)"
  artifact_name="${BIN_NAME}-${os}-${arch}-${stamp}.tar.gz"
  artifact_path="${DIST_DIR}/${artifact_name}"

  tar -czf "${artifact_path}" \
    -C "${ROOT_DIR}/target/release" "${BIN_NAME}" \
    -C "${ROOT_DIR}" README.MD

  echo "Created: ${artifact_path}"
}

run_dev() {
  local -a run_args=("$@")
  if [[ ${#run_args[@]} -gt 0 && "${run_args[0]}" == "--" ]]; then
    run_args=("${run_args[@]:1}")
  fi

  echo "==> Running ${BIN_NAME}"
  cargo run -- "${run_args[@]}"
}

main() {
  if [[ $# -lt 1 ]]; then
    usage
    exit 1
  fi

  local cmd="$1"
  shift || true

  case "${cmd}" in
    build)
      build_release
      ;;
    pack)
      pack_release
      ;;
    run)
      run_dev "$@"
      ;;
    all)
      build_release
      pack_release
      run_dev "$@"
      ;;
    -h|--help|help)
      usage
      ;;
    *)
      echo "Unknown command: ${cmd}" >&2
      usage
      exit 1
      ;;
  esac
}

main "$@"

