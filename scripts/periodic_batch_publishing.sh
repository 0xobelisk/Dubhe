#!/bin/bash

DEFAULT_URL="http://127.0.0.1:12345"
URL="${1:-$DEFAULT_URL}"

if ! command -v curl &> /dev/null; then
  echo "Error: curl is not installed. Please install curl and try again."
  exit 1
fi

if ! command -v date &> /dev/null; then
  echo "Error: date command is not available. Please ensure coreutils are installed."
  exit 1
fi

if [ -z "$PRODUCING_PERIOD" ]; then
  PRODUCING_PERIOD=12
  echo "PRODUCING_PERIOD not set. Using default of $PRODUCING_PERIOD seconds."
fi

echo "Going to trigger eth_publishBatch at $URL every $PRODUCING_PERIOD seconds."

execute_curl() {
  curl -X POST "$URL" \
       -H "Content-Type: application/json" \
       --data '{"jsonrpc":"2.0","method":"eth_publishBatch","params":[],"id":1}' \
       --silent --output /dev/null --show-error;
  echo "$(date '+%Y-%m-%d %H:%M:%S'): Batch publishing has been requested."
}

while true; do
  execute_curl
  sleep "$PRODUCING_PERIOD"
done