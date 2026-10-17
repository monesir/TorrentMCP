use serde_json::json;
use std::process::Command;

#[test]
fn exports_saved_search_and_preserves_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.json");
    let output = directory.path().join("report.md");
    std::fs::write(&input,serde_json::to_vec(&json!({"schema_version":1,"query":"Human | test","category":"all","sources":[],"results":[]})).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_torrent-search"))
            .arg("export-markdown")
            .arg("--input")
            .arg(&input)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    let original = std::fs::read_to_string(&output).unwrap();
    assert!(original.contains("Human &#124; test"));
    assert!(!run().status.success());
    assert_eq!(original, std::fs::read_to_string(&output).unwrap());
}

#[test]
fn agent_markdown_and_json_file_output_work_without_search_assets() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("request.json");
    std::fs::write(&input,serde_json::to_vec(&json!({"schema_version":1,"operation":"analyze",
        "analysis":{"target":{"title":"Test"}},"report":{"schema_version":1,"query":"Test","category":"all","partial":false,"sources":[],"results":[]}})).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_torrent-search"))
        .args(["agent", "--format", "markdown", "--request"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let markdown = String::from_utf8(result.stdout).unwrap();
    assert!(markdown.contains("حالة التشغيل: **مكتمل**"));
    assert!(markdown.contains("| النتائج |"));
    let output = directory.path().join("response.json");
    let result = Command::new(env!("CARGO_BIN_EXE_torrent-search"))
        .args(["agent", "--request"])
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(value["status"], "completed");
}
