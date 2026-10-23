import os
import sys
import json
import re
import urllib.request
import urllib.parse
import urllib.error
import zipfile
import io
import subprocess
import shutil

try:
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
except Exception:
    pass

CONFIG_PATH = r"C:\Users\username\.gemini\config\subtitles.json"

def load_config():
    if os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {
        "services": {
          "subdl": {"api_key": "", "api_url": "https://subdl.com/panel/api"},
          "subsource": {"api_key": "", "api_url": "https://subsource.net/dashboard/api"},
          "subscene": {"web_url": "https://sub-scene.com/"}
        },
        "autosubsync_path": r"C:\Users\username\Downloads\AutoSubSync\AutoSubSync.exe",
        "default_language": "ar",
        "preferred_encoding": "utf-8-sig"
    }

def convert_to_utf8_bom(file_path):
    """Ensures subtitle is saved in UTF-8 with BOM and clean line endings for correct Arabic rendering and parser compatibility."""
    if not os.path.exists(file_path):
        return False
    with open(file_path, "rb") as f:
        raw = f.read()
    
    encodings = ["utf-8-sig", "utf-8", "windows-1256", "cp1256", "iso-8859-6"]
    for enc in encodings:
        try:
            text = raw.decode(enc)
            # Normalize line endings to avoid \r\r\n bugs in ffsubsync
            text = text.replace('\r\r\n', '\n').replace('\r\n', '\n').replace('\r', '\n')
            with open(file_path, "w", encoding="utf-8-sig", newline="\n") as out_f:
                out_f.write(text)
            return True
        except Exception:
            continue
    return False

# ==========================================
# 1. SubDL Integration
# ==========================================
def search_subdl(query, imdb_id=None, lang="ar"):
    cfg = load_config()
    api_key = cfg.get("services", {}).get("subdl", {}).get("api_key")
    if not api_key:
        return {"error": True, "message": "SubDL API key not configured. Obtain at https://subdl.com/panel/api"}
    
    params = {"api_key": api_key, "languages": lang}
    if imdb_id:
        clean_imdb = imdb_id if imdb_id.startswith("tt") else f"tt{imdb_id}"
        params["imdb_id"] = clean_imdb
    else:
        params["film_name"] = query
        
    url = f"https://api.subdl.com/api/v1/subtitles?{urllib.parse.urlencode(params)}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            subs_list = data.get("subtitles", [])
            # Fallback to film_name if imdb returned no subtitles
            if not subs_list and imdb_id and query:
                fparams = {"api_key": api_key, "languages": lang, "film_name": query}
                furl = f"https://api.subdl.com/api/v1/subtitles?{urllib.parse.urlencode(fparams)}"
                freq = urllib.request.Request(furl, headers={"User-Agent": "Mozilla/5.0"})
                with urllib.request.urlopen(freq, timeout=15) as fresp:
                    fdata = json.loads(fresp.read().decode("utf-8"))
                    subs_list = fdata.get("subtitles", [])
                    
            results = []
            for s in subs_list:
                dl_url = s.get("url", "")
                if dl_url.startswith("/"):
                    dl_url = f"https://dl.subdl.com{dl_url}"
                results.append({
                    "provider": "subdl",
                    "id": s.get("subtitlePage"),
                    "name": s.get("release_name") or s.get("name"),
                    "author": s.get("author", ""),
                    "language": s.get("lang") or s.get("language"),
                    "download_url": dl_url,
                    "hi": s.get("hi", False)
                })
            return {"success": True, "provider": "subdl", "count": len(results), "subtitles": results}
    except Exception as e:
        return {"error": True, "provider": "subdl", "message": str(e)}

# ==========================================
# 2. SubSource Integration
# ==========================================
def search_subsource(query, imdb_id=None, lang="arabic"):
    cfg = load_config()
    api_key = cfg.get("services", {}).get("subsource", {}).get("api_key")
    if not api_key:
        return {"error": True, "message": "SubSource API key not configured. Obtain at https://subsource.net/dashboard/api"}
    
    headers = {"User-Agent": "Mozilla/5.0", "X-API-Key": api_key}
    
    # Step 1: Find movie ID
    movie_id = None
    if imdb_id:
        clean_imdb = imdb_id if imdb_id.startswith("tt") else f"tt{imdb_id}"
        search_url = f"https://api.subsource.net/api/v1/movies/search?searchType=imdb&imdb={clean_imdb}"
    else:
        search_url = f"https://api.subsource.net/api/v1/movies/search?searchType=text&q={urllib.parse.quote(query)}"
        
    try:
        req = urllib.request.Request(search_url, headers=headers)
        with urllib.request.urlopen(req, timeout=15) as resp:
            mdata = json.loads(resp.read().decode("utf-8"))
            items = mdata.get("data", [])
            if items:
                movie_id = items[0].get("movieId")
            elif imdb_id and query:
                # Fallback to text query
                q_url = f"https://api.subsource.net/api/v1/movies/search?searchType=text&q={urllib.parse.quote(query)}"
                q_req = urllib.request.Request(q_url, headers=headers)
                with urllib.request.urlopen(q_req, timeout=15) as q_resp:
                    qmdata = json.loads(q_resp.read().decode("utf-8"))
                    qitems = qmdata.get("data", [])
                    if qitems:
                        movie_id = qitems[0].get("movieId")
    except Exception as e:
        return {"error": True, "provider": "subsource", "message": f"Search failed: {e}"}
        
    if not movie_id:
        return {"success": True, "provider": "subsource", "count": 0, "subtitles": []}
        
    # Step 2: Query subtitles
    sub_lang = lang.lower()
    if sub_lang in ["ar", "ara"]:
        sub_lang = "arabic"
        
    subs_url = f"https://api.subsource.net/api/v1/subtitles?movieId={movie_id}&language={sub_lang}"
    try:
        req = urllib.request.Request(subs_url, headers=headers)
        with urllib.request.urlopen(req, timeout=15) as resp:
            sdata = json.loads(resp.read().decode("utf-8"))
            results = []
            for item in sdata.get("data", []):
                sub_id = item.get("subtitleId")
                rel_info = item.get("releaseInfo", [])
                name = rel_info[0] if rel_info else f"Subtitle {sub_id}"
                results.append({
                    "provider": "subsource",
                    "id": str(sub_id),
                    "name": name,
                    "release_type": item.get("releaseType"),
                    "all_releases": rel_info,
                    "downloads": item.get("downloads", 0),
                    "commentary": item.get("commentary", ""),
                    "download_url": f"https://api.subsource.net/api/v1/subtitles/{sub_id}/download"
                })
            return {"success": True, "provider": "subsource", "count": len(results), "subtitles": results}
    except Exception as e:
        return {"error": True, "provider": "subsource", "message": f"Subtitles query failed: {e}"}

# ==========================================
# 3. Sub-Scene Scraper Integration
# ==========================================
def search_subscene(query, imdb_id=None, lang="arabic"):
    clean_query = query.strip()
    slug = re.sub(r'[^a-zA-Z0-9]+', '-', clean_query).strip('-').lower()
    url = f"https://sub-scene.com/subtitles/{slug}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"})
    
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            html = resp.read().decode("utf-8", errors="ignore")
            # Parse subtitle links
            pattern = r'<div class="name">\s*<a href="(/subtitle/\d+)">\s*<span class="language[^"]*">\s*([^<]+)\s*</span>\s*<span class="rel"[^>]*title="([^"]+)"'
            matches = re.findall(pattern, html, re.DOTALL)
            results = []
            for sub_link, sub_lang, rel_title in matches:
                if "arabic" in sub_lang.strip().lower() or lang.lower() in sub_lang.strip().lower():
                    results.append({
                        "provider": "subscene",
                        "id": sub_link,
                        "name": rel_title.strip(),
                        "language": sub_lang.strip(),
                        "page_url": f"https://sub-scene.com{sub_link}"
                    })
            return {"success": True, "provider": "subscene", "count": len(results), "subtitles": results}
    except Exception as e:
        return {"error": True, "provider": "subscene", "message": str(e)}

# ==========================================
# 4. Stremio OpenSubtitles v3 Pro Integration
# ==========================================
def search_stremio_opensubtitles(imdb_id, season=None, episode=None):
    if not imdb_id:
        return {"success": False, "count": 0, "subtitles": []}
    clean_id = imdb_id if imdb_id.startswith("tt") else f"tt{imdb_id}"
    if season and episode:
        path = f"series/{clean_id}:{season}:{episode}.json"
    else:
        path = f"movie/{clean_id}.json"
    url = f"https://opensubtitlesv3-pro.dexter21767.com/eyJsYW5ncyI6WyJhcmFiaWMiXSwic291cmNlIjoiYWxsIiwiYWlUcmFuc2xhdGVkIjp0cnVlLCJhdXRvQWRqdXN0bWVudCI6dHJ1ZX0=/subtitles/{path}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=12) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            results = []
            for s in data.get("subtitles", []):
                results.append({
                    "provider": "stremio_opensubtitles",
                    "id": str(s.get("sub_id")),
                    "name": s.get("title") or f"OpenSubtitles {s.get('sub_id')}",
                    "language": s.get("lang_code") or "ar",
                    "download_url": s.get("url"),
                    "score": 0
                })
            return {"success": True, "provider": "stremio_opensubtitles", "count": len(results), "subtitles": results}
    except Exception as e:
        return {"error": True, "provider": "stremio_opensubtitles", "message": str(e)}

def vtt_to_srt(vtt_text):
    """Converts WebVTT format to standard SubRip SRT."""
    lines = vtt_text.splitlines()
    srt_lines = []
    counter = 1
    i = 0
    while i < len(lines):
        line = lines[i].strip()
        if "-->" in line:
            ts = re.sub(r'(\d{2}:\d{2}:\d{2})\.(\d{3})', r'\1,\2', line)
            srt_lines.append(str(counter))
            srt_lines.append(ts)
            counter += 1
            i += 1
            while i < len(lines) and lines[i].strip():
                # Strip out HTML tags if any
                clean_text = re.sub(r'<[^>]+>', '', lines[i])
                srt_lines.append(clean_text)
                i += 1
            srt_lines.append("")
        i += 1
    return "\n".join(srt_lines)

def deduce_release_type(name, all_releases=None):
    """Deduces if a subtitle is BluRay, WEB, or DVD from text even if classified as Other."""
    text = (name or "").lower()
    if all_releases:
        text += " " + " ".join([str(r).lower() for r in all_releases])
        
    bluray_signals = ["bluray", "blu-ray", "bdrip", "brrip", "remux", "uhd", "sparks", "lost", "fgt", "framestor", "geckos", "ctrlhd"]
    web_signals = ["web-dl", "webrip", "web", "atvp", "amzn", "nf", "dsnp", "max", "hbo", "itunes", "pahe", "yts", "rarbg"]
    dvd_signals = ["dvdrip", "dvdscr", "dvd", "screener"]
    
    if any(s in text for s in bluray_signals):
        return "bluray"
    if any(s in text for s in web_signals):
        return "web"
    if any(s in text for s in dvd_signals):
        return "dvd"
    return "other"

# ==========================================
# Unified Search & Download
# ==========================================
def search_all_subtitles(query, imdb_id=None, lang="ar", release_type=None):
    results = {
        "query": query,
        "imdb_id": imdb_id,
        "language": lang,
        "release_type": release_type,
        "sources": {}
    }
    # 1. SubDL
    subdl_res = search_subdl(query, imdb_id=imdb_id, lang=lang)
    results["sources"]["subdl"] = subdl_res
    
    # 2. SubSource
    subsource_res = search_subsource(query, imdb_id=imdb_id, lang="arabic")
    results["sources"]["subsource"] = subsource_res
    
    # 3. SubScene
    subscene_res = search_subscene(query, imdb_id=imdb_id, lang="arabic")
    results["sources"]["subscene"] = subscene_res
    
    # 4. Stremio OpenSubtitles v3 Pro (Dexter)
    if imdb_id:
        stremio_res = search_stremio_opensubtitles(imdb_id)
        results["sources"]["stremio_opensubtitles"] = stremio_res
    
    # Combine & score
    combined = []
    for prov, data in results["sources"].items():
        if data.get("success"):
            for s in data.get("subtitles", []):
                score = 0
                sname = (s.get("name") or "").lower()
                author = (s.get("author") or "").lower()
                raw_rel = str(s.get("release_type", "")).lower()
                
                # Deduce release type if missing or Other
                deduced = deduce_release_type(sname, s.get("all_releases"))
                s["deduced_type"] = deduced if raw_rel in ["", "none", "null", "other"] else raw_rel
                
                # Check release matching
                if release_type:
                    rtype = release_type.lower()
                    eff_type = s["deduced_type"].lower()
                    
                    if rtype in eff_type or ("bluray" in rtype and eff_type == "bluray") or ("web" in rtype and eff_type == "web"):
                        score += 60
                    elif eff_type == "other":
                        # "Other" candidate: don't exclude! Give reasonable base score
                        score += 20
                        s["needs_inspection"] = True
                else:
                    score += 10
                        
                # Trusted Arabic translators boost
                if any(t in author or t in sname for t in ["ali talal", "ahmed elsayed", "nizar", "loki_i", "dr.ali"]):
                    score += 25
                    
                s["score"] = score
                combined.append(s)
                
    # If no exact match found, ensure "other" candidates are still presented
    combined.sort(key=lambda x: x.get("score", 0), reverse=True)
    results["ranked_subtitles"] = combined
    return results

def download_subtitle_file(sub_item, output_dir, file_basename=None):
    os.makedirs(output_dir, exist_ok=True)
    provider = sub_item.get("provider")
    cfg = load_config()
    
    headers = {"User-Agent": "Mozilla/5.0"}
    if provider == "subsource":
        api_key = cfg.get("services", {}).get("subsource", {}).get("api_key")
        if api_key:
            headers["X-API-Key"] = api_key
            
    dl_url = sub_item.get("download_url")
    if not dl_url and provider == "subscene":
        # Resolve download page
        page_url = sub_item.get("page_url")
        req = urllib.request.Request(page_url, headers=headers)
        with urllib.request.urlopen(req, timeout=15) as resp:
            html = resp.read().decode("utf-8", errors="ignore")
            dl_m = re.search(r'href="(/subtitles/download/[^"]+)"', html)
            if dl_m:
                dl_url = f"https://sub-scene.com{dl_m.group(1)}"
                
    if not dl_url:
        return {"error": True, "message": "No download URL found for subtitle"}
        
    req = urllib.request.Request(dl_url, headers=headers)
    with urllib.request.urlopen(req, timeout=20) as resp:
        content = resp.read()
        
    # Check if zip
    extracted_files = []
    if content.startswith(b"PK\x03\x04"):
        z = zipfile.ZipFile(io.BytesIO(content))
        valid_entries = [n for n in z.namelist() if n.lower().endswith((".srt", ".ass", ".sub")) and not n.startswith("__MACOSX")]
        
        # Determine target episode if specified in file_basename
        chosen_entry = None
        if file_basename and valid_entries:
            ep_match = re.search(r'\b[sS](\d{1,2})[eE](\d{1,2})\b', file_basename)
            if ep_match:
                s_int, e_int = int(ep_match.group(1)), int(ep_match.group(2))
                patterns = [
                    rf'[sS]{s_int:02d}[eE]{e_int:02d}',
                    rf'\b[eE]{e_int:02d}\b',
                    rf'\b[eE]{e_int}\b',
                    rf'\b{s_int}x{e_int:02d}\b',
                    rf'episode[.\s_-]*{e_int}\b'
                ]
                for p in patterns:
                    for ve in valid_entries:
                        if re.search(p, ve, re.IGNORECASE):
                            chosen_entry = ve
                            break
                    if chosen_entry:
                        break

        # Fallback to first valid entry
        if not chosen_entry and valid_entries:
            chosen_entry = valid_entries[0]

        if chosen_entry:
            ext = os.path.splitext(chosen_entry)[1]
            target_name = f"{file_basename}{ext}" if file_basename else os.path.basename(chosen_entry)
            target_path = os.path.join(output_dir, target_name)
            with open(target_path, "wb") as out_f:
                out_f.write(z.read(chosen_entry))
            convert_to_utf8_bom(target_path)
            extracted_files.append(target_path)
    elif content.startswith(b"WEBVTT") or b"-->" in content:
        # direct WebVTT or SRT subtitle file
        target_name = f"{file_basename}.srt" if file_basename else "subtitle.srt"
        target_path = os.path.join(output_dir, target_name)
        if content.startswith(b"WEBVTT"):
            text = content.decode("utf-8", errors="replace")
            srt_text = vtt_to_srt(text)
            with open(target_path, "w", encoding="utf-8-sig") as out_f:
                out_f.write(srt_text)
        else:
            with open(target_path, "wb") as out_f:
                out_f.write(content)
            convert_to_utf8_bom(target_path)
        extracted_files.append(target_path)
    else:
        # direct file
        target_name = f"{file_basename}.srt" if file_basename else "subtitle.srt"
        target_path = os.path.join(output_dir, target_name)
        with open(target_path, "wb") as out_f:
            out_f.write(content)
        convert_to_utf8_bom(target_path)
        extracted_files.append(target_path)
        
    return {"success": True, "files": extracted_files}

# ==========================================
def shift_subtitle_timestamps(sub_path, offset_seconds, output_path=None):
    """Shifts all timestamps in an SRT file by offset_seconds (+ for delay, - for advance)."""
    if not os.path.exists(sub_path):
        return {"error": True, "message": f"Subtitle file not found: {sub_path}"}
        
    try:
        with open(sub_path, "r", encoding="utf-8-sig", errors="ignore") as f:
            content = f.read()
    except Exception:
        with open(sub_path, "r", encoding="utf-8", errors="ignore") as f:
            content = f.read()
            
    def shift_match(match):
        h1, m1, s1, ms1, h2, m2, s2, ms2 = match.groups()
        ms_start = (int(h1)*3600 + int(m1)*60 + int(s1))*1000 + int(ms1)
        ms_end = (int(h2)*3600 + int(m2)*60 + int(s2))*1000 + int(ms2)
        
        diff_ms = int(round(offset_seconds * 1000))
        new_start = max(0, ms_start + diff_ms)
        new_end = max(new_start + 500, ms_end + diff_ms)
        
        def fmt(ms_val):
            nh = ms_val // 3600000
            nm = (ms_val % 3600000) // 60000
            ns = (ms_val % 60000) // 1000
            nms = ms_val % 1000
            return f"{nh:02d}:{nm:02d}:{ns:02d},{nms:03d}"
            
        return f"{fmt(new_start)} --> {fmt(new_end)}"

    pattern = r"(\d{2}):(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2})[,.](\d{3})"
    new_content = re.sub(pattern, shift_match, content)
    
    if not output_path:
        base, ext = os.path.splitext(sub_path)
        output_path = f"{base}.shifted{ext}"
        
    with open(output_path, "w", encoding="utf-8-sig") as out_f:
        out_f.write(new_content)
        
    return {
        "success": True,
        "output_file": output_path,
        "offset_applied": offset_seconds,
        "message": f"Shifted subtitle by {offset_seconds:+.3f}s successfully"
    }

def compare_embedded_vs_external_sub(video_path, sub_path, auto_fix=False):
    """
    Intelligently compares embedded video subtitle timestamps against an external downloaded subtitle file.
    Specifically designed for releases without Arabic audio/subtitles.
    Probes reference dialogue stream (English/Original), checks for offset and drift via cross-correlation histogram,
    and validates sync.
    """
    from collections import Counter
    import statistics

    if not os.path.exists(video_path):
        return {"error": True, "message": f"Video file not found: {video_path}"}
    if not os.path.exists(sub_path):
        return {"error": True, "message": f"Subtitle file not found: {sub_path}"}
        
    # 1. Probe all streams in video container
    try:
        cmd = ["ffprobe", "-v", "error", "-select_streams", "s", "-show_entries", "stream=index,codec_name:stream_tags=language,title", "-of", "json", video_path]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
        sdata = json.loads(proc.stdout) if proc.returncode == 0 else {}
        streams = sdata.get("streams", [])
    except Exception as e:
        return {"error": True, "message": f"Failed to probe video streams: {e}"}

    # Check if Arabic subtitle already exists inside video
    for s in streams:
        lang = str(s.get("tags", {}).get("language", "")).lower()
        title = str(s.get("tags", {}).get("title", "")).lower()
        if lang in ["ara", "ar", "arabic"] or "arabic" in title or "عربي" in title:
            return {
                "has_embedded_arabic": True,
                "embedded_arabic_stream": s,
                "status": "EMBEDDED_ARABIC_PRESENT",
                "message": f"الفلم يحتوي مسبقاً على ترجمة عربية مدمجة رسمياً (المسار رقم #{s.get('index')}). لا حاجة لتحميل ترجمة خارجية إلا إذا رغبت بنسخة مترجم آخر."
            }

    # Find best reference stream (English or first stream)
    ref_stream = None
    for s in streams:
        lang = str(s.get("tags", {}).get("language", "")).lower()
        if lang in ["eng", "en", "english"]:
            ref_stream = s
            break
    if not ref_stream and streams:
        ref_stream = streams[0]
        
    if not ref_stream:
        # Fallback to duration comparison
        return verify_and_auto_sync(video_path, sub_path)

    # 2. Extract dialogue packet timestamps from early (min 3-7) and mid (min 30-34) sections
    intervals = "00:03:00%00:07:00,00:30:00%00:34:00"
    try:
        cmd2 = [
            "ffprobe", "-v", "error",
            "-select_streams", str(ref_stream["index"]),
            "-show_packets",
            "-read_intervals", intervals,
            "-show_entries", "packet=pts_time",
            "-of", "json",
            video_path
        ]
        proc2 = subprocess.run(cmd2, capture_output=True, text=True, timeout=25)
        pdata = json.loads(proc2.stdout) if proc2.returncode == 0 else {}
        packets = pdata.get("packets", [])
        pts_list = [float(p["pts_time"]) for p in packets if "pts_time" in p]
    except Exception as e:
        return {"error": True, "message": f"Failed to sample packets from stream #{ref_stream['index']}: {e}"}

    if not pts_list:
        return verify_and_auto_sync(video_path, sub_path)

    # 3. Parse external subtitle cues
    try:
        with open(sub_path, "r", encoding="utf-8-sig", errors="ignore") as f:
            sub_text = f.read()
    except Exception:
        with open(sub_path, "r", encoding="utf-8", errors="ignore") as f:
            sub_text = f.read()
            
    matches = re.findall(r"(\d{2}):(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2})[,.](\d{3})", sub_text)
    if not matches:
        return {"status": "unverified", "message": "Could not parse cue timestamps from external subtitle"}
        
    srt_starts = [int(h)*3600 + int(m)*60 + int(s) + int(ms)/1000.0 for h, m, s, ms, *_ in matches]

    # Split pts into early (<20 min) and mid (>=20 min)
    early_pts = [p for p in pts_list if p < 1200]
    mid_pts = [p for p in pts_list if p >= 1200]

    def find_peak_offset(p_list):
        if not p_list:
            return None, 0
        buckets = Counter()
        for p in p_list:
            for s in srt_starts:
                diff = p - s
                if -45.0 <= diff <= 45.0:
                    b = round(diff, 1)
                    buckets[b] += 1
        if not buckets:
            return None, 0
        peak, cnt = buckets.most_common(1)[0]
        return peak, cnt

    peak_early, cnt_early = find_peak_offset(early_pts)
    peak_mid, cnt_mid = find_peak_offset(mid_pts)

    if peak_early is None or cnt_early < 4:
        return {
            "status": "MISMATCH",
            "reference_stream": ref_stream,
            "message": "لم يتم العثور على أي تطابق زمني مع حوارات الفلم (غالباً نسخة فيلم مختلفة كلياً مثل Extended/Unrated)."
        }

    # If mid section has no dialogue or subtitles, default to early peak
    if peak_mid is None or cnt_mid < 4:
        drift = 0.0
        final_offset = peak_early
    else:
        drift = abs(peak_mid - peak_early)
        final_offset = peak_early

    result = {
        "reference_stream": {
            "index": ref_stream.get("index"),
            "codec": ref_stream.get("codec_name"),
            "language": ref_stream.get("tags", {}).get("language", "und")
        },
        "sampled_packets": len(pts_list),
        "early_correlation_hits": cnt_early,
        "offset_seconds": round(final_offset, 2),
        "drift_seconds": round(drift, 2)
    }

    # Evaluate verdict
    # Note: final_offset is (video_pts - srt_start).
    # If final_offset is -0.1s, SRT is ~0.1s ahead of video, perfectly matched.
    if abs(final_offset) <= 0.45 and drift <= 0.25:
        result["status"] = "PERFECT_MATCH"
        result["message"] = f"الترجمة متطابقة تماماً مع توقيت الفلم الداخلي (فرق {final_offset:+.2f} ثانية، بدون انزياح). لا تحتاج أي تعديل."
    elif drift > 0.35:
        result["status"] = "DRIFT_MISMATCH"
        result["message"] = f"يوجد انزياح تدريجي في التوقيت ({drift:.2f} ثانية بين بداية الفلم ومنتصفه) بسبب اختلاف معدل الإطارات (23.976fps vs 24/25fps)."
        if auto_fix:
            sync_res = sync_subtitle_with_autosubsync(video_path, sub_path)
            result["auto_sync"] = sync_res
    else:
        result["status"] = "CONSTANT_OFFSET"
        # Video is at video_pts, srt is at srt_start. Needed shift = +final_offset
        result["message"] = f"الترجمة متطابقة مع نسخة الفلم ولكن يوجد فارق توقيت ثابت قدره {final_offset:+.2f} ثانية (بسبب اختلاف شعارات الاستوديو)."
        if auto_fix:
            shift_res = shift_subtitle_timestamps(sub_path, final_offset)
            result["auto_shift"] = shift_res

    return result

# ==========================================
# AutoSubSync & Timing Verification
# ==========================================
def parse_sub_timestamps(sub_path):
    """Returns (first_start_sec, last_end_sec) from an srt or ass file."""
    try:
        with open(sub_path, "r", encoding="utf-8-sig", errors="ignore") as f:
            text = f.read()
        ts_matches = re.findall(r"(\d{2}):(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(\d{2}):(\d{2}):(\d{2})[,.](\d{3})", text)
        if ts_matches:
            def to_sec(h, m, s, ms):
                return int(h)*3600 + int(m)*60 + int(s) + int(ms)/1000.0
            first_start = to_sec(*ts_matches[0][:4])
            last_end = to_sec(*ts_matches[-1][4:])
            return first_start, last_end
    except Exception:
        pass
    return None, None

def verify_and_auto_sync(video_path, sub_path):
    """Checks timing against video file using ffprobe. If mismatch is detected, invokes AutoSubSync."""
    first_sub, last_sub = parse_sub_timestamps(sub_path)
    if last_sub is None:
        return {"status": "unverified", "message": "Could not parse subtitle timestamps"}
        
    try:
        cmd = ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", video_path]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
        if proc.returncode == 0:
            vdata = json.loads(proc.stdout)
            v_dur = float(vdata.get("format", {}).get("duration", 0))
            if v_dur > 0:
                # Subtitle ending beyond video or > 15m too early indicates mismatch
                if last_sub > v_dur + 5 or last_sub < v_dur - 900:
                    print(f"Timing mismatch detected (Sub end: {last_sub}s, Video: {v_dur}s). Invoking AutoSubSync...")
                    return sync_subtitle_with_autosubsync(video_path, sub_path)
                return {"status": "matched", "video_duration": v_dur, "sub_end": last_sub, "message": "Timing matches video duration"}
    except Exception as e:
        return {"status": "error", "message": str(e)}
    return {"status": "unverified"}

def sync_subtitle_with_autosubsync(video_path, subtitle_path, output_path=None):
    if not output_path:
        base, ext = os.path.splitext(subtitle_path)
        output_path = f"{base}.synced{ext}"

    # Priority 1: alass (State-of-the-art Language-Agnostic Subtitle Synchronization with multi-split detection)
    skill_dir = os.path.dirname(os.path.abspath(__file__))
    alass_bat = os.path.join(skill_dir, "bin", "alass-windows64", "alass.bat")
    if os.path.exists(alass_bat):
        try:
            convert_to_utf8_bom(subtitle_path)
            cmd = [alass_bat, video_path, subtitle_path, output_path]
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
            if os.path.exists(output_path) and os.path.getsize(output_path) > 1000:
                convert_to_utf8_bom(output_path)
                return {"success": True, "output_file": output_path, "tool": "alass"}
        except Exception:
            pass

    # Priority 2: ffsubsync (State-of-the-art Speech / VAD recognition alignment)
    ffs_bin = shutil.which("ffsubsync") or r"C:\Users\username\AppData\Local\Programs\Python\Python311\Scripts\ffsubsync.EXE"
    if ffs_bin and os.path.exists(ffs_bin):
        try:
            # First ensure input subtitle is clean UTF-8 BOM
            convert_to_utf8_bom(subtitle_path)
            cmd = [ffs_bin, video_path, "-i", subtitle_path, "-o", output_path, "--max-duration-seconds", "180"]
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
            if os.path.exists(output_path) and os.path.getsize(output_path) > 1000:
                convert_to_utf8_bom(output_path)
                return {"success": True, "output_file": output_path, "tool": "ffsubsync"}
            else:
                # If ffsubsync computed an offset but had unparseable lines, apply offset cleanly
                off_match = re.search(r'offset seconds:\s*([-\d\.]+)', proc.stderr)
                if off_match:
                    off_val = float(off_match.group(1))
                    shift_res = shift_subtitle_timestamps(subtitle_path, off_val, output_path=output_path)
                    if shift_res.get("success"):
                        return {"success": True, "output_file": output_path, "tool": "ffsubsync (shifted)", "offset": off_val}
        except Exception:
            pass

    # Priority 3: AutoSubSync.exe
    cfg = load_config()
    exe_path = cfg.get("autosubsync_path", r"C:\Users\username\Downloads\AutoSubSync\AutoSubSync.exe")
    if os.path.exists(exe_path):
        cmd = [exe_path, video_path, subtitle_path, output_path]
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
            if os.path.exists(output_path):
                convert_to_utf8_bom(output_path)
                return {"success": True, "output_file": output_path, "tool": "AutoSubSync"}
            else:
                return {"error": True, "message": proc.stderr or proc.stdout}
        except Exception as e:
            return {"error": True, "message": str(e)}

    return {"error": True, "message": "لم يتم العثور على أداة مزامنة صالحة (alass أو ffsubsync أو AutoSubSync)"}

# ==========================================
# Auto-Muxing (Embed Subtitle into MKV Container)
# ==========================================
def mux_subtitle_into_video(video_path, subtitle_path, output_path=None, sub_lang="ara", sub_title="Arabic"):
    """Losslessly embeds the subtitle into an MKV container using ffmpeg copy mode in seconds."""
    if not os.path.exists(video_path) or not os.path.exists(subtitle_path):
        return {"error": True, "message": "Video or subtitle file does not exist"}
        
    if not output_path:
        base, ext = os.path.splitext(video_path)
        output_path = f"{base}.muxed.mkv"
        
    cmd = [
        "ffmpeg", "-y",
        "-i", video_path,
        "-i", subtitle_path,
        "-c", "copy",
        "-c:s", "srt",
        f"-metadata:s:s:0", f"language={sub_lang}",
        f"-metadata:s:s:0", f"title={sub_title}",
        output_path
    ]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
        if proc.returncode == 0 and os.path.exists(output_path):
            return {"success": True, "output_file": output_path, "message": "Subtitle losslessly muxed into MKV successfully"}
        else:
            return {"error": True, "message": proc.stderr or proc.stdout}
    except Exception as e:
        return {"error": True, "message": str(e)}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage:")
        print("  python subtitle_manager.py search <query> [imdb_id] [lang] [release_type]")
        print("  python subtitle_manager.py download <subdl|subsource|subscene> <url_or_id> <out_dir> [base_name]")
        print("  python subtitle_manager.py compare <video_path> <subtitle_path> [--fix]")
        print("  python subtitle_manager.py shift <subtitle_path> <offset_seconds> [out_path]")
        print("  python subtitle_manager.py sync <video_path> <subtitle_path> [out_path]")
        print("  python subtitle_manager.py mux <video_path> <subtitle_path> [out_path]")
        sys.exit(1)
        
    action = sys.argv[1].lower()
    if action == "search":
        q = sys.argv[2]
        imdb = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        lng = sys.argv[4] if len(sys.argv) > 4 else "ar"
        rel = sys.argv[5] if len(sys.argv) > 5 else None
        res = search_all_subtitles(q, imdb_id=imdb, lang=lng, release_type=rel)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "compare" and len(sys.argv) >= 4:
        vpath = sys.argv[2]
        spath = sys.argv[3]
        fix = len(sys.argv) > 4 and sys.argv[4] in ["--fix", "-f", "true", "fix"]
        res = compare_embedded_vs_external_sub(vpath, spath, auto_fix=fix)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "shift" and len(sys.argv) >= 4:
        spath = sys.argv[2]
        offset = float(sys.argv[3])
        outp = sys.argv[4] if len(sys.argv) > 4 else None
        res = shift_subtitle_timestamps(spath, offset, outp)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "sync" and len(sys.argv) >= 4:
        vpath = sys.argv[2]
        spath = sys.argv[3]
        outp = sys.argv[4] if len(sys.argv) > 4 else None
        res = sync_subtitle_with_autosubsync(vpath, spath, outp)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "mux" and len(sys.argv) >= 4:
        vpath = sys.argv[2]
        spath = sys.argv[3]
        outp = sys.argv[4] if len(sys.argv) > 4 else None
        res = mux_subtitle_into_video(vpath, spath, outp)
        print(json.dumps(res, indent=2, ensure_ascii=False))
