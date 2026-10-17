# عقود إضافات LightDestory العامة

**تاريخ لقطة الأدلة:** 2026-10-17.  
**الغرض:** مرجع تنفيذ لترحيل ملفات Python العامة المحددة في كتالوج المجتمع إلى موصلات Rust، مع إبقاء دعم الإضافات Python. هذا تحليل ساكن لنسخ المصدر المحفوظة؛ لم تُشغّل الشيفرات.

## النطاق والمصدر والترخيص

يتضمن النطاق **10 سجلات عامة** في `community-20261017/catalog.json` التي يشير `download_url` فيها إلى `LightDestory/qBittorrent-Search-Plugins`: الأكواد 001 و026 و028 و045 و047 و051 و058 و064 و065 و075. لكل سجل الملف المحفوظ المقابل في `community-20261017/sources/`. استُبعدت سجلات المجتمع الخاصة. هذه هي حدود لقطة الكتالوج في التاريخ أعلاه، وليست ادعاءً بأن مستودع المجتمع لن يتغير لاحقًا.

المصدر الأولي هو [مستودع LightDestory](https://github.com/LightDestory/qBittorrent-Search-Plugins). يذكر [README للمستودع](https://github.com/LightDestory/qBittorrent-Search-Plugins/blob/master/README.md) أن المحتوى، باستثناء سكربتات `nova` المنقولة من مطوري qBittorrent، مرخص تحت GPL-3.0، و[ملف LICENSE](https://github.com/LightDestory/qBittorrent-Search-Plugins/blob/master/LICENSE) هو نص GPLv3. عند توزيع اقتباس أو اشتقاق من الشيفرة ينبغي الحفاظ على إشعارات النسبة والترخيص والوفاء بشروطه؛ هذا توثيق تقني وليس رأيًا قانونيًا. ملف 064 يذكر مؤلفًا مختلفًا (Deivid Soto/TorrentClaw)، لكنه وارد في هذا المستودع المشمول ببيان README العام؛ سجّل النسبة كما تظهر في الملف.

**انتبه لاختلاف سجل الكتالوج عن كود الإضافة:** بيانات `listed_version` و`site_url` في الكتالوج تمثل صفحة المجتمع، بينما ترويسة الملف وعنوانه الثابت يحددان النسخة والسلوك اللذين جرى تحليلهما. أبرز اختلاف المضيف هو Pirateiro: الكتالوج يقول `pirateiro.com` بينما الملف يستخدم `pirateiro.io`. لذلك لا تُولّد إعدادات Rust من عمود الكتالوج وحده.

## خريطة المصادر

| الكود / الاسم | النسخة في ملف Python (نسخة الكتالوج) | المضيف في الملف | الفئة | المسار، التوقف والإيقاع |
|---|---|---|---|---|
| 001 AcademicTorrents | 1.4 (1.2) | `academictorrents.com` | XML محلي مع إثراء صفحات | تنزيل قاعدة XML مرة يوميًا؛ لا ترقيم صفحات للموقع، لكن طلب `/tech` لكل نتيجة مطابقة وبالتوازي. |
| 026 GloTorrents | 1.7 (1.5) | `glodls.to` | HTML ومغناطيس في الصف | `search_results.php`, يبدأ `page=0`؛ حتى صفحة فارغة؛ انتظار 3 ثوانٍ بعد كل صفحة غير فارغة. |
| 028 Kickass Torrent | 1.2 (1.0) | `katcr.to` | HTML ثم صفحة تفاصيل لكل نتيجة | `/search/.../{page}/`, يبدأ 0؛ حتى صفحة فارغة؛ انتظار ثانية بعد حل كل نتيجة. |
| 045 Pirateiro | 1.4 (1.0) | `pirateiro.io` | HTML وروابط تفاصيل | `/search?query=...&page=...`; الصفحات 1–9 كحد أقصى؛ يمكن تمرير فئة. |
| 047 RockBox | 1.2 (1.0) | `rawkbawx.rocks` | HTML ورابط تنزيل | `torrents.php`, يبدأ `page=0`؛ حتى صفحة فارغة؛ انتظار 3 ثوانٍ بعد كل صفحة غير فارغة. |
| 051 Snowfl | 1.3 (1.3) | `snowfl.com` | token من HTML/JS ثم JSON | طلب token للصفحة والسكربت، ثم طلب JSON واحد؛ كل الفئات فقط. |
| 058 The Pirate Bay | 1.1 (1.0) | API `apibay.org` | JSON ومغناطيس مركّب | `q.php?q=...&cat=0`; طلب واحد؛ كل الفئات فقط. |
| 064 TorrentClaw | 1.0 (1.0) | `torrentclaw.com` | JSON عام ومغناطيس مباشر أو مركّب | `/api/v1/search`, `limit=50`; طلب واحد؛ الفئات `all/movie/show`. المفتاح اختياري. |
| 065 TorrentDownload | 1.2 (1.0) | `www.torrentdownload.info` | HTML، hash ومغناطيس مركّب | `/search?q=...&p=1..10`; انتظار ثانية، وإيقاف بالعدد الكلي أو صفحة بلا صفوف. |
| 075 YourBittorrent | 1.3 (1.0) | `yourbittorrent.com` | HTML ورابط تفاصيل/ملف torrent | الصفحة الرئيسية مع `?q=...&c=...`; صفحة واحدة، حد عملي 50 نتيجة بحسب تعليق الكاتب. |

الإصدارات بين القوسين مأخوذة من `listed_version` في الكتالوج؛ الإصدارات قبله من ترويسات ملفات المصدر. حقول الكتالوج وروابط الملفات وبصمات SHA-256 موجودة في `community-20261017/catalog.json`.

## تفاصيل عقود كل مصدر

### 001 — AcademicTorrents

- **الطلب والتخزين:** ينزّل `https://academictorrents.com/database.xml` (السطور 14، 55–68)، ويخزن XML مع سطر تاريخ محلي في `qbit_plugins_data/academic_cache.xml` تحت مسار النظام (15–21). إذا كان تاريخ آخر تحديث هو تاريخ اليوم لا يعيد التنزيل (55–61). يقرأ XML ابتداءً من السطر الثاني (46–53).
- **الفئات والمطابقة:** لا يدعم إلا `all` (24–32). يقسم الاستعلام إلى كلمات، ويطابق كل كلمة مفردة في عنوان العنصر أو وصفه مع تجاهل حالة الأحرف؛ يكفي تطابق كلمة واحدة (38–44، 99–106). عند التحويل، احذف كلمات الاستعلام الفارغة لتفادي مطابقة كل العناوين عند استعلام يحوي فراغًا ابتدائيًا/نهائيًا.
- **شكل النتيجة:** الحقول من `channel/item`: `title`, `size`, `infohash`, `link` (70–77). رابط العنصر الناتج `https://academictorrents.com/download/{infohash}.torrent`، ورابط الوصف هو `link` من XML.
- **الإثراء:** لكل عنصر مطابق يطلب `{link}/tech` عبر helper (78)، يستخرج من HTML `Mirrors: N complete, M downloading` (79–88) وتاريخ `Added` بتنسيق ISO (89–94)، ثم يخرج seed/leech وتاريخ Unix. إذا لم يطابق peers يعيد `-1` لهما، أما التاريخ غير الموجود قد يفشل التحليل في نسخة Python.
- **التنفيذ المتوازي:** يطلق طلب صفحة tech لكل نتيجة مطابقة في `ThreadPoolExecutor` ويخرج النتائج حسب انتهاء الطلب (99–110). حافظ على التوازي المحدود ومهلة كلية/حد أقصى، ولا تجعل استعلامًا شائعًا يسبب سيل طلبات غير محدود. جلب XML الأساسي يستخدم `urllib.request.urlopen` مباشرة بينما صفحات tech تستخدم `retrieve_url` (11، 62–63، 78).

### 026 — GloTorrents

- **الفئات:** `all=0`, `movies=1`, `tv=41`, `music=22`, `games=10`, `anime=28`, `software=18`, `books=51`, `pictures=70` (11–24).
- **المسار:** `https://glodls.to/search_results.php?search={query}&cat={id}&page={n}&incldead=0&inclexternal=0`, مع `n` يبدأ من صفر. تستبدل الإضافة `%20` بعلامة `+` (72–80). لا يوجد سقف صفحات؛ ينتهي البحث عندما لا تُستخرج صفوف (72–85). ينتظر 3 ثوانٍ بعد الصفحة غير الفارغة (82–85).
- **استخراج النتائج:** الصفوف تطابق regex حرفيًا `tr class='t-row'` ثم `td class='ttable_col1'` (49–54). من الصف: `title` لاسم العنصر، `href` لوصفه، `magnet:` للرابط، الحجم بوحدات TB/GB/MB/KB، وعددا seed/leech (55–69). يحذف الفواصل من الحجم والعدادات. `desc_link` هو دمج المضيف مع href؛ تحقّق من الروابط النسبية والمطلقة عند تحويل regex إلى DOM parser.

### 028 — Kickass Torrent

- **الفئات:** `all` بلا مقطع، ثم `movies`, `tv`, `music`, `games`, `anime`, `software -> apps`; الكتب والصور غير مدعومتين (11–22).
- **المسار والتوقف:** `https://katcr.to/search/{query}/{category-segment}/{page}/`, حيث segment يكون فارغًا لـ `all` وإلا `category/{category}/`; الصفحة تبدأ صفرًا (69–80). يستمر حتى صفحة لا تنتج صفوفًا (81–85). الملف لا يفرض سقف صفحات أو تأخير بين الصفحات.
- **استخراج النتائج:** صفوف `<tr class="odd">` أو `<tr class="even">`؛ داخلها `div.torrentname`, ورابط `a.cellMainLink`, الحجم، `td.green.center` للسيدرز و`td.red.lasttd.center` للـleechers (36–59). يزيل وسوم `strong` قبل التحليل (79–82).
- **حل المغناطيس:** لكل صف يزور صفحة التفاصيل فورًا، ثم يأخذ أول نص مقتبس يبدأ `magnet:`؛ إن لم يجده يعيد `NotFound` (45–67). بعد إخراج كل نتيجة ينتظر ثانية (45–58). هذا المصدر مكلف بطبيعته: طلب HTML لكل صفحة نتائج بالإضافة إلى طلب تفاصيل لكل نتيجة؛ اجعل الحل متزامنًا محدودًا أو كسولًا عند فتح النتيجة، مع عدم تجاوز مهلة المصدر.

### 045 — Pirateiro

- **المضيف:** الملف يثبت `https://pirateiro.io/` (12–14)، رغم قيمة `.com` في سجل المجتمع.
- **الفئات:** `all=0`, `anime=2`, `games=3`, `movies=1`, `music=4`, `software=6`, `tv=5` (12–24). في البحث لا يرسل فئة `all`؛ يضيف `&category={id}` للفئات الأخرى.
- **المسار والتوقف:** `https://pirateiro.io/search?query={query}&page={n}[&category=id]`, مع استبدال `%20` بـ`+`. حلقة `range(1, max_pages)` تعني الصفحات 1–9 (24، 84–99). تتوقف مبكرًا عند صفحة لا تحتوي نتائج (31–37).
- **الصفوف:** يحصر الاستخراج بعناصر `tr` لتجنب تكرار نسخة الجوال، ثم يطلب رابطًا ينتهي `/torrent/{digits}` داخل `a > h6`; يقرأ النص/العنوان من h6، والسيدرز من `btn-seed-home`، والـleechers من `btn-leech-home` (50–65). الحجم غير متوفر ويُرسل `-1`. رابط الوصف هو رابط التفاصيل نفسه.
- **حل النتيجة:** `download_torrent` يجلب صفحة التفاصيل بعد فك URL؛ يلتقط magnet مقتبسًا، وإلا يبحث عن `a.btn-down` ثم يعيد المحاولة على الرابط بعد استبدال `kickasstorrents` بـ`katcr`؛ وإلا يرمي خطأ (67–82). حقل `link` الصادر في البحث هو رابط التفاصيل، لا magnet (38–48، 54–64). حافظ على هذا كسلوك resolution منفصل ولا تعرض رابط الوصف كأنه magnet.

### 047 — RockBox

- **الفئات:** `all` فقط، مع تحذير صريح من أن فئات الموسيقى الدقيقة لا تلائم تصنيفات qBittorrent (12–20).
- **المسار:** `https://rawkbawx.rocks/torrents.php?active=0&search={query}&options=0&order=data&page={n}`، الصفحة تبدأ صفرًا، ويستبدل `%20` بـ`+` (76–85). يواصل حتى لا يجد نتائج، مع انتظار 3 ثوانٍ بين الصفحات (27–32، 79–89).
- **استخراج الحقول:** يلتقط صف `<TR>` كبيرًا يليه `td align="center"`; داخله href تفاصيل `details...`, نص `details:` للعنوان، href تنزيل `download...`, تاريخ `lista`, حجم TB/GB/MB/KB، وعددا peers (46–70). التاريخ `%d/%m/%Y` ويحوّل إلى Unix (55–58). الحجم والعدادات تخرج كنصوص كما ظهرت؛ لا يوجد تطبيع للفواصل هنا.
- **الرابط:** يحول `base + download-path` إلى URL-quoted ويخرجه كـ`link`; `desc_link` هو `base + details-path` (59–68). `download_torrent` يفك الرابط ثم يطبعه كما هو؛ لا يستخرج magnet بنفسه (72–74). لا تفترض أن الحقل `link` magnet قبل التحقق من redirect/استجابة الموقع.

### 051 — Snowfl

- **الفئات:** `all` فقط؛ الموقع لا يقدم تصنيفات بحسب تعليق المصدر (16–20).
- **استخراج token:** GET إلى `/index.html`، يستخرج اسم ملف يطابق `b.min.js?...`، يجلبه، ثم يستخرج token عبر regex خاص ببداية JavaScript (22–50). هذه نقطة هشاشة أعلى من بقية المصادر، فافصل token bootstrap عن parser وأظهر خطأ واضحًا عند تغير شكل السكربت.
- **استعلام JSON:** يبني مسار `/{token}/{query}/{random8}/0/SEED/NONE/1?_=timestamp_ms`، حيث random8 حروف صغيرة/أرقام، والوقت milliseconds (52–58). بحث واحد ثم `json.loads` (71–74).
- **النتائج:** من عناصر JSON: `magnet` إذا كان موجودًا، وإلا URL مشفر بالكامل؛ `name`, `size`, `seeder`, `leecher`, `url` كرابط وصف (27–40). `download_torrent` يقبل magnet كما هو؛ وإلا يطلب صفحة الوصف ويبحث عن magnet مقتبس (60–69). يلزم فك `what`/ترميزه وفق تمثيل طبقة البحث حتى لا يعاد ترميز `%` مرتين.

### 058 — The Pirate Bay

- **الفئات:** `all` فقط في الإضافة؛ يذكر تعليقها أن تصنيف الموقع يتطلب معاملات GET لكنه يختار `cat=0` (10–18، 39–43).
- **الطلب:** GET إلى `https://apibay.org/q.php?q={query}&cat=0`, ويفك JSON مباشرة. لا توجد pagination أو معالجة فشل ظاهرة في الملف (39–43).
- **الاستجابة:** إذا كان أول عنصر يحمل `name == "No results returned"` لا ينتج شيئًا. وإلا لكل عنصر يقرأ `info_hash`, `name`, `size`, `seeders`, `leechers`, `id` (20–37).
- **الرابط:** يبني magnet محليًا من hash والاسم المرمز ومجموعة tracker ثابتة؛ رابط الوصف القديم هو `https://thepiratebay.org/description.php?id={id}` (24–35). لا يحتاج فتح صفحة تفاصيل لإيجاد magnet. عند النقل حافظ على sentinel وعلى ترميز query/hash/name الصحيح، وتعامل مع استجابة فارغة أو غير JSON كفشل مصدر عادي بدل إيقاف باقي البحث.

### 064 — TorrentClaw

- **الوصول والمصادقة:** واجهة البحث العامة `https://torrentclaw.com/api/v1/search`; المفتاح اختياري للقراءة. عند ضبط `TORRENTCLAW_API_KEY` يرسل `Authorization: Bearer ...`, `User-Agent`, و`Accept: application/json` مع مهلة 15 ثانية عبر `urllib`, ما يتجاوز proxy الخاص بـqBittorrent حسب تعليق المصدر. دون المفتاح يستخدم `retrieve_url` (10–16، 25–30، 78–115). **هذا ليس مصدرًا خاصًا بحساب إلزامي في هذا الملف**: البحث العام يعمل بلا مفتاح، لذا لا تضعه ضمن قائمة الاستبعاد الخاصة؛ المفتاح فقط يفضّل magnet جاهزًا من API.
- **معاملات البحث:** يفك query مرة، ويرسل `q` و`limit=50`; `type=movie` أو `show` عند اختيار فئة، ولا يرسل type لـ`all` (62–71، 78–87، 173–178). لا pagination فوق 50.
- **شكل JSON:** جذر `results[]`; لكل عنصر `title`, `contentUrl`, `torrents[]` (180–185). كل torrent يتوقع `infoHash`; قد يحتوي `rawTitle`, `magnetUrl`, `qualityScore`, `scanStatus`, `sizeBytes`, `seeders`, `leechers`, `uploadedAt` (117–169).
- **الـmagnet والعنوان:** يفضل `magnetUrl`, وإلا يبنيه من `infoHash`, عنوان `rawTitle` أو عنوان المحتوى، ويضيف قائمة trackers ثابتة. يستبدل المسافات في `dn` بنقاط قبل URL encoding. يضيف `[TC-n]` للـqualityScore الصحيح و`[TrueSpec]` عندما `scanStatus == success` (32–47، 73–76، 117–140).
- **التطبيع:** `sizeBytes` يتحول إلى B/KB/MB/GB/TB بقسمة 1024 ومنزلتين؛ العدادات غير الصحيحة تصبح `-1`; `uploadedAt` ISO-8601 (مع تحويل `Z`) إلى Unix، والخطأ `-1`. `contentUrl` يدمج مع base كرابط وصف (49–59، 140–169). لا تسجل API key أو تضعه في URL.

### 065 — TorrentDownload

- **الفئات:** كل، anime, books, games, movies, music, software (`applications`), tv. الصفحة لا تتلقى category؛ الإضافة تصنف محليًا من وسم الفئة الموجود في الصف (13–28، 56–65، 114–123).
- **المسار والتوقف:** `/search?q={query}&p={page}`, استبدال `%20` بـ`+`, الصفحات 1–10. مهلة ثانية بين الصفحات (125–141). يقرأ إجمالي النتائج من عنوان يطابق `N - M of TOTAL for ...`; يتوقف عندما لا يوجد صف صالح أو يصل عدد hashes المرئية إلى total (54–55، 74–89، 129–141).
- **استخراج الصف:** صفوف `<tr><td...tt-name...`; regex صف مستقل ثم hash من 40 hex في رابط `/{hash}/{slug}`، اسم، فئة اختيارية ضمن `smallish`, حجم وseed/leech من خلايا `tdnormal`, `tdseed`, `tdleech` (40–53، 74–89). يحول hash إلى lowercase ويزيل الفواصل عن counters والحجم؛ النتائج المكررة تزال حسب hash. تصفية الفئة محلية، باسم فئات يتضمن قواعد substring مثل `television`, `lossless`, `playstation`, `applic` (56–65، 84–90، 107–123).
- **الـmagnet:** يركبه من `info_hash`, و`slug` كقيمة `dn`, ثم 7 trackers ثابتة مشفرة مسبقًا. العنوان الظاهر ينظف الوسوم ويفك HTML entities. رابط الوصف `/{original-hash}/{slug}` (30–38، 91–112). لا تحتاج طلب تفاصيل لكل نتيجة.
- **حدود:** لا تعتمد على مطابقة HTML regex بترتيب غامض؛ استخدم صف DOM أو parser متسامح مع attribute order، مع اختبار صف sponsored، ووسم فئة غائب، وhash مكرر، وtotal غير موجود. حد الصفحات 10 جزء من السلوك الحالي.

### 075 — YourBittorrent

- **الفئات:** `all=0`, `movies=1`, `tv=3`, `music=2`, `games=4`, `anime=6`, `software=5` (11–24).
- **المسار والعدد:** `https://yourbittorrent.com/?q={query}[&c={id}]`; يحول `%20` إلى `-`. لا توجد pagination؛ تعليق المصدر يقول إن التنقل معطوب ولا يمكن جلب أكثر من 50 نتيجة (24، 88–95).
- **استخراج النتائج:** ينتقي القسم الثاني فقط من `<div class="table-responsive">... </table></div>`, ثم صفوف `<tr class="table-default">` (50–56). يستخرج href، title، الحجم TB/GB/MB/kB، الخلية ذات `sd` للسيدرز، و`pr` للـleechers (57–78). يحذف بعض الوسوم بالاستبدال النصي، ويزيل الفواصل من المقادير.
- **الرابط والحل:** `link` عنوان absolute URL ممرر عبر `urllib.parse.quote`، و`desc_link` هو العنوان نفسه بعد فك الترميز (39–48، 63–77). لا يوجد magnet في البحث. `download_torrent` يجلب صفحة الرابط، يبحث عن `down/...torrent`, ثم يطلب ملف torrent بواسطة `download_file`; إذا لم يجده يرمي خطأ (80–86). يجب تمثيل هذا كمصدر يعيد رابطًا قابلًا للحل/ملف torrent، لا اختراع magnet من الصفحة.

## مجموعات إعادة الاستخدام المقترحة

هذه مجموعات تنفيذ تقلل الازدواجية مع إبقاء لكل موقع parser مستقلًا:

1. **موصل HTML ذاتي الصفحات:** GloTorrents وKickass وRockBox. استخدم builder لكل route، parser لكل صف، سياسة توقف منفصلة، وطلب/مهلة/تأخير مضبوطين لكل مصدر. Kickass يختلف بأنه يجلب صفحة تفاصيل لكل نتيجة.
2. **HTML bounded/category post-filter:** Pirateiro وTorrentDownload. يحتفظ الأول بتفاصيل/حل URL ويتوقف عند 9 صفحات؛ الثاني يستخدم hash dedup، total، تصنيف محلي، وmagnet من hash.
3. **تحميل رابط تفاصيل عند الحاجة:** YourBittorrent وPirateiro. كليهما لا يقدم magnet في نتائج البحث؛ YourBittorrent ينتهي بتنزيل `.torrent`, بينما Pirateiro يبحث عن magnet/رابط بديل.
4. **واجهات JSON:** TPB وTorrentClaw وSnowfl. لا توحد parsing أو schema الخاص بالموقع، لكن يمكنها مشاركة HTTP/JSON/error/timeout؛ Snowfl يحتاج خطوة bootstrap token، وTorrentClaw لديه auth اختياري.
5. **بيانات محلية إثرائية:** AcademicTorrents. فصل cache TTL اليومي عن parser XML وعن إثراء `/tech`، مع concurrency cap ونتيجة جزئية عند فشل تفاصيل عنصر.

## ثوابت التوافق التي ينبغي اختبارها عند النقل

- مرّر الفئات المدعومة كما هي في كل جدول؛ أعد خطأ فئة واضحًا أو `all` حسب عقد التطبيق، ولا ترسل category غير مدعومة للموقع.
- أنشئ لكل مصدر اختبارات URL/query، صفحة نتائج حقيقية محفوظة، حالة فارغة، parsing/normalization، pagination stop، duplicate handling، ومهلة/استجابة خطأ. لا تختبر عبر تشغيل ملفات Python الخارجية.
- افصل `magnet`, رابط صفحة التفاصيل، رابط HTTP لملف `.torrent`, ورابط التنزيل/redirect في تمثيل النتيجة. AcademicTorrents وKickass وPirateiro وRockBox وYourBittorrent لا تطابق نمط «كل نتيجة تعطي magnet جاهزًا».
- حافظ على حدود الطلبات والتأخيرات المقصودة وعلى حد كل مصدر. البحث المتوازي بين المصادر شيء، لكن لا يبرر إغراق صفحات مصدر واحد أو جلب صفحات التفاصيل دون سقف.
- احفظ `community-20261017/catalog.json` ونسخ Python والبصمات كمرجع إصداري. تحديث الكتالوج لاحقًا ينبغي أن يكشف إضافات عامة جديدة، تغييرات مسار، تغييرات license/author، وسجلات خاصة مستبعدة دون إسقاط دعم Python.

## أدلة محلية

- كتالوج المجتمع والصفوف العامة/الروابط/البصمات: `research/qbittorrent/community-20261017/catalog.json`.
- ملفات المصدر الساكنة: `research/qbittorrent/community-20261017/sources/001.py`, `026.py`, `028.py`, `045.py`, `047.py`, `051.py`, `058.py`, `064.py`, `065.py`, `075.py`.
- هذه الوثيقة تسجل سلوك ملفات المصدر الموجودة في اللقطة. قابلية الوصول الحالية لكل نطاق/endpoint لم تُختبر، ويمكن أن تتغير المواقع بعد تاريخ اللقطة.
