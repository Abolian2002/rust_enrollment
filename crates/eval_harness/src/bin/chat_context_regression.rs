use anyhow::{Context, Result};
use eval_harness::{HarnessReport, load_fixture, run_case};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let api_base = std::env::var("RUST_HARNESS_API_BASE")
        .unwrap_or_else(|_| "http://127.0.0.1:4000".to_owned());
    let fixtures_dir = std::env::var("RUST_HARNESS_FIXTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("crates/eval_harness/fixtures"));
    let suites = [
        ("v1", "chat-context-regression-cases.json"),
        ("v2", "chat-context-regression-v2-cases.json"),
        ("v3", "chat-context-regression-v3-cases.json"),
        ("v4", "chat-context-regression-v4-cases.json"),
        ("v5", "chat-context-regression-v5-cases.json"),
    ];

    let mut reports = Vec::new();
    for (suite, file_name) in suites {
        let fixture_path = fixtures_dir.join(file_name);
        let cases = load_fixture(&fixture_path).with_context(|| {
            format!("failed to load {suite} fixture {}", fixture_path.display())
        })?;
        for case in cases {
            let mut case_reports = run_case(&api_base, suite, &case).await?;
            for report in &case_reports {
                println!("{}", serde_json::to_string(report)?);
            }
            reports.append(&mut case_reports);
        }
    }

    print_summary(&reports);

    if reports.iter().any(|report| !report.passed) {
        std::process::exit(1);
    }

    Ok(())
}

fn print_summary(reports: &[HarnessReport]) {
    let total = reports.len();
    let passed = reports.iter().filter(|report| report.passed).count();
    eprintln!("summary: passed={passed}/{total}");

    for report in reports.iter().filter(|report| !report.passed).take(30) {
        eprintln!(
            "failed: {} {} turn {} expected={} actual={} type={} reply={} missing={} vector={} model={} tool={} message={}",
            report.suite,
            report.case,
            report.turn,
            report.expected_type,
            report.actual_type,
            report.type_check,
            report.reply_checks,
            report.missing_field_checks,
            report.vector_chunk_checks,
            report.model_call_checks,
            report.tool_call_checks,
            report.message
        );
    }
}
