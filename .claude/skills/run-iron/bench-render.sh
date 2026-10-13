#!/bin/bash
# The 2D render benchmark (roadmap stage H): how a composition of N shapes
# costs per frame, phase by phase, before any culling or raster cache.
#
#   .claude/skills/run-iron/bench-render.sh [shape counts…]     # default 1000 5000 10000 20000
#   SIZES="1600x1000@1 2560x1440@2" .claude/skills/run-iron/bench-render.sh 10000
#
# Needs a build (build.sh) and Vite running (serve.sh start). Runs headless
# Chrome on the GPU with vsync and the frame-rate limit off, so frames per
# second is throughput. Each shape count gets a fresh page with `?stress=N`
# (src/document_bridge/stress.rs), then these scenarios in the Stress tab:
#
#   idle         nothing changes, pointer on empty canvas: the floor
#   hover-group  pointer resting over the field: the hover outline of a
#                group with N descendants, recomputed every frame
#   select-group the field group selected, pointer elsewhere: its box and
#                handles, recomputed every frame
#   spin         scrub the `spin` slider; the field group's bound rotation
#                moves every shape while one leaf changes (what a subtree
#                raster cache is for)
#   drag-one     Cmd-drag the `sample` rect inside its cell group: one node
#                changes per frame (what damage-driven re-encoding is for);
#                then Escape, paced, up to nothing selected for what
#                follows (a click on "empty" canvas can land on a box grip)
#   pan          wheel-scroll the content 200 px down and back, 5 px a step,
#                pointer on empty canvas above it: the view's transform
#                changes (what culling and tiles are for)
#   *-zoomed     the same at zoom ≈0.37, everything small (what baking at
#                low zoom is for)
#
# Prints one line per scenario: `N SIZE scenario {json}`. Pipe the output
# through bench-render-table.py for a table.
set -u
cd "$(dirname "$0")"
COUNTS=("${@:-1000 5000 10000 20000}")
SIZES=${SIZES:-1600x1000@1}
SCRIPT=$(mktemp -t iron-bench-render)
trap 'rm -f "$SCRIPT"' EXIT

for N in ${COUNTS[@]}; do
  for SIZE in $SIZES; do
    # The slider's knob starts at panel (40, 42); the sample rect's centre is
    # at (60, 100), both at zoom 1 and pan 0 (stress.rs). (600, 50) is empty
    # canvas above the field at both zooms, so idle and pan measure no hover.
    cat > "$SCRIPT" <<EOS
nav http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=$N
ready
sleep 4000
pmove stress 600 50
measure-reset
sleep 3000
measure idle
pmove stress 60 100
sleep 300
measure-reset
sleep 3000
measure hover-group
pclick stress 60 100
sleep 500
pmove stress 600 50
sleep 300
measure-reset
sleep 3000
measure select-group
key Escape
sleep 300
key Escape
sleep 300
eval __iron.selection().length === 0 ? 'deselected' : 'STILL SELECTED ' + __iron.selection()
pmove stress 40 42
down
psweep stress 40 42 60 42 4 40
measure-reset
psweep stress 60 42 340 42 50 40
psweep stress 340 42 40 42 50 40
measure spin
up
sleep 3000
keydown Meta
pmove stress 60 100
down
psweep stress 60 100 80 110 4 40
measure-reset
psweep stress 80 110 400 400 50 40
psweep stress 400 400 80 110 50 40
measure drag-one
up
keyup Meta
key Escape
sleep 300
key Escape
sleep 300
key Escape
sleep 300
key Escape
sleep 300
pmove stress 600 50
eval __iron.selection().length === 0 ? 'deselected' : 'STILL SELECTED ' + __iron.selection()
sleep 3000
measure-reset
pwheel stress 600 50 0 -5 40 40
pwheel stress 600 50 0 5 40 40
measure pan
keydown Control
pwheel stress 40 42 0 100 1 40
keyup Control
pmove stress 600 50
sleep 2000
measure-reset
sleep 3000
measure idle-zoomed
pmove stress 40 42
down
psweep stress 40 42 50 42 4 40
measure-reset
psweep stress 50 42 150 42 50 40
psweep stress 150 42 40 42 50 40
measure spin-zoomed
up
sleep 3000
pmove stress 600 50
measure-reset
pwheel stress 600 50 0 -5 40 40
pwheel stress 600 50 0 5 40 40
measure pan-zoomed
errors
quit
EOS
    GPU=1 UNTHROTTLED=1 SIZE=$SIZE bun run ./drive.ts < "$SCRIPT" 2>&1 \
      | grep -E '^measure |STILL SELECTED|panicked|RuntimeError|^Error' \
      | sed -E "s/^measure /$N $SIZE /"
  done
done
