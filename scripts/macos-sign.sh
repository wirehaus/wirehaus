#!/bin/sh
set -eu

binary=${1:?usage: macos-sign.sh <binary>}
name=${WIREHAUS_SIGN_NAME:-Wirehaus Local Signing}
dir=${WIREHAUS_SIGN_DIR:-$HOME/.wirehaus-signing}
keychain=$dir/signing.keychain-db

mkdir -p "$dir"
chmod 700 "$dir"

if [ ! -f "$keychain" ]; then
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    cat > "$work/openssl.cnf" <<CNF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$name
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
CNF
    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/openssl.cnf" \
        -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
    openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
        -passout pass:wirehaus -out "$work/identity.p12"
    security create-keychain -p "" "$keychain"
    security set-keychain-settings "$keychain"
    security unlock-keychain -p "" "$keychain"
    security import "$work/identity.p12" -k "$keychain" -P wirehaus -T /usr/bin/codesign
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "" "$keychain" > /dev/null
fi

security unlock-keychain -p "" "$keychain"
hash=$(security find-identity -p codesigning "$keychain" | sed -n "s/.*\([0-9A-F]\{40\}\) \"$name\".*/\1/p" | head -1)
[ -n "$hash" ] || { echo "no signing identity named $name in $keychain" >&2; exit 1; }

codesign --force --sign "$hash" --keychain "$keychain" --identifier wirehaus-agent "$binary"
codesign -d -r- "$binary" 2>&1 | tail -1
