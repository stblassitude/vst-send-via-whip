#!/bin/sh
# Builds Galene from a source checkout and runs it locally for testing, with
# plain HTTP on port 8444 and these groups:
#
#   open, voc-a, voc-b, voc-c   anyone may present, no token needed
#   secure                      presenting needs a token, see below
#
# Admin user "admin", password "admin" (for galenectl and /galene-api/v0/.stats).
#
# usage: scripts/local-galene.sh GALENE_CHECKOUT [WORKDIR]
#
# Create a token for the secure group with:
#   WORKDIR/galenectl -config WORKDIR/galenectl.json -server http://127.0.0.1:8444 \
#       -admin-username admin -admin-password admin \
#       create-token -group secure -permissions present

set -eu

src=${1:?usage: $0 GALENE_CHECKOUT [WORKDIR]}
work=${2:-${TMPDIR:-/tmp}/whip-send-galene}
src=$(cd "$src" && pwd)

mkdir -p "$work/data" "$work/groups" "$work/recordings"
(cd "$src" && CGO_ENABLED=0 go build -o "$work/galene" . && go build -o "$work/galenectl" ./galenectl)

cat > "$work/data/config.json" <<'EOF'
{
    "users": {"admin": {"password": "admin", "permissions": "admin"}},
    "writableGroups": true
}
EOF

for g in open voc-a voc-b voc-c; do
    cat > "$work/groups/$g.json" <<'EOF'
{
    "users": {"op": {"password": "op", "permissions": "op"}},
    "wildcard-user": {"password": {"type": "wildcard"}, "permissions": "present"},
    "codecs": ["opus", "vp8"]
}
EOF
done

cat > "$work/groups/secure.json" <<'EOF'
{
    "users": {"op": {"password": "op", "permissions": "op"}},
    "codecs": ["opus", "vp8"]
}
EOF

echo "Galene at http://127.0.0.1:8444/ (work directory $work)"
cd "$work"
exec ./galene -insecure -http :8444 -turn "" \
    -data "$work/data" -groups "$work/groups" -recordings "$work/recordings" \
    -static "$src/static"
