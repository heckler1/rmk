#!/usr/bin/env bash

# Only the `esp` image has this file.
if [[ -f /usr/local/export-esp.sh ]]; then
    source /usr/local/export-esp.sh
fi

exec "$@"
