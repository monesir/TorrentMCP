# مصادر البحث المحلية

تشمل اللقطة 8 إضافات من المستودع الأساسي و78 إضافة عامة من قائمة المجتمع؛ 28 إضافة خاصة مستبعدة. تحفظ نسخ الموقع المختلفة بمعرفات مستقلة مثل `thepiratebay_community_059`. لا يحتاج المحرك تثبيت qBittorrent، ولا ينشر خادمًا عامًا.

- [ملفات الإضافات الأساسية](https://github.com/qbittorrent/search-plugins/tree/master/nova3/engines)
- [قائمة المجتمع](https://github.com/qbittorrent/search-plugins/wiki/Unofficial-search-plugins)

المصدر الأساسي مثبت على commit `240b57a90ca4f9e3869151b73c76c46f1b7bb59b`، وقائمة المجتمع على `e00fd9cd551258290213e684d522bc5e06130a99`. السجل التشغيلي `resources/sources.json` يثبت بصمات Python الأصلية؛ الكتالوج البحثي يحفظ رابط كل ملف وإصداره واستعادة المصادر المؤرشفة. ليست هناك عملية تنزيل أو تشغيل تلقائي لإضافات جديدة.

## التشغيل

```powershell
.\torrent-search.exe --json sources
.\torrent-search.exe --source-backend rust --json search "nirvana in fire" --source limetorrents --source nyaasi --timeout 20 --limit 30
.\torrent-search.exe --source-backend python --json search "nirvana in fire" --source limetorrents --timeout 20 --limit 30
```

`auto` يفضل التنفيذ الأصلي ويحتفظ بموصل Python للإضافات الجديدة غير المنقولة. لا يعيد طلبًا فاشلًا في Python تلقائيًا. `--limetorrents-backend` يبقى تجاوزًا خاصًا للمقارنة السابقة. البحث دون `--source` يختار جميع الإضافات المسجلة، بما فيها المواقع المتوقفة وJackett إن لم يضبط؛ للحصول على بحث عملي اختر المصادر التي تعمل في شبكتك.

كل مصدر له مهلة وحد نتائج وتقرير مستقل. لا يتجاوز التوازي العام 16 مصدرًا. تحافظ الموصلات على فئات المصدر وتوقيت طلباته وفروق العناوين والأحجام والعدادات؛ الفشل والحماية والمهلة لا يعنيان عدم وجود تورنت. بعض المصادر المؤرشفة أو المتوقفة قابلة للاختبار على عقدها المحفوظ فقط.

## إعدادات المصادر

Jackett اختياري. أنشئ `resources/qbit/engines/jackett.json` بقيم خدمة Jackett المحلية، مثل `url`, `api_key`, `tracker_first`, `thread_count`. لا يتضمن البرنامج خادم Jackett أو مفتاحًا جاهزًا.

Rutor يقرأ `resources/qbit/engines/rutor.json` إن وجد، بما فيه `magnet`, `proxy`, `proxies` و`ua`. عند تفعيل proxy يجب ضبط عنوانه. لا يدخل هذا الملف أو إعداد Jackett أو cookies أو سجلات الإضافات في الحزمة.

TorrentClaw يقبل `TORRENTCLAW_API_KEY` اختياريًا. EZTVX يقبل `EZTVX_OMDB_API_KEY` لتحسين البحث بهوية IMDb؛ البحث بالكلمات يبقى متاحًا عند عدم وجود مفتاح صالح.

FitGirl يحافظ على استخراج روابط المقالات والحجم والتاريخ وذاكرة التفاصيل وإضافة trackers. يتحكم `FITGIRL_CACHE=0` في تعطيل ذاكرة الثلاثة أيام و`FITGIRL_STREAMING=0` في الإخراج بترتيب الصفحة. `FITGIRL_SCRAPE=0` يعطل اتصالات فحص tracker؛ الفحص مفعّل افتراضيًا كما في الأصل ويرسل infohash إلى trackers عبر HTTP/UDP مباشرة. اتصالات UDP لا تمر عبر proxy HTTP. تظل مهلة المحرك حاكمة، ومهلة FitGirl القصوى 40 ثانية.

تستخدم الذاكرة المحلية `%LOCALAPPDATA%/torrent-search/cache`، أو `TORRENT_SEARCH_CACHE_DIR` إن ضبط. AcademicTorrents يحتفظ بقاعدة XML لليوم الحالي مع سقف استجابة 64 MiB، ويجلب بيانات peers من صفحات التفاصيل المطابقة. معظم استجابات المواقع الأخرى محدودة بـ4 MiB.

## مراجع التنزيل

البحث يعيد magnet أو مرجع HTTP. قد يكون HTTP صفحة تفاصيل تحتاج `resolve`، أو رابط بيانات torrent صالحًا لخدمة التنزيل المستقلة. لا يجلب البحث ملفات الفيديو. بعض المصادر، مثل Zooqle، تحتاج طلب POST:

```json
{
  "kind": "http",
  "uri": "https://zooqle.skin/torfile/#hid=example",
  "source_id": "zooqle",
  "post_form": {"hid": "example"}
}
```

يستخدم المستهلك `post_form` مع POST و`application/x-www-form-urlencoded`؛ غيابه يعني GET. جزء `#hid` يحفظ هوية المرجع عند تمريره إلى CLI ولا يرسل إلى الموقع. لا يتضمن المرجع إعدادات الحساب أو cookies. لا تندمج معرفات POST المختلفة في نتيجة واحدة.

## التوافق والتحقق

تظل ملفات Python الأصلية دون تعديل؛ العامل يكيّف أسماء المساعدات القديمة وترميز المراجع وبعض مراجع التنزيل الثنائية إلى عقد خدمة التنزيل. تضم حزمة Windows Python 3.14.8 ومكتبات Beautiful Soup وlxml وrequests وتبعياتها، مع بصمات تحقق في `resources/python-dependencies.lock.json`. يثبت مسار التغليف هذه الملفات داخل البيئة المضمنة دون الحاجة إلى pip عند المستخدم.

اختبارات `scripts/verify-native-ports.py` تقارن العنوان والمرجع والحجم والعدادات والرابط الوصفي والتاريخ ومسارات الطلبات لكل فئة. يشغّل الاختبار Rust مع مسار Python غير موجود، ويختبر حدود النتائج والفشل والنتائج الجزئية والحماية والمهل. البيانات اصطناعية لاختبار البروتوكولات، باستثناء HTML الملتقط سابقًا لـLimeTorrents. اختبارات tracker تستخدم خوادم HTTP وUDP محلية. هذه الأدلة منفصلة عن اختبار توافر المواقع الحية؛ راجع `verification/native-ports/STATE.md` والتقارير المرفقة.

استعادة مصادر قديمة وتطابق fixtures لا تثبت عملها على الإنترنت. لا يتخطى المحرك CAPTCHA أو الحماية التي يرفضها الموقع.
