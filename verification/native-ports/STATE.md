# Public qBittorrent native ports — verified completion

Completed the requested port of all 8 main-repository plugins and all 78 public community entries (86 distinct plugin identities). All 28 private entries are excluded. Original Python files remain selectable and byte-pinned; qBittorrent installation is not required.

## Final deliverable

`dist/torrent-search-0.4.0-native-public-verified-20261017.zip` (Windows x64), with Python 3.14.8, 9 pinned compatibility libraries, ffprobe, notices, public provenance, and corresponding Rust source/build files/test fixtures. Package excludes user configuration, cookies, logs and private plugin files. Source backend: `--source-backend auto|rust|python`; `auto` prefers native and does not silently retry failed requests in Python. Native sources run with no Python process.

## Verification

- `portable-all86-parity.json`: final packaged executable and bundled Python runtime, 86 source IDs, 300 source/category comparisons, 11 resolver comparisons, matching fields/routes and Zooqle POST methods/bodies. Native calls used a nonexistent Python path. Limits, HTTP failures, partial retention, malformed/empty JSON, challenge detection and deadline passed.
- These fixtures are synthetic protocol tests except captured LimeTorrents HTML; they are not proof that every upstream site currently works.
- `.cache/native-package-final.log`: 49 Rust unit/integration tests passed, release build and packaged source discovery passed. Includes HTTP/UDP tracker protocol tests, POST identity compatibility, and a regression preventing image checksums from becoming torrent hashes.
- `portable-audit.json`: executable SHA256 matches parity evidence; ZIP SHA256 sidecar verified; all86 original plugin SHA256 pins verified; no private plugins/local config packaged. Exact archive hash and byte size are recorded there.
- `.cache/native-final-exit.txt`: 0. Final operation 10098 completed successfully. Heartbeat automation-2 was disabled after waiting ended. No pending operation remains for this request.

## Live-source limitation

`live-all86-rust.json` separately records a live search of 'nirvana in fire' with limit3, timeout10 and parallel8: 33 plugins completed (including empty results), 46 failed and 7 timed out; 20 merged results came from 9 plugin identities. Result relevance is not guaranteed. Errors included network failures, HTTP403/404/429/5xx and missing local Jackett configuration. This live test used the intermediate portable binary before the last FitGirl hash-extraction fix; it is evidence about observed site availability, not a full live certification of the final binary. Main sources previously tested against both backends included LimeTorrents, PirateBay and TorrentsCSV. Do not interpret protocol parity as universal live availability.

## Scope, provenance and maintenance

Main commit: `240b57a90ca4f9e3869151b73c76c46f1b7bb59b`; wiki commit: `e00fd9cd551258290213e684d522bc5e06130a99`. Source provenance and recovered archive URLs remain in research/qbittorrent; portable public provenance is in docs/source-provenance.json. Public-only runtime registration is resources/sources.json. Core link: https://github.com/qbittorrent/search-plugins/tree/master/nova3/engines . Community link: https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins .

See docs/native-sources.md for CLI usage, optional Jackett/Rutor configuration, FitGirl tracker/cache settings, and optional post_form on HTTP download references. Search does not download video payloads. Runtime wheel hashes are in resources/python-dependencies.lock.json. The canonical packaging command is scripts/package-windows.ps1 with optional -TargetDirectory for an isolated build.

Other chat changes in analysis/media/agent/Markdown and its root target executable were preserved. Historical LimeTorrents experiment/benchmark packages remain intact. All research children were finished and interrupted. No automatic continuation is needed.


## TorrentDownloads correction — 2026-10-18

Fixed percent-decoding of whole magnet links and isolation of individual TorrentDownloads row failures. Detailed evidence: `torrentdownloads-fix-20261018/report.json`. The full saved 0.5.0 package suite passed (61 tests), with 40 controlled source/category comparisons across 12 adapters and 1 resolver comparison. Live `nirvana in fire` search completed with 20 results and no error (previously failed after 9); all four encoded C1 names remained encoded.

Hotfix bundle: `dist/torrent-search-0.5.0-torrentdownloads-fix-20261018`. Built from saved packaged 0.5.0 source plus the exact current fixes because unrelated in-progress comparison modules prevented the current workspace build. No unrelated files were changed and old packages/evidence were preserved.


## Live speed and resource comparison — 2026-10-18

Completed all 516 serial measurements (86 public adapters x Rust/Python x3 trials) using the TorrentDownloads-corrected 0.5.0 executable. Query nirvana in fire, category all, limit20, timeout20. Windows Job Object accounting includes the CLI, Python and owned console hosts; all86 plugin pins and six trial/backend resource copies were verified. No instrumentation failures.

Evidence: `performance-all86-20261018/audit.json`, `summary.json`, `comparison-readable.csv`, `sources.csv`, `matched-sources.csv`, and `SUMMARY.ar.txt`. Nine adapters had positive identity-matched completed pairs; seven also returned query-related titles. AnimeTosho community and FitGirl returned unrelated titles. Other groups:23 empty in both backends,51 failed/partial paired trials,2 different results,1 unconfigured Jackett. Relative performance conclusions exclude failures, empty/different results and configuration rows. Live network timing includes startup and is not a pure language benchmark. Automation-3 is PAUSED; no further waiting or benchmark work remains.
