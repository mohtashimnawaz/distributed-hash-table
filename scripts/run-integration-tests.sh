#!/usr/bin/env bash
set -euo pipefail

# Run only ignored (integration) tests with verbose output
cargo test -- --ignored --nocapture
