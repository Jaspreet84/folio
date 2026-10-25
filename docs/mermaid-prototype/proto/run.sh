#!/bin/bash
# Render each Mermaid file given at width $W (default 80).
cd "$(dirname "$0")" || exit 1
for f in "$@"; do python3 -I proto.py ${W:-80} < "$f"; done
