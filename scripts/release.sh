#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

REPO_ROOT="${DEFAULT_REPO_ROOT}"
AUTO_CONFIRM="false"
SKIP_BUILD="false"
REQUESTED_VERSION=""

usage() {
  cat <<'EOF'
Uso: ./release.sh [VERSAO] [opções]

Atualiza a versão do aplicativo (Cargo.toml, Cargo.lock e bundle macOS local),
faz commit da versão, cria a tag git v<VERSAO> e envia para o remoto origin.

Argumentos:
  VERSAO               Versão SemVer desejada (ex.: 1.0.0 ou v1.0.0).
                       Se omitida, incrementa +1 no patch da maior versão já existente.

Opções:
  -y, --yes            Confirma o release sem prompt interativo.
      --skip-build     Não executa 'cargo build --release --bins' nem empacotamento macOS.
      --repo-root DIR  Caminho alternativo do repositório Git (útil para testes).
  -h, --help           Exibe esta ajuda.
EOF
}

die() {
  echo "Erro: $*" >&2
  exit 1
}

normalize_semver() {
  local raw="${1#v}"
  if [[ ! "${raw}" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    return 1
  fi
  printf '%s\n' "${raw}"
}

# Retorna via stdout: -1 se a < b, 0 se a == b, 1 se a > b
semver_cmp() {
  local a1 a2 a3 b1 b2 b3
  IFS='.' read -r a1 a2 a3 <<< "${1}"
  IFS='.' read -r b1 b2 b3 <<< "${2}"

  local n_a1=$((10#${a1})) n_a2=$((10#${a2})) n_a3=$((10#${a3}))
  local n_b1=$((10#${b1})) n_b2=$((10#${b2})) n_b3=$((10#${b3}))

  if (( n_a1 != n_b1 )); then
    if (( n_a1 < n_b1 )); then echo "-1"; else echo "1"; fi
    return 0
  fi
  if (( n_a2 != n_b2 )); then
    if (( n_a2 < n_b2 )); then echo "-1"; else echo "1"; fi
    return 0
  fi
  if (( n_a3 != n_b3 )); then
    if (( n_a3 < n_b3 )); then echo "-1"; else echo "1"; fi
    return 0
  fi
  echo "0"
}

increment_patch() {
  local maj min pat
  IFS='.' read -r maj min pat <<< "${1}"
  printf '%s.%s.%s\n' "${maj}" "${min}" "$((10#${pat} + 1))"
}

ACTIVE_TMP_FILE=""

cleanup_tmp() {
  if [[ -n "${ACTIVE_TMP_FILE}" ]]; then
    rm -f -- "${ACTIVE_TMP_FILE}"
  fi
}

trap cleanup_tmp EXIT INT TERM

update_cargo_toml_version() {
  local cargo_toml="${1}"
  local new_version="${2}"
  local tmp_file
  tmp_file="$(mktemp "${cargo_toml}.tmp.XXXXXX")"
  ACTIVE_TMP_FILE="${tmp_file}"

  if ! awk -v ver="${new_version}" '
    BEGIN { in_pkg = 0; updated = 0 }
    /^\[package\][[:space:]]*$/ { in_pkg = 1; print; next }
    /^\[/ { in_pkg = 0; print; next }
    in_pkg && !updated && /^version[[:space:]]*=[[:space:]]*"[^"]*"/ {
      print "version = \"" ver "\""
      updated = 1
      next
    }
    { print }
    END { if (!updated) exit 2 }
  ' "${cargo_toml}" > "${tmp_file}"; then
    rm -f -- "${tmp_file}"
    ACTIVE_TMP_FILE=""
    die "não foi possível atualizar 'version' em '${cargo_toml}'."
  fi

  mv -f -- "${tmp_file}" "${cargo_toml}"
  ACTIVE_TMP_FILE=""
}

update_cargo_lock_version() {
  local cargo_lock="${1}"
  local pkg_name="${2}"
  local new_version="${3}"
  local tmp_file
  tmp_file="$(mktemp "${cargo_lock}.tmp.XXXXXX")"
  ACTIVE_TMP_FILE="${tmp_file}"

  if ! awk -v pkg="${pkg_name}" -v ver="${new_version}" '
    BEGIN { in_pkg = 0; is_target = 0; updated = 0 }
    /^\[\[package\]\][[:space:]]*$/ { in_pkg = 1; is_target = 0; print; next }
    /^\[/ { in_pkg = 0; is_target = 0; print; next }
    in_pkg && $0 == "name = \"" pkg "\"" { is_target = 1; print; next }
    in_pkg && is_target && !updated && /^version[[:space:]]*=[[:space:]]*"[^"]*"/ {
      print "version = \"" ver "\""
      updated = 1
      next
    }
    { print }
    END { if (!updated) exit 2 }
  ' "${cargo_lock}" > "${tmp_file}"; then
    rm -f -- "${tmp_file}"
    ACTIVE_TMP_FILE=""
    die "falha ao atualizar versão de '${pkg_name}' em '${cargo_lock}'."
  fi

  mv -f -- "${tmp_file}" "${cargo_lock}"
  ACTIVE_TMP_FILE=""
}

while [[ $# -gt 0 ]]; do
  case "${1}" in
    -y|--yes)
      AUTO_CONFIRM="true"
      shift
      ;;
    --skip-build)
      SKIP_BUILD="true"
      shift
      ;;
    --repo-root)
      [[ $# -ge 2 ]] || die "opção '--repo-root' exige um diretório."
      REPO_ROOT="${2}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      die "opção desconhecida: '${1}'. Use '--help' para ajuda."
      ;;
    *)
      if [[ -n "${REQUESTED_VERSION}" ]]; then
        die "apenas uma versão pode ser informada (recebido '${REQUESTED_VERSION}' e '${1}')."
      fi
      REQUESTED_VERSION="${1}"
      shift
      ;;
  esac
done

[[ -d "${REPO_ROOT}" ]] || die "diretório do repositório não encontrado: '${REPO_ROOT}'."
REPO_ROOT="$(cd -- "${REPO_ROOT}" && pwd)"

CARGO_TOML="${REPO_ROOT}/Cargo.toml"
CARGO_LOCK="${REPO_ROOT}/Cargo.lock"

[[ -f "${CARGO_TOML}" ]] || die "arquivo não encontrado: '${CARGO_TOML}'."
git -C "${REPO_ROOT}" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
  || die "'${REPO_ROOT}' não é um repositório Git válido."
git -C "${REPO_ROOT}" rev-parse --verify HEAD >/dev/null 2>&1 \
  || die "o repositório '${REPO_ROOT}' ainda não possui commits."
git -C "${REPO_ROOT}" remote get-url origin >/dev/null 2>&1 \
  || die "remoto Git 'origin' não configurado em '${REPO_ROOT}'."

CURRENT_BRANCH="$(git -C "${REPO_ROOT}" symbolic-ref --short -q HEAD || true)"
[[ -n "${CURRENT_BRANCH}" ]] \
  || die "HEAD destacado (detached HEAD) não é permitido para release; selecione um branch antes."

git -C "${REPO_ROOT}" diff --quiet \
  || die "existem alterações não commitadas no working tree; faça commit ou stash antes do release."
git -C "${REPO_ROOT}" diff --cached --quiet \
  || die "existem alterações staged no index; faça commit ou reset antes do release."

PACKAGE_NAME="$(awk -F'"' '
  /^\[package\][[:space:]]*$/ { in_pkg = 1; next }
  /^\[/ { in_pkg = 0 }
  in_pkg && /^name[[:space:]]*=/ { print $2; exit }
' "${CARGO_TOML}")"
[[ -n "${PACKAGE_NAME}" ]] || die "não foi possível ler '[package] name' de '${CARGO_TOML}'."

RAW_CURRENT_VERSION="$(awk -F'"' '
  /^\[package\][[:space:]]*$/ { in_pkg = 1; next }
  /^\[/ { in_pkg = 0 }
  in_pkg && /^version[[:space:]]*=/ { print $2; exit }
' "${CARGO_TOML}")"
CURRENT_VERSION="$(normalize_semver "${RAW_CURRENT_VERSION}")" \
  || die "versão atual em Cargo.toml ('${RAW_CURRENT_VERSION}') não é SemVer válido X.Y.Z."

LOCAL_TAGS_RAW="$(git -C "${REPO_ROOT}" tag -l)"
if ! REMOTE_TAGS_RAW="$(git -C "${REPO_ROOT}" ls-remote --tags --refs -- origin)"; then
  die "não foi possível consultar as tags do remoto 'origin' (verifique conexão/autenticação)."
fi
REMOTE_TAGS_NAMES="$(printf '%s\n' "${REMOTE_TAGS_RAW}" | awk '{ sub(/^refs\/tags\//, "", $2); if ($2 != "") print $2 }')"

MAX_TAG_VERSION=""
while IFS= read -r tag; do
  [[ -n "${tag}" ]] || continue
  if parsed="$(normalize_semver "${tag}")"; then
    if [[ -z "${MAX_TAG_VERSION}" ]] || [[ "$(semver_cmp "${parsed}" "${MAX_TAG_VERSION}")" == "1" ]]; then
      MAX_TAG_VERSION="${parsed}"
    fi
  fi
done < <(printf '%s\n%s\n' "${LOCAL_TAGS_RAW}" "${REMOTE_TAGS_NAMES}")

MAX_KNOWN_VERSION="${CURRENT_VERSION}"
if [[ -n "${MAX_TAG_VERSION}" ]] && [[ "$(semver_cmp "${MAX_TAG_VERSION}" "${MAX_KNOWN_VERSION}")" == "1" ]]; then
  MAX_KNOWN_VERSION="${MAX_TAG_VERSION}"
fi

if [[ -n "${REQUESTED_VERSION}" ]]; then
  TARGET_VERSION="$(normalize_semver "${REQUESTED_VERSION}")" \
    || die "versão inválida '${REQUESTED_VERSION}'. Informe no formato X.Y.Z ou vX.Y.Z (ex.: 1.0.0)."

  if [[ "$(semver_cmp "${TARGET_VERSION}" "${CURRENT_VERSION}")" == "-1" ]]; then
    die "versão '${TARGET_VERSION}' é menor que a versão atual em Cargo.toml ('${CURRENT_VERSION}')."
  fi

  if [[ -n "${MAX_TAG_VERSION}" ]] && [[ "$(semver_cmp "${TARGET_VERSION}" "${MAX_TAG_VERSION}")" != "1" ]]; then
    die "versão 'v${TARGET_VERSION}' deve ser maior que a última release realizada ('v${MAX_TAG_VERSION}')."
  fi

  if git -C "${REPO_ROOT}" rev-parse -q --verify "refs/tags/v${TARGET_VERSION}" >/dev/null 2>&1; then
    die "a tag 'v${TARGET_VERSION}' já existe localmente no repositório."
  fi

  remote_tag_rc=0
  git -C "${REPO_ROOT}" ls-remote --exit-code --tags -- origin "refs/tags/v${TARGET_VERSION}" >/dev/null || remote_tag_rc=$?
  case "${remote_tag_rc}" in
    0)
      die "a tag 'v${TARGET_VERSION}' já existe no remoto 'origin'."
      ;;
    2)
      ;;
    *)
      die "falha ao verificar a tag 'v${TARGET_VERSION}' no remoto 'origin' (código ${remote_tag_rc})."
      ;;
  esac
else
  TARGET_VERSION="$(increment_patch "${MAX_KNOWN_VERSION}")"
fi

TAG_NAME="v${TARGET_VERSION}"

echo "========================================"
echo "  OTP Tools — Preparação de Release"
echo "========================================"
echo "Repositório:          ${REPO_ROOT}"
echo "Branch ativo:         ${CURRENT_BRANCH}"
echo "Versão atual (TOML):  ${CURRENT_VERSION}"
if [[ -n "${MAX_TAG_VERSION}" ]]; then
  echo "Última tag lançada:   v${MAX_TAG_VERSION}"
else
  echo "Última tag lançada:   (nenhuma)"
fi
echo "Nova versão release:  ${TARGET_VERSION} (tag ${TAG_NAME})"
echo "----------------------------------------"

if [[ "${AUTO_CONFIRM}" != "true" ]]; then
  printf 'Deseja realmente fazer o release da versão %s? [s/N]: ' "${TAG_NAME}"
  CONFIRM=""
  IFS= read -r CONFIRM || true
  CONFIRM_LOWER="$(printf '%s' "${CONFIRM}" | tr '[:upper:]' '[:lower:]')"
  case "${CONFIRM_LOWER}" in
    s|sim|y|yes)
      ;;
    *)
      echo "Release cancelado pelo usuário." >&2
      exit 1
      ;;
  esac
fi

update_cargo_toml_version "${CARGO_TOML}" "${TARGET_VERSION}"

FILES_TO_STAGE=("Cargo.toml")
if [[ -f "${CARGO_LOCK}" ]]; then
  update_cargo_lock_version "${CARGO_LOCK}" "${PACKAGE_NAME}" "${TARGET_VERSION}"
  FILES_TO_STAGE+=("Cargo.lock")
fi

if [[ "${SKIP_BUILD}" != "true" ]]; then
  echo "Compilando binários release com a versão ${TARGET_VERSION}..."
  if [[ -f "${CARGO_LOCK}" ]]; then
    cargo build --manifest-path "${CARGO_TOML}" --locked --release --bins
  else
    cargo build --manifest-path "${CARGO_TOML}" --release --bins
  fi

  if [[ "$(uname -s)" == "Darwin" ]] && [[ -x "${REPO_ROOT}/scripts/package-macos-app.sh" ]]; then
    "${REPO_ROOT}/scripts/package-macos-app.sh" \
      "${REPO_ROOT}/target/release/otp-tools-gui" \
      "${REPO_ROOT}/dist" \
      "${TARGET_VERSION}"
  fi
fi

if ! git -C "${REPO_ROOT}" diff --quiet HEAD -- "${FILES_TO_STAGE[@]}"; then
  git -C "${REPO_ROOT}" add -- "${FILES_TO_STAGE[@]}"
  git -C "${REPO_ROOT}" commit --only -m "chore(release): ${TAG_NAME}" -- "${FILES_TO_STAGE[@]}"
fi

git -C "${REPO_ROOT}" push origin -- "${CURRENT_BRANCH}"
git -C "${REPO_ROOT}" tag -- "${TAG_NAME}"
git -C "${REPO_ROOT}" push origin -- "refs/tags/${TAG_NAME}"

echo "Release ${TAG_NAME} publicado com sucesso!"
