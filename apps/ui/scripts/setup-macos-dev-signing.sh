#!/bin/bash
# Create a self-signed code-signing identity for local Encore builds (run once per Mac).
#
# macOS privacy permissions follow an app's code signature. Ad-hoc builds get a new
# identity on every rebuild, so Screen Recording, Microphone and Input Monitoring had
# to be granted again after each build. Builds signed with this certificate keep the
# same identity, and permissions granted once stay valid.
#
# The certificate is only trusted for code signing on this Mac. It is not an Apple
# Developer ID: builds signed with it are for development and are not notarized.
set -euo pipefail

NAME="Encore Local Development"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-identity -p codesigning | grep -q "\"$NAME\""; then
  echo "\"$NAME\" already exists; nothing to do."
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cat > "$work/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

# The system LibreSSL writes PKCS#12 files that `security import` understands;
# Homebrew OpenSSL 3 defaults to algorithms the keychain rejects.
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.cnf" \
  -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
  -name "$NAME" -passout pass:encore -out "$work/identity.p12"
security import "$work/identity.p12" -k "$KEYCHAIN" -P encore -T /usr/bin/codesign >/dev/null

# Trust is optional: TCC matches the certificate hash in the designated requirement
# and does not evaluate trust. Trusting only hides the "not trusted" marker.
security add-trusted-cert -r trustAsRoot -p codeSign -k "$KEYCHAIN" "$work/cert.pem" 2>/dev/null \
  || echo "Note: the certificate is not marked trusted; signing still works for local builds."

security find-identity -p codesigning | grep "\"$NAME\"" >/dev/null \
  || { echo "The identity was not imported into the login keychain." >&2; exit 1; }
echo "Created \"$NAME\". Rebuild Encore; permissions now persist across builds."
echo "If codesign asks to use the key, choose Always Allow."
