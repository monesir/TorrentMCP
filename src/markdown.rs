//! Human-readable tables derived from the same JSON contract used by agents.
use crate::SearchError;
use serde_json::Value;
use std::fmt::Write;

fn cell(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "&#92;")
        .replace('|', "&#124;")
        .replace('`', "&#96;")
        .replace('*', "&#42;")
        .replace('_', "&#95;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace(['\r', '\n'], " ")
}
fn s(value: &Value) -> String {
    match value {
        Value::Null => "غير معروف".into(),
        Value::Bool(value) => {
            if *value {
                "نعم".into()
            } else {
                "لا".into()
            }
        }
        Value::String(text) => cell(text),
        Value::Array(values) if values.is_empty() => "لا يوجد".into(),
        Value::Array(values) => values.iter().map(s).collect::<Vec<_>>().join("، "),
        _ => cell(&value.to_string()),
    }
}
fn translated(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "completed" => "مكتمل",
        "partial" => "جزئي",
        "failed" => "فشل",
        "cancelled" => "ملغى",
        "error" => "خطأ",
        "eligible" => "مطابق للشروط المعلنة",
        "uncertain" => "غير محسوم",
        "excluded" => "مستبعد",
        "timed_out" => "انتهت المهلة",
        "unsupported" => "غير مدعوم",
        "match" => "مطابق",
        "mismatch" => "مخالف",
        "unknown" => "مجهول",
        "claimed" => "مذكور في العنوان",
        "inferred" => "مستنتج",
        "ambiguous" => "متعدد الاحتمالات",
        "video" => "فيديو",
        "audio" => "صوت",
        "subtitle" => "ترجمة",
        "attachment" => "مرفق",
        "data" => "بيانات",
        _ => return s(value),
    }
    .into()
}
fn field(name: &str) -> &str {
    match name {
        "title" => "العنوان",
        "year" => "السنة",
        "season" => "الموسم",
        "part" => "الجزء",
        "episodes" => "الحلقات",
        "resolution" => "الدقة",
        "video_codec" => "ترميز الفيديو",
        "audio_language" => "لغة الصوت",
        "subtitle_language" => "لغة الترجمة",
        "source_format" => "نوع الإصدار",
        "coverage" => "التغطية",
        "highest_reported_seeders" => "أعلى سيدرز مبلّغ عنه",
        "min_size_bytes" => "الحد الأدنى للحجم",
        "max_size_bytes" => "الحد الأعلى للحجم",
        _ => "شرط إضافي",
    }
}
fn bytes(value: &Value) -> String {
    value
        .as_u64()
        .map(|n| {
            if n >= 1073741824 {
                format!("{:.2} GiB", n as f64 / 1073741824.0)
            } else if n >= 1048576 {
                format!("{:.2} MiB", n as f64 / 1048576.0)
            } else if n >= 1024 {
                format!("{:.2} KiB", n as f64 / 1024.0)
            } else {
                format!("{n} بايت")
            }
        })
        .unwrap_or_else(|| "غير معروف".into())
}
fn bitrate(value: &Value) -> String {
    value
        .as_u64()
        .map(|n| format!("{:.3} Mbps", n as f64 / 1_000_000.0))
        .unwrap_or_else(|| "غير معروف".into())
}
fn basis(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "ffprobe_stream" => "معلومات المسار من ffprobe",
        "ffprobe_format" => "معلومات الحاوية من ffprobe",
        "container_statistics_tag" => "وسم إحصائي في الحاوية",
        "derived_from_pixel_format" => "مشتق من تنسيق البكسل",
        "sum_of_container_statistics_tags" => "مجموع وسوم إحصائية",
        "overall_minus_other_streams" => "تقدير: إجمالي الملف ناقص المسارات الأخرى",
        "unknown" => "غير معروف",
        _ => return s(value),
    }
    .into()
}

fn language(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "en" => "الإنجليزية",
        "ar" => "العربية",
        "zh" => "الصينية",
        "ja" => "اليابانية",
        "ko" => "الكورية",
        "fr" => "الفرنسية",
        "de" => "الألمانية",
        "es" => "الإسبانية",
        "ru" => "الروسية",
        "hi" => "الهندية",
        "th" => "التايلاندية",
        "pt" => "البرتغالية",
        "it" => "الإيطالية",
        "tr" => "التركية",
        _ => return s(value),
    }
    .into()
}

fn limitation(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "full_file_integrity_not_checked" => "لم تفحص سلامة الملف كاملًا.",
        "visual_quality_not_measured" => "لم تقس الجودة المرئية؛ البتريت وحده لا يثبت الأفضلية.",
        "metadata_and_language_tags_may_be_missing_or_incorrect" => {
            "بيانات الحاوية ووسوم اللغات قد تكون ناقصة أو غير صحيحة."
        }
        "track_sizes_and_bitrates_are_not_full_packet_scan_measurements" => {
            "أحجام المسارات والبتريت ليست قياسًا بمسح جميع حزم الملف."
        }
        "selection_does_not_reduce_torrent_download_bytes" => {
            "توصية حذف مسارات لا تقلل تلقائيًا حجم تنزيل التورنت."
        }
        "read_budget_exceeded" => "بلغ الفحص سقف القراءة؛ التقرير جزئي.",
        "probe_diagnostics" => "أبلغ ffprobe تشخيصًا؛ راجع رسالة التشغيل.",
        _ => return s(value),
    }
    .into()
}
fn fact(value: &Value) -> String {
    let claims = value["claims"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .map(|claim| s(&claim["value"]))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_default();
    if claims.is_empty() {
        "غير معروف".into()
    } else {
        format!("{} ({})", claims, translated(&value["state"]))
    }
}
fn table(out: &mut String, headers: &[&str]) {
    let _ = writeln!(out, "\n| {} |", headers.join(" | "));
    let _ = writeln!(out, "| {} |", vec!["---"; headers.len()].join(" | "));
}
fn row(out: &mut String, cells: &[String]) {
    let _ = writeln!(out, "| {} |", cells.join(" | "));
}

/// Accept a versioned agent response or raw search report. Unknown schemas fail explicitly.
pub fn render(report: &Value) -> Result<String, SearchError> {
    if report["schema_version"].as_u64() != Some(1) {
        return Err(SearchError::InvalidRequest(
            "Markdown export supports schema_version 1".into(),
        ));
    }
    let envelope = report.get("status").is_some();
    let data = if envelope { &report["result"] } else { report };
    let mut out = String::from("# تقرير البحث وفحص الوسائط\n");
    if envelope {
        let _ = writeln!(
            out,
            "\nحالة التشغيل: **{}**.\n",
            translated(&report["status"])
        );
        if !report["error"].is_null() {
            table(&mut out, &["رمز التشخيص", "التفاصيل"]);
            row(
                &mut out,
                &[s(&report["error"]["code"]), s(&report["error"]["message"])],
            );
        }
        if let Some(ms) = report["elapsed_ms"].as_u64() {
            let _ = writeln!(
                out,
                "\nزمن الطلب الكلي: **{:.3} ثانية**.\n",
                ms as f64 / 1000.0
            );
        }
    }
    if data.is_null() && envelope {
        return Ok(out);
    }
    if data["search"].is_object() && data["candidates"].is_array() {
        search(&mut out, &data["search"])?;
        find(&mut out, data);
    } else if data["results"].is_array() && data["sources"].is_array() {
        search(&mut out, data)?;
    } else if data["streams"].is_array() && data["format"].is_object() {
        media(&mut out, data)?;
    } else {
        return Err(SearchError::InvalidRequest(
            "Unsupported report shape for Markdown export".into(),
        ));
    }
    Ok(out)
}

fn find(out: &mut String, report: &Value) {
    let _ = writeln!(
        out,
        "\n## المرشحون المفحوصون\n\nملف واحد من كل مرشح، مرتب بتفضيلات الوسائط في الطلب. الأدلة الكاملة في `comparison` داخل JSON.\n"
    );
    table(
        out,
        &[
            "الترتيب",
            "العنوان",
            "الملف",
            "الفحص",
            "الأهلية",
            "الزمن (ثانية)",
            "التشخيص",
        ],
    );
    for candidate in report["candidates"].as_array().into_iter().flatten() {
        row(
            out,
            &[
                s(&candidate["position"]),
                s(&candidate["title"]),
                s(&candidate["file"]),
                translated(&candidate["inspection_status"]),
                translated(&candidate["eligibility"]),
                candidate["elapsed_ms"]
                    .as_u64()
                    .map_or(String::new(), |ms| format!("{:.1}", ms as f64 / 1000.0)),
                s(&candidate["inspection_error"]["code"]),
            ],
        );
    }
}

fn search(out: &mut String, report: &Value) -> Result<(), SearchError> {
    let _ = writeln!(
        out,
        "\n## البحث\n\nالاستعلام: **{}**. التصنيف: {}.\n\nالمواصفات هنا من أسماء الإصدارات والمواقع، ولا تثبت محتوى الفيديو أو الجودة الفعلية. الأعداد والأحجام وقت الرصد فقط.\n",
        s(&report["query"]),
        s(&report["category"])
    );
    if report["summary"].is_object() {
        table(out, &["النتائج", "مطابقة", "غير محسومة", "مستبعدة"]);
        row(
            out,
            &[
                s(&report["summary"]["total"]),
                s(&report["summary"]["eligible"]),
                s(&report["summary"]["uncertain"]),
                s(&report["summary"]["excluded"]),
            ],
        );
    }
    let _ = writeln!(out, "\n### المصادر\n");
    table(
        out,
        &[
            "المصدر",
            "الحالة",
            "الزمن (ثانية)",
            "العدد",
            "بلغ الحد",
            "التشخيص",
        ],
    );
    for source in report["sources"].as_array().unwrap() {
        row(
            out,
            &[
                s(&source["source_id"]),
                translated(&source["status"]),
                format!(
                    "{:.3}",
                    source["elapsed_ms"].as_f64().unwrap_or(0.0) / 1000.0
                ),
                s(&source["result_count"]),
                if source["limited"] == true {
                    "نعم".into()
                } else {
                    "لا".into()
                },
                source["error"]
                    .as_str()
                    .map(cell)
                    .unwrap_or_else(|| "—".into()),
            ],
        );
    }
    let _ = writeln!(
        out,
        "\n### مقارنة النتائج\n\nGiB = 1,073,741,824 بايت. السيدرز أعلى عدد مبلّغ عنه، ولا تجمع الأعداد بين المصادر.\n"
    );
    table(
        out,
        &["#", "الإصدار", "الحجم", "السيدرز", "الأهلية", "نقاط التفضيل"],
    );
    let results = report["results"].as_array().unwrap();
    for (index, item) in results.iter().enumerate() {
        let observations = item["observations"].as_array();
        let sizes: Vec<_> = observations
            .into_iter()
            .flatten()
            .filter_map(|value| value["size_bytes"].as_u64())
            .collect();
        let seeds = observations
            .into_iter()
            .flatten()
            .filter_map(|value| value["seeders"].as_u64())
            .max();
        let size = match (sizes.iter().min(), sizes.iter().max()) {
            (Some(a), Some(b)) if a == b => bytes(&Value::from(*a)),
            (Some(a), Some(b)) => {
                format!("{} – {}", bytes(&Value::from(*a)), bytes(&Value::from(*b)))
            }
            _ => "غير معروف".into(),
        };
        row(
            out,
            &[
                (index + 1).to_string(),
                s(&item["title"]),
                size,
                s(&seeds.map(Value::from).unwrap_or(Value::Null)),
                item.get("eligibility")
                    .map(translated)
                    .unwrap_or_else(|| "لم يحلل".into()),
                s(&item["ranking"]["preference_score"]),
            ],
        );
    }
    let _ = writeln!(out, "\n## تفاصيل النتائج والأدلة\n");
    for (index, item) in results.iter().enumerate() {
        let _ = writeln!(out, "\n### {}. {}\n", index + 1, s(&item["title"]));
        let _ = writeln!(
            out,
            "\nمرجع التورنت ({}): {}\n",
            s(&item["reference"]["kind"]),
            s(&item["reference"]["uri"])
        );
        if item["analysis"].is_object() {
            table(out, &["الخاصية", "القيمة ومصدر الثقة"]);
            for key in [
                "year",
                "season",
                "part",
                "episodes",
                "resolution",
                "video_codec",
                "audio_language",
                "subtitle_language",
                "source_format",
                "coverage",
            ] {
                row(out, &[field(key).into(), fact(&item["analysis"][key])]);
            }
            if let Some(evaluations) = item["evaluations"].as_array() {
                table(out, &["الشرط", "القرار", "السبب"]);
                for evaluation in evaluations {
                    row(
                        out,
                        &[
                            field(evaluation["field"].as_str().unwrap_or("")).into(),
                            translated(&evaluation["state"]),
                            match evaluation["state"].as_str() {
                                Some("match") => "المعلومات المتاحة تطابق الشرط المطلوب".into(),
                                Some("mismatch") => "المعلومات المتاحة تخالف الشرط المطلوب".into(),
                                Some("unknown") => {
                                    "المعلومة المطلوبة مفقودة أو متعددة الاحتمالات".into()
                                }
                                _ => s(&evaluation["reason_code"]),
                            },
                        ],
                    );
                }
            }
        }
        table(out, &["المصدر", "الحجم", "السيدرز", "الرصد (Unix)"]);
        for origin in item["observations"].as_array().into_iter().flatten() {
            row(
                out,
                &[
                    s(&origin["source_id"]),
                    bytes(&origin["size_bytes"]),
                    s(&origin["seeders"]),
                    s(&origin["observed_at_unix"]),
                ],
            );
        }
    }
    Ok(())
}

fn media(out: &mut String, report: &Value) -> Result<(), SearchError> {
    let _ = writeln!(
        out,
        "\n## فحص الوسائط\n\nالبيانات مقروءة من الملف بواسطة ffprobe ضمن سقف قراءة؛ لم تفحص سلامة الملف كاملا أو الجودة الإدراكية. Mbps = مليون بت/ثانية.\n"
    );
    table(out, &["الخاصية", "القيمة"]);
    for (name, value) in [
        ("المصدر", s(&report["input_kind"])),
        ("الحاوية", s(&report["format"]["name"])),
        ("الحجم", bytes(&report["format"]["file_size_bytes"])),
        ("المدة (ثانية)", s(&report["format"]["duration_seconds"])),
        (
            "بتريت الملف الإجمالي",
            bitrate(&report["format"]["overall_bitrate_bps"]["value"]),
        ),
        (
            "زمن الفحص (ثانية)",
            format!(
                "{:.3}",
                report["elapsed_ms"].as_f64().unwrap_or(0.0) / 1000.0
            ),
        ),
        ("بايتات القراءة المحسوبة", s(&report["read_bytes_charged"])),
        ("سقف القراءة (بايت)", s(&report["read_budget_bytes"])),
        ("طلبات النطاق", s(&report["read_requests"])),
    ] {
        row(out, &[name.into(), value]);
    }
    let _ = writeln!(
        out,
        "\nبايتات القراءة المحسوبة تشمل النطاقات المتكررة والفاشلة، وليست حجم بيانات P2P المنزلة. بتريت الملف الإجمالي لا يحل محل بتريت الفيديو.\n"
    );
    let _ = writeln!(out, "\n### مسارات الفيديو\n");
    table(
        out,
        &[
            "الفهرس",
            "الترميز / Profile",
            "الدقة",
            "العمق",
            "الإطارات/ث",
            "إشارات HDR",
            "البتريت",
            "أساس البتريت",
        ],
    );
    for stream in report["streams"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|stream| stream["kind"] == "video")
    {
        row(
            out,
            &[
                s(&stream["index"]),
                format!("{} / {}", s(&stream["codec"]), s(&stream["profile"])),
                format!("{} × {}", s(&stream["width"]), s(&stream["height"])),
                s(&stream["component_bit_depth"]["value"]),
                s(&stream["frame_rate"]),
                if stream["hdr_signals"].as_array().is_some_and(Vec::is_empty) {
                    "غير محدد؛ لا يثبت SDR".into()
                } else {
                    s(&stream["hdr_signals"])
                },
                match &stream["estimated_bitrate_bps"]["value"] {
                    estimate if stream["bitrate_bps"]["value"].is_null() && !estimate.is_null() => {
                        format!("≈ {}", bitrate(estimate))
                    }
                    _ => bitrate(&stream["bitrate_bps"]["value"]),
                },
                if stream["bitrate_bps"]["value"].is_null()
                    && !stream["estimated_bitrate_bps"]["value"].is_null()
                {
                    basis(&stream["estimated_bitrate_bps"]["basis"])
                } else {
                    basis(&stream["bitrate_bps"]["basis"])
                },
            ],
        );
    }
    for (kind, heading) in [("audio", "مسارات الصوت"), ("subtitle", "مسارات الترجمة")]
    {
        let _ = writeln!(out, "\n### {heading}\n");
        let headers: &[&str] = if kind == "audio" {
            &[
                "الفهرس",
                "اللغة",
                "الترميز",
                "القنوات",
                "تردد العينة Hz",
                "البتريت",
                "العنوان",
                "افتراضي / إجباري",
            ]
        } else {
            &["الفهرس", "اللغة", "الصيغة", "العنوان", "افتراضي / إجباري"]
        };
        table(out, headers);
        for stream in report["streams"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|stream| stream["kind"] == kind)
        {
            let mut cells = vec![
                s(&stream["index"]),
                language(&stream["language"]),
                s(&stream["codec"]),
            ];
            if kind == "audio" {
                cells.extend([
                    s(&stream["channels"]),
                    s(&stream["sample_rate_hz"]),
                    bitrate(&stream["bitrate_bps"]["value"]),
                ]);
            }
            cells.extend([
                s(&stream["title"]),
                format!(
                    "{} / {}",
                    s(&stream["dispositions"]["default"]),
                    s(&stream["dispositions"]["forced"])
                ),
            ]);
            row(out, &cells);
        }
    }
    let _ = writeln!(
        out,
        "\n### اختيار المسارات وأحجامها\n\nالاختيار توصية فقط؛ لم يحذف أو يعدل أي مسار. أحجام المسارات من وسوم الحاوية إن وجدت، وليست وفرا في تنزيل التورنت.\n"
    );
    table(
        out,
        &[
            "الفهرس",
            "النوع",
            "القرار",
            "الحجم",
            "أساس الحجم",
            "أساس البتريت",
        ],
    );
    let contains = |key: &str, index: &Value| {
        report["selection"][key]
            .as_array()
            .is_some_and(|values| values.contains(index))
    };
    for stream in report["streams"].as_array().unwrap() {
        let decision = if contains("selected_stream_indices", &stream["index"]) {
            "مختار"
        } else if contains("omitted_stream_indices", &stream["index"]) {
            "زائد عن الطلب"
        } else {
            "غير محسوم"
        };
        row(
            out,
            &[
                s(&stream["index"]),
                translated(&stream["kind"]),
                decision.into(),
                bytes(&stream["payload_size_bytes"]["value"]),
                basis(&stream["payload_size_bytes"]["basis"]),
                basis(&stream["bitrate_bps"]["basis"]),
            ],
        );
    }
    table(out, &["الاختيار", "القيمة"]);
    for (name, key) in [
        ("اختيار الفيديو غير محسوم", "video_selection_unresolved"),
        ("لغات صوت لم نجدها", "missing_audio_languages"),
        ("لغات ترجمة لم نجدها", "missing_subtitle_languages"),
    ] {
        row(out, &[name.into(), s(&report["selection"][key])]);
    }
    let _ = writeln!(out, "\n### حدود الفحص\n");
    for limitation in report["limitations"].as_array().into_iter().flatten() {
        let _ = writeln!(out, "- {}", self::limitation(limitation));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn escapes_untrusted_titles_preserves_unknowns_and_outputs_complete_tables() {
        let report = json!({"schema_version":1,"query":"<script>|test\nline","category":"all","sources":[],"results":[
            {"title":"[click](bad)|<img>\nnext","reference":{"kind":"http","uri":"https://example.org"},"observations":[]}
        ]});
        let output = render(&report).unwrap();
        assert!(!output.contains("<script>"));
        assert!(!output.contains("[click]"));
        assert!(output.contains("&#124;"));
        assert!(output.contains("غير معروف"));
        assert!(output.contains("GiB ="));
        assert!(output.contains("لم يحلل"));
        assert!(output.contains("| --- | --- |"));
        let mut untrusted = report;
        untrusted["results"][0]["analysis"] = json!({});
        untrusted["results"][0]["evaluations"] = json!([{"field":"<img>|bad", "state":"unknown"}]);
        let output = render(&untrusted).unwrap();
        assert!(!output.contains("<img>"));
        assert!(output.contains("شرط إضافي"));
    }
    #[test]
    fn exports_actual_search_and_media_reports_without_mixing_bitrates() {
        let search: Value = serde_json::from_str(
            include_str!("../verification/spiderman-brand-new-day-agent-response.json")
                .trim_start_matches('\u{feff}'),
        )
        .unwrap();
        let output = render(&search).unwrap();
        assert!(output.contains("XEBEC"));
        assert!(output.contains("مذكور في العنوان"));
        let smoke: Value = serde_json::from_str(
            include_str!("../verification/media-0.3.0-portable-smoke.json")
                .trim_start_matches('\u{feff}'),
        )
        .unwrap();
        let output = render(&smoke["local"]).unwrap();
        assert!(output.contains("مسارات الصوت"));
        assert!(output.contains("مسارات الترجمة"));
        assert!(output.contains("0.128 Mbps"));
        assert!(output.contains("غير معروف"));
        assert!(output.contains("لا يحل محل بتريت الفيديو"));
    }
    #[test]
    fn exports_errors_and_rejects_unsupported_shapes_and_versions() {
        assert!(render(&json!({"schema_version":1,"status":"error","result":null,"error":{"code":"invalid_request","message":"bad"}})).unwrap().contains("invalid&#95;request"));
        assert!(render(&json!({"schema_version":2})).is_err());
        assert!(render(&json!({"schema_version":1})).is_err());
    }
}
