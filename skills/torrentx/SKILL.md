---
name: torrentx
description: Find torrents and verify what is really inside them with torrentx. Use when the user asks for a movie, series, anime or other release, asks which release is best, or wants to know a torrent file's real resolution, codecs, audio or subtitle tracks.
---

# torrentx

torrentx searches torrent sites, lists the files inside a torrent, and reads a file's real streams with ffprobe while downloading only its headers from peers. It seamlessly handles direct Debrid streaming, high-speed multi-connection Aria2 downloads, automated weekly episodic monitoring, smart Arabic subtitle alignment, and private tracker integration.

## ⚡ Fast-Track Execution Engine (المحرك الفوري المباشر - الأولوية القصوى)

**CRITICAL INSTRUCTION FOR ANTIGRAVITY:**
When the user asks in natural language to download, stream without downloading, search, or play any movie/series/anime (e.g. *"شغل حلقة 1 من مسلسل Chernobyl بدون تحميل"*, *"حمل فيلم Inception 4K"*, *"شغل آخر حلقة"*):
**DO NOT use slow P2P MCP inspection tools or peer probing (which can hang for 5-10 minutes over peer-to-peer).**
Execute the unified engine directly via Python:
```powershell
python "~/.gemini/config/skills/torrentx/ai_assistant.py" "<exact user prompt>"
```
**Why this is mandatory:**
1. **Instant (1 to 2 seconds):** Directly resolves via Real-Debrid cloud cache and Torrentio API.
2. **Direct Cloud Streaming without Downloading:** If the user asks to stream/play without downloading, it automatically opens the user's preferred player (**MPC-HC**) streaming directly from the cloud with Arabic subtitles loaded!
3. **High-Speed Downloads:** If download is requested, it triggers 16-connection Aria2 download and schedules `alass` audio subtitle synchronization.
4. **Player Accuracy:** It ALWAYS respects the user's configured player from `debrid.json` (`mpc`) and does NOT attempt PotPlayer or VLC unless configured.

---

## 🌟 Conversational Natural Language Interaction (التفاعل بالكلام الطبيعي البسيط)

**CRITICAL GUIDELINE FOR AGENTS:**
The user should NEVER be required to write or run terminal commands manually. All interactions must be conducted in natural conversational Arabic.
When the user says:
- *"حمل لي مسلسل VisionQuest كل أسبوع 4K WEB-DL مع ترجمة عربية"* -> The agent automatically detects the schedule, registers the show via `series_monitor.py add`, sets the release window, and manages downloads and subtitles behind the scenes.
- *"شغل لي الفلم Paddington 2 في PotPlayer / MPC"* -> The agent resolves the stream, verifies Arabic subtitles, and opens the player directly with subtitles loaded.
- *"حمل لي الفلم بأقصى سرعة"* -> The agent launches the 16-connection Aria2 segmented download and downloads the matching Arabic subtitle into the folder.

## 1. Using torrentx

### Which interface

Use the MCP tools (`find`, `search`, `torrent_files`, `inspect_torrent`, `inspect_media`) when they are available. The server stays up as long as your session, keeps sources loaded and one torrent session warm, so a retry resumes a partial download instead of starting over.

Without MCP, use the command line and pipe the request on stdin. Do not write request files.

```powershell
'{"schema_version":1,"operation":"find","search":{"query":"The Bear S03"},"analysis":{"target":{"title":"The Bear","season":3}}}' | torrentx agent
```

On the command line every request carries `schema_version: 1` and `operation` (`find`, `search`, `torrent_files`, `inspect_torrent`, `inspect_media`); the MCP server adds both itself. Run `torrentx serve` in the background first if you will make more than one request: `agent` forwards to it and keeps the same warm session.

### Which tool

| The user wants | Call |
| --- | --- |
| A release, or the best release, of something | `find` |
| Only to see what exists, or names to choose from | `search` |
| What is inside a torrent they gave or you found | `torrent_files` |
| The real streams of specific files | `inspect_torrent` with `file_ids` |
| The real streams of a typical episode | `inspect_torrent` with `sample_files: 1` |
| The streams of a file already on this computer | `inspect_media` |

Start with `find` unless the user asked for something narrower. It searches, reads three files of each leading result (first, middle and last episode), compares what the files actually contain, and checks the file list for missing episodes, in one call.

### Building a request

Translate the user's words into fields; do not add wishes they did not express.

- `analysis.target`: the title, plus `year`, `season`, `part` or `episodes` when the user named them. When the user wants a whole season, put the season's full episode range from your research in `episodes` together with `season`: `find` then checks each pack's file list and reports the episodes it lacks in `coverage.missing`; `coverage.complete: true` means every requested episode has a file. Put other names the work is released under (romaji, English, original) in `aliases`.
- `analysis.constraints`: only what the user requires. A release whose name contradicts a constraint is excluded; one whose name says nothing is uncertain.
- `analysis.preferences`: ordered lists, best first. They rank and never exclude.
- `media.constraints` and `media.preferences`: the same idea checked on the inspected streams (`min_video_height`, `video_codecs`, `audio_languages`, `subtitle_languages`, bitrate and size bounds). `rank_by` sets which criterion decides first.
- Languages are ISO 639-1 codes: `en`, `ja`, `ar`.
- `search.category` narrows sources (`anime`, `tv`, `movies`, ...); the default is `all`. `search.sources` limits the search to source ids (`torrentx sources` lists them); omitted means every source.

Pass a search result's `reference` back exactly as it came. A magnet the user pasted needs only `{"kind": "magnet", "uri": "magnet:?..."}`.

### Time

- `search` takes seconds to about half a minute.
- `torrent_files` waits for the torrent's metadata from peers, up to `limits.timeout_seconds` (default 90).
- Each inspection then reads the file's headers, up to `inspection_limits.timeout_seconds` (default 60, at most 120).
- `find` therefore takes one to several minutes. Tell the user it is running; do not start a second identical call while the first is in progress.

### Reading the reply

Every reply has a `status`:

| `status` | Meaning |
| --- | --- |
| `completed` | Everything asked for was done. |
| `partial` | Real results, with some parts missing: a source failed, or some inspections failed. Use what is there and say what is missing. |
| `failed` | The work ran and nothing usable came of it; read `error`. |
| `error` | The request was rejected before any work, usually `invalid_request`; fix it from `error.message`. |
| `cancelled` | The call was stopped. |

The MCP server returns a compact view by default. Ask for `view: "full"` only when you need the per-token evidence behind a verdict; it is many times larger. `max_results` (default 20) controls how many search results the compact view shows.

In a compact `find` reply, each candidate either has `verified` (the container and streams ffprobe read from its file) and `unmet` (the conditions that file did not meet), or it has `inspection_error` and nothing else. Only candidates with `verified` have a `position`. A candidate may also carry `coverage`, the episodes found in its file names per season with `gaps` and `duplicates`, and `consistency`, how its other sampled files differ from the compared one; a difference such as an episode without the English subtitles is worth telling the user. `find` already retried a `probe_timeout` once by itself; a candidate with `attempts: 2` and still an `inspection_error` failed twice, so retry it with `inspect_torrent` only if it matters, with a larger timeout.

In `search.sources`, each source reports `completed` or `failed` with its error, and a failed one has a `failure_kind`. `search.source_failures` groups the failed sources by kind. Failed sources mean fewer results, not that the release does not exist.

### When something fails

| Code | What happened | What to do |
| --- | --- | --- |
| `probe_timeout` | Peers sent the file's header too slowly. | Retry once with `inspection_limits.timeout_seconds` raised (up to 120). The partial download is kept, so the retry continues it. |
| `torrent_service_timeout` | The torrent's metadata never arrived: no peer answered. | Retry once with a larger `limits.timeout_seconds`. If it fails again, the swarm is unreachable from here; the network may block BitTorrent. Say so; do not loop. |
| `read_budget_exceeded` | The headers were larger than `inspection_limits.max_read_bytes`. | Retry with a larger budget if the file matters. |
| `no_media_files` | The torrent has no video or audio files to inspect. | Report it. |
| `torrent_identity_mismatch`, `torrent_catalog_changed`, `torrent_metadata_invalid` | The torrent is not what the reference said, or is broken. | Do not retry; report it. |
| `torrent_service_unavailable`, `torrent_service_failed` | The local torrent engine failed. | Retry once; if it persists, report the message. |
| `invalid_request` | The request is malformed. | Fix the request from the message and call again. |

Source failures by `failure_kind`:

| Kind | Likely cause | What to say or do |
| --- | --- | --- |
| `dns_failed` | The name did not resolve through the system DNS nor through encrypted DNS: the domain is probably gone. | Say the site seems to be gone. |
| `connection_reset`, `network_unreachable` | The network blocks the site. | Say the site is blocked from this network. |
| `bot_challenge` | Cloudflare's browser check: the site admits only a real browser. | Nothing to retry; torrentx does not get past it. |
| `refused` | The site refused the request (HTTP 401/403). | Nothing to retry. |
| `rate_limited` | Too many requests. | Retry later, not now. |
| `not_found` | The site changed its search address. | Nothing to retry; torrentx needs an update for it. |
| `site_error` | The site or its proxy is down. | Retry later. |
| `timeout`, `tls_failed` | Slow, overloaded, or filtered. | One retry at most. |
| `not_configured` | Needs local setup (Jackett). | Ignore unless the user set it up. |

When an important source for the field fails (for anime, `nyaasi`), say so: the search is missing its main source, and the best release may not be in the results. Do not retry blocked sources in a loop.

A failed inspection means nothing about that file's streams is known. Say it was not inspected; do not fill the gap from the release name or from memory.

## 2. Know the field before searching

Release names, sources and quality differ by field. Before the first search, decide which field the request belongs to and read its file in this folder:

| The request is about | Read |
| --- | --- |
| Anime: TV series, OVA, ONA or anime films | [anime.md](anime.md) |
| Films (not anime) | [movies.md](movies.md) |
| TV series (not anime) | [series.md](series.md) |
| PC & Console Games (Megathread whitelist) | [games.md](games.md) |
| Music: albums, discographies, singles | [music.md](music.md) |

The field file tells you what to look up on the web about the work, which kinds of release exist, how they are named, and which sources carry them. Do that research first, then build the request from what you learned: the right titles and aliases, the right season and episode numbering, the right sources and constraints.

For a field with no file yet, still look the work up on the web first (official and alternative titles, year, seasons and episode counts) rather than searching on the user's wording alone.

In every field, a bigger file is not a better one. Codec efficiency, extra audio and subtitle tracks, disc structures and upscales all change size without changing quality the same way. Compare verified streams against what the user asked for.

Keep what the web told you apart from what torrentx verified, and say which is which when you report.

## 3. Debrid Cloud Services Integration (Multi-Debrid)

The system includes a unified Multi-Debrid manager configured via `C:\Users\9jhp9\.gemini\config\debrid.json` (with backward compatibility for `realdebrid.json`).
The helper script `C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py` manages cloud conversion, stream inspection, and sending downloads to IDM or FDM across 7 Debrid providers:

| Provider | Direct API Key / Token URL | Registration / Login URL |
| --- | --- | --- |
| **RealDebrid** | https://real-debrid.com/apitoken | https://real-debrid.com/ |
| **AllDebrid** | https://alldebrid.com/apikeys | https://alldebrid.com/ |
| **Offcloud** | https://offcloud.com/api | https://offcloud.com/ |
| **TorBox** | https://torbox.app/settings?section=account | https://torbox.app/ |
| **Premiumize** | https://www.premiumize.me/account | https://www.premiumize.me/login |
| **DebridLink** | https://debrid-link.fr/webapp/apikey | https://debrid-link.fr/webapp/login |
| **EasyDebrid** | https://easydebrid.com/user/api | https://easydebrid.com/ |

### Capabilities & Usage:
1. **View Services & Status:**
   ```powershell
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" services
   ```

2. **Add / Update an API Key:**
   Each user can configure their own service keys directly without sharing credentials:
   ```powershell
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" set-key realdebrid "<YOUR_API_TOKEN>"
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" set-key alldebrid "<YOUR_API_KEY>"
   ```

3. **Resolve & Cache Check:**
   ```powershell
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" resolve "<magnet>"
   ```
   Converts the magnet on the Debrid cloud service, selects media files, and if cached, unprotects direct links and immediately probes streams with `ffprobe`.

4. **Audio Track Counting & Russian Dub Analysis:**
   - Detects all audio streams, their languages, codecs, channels, and titles.
   - Specifically flags bloated audio tracks (e.g. Russian dubs common on Russian releases).
   - If the user asks for audio count or complains about excessive foreign audio bloat, inspect the stream via `debrid_manager.py` or `inspect_torrent` and report the exact number and languages of audio tracks.

5. **Sending Downloads to IDM / FDM:**
   - **IDM:**
     ```powershell
     python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" download "<direct_url>" idm
     ```
   - **FDM:**
     ```powershell
     python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" download "<direct_url>" fdm
     ```

6. **Debrid Media Manager (DMM) Integration:**
   - Website: https://debridmediamanager.com/
   - Allows users to connect their Real-Debrid API token and instantly view all community-cached torrents for any title.
   - Generate direct one-click DMM links:
     ```powershell
     python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" dmm "<imdb_id_or_title>" [movie|show] [season_num]
     ```
   - Example DMM direct links:
     - Movie: `https://debridmediamanager.com/movie/{imdb_id}`
     - Series: `https://debridmediamanager.com/show/{imdb_id}`
     - Search: `https://debridmediamanager.com/search?query={title}`

## 4. Smart Subtitle Finder & Synchronizer (SubDL, SubSource, Sub-Scene)

The system includes a dedicated Subtitle Manager script:
`C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py` configured via `C:\Users\9jhp9\.gemini\config\subtitles.json`.

### Supported Subtitle Sources:
1. **SubDL:** High-quality subtitles with official release tagging.
   - API Key URL: https://subdl.com/panel/api
2. **SubSource:** Comprehensive subtitle repository with release type filtering.
   - API Key URL: https://subsource.net/dashboard/api
3. **Sub-Scene:** Web-scraped community subtitles (no API key required).
   - Website: https://sub-scene.com/
4. **Stremio OpenSubtitles v3 Pro:** Direct community endpoint with WebVTT output auto-converted to SRT.
   - Endpoint: https://opensubtitlesv3-pro.dexter21767.com/

### Subtitle Workflow:
1. **Searching & Release Matching:**
   - Always match the video source format: if the video is `BluRay` / `REMUX`, pick subtitles tagged with `BluRay` / `BDRip` (e.g. SPARKS, LOST). If `WEB-DL`, pick `WEB-DL` tags.
   - Prioritize top community translators (e.g. Dr. Ali Talal, Ahmed Elsayed, Nizar Ezzeddine, etc.).
   ```powershell
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py" search "<movie_title>" "<imdb_id>" ar bluray
   ```

2. **Handling "Other" & Unclassified Subtitles:**
   - In cases where a subtitle is categorized as `Other` or lacks an explicit release tag (common with older or less popular movies where only a single subtitle exists), `subtitle_manager.py` intelligently deduces the release type from filename patterns (`bluray`, `remux`, `web`, `sparks`, `lost`, `rarbg`, `yts`, etc.).
   - If completely unclassified, the subtitle is never discarded; it is accepted as a valid candidate, downloaded, inspected for timestamp alignment with `ffprobe` against the movie's duration, and automatically synced with `AutoSubSync` if any timing drift is found.

3. **Encoding Auto-Conversion (UTF-8 with BOM):**
   - Downloaded Arabic subtitles frequently use Windows-1256, which can render as gibberish on modern TVs and media players.
   - `subtitle_manager.py` automatically detects and re-encodes subtitle files into `UTF-8 with BOM` (`utf-8-sig`) upon download.

4. **Timing Verification against Embedded Video Subtitles (When Torrent Lacks Arabic):**
   - When a torrent or video contains no Arabic audio/subtitles, `subtitle_manager.py` examines the embedded dialogue streams (such as English PGS/SRT tracks) directly from the video file using `ffprobe`.
   - It performs cross-correlation histogram analysis across early and mid-movie dialogues to calculate:
     - **Offset:** Initial delay/lead in seconds (e.g. from studio intro variations).
     - **Drift:** Timing shift across time (detecting 23.976fps vs 24/25fps mismatch).
   - If a constant offset is found, it can auto-shift the SRT timestamps directly with `--fix`.
   - If frame rate drift is found, it automatically engages `AutoSubSync.exe`.
   ```powershell
   # Compare timing and diagnose sync
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py" compare "<video_path>" "<subtitle_path>" [--fix]

   # Shift subtitle timestamps by specific seconds (+ for delay, - for advance)
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py" shift "<subtitle_path>" <offset_seconds> [out_path]

   # Full audio/video synchronization using AutoSubSync
   python "C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py" sync "<video_path>" "<subtitle_path>"
   ```

5. **Lossless Auto-Muxing (Embed Subtitle into MKV Container):**
   - Merges the downloaded `.srt` directly into the `.mkv` container as an internal selectable soft-sub track in 2-3 seconds using ffmpeg stream copy:
     ```powershell
     python "C:\Users\9jhp9\.gemini\config\skills\torrentx\subtitle_manager.py" mux "<video_path>" "<subtitle_path>"
     ```

## 5. Instant Playback (Stream without Downloading) & High-Speed Aria2 Downloads

The agent provides the user two explicit choices when resolving any movie/series:

### Option A: Direct Cloud Streaming (بدون تحميل)
Stream directly from Real-Debrid into the user's preferred media player (`potplayer`, `mpc`, `mpv`, or `vlc`):
1. Probes the stream in 1-2 seconds with `ffprobe` to check for embedded Arabic subtitles.
2. If Arabic subtitle is already embedded -> Plays directly.
3. If Arabic subtitle is missing -> Automatically searches SubDL, Subsource, and Sub-Scene, downloads the top matching subtitle, converts encoding to `UTF-8 with BOM`, and loads it directly into the player.

```powershell
# Direct stream to PotPlayer (with auto-subtitle detection/download):
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" stream "<debrid_url_or_path>" potplayer "<movie_title>" "<imdb_id>" [bluray|web]

# Direct stream to MPC-HC (Media Player Classic):
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" stream "<debrid_url_or_path>" mpc "<movie_title>" "<imdb_id>" [bluray|web]

# Direct stream to MPV (Harbor's internal MPV engine):
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" stream "<debrid_url_or_path>" mpv "<movie_title>" "<imdb_id>" [bluray|web]

# Direct stream to VLC:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" stream "<debrid_url_or_path>" vlc "<movie_title>" "<imdb_id>" [bluray|web]
```

### Option B: High-Speed Multi-Connection Download via Aria2 (تحميل بأقصى سرعة)
Downloads the file to `C:\Users\9jhp9\Downloads` using Aria2 with 16 simultaneous segmented connections and resume support:
```powershell
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\debrid_manager.py" download "<debrid_url>" aria2
```
- Also prepares and places the matching Arabic subtitle in the download folder automatically so the movie is ready to watch upon completion.

## 6. Weekly Episodic Auto-Downloader & Private Trackers (qBittorrent Seeding)

Managed by `C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py` and configured in `C:\Users\9jhp9\.gemini\config\tracked_series.json`.

### 1. Automatic Weekly Episode Tracking:
- Tracks weekly TV series and airing anime.
- Auto-detects air days and schedules via TVmaze / AniList, or monitors custom release windows (e.g. Wednesday between 14:00 and 17:00).
- When a new episode drops:
  - If **Debrid mode**: Automatically downloads with Aria2 (16 connections), probes for embedded Arabic subtitles, searches and downloads matching subtitles from SubDL / Subsource / Sub-Scene. If no subtitle exists anywhere yet, keeps the episode without subtitles for user.
  - Automatically increments episode count (`last_downloaded_episode`).

```powershell
# Track a new show (auto-detect schedule or specify window):
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" add "<show_title>" [season] [last_downloaded_ep] [debrid|private] [tracker_name] ["14:00 - 17:00"]

# Example:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" add "House of the Dragon" 2 4 debrid none "14:00 - 18:00"

# Run check on all tracked series:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" check

# View tracked shows:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" list
```

### 2. Private Trackers Support (Ratio & Seeding Protected):
- **Notice on Private Trackers & Debrid:** Most private trackers (such as TorrentLeech, Gazelle, IPTorrents, etc.) strictly forbid Debrid services and may ban your account because Debrid cannot maintain seed ratios or validate passkeys. Users should always check their tracker's rules / FAQ or ask staff/admins for clarification.
- To keep the user's account completely safe, `torrentx` automatically bypasses all Debrid services for private trackers:
- Uses the tracker's RSS feed / download link with the user's private passkey.
- Downloads the `.torrent` file directly and sends it to **qBittorrent** (`C:\Program Files\qBittorrent\qbittorrent.exe`) for official seeding and downloading.

```powershell
# Register a private tracker:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" add-tracker "<tracker_name>" "<rss_feed_url>" [passkey]

# Track a show using the private tracker:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\series_monitor.py" add "<show_title>" <season> <last_ep> private "<tracker_name>"
```

## 7. Intelligent Media Organizing & Plex / Jellyfin Standard Library Renamer

Managed by `C:\Users\9jhp9\.gemini\config\skills\torrentx\media_organizer.py` and configured in `C:\Users\9jhp9\.gemini\config\media_organizer.json`.

### 1. Features:
- **Clean Naming Styles (أنماط التسمية الاحترافية):**
  - **`clean` (Default / احترافي ومختصر):**
    - Movies: `Movie Name (Year).mkv`
    - Series: `Show Name - S01E05.mkv`
  - **`standard` (احترافي مع اسم الحلقة):**
    - Series: `Show Name - S01E05 - Episode Title.mkv` (titles fetched from TVmaze)
  - **`detailed` (احترافي كامل مع الجودة):**
    - Movies: `Movie Name (Year) [2160p Remux].mkv`
    - Series: `Show Name - S01E05 - Episode Title [2160p WEB-DL].mkv`
- **Automatic Companion Subtitle (.srt) Pairing:**
  - Renames and moves matching subtitles alongside the video (e.g. `Show - S01E05.ar.srt`) so Plex / Infuse / Jellyfin / Apple TV instantly detect it.
- **Custom Destination Drive / Folder:**
  - Users can specify any target hard drive or folder (e.g. `D:\Movies`, `E:\Series`, `F:\PlexMedia\`) permanently or per conversational request.
- **Official Folder Hierarchy:**
  - Movies: `<Target_Dir>\<Movie Title (Year)>\<Movie Title (Year)>.mkv`
  - Series: `<Target_Dir>\<Show Title>\Season 01\<Show Title - S01E05>.mkv`

```powershell
# Set default drives/folders and naming style:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\media_organizer.py" config "D:\Movies" "D:\Series" clean

# Organize and move a completed movie/episode:
python "C:\Users\9jhp9\.gemini\config\skills\torrentx\media_organizer.py" organize "<video_path>" [target_dir] [clean|standard|detailed] [sub_path]
```



