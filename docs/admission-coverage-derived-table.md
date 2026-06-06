# 录取统计覆盖关系派生表

## 目的

`admission_major_province_coverage` 是从 `admission_scores` 派生的 Postgres 物化视图，用于快速回答两类问题：

- 某个省份近年有哪些专业/方向有录取记录。
- 某个专业近年在哪些省份有录取记录。

它不是正式招生计划表。语义应表述为“近年/最新一年有录取记录的专业”，不能直接说成“当年一定招生的专业”。

## 粒度

一行表示：

```text
省份 + 专业 + 科类/选科口径 + 批次/统计类型
```

这样可以避免把本科批、艺术类、专升本，以及不同科类口径混在一起。

## 主要字段

- `province_code`, `province_name`
- `major_slug`, `major_name`, `major_code`, `is_normal_major`
- `subject_type`, `batch`
- `years`, `year_count`, `first_year`, `latest_year`
- `total_admitted_count`
- `min_recorded_score`, `max_recorded_score`, `avg_min_score`
- `latest_admitted_count`, `latest_min_score`, `latest_avg_score`, `latest_max_score`, `latest_min_rank`
- `source_mode`
- `data_boundary_note`
- `refreshed_at`

## 查询示例

山东最新一年有录取记录的专业：

```sql
SELECT latest_year, major_name, subject_type, batch, latest_admitted_count, latest_min_score
FROM admission_major_province_coverage
WHERE province_name = '山东'
  AND latest_year = (
    SELECT max(latest_year)
    FROM admission_major_province_coverage
    WHERE province_name = '山东'
  )
ORDER BY batch, major_name;
```

英语（师范类）在哪些省份有录取记录：

```sql
SELECT major_name,
       count(DISTINCT province_name) AS province_count,
       array_agg(DISTINCT province_name ORDER BY province_name) AS provinces
FROM admission_major_province_coverage
WHERE major_name = '英语（师范类）'
GROUP BY major_name;
```

物联网工程最新一年在哪些省份有录取记录：

```sql
SELECT latest_year, province_name, subject_type, batch, latest_admitted_count, latest_min_score
FROM admission_major_province_coverage
WHERE major_name = '物联网工程'
  AND latest_year = (
    SELECT max(latest_year)
    FROM admission_major_province_coverage
    WHERE major_name = '物联网工程'
  )
ORDER BY province_name;
```

## 刷新

创建脚本已经固化在：

- `/home/scm2002/Code/rust_enrollment/crates/importers/sql/admission_major_province_coverage.sql`
- `/home/scm2002/Code/rust_enrollment/crates/importers/src/bin/admission_coverage.rs`

首次创建或重建物化视图：

```bash
cargo run -p importers --bin admission_coverage -- apply
```

`admission_scores` 重导后，需要刷新物化视图：

```sql
REFRESH MATERIALIZED VIEW CONCURRENTLY admission_major_province_coverage;
```

也可以运行：

```bash
cargo run -p importers --bin admission_coverage -- refresh
```

校验视图行数和聚合字段是否与 `admission_scores` 一致：

```bash
cargo run -p importers --bin admission_coverage -- verify
```

如果索引尚未创建，第一次刷新不能使用 `CONCURRENTLY`；先运行 `apply` 创建视图和索引。
