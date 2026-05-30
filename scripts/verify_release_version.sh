#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <git-ref-name-or-semver>" >&2
  exit 2
fi

ref_name="$1"
version="${ref_name#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "release ref must be a semantic version tag like v1.2.3; got '$ref_name'" >&2
  exit 1
fi

cargo_version="$(
  awk -F '"' '
    $1 ~ /^version[[:space:]]*=[[:space:]]*$/ {
      print $2
      exit
    }
  ' Cargo.toml
)"

if [[ "$cargo_version" != "$version" ]]; then
  echo "Cargo.toml version '$cargo_version' does not match release tag '$version'" >&2
  exit 1
fi

if ! grep -Eq "^## \\[$version\\]([[:space:]]+-[[:space:]]+[0-9]{4}-[0-9]{2}-[0-9]{2})?[[:space:]]*$" CHANGELOG.md; then
  echo "CHANGELOG.md must contain a Keep a Changelog section for [$version], optionally dated as '## [$version] - YYYY-MM-DD'" >&2
  exit 1
fi

echo "release version verified: $version"
