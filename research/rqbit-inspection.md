# rqbit كخدمة قراءة مستقلة للفحص

مصادر أولية تمت مراجعتها في 2026-10-17:

- [الإصدار v9.0.1](https://github.com/ikatson/rqbit/releases/tag/v9.0.1)، ملف rqbit.exe من صفحة الإصدار، SHA-256 كما نشرته GitHub وطابقه التنزيل: `2ed683203beca628e0c45f62f99feeef4242cecc3444c8422dfe5b1b6aa38cea`.
- [عقد إضافة التورنت](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/http_api_types.rs): list_only وonly_files وoutput_folder وoverwrite.
- [API](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/api.rs): تفاصيل الملفات، update_only_files، stream، pause وforget الذي يحفظ الملفات.
- [تنفيذ HTTP streaming](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/http_api/handlers/streaming.rs): HTTP 206 وContent-Length وContent-Range، ومتوافق مع عقد الفاحص الحالي.
- [تنفيذ القراءة المسبقة](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/torrent_state/streaming.rs): نافذة 32 MiB لكل stream؛ لذلك قراءة 256 KiB عبر HTTP لا تثبت تنزيل 256 KiB فقط عبر P2P.
- [Windows sparse](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/storage/filesystem/sparse.rs) و[فتح الملفات](https://github.com/ikatson/rqbit/blob/v9.0.1/crates/librqbit/src/storage/filesystem/opened_file.rs): توجد مساعدة FSCTL_SET_SPARSE. أثبت التشغيل أن تعيين طول الملف قد يسبق تطبيقها؛ ملف 27 GB فشل تعيين طوله عند نقص المساحة. أداة القياس المحلية تنشئ placeholders sparse قبل الإضافة، دون تغيير rqbit.
- [الرخصة Apache-2.0](https://github.com/ikatson/rqbit/blob/v9.0.1/LICENSE). لم ينسخ كود rqbit إلى المحرك ولم يشغل أي plugin طرف ثالث جديد. الملف التنفيذي يستدعى منفصلا من cache ولا يضمّن في حزمة المنتج.

رُفض أول أمر جمع التنزيل والتشغيل تلقائيًا برسالة blocked by policy دون تفصيل. نجح مسار تنزيل الملف وحده ثم التحقق من بصمته، وسمح التشغيل اللاحق. النسخة ليست مثبّت نظام ولا تشغّل خدمة دائمة؛ API على loopback فقط، وUPnP وLSD والاستماع لاتصالات TCP الواردة معطلة. محدد التنزيل 4 MiB/s والرفع 1 KiB/s في القياس، ولذلك الزمن ليس معيار أقصى سرعة للعميل.

المحاولات الأولى محفوظة في verification/spiderman-rqbit-inspections.json: فشل ffprobe مبكرًا بسبب rw_timeout ثابت قدره 5 ثوان. أصلح الفاحص ليحترم مهلة الطلب الكلية، مع اختبار رجعي يؤخر أول بايت 6 ثوان. التشغيل الناجح محفوظ منفصلًا في verification/spiderman-rqbit-inspections-retry.json؛ لا تستبدل نتائج الفشل بنجاح وهمي.

ضغطت ملفات placeholders الكبيرة من الجولة الأولى باستخدام NTFS compact لتوفير المساحة مع الاحتفاظ بالمحتوى. لم تحذف ملفات المستخدم أو ملفات التورنت الجزئية. خدمة القياس الناجح توقفت بعد الفحص.
