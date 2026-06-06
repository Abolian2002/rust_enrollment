use anyhow::{Context, Result, bail};
use importers::{
    apply_admission_coverage_view, refresh_admission_coverage_view, verify_admission_coverage_view,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "verify".to_owned());
    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL is required; set it in .env or the environment")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .context("connect to Postgres")?;

    match command.as_str() {
        "apply" => {
            let stats = apply_admission_coverage_view(&pool).await?;
            println!(
                "applied admission_major_province_coverage: {} rows, {} source score rows",
                stats.coverage_rows, stats.source_score_rows
            );
        }
        "refresh" => {
            let stats = refresh_admission_coverage_view(&pool).await?;
            println!(
                "refreshed admission_major_province_coverage: {} rows, {} source score rows",
                stats.coverage_rows, stats.source_score_rows
            );
        }
        "verify" => {
            let stats = verify_admission_coverage_view(&pool).await?;
            println!(
                "verified admission_major_province_coverage: {} rows, expected {} rows, {} source score rows",
                stats.coverage_rows, stats.expected_coverage_rows, stats.source_score_rows
            );
        }
        "help" | "--help" | "-h" => {
            print_usage();
        }
        other => {
            print_usage();
            bail!("unknown command: {other}");
        }
    }

    Ok(())
}

fn print_usage() {
    eprintln!("Usage: cargo run -p importers --bin admission_coverage -- <apply|refresh|verify>");
}
