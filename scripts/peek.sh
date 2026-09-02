#!/bin/bash
# peek.sh <pane_id> [lines] - read a herdr pane's recent output
P="${1:-w6:p8}"; L="${2:-120}"
exec ~/.local/bin/herdr pane read "$P" --source recent --lines "$L"
