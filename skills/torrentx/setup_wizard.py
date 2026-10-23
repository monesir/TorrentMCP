# -*- coding: utf-8 -*-
import os
import sys
import json
import webbrowser

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
try:
    import player_installer
except ImportError:
    player_installer = None

CONFIG_DIR = os.path.expanduser(r"~/.gemini/config")
DEBRID_CONFIG_PATH = os.path.join(CONFIG_DIR, "debrid.json")
SUBTITLE_CONFIG_PATH = os.path.join(CONFIG_DIR, "subtitles.json")
TRACKED_CONFIG_PATH = os.path.join(CONFIG_DIR, "tracked_series.json")

DEBRID_PROVIDERS = {
    "None": {
        "id": "none",
        "name": "بدون ديبرد (خيار مجاني)",
        "api_url": None,
        "token_label": None,
        "note": "سيتم التنزيل مباشرة عبر شبكة التورنت العامة بواسطة برنامج qBittorrent\nيُنصح دائماً بالتحميل الكامل أولاً لتفادي التقطيع أثناء المشاهدة\nإلا في حال توفر عدد كبير جداً من مصادر المشاركة النشطة"
    },
    "RealDebrid": {
        "id": "realdebrid",
        "name": "Real-Debrid",
        "api_url": "https://real-debrid.com/apitoken",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة سحابية عالمية لتحميل وتشغيل ملفات التورنت بأقصى سرعة اتصال وبدون أي انتظار"
    },
    "Premiumize": {
        "id": "premiumize",
        "name": "Premiumize.me",
        "api_url": "https://www.premiumize.me/account",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة سحابية تتيح مساحة تخزين خاصة وتحميل التورنت بروابط مباشرة فائقة السرعة"
    },
    "AllDebrid": {
        "id": "alldebrid",
        "name": "AllDebrid",
        "api_url": "https://alldebrid.com/apikeys",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة سريعة لسحب ملفات التورنت وتحويلها إلى روابط مباشرة تدعم الاستكمال"
    },
    "DebridLink": {
        "id": "debridlink",
        "name": "Debrid-Link",
        "api_url": "https://debrid-link.fr/webapp/apikey",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة ديبرد تدعم شبكات التورنت وسيرفرات الاستضافة مع دعم المشاهدة المباشرة"
    },
    "EasyDebrid": {
        "id": "easydebrid",
        "name": "EasyDebrid",
        "api_url": "https://easydebrid.com/user/api",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة حديثة وسريعة متخصصة في كاش التورنت والتنزيل المباشر الفوري"
    },
    "OffCloud": {
        "id": "offcloud",
        "name": "OffCloud",
        "api_url": "https://offcloud.com/api",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة سحابية لتنزيل ملفات التورنت وتخزينها في السحابة مع روابط سريعة"
    },
    "TorBox": {
        "id": "torbox",
        "name": "TorBox",
        "api_url": "https://torbox.app/settings?section=account",
        "token_label": "رمز المفتاح السري للخدمة",
        "note": "خدمة مخصصة لملفات التورنت بخصوصية تامة وسرعات عالية وميزات متطورة"
    }
}

def load_json(path, default):
    if os.path.exists(path):
        try:
            with open(path, "r", encoding="utf-8") as f:
                d = json.load(f)
                default.update(d)
        except Exception:
            pass
    return default

def save_json(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)

def run_gui_wizard():
    """Dark Mode graphical wizard matching TorrentX AI Assistant design."""
    import tkinter as tk
    from tkinter import ttk, messagebox

    root = tk.Tk()
    root.title("معالج إعداد تورنت إكس الشامل (Dark Mode)")
    root.geometry("760x900")
    root.minsize(700, 780)
    root.configure(bg="#0b0f19")

    # Dark ttk style
    style = ttk.Style(root)
    style.theme_use("clam")
    style.configure("Dark.TCombobox", fieldbackground="#1e293b", background="#334155", foreground="#f8fafc", arrowcolor="#38bdf8")
    style.configure("Dark.Vertical.TScrollbar", background="#1e293b", troughcolor="#0b0f19", bordercolor="#0b0f19", arrowcolor="#94a3b8")

    # Load configs
    debrid_cfg = load_json(DEBRID_CONFIG_PATH, {
        "default_service": "realdebrid",
        "services": {},
        "default_player": "mpc",
        "auto_install_missing_players": True,
        "non_debrid_torrent_client": "qbittorrent"
    })
    
    if not debrid_cfg.get("default_player") or debrid_cfg.get("default_player") == "potplayer":
        debrid_cfg["default_player"] = "mpc"

    sub_cfg = load_json(SUBTITLE_CONFIG_PATH, {
        "services": {
            "subdl": {"api_key": "", "api_url": "https://subdl.com/panel/api"},
            "subsource": {"api_key": "", "api_url": "https://subsource.net/dashboard/api"}
        }
    })

    series_cfg = load_json(TRACKED_CONFIG_PATH, {
        "tracked_shows": [],
        "private_trackers": []
    })

    # ==========================================
    # 1. HEADER (TOP)
    # ==========================================
    header = tk.Frame(root, bg="#090d16", pady=16)
    header.pack(side="top", fill="x")
    title_lbl = tk.Label(header, text="⚙️ معالج إعداد مهارة تورنت إكس المتطورة", font=("Segoe UI", 15, "bold"), fg="#38bdf8", bg="#090d16", justify="center")
    title_lbl.pack(anchor="center")
    sub_lbl = tk.Label(header, text="تخصيص خدمات الديبرد، مشغل الفيديو، والترجمات العربية بكل سهولة", font=("Segoe UI", 9), fg="#94a3b8", bg="#090d16", justify="center")
    sub_lbl.pack(anchor="center", pady=3)

    # ==========================================
    # 2. SAVE BAR (BOTTOM)
    # ==========================================
    save_frame = tk.Frame(root, bg="#090d16", pady=14, highlightbackground="#1e293b", highlightthickness=1)
    save_frame.pack(side="bottom", fill="x")

    # ==========================================
    # 3. SCROLLABLE MIDDLE CONTAINER
    # ==========================================
    canvas_container = tk.Frame(root, bg="#0b0f19")
    canvas_container.pack(side="top", fill="both", expand=True)

    canvas = tk.Canvas(canvas_container, bg="#0b0f19", borderwidth=0, highlightthickness=0)
    scrollbar = ttk.Scrollbar(canvas_container, orient="vertical", command=canvas.yview, style="Dark.Vertical.TScrollbar")
    scrollable_frame = tk.Frame(canvas, bg="#0b0f19", padx=16, pady=10)

    canvas_window = canvas.create_window((380, 0), window=scrollable_frame, anchor="n")

    def _on_canvas_configure(event):
        canvas.coords(canvas_window, event.width // 2, 0)
        usable_w = max(660, event.width - 32)
        canvas.itemconfig(canvas_window, width=usable_w)

    canvas.bind("<Configure>", _on_canvas_configure)
    scrollable_frame.bind("<Configure>", lambda e: canvas.configure(scrollregion=canvas.bbox("all")))

    canvas.configure(yscrollcommand=scrollbar.set)
    canvas.pack(side="left", fill="both", expand=True)
    scrollbar.pack(side="right", fill="y")

    def _on_mousewheel(event):
        canvas.yview_scroll(int(-1 * (event.delta / 120)), "units")
    canvas.bind_all("<MouseWheel>", _on_mousewheel)

    # Card factory helper for consistent dark theme
    def create_card(title):
        card = tk.LabelFrame(
            scrollable_frame,
            text=f" {title} ",
            font=("Segoe UI", 11, "bold"),
            fg="#38bdf8",
            bg="#0f172a",
            highlightbackground="#1e293b",
            highlightthickness=1,
            padx=16,
            pady=12
        )
        card.pack(fill="x", pady=8, anchor="center")
        return card

    # ==========================================
    # SECTION 1: DEBRID PROVIDER
    # ==========================================
    sec1 = create_card("1. مزود خدمة ديبرد السحابية")

    deb_top_frame = tk.Frame(sec1, bg="#0f172a")
    deb_top_frame.pack(anchor="center", pady=6)

    tk.Label(deb_top_frame, text="اختر المزود:", font=("Segoe UI", 10, "bold"), fg="#f8fafc", bg="#0f172a").pack(side="left", padx=8)

    cur_service_id = debrid_cfg.get("default_service", "realdebrid")
    provider_names = list(DEBRID_PROVIDERS.keys())
    
    current_prov_name = "RealDebrid"
    if cur_service_id == "none" or not cur_service_id:
        current_prov_name = "None"
    else:
        for k, v in DEBRID_PROVIDERS.items():
            if v["id"] == cur_service_id:
                current_prov_name = k
                break

    provider_var = tk.StringVar(value=current_prov_name)
    provider_combo = ttk.Combobox(deb_top_frame, textvariable=provider_var, values=provider_names, state="readonly", width=18, font=("Segoe UI", 9, "bold"), style="Dark.TCombobox")
    provider_combo.pack(side="left", padx=6)

    token_btn = tk.Button(deb_top_frame, text="جلب المفتاح من الموقع 🔗", font=("Segoe UI", 9, "bold"), bg="#0284c7", activebackground="#0369a1", fg="white", activeforeground="white", relief="flat", padx=10, pady=4, cursor="hand2")
    token_btn.pack(side="left", padx=8)

    # API Key Row
    key_frame = tk.Frame(sec1, bg="#0f172a")
    key_frame.pack(anchor="center", pady=6)

    key_label = tk.Label(key_frame, text="رمز المفتاح السري", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=18, anchor="e")
    key_label.pack(side="left", padx=4)
    
    key_entry = tk.Entry(key_frame, font=("Consolas", 10), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat", width=42)
    key_entry.pack(side="left", padx=6, ipady=3)

    # Note Label
    note_lbl = tk.Label(sec1, text="", font=("Segoe UI", 9), fg="#94a3b8", bg="#0f172a", justify="center")
    note_lbl.pack(anchor="center", pady=5)

    # Warning Frame for Non-Debrid
    free_warn_frame = tk.Frame(sec1, bg="#1c1917", padx=14, pady=10, highlightbackground="#854d0e", highlightthickness=1)
    free_warn_lbl = tk.Label(
        free_warn_frame,
        text="⚠️ تنبيه لمستخدمي الخيار المجاني بدون ديبرد\n\n"
             "• يتم التنزيل مباشرة عبر برنامج qBittorrent بأعلى سرعة متوفرة لديكم.\n"
             "• يُفضل تنزيل الملف بالكامل أولاً لتفادي التقطيع أثناء المشاهدة.\n"
             "• يقوم النظام باختيار النسخة ذات أعلى مشاركة وسيدرز تلقائياً.",
        font=("Segoe UI", 9),
        fg="#fde047",
        bg="#1c1917",
        justify="center"
    )
    free_warn_lbl.pack(anchor="center", fill="x", pady=4)

    def update_provider_ui(*args):
        p_name = provider_var.get()
        p_meta = DEBRID_PROVIDERS.get(p_name, DEBRID_PROVIDERS["None"])
        p_id = p_meta["id"]

        if p_name == "None":
            token_btn.config(state="disabled", bg="#334155")
            key_entry.delete(0, tk.END)
            key_entry.config(state="disabled", bg="#090d16")
            key_label.config(text="المفتاح السري غير مطلوب")
            note_lbl.config(text=p_meta["note"], fg="#64748b")
            free_warn_frame.pack(anchor="center", fill="x", pady=6)
        else:
            token_btn.config(state="normal", bg="#0284c7")
            key_entry.config(state="normal", bg="#1e293b")
            key_label.config(text=p_meta["token_label"] or "رمز المفتاح السري")
            
            existing_key = debrid_cfg.get("services", {}).get(p_id, {}).get("api_key", "")
            key_entry.delete(0, tk.END)
            if existing_key:
                key_entry.insert(0, existing_key)
                
            note_lbl.config(text=p_meta["note"], fg="#38bdf8")
            free_warn_frame.pack_forget()

            def _open_url():
                if p_meta["api_url"]:
                    webbrowser.open(p_meta["api_url"])
            token_btn.config(command=_open_url)

    provider_combo.bind("<<ComboboxSelected>>", update_provider_ui)
    update_provider_ui()

    # ==========================================
    # SECTION 2: MEDIA PLAYER
    # ==========================================
    sec2 = create_card("2. مشغل الفيديو المفضل")

    player_var = tk.StringVar(value="mpc")
    cur_p = debrid_cfg.get("default_player", "mpc").lower()
    if cur_p in ["mpc", "mpc-hc", "mpchc"]:
        player_var.set("mpc")
    elif cur_p in ["potplayer", "mpv", "vlc"]:
        player_var.set(cur_p)
    else:
        player_var.set("mpc")

    auto_install_var = tk.BooleanVar(value=debrid_cfg.get("auto_install_missing_players", True))
    auto_inst_chk = tk.Checkbutton(
        sec2,
        text="تثبيت المشغل تلقائياً بواسطة نظام ويندوز في حال لم يكن متوفراً بالجهاز",
        variable=auto_install_var,
        font=("Segoe UI", 9, "bold"),
        fg="#38bdf8",
        bg="#0f172a",
        activebackground="#0f172a",
        activeforeground="#38bdf8",
        selectcolor="#090d16"
    )
    auto_inst_chk.pack(anchor="center", pady=5)

    plr_frame = tk.Frame(sec2, bg="#0f172a")
    plr_frame.pack(anchor="center", pady=4)

    players_meta = [
        ("mpc", "مشغل MPC-HC (الخيار المفضل والموصى به)", "clsid2.mpc-hc"),
        ("potplayer", "مشغل PotPlayer", "Daum.PotPlayer"),
        ("mpv", "مشغل MPV", "shinchiro.mpv"),
        ("vlc", "مشغل VLC Media Player", "VideoLAN.VLC")
    ]

    installed_map = {}
    if player_installer:
        all_status = player_installer.check_all_players()
        for k, v in all_status.items():
            installed_map[k] = v.get("installed", False)

    for val, name, w_id in players_meta:
        row = tk.Frame(plr_frame, bg="#0f172a", pady=4)
        row.pack(anchor="center", fill="x")

        is_inst = installed_map.get(val, True)
        badge_text = "(متوفر بالجهاز ✅)" if is_inst else "(غير مثبت ⬇️)"
        badge_color = "#22c55e" if is_inst else "#f97316"

        rb = tk.Radiobutton(
            row,
            text=name,
            variable=player_var,
            value=val,
            font=("Segoe UI", 9, "bold" if val == "mpc" else "normal"),
            fg="#f8fafc",
            bg="#0f172a",
            activebackground="#0f172a",
            activeforeground="#38bdf8",
            selectcolor="#090d16",
            width=32,
            anchor="w"
        )
        rb.pack(side="left")

        b_lbl = tk.Label(row, text=badge_text, font=("Segoe UI", 8, "bold"), fg=badge_color, bg="#0f172a", width=15)
        b_lbl.pack(side="left", padx=4)

        if not is_inst and player_installer:
            def make_install_cmd(v_val):
                return lambda: player_installer.install_player_via_winget(v_val)
            def make_web_cmd(v_val):
                return lambda: player_installer.open_official_site(v_val)

            btn_inst = tk.Button(row, text="تثبيت تلقائي ⚡", font=("Segoe UI", 8, "bold"), bg="#0284c7", activebackground="#0369a1", fg="white", relief="flat", cursor="hand2", padx=6, command=make_install_cmd(val))
            btn_inst.pack(side="left", padx=3)

            btn_web = tk.Button(row, text="الموقع الرسمي 🌐", font=("Segoe UI", 8), bg="#334155", activebackground="#475569", fg="#cbd5e1", relief="flat", cursor="hand2", padx=6, command=make_web_cmd(val))
            btn_web.pack(side="left", padx=2)

    plr_desc = tk.Label(
        sec2,
        text="في حال اختيار مشغل غير مثبت سيقوم النظام بتثبيته بنقرة واحدة أو تحويل البث للمشغل المتوفر لديك.",
        font=("Segoe UI", 8),
        fg="#64748b",
        bg="#0f172a",
        justify="center"
    )
    plr_desc.pack(anchor="center", pady=6)

    # ==========================================
    # SECTION 3: SUBTITLES
    # ==========================================
    sec3 = create_card("3. مفاتيح مواقع الترجمة العربية")

    sub_frame = tk.Frame(sec3, bg="#0f172a")
    sub_frame.pack(anchor="center", pady=4)

    # SubDL row
    subdl_row = tk.Frame(sub_frame, bg="#0f172a", pady=4)
    subdl_row.pack(anchor="center")
    tk.Label(subdl_row, text="مفتاح موقع SubDL", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=16, anchor="e").pack(side="left")
    subdl_entry = tk.Entry(subdl_row, width=34, font=("Consolas", 10), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat")
    subdl_entry.insert(0, sub_cfg.get("services", {}).get("subdl", {}).get("api_key", ""))
    subdl_entry.pack(side="left", padx=6, ipady=2)
    tk.Button(subdl_row, text="فتح موقع الخدمة 🔗", font=("Segoe UI", 8), bg="#334155", activebackground="#475569", fg="#cbd5e1", relief="flat", cursor="hand2", command=lambda: webbrowser.open("https://subdl.com/panel/api")).pack(side="left")

    # Subsource row
    subsrc_row = tk.Frame(sub_frame, bg="#0f172a", pady=4)
    subsrc_row.pack(anchor="center")
    tk.Label(subsrc_row, text="مفتاح موقع Subsource", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=16, anchor="e").pack(side="left")
    subsrc_entry = tk.Entry(subsrc_row, width=34, font=("Consolas", 10), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat")
    subsrc_entry.insert(0, sub_cfg.get("services", {}).get("subsource", {}).get("api_key", ""))
    subsrc_entry.pack(side="left", padx=6, ipady=2)
    tk.Button(subsrc_row, text="فتح موقع الخدمة 🔗", font=("Segoe UI", 8), bg="#334155", activebackground="#475569", fg="#cbd5e1", relief="flat", cursor="hand2", command=lambda: webbrowser.open("https://subsource.net/dashboard/api")).pack(side="left")

    sub_note = tk.Label(sec3, text="✓ موقع Sub-Scene للترجمة مدمج ومفعل افتراضياً بدون الحاجة لأي مفتاح أو اشتراك.", font=("Segoe UI", 9, "bold"), fg="#22c55e", bg="#0f172a", justify="center")
    sub_note.pack(anchor="center", pady=6)

    # ==========================================
    # SECTION 4: PRIVATE TRACKERS
    # ==========================================
    sec4 = create_card("4. التراكرات الخاصة (Private Trackers)")

    pt_warn_frame = tk.Frame(sec4, bg="#200d11", padx=14, pady=10, highlightbackground="#991b1b", highlightthickness=1)
    pt_warn_frame.pack(anchor="center", fill="x", pady=4)

    pt_warn_lbl = tk.Label(
        pt_warn_frame,
        text="🛡️ تنبيه أمان هام لمستخدمي التراكرات الخاصة\n\n"
             "• أغلب التراكرات الخاصة تمنع خدمات السحب السحابي وتعتبرها مخالفة.\n"
             "• يُنصح دائماً بمراجعة قوانين التراكر الخاص بك قبل البدء.\n"
             "• عند استخدام تراكر خاص سيتم التنزيل عبر برنامج qBittorrent للحفاظ على نسبة المشاركة (Ratio).",
        font=("Segoe UI", 9),
        fg="#fca5a5",
        bg="#200d11",
        justify="center"
    )
    pt_warn_lbl.pack(anchor="center", fill="x")

    pt_input_frame = tk.Frame(sec4, bg="#0f172a", pady=6)
    pt_input_frame.pack(anchor="center")

    # Name row
    r1 = tk.Frame(pt_input_frame, bg="#0f172a", pady=2)
    r1.pack(anchor="center")
    tk.Label(r1, text="اسم التراكر", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=18, anchor="e").pack(side="left")
    trk_name_entry = tk.Entry(r1, width=36, font=("Segoe UI", 9), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat")
    trk_name_entry.pack(side="left", padx=6, ipady=2)

    # RSS row
    r2 = tk.Frame(pt_input_frame, bg="#0f172a", pady=2)
    r2.pack(anchor="center")
    tk.Label(r2, text="رابط خلاصات التراكر", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=18, anchor="e").pack(side="left")
    trk_rss_entry = tk.Entry(r2, width=36, font=("Consolas", 9), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat")
    trk_rss_entry.pack(side="left", padx=6, ipady=2)

    # Passkey row
    r3 = tk.Frame(pt_input_frame, bg="#0f172a", pady=2)
    r3.pack(anchor="center")
    tk.Label(r3, text="مفتاح العضوية (اختياري)", font=("Segoe UI", 9, "bold"), fg="#cbd5e1", bg="#0f172a", width=18, anchor="e").pack(side="left")
    trk_pass_entry = tk.Entry(r3, width=36, font=("Consolas", 9), bg="#1e293b", fg="#f8fafc", insertbackground="#38bdf8", highlightbackground="#334155", highlightthickness=1, relief="flat")
    trk_pass_entry.pack(side="left", padx=6, ipady=2)

    existing_trackers = series_cfg.get("private_trackers", [])
    if existing_trackers:
        tk.Label(sec4, text=f"التراكرات المسجلة حالياً: {len(existing_trackers)} تراكر", font=("Segoe UI", 9, "bold"), fg="#22c55e", bg="#0f172a", justify="center").pack(anchor="center", pady=4)

    # ==========================================
    # SAVE BUTTON
    # ==========================================
    def save_all():
        chosen_prov = provider_var.get()
        p_meta = DEBRID_PROVIDERS.get(chosen_prov, DEBRID_PROVIDERS["None"])
        p_id = p_meta["id"]

        # Update debrid config
        debrid_cfg["default_service"] = p_id
        debrid_cfg["default_player"] = player_var.get()
        debrid_cfg["auto_install_missing_players"] = auto_install_var.get()
        debrid_cfg["non_debrid_torrent_client"] = "qbittorrent"

        if p_id != "none":
            if "services" not in debrid_cfg:
                debrid_cfg["services"] = {}
            if p_id not in debrid_cfg["services"]:
                debrid_cfg["services"][p_id] = {
                    "name": p_meta["name"],
                    "api_key": "",
                    "api_url": p_meta["api_url"]
                }
            debrid_cfg["services"][p_id]["api_key"] = key_entry.get().strip()
            debrid_cfg["services"][p_id]["name"] = p_meta["name"]
            if p_meta["api_url"]:
                debrid_cfg["services"][p_id]["api_url"] = p_meta["api_url"]

        save_json(DEBRID_CONFIG_PATH, debrid_cfg)

        # Update subtitles config
        sub_cfg["services"]["subdl"]["api_key"] = subdl_entry.get().strip()
        sub_cfg["services"]["subsource"]["api_key"] = subsrc_entry.get().strip()
        save_json(SUBTITLE_CONFIG_PATH, sub_cfg)

        # Update private trackers if filled
        trk_name = trk_name_entry.get().strip()
        trk_rss = trk_rss_entry.get().strip()
        trk_pass = trk_pass_entry.get().strip()
        if trk_name and trk_rss:
            new_trk = {
                "name": trk_name,
                "rss_url": trk_rss,
                "passkey": trk_pass
            }
            series_cfg["private_trackers"] = [t for t in series_cfg.get("private_trackers", []) if t.get("name") != trk_name]
            series_cfg["private_trackers"].append(new_trk)
            save_json(TRACKED_CONFIG_PATH, series_cfg)

        messagebox.showinfo("تم الحفظ بنجاح", "✅ تم حفظ وتطبيق جميع الإعدادات بنجاح!\nالمهارة جاهزة للاستخدام الآن.")
        root.destroy()

    save_btn = tk.Button(
        save_frame,
        text="💾 حفظ وتطبيق جميع الإعدادات",
        command=save_all,
        font=("Segoe UI", 11, "bold"),
        bg="#15803d",
        activebackground="#16a34a",
        fg="white",
        activeforeground="white",
        relief="flat",
        padx=35,
        pady=8,
        cursor="hand2"
    )
    save_btn.pack(anchor="center")

    root.mainloop()

def run_cli_wizard():
    """Clean CLI wizard avoiding Windows CMD Arabic shaping bugs."""
    print("=" * 65)
    print("        torrentx Skill Setup Wizard - Configuration Menu        ")
    print("=" * 65)
    print("Tip: Run with --gui for graphical window: python setup_wizard.py --gui\n")
    
    debrid_cfg = load_json(DEBRID_CONFIG_PATH, {"default_service": "realdebrid", "services": {}, "default_player": "mpc", "non_debrid_torrent_client": "qbittorrent"})
    sub_cfg = load_json(SUBTITLE_CONFIG_PATH, {"services": {"subdl": {"api_key": ""}, "subsource": {"api_key": ""}}})
    series_cfg = load_json(TRACKED_CONFIG_PATH, {"tracked_shows": [], "private_trackers": []})
    
    print("[1] Debrid Provider Selection:")
    prov_list = list(DEBRID_PROVIDERS.keys())
    for idx, p in enumerate(prov_list, 1):
        print(f"  {idx}. {p}")
        
    choice = input("Select provider number [default: 2 (RealDebrid)]: ").strip() or "2"
    try:
        chosen_prov = prov_list[int(choice) - 1]
    except Exception:
        chosen_prov = "RealDebrid"
        
    p_meta = DEBRID_PROVIDERS[chosen_prov]
    p_id = p_meta["id"]
    debrid_cfg["default_service"] = p_id
    
    if p_id == "none":
        print("\n  >> NOTICE FOR NON-DEBRID USERS:")
        print("     - Without Debrid, downloads will open via qBittorrent.")
        print("     - ALWAYS DOWNLOAD FIRST rather than streaming directly unless")
        print("       the torrent has a high number of active seeders.")
        print("     - The skill will automatically select releases with the MOST SEEDERS.\n")
    else:
        print(f"\n  >> {p_meta['name']} Token URL: {p_meta['api_url']}")
        curr_k = debrid_cfg.get("services", {}).get(p_id, {}).get("api_key", "")
        prompt = f"Enter {p_meta['name']} API Key" + (f" [Press Enter to keep current]" if curr_k else "") + ": "
        k = input(prompt).strip()
        if k or not curr_k:
            if "services" not in debrid_cfg:
                debrid_cfg["services"] = {}
            if p_id not in debrid_cfg["services"]:
                debrid_cfg["services"][p_id] = {}
            debrid_cfg["services"][p_id]["api_key"] = k or curr_k
            debrid_cfg["services"][p_id]["name"] = p_meta["name"]
            if p_meta["api_url"]:
                debrid_cfg["services"][p_id]["api_url"] = p_meta["api_url"]
                
    print("\n[2] Preferred Media Player:")
    print("1. MPC-HC (Default & Recommended)")
    print("2. PotPlayer")
    print("3. MPV")
    print("4. VLC")
    plr = input("Choose player number [default: 1]: ").strip() or "1"
    plr_map = {"1": "mpc", "2": "potplayer", "3": "mpv", "4": "vlc"}
    debrid_cfg["default_player"] = plr_map.get(plr, "mpc")
    save_json(DEBRID_CONFIG_PATH, debrid_cfg)
    
    print("\n[3] Subtitle APIs Setup (SubDL & Subsource):")
    print("  >> SubDL Token URL:     https://subdl.com/panel/api")
    print("  >> Subsource Token URL: https://subsource.net/dashboard/api")
    print("  >> Sub-Scene:           Works automatically (No key needed)")
    
    want_subs = input("Update Subtitle API keys? (y/n) [default: n]: ").strip().lower()
    if want_subs in ["y", "yes"]:
        k1 = input("SubDL API Key: ").strip()
        if k1: sub_cfg["services"]["subdl"]["api_key"] = k1
        k2 = input("Subsource API Key: ").strip()
        if k2: sub_cfg["services"]["subsource"]["api_key"] = k2
        save_json(SUBTITLE_CONFIG_PATH, sub_cfg)
        
    print("\n[4] Private Trackers Setup (Optional):")
    print("  >> WARNING: Most private trackers strictly forbid Debrid services and may ban your account.")
    print("     Always check your tracker rules and ask admins before connecting.")
    want_pt = input("Add a private tracker now? (y/n) [default: n]: ").strip().lower()
    if want_pt in ["y", "yes"]:
        tname = input("Tracker Name: ").strip()
        trss = input("RSS Feed URL: ").strip()
        tpass = input("Passkey (Optional): ").strip()
        if tname and trss:
            series_cfg["private_trackers"].append({"name": tname, "rss_url": trss, "passkey": tpass})
            save_json(TRACKED_CONFIG_PATH, series_cfg)
            print(f"Tracker '{tname}' saved.")
        
    print("\n" + "=" * 65)
    print("SUCCESS: Setup complete! MPC-HC set as default player.")
    print("=" * 65)

if __name__ == "__main__":
    if "--cli" in sys.argv or "--console" in sys.argv:
        run_cli_wizard()
    else:
        try:
            run_gui_wizard()
        except Exception:
            run_cli_wizard()
