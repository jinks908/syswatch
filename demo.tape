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

# Background load so the new readouts move on camera: `yes` for the CPU
# bars + the ~W energy attribution, and a rate-limited download so the
# measured per-process NET columns (nettop) have a process to catch.
# Start them first so there's already history by the time the tour
# reaches those tabs. All hidden.
Hide
Type "yes > /dev/null & yes > /dev/null & yes > /dev/null &"
Enter
Type "curl -s -m 90 --limit-rate 3M http://archive.ubuntu.com/ubuntu/dists/noble/Contents-amd64.gz -o /dev/null &"
Enter
Type "./target/release/syswatch --tick 500"
Enter
Sleep 5s
Show

# [1] Overview — KPI strip, per-core grid, top procs (%MEM), live insights.
Sleep 5s

# [2] CPU — aggregate sparkline + per-core bars (busy now). On Linux the
# counters include the PSI stall line.
Type "2"
Sleep 3s

# [3] Memory — NEW in v0.7.2: honest per-process breakdown. Footprint
# (Activity Monitor's number) on macOS, PSS/private/swap on Linux,
# lifetime PEAK on both — RSS demoted to the supporting column it is.
Type "3"
Sleep 6s

# [6] Procs — %MEM utilisation, R/s + W/s split, measured NET columns
# (estimates get a ~ marker). Sort by the new mem key, then walk the
# selection to show the drill-in: threads, peak memory, ~W draw.
Type "6"
Sleep 3s
Type "s"
Sleep 3s
Down@250ms 4
Sleep 3s

# [7] GPU — per-device utilization + VRAM time-series, renderer/tiler
# split on Apple Silicon.
Type "7"
Sleep 4s

# [8] Power — NEW in v0.7.2: top energy consumers, measured CPU-rail
# watts attributed by CPU share (~ marks the estimate).
Type "8"
Sleep 6s

# [-] Timeline — stacked activity strips + scrubber. Walk back, then return live.
Type "-"
Sleep 3s
Left@200ms 12
Sleep 2s
Right@200ms 12
Sleep 2s

# [+] Insights — plain-English anomaly cards (now PSI stall, leak
# detection, energy hogs).
Type "+"
Sleep 4s

# Back to Overview to land the closing frame.
Type "1"
Sleep 4s

Type "q"
Sleep 500ms

# Clean up the background load (hidden).
Hide
Type "pkill -x yes 2>/dev/null; pkill -f 'archive.ubuntu' 2>/dev/null; clear"
Enter
Sleep 500ms
Show
