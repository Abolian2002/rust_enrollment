use anyhow::{Context, Result, bail};
use sqlx::{Executor, PgPool, Row};

const ADMISSION_COVERAGE_SQL: &str = include_str!("../sql/admission_major_province_coverage.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImporterPhase {
    ExcelScores,
    AdmissionCoverage,
    PdfKnowledge,
    FaqVectors,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterPlan {
    pub phase: ImporterPhase,
    pub description: String,
}

pub fn planned_importers() -> Vec<ImporterPlan> {
    vec![
        ImporterPlan {
            phase: ImporterPhase::ExcelScores,
            description: "Migrate 2021-2025 Excel score importer after API/runtime parity.".to_owned(),
        },
        ImporterPlan {
            phase: ImporterPhase::AdmissionCoverage,
            description: "Create and refresh derived province-major admission coverage view from admission_scores.".to_owned(),
        },
        ImporterPlan {
            phase: ImporterPhase::PdfKnowledge,
            description: "Migrate 2025 brochure and training-plan PDF chunk importer with better semantic sections.".to_owned(),
        },
        ImporterPlan {
            phase: ImporterPhase::FaqVectors,
            description: "Migrate FAQ vector indexing with published-only and quality filters.".to_owned(),
        },
    ]
}

pub async fn run_importer(phase: ImporterPhase) -> Result<()> {
    match phase {
        ImporterPhase::AdmissionCoverage => {
            let database_url = std::env::var("DATABASE_URL")
                .context("DATABASE_URL is required for AdmissionCoverage importer")?;
            let pool = PgPool::connect(&database_url)
                .await
                .context("connect to Postgres")?;
            apply_admission_coverage_view(&pool).await?;
            Ok(())
        }
        ImporterPhase::ExcelScores | ImporterPhase::PdfKnowledge | ImporterPhase::FaqVectors => {
            anyhow::bail!(
                "this importer phase is intentionally staged after core Rust agent runtime"
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionCoverageStats {
    pub source_score_rows: i64,
    pub expected_coverage_rows: i64,
    pub coverage_rows: i64,
}

pub async fn apply_admission_coverage_view(pool: &PgPool) -> Result<AdmissionCoverageStats> {
    sqlx::raw_sql(ADMISSION_COVERAGE_SQL)
        .execute(pool)
        .await
        .context("apply admission_major_province_coverage SQL")?;

    verify_admission_coverage_view(pool).await
}

pub async fn refresh_admission_coverage_view(pool: &PgPool) -> Result<AdmissionCoverageStats> {
    pool.execute("REFRESH MATERIALIZED VIEW CONCURRENTLY admission_major_province_coverage")
        .await
        .context("refresh admission_major_province_coverage concurrently")?;

    verify_admission_coverage_view(pool).await
}

pub async fn verify_admission_coverage_view(pool: &PgPool) -> Result<AdmissionCoverageStats> {
    let source_score_rows = scalar_i64(pool, "SELECT count(*) FROM admission_scores").await?;
    let expected_coverage_rows = scalar_i64(
        pool,
        "SELECT count(*) FROM (
            SELECT 1
            FROM admission_scores
            GROUP BY province_id, major_id, subject_type, batch
        ) expected",
    )
    .await?;
    let coverage_rows = scalar_i64(
        pool,
        "SELECT count(*) FROM admission_major_province_coverage",
    )
    .await
    .context("read admission_major_province_coverage row count")?;

    if coverage_rows != expected_coverage_rows {
        bail!(
            "admission_major_province_coverage row count mismatch: expected {expected_coverage_rows}, got {coverage_rows}"
        );
    }

    let missing_or_changed = scalar_i64(
        pool,
        "WITH expected AS (
            SELECT
                s.province_id,
                s.major_id,
                s.subject_type,
                s.batch,
                array_agg(DISTINCT s.year ORDER BY s.year) AS years,
                count(DISTINCT s.year)::integer AS year_count,
                min(s.year) AS first_year,
                max(s.year) AS latest_year,
                sum(COALESCE(s.admitted_count, 0))::integer AS total_admitted_count,
                min(s.min_score) AS min_recorded_score,
                max(s.max_score) AS max_recorded_score,
                round(avg(s.min_score), 2) AS avg_min_score,
                count(*)::integer AS record_count
            FROM admission_scores s
            GROUP BY s.province_id, s.major_id, s.subject_type, s.batch
        )
        SELECT count(*)
        FROM expected e
        LEFT JOIN admission_major_province_coverage c
            ON c.province_id = e.province_id
            AND c.major_id = e.major_id
            AND c.subject_type = e.subject_type
            AND c.batch = e.batch
        WHERE c.province_id IS NULL
            OR c.years IS DISTINCT FROM e.years
            OR c.year_count IS DISTINCT FROM e.year_count
            OR c.first_year IS DISTINCT FROM e.first_year
            OR c.latest_year IS DISTINCT FROM e.latest_year
            OR c.total_admitted_count IS DISTINCT FROM e.total_admitted_count
            OR c.min_recorded_score IS DISTINCT FROM e.min_recorded_score
            OR c.max_recorded_score IS DISTINCT FROM e.max_recorded_score
            OR c.avg_min_score IS DISTINCT FROM e.avg_min_score
            OR c.record_count IS DISTINCT FROM e.record_count",
    )
    .await?;

    if missing_or_changed != 0 {
        bail!(
            "admission_major_province_coverage has {missing_or_changed} missing or changed aggregate rows"
        );
    }

    Ok(AdmissionCoverageStats {
        source_score_rows,
        expected_coverage_rows,
        coverage_rows,
    })
}

async fn scalar_i64(pool: &PgPool, sql: &str) -> Result<i64> {
    let row = sqlx::query(sql)
        .fetch_one(pool)
        .await
        .with_context(|| format!("run scalar query: {sql}"))?;
    row.try_get::<i64, _>(0)
        .with_context(|| format!("read scalar query result: {sql}"))
}
