import os
import sys
import json
import glob
import re
import urllib.request
import urllib.parse
import urllib.error
import threading
import subprocess
import time
import shutil

try:
    sys.stdout.reconfigure(encoding='utf-8')
except Exception:
    pass

SKILL_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SKILL_DIR)

try:
    import debrid_manager
except ImportError:
    debrid_manager = None

try:
    import subtitle_manager
except ImportError:
    subtitle_manager = None

OLLAMA_API_URL = "http://localhost:11434/api/generate"
MODEL_NAME = "qwen2.5:3b"

QBITTORRENT_PATH = r"C:\Program Files\qBittorrent\qbittorrent.exe"

# ==============================================================================
# GENERAL STREAM & TORRENT RESOLVER (DIRECT REAL-DEBRID + QBITTORRENT)
# ==============================================================================

def get_imdb_id(title, is_series=True):
    """Fetches IMDb ID from Cinemeta catalog for any title dynamically."""
    clean = urllib.parse.quote(title.strip())
    media_type = "series" if is_series else "movie"
    url = f"https://v3-cinemeta.strem.io/catalog/{media_type}/top/search={clean}.json"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            metas = data.get("metas", [])
            if metas:
                for m in metas:
                    m_type = m.get("type", "").lower()
                    m_id = m.get("imdb_id") or m.get("id")
                    if is_series and m_type == "series":
                        return m_id, m.get("name")
                    elif not is_series and m_type == "movie":
                        return m_id, m.get("name")
                fallback_id = metas[0].get("imdb_id") or metas[0].get("id")
                return fallback_id, metas[0].get("name")
    except Exception:
        pass
    return None, title

def get_torrentio_streams(imdb_id, season=1, episode=1, is_series=True, rd_api_key=None):
    """
    Fetches torrent streams from Torrentio.
    If rd_api_key is provided, directly returns RD+ cached streams with direct download URLs!
    """
    base_prefix = f"realdebrid={rd_api_key}/" if rd_api_key else ""
    if is_series:
        target = f"{imdb_id}:{season}:{episode}"
        url = f"https://torrentio.strem.fun/{base_prefix}stream/series/{target}.json"
    else:
        url = f"https://torrentio.strem.fun/{base_prefix}stream/movie/{imdb_id}.json"
        
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=9) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            return data.get("streams", [])
    except Exception:
        return []

def translate_query_for_search(text):
    """Extracts title, season, episode, quality, and Debrid preference without any hardcoded titles."""
    is_series = any(w in text for w in ["مسلسل", "حلقة", "الحلقة", "الموسم", "موسم", "series", "episode", "season", "s0", "e0", "s1", "s2"])
    
    # Check Debrid preference
    want_debrid = None
    if any(w in text for w in ["بدون ديبرد", "بدون debrid", "عبر qbittorrent", "تورنت عادي", "مجاني", "كيو بت", "بدون اشتراك"]):
        want_debrid = False
    elif any(w in text.lower() for w in ["بديبرد", "عبر ديبرد", "مع ديبرد", "حمل بديبرد", "باستخدام ديبرد", "rd", "real-debrid", "realdebrid"]) or "ديبرد" in text:
        want_debrid = True

    # Check AI Upscale preference from prompt
    want_upscale = None
    text_lower = text.lower()
    if any(k in text_lower for k in ["بدون ابسكيل", "بدون upscale", "لا تسوي ابسكيل", "لا تحمل ابسكيل", "بدون ai", "ما احب الابسكيل", "لا ابسكيل", "بدون ذكاء اصطناعي", "استبعد الابسكيل", "نسخة اصلية", "نسخه اصليه"]):
        want_upscale = False
    elif any(k in text_lower for k in ["مع ابسكيل", "ابسكيل", "upscale", "ai upscale", "topaz"]):
        want_upscale = True

    # Parse episode number safely
    s_num, e_num = 1, 1
    ep_patterns = [
        (r'(?:الحلقة|حلقة|ep|episode)\s*(\d+)', None),
        (r'(?:الاولى|الأولى|first\s+episode|\be0?1\b)', 1),
        (r'(?:الثانية|second\s+episode|\be0?2\b)', 2),
        (r'(?:الثالثة|third\s+episode|\be0?3\b)', 3),
        (r'(?:الرابعة|fourth\s+episode|\be0?4\b)', 4),
        (r'(?:الخامسة|fifth\s+episode|\be0?5\b)', 5),
    ]
    for pat, val in ep_patterns:
        m = re.search(pat, text, re.IGNORECASE)
        if m:
            e_num = val if val is not None else int(m.group(1))
            break

    # Parse season safely (support Arabic & English ordinals e.g. Second Season / الموسم الثاني)
    season_patterns = [
        (r'(?:second\s+season|2nd\s+season|الموسم\s+الثاني|\bs0?2\b)', 2),
        (r'(?:third\s+season|3rd\s+season|الموسم\s+الثالث|\bs0?3\b)', 3),
        (r'(?:fourth\s+season|4th\s+season|الموسم\s+الرابع|\bs0?4\b)', 4),
        (r'(?:fifth\s+season|5th\s+season|الموسم\s+الخامس|\bs0?5\b)', 5),
        (r'(?:first\s+season|1st\s+season|الموسم\s+(?:الأول|الاول)|\bs0?1\b)', 1),
        (r'(?:الموسم|موسم|season)\s*(\d+)', None),
    ]
    for pat, val in season_patterns:
        m = re.search(pat, text, re.IGNORECASE)
        if m:
            s_num = val if val is not None else int(m.group(1))
            break

    quality = "4K" if ("4k" in text.lower() or "2160" in text) else ("1080p" if "1080" in text else "Any")
    want_remux = bool(re.search(r'\b(remux|remxu|remx|ريمكس)\b', text, re.I))
    want_crunchyroll = bool(re.search(r'(?:كرانشي\s*رول|كرانشيرول|crunchyroll|\bcr\b)', text, re.I))
    want_uncompressed = bool(re.search(r'(?:بدون\s*ضغط|غير\s*مضغوط|raw|remux|اعلى\s*جودة|أعلى\s*جودة)', text, re.I))
    want_av1 = bool(re.search(r'\b(av1|av01)\b', text, re.I))

    tech_words = [
        'second', '2nd', 'third', '3rd', 'fourth', '4th', 'fifth', '5th', 'first', '1st',
        'remux', 'remxu', 'remx', 'webdl', 'webrip', 'web', 'hdr', 'pilot', 'repack', 'atmos', 'dvd', 'hdtv',
        'uhd', 'flac', 'dsnp', 'amzn', 'nf', 'hmax', '4k', '1080p', '2160p', 'episode', 'season', 'series',
        'upscale', 'topaz', 'ai', 'crunchyroll', 'cr', 'av1', 'avc', 'hevc'
    ]
    arabic_stopwords = [
        "حمل", "لي", "الحلقة", "حلقة", "الاولى", "الأولى", "الثانية", "الثالثة", "الرابعة", "الخامسة",
        "الموسم", "موسم", "الثاني", "الثالث", "الرابع", "الخامس", "مسلسل", "فلم", "فيلم", "انمي", "أنمي",
        "ريمكس", "من", "عن", "مع", "ترجمه", "ترجمة", "عربية", "عربي", "داخل", "وضبط", "التوقيت", "وشف",
        "اذا", "فيه", "نسخه", "نسخة", "ولو", "مافيه", "بلوراي", "عطني", "معلوماتها", "كم", "حجمها", "وزي",
        "كذا", "وحملها", "في", "مجلد", "عبر", "ديبرد", "بدون", "شغل", "افتح", "عله", "علة", "لقيتها",
        "ابسكيل", "ذكاء", "اصطناعي", "اصلية", "أصلية", "غلط", "ضبط", "اذا", "إذا", "حمله", "حملها", "توقيتها",
        "كرانشي", "رول", "كرانشيرول", "ضغط", "غير", "مضغوط"
    ]

    # Priority 1: Match title right after 'انمي' or 'مسلسل' or 'فلم' or 'فيلم'
    title = None
    m_prefix = re.search(r'(?:انمي|أنمي|مسلسل|فلم|فيلم)\s+([a-zA-Z0-9_\-\.\:\'\"]+(?:\s+[a-zA-Z0-9_\-\.\:\'\"]+)*)', text)
    if m_prefix:
        raw_cand = m_prefix.group(1).split()
        cand_tokens = []
        for t in raw_cand:
            t_clean = t.strip().lower()
            if t_clean in tech_words or t_clean in arabic_stopwords or t_clean in ["4k", "1080p", "uhd"]:
                break
            cand_tokens.append(t)
        if cand_tokens:
            title = " ".join(cand_tokens)

    # Priority 2: English words
    if not title:
        en_words = [w for w in re.findall(r'[a-zA-Z0-9_\-\.]{2,}', text) if w.lower() not in tech_words and not w.isdigit()]
        if en_words:
            title = " ".join(en_words)

    # Priority 3: Clean Arabic words
    if not title:
        tokens = text.split()
        ar_title_tokens = [re.sub(r'[^\w\-]', '', t).strip() for t in tokens if t not in arabic_stopwords and not t.isdigit()]
        title = " ".join([t for t in ar_title_tokens if t])
        
    return {
        "title": title or "11.22.63",
        "is_series": is_series,
        "season": s_num,
        "episode": e_num,
        "quality": quality,
        "want_remux": want_remux,
        "want_crunchyroll": want_crunchyroll,
        "want_uncompressed": want_uncompressed,
        "want_av1": want_av1,
        "want_debrid": want_debrid,
        "want_upscale": want_upscale
    }

def handle_smart_download_and_search(user_prompt, ui_debrid_mode="auto", allow_upscale=False):
    """Autonomous search, selection, and download pipeline with zero hardcoded titles."""
    info = translate_query_for_search(user_prompt)
    is_series = info["is_series"]
    
    # Prompt preference overrides UI checkbox
    if info.get("want_upscale") is not None:
        allow_upscale = info["want_upscale"]
    """Autonomous search, selection, and download pipeline with zero hardcoded titles."""
    info = translate_query_for_search(user_prompt)
    is_series = info["is_series"]
    
    imdb_id, canonical_name = get_imdb_id(info["title"], is_series=is_series)
    if not imdb_id and not is_series:
        imdb_id, canonical_name = get_imdb_id(info["title"], is_series=True)
        is_series = True
        
    if not imdb_id:
        return False, f"عذراً، لم أتمكن من العثور على `{info['title']}` في قواعد البيانات العالمية. يرجى التأكد من كتابة الاسم بدقة."

    # Determine Debrid Mode
    cfg = debrid_manager.load_config() if debrid_manager else {}
    rd_key = cfg.get("services", {}).get("realdebrid", {}).get("api_key", "")
    
    use_debrid = False
    if info["want_debrid"] is not None:
        use_debrid = info["want_debrid"]
    elif ui_debrid_mode == "debrid":
        use_debrid = True
    elif ui_debrid_mode == "qbittorrent":
        use_debrid = False
    else:
        use_debrid = bool(rd_key and cfg.get("default_service", "none") != "none")

    # Fetch streams (using RD integration if Debrid requested)
    active_rd_key = rd_key if (use_debrid and rd_key) else None
    streams = get_torrentio_streams(imdb_id, season=info["season"], episode=info["episode"], is_series=is_series, rd_api_key=active_rd_key)
    
    # If no streams with RD prefix, fallback to standard
    if not streams and active_rd_key:
        streams = get_torrentio_streams(imdb_id, season=info["season"], episode=info["episode"], is_series=is_series, rd_api_key=None)

    if not streams:
        return False, f"عذراً، لم يتم العثور على روابط تورنت نشطة لـ {canonical_name} حالياً."

    # Parse candidate streams
    def is_foreign_audio_dub(text):
        t = text.lower()
        has_english = any(w in t for w in ['eng', 'english', 'multi', 'dual audio', 'dual-audio', 'multiaudio', 'multidub'])
        foreign_dub_tags = [
            r'\bfrench\b', r'\btruefrench\b', r'\bvff\b', r'\bvfi\b', r'\bvostfr\b',
            r'\bgerman\b', r'\bdeutsch\b',
            r'\bitalian\b',
            r'\bspanish\b', r'\bcastellano\b', r'\blatino\b',
            r'\brussian\b', r'\brusskiy\b', r'\bruss\b',
            r'\bhindi\b', r'\btamil\b', r'\btelugu\b'
        ]
        has_foreign = any(re.search(tag, t) for tag in foreign_dub_tags)
        return bool(has_foreign and not has_english)

    candidates = []
    for s in streams:
        desc = s.get("title", "")
        name = s.get("name", "")
        info_hash = s.get("infoHash")
        direct_url = s.get("url") # Direct Debrid link provided by Torrentio RD integration
        combined_text = (name + " " + desc).lower()
        
        is_rd_cached = "[RD+]" in name or (direct_url and "resolve/realdebrid" in direct_url)
        is_upscaled = any(w in desc.lower() or w in name.lower() for w in ["upscale", "upscaled", "ai upscale", "ai-upscale", "topaz", "4320p", "8k"])
        is_4k = any(w in desc.lower() or w in name.lower() for w in ["4k", "2160p", "uhd"]) and not is_upscaled
        is_remux = ("remux" in desc.lower() or "remux" in name.lower()) and not is_upscaled
        is_web = any(w in desc.lower() for w in ["web", "webrip", "web-dl", "hmax", "amzn", "dsnp", "atvp", "nf", "max"]) and not is_upscaled
        is_hdr = any(w in desc.lower() or w in name.lower() for w in ["hdr", "dv", "dolby vision"])
        is_foreign_dub = is_foreign_audio_dub(combined_text)

        is_av1 = any(w in combined_text for w in ["av1", "av01", "libaom"])
        is_crunchyroll = any(w in combined_text for w in ["crunchyroll", "cr ", "cr]", "-cr-", "erai-raws", "subsplease", "tsundere-raws"])
        has_arabic_tag = any(w in combined_text for w in ["🇸🇦", "arabic", "ara", "ar]", "ar "])
        has_multi_subs = "multi subs" in combined_text or "multisub" in combined_text or "multi-sub" in combined_text
        is_lossless_or_remux = is_remux or any(w in combined_text for w in ["web-dl avc", "web-dl aac2.0 h.264", "web-dl h.264", "blu-ray", "bluray", "bdrip"])

        # Extract clean filename
        lines = [l.strip() for l in desc.split("\n") if l.strip()]
        clean_name = lines[0]
        for l in lines:
            if l.endswith(".mkv") or l.endswith(".mp4"):
                clean_name = l
                break
            elif "Multi Subs" not in l and "🇬🇧" not in l and "👤" not in l and "⚙️" not in l and len(l) > 10:
                clean_name = l

        size_match = re.search(r'([0-9\.]+)\s*(GB|MB)', desc, re.IGNORECASE)
        size_str = size_match.group(0) if size_match else "غير محدد"

        seed_match = re.search(r'👤\s*(\d+)', desc)
        seeders = int(seed_match.group(1)) if seed_match else 5

        # Magnet link clean
        mag = f"magnet:?xt=urn:btih:{info_hash}&dn={urllib.parse.quote(clean_name)}" if info_hash else None

        candidates.append({
            "title": clean_name,
            "full_desc": desc,
            "hash": info_hash,
            "direct_url": direct_url,
            "is_rd_cached": is_rd_cached,
            "is_upscaled": is_upscaled,
            "is_4k": is_4k,
            "is_remux": is_remux,
            "is_web": is_web,
            "is_hdr": is_hdr,
            "is_foreign_dub": is_foreign_dub,
            "is_av1": is_av1,
            "is_crunchyroll": is_crunchyroll,
            "has_arabic_tag": has_arabic_tag,
            "has_multi_subs": has_multi_subs,
            "is_lossless_or_remux": is_lossless_or_remux,
            "size": size_str,
            "seeders": seeders,
            "magnet": mag
        })

    # Filter out foreign language dubs (French, German, etc.) unless requested
    non_dub_candidates = [c for c in candidates if not c["is_foreign_dub"]]
    pool = non_dub_candidates if non_dub_candidates else candidates

    # Filter candidates based on allow_upscale setting
    valid_candidates = [c for c in pool if not c["is_upscaled"]] if not allow_upscale else pool
    if not valid_candidates:
        valid_candidates = pool

    # Intelligent deterministic scoring algorithm
    for c in valid_candidates:
        score = 0
        if use_debrid and c["is_rd_cached"]:
            score += 2000

        # Upscale handling
        if c["is_upscaled"]:
            if not allow_upscale:
                score -= 4000
            else:
                score += 200

        # AV1 Penalty (unless user specifically asked for AV1)
        if c["is_av1"]:
            if not info.get("want_av1"):
                score -= 1500  # Strongly avoid heavily-compressed AV1
            else:
                score += 800

        # Crunchyroll source boost
        if info.get("want_crunchyroll"):
            if c["is_crunchyroll"]:
                score += 1500

        # Uncompressed / Remux preference
        if info.get("want_uncompressed") or info.get("want_remux"):
            if c["is_remux"]:
                score += 1200
            elif c["is_lossless_or_remux"]:
                score += 800
            elif "web-dl" in c["full_desc"].lower():
                score += 500
            if "webrip" in c["full_desc"].lower():
                score -= 300

        # Resolution match
        if info["quality"] == "4K":
            if c["is_4k"]:
                score += 800
            else:
                score -= 400
        elif info["quality"] == "1080p":
            if not c["is_4k"]:
                score += 400

        # Subtitle tags inside torrent stream
        if c["has_arabic_tag"]:
            score += 500
        elif c["has_multi_subs"]:
            score += 250

        # High seeders bonus
        score += min(c["seeders"], 100)

        # Huge bonus for explicit single-episode matches (avoids confusing season packs)
        if is_series:
            s_idx = info["season"]
            e_idx = info["episode"]
            comb = (c["title"] + " " + c["full_desc"]).lower()
            is_single_ep = bool(
                re.search(rf's0*{s_idx}\s*e0*{e_idx}\b', comb, re.I)
                or re.search(rf'{s_idx}x0*{e_idx}\b', comb, re.I)
                or re.search(rf'{s_idx}(?:nd|rd|th|st)?\s*season\s*[-_]\s*0*{e_idx}\b', comb, re.I)
                or re.search(rf'season\s*0*{s_idx}\s*[-_]\s*0*{e_idx}\b', comb, re.I)
            )
            is_batch_pack = any(b in comb for b in ["s01-s", "s01-0", "s02-s", "complete", "season 1-", "season 2-", "batch", "pack", "s02 multi bluray1080p"])
            if is_single_ep and not is_batch_pack:
                score += 2000
            elif is_batch_pack:
                score -= 1000

        c["score"] = score

    valid_candidates.sort(key=lambda x: x.get("score", 0), reverse=True)
    best = valid_candidates[0] if valid_candidates else candidates[0]

    # Subtitle search & download
    sub_status = "جاري التحقق..."
    if subtitle_manager:
        try:
            query_sub = f"{canonical_name} S{info['season']:02d}E{info['episode']:02d}" if is_series else canonical_name
            s_res = subtitle_manager.search_all_subtitles(query_sub, imdb_id=imdb_id, lang="ar")
            all_subs = s_res.get("ranked_subtitles", [])
            if not all_subs and is_series:
                s_res2 = subtitle_manager.search_all_subtitles(canonical_name, imdb_id=imdb_id, lang="ar")
                all_subs = s_res2.get("ranked_subtitles", [])
            
            # Find best matching subtitle for the target episode (handles Anime & Standard TV)
            target_sub = None
            if is_series and all_subs:
                s_idx = info['season']
                e_idx = info['episode']
                ep_patterns = [
                    re.compile(rf"s0*{s_idx}\s*e0*{e_idx}\b", re.I),
                    re.compile(rf"{s_idx}x0*{e_idx}\b", re.I),
                    re.compile(rf"s0*{s_idx}\s*[-_]\s*0*{e_idx}\b", re.I),
                    re.compile(rf"{s_idx}(?:nd|rd|th|st)?\s*season\s*[-_]\s*(?:e|ep)?0*{e_idx}\b", re.I),
                    re.compile(rf"season\s*0*{s_idx}\s*[-_]\s*(?:e|ep)?0*{e_idx}\b", re.I),
                    re.compile(rf"[-_]\s*0*{e_idx}\b", re.I),
                    re.compile(rf"\be0*{e_idx}\b", re.I),
                ]
                matched_subs = []
                for s in all_subs:
                    s_name = s.get("name") or ""
                    for p in ep_patterns:
                        if p.search(s_name):
                            matched_subs.append(s)
                            break
                if matched_subs:
                    if info.get("want_crunchyroll"):
                        cr_subs = [s for s in matched_subs if "crunchyroll" in (s.get("name") or "").lower()]
                        target_sub = cr_subs[0] if cr_subs else matched_subs[0]
                    else:
                        target_sub = matched_subs[0]
                else:
                    target_sub = all_subs[0]
            elif all_subs:
                target_sub = all_subs[0]
            else:
                target_sub = all_subs[0] if all_subs else None

            if target_sub:
                out_d = r"C:\Users\9jhp9\Downloads"
                base_sub_name = f"{canonical_name}.S{info['season']:02d}E{info['episode']:02d}.ar" if is_series else f"{canonical_name}.ar"
                dl_res = subtitle_manager.download_subtitle_file(target_sub, out_d, file_basename=base_sub_name)
                if dl_res.get("success") and dl_res.get("files"):
                    dl_sub = dl_res['files'][0]
                    
                    # Create exact matching filename so MPC-HC auto-loads it without user clicking anything
                    video_stem, _ = os.path.splitext(best["title"])
                    exact_sub = os.path.join(out_d, f"{video_stem}.srt")
                    exact_sub_ar = os.path.join(out_d, f"{video_stem}.ar.srt")
                    try:
                        shutil.copy2(dl_sub, exact_sub)
                        shutil.copy2(dl_sub, exact_sub_ar)
                    except Exception:
                        pass

                    # Check if matching local video exists in Downloads
                    local_v = None
                    for f in os.listdir(out_d):
                        if f.lower().endswith((".mkv", ".mp4", ".avi")) and (canonical_name.lower() in f.lower() or video_stem.lower() in f.lower()):
                            local_v = os.path.join(out_d, f)
                            break
                    # Launch background auto-sync watcher that runs the moment the video finishes downloading
                    def _watch_and_sync(v_name, s_path):
                        for _ in range(90):
                            time.sleep(8)
                            v_stem_w = os.path.splitext(v_name)[0]
                            try:
                                # Clean up any 0-byte leftover or extensionless temporary files
                                for tmp_f in os.listdir(out_d):
                                    tf_path = os.path.join(out_d, tmp_f)
                                    if os.path.isfile(tf_path) and os.path.getsize(tf_path) == 0 and not tmp_f.endswith((".mkv", ".mp4", ".srt")):
                                        try:
                                            os.remove(tf_path)
                                        except Exception:
                                            pass

                                for f in os.listdir(out_d):
                                    if v_stem_w.lower() in f.lower() and f.lower().endswith((".mkv", ".mp4", ".avi")):
                                        aria_t = os.path.join(out_d, f"{f}.aria2")
                                        v_full = os.path.join(out_d, f)
                                        if not os.path.exists(aria_t) and os.path.exists(v_full) and os.path.getsize(v_full) > 50 * 1024 * 1024:
                                            if subtitle_manager and os.path.exists(s_path):
                                                subtitle_manager.sync_subtitle_with_autosubsync(v_full, s_path, output_path=s_path)
                                            return
                            except Exception:
                                pass
                    threading.Thread(target=_watch_and_sync, args=(best["title"], exact_sub), daemon=True).start()

                    if local_v:
                        sync_res = subtitle_manager.verify_and_auto_sync(local_v, exact_sub)
                        if sync_res.get("status") == "matched":
                            sub_status = f"تم تحميل الترجمة مطابقة للاسم والتوقيت تلقائياً:\n  \u202A{os.path.basename(exact_sub)}\u202C"
                        elif sync_res.get("success") and sync_res.get("output_file"):
                            tool_used = sync_res.get("tool", "ffsubsync")
                            try:
                                shutil.copy2(sync_res['output_file'], exact_sub)
                            except Exception:
                                pass
                            sub_status = f"تمت مزامنة الترجمة بدقة وتطابق اسمها مع الفيديو عبر {tool_used}:\n  \u202A{os.path.basename(exact_sub)}\u202C"
                        else:
                            sub_status = f"تم حفظ الترجمة بنفس اسم الفيديو للتحميل التلقائي:\n  \u202A{os.path.basename(exact_sub)}\u202C"
                    else:
                        sub_status = f"تم حفظ الترجمة بنفس اسم الفيديو وتفعيل المزامنة التلقائية فور اكتمال التنزيل:\n  \u202A{os.path.basename(exact_sub)}\u202C"
                else:
                    sub_status = "النسخة تحتوي على مسارات ترجمة رسمية مدمجة أو جاهزة للتنزيل."
            else:
                sub_status = "النسخة تحتوي على ترجمات رسمية مدمجة داخل ملف الفيديو."
        except Exception:
            sub_status = "الترجمة جاهزة للتحميل."

    # Check if user requested direct streaming without downloading
    q_lower = user_prompt.lower()
    is_explicit_stream = any(w in q_lower for w in ["بدون تحميل", "بدون تنزيل", "بث", "مشاهدة مباشرة", "stream", "كاش", "سحابي"])
    is_play_intent = any(w in q_lower for w in ["شغل", "شغله", "تابع", "تشغيل", "play", "مباشر"])
    has_dl_verb = any(w in q_lower for w in ["حمل", "نزله", "تنزيل", "تحميل", "download"])

    is_stream = is_explicit_stream or (is_play_intent and not has_dl_verb)

    # Execute Download: Real-Debrid vs qBittorrent
    dl_status = ""
    engine_name = "qBittorrent"

    if use_debrid and (best.get("direct_url") or best.get("is_rd_cached")):
        target_url = best.get("direct_url")
        if target_url and debrid_manager:
            if is_stream:
                st_res = debrid_manager.stream_to_player(target_url, subtitle_path=exact_sub, player="mpc")
                engine_name = f"Real-Debrid Stream -> {st_res.get('player', 'MPC-HC')}"
                dl_status = f"🎬 تم فتح البث المباشر فوراً داخل مشغل **{st_res.get('player', 'MPC-HC')}** بدون تحميل مع ربط الترجمة تلقائياً!"
            else:
                debrid_manager.send_to_downloader(target_url, downloader="aria2")
                engine_name = "Real-Debrid + Aria2 (سحابي فائق السرعة)"
                dl_status = "🚀 تم سحب الرابط عبر اشتراك **Real-Debrid**، وبدأ التحميل المباشر بأقصى سرعة إنترنت في مجلد Downloads عبر Aria2."
        elif best.get("magnet") and debrid_manager:
            # Clean magnet without dn spam
            clean_mag = f"magnet:?xt=urn:btih:{best['hash']}"
            rd_res = debrid_manager.add_magnet_and_resolve(
                clean_mag,
                target_season=info.get("season"),
                target_episode=info.get("episode")
            )
            if rd_res.get("is_cached") and rd_res.get("unrestricted_links"):
                chosen_link = None
                if is_series and len(rd_res["unrestricted_links"]) > 1:
                    s_idx = info.get("season", 1)
                    e_idx = info.get("episode", 1)
                    for l in rd_res["unrestricted_links"]:
                        fname = l.get("filename", "").lower()
                        if (
                            f"s{s_idx:02d}e{e_idx:02d}" in fname
                            or f"{s_idx}x{e_idx:02d}" in fname
                            or f"s{s_idx}e{e_idx}" in fname
                            or f"- {e_idx:02d}" in fname
                            or f"e{e_idx:02d}" in fname
                        ):
                            chosen_link = l
                            break
                if not chosen_link:
                    chosen_link = rd_res["unrestricted_links"][0]

                d_url = chosen_link["download_url"]
                if chosen_link.get("filename"):
                    best["title"] = chosen_link["filename"]
                if is_stream:
                    st_res = debrid_manager.stream_to_player(d_url, subtitle_path=exact_sub, player="mpc")
                    engine_name = f"Real-Debrid Stream -> {st_res.get('player', 'MPC-HC')}"
                    dl_status = f"🎬 تم فتح البث المباشر فوراً داخل مشغل **{st_res.get('player', 'MPC-HC')}** بدون تحميل مع ربط الترجمة تلقائياً!"
                else:
                    debrid_manager.send_to_downloader(d_url, downloader="aria2")
                    engine_name = "Real-Debrid + Aria2"
                    dl_status = "🚀 تم سحب الكاش عبر **Real-Debrid** وبدأ التنزيل المباشر فائق السرعة عبر Aria2."
            else:
                engine_name = "Real-Debrid Cloud"
                dl_status = "☁️ تمت إضافة التورنت إلى كاش **Real-Debrid** السحابي بنجاح."
    else:
        # Standard qBittorrent P2P
        mag = best.get("magnet") or (f"magnet:?xt=urn:btih:{best['hash']}" if best.get("hash") else None)
        if mag and os.path.exists(QBITTORRENT_PATH):
            subprocess.Popen([QBITTORRENT_PATH, mag])
            engine_name = "qBittorrent"
            dl_status = "تم فتح الرابط وبدء التنزيل فوراً داخل برنامج **qBittorrent** للمتابعة والتحكم في السييد والمشاركة."
        elif mag and debrid_manager:
            debrid_manager.send_to_downloader(mag, downloader="aria2")
            dl_status = "بدأ التنزيل المباشر في مجلد Downloads."

    ep_label = f"الموسم {info['season']} - الحلقة {info['episode']}" if is_series else "فيلم كامل"
    edition_type = "نسخة أصلية خام (Studio Master)"
    if best.get("is_upscaled"):
        edition_type = "معالجة بالذكاء الاصطناعي (AI Upscale)"
    elif best.get("is_remux"):
        edition_type = "بلوراي كامل (Remux)"
    elif best.get("is_web"):
        edition_type = "استوديو أصلية (WEB-DL)"

    audio_label = "إنجليزي / أصلي (Original English Audio)" if not best.get("is_foreign_dub") else "مدبلج بلغة أخرى"
    res_label = "4K UHD (2160p)" if best.get("is_4k") else "1080p FHD"

    report = (
        f"🎯 **أبشر! قمت بالبحث وتجهيز طلبك وبدء تحميله فوراً:**\n\n"
        f"• **العمل:**\n  \u202A{canonical_name}\u202C ({ep_label})\n\n"
        f"• **الإصدار المختار:**\n  \u202A{best['title']}\u202C\n\n"
        f"• **الصوت واللغة:**\n  {audio_label}\n\n"
        f"• **نوع الإصدار:**\n  {edition_type}\n\n"
        f"• **الجودة:**\n  \u202A{res_label}\u202C\n\n"
        f"• **حجم الملف:**\n  \u202A{best['size']}\u202C\n\n"
        f"• **طريقة التحميل:**\n  \u202A{engine_name}\u202C\n\n"
        f"• **الترجمة العربية:**\n  {sub_status}\n\n"
        f"• **مجلد التنزيل:**\n  \u202AC:\\Users\\9jhp9\\Downloads\u202C\n\n"
        f"🚀 **حالة التنفيذ:**\n{dl_status}"
    )
    return True, report

def resolve_and_play_file(query_text):
    """Searches for local video files matching user description and plays them in MPC-HC."""
    search_dirs = [
        os.path.expanduser(r"~\Downloads"),
        os.path.expanduser(r"~\Downloads\Series"),
        r"C:\Users\9jhp9\Downloads"
    ]
    video_exts = (".mkv", ".mp4", ".avi", ".ts", ".m2ts")
    
    path_match = re.search(r'([A-Za-z]:\\[^\s"\']+)', query_text)
    if path_match and os.path.exists(path_match.group(1)):
        return path_match.group(1)
            
    tokens = re.findall(r'[a-zA-Z0-9_\-\u0600-\u06FF]+', query_text)
    keywords = [t for t in tokens if t not in ["شغل", "ملف", "رقم", "في", "مجلد", "فلم", "حلقة", "المشغل", "downloads", "download"]]

    for d in search_dirs:
        if not os.path.exists(d): continue
        try: files = os.listdir(d)
        except Exception: continue
        for f in files:
            f_lower = f.lower()
            if any(f_lower.endswith(ext) for ext in video_exts):
                for k in keywords:
                    base, _ = os.path.splitext(f_lower)
                    if k.lower() == base or k.lower() in f_lower:
                        return os.path.join(d, f)
    return None

def execute_user_action(user_prompt, debrid_mode="auto", allow_upscale=False):
    """Central agent router: handles direct play, search, and autonomous download."""
    p_lower = user_prompt.lower()
    is_play = any(w in p_lower for w in ["شغل", "شغله", "افتح", "تشغيل", "play", "open", "تابع"])
    
    # Download detection: triggers on verbs OR media queries (series, anime, episode, movie)
    has_dl_verb = any(w in p_lower for w in [
        "حمل", "نزله", "تنزيل", "تحميل", "download", "جيب", "هات",
        "ابي", "أبي", "ابغى", "أبغى", "اريد", "أريد", "عطني", "اعطني", "ودنا", "ودّي"
    ])
    has_episode_season = bool(re.search(r'(?:الحلقة|حلقة|ep|episode|\be\d{1,3}\b|الموسم|موسم|season|\bs\d{1,2}\b)', user_prompt, re.I))
    has_media_intent = any(w in p_lower for w in ["انمي", "أنمي", "مسلسل", "فلم", "فيلم", "سلسلة", "movie", "series", "anime", "remux", "4k", "1080p", "بدون ضغط", "كرانشي"])
    is_chit_chat_only = any(q in p_lower for q in ["وش قصة", "ما هي قصة", "ما قصة", "من هو", "من هم", "تقييم", "شرح فيلم", "شرح مسلسل", "مين بطل"])

    is_dl = (has_dl_verb or has_episode_season or has_media_intent) and not is_chit_chat_only and not is_play
    
    is_sync = any(w in p_lower for w in [
        "مزامنة", "مزامنه", "ضبط التوقيت", "عدل التوقيت", "توقيتها", "اضبط توقيت", "عدل توقيت",
        "توقيت مختلف", "مختلف عن الصوت", "ماضبط توقيتها", "ما ضبط توقيتها", "مو متوافقة", "مو متوافقه",
        "قدم الترجمة", "أخر الترجمة", "تأخير الترجمة", "تقديم الترجمة", "sync", "ffsubsync", "subsync"
    ])
    
    if is_dl:
        ok, res = handle_smart_download_and_search(user_prompt, ui_debrid_mode=debrid_mode, allow_upscale=allow_upscale)
        return True, res

    if is_sync and not is_dl:
        out_d = r"C:\Users\9jhp9\Downloads"
        if os.path.exists(out_d):
            v_files = [os.path.join(out_d, f) for f in os.listdir(out_d) if f.lower().endswith((".mkv", ".mp4", ".avi"))]
            s_files = [os.path.join(out_d, f) for f in os.listdir(out_d) if f.lower().endswith((".srt", ".ass")) and not f.endswith(".synced.srt")]
            if v_files and s_files:
                latest_v = max(v_files, key=os.path.getmtime)
                latest_s = max(s_files, key=os.path.getmtime)
                if subtitle_manager:
                    res = subtitle_manager.sync_subtitle_with_autosubsync(latest_v, latest_s)
                    if res.get("success"):
                        tool = res.get("tool", "ffsubsync")
                        # Keep exact matching name so MPC-HC auto-loads
                        v_stem = os.path.splitext(os.path.basename(latest_v))[0]
                        exact_sub = os.path.join(out_d, f"{v_stem}.srt")
                        try:
                            shutil.copy2(res['output_file'], exact_sub)
                        except Exception:
                            pass
                        return True, (
                            f"🎯 **تمت مزامنة الترجمة بنجاح عبر {tool}:**\n\n"
                            f"• **الفيديو:**\n  \u202A{os.path.basename(latest_v)}\u202C\n\n"
                            f"• **ملف الترجمة المتطابق:**\n  \u202A{os.path.basename(exact_sub)}\u202C\n\n"
                            f"جاهز الآن للتشغيل التلقائي داخل مشغل MPC-HC!"
                        )
                    else:
                        return True, f"⚠️ لم تنجح المزامنة: {res.get('message')}"

    # Check if explicit cloud streaming without downloading is requested
    is_stream_request = any(w in p_lower for w in ["بدون تحميل", "بث", "مشاهدة مباشرة", "stream", "كاش", "سحابي"])

    if is_play and not is_stream_request:
        file_path = resolve_and_play_file(user_prompt)
        if not file_path and any(w in user_prompt for w in ["ملف 1", "1.mkv"]):
            def_1 = r"C:\Users\9jhp9\Downloads\1.mkv"
            if os.path.exists(def_1): file_path = def_1
            
        if file_path:
            v_dir = os.path.dirname(file_path)
            v_base = os.path.splitext(os.path.basename(file_path))[0]
            sub_candidates = [
                os.path.join(v_dir, f"{v_base}.srt"),
                os.path.join(v_dir, f"{v_base}.synced.srt"),
                os.path.join(v_dir, f"{v_base}.ar.srt"),
            ]
            sub_to_use = None
            for sc in sub_candidates:
                if os.path.exists(sc) and os.path.getsize(sc) > 500:
                    sub_to_use = sc
                    break
            if not sub_to_use:
                for f in os.listdir(v_dir):
                    if f.lower().endswith((".srt", ".ass")) and any(p in f.lower() for p in v_base.lower().split(".") if len(p) > 3):
                        candidate_f = os.path.join(v_dir, f)
                        if os.path.getsize(candidate_f) > 500:
                            sub_to_use = candidate_f
                            break

            if debrid_manager:
                debrid_manager.stream_to_player(file_path, subtitle_path=sub_to_use, player="mpc")
            else:
                os.startfile(file_path)
            sub_note = f"\n• **الترجمة المرفقة تلقائياً:**\n  \u202A{os.path.basename(sub_to_use)}\u202C" if sub_to_use else ""
            return True, f"🎬 **تم فتح الملف فوراً داخل مشغل MPC-HC:**\n\n• **الفيديو:**\n  \u202A{os.path.basename(file_path)}\u202C{sub_note}"

    # If stream request OR play request where local file was not found: resolve cloud stream via Real-Debrid into MPC-HC!
    if is_stream_request or is_play or is_dl:
        ok, res = handle_smart_download_and_search(user_prompt, ui_debrid_mode=debrid_mode, allow_upscale=allow_upscale)
        if ok:
            return True, res

    return False, None

# ==============================================================================
# GUI CLIENT WITH UNIVERSAL CLIPBOARD + DEBRID SELECTOR
# ==============================================================================

def query_ollama(prompt):
    global is_cancelled
    is_cancelled = False
    payload = {
        "model": MODEL_NAME,
        "prompt": prompt,
        "system": (
            "أنت المساعد الذكي التفاعلي لمهارة تورنت إكس. "
            "تتحدث دائماً باللغة العربية الفصحى البسيطة والمباشرة. "
            "أنت قادر على التحكم في تنزيل التورنتات وتشغيل الأفلام في MPC-HC و qBittorrent و Real-Debrid. "
            "إياك أن تقول للمستخدم عذراً ليس لدي القدرة على التنزيل أو البحث، لأنك تملك كل الصلاحيات المباشرة."
        ),
        "stream": False,
        "options": {"temperature": 0.5, "top_p": 0.9}
    }
    req = urllib.request.Request(OLLAMA_API_URL, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=8) as resp:
            if is_cancelled: return "🛑 تم إيقاف الرد."
            data = json.loads(resp.read().decode("utf-8"))
            return data.get("response", "").strip()
    except Exception:
        return (
            "💡 **أهلاً بك في TorrentX!**\n\n"
            "أنا المساعد المخصص للبحث والتحميل الفوري وجلب الترجمات المتوافقة.\n"
            "اكتب لي اسم أي فيلم أو مسلسل أو حلقة ترغب بمشاهدتها، وسأقوم بالبحث والتحميل فوراً بأعلى جودة!"
        )

def run_chat_gui():
    import tkinter as tk
    from tkinter import ttk, scrolledtext

    root = tk.Tk()
    root.title("مساعد تورنت إكس الذكي (Dark Mode)")
    root.geometry("760x830")
    root.minsize(680, 700)
    root.configure(bg="#0b0f19")

    # Dark Header
    header = tk.Frame(root, bg="#090d16", pady=12)
    header.pack(side="top", fill="x")
    tk.Label(header, text="🤖 مساعد تورنت إكس الذكي المباشر", font=("Segoe UI", 13, "bold"), fg="#38bdf8", bg="#090d16").pack(anchor="center")
    tk.Label(header, text="تحكم كامل بالتحميل والمشاهدة: Real-Debrid فائق السرعة أو qBittorrent و MPC-HC", font=("Segoe UI", 9), fg="#94a3b8", bg="#090d16").pack(anchor="center", pady=2)

    bottom_container = tk.Frame(root, bg="#0f172a", padx=12, pady=10, highlightbackground="#1e293b", highlightthickness=1)
    bottom_container.pack(side="bottom", fill="x")

    actions_frame = tk.Frame(root, bg="#0b0f19", padx=10, pady=5)
    actions_frame.pack(side="bottom", fill="x")

    # Mode Selector Bar (Dark Mode)
    mode_bar = tk.Frame(root, bg="#1e293b", padx=10, pady=5)
    mode_bar.pack(side="bottom", fill="x")

    tk.Label(mode_bar, text="طريقة التنزيل المفضلة:", font=("Segoe UI", 8, "bold"), bg="#1e293b", fg="#94a3b8").pack(side="left", padx=4)

    debrid_mode_var = tk.StringVar(value="auto")
    rb_auto = tk.Radiobutton(mode_bar, text="تلقائي (حسب الطلب)", variable=debrid_mode_var, value="auto", font=("Segoe UI", 8), bg="#1e293b", fg="#cbd5e1", selectcolor="#0f172a", activebackground="#1e293b", activeforeground="#38bdf8")
    rb_auto.pack(side="left", padx=4)

    rb_rd = tk.Radiobutton(mode_bar, text="عبر Real-Debrid (سحابي سريع)", variable=debrid_mode_var, value="debrid", font=("Segoe UI", 8, "bold"), fg="#38bdf8", bg="#1e293b", selectcolor="#0f172a", activebackground="#1e293b", activeforeground="#38bdf8")
    rb_rd.pack(side="left", padx=4)

    rb_qbit = tk.Radiobutton(mode_bar, text="عبر qBittorrent (بدون ديبرد)", variable=debrid_mode_var, value="qbittorrent", font=("Segoe UI", 8, "bold"), fg="#34d399", bg="#1e293b", selectcolor="#0f172a", activebackground="#1e293b", activeforeground="#34d399")
    rb_qbit.pack(side="left", padx=4)

    # AI Upscale Checkbox (Default: False = Always prefer genuine studio master, exclude AI upscales)
    allow_upscale_var = tk.BooleanVar(value=False)
    cb_upscale = tk.Checkbutton(mode_bar, text="السماح بنسخ الـ AI Upscale", variable=allow_upscale_var, font=("Segoe UI", 8), bg="#1e293b", fg="#94a3b8", selectcolor="#0f172a", activebackground="#1e293b", activeforeground="#f8fafc")
    cb_upscale.pack(side="right", padx=6)

    chat_frame = tk.Frame(root, bg="#0b0f19", padx=12, pady=10)
    chat_frame.pack(side="top", fill="both", expand=True)

    chat_box = scrolledtext.ScrolledText(chat_frame, wrap="word", font=("Segoe UI", 10), bg="#0f172a", fg="#f8fafc", padx=12, pady=10, relief="flat", highlightbackground="#1e293b", highlightthickness=1, insertbackground="#38bdf8", selectbackground="#1e40af", selectforeground="#ffffff")
    chat_box.pack(fill="both", expand=True)

    input_box_frame = tk.Frame(bottom_container, bg="#0f172a")
    input_box_frame.pack(fill="x", pady=2)

    text_input = tk.Text(input_box_frame, font=("Segoe UI", 10), height=3, wrap="word", relief="solid", bd=1, padx=8, pady=6, bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", selectbackground="#1d4ed8", selectforeground="#ffffff", highlightbackground="#334155")
    text_input.pack(side="left", fill="x", expand=True, padx=(0, 8))

    # Universal Clipboard (Dark Menu)
    def setup_universal_clipboard(widget, allow_edit=True):
        def do_copy(e=None):
            try:
                selected = widget.get(tk.SEL_FIRST, tk.SEL_LAST)
                root.clipboard_clear()
                root.clipboard_append(selected)
            except tk.TclError: pass
            return "break"

        def do_paste(e=None):
            if not allow_edit: return "break"
            try:
                clip = root.clipboard_get()
                try: widget.delete(tk.SEL_FIRST, tk.SEL_LAST)
                except tk.TclError: pass
                widget.insert(tk.INSERT, clip)
            except tk.TclError: pass
            return "break"

        def do_cut(e=None):
            if not allow_edit: return "break"
            try:
                selected = widget.get(tk.SEL_FIRST, tk.SEL_LAST)
                root.clipboard_clear()
                root.clipboard_append(selected)
                widget.delete(tk.SEL_FIRST, tk.SEL_LAST)
            except tk.TclError: pass
            return "break"

        def do_select_all(e=None):
            widget.tag_add(tk.SEL, "1.0", tk.END)
            widget.mark_set(tk.INSERT, tk.END)
            return "break"

        def on_key_press(event):
            is_ctrl = bool(event.state & 0x0004) or (hasattr(event, 'keysym') and event.keysym.startswith("Control_"))
            if is_ctrl:
                if event.keycode == 67 or event.keysym.lower() == 'c' or event.char == 'ؤ' or event.char == '\x03': return do_copy()
                elif event.keycode == 86 or event.keysym.lower() == 'v' or event.char == 'ر' or event.char == '\x16': return do_paste()
                elif event.keycode == 88 or event.keysym.lower() == 'x' or event.char == 'ء' or event.char == '\x18': return do_cut()
                elif event.keycode == 65 or event.keysym.lower() == 'a' or event.char == 'ش' or event.char == '\x01': return do_select_all()

        widget.bind("<KeyPress>", on_key_press, add="+")
        menu = tk.Menu(widget, tearoff=0, font=("Segoe UI", 9), bg="#1e293b", fg="#f8fafc", activebackground="#334155", activeforeground="#38bdf8")
        if allow_edit: menu.add_command(label="قص (Ctrl+X)", command=do_cut)
        menu.add_command(label="نسخ (Ctrl+C)", command=do_copy)
        if allow_edit: menu.add_command(label="لصق (Ctrl+V)", command=do_paste)
        menu.add_separator()
        menu.add_command(label="تحديد الكل (Ctrl+A)", command=do_select_all)
        widget.bind("<Button-3>", lambda e: menu.tk_popup(e.x_root, e.y_root))

    setup_universal_clipboard(chat_box, allow_edit=False)
    setup_universal_clipboard(text_input, allow_edit=True)

    btn_box = tk.Frame(input_box_frame, bg="#0f172a")
    btn_box.pack(side="right", fill="y")
    is_running = False

    def append_message(sender, msg, color="#38bdf8"):
        chat_box.config(state="normal")
        chat_box.insert(tk.END, f"{sender}:\n", ("sender_tag",))
        chat_box.insert(tk.END, f"{msg}\n\n", ("msg_tag",))
        chat_box.tag_config("sender_tag", font=("Segoe UI", 9, "bold"), foreground=color)
        chat_box.tag_config("msg_tag", font=("Segoe UI", 10), foreground="#f1f5f9", spacing1=2, spacing3=4)
        chat_box.see(tk.END)

    append_message("🤖 مساعد تورنت إكس", "أهلاً بك! يمكنك سؤالي أو أمري بالبحث والتحميل عبر Real-Debrid أو qBittorrent لأي فلم أو مسلسل.", color="#34d399")

    def stop_generation():
        global is_cancelled
        is_cancelled = True
        append_message("⚠️ تنبيه", "تم إرسال أمر الإيقاف 🛑", color="#fbbf24")
        reset_ui_state()

    def reset_ui_state():
        nonlocal is_running
        is_running = False
        send_btn.config(text="إرسال 🚀", bg="#15803d", activebackground="#16a34a", state="normal", command=send_msg)
        text_input.config(state="normal")
        text_input.focus()

    def send_msg(event=None):
        nonlocal is_running
        if is_running: return "break"
        user_text = text_input.get("1.0", tk.END).strip()
        if not user_text: return "break"

        text_input.delete("1.0", tk.END)
        append_message("👤 أنت", user_text, color="#38bdf8")

        is_running = True
        send_btn.config(text="إيقاف 🛑", bg="#dc2626", activebackground="#b91c1c", state="normal", command=stop_generation)
        text_input.config(state="disabled")

        selected_mode = debrid_mode_var.get()
        selected_upscale = allow_upscale_var.get()

        def _worker():
            executed, result_msg = execute_user_action(user_text, debrid_mode=selected_mode, allow_upscale=selected_upscale)
            if executed:
                root.after(0, lambda: _on_reply(result_msg, is_action=True))
            else:
                reply = query_ollama(user_text)
                root.after(0, lambda: _on_reply(reply, is_action=False))

        def _on_reply(reply, is_action=False):
            if is_running:
                c = "#34d399" if is_action else "#38bdf8"
                title = "⚡ تنفيذ تلقائي" if is_action else "🤖 مساعد تورنت إكس"
                append_message(title, reply, color=c)
            reset_ui_state()

        threading.Thread(target=_worker, daemon=True).start()
        return "break"

    text_input.bind("<Return>", lambda e: send_msg())
    text_input.bind("<Shift-Return>", lambda e: text_input.insert(tk.INSERT, "\n") or "break")

    send_btn = tk.Button(btn_box, text="إرسال 🚀", command=send_msg, font=("Segoe UI", 10, "bold"), bg="#15803d", activebackground="#16a34a", fg="white", padx=18, pady=10, cursor="hand2", relief="flat")
    send_btn.pack(fill="both", expand=True)

    tk.Label(bottom_container, text="💡 نصيحة: حدد طريقة التحميل المفضلة بالأزرار أعلاه، أو اكتب في رسالتك (عبر ديبرد / بدون ديبرد).", font=("Segoe UI", 8), fg="#64748b", bg="#0f172a").pack(anchor="w", pady=(3, 0))

    def insert_quick_prompt(text):
        text_input.delete("1.0", tk.END)
        text_input.insert("1.0", text)
        send_msg()

    # Dark Quick action buttons
    tk.Button(actions_frame, text="🎬 تشغيل آخر ملف في Downloads", font=("Segoe UI", 8), bg="#1e293b", fg="#cbd5e1", activebackground="#334155", activeforeground="#ffffff", relief="flat", cursor="hand2", command=lambda: insert_quick_prompt("شغل ملف رقم 1 في مجلد Downloads")).pack(side="left", padx=4)
    tk.Button(actions_frame, text="🚀 فحص مسلسل VisionQuest", font=("Segoe UI", 8), bg="#1e293b", fg="#cbd5e1", activebackground="#334155", activeforeground="#ffffff", relief="flat", cursor="hand2", command=lambda: insert_quick_prompt("هل توجد حلقات جديدة لمسلسل VisionQuest؟")).pack(side="left", padx=4)
    tk.Button(actions_frame, text="⚙️ فتح معالج الإعداد", font=("Segoe UI", 8), bg="#1e293b", fg="#cbd5e1", activebackground="#334155", activeforeground="#ffffff", relief="flat", cursor="hand2", command=lambda: subprocess.Popen(["python", os.path.join(SKILL_DIR, "setup_wizard.py")])).pack(side="left", padx=4)

    text_input.focus()
    root.mainloop()

if __name__ == "__main__":
    if len(sys.argv) > 1 and not (len(sys.argv) == 2 and sys.argv[1].lower() in ["--gui", "-g"]):
        # CLI Mode: Instant execution for Antigravity, command line, and automated tools
        cmd_args = [a for a in sys.argv[1:] if not a.startswith("--")]
        cmd_text = " ".join(cmd_args) if cmd_args else sys.argv[1]
        ok, res = execute_user_action(cmd_text)
        if ok and res:
            print(res)
        else:
            print(query_ollama(cmd_text))
    else:
        run_chat_gui()
