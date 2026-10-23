import os
import sys
import json
import re
import shutil
import urllib.request
import urllib.parse

try:
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
except Exception:
    pass

CONFIG_PATH = r"C:\Users\username\.gemini\config\media_organizer.json"

def load_config():
    default_cfg = {
        "movies_dir": r"D:\Movies" if os.path.exists(r"D:\ ") else os.path.expanduser(r"~\Downloads\Movies"),
        "series_dir": r"D:\Series" if os.path.exists(r"D:\ ") else os.path.expanduser(r"~\Downloads\Series"),
        "naming_style": "clean",  # Options: "clean" (احترافي مختصر), "standard" (قياسي), "detailed" (كامل مع الجودة)
        "create_movie_folder": True,
        "create_season_folder": True
    }
    if os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                loaded = json.load(f)
                default_cfg.update(loaded)
        except Exception:
            pass
    return default_cfg

def save_config(cfg):
    os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
    with open(CONFIG_PATH, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)

def set_directories(movies_dir=None, series_dir=None, naming_style=None):
    """Updates preferred target drives/folders and naming style for Plex/Jellyfin."""
    cfg = load_config()
    if movies_dir:
        os.makedirs(movies_dir, exist_ok=True)
        cfg["movies_dir"] = os.path.abspath(movies_dir)
    if series_dir:
        os.makedirs(series_dir, exist_ok=True)
        cfg["series_dir"] = os.path.abspath(series_dir)
    if naming_style and naming_style.lower() in ["clean", "standard", "detailed"]:
        cfg["naming_style"] = naming_style.lower()
    save_config(cfg)
    return {
        "success": True,
        "movies_dir": cfg["movies_dir"],
        "series_dir": cfg["series_dir"],
        "naming_style": cfg["naming_style"],
        "message": "Media library directories and naming style updated successfully"
    }

def extract_quality_tags(filename):
    """Extracts resolution and source format cleanly."""
    tags = []
    if re.search(r'2160p|4k|uhd', filename, re.IGNORECASE):
        tags.append("2160p")
    elif re.search(r'1080p', filename, re.IGNORECASE):
        tags.append("1080p")
    elif re.search(r'720p', filename, re.IGNORECASE):
        tags.append("720p")
        
    if re.search(r'remux', filename, re.IGNORECASE):
        tags.append("Remux")
    elif re.search(r'web-?dl|webrip', filename, re.IGNORECASE):
        tags.append("WEB-DL")
    elif re.search(r'bluray|bdrip', filename, re.IGNORECASE):
        tags.append("BluRay")
    return " ".join(tags)

def fetch_episode_title(show_name, season_num, episode_num):
    """Fetches official episode title from TVmaze API."""
    try:
        url = f"https://api.tvmaze.com/singlesearch/shows?{urllib.parse.urlencode({'q': show_name, 'embed': 'episodes'})}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=8) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            episodes = data.get("_embedded", {}).get("episodes", [])
            for ep in episodes:
                if ep.get("season") == int(season_num) and ep.get("number") == int(episode_num):
                    raw_name = ep.get("name", "")
                    clean_name = re.sub(r'[\\/*?:"<>|]', "", raw_name).strip()
                    return clean_name
    except Exception:
        pass
    return None

def parse_media_filename(file_path):
    """
    Intelligently analyzes a torrent filename and returns media metadata.
    """
    basename = os.path.basename(file_path)
    stem, ext = os.path.splitext(basename)
    
    # 1. Check for TV series pattern: S01E05 or 1x05
    m_tv = re.search(r'^(.*?)[._ ]+(?:S(\d{1,2})E(\d{1,2})|(\d{1,2})x(\d{1,2}))', stem, re.IGNORECASE)
    if m_tv:
        raw_title = m_tv.group(1).replace('.', ' ').strip()
        s_num = int(m_tv.group(2) or m_tv.group(4))
        e_num = int(m_tv.group(3) or m_tv.group(5))
        
        # Clean up title formatting
        clean_title = re.sub(r'\s+', ' ', raw_title).strip().title()
        quality = extract_quality_tags(stem)
        
        return {
            "media_type": "series",
            "title": clean_title,
            "season": s_num,
            "episode": e_num,
            "quality": quality,
            "extension": ext
        }

    # 2. Check for Movie pattern: Title.2017...
    m_mov = re.search(r'^(.*?)[._ ]+(\d{4})(?=[._ ]|$)', stem)
    if m_mov:
        raw_title = m_mov.group(1).replace('.', ' ').strip()
        year = m_mov.group(2)
        clean_title = re.sub(r'\s+', ' ', raw_title).strip().title()
        quality = extract_quality_tags(stem)
        return {
            "media_type": "movie",
            "title": clean_title,
            "year": year,
            "quality": quality,
            "extension": ext
        }

    # Fallback
    clean_fallback = re.sub(r'\s+', ' ', stem.replace('.', ' ')).strip().title()
    return {
        "media_type": "movie",
        "title": clean_fallback,
        "year": None,
        "quality": extract_quality_tags(stem),
        "extension": ext
    }

def organize_and_rename_file(video_path, custom_dest_dir=None, naming_style=None, companion_sub_path=None, move=True):
    """
    Renames and moves/copies a downloaded video and its companion subtitle
    into the standard Plex / Jellyfin / Infuse / Emby library structure.
    """
    if not os.path.exists(video_path):
        return {"error": True, "message": f"Source video file not found: {video_path}"}
        
    cfg = load_config()
    style = (naming_style or cfg.get("naming_style", "clean")).lower()
    
    meta = parse_media_filename(video_path)
    ext = meta.get("extension", ".mkv")
    
    # ----------------------------------------------------
    # Case A: TV Series Organization
    # ----------------------------------------------------
    if meta.get("media_type") == "series":
        base_series_dir = custom_dest_dir or cfg.get("series_dir")
        show_title = meta["title"]
        s_num = meta["season"]
        e_num = meta["episode"]
        quality = meta.get("quality", "")
        
        # Build clean professional filename according to style
        if style == "clean":
            # احترافي ومختصر: Show - S01E05.mkv
            target_filename = f"{show_title} - S{s_num:02d}E{e_num:02d}{ext}"
        elif style == "standard":
            # احترافي قياسي: Show - S01E05 - Episode Name.mkv
            ep_name = fetch_episode_title(show_title, s_num, e_num)
            if ep_name:
                target_filename = f"{show_title} - S{s_num:02d}E{e_num:02d} - {ep_name}{ext}"
            else:
                target_filename = f"{show_title} - S{s_num:02d}E{e_num:02d}{ext}"
        else: # detailed
            # احترافي كامل مع الجودة: Show - S01E05 - Episode Name [2160p WEB-DL].mkv
            ep_name = fetch_episode_title(show_title, s_num, e_num)
            q_part = f" [{quality}]" if quality else ""
            if ep_name:
                target_filename = f"{show_title} - S{s_num:02d}E{e_num:02d} - {ep_name}{q_part}{ext}"
            else:
                target_filename = f"{show_title} - S{s_num:02d}E{e_num:02d}{q_part}{ext}"

        # Standard Plex Folder Hierarchy: Series\Show Name\Season 01\
        target_dir = os.path.join(base_series_dir, show_title, f"Season {s_num:02d}")
        
    # ----------------------------------------------------
    # Case B: Movie Organization
    # ----------------------------------------------------
    else:
        base_movies_dir = custom_dest_dir or cfg.get("movies_dir")
        movie_title = meta["title"]
        year_str = f" ({meta['year']})" if meta.get("year") else ""
        quality = meta.get("quality", "")
        
        if style == "clean" or style == "standard":
            # احترافي ومختصر: Movie Name (Year).mkv
            target_filename = f"{movie_title}{year_str}{ext}"
        else: # detailed
            # احترافي كامل مع الجودة: Movie Name (Year) [2160p Remux].mkv
            q_part = f" [{quality}]" if quality else ""
            target_filename = f"{movie_title}{year_str}{q_part}{ext}"

        # Standard Plex Movie Hierarchy: Movies\Movie Name (Year)\
        target_dir = os.path.join(base_movies_dir, f"{movie_title}{year_str}")

    os.makedirs(target_dir, exist_ok=True)
    final_video_path = os.path.join(target_dir, target_filename)
    
    # Move or copy video
    if move:
        shutil.move(video_path, final_video_path)
    else:
        shutil.copy2(video_path, final_video_path)

    # ----------------------------------------------------
    # Auto-Organize Companion Subtitle (.srt)
    # ----------------------------------------------------
    final_sub_path = None
    sub_source = companion_sub_path
    if not sub_source:
        # Check if srt with matching name exists in source directory
        src_dir = os.path.dirname(video_path)
        src_stem = os.path.splitext(os.path.basename(video_path))[0]
        for candidate in os.listdir(src_dir):
            if candidate.lower().endswith(".srt") and (src_stem.lower() in candidate.lower() or meta["title"].lower() in candidate.lower()):
                sub_source = os.path.join(src_dir, candidate)
                break
                
    if sub_source and os.path.exists(sub_source):
        # Target subtitle name matches video exactly for automatic pairing: e.g. Show - S01E05.ar.srt
        target_stem = os.path.splitext(target_filename)[0]
        target_sub_filename = f"{target_stem}.ar.srt"
        final_sub_path = os.path.join(target_dir, target_sub_filename)
        if move:
            shutil.move(sub_source, final_sub_path)
        else:
            shutil.copy2(sub_source, final_sub_path)

    return {
        "success": True,
        "media_type": meta.get("media_type"),
        "title": meta.get("title"),
        "style": style,
        "organized_video": final_video_path,
        "organized_subtitle": final_sub_path,
        "target_directory": target_dir,
        "message": f"Organized into standard {meta.get('media_type').upper()} library structure: {target_filename}"
    }

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage:")
        print("  python media_organizer.py organize <file_path> [dest_dir] [clean|standard|detailed] [sub_path]")
        print("  python media_organizer.py config [movies_dir] [series_dir] [clean|standard|detailed]")
        print("  python media_organizer.py parse <file_path>")
        sys.exit(1)
        
    action = sys.argv[1].lower()
    if action == "organize" and len(sys.argv) >= 3:
        v_path = sys.argv[2]
        d_dir = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        n_style = sys.argv[4] if len(sys.argv) > 4 and sys.argv[4] != "none" else None
        s_path = sys.argv[5] if len(sys.argv) > 5 and sys.argv[5] != "none" else None
        res = organize_and_rename_file(v_path, custom_dest_dir=d_dir, naming_style=n_style, companion_sub_path=s_path)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "config":
        m_dir = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] != "none" else None
        s_dir = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        n_style = sys.argv[4] if len(sys.argv) > 4 and sys.argv[4] != "none" else None
        res = set_directories(movies_dir=m_dir, series_dir=s_dir, naming_style=n_style)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "parse" and len(sys.argv) >= 3:
        v_path = sys.argv[2]
        res = parse_media_filename(v_path)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    else:
        print("Invalid arguments")
