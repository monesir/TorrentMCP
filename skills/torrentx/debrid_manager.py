import os
import sys
import json
import time
import re
import urllib.request
import urllib.parse
import urllib.error
import subprocess

CONFIG_PATH_DEBRID = r"C:\Users\username\.gemini\config\debrid.json"
CONFIG_PATH_RD = r"C:\Users\username\.gemini\config\realdebrid.json"

SERVICES_META = {
    "realdebrid": {
        "name": "RealDebrid",
        "api_url": "https://real-debrid.com/apitoken",
        "login_url": "https://real-debrid.com/"
    },
    "alldebrid": {
        "name": "AllDebrid",
        "api_url": "https://alldebrid.com/apikeys",
        "login_url": "https://alldebrid.com/"
    },
    "offcloud": {
        "name": "Offcloud",
        "api_url": "https://offcloud.com/api",
        "login_url": "https://offcloud.com/"
    },
    "torbox": {
        "name": "TorBox",
        "api_url": "https://torbox.app/settings?section=account",
        "login_url": "https://torbox.app/"
    },
    "premiumize": {
        "name": "Premiumize",
        "api_url": "https://www.premiumize.me/account",
        "login_url": "https://www.premiumize.me/login"
    },
    "debridlink": {
        "name": "DebridLink",
        "api_url": "https://debrid-link.fr/webapp/apikey",
        "login_url": "https://debrid-link.fr/webapp/login"
    },
    "easydebrid": {
        "name": "EasyDebrid",
        "api_url": "https://easydebrid.com/user/api",
        "login_url": "https://easydebrid.com/"
    }
}

def load_config():
    cfg = {
        "default_service": "realdebrid",
        "services": {},
        "idm_path": r"C:\Program Files (x86)\Internet Download Manager\IDMan.exe",
        "fdm_path": r"C:\Users\username\AppData\Local\Programs\Softdeluxe\Free Download Manager\fdm.exe",
        "default_downloader": "idm"
    }
    
    # Initialize services meta
    for k, v in SERVICES_META.items():
        cfg["services"][k] = {
            "name": v["name"],
            "api_key": "",
            "api_url": v["api_url"],
            "login_url": v["login_url"]
        }
        
    if os.path.exists(CONFIG_PATH_DEBRID):
        try:
            with open(CONFIG_PATH_DEBRID, "r", encoding="utf-8") as f:
                loaded = json.load(f)
                cfg.update(loaded)
        except Exception:
            pass
    elif os.path.exists(CONFIG_PATH_RD):
        try:
            with open(CONFIG_PATH_RD, "r", encoding="utf-8") as f:
                rd = json.load(f)
                if "api_key" in rd:
                    cfg["services"]["realdebrid"]["api_key"] = rd["api_key"]
                if "idm_path" in rd:
                    cfg["idm_path"] = rd["idm_path"]
                if "fdm_path" in rd:
                    cfg["fdm_path"] = rd["fdm_path"]
                if "default_downloader" in rd:
                    cfg["default_downloader"] = rd["default_downloader"]
        except Exception:
            pass
            
    return cfg

def save_config(cfg):
    with open(CONFIG_PATH_DEBRID, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)
    # Also sync realdebrid.json for legacy tools
    rd_key = cfg.get("services", {}).get("realdebrid", {}).get("api_key", "")
    rd_cfg = {
        "api_key": rd_key,
        "idm_path": cfg.get("idm_path"),
        "fdm_path": cfg.get("fdm_path"),
        "default_downloader": cfg.get("default_downloader")
    }
    with open(CONFIG_PATH_RD, "w", encoding="utf-8") as f:
        json.dump(rd_cfg, f, indent=2, ensure_ascii=False)

def get_headers(api_key):
    return {
        "Authorization": f"Bearer {api_key}",
        "User-Agent": "Mozilla/5.0"
    }

def get_rd_api_key():
    cfg = load_config()
    svc = cfg.get("services", {}).get("realdebrid", {})
    return svc.get("api_key") or cfg.get("api_key")

def api_request(endpoint, data=None, method="GET"):
    api_key = get_rd_api_key()
    url = f"https://api.real-debrid.com/rest/1.0/{endpoint}"
    headers = get_headers(api_key)
    
    encoded_data = None
    if data is not None:
        encoded_data = urllib.parse.urlencode(data).encode("utf-8")
        
    req = urllib.request.Request(url, data=encoded_data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            content = resp.read().decode("utf-8")
            return json.loads(content) if content else {}
    except urllib.error.HTTPError as e:
        err_msg = e.read().decode("utf-8") if hasattr(e, "read") else str(e)
        return {"error": True, "code": e.code, "message": err_msg}
    except Exception as e:
        return {"error": True, "message": str(e)}

def probe_streams(media_url):
    """Inspects audio/video streams of a direct URL using ffprobe without downloading the whole file."""
    try:
        cmd = [
            "ffprobe",
            "-v", "error",
            "-show_entries", "stream=index,codec_type,codec_name,channels,channel_layout,bit_rate:stream_tags=language,title",
            "-of", "json",
            media_url
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=25)
        if proc.returncode == 0:
            data = json.loads(proc.stdout)
            streams = data.get("streams", [])
            audio_tracks = []
            video_tracks = []
            subtitle_tracks = []
            
            for s in streams:
                ctype = s.get("codec_type")
                tags = s.get("tags") or {}
                lang = tags.get("language") or "und"
                title = tags.get("title") or ""
                codec = s.get("codec_name") or ""
                channels = s.get("channels") or 0
                ch_layout = s.get("channel_layout") or ""
                bitrate = s.get("bit_rate")
                
                track_info = {
                    "index": s.get("index"),
                    "codec": codec,
                    "language": lang,
                    "title": title,
                    "channels": channels,
                    "layout": ch_layout,
                    "bitrate": bitrate
                }
                
                if ctype == "audio":
                    audio_tracks.append(track_info)
                elif ctype == "video":
                    video_tracks.append(track_info)
                elif ctype == "subtitle":
                    subtitle_tracks.append(track_info)
                    
            russian_tracks = [
                a for a in audio_tracks 
                if a["language"].lower() in ["rus", "ru", "russian"] or "rus" in a["title"].lower()
            ]
            
            return {
                "success": True,
                "video": video_tracks,
                "audio": audio_tracks,
                "subtitles": subtitle_tracks,
                "audio_count": len(audio_tracks),
                "russian_count": len(russian_tracks),
                "has_russian": len(russian_tracks) > 0
            }
        else:
            return {"success": False, "error": proc.stderr}
    except Exception as e:
        return {"success": False, "error": str(e)}

def inspect_torrent_id(torrent_id):
    info = api_request(f"torrents/info/{torrent_id}")
    if info.get("error"):
        return info
    
    status = info.get("status")
    links = info.get("links", [])
    result = {
        "torrent_id": torrent_id,
        "filename": info.get("filename"),
        "bytes": info.get("bytes"),
        "status": status,
        "progress": info.get("progress"),
        "is_cached": status == "downloaded" and len(links) > 0,
        "unrestricted_links": []
    }
    
    if result["is_cached"]:
        for link in links:
            unres = api_request("unrestrict/link", data={"link": link}, method="POST")
            if not unres.get("error") and unres.get("download"):
                direct_url = unres.get("download")
                probe = probe_streams(direct_url)
                result["unrestricted_links"].append({
                    "filename": unres.get("filename"),
                    "filesize": unres.get("filesize"),
                    "download_url": direct_url,
                    "probe": probe
                })
    return result

def add_magnet_and_resolve(magnet_uri, target_season=None, target_episode=None):
    """Adds a magnet to RD, intelligently selects target episode file, and checks cache."""
    res = api_request("torrents/addMagnet", data={"magnet": magnet_uri}, method="POST")
    if res.get("error"):
        return res
    
    torrent_id = res.get("id")
    if not torrent_id:
        return {"error": True, "message": "No torrent ID returned"}
    
    # Get torrent info
    info = api_request(f"torrents/info/{torrent_id}")
    if info.get("status") == "waiting_files_selection":
        files = info.get("files", [])
        media_extensions = ('.mkv', '.mp4', '.avi', '.ts', '.m2ts')
        media_files = [f for f in files if any(f["path"].lower().endswith(ext) for ext in media_extensions)]
        
        selected_id = None
        if target_episode is not None and media_files:
            # 1. Look for explicit episode tag in file path
            e_pats = []
            if target_season is not None:
                e_pats.append(re.compile(rf's0*{target_season}\s*e0*{target_episode}\b', re.I))
                e_pats.append(re.compile(rf'{target_season}x0*{target_episode}\b', re.I))
                e_pats.append(re.compile(rf'{target_season}(?:nd|rd|th|st)?\s*season\s*[-_]\s*0*{target_episode}\b', re.I))
            e_pats.append(re.compile(rf'[-_\s]e0*{target_episode}\b', re.I))
            e_pats.append(re.compile(rf'[-_\s]ep0*{target_episode}\b', re.I))
            e_pats.append(re.compile(rf'[-_\s]0*{target_episode}\b', re.I))

            for pat in e_pats:
                for mf in media_files:
                    if pat.search(mf["path"]):
                        selected_id = str(mf["id"])
                        break
                if selected_id:
                    break
            
            # 2. Sequential fallback if it's a season batch pack
            if not selected_id and 1 <= target_episode <= len(media_files):
                sorted_files = sorted(media_files, key=lambda x: x["path"])
                selected_id = str(sorted_files[target_episode - 1]["id"])

        if selected_id:
            selection = selected_id
        else:
            selection = ",".join([str(f["id"]) for f in media_files]) if media_files else "all"

        api_request(f"torrents/selectFiles/{torrent_id}", data={"files": selection}, method="POST")
        time.sleep(1)
        
    return inspect_torrent_id(torrent_id)

QBITTORRENT_PATH = r"C:\Program Files\qBittorrent\qbittorrent.exe"

def send_to_downloader(download_url, downloader=None, output_dir=None, file_name=None):
    cfg = load_config()
    
    # Auto-detect if raw magnet and should go to qBittorrent
    if not downloader:
        if download_url.startswith("magnet:") or download_url.endswith(".torrent") or cfg.get("default_service") == "none":
            downloader = "qbittorrent" if os.path.exists(QBITTORRENT_PATH) else "aria2"
        else:
            downloader = cfg.get("default_downloader", "aria2")
            
    downloader = (downloader or "aria2").lower()
    if not output_dir:
        output_dir = os.path.expanduser(r"~\Downloads")
        
    if downloader in ["qbittorrent", "qbit"]:
        if os.path.exists(QBITTORRENT_PATH):
            subprocess.Popen([QBITTORRENT_PATH, download_url])
            return {
                "success": True,
                "downloader": "qBittorrent",
                "message": "تم فتح التورنت مباشرة داخل برنامج qBittorrent لمتابعة التحميل والتحكم بالسييد والحذف بسهولة"
            }
        else:
            downloader = "aria2"

    if downloader == "aria2":
        try:
            import shutil
            aria2_cmd = shutil.which("aria2c") or "aria2c"
            cmd = [
                aria2_cmd,
                "-s", "16",
                "-x", "16",
                "-k", "1M",
                "-j", "4",
                "-c",
                "--file-allocation=none",
                "--auto-file-renaming=false",
                "-d", output_dir,
                download_url
            ]
            if file_name:
                cmd.extend(["-o", file_name])
            subprocess.Popen(cmd, creationflags=subprocess.CREATE_NEW_CONSOLE)
            return {
                "success": True,
                "downloader": "Aria2",
                "connections": 16,
                "output_dir": output_dir,
                "message": "High-speed 16-connection download launched in Aria2 console"
            }
        except Exception as e:
            return {"success": False, "error": f"Failed to launch Aria2: {e}"}

    elif downloader == "idm":
        idm_path = cfg.get("idm_path")
        if not idm_path or not os.path.exists(idm_path):
            return {"success": False, "error": f"IDM not found at {idm_path}"}
        cmd = [idm_path, "/d", download_url]
        subprocess.Popen(cmd)
        return {"success": True, "downloader": "IDM", "message": "Download sent to IDM successfully"}
        
    elif downloader == "fdm":
        fdm_path = cfg.get("fdm_path")
        if not fdm_path or not os.path.exists(fdm_path):
            return {"success": False, "error": f"FDM not found at {fdm_path}"}
        cmd = [fdm_path, download_url]
        subprocess.Popen(cmd)
        return {"success": True, "downloader": "FDM", "message": "Download sent to FDM successfully"}
    else:
        return {"success": False, "error": f"Unknown downloader: {downloader}"}

HARBOR_MPV_PATH = r"C:\Users\username\AppData\Local\Harbor\mpv.exe"
HARBOR_EXE_PATH = r"C:\Users\username\AppData\Local\Harbor\harbor.exe"

def stream_to_player(video_path_or_url, subtitle_path=None, player=None):
    """Streams a direct Debrid URL or plays a local video file inside MPC-HC, PotPlayer, MPV, or VLC with subtitles."""
    if not player or player.lower() in ["default", "none", "auto"]:
        cfg = load_config()
        player = cfg.get("default_player", "mpc")
    player = player.lower()
    
    # Check player binary path dynamically via player_installer
    player_bin = None
    try:
        import player_installer
        player_bin = player_installer.get_installed_player_path(player)
    except Exception:
        pass
        
    if player in ["mpc", "mpc-hc", "mpchc"]:
        mpc_bin = player_bin or r"C:\Program Files (x86)\K-Lite Codec Pack\MPC-HC64\mpc-hc64.exe"
        if not os.path.exists(mpc_bin):
            # Fallback to another installed player if available
            return _handle_missing_player("mpc", video_path_or_url, subtitle_path)
        cmd = [mpc_bin, video_path_or_url]
        if subtitle_path and os.path.exists(subtitle_path):
            cmd.extend(["/sub", subtitle_path])
        subprocess.Popen(cmd)
        return {"success": True, "player": "MPC-HC", "target": video_path_or_url, "subtitle": subtitle_path, "message": "Opened in MPC-HC successfully"}

    elif player == "potplayer":
        pot_bin = player_bin or r"C:\Program Files\DAUM\PotPlayer\PotPlayer64.exe"
        if not os.path.exists(pot_bin):
            return _handle_missing_player("potplayer", video_path_or_url, subtitle_path)
        cmd = [pot_bin, video_path_or_url]
        if subtitle_path and os.path.exists(subtitle_path):
            cmd.append(f"/sub={subtitle_path}")
            cmd.append(subtitle_path)
        subprocess.Popen(cmd)
        return {"success": True, "player": "PotPlayer", "target": video_path_or_url, "subtitle": subtitle_path, "message": "Opened in PotPlayer successfully"}
        
    elif player in ["mpv", "harbor"]:
        mpv_bin = player_bin or (HARBOR_MPV_PATH if os.path.exists(HARBOR_MPV_PATH) else "mpv")
        cmd = [mpv_bin, video_path_or_url, "--force-window=yes"]
        if subtitle_path and os.path.exists(subtitle_path):
            cmd.append(f"--sub-file={subtitle_path}")
        try:
            subprocess.Popen(cmd)
            return {"success": True, "player": "MPV", "target": video_path_or_url, "subtitle": subtitle_path, "message": "Opened in MPV engine successfully"}
        except Exception:
            return _handle_missing_player("mpv", video_path_or_url, subtitle_path)
        
    elif player == "vlc":
        vlc_bin = player_bin or r"C:\Program Files\VideoLAN\VLC\vlc.exe"
        if not os.path.exists(vlc_bin):
            return _handle_missing_player("vlc", video_path_or_url, subtitle_path)
        cmd = [vlc_bin, video_path_or_url]
        if subtitle_path and os.path.exists(subtitle_path):
            cmd.append(f"--sub-file={subtitle_path}")
        subprocess.Popen(cmd)
        return {"success": True, "player": "VLC", "target": video_path_or_url, "subtitle": subtitle_path, "message": "Opened in VLC successfully"}
        
    return {"error": True, "message": f"Unknown or unsupported player: {player}. Supported: mpc, potplayer, mpv, vlc"}

def _handle_missing_player(requested_player, video_url_or_path, subtitle_path):
    """Graceful fallback if requested player is not installed on the system."""
    import player_installer
    all_players = player_installer.check_all_players()
    
    # Find any installed player
    fallback = None
    for p_key in ["mpc", "potplayer", "mpv", "vlc"]:
        if p_key != requested_player and all_players.get(p_key, {}).get("installed"):
            fallback = p_key
            break
            
    if fallback:
        res = stream_to_player(video_url_or_path, subtitle_path=subtitle_path, player=fallback)
        res["warning"] = f"مشغل {requested_player} غير مثبت على هذا الجهاز. تم التشغيل تلقائياً عبر {fallback} لضمان استمرار المشاهدة."
        res["auto_install_command"] = f"python player_installer.py install {requested_player}"
        return res
        
    return {
        "error": True,
        "message": f"مشغل {requested_player} غير مثبت على هذا الجهاز ولا يوجد مشغل بديل مثبت.",
        "auto_install_available": True,
        "winget_id": all_players.get(requested_player, {}).get("winget_id"),
        "official_url": all_players.get(requested_player, {}).get("official_url"),
        "how_to_install": f"يمكنك تثبيته تلقائياً عبر أمر: python player_installer.py install {requested_player} أو تحميله من موقعه الرسمي."
    }

def smart_stream_or_download(media_url_or_path, mode="stream", player=None, query=None, imdb_id=None, release_type=None):
    """
    Intelligent unified controller for streaming or downloading:
    - mode='stream': Checks if stream has embedded Arabic subtitles.
                     If missing, auto-fetches best Arabic subtitle and launches selected player (defaults to MPC-HC).
    - mode='download': Downloads via Aria2 (16 connections) at maximum internet line speed.
    """
    mode = mode.lower()
    if not player or player.lower() in ["default", "none", "auto"]:
        cfg = load_config()
        player = cfg.get("default_player", "mpc")
    if mode == "download":
        res = send_to_downloader(media_url_or_path, downloader="aria2")
        if query or imdb_id:
            try:
                import subtitle_manager
                s_res = subtitle_manager.search_all_subtitles(query or "", imdb_id=imdb_id, lang="ar", release_type=release_type)
                subs = s_res.get("all_subtitles", [])
                if subs:
                    out_dir = os.path.expanduser(r"~\Downloads")
                    sub_dl = subtitle_manager.download_subtitle_file(subs[0], out_dir)
                    res["subtitle_prepared"] = sub_dl
            except Exception as e:
                res["subtitle_error"] = str(e)
        return res

    elif mode == "stream":
        has_arabic = False
        try:
            probe_res = probe_streams(media_url_or_path)
            for s in probe_res.get("subtitles", []):
                lng = str(s.get("language", "")).lower()
                tit = str(s.get("title", "")).lower()
                if lng in ["ara", "ar", "arabic"] or "arabic" in tit or "عربي" in tit:
                    has_arabic = True
                    break
        except Exception:
            pass

        sub_file = None
        if has_arabic:
            sub_msg = "الترجمة العربية مدمجة رسمياً داخل مسارات الفيديو"
        else:
            sub_msg = "الملف لا يحتوي ترجمة عربية مدمجة"
            if query or imdb_id:
                try:
                    import subtitle_manager
                    s_res = subtitle_manager.search_all_subtitles(query or "", imdb_id=imdb_id, lang="ar", release_type=release_type)
                    subs = s_res.get("ranked_subtitles", [])
                    if subs:
                        out_dir = os.path.expanduser(r"~\Downloads")
                        # Pick first candidate
                        sub_dl = subtitle_manager.download_subtitle_file(subs[0], out_dir, file_basename=f"{query or 'movie'}.ar")
                        if sub_dl.get("success") and sub_dl.get("files"):
                            sub_file = sub_dl["files"][0]
                            sub_msg = f"تم تحميل أفضل ترجمة عربية مطابقة تلقائياً: {os.path.basename(sub_file)}"
                except Exception as e:
                    sub_msg += f" (تعذر جلب الترجمة: {e})"

        p_res = stream_to_player(media_url_or_path, subtitle_path=sub_file, player=player)
        p_res["has_embedded_arabic"] = has_arabic
        p_res["subtitle_status"] = sub_msg
        return p_res

    return {"error": True, "message": f"Unknown mode: {mode}. Must be 'stream' or 'download'"}

def get_harbor_info(imdb_id, media_type="movie", season=None, episode=None):
    """Generates direct Harbor deep link protocol."""
    clean_id = imdb_id.strip() if imdb_id.strip().startswith("tt") else f"tt{imdb_id.strip()}"
    mtype = media_type.lower()
    
    if season and episode:
        target = f"{clean_id}:{season}:{episode}"
    else:
        target = clean_id
        
    harbor_link = f"harbor://detail/{mtype}/{target}"
    
    return {
        "harbor_link": harbor_link,
        "description": "Opens Harbor directly to this title with your configured Real-Debrid and subtitle addons ready."
    }

def get_dmm_info(target_id_or_title, media_type="movie", season=None):
    """Generates direct Debrid Media Manager (DMM) URLs and metadata."""
    clean_target = str(target_id_or_title).strip()
    is_imdb = bool(re.match(r"^tt\d+$", clean_target.lower()))
    if is_imdb:
        clean_imdb = clean_target.lower()
        if media_type == "show" or season is not None:
            url = f"https://debridmediamanager.com/show/{clean_imdb}" + (f"/{season}" if season else "")
        else:
            url = f"https://debridmediamanager.com/movie/{clean_imdb}"
    else:
        url = f"https://debridmediamanager.com/search?query={urllib.parse.quote(clean_target)}"
        
    return {
        "service": "Debrid Media Manager (DMM)",
        "dmm_url": url,
        "is_imdb": is_imdb,
        "description": "Browse and manage cached/uncached torrents directly on DMM with 1-click cloud library addition."
    }

def list_recent():
    torrents = api_request("torrents?limit=10")
    if isinstance(torrents, list):
        return torrents
    return []

def list_services():
    cfg = load_config()
    res = {
        "default_service": cfg.get("default_service", "realdebrid"),
        "services": {}
    }
    for k, v in cfg.get("services", {}).items():
        res["services"][k] = {
            "name": v.get("name"),
            "has_key": bool(v.get("api_key")),
            "api_url": v.get("api_url"),
            "login_url": v.get("login_url")
        }
    return res

def set_service_key(service_name, key):
    cfg = load_config()
    sname = service_name.lower().replace("-", "").replace("_", "")
    matched = None
    for k in cfg.get("services", {}):
        if k.replace("-", "").replace("_", "") == sname:
            matched = k
            break
    if not matched:
        return {"error": True, "message": f"Service '{service_name}' not supported. Supported: {list(SERVICES_META.keys())}"}
        
    cfg["services"][matched]["api_key"] = key.strip()
    save_config(cfg)
    return {"success": True, "service": matched, "message": f"API key updated for {matched}"}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage:")
        print("  python debrid_manager.py resolve <magnet>")
        print("  python debrid_manager.py stream <url_or_path> [player] [query] [imdb_id] [release_type]")
        print("  python debrid_manager.py download <url> [aria2|idm] [out_dir] [filename]")
        print("  python debrid_manager.py play <url_or_path> [sub_path] [potplayer|mpv|vlc]")
        print("  python debrid_manager.py info <torrent_id>")
        print("  python debrid_manager.py probe <url>")
        print("  python debrid_manager.py services")
        print("  python debrid_manager.py set-key <service> <api_key>")
        print("  python debrid_manager.py list")
        sys.exit(1)
        
    action = sys.argv[1].lower()
    if action == "resolve" and len(sys.argv) >= 3:
        magnet = sys.argv[2]
        res = add_magnet_and_resolve(magnet)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "stream" and len(sys.argv) >= 3:
        target = sys.argv[2]
        plr = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        qry = sys.argv[4] if len(sys.argv) > 4 and sys.argv[4] != "none" else None
        imdb = sys.argv[5] if len(sys.argv) > 5 and sys.argv[5] != "none" else None
        rel = sys.argv[6] if len(sys.argv) > 6 and sys.argv[6] != "none" else None
        res = smart_stream_or_download(target, mode="stream", player=plr, query=qry, imdb_id=imdb, release_type=rel)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "download" and len(sys.argv) >= 3:
        url = sys.argv[2]
        dl = sys.argv[3] if len(sys.argv) > 3 else "aria2"
        out_d = sys.argv[4] if len(sys.argv) > 4 else None
        fname = sys.argv[5] if len(sys.argv) > 5 else None
        res = send_to_downloader(url, dl, output_dir=out_d, file_name=fname)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "info" and len(sys.argv) >= 3:
        tid = sys.argv[2]
        res = inspect_torrent_id(tid)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "probe" and len(sys.argv) >= 3:
        url = sys.argv[2]
        res = probe_streams(url)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "services":
        print(json.dumps(list_services(), indent=2, ensure_ascii=False))
    elif action == "set-key" and len(sys.argv) >= 4:
        svc = sys.argv[2]
        k = sys.argv[3]
        print(json.dumps(set_service_key(svc, k), indent=2, ensure_ascii=False))
    elif action == "play" and len(sys.argv) >= 3:
        target_media = sys.argv[2]
        sub = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] != "none" else None
        plr = sys.argv[4] if len(sys.argv) > 4 else "potplayer"
        res = stream_to_player(target_media, sub, plr)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action in ["harbor", "stremio"] and len(sys.argv) >= 3:
        imdb = sys.argv[2]
        mtype = sys.argv[3] if len(sys.argv) > 3 else "movie"
        snum = sys.argv[4] if len(sys.argv) > 4 else None
        enum = sys.argv[5] if len(sys.argv) > 5 else None
        res = get_harbor_info(imdb, mtype, snum, enum)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "dmm" and len(sys.argv) >= 3:
        target = sys.argv[2]
        mtype = sys.argv[3] if len(sys.argv) > 3 else "movie"
        snum = sys.argv[4] if len(sys.argv) > 4 else None
        res = get_dmm_info(target, mtype, snum)
        print(json.dumps(res, indent=2, ensure_ascii=False))
    elif action == "list":
        res = list_recent()
        print(json.dumps(res, indent=2, ensure_ascii=False))
    else:
        print("Invalid arguments")
