#!/bin/bash
BIN="$(git rev-parse --show-toplevel)/target/release/herdr-axi"
while true; do
  STATE=$("$BIN" agents | awk '/^tester /{print $3}')
  if [ "$STATE" = "done" ] || [ "$STATE" = "blocked" ] || [ -z "$STATE" ]; then
    echo "tester reached terminal state: ${STATE:-gone}"
    break
  fi
  sleep 15
done
