# Music

Read this before searching for an album, a discography or a single. Music is audio only, so most of the request fields made for video do not apply.

## 1. Research the release on the web

MusicBrainz and Discogs list every edition of an album. Note your sources: these are web facts, not torrentx verification.

- **Artist and album title, and the year.** Albums are re-released: an original pressing, remasters, deluxe editions with extra tracks, anniversary editions. Find which one the user means, and its track count.
- **Formats it was published in:** CD (16-bit, 44.1 kHz), hi-res digital (24-bit, 48 to 192 kHz), vinyl. A 24-bit release of an album that was only ever published on CD has been converted, not mastered at that resolution.

## 2. Kinds of release

| Kind | Name signs | Notes |
| --- | --- | --- |
| Lossless CD rip | `FLAC`, `16bit`, `44.1kHz`, `CD`, `Log`, `Cue` | A log and cue show a careful rip. |
| Hi-res | `24bit`, `24-96`, `24-192`, `WEB`, `Hi-Res` | From a hi-res store. |
| Vinyl rip | `Vinyl`, `LP`, `24-96` | Recorded from a record; the resolution is the recording's, not the music's. |
| Lossy | `MP3 320`, `V0`, `AAC`, `OGG` | Smaller; good enough for most listening. |
| Discography | `Discography`, `Complete`, year ranges | Many albums in one torrent, often mixed formats. |

Bigger is not better here either. A 24-bit file of a CD master is larger and sounds the same. A lossless file made from an MP3 (a "fake FLAC") is lossless in name only. Choose the format the user asked for, at the edition they asked for.

## 3. What inspection shows

`find` and `inspect_torrent` inspect an audio file when the torrent has no video. The verified streams give `codec`, `sample_rate_hz`, `sample_bit_depth` (lossless codecs only), `channels`, and the overall bitrate.

- Codec and sample depth confirm FLAC versus MP3, and 16-bit versus 24-bit.
- Inspection cannot detect a lossless file transcoded from a lossy source, or an upsampled one. Say so when the user cares about true lossless or hi-res.
- One track stands for the album. A discography can mix formats between albums: inspect a few files with `sample_files` or chosen `file_ids`.

## 4. From the research to the request

- `search.query`: artist and album, with the year or edition when it matters: `Pink Floyd The Dark Side of the Moon 2023 Remaster`. Use `category: "music"`.
- `analysis.target.title`: the album title; `year` for the edition's year.
- Leave out the video fields (`resolutions`, `codecs`, `formats`); they describe film releases.
- There are no audio-quality constraint fields yet. Check the verified `codec`, `sample_rate_hz` and `sample_bit_depth` against what the user asked for, and say what you checked.
- Use `torrent_files` to check the track list against the edition's track count before recommending a torrent.
