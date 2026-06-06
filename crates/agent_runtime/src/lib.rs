use anyhow::{Result, anyhow};
use async_trait::async_trait;
use domain::{
    AgentTraceStep, ChatDiagnostics, ChatIntent, ChatStructuredResult,
    ContextCompressionDiagnostics, ConversationMessage, ResolvedMemory,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeNode {
    Router,
    ContextResolution,
    RetrievalPlan,
    ReActToolLoop,
    EvidenceGrading,
    ContextCompression,
    Synthesis,
    MemoryWrite,
}

impl RuntimeNode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Router => "router",
            Self::ContextResolution => "context_resolution",
            Self::RetrievalPlan => "retrieval_plan",
            Self::ReActToolLoop => "react_tool_loop",
            Self::EvidenceGrading => "evidence_grading",
            Self::ContextCompression => "context_compression",
            Self::Synthesis => "synthesis",
            Self::MemoryWrite => "memory_write",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub conversation_id: String,
    pub user_message: String,
    pub memory: ResolvedMemory,
    pub history: Vec<ConversationMessage>,
    pub route_intent: Option<ChatIntent>,
    pub structured_result: Option<ChatStructuredResult>,
    pub draft_reply: Option<String>,
    pub compression: Option<ContextCompressionDiagnostics>,
}

#[derive(Debug, Clone)]
pub struct RuntimeOutput {
    pub context: RuntimeContext,
    pub diagnostics: ChatDiagnostics,
}

#[async_trait]
pub trait AgentNode: Send + Sync {
    fn node(&self) -> RuntimeNode;
    async fn run(&self, context: RuntimeContext) -> Result<RuntimeContext>;
}

#[derive(Default)]
pub struct AgentRuntime {
    nodes: Vec<Box<dyn AgentNode>>,
}

impl AgentRuntime {
    pub fn new(nodes: Vec<Box<dyn AgentNode>>) -> Self {
        Self { nodes }
    }

    pub async fn run(&self, mut context: RuntimeContext) -> Result<RuntimeOutput> {
        let started_at = Instant::now();
        let mut trace = Vec::new();
        let mut tool_call_count = 0;

        for (step, node) in self.nodes.iter().enumerate() {
            let node_started = Instant::now();
            let node_name = node.node().as_str().to_owned();
            match node.run(context).await {
                Ok(next) => {
                    if node.node() == RuntimeNode::ReActToolLoop {
                        tool_call_count += 1;
                    }
                    context = next;
                    trace.push(AgentTraceStep {
                        step,
                        node: node_name,
                        tool_name: None,
                        duration_ms: Some(node_started.elapsed().as_millis()),
                        error: None,
                    });
                }
                Err(error) => {
                    trace.push(AgentTraceStep {
                        step,
                        node: node_name,
                        tool_name: None,
                        duration_ms: Some(node_started.elapsed().as_millis()),
                        error: Some(error.to_string()),
                    });
                    return Err(error);
                }
            }
        }

        let diagnostics = ChatDiagnostics {
            mode: "custom_runtime".to_owned(),
            route_intent: context.route_intent.clone(),
            total_duration_ms: started_at.elapsed().as_millis(),
            model_call_count: 0,
            llm_model: None,
            synthesis_used: false,
            tool_call_count,
            trace,
            compression: context.compression.clone(),
        };

        Ok(RuntimeOutput {
            context,
            diagnostics,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RequiredToolGuard {
    pub intent: ChatIntent,
    pub has_evidence: bool,
}

impl RequiredToolGuard {
    pub fn validate(&self) -> Result<()> {
        let requires_evidence = matches!(
            self.intent,
            ChatIntent::ScoreQuery
                | ChatIntent::ProbabilityAssessment
                | ChatIntent::KnowledgeAnswer
        );
        if requires_evidence && !self.has_evidence {
            return Err(anyhow!(
                "runtime guard blocked factual answer without deterministic tool evidence"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressionLevel {
    None,
    Soft,
    Hard,
}

#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub window_tokens: usize,
    pub soft_threshold_ratio: f64,
    pub hard_threshold_ratio: f64,
    pub soft_recent_messages: usize,
    pub hard_recent_messages: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            window_tokens: 32_000,
            soft_threshold_ratio: 0.72,
            hard_threshold_ratio: 0.85,
            soft_recent_messages: 8,
            hard_recent_messages: 6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressedContext {
    pub messages: Vec<ConversationMessage>,
    pub summary: Option<String>,
    pub diagnostics: ContextCompressionDiagnostics,
}

pub fn estimate_tokens(text: &str) -> usize {
    let ascii_count = text.chars().filter(|ch| ch.is_ascii()).count();
    let non_ascii_count = text.chars().count().saturating_sub(ascii_count);
    ((ascii_count as f64 / 4.0) + (non_ascii_count as f64 / 1.6)).ceil() as usize
}

pub fn compress_context(
    history: &[ConversationMessage],
    current_message: &str,
    memory: &ResolvedMemory,
    config: &CompressionConfig,
) -> CompressedContext {
    let original_tokens = history
        .iter()
        .map(|message| estimate_tokens(&message.content) + 4)
        .sum::<usize>()
        + estimate_tokens(current_message);
    let soft_threshold =
        (config.window_tokens as f64 * config.soft_threshold_ratio).floor() as usize;
    let hard_threshold =
        (config.window_tokens as f64 * config.hard_threshold_ratio).floor() as usize;

    if original_tokens < soft_threshold {
        return CompressedContext {
            messages: history.to_vec(),
            summary: None,
            diagnostics: ContextCompressionDiagnostics {
                applied: false,
                level: "none".to_owned(),
                original_token_estimate: original_tokens,
                compressed_token_estimate: original_tokens,
                threshold_token_estimate: soft_threshold,
                recent_message_count: history.len(),
                summary_token_estimate: 0,
            },
        };
    }

    let level = if original_tokens >= hard_threshold {
        CompressionLevel::Hard
    } else {
        CompressionLevel::Soft
    };
    let recent_count = match level {
        CompressionLevel::Hard => config.hard_recent_messages,
        CompressionLevel::Soft => config.soft_recent_messages,
        CompressionLevel::None => history.len(),
    };
    let older_count = history.len().saturating_sub(recent_count);
    let older = &history[..older_count];
    let recent = history[older_count..].to_vec();
    let summary = build_structured_summary(older, memory, &level);
    let summary_tokens = estimate_tokens(&summary);
    let compressed_tokens = recent
        .iter()
        .map(|message| estimate_tokens(&message.content) + 4)
        .sum::<usize>()
        + summary_tokens
        + estimate_tokens(current_message);

    CompressedContext {
        messages: recent,
        summary: Some(summary),
        diagnostics: ContextCompressionDiagnostics {
            applied: true,
            level: match level {
                CompressionLevel::Soft => "soft",
                CompressionLevel::Hard => "hard",
                CompressionLevel::None => "none",
            }
            .to_owned(),
            original_token_estimate: original_tokens,
            compressed_token_estimate: compressed_tokens,
            threshold_token_estimate: if matches!(level, CompressionLevel::Hard) {
                hard_threshold
            } else {
                soft_threshold
            },
            recent_message_count: recent_count.min(history.len()),
            summary_token_estimate: summary_tokens,
        },
    }
}

fn build_structured_summary(
    older: &[ConversationMessage],
    memory: &ResolvedMemory,
    level: &CompressionLevel,
) -> String {
    let max_items = match level {
        CompressionLevel::Hard => 12,
        CompressionLevel::Soft => 18,
        CompressionLevel::None => 0,
    };
    let items = older
        .iter()
        .rev()
        .take(max_items)
        .map(|message| {
            let compact = message
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "{}:{}",
                message.role,
                compact.chars().take(90).collect::<String>()
            )
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "compression": {
            "version": 1,
            "level": match level {
                CompressionLevel::Hard => "hard",
                CompressionLevel::Soft => "soft",
                CompressionLevel::None => "none",
            },
            "method": "deterministic_structured_summary"
        },
        "confirmedContext": {
            "province": memory.province_name.as_ref().or(memory.province_code.as_ref()),
            "subjectType": memory.subject_type,
            "score": memory.score,
            "rank": memory.rank,
            "major": memory.major_name.as_ref().or(memory.major_slug.as_ref()),
            "pendingIntent": memory.pending_intent
        },
        "conversationSummary": items,
        "activeTask": "优先使用本轮工具结果、结构化短期记忆和最近原文；压缩摘要只用于多轮指代。"
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(index: usize, role: &str, content: String) -> ConversationMessage {
        ConversationMessage {
            role: role.to_owned(),
            content: format!("{index}. {content}"),
            structured_payload: None,
            citations: Vec::new(),
            created_at: None,
        }
    }

    #[test]
    fn guard_blocks_factual_answer_without_evidence() {
        let guard = RequiredToolGuard {
            intent: ChatIntent::KnowledgeAnswer,
            has_evidence: false,
        };
        assert!(guard.validate().is_err());
    }

    #[test]
    fn hard_compression_preserves_recent_messages_and_memory() {
        let history = (0..20)
            .map(|index| {
                message(
                    index,
                    if index % 2 == 0 { "user" } else { "assistant" },
                    "山西480分计算机科学与技术录取概率和培养方案。".repeat(20),
                )
            })
            .collect::<Vec<_>>();
        let memory = ResolvedMemory {
            province_name: Some("山西".to_owned()),
            subject_type: Some("未区分".to_owned()),
            score: Some(480.0),
            major_name: Some("计算机科学与技术".to_owned()),
            ..ResolvedMemory::default()
        };
        let compressed = compress_context(
            &history,
            "继续看主要课程",
            &memory,
            &CompressionConfig {
                window_tokens: 800,
                soft_threshold_ratio: 0.3,
                hard_threshold_ratio: 0.6,
                ..CompressionConfig::default()
            },
        );
        assert!(compressed.diagnostics.applied);
        assert_eq!(compressed.diagnostics.level, "hard");
        assert!(compressed.summary.unwrap().contains("山西"));
        assert_eq!(compressed.messages.len(), 6);
    }
}
