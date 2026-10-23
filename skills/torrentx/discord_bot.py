# -*- coding: utf-8 -*-
"""
Discord Bot for TorrentX & AI Assistant
Allows remote control of downloads, playback, and subtitle sync from Discord.
"""

import os
import sys
import json
import logging
import asyncio

if sys.stdout is None:
    sys.stdout = open(os.devnull, "w", encoding="utf-8")
if sys.stderr is None:
    sys.stderr = open(os.devnull, "w", encoding="utf-8")
import discord
from discord.ext import commands

CURRENT_DIR = os.path.dirname(os.path.abspath(__file__))
if CURRENT_DIR not in sys.path:
    sys.path.insert(0, CURRENT_DIR)

from ai_assistant import execute_user_action, query_ollama

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("TorrentX-Discord")

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
            if "discord" not in cfg:
                cfg["discord"] = default_cfg["discord"]
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

intents = discord.Intents.default()
intents.message_content = True
bot = commands.Bot(command_prefix="!", intents=intents)

@bot.event
async def on_ready():
    logger.info(f"🚀 Discord Bot connected as {bot.user.name} (ID: {bot.user.id})")
    print(f"🚀 Discord Bot is active as {bot.user.name}!")

@bot.command(name="status")
async def cmd_status(ctx):
    cfg = load_config()
    allowed = cfg["discord"].get("allowed_user_ids", [])
    if allowed and ctx.author.id not in allowed:
        await ctx.reply("⛔ عذراً، هذا البوت خاص بمالك الجهاز فقط.")
        return

    d_dir = r"C:\Users\9jhp9\Downloads"
    files_count = 0
    recent_file = "لا يوجد"
    if os.path.exists(d_dir):
        m_files = [f for f in os.listdir(d_dir) if f.lower().endswith((".mkv", ".mp4", ".avi"))]
        files_count = len(m_files)
        if m_files:
            latest = max([os.path.join(d_dir, f) for f in m_files], key=os.path.getmtime)
            recent_file = os.path.basename(latest)

    embed = discord.Embed(title="📊 حالة خادم TorrentX وجهاز الكمبيوتر", color=0x38bdf8)
    embed.add_field(name="حالة النظام", value="متصل ويعمل بكفاءة ✅", inline=False)
    embed.add_field(name="ملفات الفيديو في Downloads", value=f"{files_count} ملف", inline=True)
    embed.add_field(name="آخر ملف تم تنزيله", value=f"`{recent_file}`", inline=False)
    await ctx.reply(embed=embed)

@bot.command(name="help_cmd", aliases=["مساعدة", "تعليمات"])
async def cmd_help(ctx):
    embed = discord.Embed(title="📖 دليل استخدام بوت TorrentX", color=0x22c55e)
    embed.description = (
        "يمكنك إرسال رسائل نصية عادية مباشرة في الخاص أو القناة لتنفيذ الأوامر:\n\n"
        "• **التحميل:** `حمل مسلسل 11.22.63 4k مع ترجمه`\n"
        "• **التشغيل:** `شغل الحلقة 1`\n"
        "• **المزامنة:** `ضبط توقيت الترجمة`\n"
        "• **الحالة:** `!status`"
    )
    await ctx.reply(embed=embed)

@bot.event
async def on_message(message):
    # Ignore bot's own messages
    if message.author.bot:
        return

    # Handle prefixed commands first
    if message.content.startswith("!"):
        await bot.process_commands(message)
        return

    text = message.content.strip()
    if not text:
        return

    cfg = load_config()
    allowed = cfg["discord"].get("allowed_user_ids", [])

    # Auto-claim owner if allowed_user_ids is empty
    if not allowed:
        allowed.append(message.author.id)
        cfg["discord"]["allowed_user_ids"] = allowed
        save_config(cfg)
        welcome_owner = (
            f"🎉 أهلاً بك يا {message.author.name}!\n\n"
            f"تم تسجيل حسابك كمالك ومتحكم وحيد بالبوت بنجاح.\n"
            f"معرّفك (User ID): `{message.author.id}`\n"
            "الآن يمكنك إرسال أي طلب وسينفذه جهازك فوراً!"
        )
        await message.reply(welcome_owner)

    # Security check
    if message.author.id not in allowed:
        await message.reply("⛔ عذراً، هذا البوت شخصي ومخصص للتحكم بجهاز الكمبيوتر الخاص بمالكه فقط.")
        return

    # Processing indicator with immediate message
    wait_msg = await message.reply("⏳ جاري فحص ومعالجة طلبك والبحث في القواعد...")
    async with message.channel.typing():
        try:
            # 1. Direct Intent Router (Async thread)
            is_action, result = await asyncio.to_thread(execute_user_action, text, "auto", False)
            if is_action and result:
                clean_res = result.replace("\u202A", "").replace("\u202C", "")
                if len(clean_res) <= 1900:
                    await wait_msg.edit(content=clean_res)
                else:
                    await wait_msg.edit(content=clean_res[:1900])
                    for i in range(1900, len(clean_res), 1900):
                        await message.reply(clean_res[i:i+1900])
                return

            # 2. Local AI Assistant (Ollama fallback)
            ai_reply = await asyncio.to_thread(query_ollama, text)
            if ai_reply and not ("خطأ أثناء الاتصال" in ai_reply or "timed out" in ai_reply):
                if len(ai_reply) <= 1900:
                    await wait_msg.edit(content=ai_reply)
                else:
                    await wait_msg.edit(content=ai_reply[:1900])
                    for i in range(1900, len(ai_reply), 1900):
                        await message.reply(ai_reply[i:i+1900])
            else:
                fallback_msg = (
                    "💡 **مرحباً بك!**\n\n"
                    "أنا جاهز لتنفيذ طلبات التحميل والتشغيل فوراً. أرسل لي مثلاً:\n"
                    "• `الحلقة الاولى من الموسم الثاني من انمي Black Clover بدون ضغط`\n"
                    "• `حمل مسلسل 11.22.63 4k`\n"
                    "• `شغل آخر حلقة تم تحميلها`\n"
                    "• `مزامنة الترجمة`"
                )
                await wait_msg.edit(content=fallback_msg)
        except Exception as ex:
            logger.error(f"Error handling message: {ex}")
            await wait_msg.edit(content=f"⚠️ حدث خطأ أثناء المعالجة: {ex}")

def main():
    cfg = load_config()
    token = cfg.get("discord", {}).get("token", "").strip()

    if not token or token == "YOUR_DISCORD_BOT_TOKEN_HERE":
        print("\n" + "="*60)
        print("🤖 [TorrentX Discord Bot Setup]")
        print("="*60)
        print("لم يتم العثور على توكن بوت الديسكورد في bot_config.json.")
        print("للحصول على توكن ديسكورد:")
        print("1. ادخل إلى: https://discord.com/developers/applications")
        print("2. أنشئ تطبيقا جديداً (New Application) ثم اذهب لتبويب Bot.")
        print("3. فعّل (Message Content Intent) وانسخ الـ Token.")
        print("="*60)
        token_input = input("أدخل توكن بوت ديسكورد هنا (أو اضغط Enter للإلغاء): ").strip()
        if token_input:
            cfg["discord"]["token"] = token_input
            save_config(cfg)
            token = token_input
        else:
            print("❌ تم الإلغاء. يرجى وضع التوكن في bot_config.json ثم إعادة التشغيل.")
            return

    bot.run(token)

if __name__ == "__main__":
    main()
