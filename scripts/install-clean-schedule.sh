#!/bin/zsh
# Installs (or with --remove, removes) a weekly launchd job that runs
# scripts/clean-disk.sh: Sundays at 03:00, or at the next wake if the Mac was asleep.
# Output goes to ~/Library/Logs/iron-clean-disk.log.
set -euo pipefail

label="local.iron.clean-disk"
plist="$HOME/Library/LaunchAgents/$label.plist"
script="${0:A:h}/clean-disk.sh"
log="$HOME/Library/Logs/iron-clean-disk.log"

if [[ "${1:-}" == --remove ]]; then
  launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
  rm -f "$plist"
  echo "removed $label"
  exit 0
fi

mkdir -p "${plist:h}"
cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array><string>/bin/zsh</string><string>$script</string></array>
  <key>StartCalendarInterval</key>
  <dict><key>Weekday</key><integer>0</integer><key>Hour</key><integer>3</integer><key>Minute</key><integer>0</integer></dict>
  <key>StandardOutPath</key><string>$log</string>
  <key>StandardErrorPath</key><string>$log</string>
  <key>LowPriorityIO</key><true/>
  <key>Nice</key><integer>10</integer>
</dict>
</plist>
EOF

launchctl bootout "gui/$(id -u)/$label" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$plist"
echo "installed $label (weekly, Sun 03:00); log: $log"
echo "run it now with: launchctl kickstart gui/$(id -u)/$label"
