# TV series

Read this before searching for a live-action or Western animated series (anime: [anime.md](anime.md)). Release kinds, HDR and audio are as in [movies.md](movies.md); this file covers what is different for series.

## 1. Research the series on the web

TMDB, TheTVDB and TVmaze list seasons and episodes. Note your sources: these are web facts, not torrentx verification.

- **Title and year:** remakes and same-named shows are common (`The Office` UK 2001, US 2005). Releases often add the year or country: `Doctor Who 2005`, `The Office US`.
- **Seasons and episodes:** how many seasons, episodes per season, and which ones have aired. For a running season, which episode is the latest.
- **Numbering:** releases follow the broadcast order, `S02E05`. Some shows have other orders (DVD order, a streaming service's order) or specials as `S00`. A daily show is numbered by date: `2024.05.13`.
- **Where it streams:** the service that makes the show is usually where the best WEB-DL comes from (`NF`, `AMZN`, `DSNP`, `ATVP`, `MAX`, `HULU`, `PCOK`).
- **Disc releases:** whether the season is out on Blu-ray, and in 4K.

## 2. Episodes and packs

| Coverage | Name signs | When |
| --- | --- | --- |
| One episode | `S02E05` | The user wants that episode, or the season is still airing. |
| Several episodes | `S02E01-E04` | Rarely the best choice. |
| Season pack | `S02`, `Season 2`, `COMPLETE` | A finished season. |
| Several seasons | `S01-S05`, `Complete Series` | The user wants all of it. |

Use `analysis.constraints.coverage` when the user wants one form: `single_episode`, `season_pack`, `multi_season_pack`. A season pack named `COMPLETE` is a claim: check the file list with `torrent_files` before telling the user it has every episode.

`PROPER` and `REPACK` fix a broken earlier release; prefer them.

The largest pack is not the best. Episode size depends on the codec and the source: an HEVC WEB-DL season can be a quarter of an H.264 remux and look close. Compare the inspected episode, not the pack size.

## 3. Inspection of a pack

`find` reads three episodes of each pack by default (first, middle, last) and compares the middle one; extras and bundled films are left out. `consistency.differences` names any episode whose video, audio or subtitles differ from it: groups encode a season the same way, but not always. `coverage` shows which episodes the file list holds; give `season` and the full `episodes` range from your research so `coverage.missing` can name the absent ones. A name saying "Complete" proves nothing; the file list does.

## 4. Sources

General sites carry most series. `eztv` and `eztvx` carry episodes soon after airing. Language-specific sites carry dubs in their language.

## 5. From the research to the request

- `search.query`: the title with the year or country when needed, and the season: `Severance S02`. Use `category: "tv"`.
- `analysis.target`: `title`, `year` if it separates the show from another, `season`, and `episodes` for specific episodes.
- Audio and subtitle languages go in `media.constraints` when required, `media.preferences` when wanted.
