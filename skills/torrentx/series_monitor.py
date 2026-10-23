import os
import sys
import json
import re
import urllib.request
import urllib.parse
import subprocess
from datetime import datetime

try:
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
except Exception:
    pass

CONFIG_PATH = r"C:\Users\username\.gemini\config\tracked_series.json"
QBITTORRENT_PATH = r"C:\Program Files\qBittorrent\qbittorrent.exe"

def load_config():
    if os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {
        "tracked_shows": [],
        "private_trackers": [],
        "download_dir": os.path.expanduser(r"~\Downloads\Series"),
        "default_quality": "1080p",
        "default_source": "debrid"
    }

def save_config(cfg):
    os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
    with open(CONFIG_PATH, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)

def get_tvmaze_schedule(title):
    """Fetches air day, time, and episode info from TVmaze public API without any key."""
    url = f"https://api.tvmaze.com/singlesearch/shows?{urllib.parse.urlencode({'q': title, 'embed': 'episodes'})}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=12) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            schedule = data.get("schedule", {})
            episodes = data.get("_embedded", {}).get("episodes", [])
            latest_ep = episodes[-1] if episodes else None
            
            return {
                "name": data.get("name"),
                "status": data.get("status"),
                "air_days": schedule.get("days", []),
                "air_time": schedule.get("time", ""),
                "latest_season": latest_ep.get("season") if latest_ep else 1,
                "latest_episode": latest_ep.get("number") if latest_ep else 1,
                "latest_airdate": latest_ep.get("airdate") if latest_ep else None
            }
    except Exception:
        return None

def add_tracked_show(title, season=None, current_episode=0, source_type="debrid", tracker_name=None, air_day=None, release_window=None, quality="4k web-dl", episodes_per_drop=1):
    """
    Adds a new show to weekly automatic tracker:
    - source_type: 'debrid' (Aria2 + RD) or 'private' (qBittorrent + Seeding, NO Debrid)
    - quality: e.g. '4k web-dl', '1080p web-dl', 'remux'
    - episodes_per_drop: number of episodes released simultaneously (e.g. 2 for VisionQuest)
    """
    cfg = load_config()
    source_type = source_type.lower()
    
    # Auto-detect schedule if possible
    sched = get_tvmaze_schedule(title)
    clean_title = sched["name"] if sched else title.strip()
    detected_day = sched["air_days"][0] if sched and sched.get("air_days") else (air_day or "Weekly")
    target_season = season or (sched["latest_season"] if sched else 1)
    
    # Check if already tracked
    for s in cfg.get("tracked_shows", []):
        if s.get("title", "").lower() == clean_title.lower() and s.get("season") == target_season:
            s["source_type"] = source_type
            s["tracker_name"] = tracker_name
            s["quality"] = quality
            s["episodes_per_drop"] = episodes_per_drop
            if release_window:
                s["release_window"] = release_window
            save_config(cfg)
            return {"success": True, "message": f"Updated existing show: {clean_title} S{target_season}", "show": s}

    show_entry = {
        "title": clean_title,
        "season": int(target_season),
        "last_downloaded_episode": int(current_episode),
        "next_expected_episode": int(current_episode) + 1,
        "episodes_per_drop": int(episodes_per_drop),
        "quality": quality,
        "air_day": detected_day,
        "release_window": release_window or "14:00 - 18:00",
        "source_type": source_type,
        "tracker_name": tracker_name if source_type == "private" else None,
        "active": True,
        "created_at": datetime.now().strftime("%Y-%m-%d %H:%M")
    }
    
    cfg["tracked_shows"].append(show_entry)
    save_config(cfg)
    return {"success": True, "message": f"Successfully tracked: {clean_title} S{target_season} ({quality})", "show": show_entry}

def add_private_tracker(name, rss_url=None, passkey=None, base_url=None):
    """Registers a private tracker. Never routes through Debrid services to protect user ratio and passkey."""
    cfg = load_config()
    clean_name = name.strip()
    
    tracker_entry = {
        "name": clean_name,
        "rss_url": rss_url.strip() if rss_url else "",
        "passkey": passkey.strip() if passkey else "",
        "base_url": base_url.strip() if base_url else "",
        "client": "qbittorrent",
        "seeding_required": True,
        "use_debrid": False # Strictly forbidden for private trackers
    }
    
    cfg["private_trackers"] = [t for t in cfg.get("private_trackers", []) if t.get("name").lower() != clean_name.lower()]
    cfg["private_trackers"].append(tracker_entry)
    save_config(cfg)
    return {"success": True, "message": f"Private tracker '{clean_name}' registered. Mode: Direct qBittorrent seeding (No Debrid)."}

def list_tracked():
    cfg = load_config()
    return {
        "total_shows": len(cfg.get("tracked_shows", [])),
        "tracked_shows": cfg.get("tracked_shows", []),
        "private_trackers": cfg.get("private_trackers", [])
    }

def process_show_episode(show_entry):
    """
    Checks for the next expected episode of a tracked show.
    If found:
    - If Debrid: Resolves link -> downloads with Aria2 (16 conn) -> checks/downloads Arabic sub.
    - If Private Tracker: Downloads .torrent with passkey -> opens in qBittorrent for ratio/seeding.
    """
    title = show_entry.get("title")
    season = show_entry.get("season", 1)
    target_ep = show_entry.get("next_expected_episode", show_entry.get("last_downloaded_episode", 0) + 1)
    source_type = show_entry.get("source_type", "debrid").lower()
    
    ep_str = f"S{season:02d}E{target_ep:02d}"
    query = f"{title} {ep_str}"
    
    # ----------------------------------------------------
    # Case A: Private Tracker Mode (Strictly via qBittorrent)
    # ----------------------------------------------------
    if source_type in ["private", "private_tracker"]:
        cfg = load_config()
        tracker_name = show_entry.get("tracker_name")
        matched_tracker = None
        for t in cfg.get("private_trackers", []):
            if t.get("name").lower() == (tracker_name or "").lower():
                matched_tracker = t
                break
        if not matched_tracker and cfg.get("private_trackers"):
            matched_tracker = cfg["private_trackers"][0]
            
        rss_url = matched_tracker.get("rss_url") if matched_tracker else None
        if not rss_url:
            return {"status": "skipped", "message": f"No private tracker RSS found for {title}. Please add via 'add-tracker'."}
            
        # Parse RSS for episode
        try:
            req = urllib.request.Request(rss_url, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=15) as resp:
                xml_data = resp.read().decode("utf-8", errors="ignore")
                
            # Search for item with title and ep_str
            items = re.findall(r"<item>(.*?)</item>", xml_data, re.DOTALL)
            found_torrent_url = None
            found_title = None
            for it in items:
                t_match = re.search(r"<title>(.*?)</title>", it)
                l_match = re.search(r"<link>(.*?)</link>", it) or re.search(r'url="(.*?\.torrent[^"]*)"', it)
                if t_match and l_match:
                    item_title = t_match.group(1)
                    if title.lower() in item_title.lower() and (ep_str.lower() in item_title.lower() or f"{season}x{target_ep:02d}" in item_title.lower()):
                        found_torrent_url = l_match.group(1)
                        found_title = item_title
                        break
                        
            if found_torrent_url:
                # Download .torrent directly
                torrent_dir = os.path.expanduser(r"~\Downloads\PrivateTorrents")
                os.makedirs(torrent_dir, exist_ok=True)
                clean_fname = re.sub(r'[\\/*?:"<>|]', "", f"{found_title}.torrent")
                local_torrent_path = os.path.join(torrent_dir, clean_fname)
                
                treq = urllib.request.Request(found_torrent_url, headers={"User-Agent": "Mozilla/5.0"})
                with urllib.request.urlopen(treq, timeout=20) as tresp:
                    with open(local_torrent_path, "wb") as out_f:
                        out_f.write(tresp.read())
                        
                # Add to qBittorrent for downloading and seeding
                if os.path.exists(QBITTORRENT_PATH):
                    subprocess.Popen([QBITTORRENT_PATH, local_torrent_path])
                else:
                    os.startfile(local_torrent_path)
                    
                # Update progress
                show_entry["last_downloaded_episode"] = target_ep
                show_entry["next_expected_episode"] = target_ep + 1
                show_entry["last_download_time"] = datetime.now().strftime("%Y-%m-%d %H:%M")
                save_config(cfg)
                
                return {
                    "status": "downloaded_private",
                    "show": title,
                    "episode": ep_str,
                    "torrent_file": local_torrent_path,
                    "client": "qBittorrent (Seeding active)",
                    "message": f"Added {ep_str} to qBittorrent from private tracker '{matched_tracker.get('name')}'. Seed ratio preserved."
                }
            else:
                return {"status": "not_available_yet", "show": title, "episode": ep_str, "message": f"{ep_str} has not dropped on {matched_tracker.get('name')} yet."}
                
        except Exception as e:
            return {"status": "error", "message": f"Private tracker RSS error: {e}"}

    # ----------------------------------------------------
    # Case B: Public / Real-Debrid Mode (Aria2 16-Connections + Auto Subtitle)
    # ----------------------------------------------------
    import debrid_manager
    import subtitle_manager
    
    # 1. Search debrid cache via DMM / Search
    # Search RD torrents or public search
    dmm_info = debrid_manager.get_dmm_info(query)
    
    # Try finding torrent magnet or direct link via recent debrid or search
    from debrid_manager import api_request, add_magnet_and_resolve, send_to_downloader
    
    # Check if user already resolved or search RD library
    torrents = debrid_manager.list_recent()
    matched_url = None
    for t in torrents:
        t_filename = t.get("filename", "").lower()
        if title.lower() in t_filename and ep_str.lower() in t_filename:
            links = t.get("links", [])
            if links:
                unres = api_request("unrestrict/link", data={"link": links[0]}, method="POST")
                if unres and "download" in unres:
                    matched_url = unres["download"]
                    break
                    
    if not matched_url:
        return {
            "status": "monitoring",
            "show": title,
            "episode": ep_str,
            "air_day": show_entry.get("air_day"),
            "window": show_entry.get("release_window"),
            "dmm_url": dmm_info.get("dmm_url"),
            "message": f"{ep_str} not yet cached on Real-Debrid. Scheduled window: {show_entry.get('air_day')} ({show_entry.get('release_window')})."
        }

    # 2. Download with Aria2
    out_dir = os.path.join(os.path.expanduser(r"~\Downloads\Series"), title)
    os.makedirs(out_dir, exist_ok=True)
    dl_res = send_to_downloader(matched_url, downloader="aria2", output_dir=out_dir)
    
    # 3. Check / Download Arabic Subtitle
    sub_status = "Not checked"
    try:
        s_res = subtitle_manager.search_all_subtitles(f"{title} {ep_str}", lang="ar")
        subs = s_res.get("ranked_subtitles", [])
        if subs:
            s_dl = subtitle_manager.download_subtitle_file(subs[0], out_dir, file_basename=f"{title}.{ep_str}.ar")
            if s_dl.get("success"):
                sub_status = f"Arabic subtitle downloaded: {os.path.basename(s_dl['files'][0])}"
        else:
            sub_status = "No Arabic subtitles found online yet (left without sub for now)"
    except Exception as e:
        sub_status = f"Subtitle search skipped: {e}"

    # Update config
    cfg = load_config()
    for s in cfg.get("tracked_shows", []):
        if s.get("title") == title and s.get("season") == season:
            s["last_downloaded_episode"] = target_ep
            s["next_expected_episode"] = target_ep + 1
            s["last_download_time"] = datetime.now().strftime("%Y-%m-%d %H:%M")
            break
    save_config(cfg)
    
    return {
        "status": "downloaded_debrid",
        "show": title,
        "episode": ep_str,
        "download": dl_res,
        "subtitle": sub_status
    }

def check_all_tracked():
    """Loops through all active tracked shows and processes any newly available episodes."""
    cfg = load_config()
    results = []
    for s in cfg.get("tracked_shows", []):
        if s.get("active", True):
            res = process_show_episode(s)
            results.append(res)
    return {
        "checked_at": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
        "total_shows": len(results),
        "results": results
    }

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage:")
        print("  python series_monitor.py add <title> [season] [current_ep] [debrid|private] [tracker_name] [window]")
        print("  python series_monitor.py add-tracker <name> <rss_url> [passkey]")
        print("  python series_monitor.py list")
        print("  python series_monitor.py check")
        sys.exit(1)
        
    action = sys.argv[1].lower()
    if action == "add" and len(sys.argv) >= 3:
        tit = sys.argv[2]
        s_num = int(sys.argv[3]) if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        c_ep = int(sys.argv[4]) if len(sys.argv) > 4 else 0
        src = sys.argv[5] if len(sys.argv) > 5 else "debrid"
        t_name = sys.argv[6] if len(sys.argv) > 6 and sys.argv[6] != "none" else None
        win = sys.argv[7] if len(sys.argv) > 7 else "14:00 - 18:00"
        res = add_tracked_show(tit, season=s_num, current_episode=c_ep, source_type=src, tracker_name=t_name, release_window=win)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "add-tracker" and len(sys.argv) >= 4:
        tname = sys.argv[2]
        rurl = sys.argv[3]
        pkey = sys.argv[4] if len(sys.argv) > 4 else None
        res = add_private_tracker(tname, rss_url=rurl, passkey=pkey)
        print(json.dumps(res, indent=2, ensure_ascii=False))
        
    elif action == "list":
        print(json.dumps(list_tracked(), indent=2, ensure_ascii=False))
        
    elif action == "check":
        res = check_all_tracked()
        print(json.dumps(res, indent=2, ensure_ascii=False))
    else:
        print("Invalid command")
