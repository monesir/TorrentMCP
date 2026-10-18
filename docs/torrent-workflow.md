# البحث ثم اختيار ملف تورنت وفحصه

واجهة JSON محلية للإيجنت. يبقى البحث في محرك البحث، وتتولى عملية rqbit المستقلة جلب بيانات التورنت وقطع الملفات. لا يلزم تثبيت qBittorrent، ولا يفتح API على الشبكة العامة. تحتوي حزمة Windows على rqbit 9.0.1 المثبت ببصمته.

## التسلسل

يمكن إضافة `analysis` إلى طلب `inspect_torrent_file` لإرجاع `result.assessment` بجانب المسارات. ولمقارنة عدة فحوص محفوظة استخدم `compare_media`؛ التفاصيل في [عقد مقارنة الوسائط](media-comparison.md). لا يتغير الطلب السابق عند غياب analysis.

1. نفذ `search` واختر `reference` من إحدى النتائج.
2. أرسل `operation: torrent_files` مع المرجع. تعيد `result.torrent` هوية التورنت وقائمة `files`، لكل ملف `id`, `path`, `size_bytes`, `media_kind`. هذه خطوة بيانات تورنت فقط؛ لا تضيف تنزيل ملف وسائط.
3. اختَر `file_id` صراحة، بناء على قائمة الملفات؛ لا تختار الأداة أكبر ملف تلقائيا.
4. أرسل `operation: inspect_torrent_file` مع كائن `torrent` المعاد والفهرس المطلوب. يعيد `result.torrent`, `result.file`, `result.media` في استجابة واحدة. يحتوي `media.streams` على المسارات ولغاتها وترميزها وبتريتها وأحجامها المتاحة ومصدر كل معلومة.

```powershell
$reference = (Get-Content -LiteralPath search.json -Raw | ConvertFrom-Json).result.results[0].reference
@{schema_version=1; operation='torrent_files'; reference=$reference} |
    ConvertTo-Json -Depth 30 | .\torrent-search.exe agent --output files.json
$catalog = (Get-Content -LiteralPath files.json -Raw | ConvertFrom-Json).result.torrent
$catalog.files | Format-Table id,path,size_bytes,media_kind
# يختار الإيجنت الفهرس المناسب من القائمة؛ 0 مثال فقط.
@{schema_version=1; operation='inspect_torrent_file'; torrent=$catalog; file_id=0;
  limits=@{timeout_seconds=60}; inspection_limits=@{timeout_seconds=45; max_read_bytes=16777216};
  selection=@{audio_languages=@('en'); subtitle_languages=@('ar','en')}} |
    ConvertTo-Json -Depth 30 | .\torrent-search.exe agent --output inspection.json
```

التصدير لهذه العمليات هو JSON. مرجع `examples/torrent-files.json` توضيحي؛ استبدله بنتيجة فعلية. للتطوير يمكن تمرير `--rqbit C:\Tools\rqbit.exe --ffprobe C:\Tools\ffprobe.exe --torrent-cache C:\Temp\torrent-inspections`. المسارات الافتراضية بجانب التنفيذي في `runtime/torrent` و`runtime/media`، ومجلد العمل الافتراضي `%LOCALAPPDATA%/torrent-search/inspections` على Windows.

## الهوية والعقود

`torrent_files.resolve_http` افتراضيا true: يحل صفحات التفاصيل بواسطة موصل المصدر. للمرجع الذي تعرف أنه رابط بيانات torrent مباشر، مرر false لتجاوز الحل وقراءة الرابط مباشرة دون تحميل مصادر البحث. مراجع POST المحلولة وmagnet لا تحتاج مصادر البحث.

يحفظ الكتالوج `source_reference` الأصلي و`resolved_reference` بعد حل صفحة التفاصيل و`info_hash_v1`. تستعمل مصادر البحث المسجلة للحل عندما تدعم ذلك. مرجع HTTP المباشر يجلب بيانات torrent عبر GET، أو POST مع `post_form` إن كان موجودا. لا ترسل حقول POST إلى صفحة حل أخرى ولا تسقطها. بيانات HTTP محدودة بـ10 MiB، واستجابة الخدمة بـ8 MiB، وعدد الملفات بـ10000. يدعم موصل rqbit الحالي هوية v1؛ لا يقبل magnet من نوع v2 فقط.

قبل الفحص تقارن بيانات التورنت بالكتالوج، ويجب تطابق الهاش وكامل قائمة الملفات. تقرأ هذه البيانات من ذاكرة metadata المحلية إن وجدت، دون سؤال الشبكة مجددا. لا يعاد حل رابط البحث في هذه الخطوة. عند إضافة قارئ الملف تتحقق الهوية والقائمة ومجلد الإخراج ثانية. تمنع المسارات المتجاوزة للمجلد والروابط الرمزية والأسماء غير الصالحة على Windows. لا تعدل الأداة اسم ملف المستخدم أو ملفا قائما؛ تنشئ لكل جلسة مجلدا جديدا. تنشأ placeholders sparse قبل تحديد أطوالها على Windows. هذا شرط وليس تحسينا: القراءة من إزاحة بعيدة في ملف غير sparse تجعل النظام يملأ ما قبلها بالأصفار، وقد ملأت قراءة عند 20 GB القرص في تجربة.

`limits.timeout_seconds` من 1 إلى 120، افتراضيا 60، ويحد عمل بيانات التورنت وفتح الملف وفحصه معا. بدء العملية له حد مستقل أقصاه 10 ثوان وإغلاقها 5 ثوان. `inspection_limits` هو حد الفاحص الداخلي كما في عقد الوسائط. إلغاء Ctrl+C يوقف العمل ويغلق الخدمة. العملية مملوكة للطلب و`kill_on_drop` يحمي مسار إسقاط المستقبل داخل العملية.

`result.service` يحمل backend ومجلد العمل و`started`, `stopped`, `shutdown_error`. لا يعلن الإكمال عند عدم تأكيد الإغلاق. الفشل أثناء الفحص يحتفظ بالكتالوج والملف المتحقق منهما، و`media` يكون null إذا لم تتوفر قراءة صالحة. حالات الخروج المعتادة: completed/0، partial/3 عند توفر فحص جزئي، failed/1، cancelled/130. رموز الأخطاء تشمل `torrent_service_unavailable`, `torrent_service_timeout`, `torrent_identity_mismatch`, `torrent_catalog_changed`, `torrent_metadata_invalid` وأخطاء الفاحص الموجودة.

## الحدود

لا توجد إعادة محاولة تلقائية أو قياس لجودة الصورة الإدراكية أو إنشاء remux. تتوفر مقارنة الخصائص المفحوصة عبر compare_media. يبقى حجم المسار مجهولا إن لم تتوفر إحصاءاته في الحاوية؛ لا تمسح الأداة كل الملف لحسابه. قارئ الخدمة قد يجلب قطع P2P أكبر من نطاقات الفاحص؛ حد القراءة لا يحد حركة التورنت نفسها. لا يوجد سقف للتنزيل افتراضيا (`TorrentRuntime.download_limit_bytes_per_second`)، لأن الملف المختار لا يبقى محددا إلا مدة فحصه. الرفع محدود بـ1 KiB/s والأقران بـ30.

## الجلسة والذاكرة

`RqbitService` جلسة مشتركة: عملية rqbit واحدة تخدم كل العمليات حتى `shutdown`، ويمكن استدعاؤها بالتوازي. `perform` ينفذ عملية دون إغلاق الجلسة. أما `run`/`run_with_service` فيغلقانها بعد عملية واحدة، وهذا ما يفعله سطر الأوامر الآن. خادم دائم مستقبلا يحتفظ بالجلسة.

- **metadata**: يحول magnet إلى بايتات .torrent مرة واحدة عبر `resolve_magnet`، وتحفظ في `<cache>/metadata/<hash>.torrent` بحد 2000 ملف. ما بعد ذلك يضاف من البايتات فورا دون DHT. في القياس: أول مرة 1.5 ثانية، ومن الذاكرة 0.38 ثانية مع تشغيل rqbit.
- **الملفات**: يضاف التورنت إلى rqbit مرة واحدة ويبقى ما دام فيه ملف مفتوح. كل `open_file` يقابله `close_file`، وإغلاق آخر ملف يحذف التورنت وبياناته الجزئية. يبقى الملف المقروء ضمن `only_files` طوال القراءة: إذا لم يبق شيء للتنزيل يعلّم rqbit كل الأقران "غير مطلوبين"، فتنتظر قراءة القطع الناقصة إلى الأبد. هذا كان عيبا في التصميم السابق.
- **التنظيف**: مجلد الجلسة `<cache>/sessions/<pid>-<stamp>` يحذف عند `shutdown`. تمسك الجلسة الحية `session.lock` مفتوحا دون مشاركة حذف، فتحذف الجلسة التالية مجلدات الجلسات الميتة ومجلدات `request-*` القديمة. كانت الأخيرة تتراكم بلا حذف (147 MB).
- **التوازي**: `TorrentOperation::InspectFiles` و`inspect_files` يفحصان عدة ملفات عبر الجلسة نفسها، بحد `MAX_PARALLEL_INSPECTIONS` = 8، مع الحفاظ على ترتيب الطلب. تحمل كل نتيجة `media` أو `failure` و`elapsed_ms`. في القياس استغرق فحص 3 حلقات معا 22–30 ثانية، مقابل 43–67 ثانية متتالية. لم تربط هذه العملية بواجهة الإيجنت بعد.

اختبار حي: `cargo test --test rqbit_live -- --ignored --nocapture` مع `TORRENT_SEARCH_LIVE_MAGNET` و`TORRENT_SEARCH_LIVE_RUNTIME`. لا تنفذ هذه الواجهة تنزيل فيلم كاملا كعملية مستمرة؛ هي مسار جلب البيانات وقراءة ملف مختار للفحص عبر خدمة مستقلة.
