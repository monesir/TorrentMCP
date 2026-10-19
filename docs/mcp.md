# خادم MCP

[English](mcp.en.md)

`torrentx mcp` يعرض عمليات torrentx أدوات MCP عبر stdin/stdout. الإيجنت يستدعي الأداة مباشرة، فلا يكتب ملفات طلبات ولا يشغل أوامر طرفية. يعيش الخادم ما دام العميل يعمل، ويبقي المصادر محملة وجلسة rqbit جاهزة، فإعادة فحص ملف تكمل التحميل الجزئي بدل البدء من الصفر.

## التثبيت

```powershell
torrentx setup
```

يكتشف الوكلاء المثبتين ويسألك عن كل واحد: هل تريد إعداد torrentx فيه، ثم هل تريد تثبيت مهارة torrentx أيضا. يعرض كل تغيير، ولا يطبق شيئا قبل موافقتك. يحفظ نسخة احتياطية من كل ملف قبل تعديله (`<الملف>.torrentx-backup-<الوقت>`). أعد تشغيل الوكيل بعدها.

| الوكيل | المعرف | ما يفعله setup |
| --- | --- | --- |
| Claude Code | `claude_code` | `claude mcp add --scope user torrentx -- <torrentx.exe> mcp` |
| Codex | `codex` | `codex mcp add torrentx -- <torrentx.exe> mcp` |
| Antigravity | `antigravity` | يضيف `torrentx` إلى `mcpServers` في `~/.gemini/config/mcp_config.json` |
| Claude Desktop | `claude_desktop` | `%APPDATA%\Claude\claude_desktop_config.json` |
| Gemini CLI | `gemini_cli` | `~/.gemini/settings.json` |
| Cursor | `cursor` | `~/.cursor/mcp.json` |

يستخدم setup أداة Claude Code وCodex الخاصة بهما، لأن كلا منهما يكتب ملف إعداده أثناء عمله. إن لم تكن أداة Claude Code في PATH يعدل `~/.claude.json` مباشرة.

```powershell
torrentx setup --client antigravity,codex   # وكلاء محددون فقط
torrentx setup --yes                        # دون سؤال: كل الوكلاء المكتشفين مع المهارة
torrentx setup --no-skill                   # تسجيل الخادم فقط
torrentx setup --remove                     # إزالة التسجيل والمهارة
```

المهارة تعلّم الوكيل استخدام torrentx: أي أداة يستدعي، وماذا يفعل عند كل فشل، وما الذي يبحث عنه أولا في الأنمي والأفلام والمسلسلات والموسيقى. ينسخ setup المجلد `skills	orrentx` من الحزمة إلى `~/.claude/skills` (Claude Code)، أو `~/.gemini/config/skills` (Antigravity)، أو المجلد المشترك `~/.agents/skills` (Codex والبقية). ولا يلمس مجلد مهارة باسم `torrentx` لم يكتبه هو.

المسار المسجل هو مسار `torrentx.exe` الذي شغلت منه setup. إذا نقلت الحزمة أعد تشغيل setup.

### التسجيل اليدوي

لأي عميل MCP آخر: الأمر هو المسار الكامل لـ`torrentx.exe`، والمعامل `mcp`.

```json
{
  "mcpServers": {
    "torrentx": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\torrentx\\torrentx.exe",
      "args": ["mcp"]
    }
  }
}
```

في Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.torrentx]
command = 'C:\Users\<you>\AppData\Local\torrentx\torrentx.exe'
args = ["mcp"]
```

## الأدوات

| الأداة | العمل |
| --- | --- |
| `find` | بحث، ثم فحص ملف معتاد من كل مرشح متقدم، ثم مقارنة البث الفعلي. ابدأ بها. |
| `search` | بحث وتحليل الأسماء فقط؛ كل ما فيه ادعاءات أسماء. |
| `torrent_files` | قائمة ملفات التورنت وأرقامها. |
| `inspect_torrent` | فحص ملفات محددة (`file_ids`) أو عينة (`sample_files`). |
| `inspect_media` | فحص ملف وسائط محلي. |

المدخلات هي حقول [عقد الإيجنت](agent-contract.md) نفسها دون `operation` و`schema_version`؛ الخادم يضيفهما. `search.category` افتراضيا `all`، و`search.sources` افتراضيا كل المصادر، في الخادم وفي سطر الأوامر.

### شكل الرد

افتراضيا يعيد الخادم عرضا مختصرا: أول 20 نتيجة بحث (`max_results`)، لكل نتيجة العنوان والمرجع والحجم والسيدرز والأهلية وأسباب الاستبعاد وما ادعاه الاسم تحت `claimed`. قائمة الملفات تعرض ملفات الفيديو والصوت فقط (`max_files`، افتراضيا 200). تحذف القيم الفارغة ووسوم الحاوية. بحث 20 نتيجة نحو 20 KB بدل 270 KB للرد الكامل. `view: "full"` يعيد استجابة الإيجنت الكاملة.

في `find` المختصر يحمل كل مرشح دليله: `verified` فيه الحاوية والبث كما قرأه ffprobe من ملفه، و`unmet` الشروط التي لم يحققها ذلك الملف. المرشح الذي لم يقرأ ملفه ليس له `verified` ولا `position`، بل `inspection_error` فقط.

`isError` يكون true عند `error` أو `failed` أو `cancelled`. الحالة `partial` ليست خطأ: النتائج حقيقية وبعضها ناقص.

### ما يقرؤه الإيجنت عند الاتصال

يرسل الخادم تعليمات تفرق بين نوعين من الحقائق:
- **ادعاء:** كل ما في نتائج البحث مأخوذ من اسم الإصدار، وقد يكون خطأ.
- **مؤكد:** البث في فحص مكتمل مأخوذ من الملف نفسه.

الفحص الفاشل (`probe_timeout` مثلا) يعني أنه لا يعرف شيء عن بث ذلك الملف. يعيد الإيجنت المحاولة بمهلة أكبر (حتى 120 ثانية)، والخادم يحتفظ بما حمل. ومرشح `find` الذي لم يفحص لا يأخذ مركزا في الترتيب.
