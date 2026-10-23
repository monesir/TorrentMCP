import os
import sys
import subprocess
import shutil
import webbrowser

PLAYER_REGISTRY = {
    "mpc": {
        "name": "MPC-HC (Media Player Classic)",
        "winget_id": "clsid2.mpc-hc",
        "official_url": "https://github.com/clsid2/mpc-hc/releases",
        "known_paths": [
            r"C:\Program Files (x86)\K-Lite Codec Pack\MPC-HC64\mpc-hc64.exe",
            r"C:\Program Files\MPC-HC\mpc-hc64.exe",
            r"C:\Program Files (x86)\MPC-HC\mpc-hc.exe",
            r"C:\Program Files\K-Lite Codec Pack\MPC-HC64\mpc-hc64.exe"
        ]
    },
    "potplayer": {
        "name": "PotPlayer",
        "winget_id": "Daum.PotPlayer",
        "official_url": "https://potplayer.daum.net/",
        "known_paths": [
            r"C:\Program Files\DAUM\PotPlayer\PotPlayer64.exe",
            r"C:\Program Files (x86)\DAUM\PotPlayer\PotPlayer.exe"
        ]
    },
    "mpv": {
        "name": "MPV Player",
        "winget_id": "shinchiro.mpv",
        "official_url": "https://mpv.io/installation/",
        "known_paths": [
            r"C:\Users\username\AppData\Local\Harbor\mpv.exe",
            os.path.expanduser(r"~\AppData\Local\Harbor\mpv.exe"),
            r"C:\Program Files\mpv\mpv.exe",
            r"C:\mpv\mpv.exe"
        ]
    },
    "vlc": {
        "name": "VLC Media Player",
        "winget_id": "VideoLAN.VLC",
        "official_url": "https://www.videolan.org/vlc/",
        "known_paths": [
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe"
        ]
    }
}

def get_installed_player_path(player_key):
    """Returns executable path if player is installed, else None."""
    p_meta = PLAYER_REGISTRY.get(player_key.lower())
    if not p_meta:
        return None
        
    # Check known paths
    for path in p_meta["known_paths"]:
        if os.path.exists(path):
            return path
            
    # Check PATH environment
    which_bin = shutil.which(player_key)
    if which_bin and os.path.exists(which_bin):
        return which_bin
        
    return None

def check_all_players():
    """Checks the status of all supported players."""
    status = {}
    for key, meta in PLAYER_REGISTRY.items():
        found_path = get_installed_player_path(key)
        status[key] = {
            "name": meta["name"],
            "installed": bool(found_path),
            "path": found_path,
            "winget_id": meta["winget_id"],
            "official_url": meta["official_url"]
        }
    return status

def install_player_via_winget(player_key):
    """Installs player using Windows Package Manager in a separate console window."""
    meta = PLAYER_REGISTRY.get(player_key.lower())
    if not meta:
        return {"success": False, "error": f"Unknown player: {player_key}"}
        
    winget_bin = shutil.which("winget") or "winget"
    cmd = [
        winget_bin, "install",
        "--id", meta["winget_id"],
        "--accept-source-agreements",
        "--accept-package-agreements"
    ]
    
    try:
        # Launch winget in a new visible console so user can see progress and handle any UAC prompt
        proc = subprocess.Popen(cmd, creationflags=subprocess.CREATE_NEW_CONSOLE)
        return {
            "success": True,
            "method": "winget",
            "winget_id": meta["winget_id"],
            "player": meta["name"],
            "message": f"بدأ تثبيت مشغل {meta['name']} عبر Windows Package Manager (winget). تابع نافذة التثبيت."
        }
    except Exception as e:
        # Fallback to browser
        webbrowser.open(meta["official_url"])
        return {
            "success": False,
            "error": str(e),
            "fallback": "opened_browser",
            "url": meta["official_url"],
            "message": f"تعذر تشغيل winget تلقائياً، تم فتح الموقع الرسمي لتحميل {meta['name']}."
        }

def open_official_site(player_key):
    """Opens the official download page for the specified player."""
    meta = PLAYER_REGISTRY.get(player_key.lower())
    if meta and meta.get("official_url"):
        webbrowser.open(meta["official_url"])
        return {"success": True, "url": meta["official_url"]}
    return {"success": False, "error": "Unknown player"}

if __name__ == "__main__":
    import json
    if len(sys.argv) > 1 and sys.argv[1] == "install" and len(sys.argv) > 2:
        res = install_player_via_winget(sys.argv[2])
        print(json.dumps(res, indent=2, ensure_ascii=False))
    else:
        status = check_all_players()
        print(json.dumps(status, indent=2, ensure_ascii=False))
