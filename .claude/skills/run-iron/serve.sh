#!/bin/zsh
# Start or stop the Vite dev server on :5173 (base path /Ironfell/).
#   serve.sh start   -> background, waits until it answers, log in this dir
#   serve.sh stop    -> kills whatever listens on :5173
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/../../.."
case "${1:-start}" in
  start)
    lsof -ti:5173 -sTCP:LISTEN | xargs kill 2>/dev/null || true
    (bun run dev > "$HERE/vite.log" 2>&1 &)
    for i in $(seq 1 30); do curl -sf http://localhost:5173/Ironfell/ >/dev/null && break; sleep 1; done
    curl -sf http://localhost:5173/Ironfell/ >/dev/null && echo "vite serving http://localhost:5173/Ironfell/" || { echo "vite did not start; see $HERE/vite.log"; exit 1; }
    ;;
  stop)
    lsof -ti:5173 -sTCP:LISTEN | xargs kill 2>/dev/null || true
    echo "stopped"
    ;;
  *) echo "usage: serve.sh [start|stop]"; exit 1 ;;
esac
