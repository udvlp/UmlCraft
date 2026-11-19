#!/bin/sh
# Signs the release's checksums for the app's update check (`UPDATE_KEY` in
# crates/umlcraft-app/src/update.rs), with the trusted comment `umlcraft <version>`.
#
#   sh sign-checksums.sh <SHA256SUMS.txt> <version>
#
# The secret key comes from $UPDATE_SIGNING_KEY: the contents of a minisign secret key made
# without a password (`minisign -G -W`), a secret of the `release` environment on GitHub.
# Without it the checksums go out unsigned, unless the app already checks signatures.
set -eu
sums=$1
version=$2
source=$(dirname "$0")/../crates/umlcraft-app/src/update.rs
key=$(sed -n 's/^const UPDATE_KEY: Option<&str> = Some("\([^"]*\)");$/\1/p' "$source")
if [ -z "${UPDATE_SIGNING_KEY:-}" ]; then
  if [ -n "$key" ]; then
    echo "::error::update.rs sets UPDATE_KEY, but the secret UPDATE_SIGNING_KEY is missing:" \
      "the app would refuse this release."
    exit 1
  fi
  echo "::warning::No UPDATE_SIGNING_KEY: the checksums go out unsigned."
  exit 0
fi
if [ -z "$key" ]; then
  echo "::error::UPDATE_SIGNING_KEY is set, but update.rs has no UPDATE_KEY to check it with."
  exit 1
fi
if ! command -v minisign >/dev/null; then
  sudo apt-get update -qq
  sudo apt-get install -y -qq minisign
fi
secret=$(mktemp)
trap 'rm -f "$secret"' EXIT
printf '%s\n' "$UPDATE_SIGNING_KEY" > "$secret"
minisign -S -s "$secret" -m "$sums" -x "$sums.minisig" -t "umlcraft $version"
# The signature has to pass the check the app makes.
minisign -V -P "$key" -m "$sums" -x "$sums.minisig"
