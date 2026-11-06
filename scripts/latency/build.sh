#!/bin/zsh
# Build the latency probe (plans/rendering/latency-view.md §4) beside this script.
set -e
cd "$(dirname "$0")"
swiftc -O -o latency-probe probe/main.swift
swiftc -O -o latency-key key/main.swift
