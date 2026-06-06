-- Derived coverage view for questions such as:
-- - "Which majors have historical admission records in this province?"
-- - "Which provinces have historical admission records for this major?"
--
-- Source of truth: admission_scores.
-- Boundary: this is historical admission coverage derived from score statistics.
-- It is not an official current-year admissions plan.

DROP MATERIALIZED VIEW IF EXISTS admission_major_province_coverage;

CREATE MATERIALIZED VIEW admission_major_province_coverage AS
WITH grouped AS (
    SELECT
        s.province_id,
        p.code AS province_code,
        p.name AS province_name,
        s.major_id,
        m.slug AS major_slug,
        m.name AS major_name,
        m.code AS major_code,
        m.is_normal_major,
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
        count(*)::integer AS record_count,
        min(s.source_label) AS source_label
    FROM admission_scores s
    JOIN provinces p ON p.id = s.province_id
    JOIN majors m ON m.id = s.major_id
    GROUP BY
        s.province_id,
        p.code,
        p.name,
        s.major_id,
        m.slug,
        m.name,
        m.code,
        m.is_normal_major,
        s.subject_type,
        s.batch
),
latest AS (
    SELECT DISTINCT ON (s.province_id, s.major_id, s.subject_type, s.batch)
        s.province_id,
        s.major_id,
        s.subject_type,
        s.batch,
        s.year AS latest_year,
        s.admitted_count AS latest_admitted_count,
        s.min_score AS latest_min_score,
        s.avg_score AS latest_avg_score,
        s.max_score AS latest_max_score,
        s.min_rank AS latest_min_rank
    FROM admission_scores s
    ORDER BY
        s.province_id,
        s.major_id,
        s.subject_type,
        s.batch,
        s.year DESC,
        s.min_score
)
SELECT
    g.province_id,
    g.province_code,
    g.province_name,
    g.major_id,
    g.major_slug,
    g.major_name,
    g.major_code,
    g.is_normal_major,
    g.subject_type,
    g.batch,
    g.years,
    g.year_count,
    g.first_year,
    g.latest_year,
    g.total_admitted_count,
    g.min_recorded_score,
    g.max_recorded_score,
    g.avg_min_score,
    g.record_count,
    g.source_label,
    l.latest_admitted_count,
    l.latest_min_score,
    l.latest_avg_score,
    l.latest_max_score,
    l.latest_min_rank,
    'admission_scores_2021_2025'::text AS source_mode,
    '由录取统计表派生：表示该省该专业在对应年份有录取记录，不等同于当年正式招生计划。'::text AS data_boundary_note,
    now() AS refreshed_at
FROM grouped g
JOIN latest l
    ON l.province_id = g.province_id
    AND l.major_id = g.major_id
    AND l.subject_type = g.subject_type
    AND l.batch = g.batch;

CREATE UNIQUE INDEX admission_major_province_coverage_uidx
    ON admission_major_province_coverage (province_id, major_id, subject_type, batch);

CREATE INDEX admission_major_province_coverage_province_idx
    ON admission_major_province_coverage (province_name, latest_year DESC, major_name);

CREATE INDEX admission_major_province_coverage_major_idx
    ON admission_major_province_coverage (major_name, latest_year DESC, province_name);

CREATE INDEX admission_major_province_coverage_latest_idx
    ON admission_major_province_coverage (latest_year DESC, province_name, major_name);
