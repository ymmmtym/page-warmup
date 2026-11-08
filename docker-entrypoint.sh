#!/bin/sh
set -eu

if [ "${HEADLESS:-true}" = "false" ]; then
    exec xvfb-run --auto-servernum /usr/local/bin/page-warmup "$@"
fi

exec /usr/local/bin/page-warmup "$@"
