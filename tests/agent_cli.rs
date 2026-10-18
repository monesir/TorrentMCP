use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run(request: &[u8], file: bool) -> (i32, Value) {
    let directory = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_torrentx"));
    // Offline analysis must work without any runtime or source assets.
    command
        .args([
            "--assets",
            "missing-assets",
            "--python",
            "missing-python",
            "--rqbit",
            "missing-rqbit",
            "agent",
        ])
        .current_dir(directory.path())
        .env(
            "TORRENT_SEARCH_DAEMON_FILE",
            directory.path().join("daemon.json"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = if file {
        let path = directory.path().join("request.json");
        std::fs::write(&path, request).unwrap();
        command.arg("--request").arg(path).output().unwrap()
    } else {
        let mut process = command.stdin(Stdio::piped()).spawn().unwrap();
        process.stdin.take().unwrap().write_all(request).unwrap();
        process.wait_with_output().unwrap()
    };
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn request() -> Value {
    json!({"schema_version":1,"operation":"analyze",
        "analysis":{"target":{"title":"Nirvana in Fire","year":2015}},
        "report":{"schema_version":1,"query":"nirvana in fire","category":"all",
            "partial":false,"sources":[],"results":[]}})
}

#[test]
fn accepts_file_stdin_and_windows_utf8_bom_without_assets() {
    for file in [true, false] {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(serde_json::to_vec(&request()).unwrap());
        let (code, response) = run(&bytes, file);
        assert_eq!(code, 0);
        assert_eq!(response["status"], "completed");
        assert_eq!(response["result"]["summary"]["total"], 0);
    }
}

#[test]
fn malformed_unknown_and_invalid_requests_return_machine_readable_errors() {
    let mut unknown = request();
    unknown["analysis"]["target"]["yeer"] = json!(2015);
    let mut version = request();
    version["schema_version"] = json!(2);
    let mut range = request();
    range["analysis"]["target"]["episodes"] = json!({"start":54,"end":1});
    for bytes in [
        b"not-json".to_vec(),
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&version).unwrap(),
        serde_json::to_vec(&range).unwrap(),
    ] {
        let (code, response) = run(&bytes, true);
        assert_eq!(code, 1);
        assert_eq!(response["status"], "error");
        assert_eq!(response["error"]["code"], "invalid_request");
        assert!(response["result"].is_null());
    }
}

#[test]
fn validates_torrent_operations_before_loading_runtimes() {
    for reference in [
        json!({"kind":"magnet", "uri":"magnet:?xt=urn:btih:1111111111111111111111111111111111111111", "info_hash_v1":"2222222222222222222222222222222222222222"}),
        json!({"kind":"http", "uri":"file:///C:/example.torrent", "source_id":"fixture"}),
    ] {
        let request = json!({"schema_version":1,"operation":"torrent_files","reference":reference});
        let (code, response) = run(&serde_json::to_vec(&request).unwrap(), false);
        assert_eq!(code, 1);
        assert_eq!(response["error"]["code"], "invalid_request");
    }
    let request = json!({"schema_version":1,"operation":"torrent_files","reference":{"kind":"magnet","uri":"magnet:?xt=urn:btih:1111111111111111111111111111111111111111","info_hash_v1":"1111111111111111111111111111111111111111"}});
    let (code, response) = run(&serde_json::to_vec(&request).unwrap(), true);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "torrent_service_unavailable");
}

#[test]
fn failed_source_report_keeps_results_and_returns_nonzero() {
    let mut input = request();
    input["report"]["partial"] = json!(true);
    input["report"]["sources"] = json!([{"source_id":"fixture","status":"failed",
        "elapsed_ms":1,"result_count":1,"limited":false,"error":"fixture failure"}]);
    input["report"]["results"] = json!([{
        "title":"Nirvana in Fire 2015 1080p",
        "reference":{"kind":"http","uri":"https://example.org/partial","source_id":"fixture"},
        "observations":[]
    }]);
    let (code, response) = run(&serde_json::to_vec(&input).unwrap(), false);
    assert_eq!(code, 1);
    assert_eq!(response["status"], "failed");
    assert!(response["result"].is_object());
    assert_eq!(response["error"]["code"], "no_sources_completed");
    assert_eq!(response["result"]["summary"]["total"], 1);
    assert_eq!(
        response["result"]["results"][0]["reference"]["uri"],
        "https://example.org/partial"
    );

    input["report"]["sources"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "source_id":"healthy","status":"completed","elapsed_ms":1,
            "result_count":0,"limited":false,"error":null
        }));
    let (code, response) = run(&serde_json::to_vec(&input).unwrap(), false);
    assert_eq!(code, 3);
    assert_eq!(response["status"], "partial");
    assert_eq!(response["result"]["summary"]["total"], 1);
}

#[test]
fn inspect_torrent_validates_its_choices_before_loading_runtimes() {
    let magnet = json!({"kind":"magnet","uri":"magnet:?xt=urn:btih:1111111111111111111111111111111111111111","info_hash_v1":"1111111111111111111111111111111111111111"});
    for extra in [
        json!({}),
        json!({"reference": magnet, "file_ids": [0], "sample_files": 2}),
        json!({"reference": magnet, "sample_files": 0}),
        json!({"reference": magnet, "sample_files": 33}),
        json!({"reference": magnet, "parallel": 9}),
        json!({"reference": magnet, "file_ids": [0, 0]}),
    ] {
        let mut request = json!({"schema_version":1,"operation":"inspect_torrent"});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        let (code, response) = run(&serde_json::to_vec(&request).unwrap(), false);
        assert_eq!(code, 1, "{request}");
        assert_eq!(response["error"]["code"], "invalid_request", "{request}");
    }
    let request = json!({"schema_version":1,"operation":"inspect_torrent","reference":magnet,"sample_files":3});
    let (code, response) = run(&serde_json::to_vec(&request).unwrap(), false);
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "torrent_service_unavailable");
}
