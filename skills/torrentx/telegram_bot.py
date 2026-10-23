# -*- coding: utf-8 -*-
"""
Telegram Bot for TorrentX & AI Assistant
Allows remote control of downloads, playback, and subtitle sync from Telegram.
"""

import os
import sys
import time
import json
import logging
import threading
import requests

if sys.stdout is None:
    sys.stdout = open(os.devnull, "w", encoding="utf-8")
if sys.stderr is None:
    sys.stderr = open(os.devnull, "w", encoding="utf-8")

CURRENT_DIR = os.path.dirname(os.path.abspath(__file__))
if CURRENT_DIR not in sys.path:
    sys.path.insert(0, CURRENT_DIR)

from ai_assistant import execute_user_action, query_ollama

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("TorrentX-Telegram")

CONFIG_PATH = os.path.join(CURRENT_DIR, "bot_config.json")

def load_config():
    default_cfg = {
        "telegram": {
            "token": "",
            "allowed_chat_ids": []
        },
        "discord": {
            "token": "",
            "allowed_user_ids": []
        }
    }
    if not os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump(default_cfg, f, indent=4, ensure_ascii=False)
        except Exception as e:
            logger.error(f"Error creating config: {e}")
        return default_cfg

    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            cfg = json.load(f)
            if "telegram" not in cfg:
                cfg["telegram"] = default_cfg["telegram"]
            return cfg
    except Exception as e:
        logger.error(f"Error reading config: {e}")
        return default_cfg

def save_config(cfg):
    try:
        with open(CONFIG_PATH, "w", encoding="utf-8") as f:
            json.dump(cfg, f, indent=4, ensure_ascii=False)
    except Exception as e:
        logger.error(f"Error saving config: {e}")

class TelegramBot:
    def __init__(self, token):
        self.token = token
        self.base_url = f"https://api.telegram.org/bot{self.token}"
        self.offset = 0
        self.running = True

    def send_request(self, method, payload=None, timeout=35):
        url = f"{self.base_url}/{method}"
        try:
            r = requests.post(url, json=payload, timeout=timeout)
            return r.json()
        except requests.exceptions.Timeout:
            return None
        except Exception as e:
            logger.error(f"Telegram API error ({method}): {e}")
            return None

    def send_message(self, chat_id, text, reply_to_message_id=None):
        # Clean directional marks if any
        clean_text = text.replace("\u202A", "").replace("\u202C", "")
        payload = {
            "chat_id": chat_id,
            "text": clean_text,
            "disable_web_page_preview": True
        }
        if reply_to_message_id:
            payload["reply_to_message_id"] = reply_to_message_id

        # Try sending with Markdown, fallback to plain text if syntax issues occur
        payload["parse_mode"] = "Markdown"
        res = self.send_request("sendMessage", payload, timeout=15)
        if not res or not res.get("ok"):
            payload.pop("parse_mode", None)
            res = self.send_request("sendMessage", payload, timeout=15)
        return res

    def send_chat_action(self, chat_id, action="typing"):
        payload = {"chat_id": chat_id, "action": action}
        self.send_request("sendChatAction", payload, timeout=10)

    def handle_message(self, msg):
        chat = msg.get("chat", {})
        chat_id = chat.get("id")
        user = msg.get("from", {})
        user_name = user.get("first_name", "صديقي")
        text = (msg.get("text") or "").strip()
        msg_id = msg.get("message_id")

        if not text or not chat_id:
            return

        cfg = load_config()
        allowed = cfg["telegram"].get("allowed_chat_ids", [])

        # Auto-claim: if list is empty, claim first user as authorized owner
        if not allowed:
            allowed.append(chat_id)
            cfg["telegram"]["allowed_chat_ids"] = allowed
            save_config(cfg)
            welcome_owner = (
                f"🎉 أهلاً بك يا {user_name}!\n\n"
                f"تم اعتماد حسابك كمالك ومتحكم رئيسي بالبوت بنجاح.\n"
                f"معرّفك (ID): `{chat_id}`\n\n"
                "الآن يمكنك إرسال أي طلب وسينفذه جهازك فوراً!"
            )
            self.send_message(chat_id, welcome_owner, reply_to_message_id=msg_id)

        # Check authorization
        if chat_id not in allowed:
            self.send_message(
                chat_id,
                "⛔ عذراً، هذا البوت شخصي ومخصص للتحكم بجهاز الكمبيوتر الخاص بمالكه فقط.",
                reply_to_message_id=msg_id
            )
            return

        # Commands
        if text.startswith("/start"):
            welcome = (
                f"🤖 **مرحباً بك في بوت TorrentX الذكي!**\n\n"
                "يمكنك التحكم بجهازك والتحميل والمشاهدة مباشرة عبر المحادثة:\n\n"
                "📥 **أمثلة لأوامر التحميل:**\n"
                "• `حمل مسلسل 11.22.63 4k مع ترجمه`\n"
                "• `حمل الحلقة 2 من مسلسل The Day of the Jackal`\n"
                "• `حمل فلم Gladiator 2 remux`\n\n"
                "🎬 **أوامر المشغل والمزامنة:**\n"
                "• `شغل آخر حلقة تم تحميلها`\n"
                "• `ضبط توقيت الترجمة` (مزامنة تلقائية بـ alass)\n\n"
                "📊 **أوامر النظام:**\n"
                "• `/status` - حالة الاتصال ومجلد التنزيلات\n"
                "• `/help` - قائمة المساعدة\n\n"
                "💡 يمكنك أيضاً سؤالي عن أي فيلم أو طلب ترشيحات!"
            )
            self.send_message(chat_id, welcome, reply_to_message_id=msg_id)
            return

        if text.startswith("/help"):
            help_text = (
                "📖 **دليل استخدام البوت:**\n\n"
                "1️⃣ **التحميل الذكي السريع:**\n"
                "اكتب اسم الفيلم أو المسلسل مباشرة، وسيتم البحث في تورنتيو وسحبه بسرعة قصوى عبر Real-Debrid وحفظه مع الترجمة المتطابقة في Downloads.\n\n"
                "2️⃣ **المزامنة الصوتية (alass):**\n"
                "إذا كان توقيت الترجمة غير دقيق، اكتب `مزامنة الترجمة` وسيقوم البوت بضبطها بمطابقة البصمة الصوتية فوراً.\n\n"
                "3️⃣ **التشغيل المباشر:**\n"
                "اكتب `شغل الحلقة 1` وسيفتح مشغل MPC-HC في جهازك مع الترجمة المختارة."
            )
            self.send_message(chat_id, help_text, reply_to_message_id=msg_id)
            return

        if text.startswith("/status"):
            d_dir = r"C:\Users\9jhp9\Downloads"
            files_count = 0
            recent_file = "لا يوجد"
            if os.path.exists(d_dir):
                m_files = [f for f in os.listdir(d_dir) if f.lower().endswith((".mkv", ".mp4", ".avi"))]
                files_count = len(m_files)
                if m_files:
                    latest = max([os.path.join(d_dir, f) for f in m_files], key=os.path.getmtime)
                    recent_file = os.path.basename(latest)

            status_text = (
                "📊 **حالة الخادم وجهاز الكمبيوتر:**\n\n"
                f"• **النظام:** متصل ويعمل بنجاح ✅\n"
                f"• **مجلد Downloads:** به {files_count} ملفات فيديو\n"
                f"• **آخر ملف تم تحميله:**\n`{recent_file}`\n\n"
                "جاهز لتلقي أوامرك!"
            )
            self.send_message(chat_id, status_text, reply_to_message_id=msg_id)
            return

        # Regular intent handling
        self.send_chat_action(chat_id, "typing")
        self.send_message(chat_id, "⏳ جاري فحص ومعالجة طلبك...", reply_to_message_id=msg_id)

        def process_request():
            try:
                # 1. Direct high-speed execution (Downloads / Playback / Sync)
                is_action, result = execute_user_action(text, debrid_mode="auto", allow_upscale=False)
                if is_action and result:
                    self.send_message(chat_id, result)
                    return

                # 2. Local AI Assistant (Recommendations / Chit-chat / Inquiries)
                self.send_chat_action(chat_id, "typing")
                ai_reply = query_ollama(text)
                if ai_reply and not ai_reply.startswith("⚠️ خطأ في الاتصال"):
                    self.send_message(chat_id, ai_reply)
                else:
                    fallback_msg = (
                        "💡 **مرحباً بك!**\n\n"
                        "أنا في وضع الاستعداد لتنفيذ أوامرك المباشرة. يمكنك كتابة:\n"
                        "• `حمل مسلسل [اسم المسلسل] [الجودة]`\n"
                        "• `شغل [اسم الملف]`\n"
                        "• `مزامنة الترجمة`\n\n"
                        "وسيقوم الكمبيوتر بالتنفيذ فوراً."
                    )
                    self.send_message(chat_id, fallback_msg)
            except Exception as ex:
                logger.error(f"Error processing text: {ex}")
                self.send_message(chat_id, f"⚠️ حدث خطأ أثناء المعالجة: {ex}")

        threading.Thread(target=process_request, daemon=True).start()

    def poll_updates(self):
        payload = {"offset": self.offset, "timeout": 25}
        res = self.send_request("getUpdates", payload, timeout=30)
        if not res or not res.get("ok"):
            return

        for update in res.get("result", []):
            self.offset = update["update_id"] + 1
            if "message" in update:
                self.handle_message(update["message"])

    def start(self):
        logger.info("🚀 Telegram Bot is running in polling mode...")
        print(f"🚀 Telegram Bot started! Waiting for messages...")
        while self.running:
            try:
                self.poll_updates()
            except Exception as e:
                logger.error(f"Polling loop exception: {e}")
                time.sleep(3)

def main():
    cfg = load_config()
    token = cfg.get("telegram", {}).get("token", "").strip()

    if not token or token == "YOUR_TELEGRAM_BOT_TOKEN_HERE":
        print("\n" + "="*60)
        print("🤖 [TorrentX Telegram Bot Setup]")
        print("="*60)
        print("لم يتم العثور على توكن البوت في bot_config.json.")
        print("للحصول على توكن مجاني وسريع:")
        print("1. افتح تطبيق تيليجرام وابحث عن @BotFather")
        print("2. أرسل الأمر /newbot واتبع التعليمات لاختيار اسم للبوت.")
        print("3. سيعطيك BotFather رمز التوكن (مثل: 7123456789:AAHq...)")
        print("="*60)
        token_input = input("أدخل توكن البوت هنا (أو اضغط Enter للإلغاء): ").strip()
        if token_input:
            cfg["telegram"]["token"] = token_input
            save_config(cfg)
            token = token_input
        else:
            print("❌ تم الإلغاء. يرجى وضع التوكن في bot_config.json ثم إعادة التشغيل.")
            return

    bot = TelegramBot(token)
    bot.start()

if __name__ == "__main__":
    main()
