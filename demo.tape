Output demo.gif

# NOTE: this demo is recorded with the btop-style fade rendering enabled.
# There's no CLI flag for it, so set `graph_fade = true` in
# ~/.config/syswatch/config.toml before regenerating, or the charts/tables
# render solid instead of faded.

Require syswatch

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 700
Set Theme "Catppuccin Mocha"
Set TypingSpeed 30ms
Set Framerate 18
Set Padding 10

# Spin up background CPU work (yes) and a Metal GPU compute load (gpuload)
# so the CPU per-core bars AND the new GPU util/VRAM graphs actually move on
# camera. Start them first so there's already history by the time the tour
# reaches those tabs. All hidden.
Hide
Type "yes > /dev/null & yes > /dev/null & yes > /dev/null &"
Enter
Type "/tmp/gpuload > /dev/null 2>&1 &"
Enter
Type "./target/release/syswatch --tick 500"
Enter
Sleep 4s
Show

# [1] Overview — KPI strip, per-core grid, top procs, live insights.
Sleep 5s

# [2] CPU — aggregate sparkline + per-core utilization bars (busy now).
Type "2"
Sleep 4s

# [3] Memory — RAM bar, swap, top RSS.
Type "3"
Sleep 3s

# [7] GPU — NEW in v0.7.0: per-device utilization + VRAM time-series
# (~120s history), renderer/tiler engine split, live counters.
Type "7"
Sleep 6s

# [6] Procs — sortable table; cycle the sort key so the table reshuffles.
Type "6"
Sleep 3s
Type "s"
Sleep 2s
Type "s"
Sleep 2s

# [8] Power — battery, source, throttle, per-rail draw, fans.
Type "8"
Sleep 4s

# [-] Timeline — stacked activity strips + scrubber. Walk back, then return live.
Type "-"
Sleep 3s
Left@200ms 12
Sleep 2s
Right@200ms 12
Sleep 2s

# [+] Insights — plain-English anomaly cards.
Type "+"
Sleep 5s

# Back to Overview to land the closing frame.
Type "1"
Sleep 4s

Type "q"
Sleep 500ms

# Clean up the background load (hidden).
Hide
Type "pkill -x yes 2>/dev/null; pkill -f /tmp/gpuload 2>/dev/null; clear"
Enter
Sleep 500ms
Show
