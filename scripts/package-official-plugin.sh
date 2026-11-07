#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <plugin-dir> <ed25519-private-key.pem> <output-dir>" >&2
  exit 2
fi

plugin_dir="$1"
signing_key="$2"
out_dir="$3"
manifest="$plugin_dir/plugin.json"

[[ -f "$manifest" ]] || {
  echo "missing manifest: $manifest" >&2
  exit 2
}
[[ -f "$signing_key" ]] || {
  echo "missing signing key: $signing_key" >&2
  exit 2
}

id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$manifest")"
version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$manifest")"
[[ -n "$id" && -n "$version" ]] || {
  echo "manifest must declare id and version" >&2
  exit 2
}

mkdir -p "$out_dir"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

archive="$out_dir/$id-$version.anvil-plugin"
signature="$archive.sig"
public_key="$out_dir/official.pub"

tar \
  --sort=name \
  --mtime='@0' \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -czf "$archive" \
  -C "$plugin_dir" .

digest_hex="$(sha256sum "$archive" | awk '{print $1}')"
printf '%s\n' "$digest_hex" > "$archive.sha256"
python3 -c 'import binascii,sys; sys.stdout.buffer.write(binascii.unhexlify(sys.argv[1]))' \
  "$digest_hex" > "$tmp/digest.bin"

openssl pkeyutl -sign -inkey "$signing_key" -rawin -in "$tmp/digest.bin" -out "$signature"
openssl pkey -in "$signing_key" -pubout -outform DER -out "$tmp/public.der"
tail -c 32 "$tmp/public.der" | od -An -v -tx1 | tr -d ' \n' > "$public_key"
printf '\n' >> "$public_key"

printf 'archive=%s\nsignature=%s\npublic_key=%s\n' "$archive" "$signature" "$public_key"
