"""Create a human comparison from saved measured search and torrent inspection reports.

This report sums explicitly separate phase measurements. It does not claim a single
end-to-end stopwatch or infer missing stream bitrate from the total file bitrate.
"""
import argparse
import html
import json
import os
from pathlib import Path


def load(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def cell(value):
    return html.escape(str(value), quote=False).replace("|", "&#124;").replace("\n", " ").replace("\r", " ")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inspections",type=Path,required=True)
    parser.add_argument("--search-timing",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    inspected=load(args.inspections)
    search=load(args.search_timing)
    lines=["# سبايدرمان: مقارنة فحص الملفات والأزمنة", "",
           "العمل: **Spider-Man: Brand New Day (2026)**. رصد 17 أكتوبر 2026 على الجهاز والشبكة الحاليين.", "",
           "فُحصت ثلاثة إصدارات تمثل 4K WEB-DL و4K WEBRip و1080p WEBRip، وليست جميع نتائج البحث الـ125. كل بيانات المسارات أدناه مقروءة عبر أجزاء التورنت نفسها، وليست من اسم الإصدار وحده.", "",
           "## المقارنة", "",
           "الحجم بوحدة GiB؛ البتريت بوحدة Mbps. القيم المأخوذة من وسوم إحصائية في الحاوية ليست مسحًا لجميع حزم الملف.", "",
           "| النسخة | الحجم | الفيديو الفعلي | بتريت الفيديو | الصوت | الترجمة | العربية |",
           "| --- | --- | --- | --- | --- | --- | --- |"]
    short_names={0:"XEBEC — WEB-DL",1:"YTS — WEBRip",2:"NeoNoir — WEBRip"}
    language={"en":"إنجليزي","ar":"عربي"}
    for item in inspected["candidates"]:
        report=item.get("inspection",{}).get("result")
        if not report:
            lines.append(f"| {cell(item['title'])} | — | فشل الفحص | غير معروف | غير معروف | غير معروف | غير معروف |")
            continue
        videos=[stream for stream in report["streams"] if stream["kind"]=="video"]
        audio=[stream for stream in report["streams"] if stream["kind"]=="audio"]
        subtitles=[stream for stream in report["streams"] if stream["kind"]=="subtitle"]
        video=videos[0] if videos else {}
        bitrate=video.get("bitrate_bps",{}).get("value")
        video_text=f"{video.get('width','?')}×{video.get('height','?')} {video.get('codec','?')} / {video.get('component_bit_depth',{}).get('value','?')}bit"
        audio_text=[]
        for stream in audio:
            bps=stream["bitrate_bps"]["value"]
            detail=f"{language.get(stream['language'],stream['language'] or 'لغة غير موسومة')}، {stream['codec']}، {stream['channels']} قنوات"
            if bps is not None: detail+=f"، {bps/1000:g} kbps"
            audio_text.append(detail)
        row=[short_names.get(item["index"],item["title"]),f"{report['format']['file_size_bytes']/2**30:.2f}",video_text,
             f"{bitrate/1e6:.3f}" if bitrate is not None else "غير معروف",
             " / ".join(audio_text),f"{len(subtitles)} مسارًا","موجودة" if any(stream["language"]=="ar" for stream in subtitles) else "لم يظهر مسار موسوم بالعربية"]
        lines.append("| "+" | ".join(cell(value) for value in row)+" |")
    lines += ["", "- XEBEC أظهر PQ وبيانات Dolby Vision في ترويسة الفيديو؛ بقية إشارات HDR بالتفصيل في تقرير كل نسخة.",
              "- في XEBEC، وسم حجم الفيديو نحو 24.51 GiB، والصوت نحو 0.78 GiB، وجميع الترجمات الـ42 نحو 2.32 MiB فقط؛ الحجم الكبير هنا أساسه الفيديو، لا عشرات المسارات الصوتية.",
              "- الصوت الافتراضي في YTS غير موسوم باللغة؛ لا نعتبره إنجليزيًا لمجرد اسم الفيلم. بتريت الفيديو مفقود أيضًا ولم نستبدله بالبتريت الإجمالي.",
              "- وجود عشرات الترجمات لا يعني أنها كبيرة: هذه نصوص SubRip؛ حجمها المتاح من الوسوم مدرج في التفاصيل.",
              "- تطابق الدقة لا يثبت تطابق الفيديو. هذه قراءة خصائص وليست مقارنة جودة مرئية.", "",
              "## الزمن المقاس", "",
              f"البحث بالمصدرين LimeTorrents وTorrents CSV: **{search['wall_ms']/1000:.3f} ثانية** من بدء العملية إلى خروجها؛ زمن الطلب الداخلي {search['request_ms']/1000:.3f} ثانية. وجد 125 نتيجة، وبلغ LimeTorrents سقف 100 نتيجة.", "",
              "| النسخة | حل رابط التفاصيل | بيانات التورنت | إضافة وتهيئة القراءة | فحص المسارات | مجموع المراحل |",
              "| --- | --- | --- | --- | --- | --- |"]
    resolve_total=0
    for item in inspected["candidates"]:
        values=[item.get(key,0) for key in ("resolve_ms","metadata_ms","add_and_initialize_ms","probe_wall_ms")]
        resolve_total+=values[0]
        lines.append("| "+short_names.get(item["index"],str(item["index"]))+" | "+" | ".join(f"{value/1000:.3f} ث" for value in values+[sum(values)])+" |")
    combined=search["wall_ms"]+inspected["total_wall_ms"]+resolve_total
    lines += ["", f"جولة خدمة التنزيل والفحص للنسخ الثلاث بالتتابع استغرقت **{inspected['total_wall_ms']/1000:.3f} ثانية**، بما يشمل بدء الخدمة وإيقافها وطلبات الإدارة. حل الروابط الثلاثة استغرق {resolve_total/1000:.3f} ثانية منفصلًا.", "",
              f"**جمع الأزمنة المقاسة للبحث وحل الروابط وفحص النسخ الثلاث = {combined/1000:.3f} ثانية، أي نحو {combined/60000:.2f} دقيقة.** هذا مجموع مراحل قيس بعضها في تشغيل منفصل، وليس مؤقتًا واحدًا لجولة متصلة. زمن التطوير وتنزيل الأدوات وبناء البرنامج غير داخل القياس.", "",
              "الزمن يختلف مع النظراء وحجم القطع والكاش. هذه تجربة اتصال واحد، وليست متوسطًا إحصائيًا أو ضمانًا. حد الفحص 60 ثانية لكل ملف و16 MiB للقراءة؛ فشل النظراء قد يستنفد المهلة دون تقرير.", "",
              "## كمية القراءة والتنزيل", "",
              "| النسخة | بايتات الفحص المحسوبة | ما جلبه rqbit | ما رُفع |",
              "| --- | --- | --- | --- |"]
    for item in inspected["candidates"]:
        report=item.get("inspection",{}).get("result") or {}
        stats=item.get("stats_after",{})
        snapshot=(stats.get("live") or {}).get("snapshot",{})
        fetched=snapshot.get("fetched_bytes")
        uploaded=snapshot.get("uploaded_bytes",stats.get("uploaded_bytes"))
        lines.append("| "+" | ".join([short_names.get(item["index"],str(item["index"])),
            f"{report.get('read_bytes_charged',0)/2**20:.3f} MiB",
            f"{fetched/2**20:.2f} MiB" if fetched is not None else "غير معروف",
            f"{uploaded/1024:.2f} KiB" if uploaded is not None else "غير معروف"])+" |")
    lines += ["", "الفاحص يقرأ نطاقات محددة، لكن محرك التورنت يجلب قطعًا كاملة وقد يقرأ مسبقًا. لذلك لا تساوي بايتات HTTP تنزيل P2P. حُدد التنزيل بـ4 MiB/s والرفع بـ1 KiB/s في هذه التجربة؛ لا تمثل سرعة العميل القصوى. الملفات على القرص sparse وغير مكتملة؛ لا تستخدمها كأفلام مكتملة.", "",
              "## تفاصيل الملفات والمسارات", ""]
    for item in inspected["candidates"]:
        lines += [f"### {cell(short_names.get(item['index'],str(item['index'])))}", "", cell(item["title"]), "",
                  "| ملف داخل التورنت | الحجم GiB | اختير للفحص |", "| --- | --- | --- |"]
        for i,file in enumerate(item.get("metadata",{}).get("details",{}).get("files",[])):
            lines.append(f"| {cell(file['name'])} | {file['length']/2**30:.6f} | {'نعم' if i==item.get('selected_file_id') else 'لا'} |")
        details=args.output.parent/f"spiderman-inspected-{item['index']}.md"
        if details.exists():
            # The actual tables come from the product's Markdown renderer.
            lines += ["", details.read_text(encoding="utf-8-sig"), ""]
    research_path=os.path.relpath(Path(__file__).resolve().parents[1]/"research/rqbit-inspection.md",args.output.parent.resolve()).replace("\\","/")
    lines += ["## الأدلة", "", "- [نتائج البحث الكاملة](spiderman-search.md)",
              "- [أزمنة البحث](spiderman-search-timing.json)",
              "- [تقارير فحص التورنت الخام وإحصاءاته](spiderman-rqbit-inspections-retry.json)",
              f"- [مراجعة مصدر rqbit ومحددات القياس]({research_path})", "",
              f"أُوقفت خدمة القياس بعد التشغيل: **{'نعم' if inspected['service_stopped'] else 'لا'}**. لم تُحذف ملفات جزئية أو مسارات صوت، ولم ينزل أي ملف فيديو كامل ضمن هذا القياس."]
    with args.output.open("x",encoding="utf-8") as output:
        output.write("\n".join(lines)+"\n")
    print(json.dumps({"report":str(args.output),"combined_phase_ms":combined,"inspection_wall_ms":inspected["total_wall_ms"]}))


if __name__=="__main__": main()
