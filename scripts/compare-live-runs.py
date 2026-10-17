"""Compare two completed live runs without reissuing network requests."""
import argparse
from collections import Counter
import json
from pathlib import Path


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def summarize(directory):
    state = read(directory / "state.json")
    if state["phase"] != "completed":
        raise ValueError(f"Run is not complete: {directory}")
    summary = {"settings": state, "backends": {}}
    identities = None
    for backend in ("rust", "python"):
        report = read(directory / f"{backend}.json")
        sources = {row["source_id"]: row for row in report["sources"]}
        if len(sources) != 86 or report["query"] != state["query"]:
            raise ValueError("Source coverage or query mismatch")
        if identities is not None and set(sources) != identities:
            raise ValueError("Backend source identities differ")
        identities = set(sources)
        matched_sources = set()
        matched_results = set()
        for result in report["results"]:
            for origin in result["observations"]:
                # Jackett's unconfigured Python plugin prints its error as a
                # search result, including the query. It is not a title match.
                if origin["source_id"] == "jackett":
                    continue
                title = "".join(c for c in origin["title"].casefold() if c.isalnum())
                if any(term in title for term in ("nirvanainfire", "langyabang", "琅琊榜")):
                    matched_sources.add(origin["source_id"])
                    ref = result["reference"]
                    matched_results.add(ref.get("info_hash_v1") or ref["uri"])
        summary["backends"][backend] = dict(
            statuses=dict(Counter(row["status"] for row in sources.values())),
            merged_results=len(report["results"]),
            matching_unique=len(matched_results),
            matching_sources=sorted(matched_sources),
            matching_source_count=len(matched_sources),
            source_reports=sources,
        )
    return summary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    args = parser.parse_args()
    before, after = (summarize(path) for path in (args.before, args.after))
    for key in ("query", "timeout", "limit", "parallel", "executable_sha256"):
        if before["settings"][key] != after["settings"][key]:
            raise ValueError(f"Comparison setting changed: {key}")
    comparison = dict(before=str(args.before), after=str(args.after), query=after["settings"]["query"], backends={})
    lines = ["# إعادة الاختبار بعد تشغيل VPN", "",
        "جُربت الإضافات العامة الـ86 في Rust وPython بالبحث نفسه `nirvana in fire`، والفئة all، والمهلة20 ثانية وحد20 نتيجة وتوازٍ6. البصمة التنفيذية والإعدادات مطابقة للتجربة السابقة. تشغيل VPN معلومة من المستخدم؛ لم يغير الاختبار إعداداته أو يتحقق من مسار خروج كل اتصال.", "",
        "الاختباران متعاقبان على مواقع حية؛ التغير المرصود لا يثبت أن VPN وحده سبب كل فرق. النتائج المتعلقة أدناه مبنية على تطابق العنوان مع Nirvana in Fire أو Lang Ya Bang أو 琅琊榜، بعد استبعاد صف تشخيص Jackett. ليست تحققًا من محتوى الملفات، وتشمل النتائج الجزئية.", "",
        "| المقياس | Rust سابقًا | Rust بعد VPN | Python سابقًا | Python بعد VPN |",
        "|---|---:|---:|---:|---:|"]
    metrics = [("اكتملت وفق التقرير", "completed"), ("فشلت", "failed"), ("انتهت المهلة", "timed_out")]
    for label, status in metrics:
        values = [summary["backends"][backend]["statuses"].get(status, 0) for backend in ("rust", "python") for summary in (before, after)]
        lines.append(f"| {label} | " + " | ".join(map(str, values)) + " |")
    for label, key in [("إضافات أعادت عناوين متعلقة", "matching_source_count"), ("نتائج فريدة بعناوين متعلقة", "matching_unique")]:
        values = [summary["backends"][backend][key] for backend in ("rust", "python") for summary in (before, after)]
        lines.append(f"| {label} | " + " | ".join(map(str, values)) + " |")
    for backend in ("rust", "python"):
        old, new = before["backends"][backend], after["backends"][backend]
        if set(old["source_reports"]) != set(new["source_reports"]):
            raise ValueError("Source identity set changed")
        gained = sorted(set(new["matching_sources"]) - set(old["matching_sources"]))
        lost = sorted(set(old["matching_sources"]) - set(new["matching_sources"]))
        comparison["backends"][backend] = dict(before=old, after=new, gained_title_matches=gained, lost_title_matches=lost)
        lines += ["", f"{backend}: إضافات ظهرت لها عناوين متعلقة في الإعادة: {', '.join(gained) or 'لا شيء'}. إضافات غابت عنها العناوين المتعلقة في الإعادة: {', '.join(lost) or 'لا شيء'}."]
    lines += ["", "## حالة إضافات Nyaa", "", "| الإضافة | Rust سابقًا | Rust بعد VPN | Python سابقًا | Python بعد VPN |", "|---|---|---|---|---|"]
    for identifier in ("nyaasi", "sukebeisi", "nyaa", "nyaapantsu", "pantsu", "sukebei"):
        values = []
        for backend in ("rust", "python"):
            for summary in (before, after):
                row = summary["backends"][backend]["source_reports"][identifier]
                values.append(f"{row['status']} / {row['result_count']} نتيجة")
        lines.append(f"| {identifier} | " + " | ".join(values) + " |")
    lines += ["", "Jackett بقي دون إعداد محلي؛ قد تصنف نسخته الأصلية رسالة الخطأ كمكتمل. اكتمال المصدر بصفر نتائج لا يثبت عمل محلله أو عدم وجود إصدارات في الموقع.", "",
        "[جدول المصادر الـ86 بعد VPN](README.md) · [CSV بالأخطاء والأوقات](sources.csv) · [مقارنة JSON الكاملة مع السابق](vpn-comparison.json)", ""]
    (args.after / "vpn-comparison.json").write_text(json.dumps(comparison, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (args.after / "VPN-SUMMARY.ar.md").write_text("\n".join(lines), encoding="utf-8")
    print(json.dumps({backend: {key: value for key, value in after["backends"][backend].items() if key != "source_reports"} for backend in ("rust", "python")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
