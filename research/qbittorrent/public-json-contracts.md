# عقود ثمانية مصادر عامة من إضافات qBittorrent

**تاريخ لقطة المصدر:** 2026-10-17. هذا تحليل ساكن لنسخ Python المحفوظة؛ لم أشغّل إضافات المجتمع ولم أختبر المواقع مباشرة. الغرض هو توثيق السلوك المطلوب لإعادة تنفيذه في Rust مع إبقاء Python للتوافق.

## النطاق والثبات

السجلات الثمانية عامة (visibility=public وin_scope=true) في community-20261017/catalog.json: 015 dark-libria، 024 EZTV، 027 Gog-games، 038 nekoBT، 052 Snowfl، 071 TsukiHime، 076 YTS، 077 YGGtracker. الأرقام معرّفات الكتالوج وليست معرفات Rust.

| السجل | النسخة في ترويسة الملف (الويكي) | المستودع ومصدر الملف | SHA-256 لنسخة Python المحفوظة |
|---|---|---|---|
| 015 dark-libria | 0.13 (0.10) | [bugsbringer/qbit-plugins](https://github.com/bugsbringer/qbit-plugins) · [darklibria.py](https://raw.githubusercontent.com/bugsbringer/qbit-plugins/master/darklibria.py) | c2e2cd728b69e3a1ddb75374dc95a0010b6f85d037dc9f38eefb48c7f42b058f |
| 024 EZTV | 3.20 (2.00) | [DrPurp/eztvx-qbittorrent-plugin](https://github.com/DrPurp/eztvx-qbittorrent-plugin) · [eztvx.py](https://raw.githubusercontent.com/DrPurp/eztvx-qbittorrent-plugin/main/eztvx.py) | 07383109341fd035073f35ca3ddcd9095a407afcba8248b573419ae90759767a |
| 027 Gog-games | 1.0 (1.0) | [tolotp/qbittorrent-search-plugins-de-busqueda](https://github.com/tolotp/qbittorrent-search-plugins-de-busqueda) · [goggames.py](https://raw.githubusercontent.com/tolotp/qbittorrent-search-plugins-de-busqueda/refs/heads/main/Plugins/goggames.py) | 756bf172b89051f15f726618fc38cd3a44cb74a2a810a46ed2d1ed8fcbdb7642 |
| 038 nekoBT | 1.0 (1.0) | [tolotp/qbittorrent-search-plugins-de-busqueda](https://github.com/tolotp/qbittorrent-search-plugins-de-busqueda) · [nekobt.py](https://raw.githubusercontent.com/tolotp/qbittorrent-search-plugins-de-busqueda/refs/heads/main/Plugins/nekobt.py) | 29aba65332f19823208c1186d4366b5d909f0cab4043a6876f204e417819964a |
| 052 Snowfl | 2.1 (2.1) | [ChocoTonic/snowfl](https://github.com/ChocoTonic/snowfl) · [snowfl.py](https://github.com/ChocoTonic/snowfl/releases/latest/download/snowfl.py) | 58e17e41590dd362cbee0f148f896b2b72bcaba619cfd5d9da3673dbe8c6942c |
| 071 TsukiHime | 1.00 (1.0) | [dominc8/qbittorrent-tsukihime-search-plugin](https://github.com/dominc8/qbittorrent-tsukihime-search-plugin) · [tsukihime.py](https://raw.githubusercontent.com/dominc8/qbittorrent-tsukihime-search-plugin/refs/heads/master/tsukihime.py) | 1b12c5ee8ea7e9dfdab0d7f1c705b150f57cee249d18ed18d88e719bf73b535a |
| 076 YTS | 1.9 (1.6) | [lazulyra/qbit-plugins](https://codeberg.org/lazulyra/qbit-plugins) · [yts.py](https://codeberg.org/lazulyra/qbit-plugins/raw/branch/main/yts/yts.py) | cb066319b5f6092621b21debc4303d937bacd2bdaef643f1740a516145cbfb9e |
| 077 YGGtracker | 1.1.0 (1.1.0) | [YGGverse/qbittorrent-yggtracker-search-plugin](https://github.com/YGGverse/qbittorrent-yggtracker-search-plugin) · [yggtracker.py](https://raw.githubusercontent.com/YGGverse/qbittorrent-yggtracker-search-plugin/main/yggtracker.py) | e5fdede3e0f2f721c8fbcf5cbd1af9643e423b7a1baf8bf689e65b623d78d8d9 |

قيمة إصدار الويكي تختلف عن ترويسة الملف المحفوظ في dark-libria (0.10 مقابل 0.13)، EZTV (2.00 مقابل 3.20)، وYTS (1.6 مقابل 1.9). استخدم البصمة ورأس المصدر كنسخة الكود المراد نقلها، ولا تستنتج سلوك التنفيذ من عمود الإصدار في الويكي. حالة الترخيص في manifest هي pending_upstream_review للسجلات الثمانية. TsukiHime يضم نص MIT كاملا في الأسطر 4–21؛ YGGtracker يذكر MIT في الرأس. بقية الملفات الستة لا تذكر ترخيصا في الملف المحفوظ؛ راجع ترخيص المستودع الأصلي قبل توزيع منفذ مشتق. [كتالوج اللقطة](community-20261017/catalog.json) · [نسخة الويكي](community-20261017/Unofficial-search-plugins.mediawiki)

## عقود التنفيذ

### 015 — dark-libria

- **الفئات والاستعلام:** all فقط؛ cat لا يغير الطلب. يفك query ثم يعيد ترميزه، ويطلب https://dark-libria.it/search?page={page}&find={what}. يبدأ بالصفحة الأولى، ويحسب عدد الصفحات من span.text.text-light.mt-0 بقسمة العدد الإجمالي على عدد عناصر الصفحة وتقريب ceil، ثم يطلب 2..N. غياب العنصر يجعل pages_count صفرا. [البحث والصفحات](community-20261017/sources/015.py#L25-L57) [حساب الصفحات](community-20261017/sources/015.py#L122-L134)
- **النتائج:** من صفحة البحث يأخذ كل tbody ذات style="vertical-align: center"، ثم أول رابط a إلى صفحة المسلسل. فيها يقرأ الاسم من id="russian_name" وصفوف tr.torrent؛ ترتيب خلايا كل صف: النوع، الجودة، الحجم، التاريخ، التنزيل، seeds، leech. الرابط يفضل anchor بعنوان Magnet-ссылка؛ يحاول Скачать торрент فقط إن كان href الأول فارغا، بينما غياب anchor الأول يسبب استثناء. الاسم الناتج [التاريخ المحلي] الاسم [النوع] الجودة. التاريخ يقرأ YYYY-MM-DD HH:MM:SS على أنه UTC ثم يحوله إلى محلي، لكنه يضيف التاريخ للاسم ولا يخرج pub_date. الحجم يحتفظ بالرقم ويحوّل الوحدات الروسية Тб/Гб/Мб/Кб/б إلى TB/GB/MB/KB/B. desc_link صفحة المسلسل وengine_url الموقع. [استخراج الحقول](community-20261017/sources/015.py#L59-L100)
- **الأخطاء والتوازي:** فشل أي GET يطبع نتيجة خطأ اصطناعية: link=Error وname=Connection failed وsize=0 وseeds/leech=-1. صفحات المسلسلات والتفاصيل تجلب بالتوازي؛ ترتيب النتائج بحسب اكتمال الطلبات. عند النقل مثّل الفشل بتقرير مصدر، ولا تعامل الصف الاصطناعي كنتيجة قابلة للتنزيل. [التوازي](community-20261017/sources/015.py#L41-L57) [الأخطاء](community-20261017/sources/015.py#L102-L120)

### 024 — EZTV

- **الفئات والتطبيع:** يعلن all وtv لكن لا يستخدم cat. يستبدل %20 بمسافة، والنقطة والشرطة بمسافة ثم يختزل المسافات. لا يفك + صراحة؛ وهذا مهم لأن أمثلة عقد qBittorrent تمرر query escaped مثل Ubuntu+Linux. اختبر + و%20؛ التصفية المحلية الحالية قد تختلف عن query الذي يفهمه الخادم. [المصدر](community-20261017/sources/024.py#L10-L37)
- **الوسوم:** يتعرف على S02E03 وS02 وE03 و2x03 وseason 2 episode 3 وseason 2. يحذف وسم الحلقة من العنوان، ويحذف وسوم جودة/ترميز محددة: 1080p و720p و480p و2160p و4k وx264 وx265 وhevc وavc وbluray وwebrip وweb-dl وhdtv وdvdrip وproper وrepack وextended وtheatrical وdirectors.cut وremux. بعدها يحاول OMDb type=series لجلب IMDb ID؛ المفتاح في النسخة المحفوظة هو النص البديل YOUR_OMDB_API_KEY والأخطاء مخفية، لذلك غالبا يتراجع إلى بحث الكلمات ما لم يضاف مفتاح صالح محليا. [تحليل وتنظيف query](community-20261017/sources/024.py#L39-L87) [OMDb](community-20261017/sources/024.py#L124-L140)
- **الطلب والتصفح:** IMDb: /api/get-torrents?limit=100&page=N&imdb_id=ID. كلمات: /api/get-torrents?limit=100&page=N&Keywords={title مع + للمسافات}. يقرأ torrents[] وtorrents_count ويتوقف عند قائمة فارغة أو page*100 >= total أو أقل من 100 عنصر. بحث الكلمات يشترط محليا ظهور كل term في العنوان lowercase ثم يطابق الموسم/الحلقة؛ IMDb يكتفي بالموسم/الحلقة. فشل النقل أو JSON يوقف الصفحات بصمت ويحفظ النتائج السابقة. [IMDb](community-20261017/sources/024.py#L142-L167) [الكلمات](community-20261017/sources/024.py#L168-L196)
- **الخرج:** link من magnet_url وإلا torrent_url، مع إسقاط العنصر إذا غابا؛ name=title، الحجم من size_bytes إلى KB/MB/GB بتقريب 1/1/2 منازل، seeds وpeers كعدادات، desc_link من episode_url وإلا الموقع. لا pub_date. [المصدر](community-20261017/sources/024.py#L198-L225)

### 027 — Gog-games

- **الفئات والطلب:** all فقط. يفك query ويقصه ويحول الحروف إلى lowercase. الاستعلام الحرفي . أو * أو ! يطلب /api/web/recent-torrents؛ خلاف ذلك /search?page=1&search={quoted query}&sort_by=lastUpdateDescending. لا pagination. يقبل root JSON كقائمة أو data داخل object. [المصدر](community-20261017/sources/027.py#L11-L45)
- **الاسم والتاريخ:** نتيجة فارغة تطبع نتيجة نصية NO SE ENCONTRARON RESULTADOS بحجم وعدّادات 0. لكل نتيجة يزيل كل محارف الاسم غير ASCII ويبدل | بشرطة؛ في وضع المستجدات يضيف [الرمز] إلى البداية. يقرأ التاريخ من torrent_date ثم last_update؛ يحذف الجزء بعد النقطة ويقرأ ISO محليا دون timezone، لذا يعتمد Unix timestamp على منطقة النظام. أخطاء التاريخ تصبح -1. [المصدر](community-20261017/sources/027.py#L44-L83)
- **المغناطيس والأخطاء:** إن وجد infohash يبني magnet من hash واسم URL-encoded؛ وإلا يضع رابط الموقع ويضيف No torrent للاسم. size وseeds وleech كلها -1، والوصف /game/{slug}. أخطاء parsing لكل عنصر يتم تجاوزها. **النسخة Python تعطل تحقق TLS** باستخدام CERT_NONE؛ حافظ على التحقق الافتراضي في Rust ولا تنقل هذا التجاوز. [تعطيل TLS](community-20261017/sources/027.py#L17-L21) [الحقول](community-20261017/sources/027.py#L84-L111)

### 038 — nekoBT

- **الفئات والطلب:** all فقط. يفك query ثم يرمزه بصيغة query form، ويطلب https://nekobt.to/api/v1/torrents/search?query=... مع User-Agent خاص وAccept: application/json. [المصدر](community-20261017/sources/038.py#L10-L41)
- **JSON:** يقبل قائمة جذرية، أو object فيه data كقائمة، أو data ككائن يأخذ منه results وإلا torrents. غير ذلك/القائمة الفارغة تطبع خطأ نصيا بلا صف نتيجة. الاسم title وإلا auto_title وإلا Desconocido؛ filesize افتراضه 0، وseeders/leechers افتراضهما 0. وقت uploaded_at وإلا created_at بالمللي ثانية ويحوله إلى ثوان Unix، وغير الصالح -1. [المصدر](community-20261017/sources/038.py#L43-L81)
- **الرابط والحجم:** يفضل magnet، وإلا يبني رابط magnet من infohash وdn، وإلا يستخدم /api/v1/torrents/download/{id}. الوصف /torrents/{id}. الحجم يقسم على 1024 ويخرج B/KB/MB/GB/TB/PB بمنزلتين؛ الخطأ 0 MB. العدادات تخرج strings. لا يقرأ cat أو يضيف trackers. أخطاء الطلب/JSON تطبع رسالة معالجة. [الحجم](community-20261017/sources/038.py#L18-L27) [النتيجة والأخطاء](community-20261017/sources/038.py#L83-L109)

### 052 — Snowfl

- **الفئات واستخراج المفتاح:** all فقط. كل بحث يطلب الصفحة الرئيسية. الـregex يعيد السلسلة ابتداء من b.min.js (ولا يحفظ أي prefix قبلها)، ثم يطلب BASE_URL + هذه السلسلة، وبعد ذلك يستخرج أول نص مقتبس بعد findNextItem على أنه API key. فشل النمط يرفع ApiError؛ أخطاء HTTP تمر كما هي. لا تحفظ المفتاح المتغير كإعداد أو في سجل. [الثوابت](community-20261017/sources/052.py#L27-L40) [التهيئة](community-20261017/sources/052.py#L63-L79)
- **query والطلب:** يرفض query بطول <=2، ثم يستخدمه مباشرة في مسار /{key}/{query}{sortPath}{nsfwFlag} من دون ترميز إضافي. sort في search هو MAX_SEED أي /DH5kKsJw/0/SEED/NONE/، وNSFW=0. لا pagination. يجب فهم ترميز qBittorrent قبل URL encoding كي لا تتكرر علامة %. [بناء URL](community-20261017/sources/052.py#L82-L87) [البحث](community-20261017/sources/052.py#L114-L133) [الاستدعاء](community-20261017/sources/052.py#L168-L181)
- **المغناطيس والنتيجة:** كل عنصر بلا magnet يحل عبر /{key}/{site}/{quoted url}; فشل حل رابط لا يسقط العنصر. link=magnet وإلا url، الاسم يضيف [site]، size خام أو -1، العدادات بالمفتاحين المفردين seeder وleecher وإلا -1، desc_link=url. pub_date تقديري من age النسبي؛ month=30 يوما وyear=365، وغير المفهوم -1. now يُحسب مرة للمجموعة. [حل الروابط](community-20261017/sources/052.py#L89-L111) [التاريخ](community-20261017/sources/052.py#L136-L165) [الخرج](community-20261017/sources/052.py#L182-L202)
- **حل التنزيل:** download_torrent يطبع magnet مرتين إذا كان المدخل magnet؛ وإلا يجلب الصفحة ويبحث عن magnet مقتبس وإلا يرفع خطأ. هذا ليس تنزيل محتوى torrent. [المصدر](community-20261017/sources/052.py#L204-L215)

### 071 — TsukiHime

- **الفئات والطلب:** يعلن all=0 وanime=7 لكن cat غير مستخدمة. يطلب https://api.tsukihime.org/v1/search/torrents?q={what} مباشرة دون unquote أو quote، حسب تعليق الملف query escaped مسبقا. لكل نتيجة طلب إضافي /v1/torrents/{id}. لا pagination ولا try/except حول HTTP/JSON. [الفئات](community-20261017/sources/071.py#L40-L45) [الطلب](community-20261017/sources/071.py#L62-L80)
- **النتيجة:** يبني magnet من btih وdn مع URL encoding كامل، ثم يضيف tr لكل tracker مع ترميز عنوان tracker. يجمع seeds وleechers عبر كل trackers. name=name، size={totalsize} B، desc_link=https://tsukihime.org/view/{id}، pub_date=source_date كما جاء، engine_url هو موقع الويب. يحتوي المصدر نص MIT كاملا في رأسه (4–21)؛ download_torrent يستعمل download_file. [الخرج](community-20261017/sources/071.py#L76-L98) [الترخيص والمساعد](community-20261017/sources/071.py#L1-L25) [download_torrent](community-20261017/sources/071.py#L53-L60)

### 076 — YTS

- **الواجهة:** endpoint المحفوظ فيه مسافة بادئة قبل https://movies-api.accel.li/api/v2/list_movies.json?؛ استخدم URL نظيفا في Rust. url الظاهر للنتائج https://yts.bz/، والفئتان المعلنتان all وmovies=1؛ cat غير مستخدمة. [المصدر](community-20261017/sources/076.py#L88-L106)
- **وسوم query:** يفك percent encoding. يستخرج الجودة 2160p/1440p/1080p/720p/480p/240p/3D مع بادئة اختيارية quality=، وترميز x264/h264/x265/h265، وتقييم rating=N أو min_rating=N أو minimum_rating=N، ونوع genre=word. يحذف أيضا &page=N من query. يرسل quality وminimum_rating وgenre وquery_term عبر urlencode. codec يُلصق بـquality على هيئة .x264/.x265 إذا وجدت الجودة، لكنه يحذف codec من query ولا يضيفه إلى parameters إذا جاء منفردا. [البناء](community-20261017/sources/076.py#L115-L165)
- **JSON والصفحات:** response يحمل status/status_message/data؛ data يحمل movie_count/limit/page_number/movies؛ movie وtorrent dataclasses تتجاهل المفاتيح الزائدة. لا ينتج شيئا عند status غير ok أو غياب الأفلام. الطلب الأول بلا page ثم يطلب page=1..floor(movie_count/limit). هذا قد يعيد صفحة 1 وقد لا يغطي الصفحة الأخيرة إذا كان العدد من مضاعفات limit؛ سجّل السلوك قبل أي إصلاح. خطأ JSON يوقف الصفحات التالية. [النماذج](community-20261017/sources/076.py#L27-L86) [الطلب والصفحات](community-20261017/sources/076.py#L167-L194)
- **خلل فلترة محتمل وواضح من الكود:** search_codec وsearch_resolution متغيران محليان ولا يضافان إلى search_params؛ ثم process_movies يفحص keys نفسها داخل search_params، لذلك فلاتر الجودة والترميز لا تنفذ في النسخة الحالية. على Rust مطابقة السلوك الحالي أو تسجيل إصلاح سلوكي منفصل مع اختبار. [استخراج الوسوم](community-20261017/sources/076.py#L120-L147) [الفلترة](community-20261017/sources/076.py#L196-L220)
- **الخرج:** لكل torrent غير مستبعد، name={title_long أو title} ثم quality وvideo_codec وtype وaudio_channels بين أقواس مربعة ثم [YTS]، مع بقاء مسافات الحقول الاختيارية الفارغة. link=torrent.url، size=torrent.size كما هو، العدادات seeds/peers، desc_link=movie.url، pub_date=date_uploaded_unix. إن وصل size_bytes وحده لا يحوله المصدر إلى size. [المصدر](community-20261017/sources/076.py#L196-L220)

### 077 — YGGtracker

- **الاتصال:** يبدأ بجلب nodes.json من GitHub، ثم يطلب كل node على التوالي. الإضافة عامة في الكتالوج لكن الويكي تشترط اتصال Yggdrasil؛ registry على GitHub لا يجعل tracker nodes متاحة عبر الإنترنت العام. [الطلب](community-20261017/sources/077.py#L30-L42)
- **الفئات وquery:** يدعم أسماء الفئات القياسية. يفك query، ويرسل query وfilter=true. يقرأ node.categories[cat] ويرسلها مفصولة بعلامة | إذا كانت غير فارغة. يركب عنوان الطلب من قالب node.url مع urlencode. لا pagination؛ registry فارغ يرجع بلا نتائج، وأخطاء شبكة/API غير ممسوكة محليا. [الفئات](community-20261017/sources/077.py#L11-L25) [المصدر](community-20261017/sources/077.py#L43-L64)
- **النتيجة:** يتوقع response.tracker.url وitem.torrent.file.url/name/size وitem.torrent.scrape.seeders/leechers. الرابط كما أتى (قد يكون ملف torrent وليس magnet)، الحجم bytes، الوصف item.torrent.url، ولا pub_date أو resolver. الرأس يعلن MIT؛ احفظ إشعار الترخيص من upstream عند التوزيع. [الخرج](community-20261017/sources/077.py#L65-L77) [الرأس](community-20261017/sources/077.py#L1-L3)

## حالات مطابقة مفيدة لهذه الدفعة

1. Query UTF-8 و+ و%20 والاستعلام الفارغ؛ مع . و* و! في Gog-games.
2. صفحة EZTV الأخيرة عند أقل من 100 عنصر، وفلترة الموسم/الحلقة، ومسار OMDb بالاعتماد البديل.
3. YTS page=1 والعدد الذي يقبل القسمة على limit، والوسوم quality/codec وحدها أو مجتمعة؛ افصل إصلاح السلوك عن port مطابق.
4. استجابات NekoBT بأشكال الجذر الثلاثة، والمفاتيح الغائبة، وmagnet مقابل infohash مقابل download URL.
5. فشل استخراج token أو حل magnet جزئي في Snowfl؛ لا تسقط كل النتائج عند فشل حل عنصر.
6. TsukiHime بلا trackers وبعدة trackers مع جمع counts؛ registry node غير متاح في YGGtracker؛ الصفحات/العدادات/الروابط الناقصة في dark-libria.

الاختبارات الحية لم تُنفذ. عند بناء fixtures، احفظ الطلب والاستجابة المسجلين ومصدرهما وبصمتهما؛ لا تعتمد على اتصال حي وحده للمقارنة.
