#!/bin/sh
# Sets the workspace version in Cargo.toml. All crates inherit it, and the
# plugins report it as their VST3/CLAP version.
#
# usage: scripts/set-version.sh 1.2.3

set -eu

version=${1:?usage: $0 VERSION}
case $version in
    *[!0-9A-Za-z.-]* | "" | -* | .*)
        echo "invalid version: $version" >&2
        exit 1
        ;;
esac

cd "$(dirname "$0")/.."
VERSION=$version perl -0pi -e \
    's/(\[workspace\.package\]\s*\nversion\s*=\s*")[^"]*(")/$1$ENV{VERSION}$2/' Cargo.toml

if ! grep -q "^version = \"$version\"$" Cargo.toml; then
    echo "could not set the version in Cargo.toml" >&2
    exit 1
fi
echo "version set to $version"
