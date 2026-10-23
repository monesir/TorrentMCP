# -*- coding: utf-8 -*-
"""
TorrentX Unified Bot Launcher
Runs both Telegram and Discord bots concurrently.
Works interactively or silently in the background with ~0% CPU/RAM impact.
"""
import os
import sys
import subprocess
import time

CURRENT_DIR = os.path.dirname(os.path.abspath(__file__))

def run():
    python_exe = sys.executable
    is_bg = "--background" in sys.argv or "-b" in sys.argv or "--silent" in sys.argv

    if is_bg and sys.platform == "win32":
        pyw = os.path.join(os.path.dirname(python_exe), "pythonw.exe")
        if os.path.exists(pyw):
            python_exe = pyw

    tg_script = os.path.join(CURRENT_DIR, "telegram_bot.py")
    dc_script = os.path.join(CURRENT_DIR, "discord_bot.py")

    procs = []
    if os.path.exists(tg_script):
        p_tg = subprocess.Popen([python_exe, tg_script], cwd=CURRENT_DIR)
        procs.append(p_tg)

    if os.path.exists(dc_script):
        p_dc = subprocess.Popen([python_exe, dc_script], cwd=CURRENT_DIR)
        procs.append(p_dc)

    if is_bg:
        # Exit parent process, children keep running
        return

    print("==================================================")
    print(" TorrentX Bots Running (Telegram + Discord)")
    print(" Resource usage: ~0% CPU / RAM")
    print(" Press Ctrl+C to stop all bots.")
    print("==================================================")
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        for p in procs:
            try:
                p.terminate()
            except Exception:
                pass
        print("\nAll bots stopped successfully.")

if __name__ == "__main__":
    run()
