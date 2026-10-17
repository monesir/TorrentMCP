# Live paired-source task continuation

User requested: try all sources with the same search. We committed to all86 public plugin identities, both native Rust and retained Python, using exactly `nirvana in fire`, category all, source timeout20s, result limit20 and parallel6. Do not change the query or silently exclude failed sources. Private entries remain excluded. This task is NOT complete yet.

Operation: unified exec session80797, `python -B -X utf8 scripts/verify-live-sources.py --bundle dist/torrent-search-0.4.0-native-public-verified-20261017 --output verification/native-ports/live-paired-20261017 --query "nirvana in fire" --timeout20 --limit20 --parallel6` (actual flags in process use --timeout 20, --limit 20, --parallel 6).

Log: .cache/live-paired-20261017.log. Exit marker: .cache/live-paired-20261017-exit.txt. Progress: state.json beside this file (phase rust, python or completed; backend status summaries). Each backend runs sequentially. Raw rust.json/python.json, stderr logs, isolated resource copies/homes/caches are retained in this directory. The shipped bundle is unchanged. Runner verifies original pins and expects86 source reports for each backend. Normal source failures produce partial reports and nonzero CLI exit codes; that alone is not harness failure.

When heartbeat fires, inspect this operation once. If still running, keep heartbeat/back off and avoid duplicate launches or unchanged status messages. If completed, stop heartbeat and immediately continue user task: validate state phase completed, 86 IDs per backend, exactly the same query/settings, and86 rows in comparison.json/sources.csv. Read error types and result counts, inspect representative title matches from raw report observations, and distinguish completed-empty, failed and timed_out. Advisory title_matches only recognizes Nirvana in Fire, Lang Ya Bang and 琅琊榜; it is not proof of full relevance. Timings are per-source wall observations with live network/cache variation, not a controlled speed benchmark. Do not claim native speed improvements from this run.

On failure/stall, inspect log, progress and process before recovering; preserve raw outputs, reuse completed backend reports where feasible rather than repeating expensive live work. The user asked to test, not redesign or port more plugins. Do not silently treat unavailable websites as an implementation success or a zero-result search. Jackett is unconfigured and should appear honestly as such.

After successful completion, write a compact Arabic summary Markdown beside reports using the actual counts and links to full86-source CSV/table; final answer should include summary counts for Rust/Python, sources returning relevant-looking results vs unrelated/empty, material failure limits, and clickable absolute report links. Do not print all86 rows in the chat unless useful/requested. Disable heartbeat after waiting ends. Complete the outcome rather than merely reporting that the process ended.

Previous port task completed and packaged: verification/native-ports/STATE.md. Do not overwrite that completion checkpoint with this new task. All code changes and original artifacts are preserved; no worktree or subagent is involved. Only test runner scripts/verify-live-sources.py was added for this request.

Memory quick pass used MEMORY.md line4 (live-site behavior requires fresh validation). Append one final memory citation block if relied upon, with rollout id01a14717-c5e0-7330-a0b1-4730a8e4e81a. All results must come from this fresh run, not old memory or earlier live-all86-rust.json.

Active heartbeat: automation-3, every 3 minutes, current thread; created and verified. Disable when waiting ends.

COMPLETED: exit marker0; both backend reports cover86 IDs and identical query/settings; comparison.json and sources.csv each cover86 sources. Review saved in SUMMARY.ar.md/review-summary.json, with Jackett diagnostic excluded from relevant counts. Heartbeat automation-3 disabled (PAUSED). No further waiting or engine changes.
