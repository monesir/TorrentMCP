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

قبل الفحص تعاد قراءة بيانات التورنت ويجب تطابق الهاش وكامل قائمة الملفات مع الكتالوج. لا يعاد حل رابط البحث في هذه الخطوة. عند إضافة قارئ الملف تتحقق الهوية والقائمة ومجلد الإخراج ثانية. تمنع المسارات المتجاوزة للمجلد والروابط الرمزية والأسماء غير الصالحة على Windows. لا تعدل الأداة اسم ملف المستخدم أو ملفا قائما؛ تنشئ لكل طلب مجلدا جديدا. تنشأ placeholders sparse قبل تحديد أطوالها على Windows، لتجنب حجز الحجم المنطقي الكامل.

`limits.timeout_seconds` من 1 إلى 120، افتراضيا 60، ويحد عمل بيانات التورنت وفتح الملف وفحصه معا. بدء العملية له حد مستقل أقصاه 10 ثوان وإغلاقها 5 ثوان. `inspection_limits` هو حد الفاحص الداخلي كما في عقد الوسائط. إلغاء Ctrl+C يوقف العمل ويغلق الخدمة. العملية مملوكة للطلب و`kill_on_drop` يحمي مسار إسقاط المستقبل داخل العملية.

`result.service` يحمل backend ومجلد العمل و`started`, `stopped`, `shutdown_error`. لا يعلن الإكمال عند عدم تأكيد الإغلاق. الفشل أثناء الفحص يحتفظ بالكتالوج والملف المتحقق منهما، و`media` يكون null إذا لم تتوفر قراءة صالحة. حالات الخروج المعتادة: completed/0، partial/3 عند توفر فحص جزئي، failed/1، cancelled/130. رموز الأخطاء تشمل `torrent_service_unavailable`, `torrent_service_timeout`, `torrent_identity_mismatch`, `torrent_catalog_changed`, `torrent_metadata_invalid` وأخطاء الفاحص الموجودة.

## الحدود

لا توجد إعادة محاولة تلقائية أو قياس لجودة الصورة الإدراكية أو إنشاء remux. تتوفر مقارنة الخصائص المفحوصة عبر compare_media. يبقى حجم المسار مجهولا إن لم تتوفر إحصاءاته في الحاوية؛ لا تمسح الأداة كل الملف لحسابه. قارئ الخدمة قد يجلب قطع P2P أكبر من نطاقات الفاحص؛ حد القراءة لا يحد حركة التورنت نفسها. تطبق عملية rqbit الحالية سقف 4 MiB/s للتنزيل و1 KiB/s للرفع و30 قرينا، كما في مسار التجارب السابق.

تغلق الخدمة عند نهاية كل طلب، وتبقى سجلاتها والملفات الجزئية في مجلد العمل دون حذف تلقائي. إدارة جلسات مستمرة وذاكرة مشتركة وتحسين السرعة مؤجلة. لا تنفذ هذه الواجهة تنزيل فيلم كاملا كعملية مستمرة؛ هي مسار جلب البيانات وقراءة ملف مختار للفحص عبر خدمة مستقلة.
