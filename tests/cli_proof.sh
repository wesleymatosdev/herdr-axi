#!/bin/bash
# Live proof script for herdr-axi v0.1 (kept in-repo per repo conventions).
# Runs the four subcommands against the live herdr server.
set -u
B="$(dirname "$0")/../target/debug/herdr-axi"
"$B" agents; echo "agents=$?"
"$B" fleet; echo "fleet=$?"
"$B" dispatch ghost-agent hi --timeout 3000; echo "dispatch-unknown=$? (want 2)"
"$B" wait spike-vec --until idle --timeout 15000; echo "wait=$?"
