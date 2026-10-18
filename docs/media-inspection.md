# فحص الوسائط — عقد الإصدار 1

`inspect_media` يقرأ خصائص الحاوية والمسارات بواسطة ffprobe ويعيدها منفصلة عن ادعاءات أسماء الإصدارات. لا يعدل الملف أو يقيس الجودة الإدراكية.

```powershell
.\torrent-search.exe agent --request .\examples\inspect-media.json
```

استبدل مسار المثال بمسار فعلي. للتطوير يوجد الخيار `--ffprobe C:\Tools\ffprobe.exe`؛ الحزمة المحمولة تحتوي runtime مستقلا. لا تحتاج هذه العملية Python أو أصول مصادر البحث.

## الطلب

الحقول: `schema_version: 1`, `operation: inspect_media`, `media: {input, limits?, selection?}`.

- `input: {kind: local_file, path}`: ملف عادي غير فارغ. تمر القراءة عبر بوابة HTTP مؤقتة على 127.0.0.1 ومنفذ عشوائي وتغلق عند النهاية أو الإلغاء.
- `input: {kind: download_service, url, size_bytes}`: رابط HTTP لملف واحد معلوم الحجم على IP loopback مثل 127.0.0.1 أو [::1]. لا تقبل أسماء DNS أو redirects أو بيانات دخول في authority. يسمح برمز وصول في query؛ لا يعاد الرابط في التقرير.
- `limits: {timeout_seconds: 60, max_read_bytes: 16777216}` افتراضيا. الحدود 1..120 ثانية و64 KiB..128 MiB.
- `selection: {audio_languages: [zh], subtitle_languages: [ar, en], video_stream_index?: 0}`: لغات بحروف صغيرة بطول 2 أو 3. تطبع الرموز المعروفة مثل eng/en وchi/zho/zh وara/ar. und/mul/zxx لا تصلح طلب لغة.

عنوان `examples/inspect-download-service.json` توضيحي يحتاج endpoint فعليا. عملية inspect_media نفسها مستهلك نطاقات البايتات؛ لا تحول magnet إلى endpoint تلقائيا. أضيفت عمليتا torrent_files وinspect_torrent_file لتوفير هذا الربط عبر عملية rqbit مستقلة؛ راجع [مسار التورنت](torrent-workflow.md). تتولى خدمة التنزيل تحويل النطاق إلى قطع التورنت والتحقق منها ثم إرجاع البايتات. يمكن استخدام أي خدمة ملفات محلية تطبق العقد دون qBittorrent.

## عقد خدمة التنزيل

```http
GET /files/{file_id}/content HTTP/1.1
Range: bytes=0-262143
Accept-Encoding: identity

HTTP/1.1 206 Partial Content
Content-Range: bytes 0-262143/12369807
Content-Length: 262144

<exact bytes of this file>
```

يجب مطابقة البداية والنهاية والحجم الكلي مع Content-Length ودون Content-Encoding. استجابة 200 أو نطاق خاطئ أو محتوى ناقص تعني فشل المصدر. لا تملأ الخدمة القطع غير المتاحة بأصفار؛ تنتظرها ضمن المهلة أو تفشل. لا تتطلب البوابة HEAD.

كل قراءة لا تتجاوز 256 KiB. `read_bytes_charged` مجموع أطوال النطاقات المحجوزة، بما فيها المتكررة والفاشلة؛ هو حد أعلى لبايتات المحتوى المقروءة. لا يشمل رؤوس HTTP أو تنزيل قطع إضافية يجريه محرك التورنت، ولا يقيس حركة P2P الفعلية. `read_requests` عدد طلبات النطاق. قد يقرأ ffprobe البداية أو النهاية أو مواقع أخرى، ولا نضمن أن 16 MiB تكفي لكل ملف.

سقف stdout هو 2 MiB وstderr هو 128 KiB. يسمح الفاحص بحاويات وسائط محددة وبروتوكولي HTTP/TCP فقط، دون قوائم تشغيل عامة. عند الحد أو الإلغاء يوقف ffprobe والبوابة.

## التقرير

غلاف الاستجابة `schema_version, status, result, error`؛ يحمل result تقرير وسائط:

- `evidence_source: ffprobe_media_headers`, `inspection_scope: bounded_header_probe`.
- `format`: الحاوية والمدة المتاحة والحجم و`overall_bitrate_bps` للملف الإجمالي.
- `streams`: فهرس ونوع كل مسار، الترميز وprofile، الدقة وتنسيق البكسل وعمق المكونات ومعدل الإطارات، بيانات اللون وإشارات HDR المتاحة، قنوات الصوت وتردد العينة واللغة والعنوان، dispositions وtags وside_data.
- `bitrate_bps` مستقل لكل مسار. لا يستبدل بتريت الفيديو المفقود ببتريت الملف الإجمالي.
- `payload_size_bytes` من وسم NUMBER_OF_BYTES إن وجد، وإلا مجهول. لا يفترض أن الحجم الزائد كله صوت.

كل `MediaValue` يحمل `value` و`basis`. المجهول null/unknown. الأسس: ffprobe_stream، ffprobe_format، container_statistics_tag، derived_from_pixel_format، sum_of_container_statistics_tags. هذه معلومات مقروءة أو مشتقة معلنة، وليست قياسا بمسح جميع حزم الملف. بيانات الحاوية ووسوم اللغات قد تكون ناقصة أو خاطئة.

`hdr_signals` يعرض PQ أو HLG أو بيانات Dolby Vision إن ظهرت؛ غيابها لا يثبت SDR. قد توجد بيانات HDR داخل الإطارات لا يكتشفها الفحص المحدود.

## توصية المسارات

قائمة اللغات الفارغة تبقي كل مسارات النوع. غير الفارغة تصنف المطابق في `selected_stream_indices`، واللغة الأخرى في `omitted_stream_indices`، والمجهول في `unclassified_stream_indices`. لا يستمع البرنامج للصوت ولا يفترض أن المسار الافتراضي هو اللغة الأصلية. تبقى كل مسارات اللغة المطابقة مرشحة؛ يمكن للإيجنت فحص title وdispositions لتمييز التعليق أو الوصف الصوتي حين يكون موسوما.

مع أكثر من مسار فيديو يصبح `video_selection_unresolved: true` حتى يحدد الإيجنت فهرسا صالحا. المرفقات وصور الأغلفة تبقى غير محسومة؛ بعض الترجمات تحتاج خطوطا مرفقة. missing_audio_languages وmissing_subtitle_languages يكشفان اللغات المطلوبة التي لم نجدها.

`selected_payload_bytes` و`omitted_payload_bytes` يجمعان وسوم أحجام المسارات عندما تتوفر؛ الأول مجهول إذا بقي اختيار غير محسوم. ليست هذه أحجام MKV النهائية ولا وفرا في تنزيل التورنت. `changes_applied: false` دائما. إزالة المسارات وإنشاء Remux غير منفذين هنا.

## الحالات

completed/0: نجاح، partial/3: تقرير مع تشخيص أو حد قراءة، failed/1: لا تقرير قابل للاستخدام، cancelled/130: إلغاء، error/1: طلب أو تشغيل غير صالح. الرموز: read_budget_exceeded، source_read_failed، probe_timeout، probe_failed، probe_diagnostics، cancelled. نقص لغة أو اختيار مسار غير محسوم لا يعني فشل قراءة الملف؛ افحص selection أيضا.

لا يثبت الفحص سلامة الملف كاملا أو تطابق فيديو نسختين أو جودة الصورة الإدراكية. قد يتطلب البتريت الدقيق وحجم المسار مسح جميع الحزم؛ لا تنفذ هذه العملية ذلك.

المراجع: [ffprobe](https://ffmpeg.org/ffprobe.html)، [Streamcopy](https://ffmpeg.org/ffmpeg.html#Streamcopy)، [نطاقات HTTP](https://www.rfc-editor.org/rfc/rfc9110.html#name-range-requests).
