#!/usr/bin/env bash

set -e

COMMON_ARGS="--speed 921600 --partition-table partitions.csv --monitor"
TARGET_TTY="/dev/ttyUSB0"

case "$1" in
"" | "release")
    cargo espflash $COMMON_ARGS --release "${TARGET_TTY}"
    ;;
"debug")
    cargo espflash $COMMON_ARGS "${TARGET_TTY}"
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac


