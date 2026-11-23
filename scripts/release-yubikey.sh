#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
plugin_slot="${ANVIL_PLUGIN_PIV_SLOT:-9c}"
checksum_slot="${ANVIL_CHECKSUM_PIV_SLOT:-9d}"
version="$(python3 - "$root/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as file:
    print(tomllib.load(file)["workspace"]["package"]["version"])
PY
)"
tag="v$version"
out="$root/dist/anvil-$version"
verify_only=false
inputs=""
expected_fingerprint=""
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "usage: $0 [--out DIR] [--inputs DIR] [--expected-checksum-key-sha256 HEX] [--verify-only DIR]" >&2
    exit 0
fi
while [[ $# -gt 0 ]]; do
    case "$1" in
        --out)
            [[ $# -ge 2 ]] || { echo "--out needs a directory" >&2; exit 2; }
            out="$2"
            shift 2
            ;;
        --verify-only)
            [[ $# -ge 2 ]] || { echo "--verify-only needs a directory" >&2; exit 2; }
            out="$2"
            verify_only=true
            shift 2
            ;;
        --inputs)
            [[ $# -ge 2 ]] || { echo "--inputs needs a directory" >&2; exit 2; }
            inputs="$2"
            shift 2
            ;;
        --expected-checksum-key-sha256)
            [[ $# -ge 2 ]] || { echo "--expected-checksum-key-sha256 needs a value" >&2; exit 2; }
            expected_fingerprint="${2,,}"
            shift 2
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

for tool in openssl sha256sum tar syft yubico-piv-tool; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "missing required tool: $tool" >&2
        exit 2
    }
done

verify_layout() {
    local layout="$1"
    [[ -x "$layout/bin/anvil" ]] || { echo "missing layout binary: $layout/bin/anvil" >&2; exit 2; }
    [[ -f "$layout/share/anvil/packages/official.pub" ]] || {
        echo "missing plugin public key ID" >&2
        exit 2
    }
}

verify_artifacts() {
    local dir="$1"
    local tarball="$dir/anvil-$version-linux-x86_64.tar.gz"
    local sbom="$dir/anvil-$version.sbom.spdx.json"
    local sums="$dir/SHA256SUMS"
    local sig="$dir/SHA256SUMS.sig"
    local public_key="$dir/release.pub.pem"
    for file in "$tarball" "$sbom" "$sums" "$sig" "$public_key"; do
        [[ -f "$file" ]] || { echo "missing release artifact: $file" >&2; exit 2; }
    done
    (
        cd "$dir"
        sha256sum -c SHA256SUMS
        openssl pkeyutl -verify -pubin -inkey release.pub.pem -rawin \
            -in SHA256SUMS -sigfile SHA256SUMS.sig
    )
    local plugin_id checksum_id
    plugin_id="$(tr -d '\n' < "$dir/layout/share/anvil/packages/official.pub")"
    checksum_id="$(openssl pkey -pubin -in "$public_key" -outform DER \
        | tail -c 32 | od -An -v -tx1 | tr -d ' \n')"
    [[ "$plugin_id" != "$checksum_id" ]] || {
        echo "plugin and checksum keys must differ" >&2
        exit 2
    }
    if [[ -n "$expected_fingerprint" ]]; then
        local actual_fingerprint
        actual_fingerprint="$(openssl pkey -pubin -in "$public_key" -outform DER \
            | sha256sum | awk '{print $1}')"
        [[ "$actual_fingerprint" == "$expected_fingerprint" ]] || {
            echo "checksum key fingerprint mismatch: $actual_fingerprint" >&2
            exit 2
        }
    fi
    echo "plugin key:   sha256:$plugin_id"
    echo "checksum key: sha256:$checksum_id"
    echo "verified: $dir"
}

if [[ "$verify_only" == true ]]; then
    verify_layout "$out/layout"
    verify_artifacts "$out"
    exit 0
fi

[[ -z "$(git -C "$root" status --porcelain --untracked-files=no)" ]] || {
    echo "tracked worktree changes must be committed before release signing" >&2
    exit 2
}
git -C "$root" rev-parse -q --verify "refs/tags/$tag" >/dev/null || {
    echo "missing local tag: $tag" >&2
    exit 2
}
[[ "$(git -C "$root" rev-parse "$tag^{}")" == "$(git -C "$root" rev-parse HEAD)" ]] || {
    echo "$tag does not point at HEAD" >&2
    exit 2
}

rm -rf "$out"
mkdir -p "$out"
if [[ -n "$inputs" ]]; then
    [[ -d "$inputs" ]] || { echo "inputs directory missing: $inputs" >&2; exit 2; }
    input_archive="$inputs/anvil-$version-linux-x86_64-inputs.tar.gz"
    input_sums="$inputs/anvil-$version-inputs.sha256"
    for file in "$input_archive" "$input_sums"; do
        [[ -f "$file" ]] || { echo "missing build input: $file" >&2; exit 2; }
    done
    (cd "$inputs" && sha256sum -c "anvil-$version-inputs.sha256")
    mkdir -p "$out/layout"
    tar -xzf "$input_archive" -C "$out/layout"
    echo "re-signing CI layout for $tag in $out/layout"
    for plugin in "$out/layout/share/anvil/official/plugins"/*; do
        [[ -d "$plugin" ]] || continue
        "$root/scripts/package-official-plugin.sh" \
            "$plugin" \
            "piv:$plugin_slot" \
            "$out/layout/share/anvil/packages"
    done
    verify_layout "$out/layout"
    cp "$input_sums" "$out/anvil-$version-inputs.sha256"
else
    echo "building YubiKey-signed layout for $tag in $out/layout"
    ANVIL_PLUGIN_SIGNING_KEY="piv:$plugin_slot" \
        "$root/scripts/release-layout.sh" "$out/layout"
fi
"$root/scripts/release-lifecycle-smoke.sh" "$out/layout"
syft "$out/layout" -o "spdx-json=$out/anvil-$version.sbom.spdx.json"
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
    -czf "$out/anvil-$version-linux-x86_64.tar.gz" -C "$out/layout" .
yubico-piv-tool -a read-public-key -s "$checksum_slot" -o "$out/release.pub.pem"
(
    cd "$out"
    sums=(
        "anvil-$version-linux-x86_64.tar.gz"
        "anvil-$version.sbom.spdx.json"
        release.pub.pem
    )
    if [[ -f "anvil-$version-inputs.sha256" ]]; then
        sums+=("anvil-$version-inputs.sha256")
    fi
    sha256sum "${sums[@]}" > SHA256SUMS
)
yubico-piv-tool -a verify-pin --sign -s "$checksum_slot" -A ED25519 \
    -i "$out/SHA256SUMS" -o "$out/SHA256SUMS.sig"

verify_artifacts "$out"
