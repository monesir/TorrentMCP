# استعادة مصادر الإضافات العامة التي تعذّر تنزيلها

تاريخ التحقق: 2026-10-17. النطاق مقتصر على المعرفات العامة 039 و057 و072 و073 في `community-20261017/catalog.json`. لم أفحص أو أجلب أي مصدر خاص، ولم أشغّل أي إضافة.

## النتيجة

| المعرف | الإضافة | حالة المصدر | رابط النسخة المثبتة | الترخيص وحالة الصيانة |
|---|---|---|---|---|
| 039 | Nyaa.Pantsu | استعدت النسخة 1.21 من مستودع المؤلف الأصلي، مثبتة على آخر commit للملف | [`pantsu.py`](https://raw.githubusercontent.com/4chenz/pantsu-plugin/8c173a27386827f213adf40fd69414b5b7bb57db/pantsu.py) · SHA-256 `fab225b2231918665fc370737be42118d359c637ee29720d31bd27883bbb0010` | مستودع المؤلف مؤرشف وآخر دفع 2021-06-30. GitHub لا يعلن له ترخيصا. |
| 057 | Sukebei.Pantsu | استعدت النسخة 1.21 من مستودع المؤلف الأصلي، مثبتة على آخر commit للملف | [`sukebei.py`](https://raw.githubusercontent.com/4chenz/pantsu-plugin/42e0dc9dc99103b43fcc98b88b3062f25aa41d25/sukebei.py) · SHA-256 `97d2084383a56b368b5977ddff4088d421efbcd4b2755061a167ce45f53022d3` | المستودع نفسه: مؤرشف، وآخر دفع 2021-06-30، ولا يوجد ترخيص معلن. |
| 072 | UIndex | استعدت النسخة 1.2 بعد أن نقل المؤلف الملف إلى مجلد deprecated | [المصدر المثبت على commit الأرشفة](https://raw.githubusercontent.com/tolotp/qbittorrent-search-plugins-de-busqueda/b70fafa9f6615d9ddb327eaeeaa5594dfcc97c35/Plugins/%28deprecated%29%20uindex.py) · SHA-256 `41ee5886d00dd4cf90a08e98f110653f61f6a6dc909fcc03115cc6e6809aa773` | المستودع يعلن MIT وما زال موجودا؛ الإضافة نفسها مؤرشفة صراحة منذ 2026-06-25. آخر تعديل فعلي للملف 2026-04-10. |
| 073 | UnionDHT | استعدت نسخة 1.2 من أرشيف Wayback لمسار GitHub الأصلي | [نسخة خام مؤرشفة بتاريخ 2026-04-07](https://web.archive.org/web/20260407172042id_/https://raw.githubusercontent.com/msagca/qbittorrent_plugins/main/uniondht.py) · SHA-256 `4b298abdd30844dde360729c718d31be19410207659d7d667a83b2dd43304877` | نسخة مستودع GitHub المؤرشفة في 2026-05-25 تعرض LICENSE من نوع MIT. المستودع الحي غير متاح الآن؛ رابط المصدر القديم في سجل المجتمع يعيد 404. |

## ما الذي يثبت الاستعادة

- صفحة مستودع [4chenz/pantsu-plugin](https://github.com/4chenz/pantsu-plugin) المؤرشفة تسمي نفسها إضافة qBittorrent لموقعي Nyaa وSukebei، وتضع روابط التثبيت إلى الملفين. سجل GitHub يثبت آخر تحديث لكل ملف في 2021-06-30. رابط `libellula/qbt-plugins` الموجود في الفهرس غير متاح، لذلك استخدمت المستودع الأصلي الذي يعرّف نفسه في README كمصدر الإضافتين، لا نسخة عشوائية.
- سجل [UIndex في المستودع الأصلي](https://github.com/tolotp/qbittorrent-search-plugins-de-busqueda/commits/main/Plugins/uindex.py) يعرض تحديث 2026-04-10 ثم commit بعنوان `archive uindex plugin` بتاريخ 2026-06-25. عند commit الأرشفة أصبح المسار `Plugins/(deprecated) uindex.py`، وترويسة الملف تثبت `VERSION: 1.2`. بلاغ [qBittorrent #475](https://github.com/qbittorrent/search-plugins/issues/475) يوثق أن رابط UIndex الموجود في الويكي كان يعيد 404.
- أرشيف [مستودع UnionDHT الأصلي](https://web.archive.org/web/20260525015307id_/https://github.com/msagca/qbittorrent-plugins) يعرض ملفات `LICENSE` و`README.md` و`uniondht.py` ووسم MIT. النسخة الخام المؤرشفة تبدأ بترويسة `VERSION: 1.2` و`AUTHOR: msagca`. فحصت رابطي المستودع المعروفين بالشرطة والشرطة السفلية عبر GitHub API؛ كلاهما غير متاح حاليا. التقط Wayback الملف ذاته في 2024-11-10 و2026-04-07 ببصمة محتوى SHA-256 نفسها، لذا رابط 2026-04-07 هو النسخة الأحدث التي أمكن استردادها.

## قيد الترخيص

استعادة الشيفرة لا تنشئ ترخيصا. مستودع Pantsu لا يعلن ترخيصا في بيانات GitHub، لذا يلزم إبقاء هذا القيد واضحا قبل إعادة توزيع نسخة Rust المشتقة منه. UIndex وUnionDHT يعرضان MIT على مستوى المستودع؛ احتفظ بملف الترخيص ونسب المصدر عند إدخالهما.

هذه نتيجة استعادة وتثبيت مراجع فقط؛ لا تغيّر فهرس المصادر ولا تسجّل أي محرك في التطبيق.
