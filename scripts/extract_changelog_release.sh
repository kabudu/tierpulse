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

awk -v version="$version" '
  BEGIN {
    heading = "^## \\[" version "\\]([[:space:]]+-[[:space:]]+[0-9]{4}-[0-9]{2}-[0-9]{2})?[[:space:]]*$"
    in_section = 0
    found = 0
  }
  $0 ~ heading {
    in_section = 1
    found = 1
    next
  }
  in_section && /^## / {
    exit
  }
  in_section {
    print
  }
  END {
    if (!found) {
      exit 1
    }
  }
' CHANGELOG.md | awk '
  NF {
    seen = 1
  }
  seen {
    lines[++count] = $0
  }
  END {
    while (count > 0 && lines[count] ~ /^[[:space:]]*$/) {
      count--
    }
    if (count == 0) {
      exit 1
    }
    for (i = 1; i <= count; i++) {
      print lines[i]
    }
  }
'
