# فهرس إضافات بحث qBittorrent ونطاق نقلها إلى Rust

تاريخ التحقق: 2026-10-17. هذا فحص ساكن للمستودعات والويكي الرسمية المرتبطة بمشروع qBittorrent؛ لم تُشغّل أي إضافة خارجية.

## الخلاصة ونطاق «كل الإضافات»

هناك مصدران لهما صلة مباشرة بطلب النقل:

| المجموعة | العدد المرصود | ما الذي يمثله العدد؟ |
|---|---:|---|
| ملفات المحركات في المستودع `qbittorrent/search-plugins` | 8 | إضافات Python الموجودة في `nova3/engines`، بعد استثناء `__init__.py` |
| صفحة الويكي للإضافات غير الرسمية | 78 عامة + 28 خاصة = 106 | صفوف/تسجيلات إضافات، لا عدد مواقع أو محركات فريدة |

العدد الثاني يتضمن أكثر من تطبيق للموقع نفسه، وبدائل وتصنيفات مختلفة، وروابط إلى مستودعات GitHub وGitLab وCodeberg. بعض الصفوف تربط بصفحة مستودع أو ملف قراءة بدلا من رابط تنزيل مباشر. لذلك لا يساوي 106 محولا مستقلا، ولا ينبغي تنزيل كل ما يظهر في الصفحة وتشغيله كما هو.

المستودع تحت منظمة qBittorrent هو الفهرس البرمجي الأساسي، لكن README يوضح أن صيانته من مساهمين مجتمعيين وأنه ليس مُدارا مباشرة من فريق qBittorrent الأساسي. أما الويكي فتصف نفسها بأنها تجميعة إضافات مجتمعية وتحذر صراحة من أن سكربتات Python غير الرسمية غير آمنة بطبيعتها. وعليه، يجب تسمية المجموعة الأولى «إضافات المستودع الأساسي»، والثانية «إضافات الويكي غير الرسمية»، لا افتراض أن كل صف معتمد أو مدعوم من فريق التطبيق. [README للمستودع](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/README.md) · [الويكي غير الرسمية](https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins)

## ملفات المستودع الأساسي

الفرع `master` كان عند commit `240b57a90ca4f9e3869151b73c76c46f1b7bb59b` بتاريخ 2026-10-16. ويكي GitHub كان عند HEAD `e00fd9cd551258290213e684d522bc5e06130a99`، وصفحتها أظهرت آخر تحرير في 2026-10-16. هذه الملفات الثمانية هي قائمة قابلة للتثبيت من ذلك المصدر، وروابطها مثبتة على commit الفحص:

| المحرك | ملف المصدر |
|---|---|
| EZTV | [`eztv.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/eztv.py) |
| Jackett | [`jackett.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/jackett.py) |
| LimeTorrents | [`limetorrents.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/limetorrents.py) |
| The Pirate Bay | [`piratebay.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/piratebay.py) |
| SolidTorrents | [`solidtorrents.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/solidtorrents.py) |
| TorLock | [`torlock.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/torlock.py) |
| TorrentProject | [`torrentproject.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/torrentproject.py) |
| Torrents CSV | [`torrentscsv.py`](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/torrentscsv.py) |

في نسخة العمل الحالية، LimeTorrents Python مثبت على 4.14 بينما المصدر عند commit الفحص 4.15؛ لذلك يجب مقارنة Rust مع النسخة 4.15 الحالية قبل إعلان تكافؤ أو إتمام الترحيل. Torrents CSV عند 1.8 في المصدر الحالي، وهي النسخة الموجودة محليا.

## كيف نعيد جلب الفهرس كاملا

لا توجد قائمة ثابتة ينبغي نسخها يدويا. استخدم GitHub API لتعداد ملفات المستودع، ثم ثبّت المصدر على commit وسجّل بصمات الملفات:

```powershell
$repo = Invoke-RestMethod 'https://api.github.com/repos/qbittorrent/search-plugins'
$branch = Invoke-RestMethod "https://api.github.com/repos/qbittorrent/search-plugins/branches/$($repo.default_branch)"
$commit = $branch.commit.sha
$tree = Invoke-RestMethod "https://api.github.com/repos/qbittorrent/search-plugins/git/trees/${commit}?recursive=1"
$plugins = $tree.tree | Where-Object {
  $_.path -match '^nova3/engines/[^/]+\.py$' -and
  $_.path -ne 'nova3/engines/__init__.py'
}
$plugins | ForEach-Object {
  [pscustomobject]@{
    path = $_.path
    source = "https://raw.githubusercontent.com/qbittorrent/search-plugins/$commit/$($_.path)"
    blob = $_.sha
  }
}
```

لجرد المجتمع، استنسخ مستودع الويكي نفسه واقرأ جدولي `Plugins for Public Sites` و`Plugins for Private Sites` من صفحة `Unofficial-search-plugins.md`:

```powershell
git clone --depth 1 https://github.com/qbittorrent/search-plugins.wiki.git qbit-plugin-wiki
```

سجّل روابط التنزيل كما هي في الجدول. بعض الروابط على GitHub وبعضها على GitLab أو Codeberg؛ لا تستنتج مسار الملف من اسم صاحب الإضافة. ثبّت كل رابط على commit أو إصدار محفوظ، واحفظ رابط الأصل وSHA-256 مع كل نسخة. جرد الويكي ليس API بإصدارات ثابتة: هو قائمة تتغير، وبعض روابطها متحركة أو تحتاج اختيار الملف يدويا.

## ما يلزم الحفاظ عليه في التوافق

مواصفة qBittorrent الحالية تعرف الإضافة كملف Python يحوي صنفا باسم ملف المحرك، وحقول `url` و`name` و`supported_categories`، ودالة `search(query, category)`، مع `download_torrent(url)` اختيارية. تمرر النتائج عبر `stdout` بصيغة الحقول الثمانية: `link|name|size|seeds|leech|engine_url|desc_link|pub_date`. توثق المواصفة أن الحجم بالبايت، ووقت النشر Unix timestamp، وأن القيم المجهولة تستخدم `-1`. كما تقول إن الاستعلام يصل إلى `search` بعد الترميز. [مواصفة الإضافات](https://github.com/qbittorrent/search-plugins/wiki/How-to-write-a-search-plugin)

إذن النقل إلى Rust يجب أن يضيف تنفيذا أصليا لكل مصدر إلى محركنا، مع إبقاء ملف Python الأصلي ومشغل التوافق كمسار صالح للمستخدم. نحتفظ بواجهة داخلية موحدة للنتائج، ونحفظ بيانات المصدر والإصدار والبصمة. عندما يُختار Python، تظل الإضافة تعمل بعقد qBittorrent الحالي. وعندما يُختار Rust، يقارن اختبار المطابقة الحقول والرابط والتصنيف والتصفح بين التطبيقين. لا ينبغي حذف ملف Python أو تحويل الاختيار الافتراضي قبل تحقق التوافق لكل مصدر.

دالة `download_torrent` ومساعد `download_file()` في qBittorrent يعالجان جلب/حل ملف `.torrent` المؤقت عند حاجة الإضافة إليه؛ هذا ليس تنزيل محتوى الملفات عبر BitTorrent. عند توصيلهما بالمنتج، نبقي جلب ملف `.torrent` أو استخراج magnet ضمن تحضير المرجع، ونترك تنزيل المحتوى لخدمة التنزيل المنفصلة. [مساعد `download_file` في المواصفة](https://github.com/qbittorrent/search-plugins/wiki/How-to-write-a-search-plugin#download_file-helper-function)

المستودع يوصي بمكتبات Python القياسية فقط لأن المكتبات الخارجية غير مضمونة في بيئة مستخدم qBittorrent. الإضافات الرسمية تستورد `helpers` و`novaprinter` المشتركتين، وJackett يلمس إعدادات وأدوات إضافية. عند إعادة التنفيذ في Rust، نحتاج فهم سلوك المساعدات المشتركة لكل إضافة؛ أسماء دوال Python ليست عقد HTTP كافيا وحدها. [دليل الكتابة](https://github.com/qbittorrent/search-plugins/wiki/How-to-write-a-search-plugin) · [مساعدات runtime في commit الحالي](https://github.com/qbittorrent/search-plugins/tree/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3)

## التجميعات والاعتماد على خدمات أخرى

- إضافة `jackett.py` ليست محرك موقع واحد؛ إنها عميل لخدمة Jackett وفهارسها، ولها عنوان خدمة ومفتاح API وإعدادات محلية. نقلها يعني دعم عقد Jackett API أو الإبقاء على Python، ولا يعني كتابة scraper آخر لموقع تورنت.
- تسجل الويكي `Prowlarr` و`TorrentClaw` أيضا، وTorrentClaw موصوف كمجمّع Torznab لنحو عشرة مصادر مع مفتاح API اختياري. كما تسجل `YGGtracker` مع شرط اتصال Yggdrasil. هذه موصلات إلى خدمات/شبكات وسيطة وتحتاج تصنيفا مستقلا عن محركات المواقع المباشرة. [Jackett في المصدر](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines/jackett.py) · [صفحة الويكي](https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins)
- تكرر الويكي مواقع وروابط ذات أكثر من تطبيق؛ مثلا DMHY وRuTracker وUnionFansub، وتعرض بدائل معنونة «with categories» لـMagnetDL وThePirateBay وTorrentDownload. نجمعها تحت المصدر المنطقي نفسه فقط بعد مقارنة السلوك والحقول والتصنيفات؛ لا نحذف خيارا لمجرد تشابه اسم الموقع.

## عوائق التشغيل والبيانات السرية

الويكي تقسم الإضافات صراحة إلى مواقع عامة وخاصة، وتحذر أن الخاصة قد تتطلب التسجيل أو إدخال بيانات اعتماد في السكربت. أمثلة مذكورة في الصفحة نفسها: BitPorn يتطلب login/cookies؛ C411 يتطلب API key؛ وTR4KER موصوف بأنه شبه خاص ويتطلب API key. وهناك إضافات تشير إلى حماية Cloudflare أو توافق محدود مع إصدارات qBittorrent/Python. [قائمة الإضافات غير الرسمية](https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins)

لا يمكن اعتبار مصدر خاص «مختبرا حيا» من دون حساب مشروع لدى المستخدم. إذا نُقل، تُدخل بيانات الحساب محليا كإعداد سري مستقل، ولا توضع داخل Rust source أو Git أو سجل نتائج البحث. ويظل خيار Python مفيدا عندما لا نستطيع إثبات تكافؤ مسار المصادقة أو يطلب الموقع تحديثا.

## الترخيص والنسب

تحدد بيانات المستودع الأساسي ترخيص المستودع `GPL-2.0`، لكن مراجعة ملفات المحركات تُظهر إشعارات مخصصة داخل بعض الملفات: `piratebay.py` و`torrentscsv.py` يحملان نص ترخيص BSD-style، و`solidtorrents.py` يحمل MIT-style، بينما ملفات أخرى لا تعرض إشعارا مماثلا في رأسها. لذلك لا نتعامل مع ترخيص صف الويكي أو ترخيص مشروع qBittorrent كأنه تفويض موحد لكل كود مجتمعي مرتبط. [ترخيص المستودع](https://github.com/qbittorrent/search-plugins/blob/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/LICENSE) · [ملفات الإضافات عند commit الفحص](https://github.com/qbittorrent/search-plugins/tree/240b57a90ca4f9e3869151b73c76c46f1b7bb59b/nova3/engines)

قبل كل نقل مباشر للكود، نسجل المؤلفين ونسخة المصدر وملف `LICENSE` أو الترويسة وأي إشعار يجب أن يصاحب التوزيع؛ ثم نحتفظ بملفات الإشعار في حزمة البرنامج. كل مستودع خارجي من الويكي يحتاج فحص ترخيصه الخاص. لا يُفترض أن رابط التنزيل يمنح حق إعادة توزيع أو تحويل المصدر.

## ترتيب تنفيذ عملي

1. نبدأ بثمانية محركات المستودع الأساسي؛ اثنان منها موجودان في المشروع بالفعل. نثبت snapshot حديثا لكل ملف ونحدّث LimeTorrents من 4.14 إلى 4.15 قبل قياسه.
2. نضيف محرك Rust واحدا في كل مرة، ونبقي Python الأصلي مسجلا بإصدار وبصمة. نختبرهما بنفس الاستعلام والاستجابة المسجلة، ثم بموقع فعلي عندما يكون متاحا.
3. نجرد صفوف الويكي الـ106 إلى manifest فيه اسم المصدر، نوعه (موقع/مجمّع/خاص)، upstream URL وcommit، المؤلف والترخيص، المصادقة، Python compatibility، الدليل الحي، وخيار Rust/Python. نجمع المصادر المتكررة بعد المقارنة فقط.
4. ننقل أولا المصادر العامة ذات HTTP/JSON أو HTML المستقر، ونؤجل المصدر الذي يتطلب حسابا أو API خارجي أو تجاوز حماية إلى حين توافر حساب/عقد رسمي واختبار قابل للإعادة. يظل Python خيار تشغيل له.

هذا يجعل «كل إضافات qBittorrent» هدفا قابلا للتدقيق والتقدم: الفهرس المرجعي محدد، كل منفذ قابل للمقارنة، والتوافق لا يضيع أثناء الترحيل.
