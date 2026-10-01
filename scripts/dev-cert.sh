#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
umask 077
mkdir -p .dev
if [ -e .dev/localhost.pem ] || [ -e .dev/localhost-key.pem ]; then
    echo "Development certificate already exists; leaving it unchanged."
    exit 0
fi
cat > .dev/openssl.cnf <<'EOF'
[req]
distinguished_name = dn
x509_extensions = extensions
prompt = no
[dn]
CN = localhost
[extensions]
subjectAltName = DNS:localhost,IP:127.0.0.1,IP:10.0.2.2
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = serverAuth
EOF
openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
    -config .dev/openssl.cnf -keyout .dev/localhost-key.pem \
    -out .dev/localhost.pem
echo "Created .dev/localhost.pem and .dev/localhost-key.pem for local development."
