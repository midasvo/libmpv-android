#!/bin/bash -e

cd "$( dirname "${BASH_SOURCE[0]}" )"

./include/install-system-deps.sh
./include/download-sdk.sh
./include/download-deps.sh
