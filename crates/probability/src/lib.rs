use serde::{Deserialize, Serialize};

const DISCLAIMER: &str = "该结果基于历史数据推断，仅供志愿填报参考，不构成官方录取承诺。";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilityLevel {
    Low,
    Medium,
    High,
}

impl ProbabilityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilityConfidence {
    Low,
    Medium,
    High,
}

impl ProbabilityConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilitySourceMode {
    Major,
    UniversityFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityScoreHistoryItem {
    pub year: i32,
    pub min_score: i32,
    pub min_rank: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityPlanHistoryItem {
    pub year: i32,
    pub planned_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityEngineInput {
    pub score: f64,
    pub rank: Option<f64>,
    pub score_history: Vec<ProbabilityScoreHistoryItem>,
    pub plan_history: Vec<ProbabilityPlanHistoryItem>,
    pub source_mode: ProbabilitySourceMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityEngineMetrics {
    pub score_gap_weighted: f64,
    pub score_component: f64,
    pub normalized_rank_advantage: f64,
    pub rank_component: f64,
    pub score_baseline: f64,
    pub latest_score_gap: f64,
    pub trend_slope: f64,
    pub trend_penalty: f64,
    pub plan_change_rate: f64,
    pub plan_adjustment: f64,
    pub score_std_dev: f64,
    pub uncertainty_penalty: f64,
    pub missing_factor: f64,
    pub data_penalty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbabilityEngineOutput {
    pub probability: i32,
    pub level: ProbabilityLevel,
    pub confidence: ProbabilityConfidence,
    pub summary: String,
    pub factors: Vec<String>,
    pub disclaimer: String,
    pub metrics: ProbabilityEngineMetrics,
}

pub fn calculate_admission_probability(input: ProbabilityEngineInput) -> ProbabilityEngineOutput {
    let score_history = latest_five_by_year(input.score_history);
    let plan_history = latest_five_by_year(input.plan_history);
    let rank_history = score_history
        .iter()
        .filter(|item| item.min_rank.is_some())
        .collect::<Vec<_>>();
    let latest_score_record = score_history.last();
    let earliest_score_record = score_history.first();
    let latest_plan_record = plan_history.last();
    let earliest_plan_record = plan_history.first();

    let score_baseline = if score_history.is_empty() {
        input.score
    } else {
        weighted_average_by_recency(&score_history, |item| item.min_score as f64)
    };
    let latest_score_gap = latest_score_record
        .map(|item| input.score - item.min_score as f64)
        .unwrap_or(0.0);
    let score_gap_weighted = input.score - score_baseline;
    let score_component = clamp((score_gap_weighted / 18.0).tanh() * 34.0, -34.0, 34.0);

    let normalized_rank_advantage = match input.rank {
        Some(rank) if !rank_history.is_empty() => {
            weighted_average_by_recency(&rank_history, |item| {
                let min_rank = item.min_rank.unwrap_or(rank as i32) as f64;
                clamp((min_rank - rank) / min_rank.max(1.0), -1.0, 1.0)
            })
        }
        _ => 0.0,
    };
    let rank_component = clamp(
        (normalized_rank_advantage / 0.28).tanh() * 24.0,
        -24.0,
        24.0,
    );

    let trend_slope = if score_history.len() >= 2 {
        match (earliest_score_record, latest_score_record) {
            (Some(earliest), Some(latest)) => {
                (latest.min_score - earliest.min_score) as f64 / (score_history.len() - 1) as f64
            }
            _ => 0.0,
        }
    } else {
        0.0
    };
    let trend_penalty = clamp(trend_slope * 3.5, -7.0, 9.0);

    let plan_change_rate = if plan_history.len() >= 2 {
        match (earliest_plan_record, latest_plan_record) {
            (Some(earliest), Some(latest)) if earliest.planned_count > 0 => {
                (latest.planned_count - earliest.planned_count) as f64
                    / earliest.planned_count as f64
            }
            _ => 0.0,
        }
    } else {
        0.0
    };
    let plan_adjustment = clamp(plan_change_rate * 14.0, -8.0, 8.0);

    let score_std_dev = standard_deviation(
        &score_history
            .iter()
            .map(|item| item.min_score as f64)
            .collect::<Vec<_>>(),
    );
    let near_line_weight = (-score_gap_weighted.abs() / 18.0).exp();
    let uncertainty_penalty = clamp(score_std_dev * 0.7 * near_line_weight, 0.0, 10.0);

    let score_coverage_penalty = (if score_history.is_empty() {
        0.55
    } else if score_history.len() < 2 {
        0.35
    } else {
        0.0
    }) + (usize::saturating_sub(5, score_history.len()) as f64 / 5.0)
        * 0.18;
    let rank_missing_penalty = match input.rank {
        None => 0.14,
        Some(_) if rank_history.is_empty() => 0.16,
        Some(_) => {
            (score_history.len().saturating_sub(rank_history.len()) as f64
                / score_history.len().max(1) as f64)
                * 0.07
        }
    };
    let plan_missing_penalty = if plan_history.len() < 2 {
        0.1
    } else {
        (score_history.len().saturating_sub(plan_history.len()) as f64 / 5.0) * 0.06
    };
    let fallback_penalty = if matches!(input.source_mode, ProbabilitySourceMode::UniversityFallback)
    {
        0.22
    } else {
        0.0
    };
    let missing_factor = clamp(
        score_coverage_penalty + rank_missing_penalty + plan_missing_penalty + fallback_penalty,
        0.0,
        1.0,
    );
    let data_penalty = missing_factor * 12.0;

    let probability_score = 50.0 + score_component + rank_component + plan_adjustment
        - trend_penalty
        - uncertainty_penalty
        - data_penalty;
    let probability = clamp(probability_score, 5.0, 95.0).round() as i32;
    let level = resolve_level(probability);
    let confidence = resolve_confidence(score_history.len(), missing_factor);

    let mut factors = Vec::new();
    if score_history.is_empty() {
        factors.push("当前没有可用的历年分数数据，结果主要依赖降级规则。".to_owned());
    } else {
        factors.push(format!(
            "近 {} 年最低分数加权后，你{}参考线 {} 分。",
            score_history.len(),
            if score_gap_weighted >= 0.0 {
                "高出"
            } else {
                "低于"
            },
            round2(score_gap_weighted.abs())
        ));
    }

    match input.rank {
        None => factors.push("未提供位次，位次因素已降权处理。".to_owned()),
        Some(_) if !rank_history.is_empty() => factors.push(format!(
            "按可用位次数据估算，你的位次整体{}历史最低录取位次。",
            if normalized_rank_advantage >= 0.0 {
                "优于"
            } else {
                "落后于"
            }
        )),
        Some(_) => factors.push("当前缺少可用位次数据，位次因素已降权处理。".to_owned()),
    }

    if plan_history.len() >= 2 {
        factors.push(format!(
            "该专业近 {} 年计划数{} {}%。",
            plan_history.len(),
            if plan_change_rate >= 0.0 {
                "增长"
            } else {
                "缩减"
            },
            round2((plan_change_rate * 100.0).abs())
        ));
    } else {
        factors.push("计划数据不足，计划因素影响已降权处理。".to_owned());
    }

    if trend_slope > 0.5 {
        factors.push(format!(
            "近年录取线呈上升趋势，年均抬升约 {} 分。",
            round2(trend_slope)
        ));
    } else if trend_slope < -0.5 {
        factors.push(format!(
            "近年录取线呈下降趋势，年均下降约 {} 分。",
            round2(trend_slope.abs())
        ));
    } else {
        factors.push("近年录取线整体较为平稳。".to_owned());
    }

    if score_std_dev >= 4.0 {
        factors.push(format!(
            "历年分数波动较大，标准差约 {} 分，临近录取线时风险会更高。",
            round2(score_std_dev)
        ));
    }
    if matches!(input.source_mode, ProbabilitySourceMode::UniversityFallback) {
        factors.push("目标专业历史数据不足，已回退到院校级历史线估算。".to_owned());
    }
    if matches!(confidence, ProbabilityConfidence::Low) {
        factors.push("当前数据完整度较低，建议结合更多官方数据综合判断。".to_owned());
    }

    ProbabilityEngineOutput {
        probability,
        level,
        confidence,
        summary: build_summary(level, input.source_mode),
        factors,
        disclaimer: DISCLAIMER.to_owned(),
        metrics: ProbabilityEngineMetrics {
            score_gap_weighted: round2(score_gap_weighted),
            score_component: round2(score_component),
            normalized_rank_advantage: round2(normalized_rank_advantage),
            rank_component: round2(rank_component),
            score_baseline: round2(score_baseline),
            latest_score_gap: round2(latest_score_gap),
            trend_slope: round2(trend_slope),
            trend_penalty: round2(trend_penalty),
            plan_change_rate: round2(plan_change_rate),
            plan_adjustment: round2(plan_adjustment),
            score_std_dev: round2(score_std_dev),
            uncertainty_penalty: round2(uncertainty_penalty),
            missing_factor: round2(missing_factor),
            data_penalty: round2(data_penalty),
        },
    }
}

fn latest_five_by_year<T: Clone + HasYear>(items: Vec<T>) -> Vec<T> {
    let mut items = items;
    items.sort_by_key(|item| -item.year());
    let mut seen = Vec::new();
    let mut results = Vec::new();
    for item in items {
        if seen.contains(&item.year()) {
            continue;
        }
        seen.push(item.year());
        results.push(item);
        if results.len() == 5 {
            break;
        }
    }
    results.sort_by_key(|item| item.year());
    results
}

trait HasYear {
    fn year(&self) -> i32;
}

impl HasYear for ProbabilityScoreHistoryItem {
    fn year(&self) -> i32 {
        self.year
    }
}

impl HasYear for ProbabilityPlanHistoryItem {
    fn year(&self) -> i32 {
        self.year
    }
}

fn weighted_average_by_recency<T>(items: &[T], get_value: impl Fn(&T) -> f64) -> f64 {
    let mut sum = 0.0;
    let mut weight_sum = 0.0;
    for (index, item) in items.iter().enumerate() {
        let weight = index as f64 + 1.0;
        sum += get_value(item) * weight;
        weight_sum += weight;
    }
    if weight_sum == 0.0 {
        0.0
    } else {
        sum / weight_sum
    }
}

fn standard_deviation(values: &[f64]) -> f64 {
    if values.len() <= 1 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn resolve_level(probability: i32) -> ProbabilityLevel {
    if probability >= 72 {
        ProbabilityLevel::High
    } else if probability >= 42 {
        ProbabilityLevel::Medium
    } else {
        ProbabilityLevel::Low
    }
}

fn resolve_confidence(years_used: usize, missing_factor: f64) -> ProbabilityConfidence {
    if years_used < 2 || missing_factor >= 0.45 {
        ProbabilityConfidence::Low
    } else if years_used >= 4 && missing_factor <= 0.22 {
        ProbabilityConfidence::High
    } else {
        ProbabilityConfidence::Medium
    }
}

fn build_summary(level: ProbabilityLevel, source_mode: ProbabilitySourceMode) -> String {
    let source_note = if matches!(source_mode, ProbabilitySourceMode::UniversityFallback) {
        "当前已回退到院校级历史线估算，"
    } else {
        ""
    };
    match level {
        ProbabilityLevel::High => {
            format!("{source_note}你的分数与位次整体优于近年录取基线，录取概率较高。")
        }
        ProbabilityLevel::Medium => {
            format!("{source_note}你的分数与位次接近近年录取基线，录取概率中等。")
        }
        ProbabilityLevel::Low => {
            format!("{source_note}你的分数或位次低于近年录取基线，录取概率较低。")
        }
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.min(max).max(min)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_score_gets_strong_probability() {
        let output = calculate_admission_probability(ProbabilityEngineInput {
            score: 620.0,
            rank: None,
            score_history: vec![
                ProbabilityScoreHistoryItem {
                    year: 2023,
                    min_score: 540,
                    min_rank: None,
                },
                ProbabilityScoreHistoryItem {
                    year: 2024,
                    min_score: 545,
                    min_rank: None,
                },
                ProbabilityScoreHistoryItem {
                    year: 2025,
                    min_score: 550,
                    min_rank: None,
                },
            ],
            plan_history: Vec::new(),
            source_mode: ProbabilitySourceMode::Major,
        });
        assert!(output.probability >= 65);
    }

    #[test]
    fn low_score_gets_low_probability() {
        let output = calculate_admission_probability(ProbabilityEngineInput {
            score: 500.0,
            rank: None,
            score_history: vec![
                ProbabilityScoreHistoryItem {
                    year: 2023,
                    min_score: 540,
                    min_rank: None,
                },
                ProbabilityScoreHistoryItem {
                    year: 2024,
                    min_score: 545,
                    min_rank: None,
                },
                ProbabilityScoreHistoryItem {
                    year: 2025,
                    min_score: 550,
                    min_rank: None,
                },
            ],
            plan_history: Vec::new(),
            source_mode: ProbabilitySourceMode::Major,
        });
        assert!(output.probability < 42);
        assert_eq!(output.level, ProbabilityLevel::Low);
    }
}
