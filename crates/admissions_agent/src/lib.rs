use agent_runtime::{
    CompressionConfig, RequiredToolGuard, RuntimeContext, RuntimeOutput, compress_context,
};
use anyhow::{Context, Result};
use db::{Database, memory_from_profile};
use domain::{
    ChatCitation, ChatIntent, ChatReply, ChatRequest, ChatStructuredResult, ConversationMessage,
    ResolvedMemory,
};
use llm::{LlmMessage, LlmProvider, MessageRole, OpenAiCompatibleClient};
use probability::{
    ProbabilityEngineInput, ProbabilityPlanHistoryItem, ProbabilityScoreHistoryItem,
    ProbabilitySourceMode, calculate_admission_probability,
};
use retrieval::{
    RetrievalIntent, RetrievalService, extract_college_major_catalog_query,
    render_college_major_answer, render_knowledge_answer, render_major_disambiguation,
    route_message,
};
use serde_json::json;
use std::collections::HashSet;
use std::time::Instant;

#[derive(Clone)]
pub struct AdmissionsAgent {
    db: Database,
    retrieval: RetrievalService,
    compression_config: CompressionConfig,
    llm: Option<OpenAiCompatibleClient>,
}

impl AdmissionsAgent {
    pub fn new(db: Database) -> Self {
        let retrieval = RetrievalService::new(db.clone());
        Self {
            db,
            retrieval,
            compression_config: CompressionConfig::default(),
            llm: OpenAiCompatibleClient::from_env_for_synthesis(),
        }
    }

    pub async fn chat(&self, input: ChatRequest) -> Result<ChatReply> {
        let started_at = Instant::now();
        let conversation_id = self
            .db
            .get_or_create_conversation(input.conversation_id.as_deref())
            .await
            .context("failed to get or create conversation")?;
        let history = self
            .db
            .get_conversation_history(&conversation_id)
            .await?
            .map(|history| history.messages)
            .unwrap_or_default();
        let mut memory = memory_from_profile(input.profile.as_ref());
        enrich_memory_from_history(&mut memory, &history);
        enrich_memory_from_message(&mut memory, &input.message);
        self.resolve_major_from_message(&mut memory, &input.message)
            .await?;
        let last_assistant_structured = latest_assistant_structured(&history);

        self.db
            .append_message(&conversation_id, "user", &input.message, None, &[])
            .await?;

        let route = apply_contextual_route(route_message(&input.message), &input.message, &memory);
        let route_intent = to_chat_intent(&route.intent);
        let compressed =
            compress_context(&history, &input.message, &memory, &self.compression_config);
        let mut trace = vec![domain::AgentTraceStep {
            step: 0,
            node: "router".to_owned(),
            tool_name: None,
            duration_ms: None,
            error: None,
        }];

        let combined_plan = combined_request_plan(
            &input.message,
            &memory,
            &route.intent,
            last_assistant_structured,
        );
        let (reply, structured_result, citations, tool_call_count) = if let Some(plan) =
            combined_plan
        {
            self.handle_combined_request(&input.message, &memory, plan)
                .await?
        } else {
            match route.intent {
                RetrievalIntent::Greeting => {
                    let structured = ChatStructuredResult::Greeting {
                        message: "用户在寒暄、询问身份或询问能力范围。请调用大模型按用户原话自然回应，说明你是哈尔滨师范大学招生智能顾问；能力范围包括录取分数、概率参考、招生简章、FAQ、培养方案、专业课程、校园生活等。不要编造具体录取事实，不要在身份/能力介绍结尾主动索要省份、科类、分数、位次。".to_owned(),
                    };
                    (
                        "请根据用户问法自然介绍身份或能力范围。".to_owned(),
                        structured,
                        Vec::new(),
                        0,
                    )
                }
                RetrievalIntent::KnowledgeAnswer => {
                    if asks_province_admission_major_list(&input.message) {
                        self.handle_province_major_list_request(&input.message, &memory)
                            .await?
                    } else if asks_major_admission_province_list(&input.message) {
                        self.handle_major_province_list_request(&input.message)
                            .await?
                    } else if let Some(college_name) =
                        extract_college_major_catalog_query(&input.message)
                    {
                        let majors = self.retrieval.list_college_majors(&college_name).await?;
                        let (reply, structured, citations) =
                            render_college_major_answer(&college_name, &majors);
                        (reply, structured, citations, 1)
                    } else if asks_major_group_without_college(&input.message) {
                        let candidates = self
                            .retrieval
                            .search_major_candidates(&input.message, 8)
                            .await?;
                        let structured = render_major_disambiguation(
                            &input.message,
                            candidates,
                            ChatIntent::ScoreQuery,
                        );
                        let reply = render_major_disambiguation_reply(&structured);
                        (reply, structured, Vec::new(), 1)
                    } else {
                        let knowledge_query = contextual_knowledge_query_with_history(
                            &input.message,
                            &memory,
                            &history,
                        );
                        let result = self.retrieval.retrieve_knowledge(&knowledge_query).await?;
                        RequiredToolGuard {
                            intent: ChatIntent::KnowledgeAnswer,
                            has_evidence: has_knowledge_evidence(&result.structured_result),
                        }
                        .validate()
                        .or_else(|_| {
                            // The guard blocks direct hallucination; synthesis still returns a boundary-aware
                            // answer when tools ran but no reliable evidence was found.
                            Ok::<(), anyhow::Error>(())
                        })?;
                        let reply = render_knowledge_answer(&result.structured_result);
                        (reply, result.structured_result, result.citations, 1)
                    }
                }
                RetrievalIntent::ScoreQuery => {
                    let missing = missing_score_fields(&memory);
                    if !missing.is_empty() {
                        let structured = ChatStructuredResult::FollowUp {
                            pending_intent: ChatIntent::ScoreQuery,
                            missing_fields: missing.clone(),
                            collected_profile: memory.clone(),
                        };
                        (
                            render_follow_up(&missing, &memory),
                            structured,
                            Vec::new(),
                            0,
                        )
                    } else {
                        let structured = self.query_scores_from_memory(&memory).await?;
                        let citations = citations_from_structured_result(&structured);
                        let reply = render_score_answer(&structured);
                        (reply, structured, citations, 1)
                    }
                }
                RetrievalIntent::ProbabilityAssessment => {
                    let missing = effective_probability_missing_fields(&input.message, &memory);
                    if !missing.is_empty() {
                        let structured = ChatStructuredResult::FollowUp {
                            pending_intent: ChatIntent::ProbabilityAssessment,
                            missing_fields: missing.clone(),
                            collected_profile: memory.clone(),
                        };
                        (
                            render_follow_up(&missing, &memory),
                            structured,
                            Vec::new(),
                            0,
                        )
                    } else {
                        let score_records = self.query_scores_from_memory(&memory).await?;
                        let structured = build_probability_from_memory(&memory, &score_records);
                        let reply = render_probability_answer(&structured);
                        (
                            reply,
                            structured,
                            vec![ChatCitation {
                                year: None,
                                source_label: "哈尔滨师范大学历年录取统计表".to_owned(),
                                source_url: None,
                            }],
                            1,
                        )
                    }
                }
                RetrievalIntent::GeneralAnswer => {
                    if asks_province_admission_major_list(&input.message) {
                        self.handle_province_major_list_request(&input.message, &memory)
                            .await?
                    } else if asks_major_admission_province_list(&input.message) {
                        self.handle_major_province_list_request(&input.message)
                            .await?
                    } else {
                        let redirect = build_redirect_prompt(&memory);
                        let answer = "这是普通咨询问题，需要调用大模型生成自然回答。边界：录取线、录取概率、招生计划、招生政策、招生电话、官网链接、专业培养方案、学分和课程等事实，不能脱离工具证据编造；城市印象、校园生活体验、备考建议、沟通建议等低风险内容，可以用常识和模型能力给出亲切建议，并说明具体安排以学校官方通知为准。".to_owned();
                        let structured = ChatStructuredResult::GeneralAnswer {
                            answer: answer.clone(),
                            redirect_prompt: redirect.clone(),
                            collected_profile: memory.clone(),
                        };
                        (format!("{answer} {redirect}"), structured, Vec::new(), 0)
                    }
                }
            }
        };
        trace.push(domain::AgentTraceStep {
            step: 1,
            node: "retrieval_or_draft".to_owned(),
            tool_name: if tool_call_count > 0 {
                Some(match route_intent {
                    ChatIntent::ScoreQuery | ChatIntent::ProbabilityAssessment => {
                        "query_admission_scores".to_owned()
                    }
                    ChatIntent::KnowledgeAnswer => "search_knowledge".to_owned(),
                    _ => "tool".to_owned(),
                })
            } else {
                None
            },
            duration_ms: None,
            error: None,
        });

        let mut final_reply = reply;
        let mut model_call_count = 0usize;
        let mut synthesis_used = false;
        let mut llm_model = self.llm.as_ref().map(|client| client.model().to_owned());
        if should_synthesize(&structured_result) {
            match self
                .synthesize_reply(&input.message, &final_reply, &structured_result, &citations)
                .await
            {
                Ok(Some(synthesized)) => {
                    model_call_count = 1;
                    synthesis_used = true;
                    final_reply = synthesized;
                    trace.push(domain::AgentTraceStep {
                        step: 2,
                        node: "llm_synthesis".to_owned(),
                        tool_name: None,
                        duration_ms: None,
                        error: None,
                    });
                }
                Ok(None) => {
                    llm_model = None;
                }
                Err(error) => {
                    tracing::warn!(error = %error, "llm synthesis failed; falling back to draft reply");
                    trace.push(domain::AgentTraceStep {
                        step: 2,
                        node: "llm_synthesis".to_owned(),
                        tool_name: None,
                        duration_ms: None,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        final_reply =
            ensure_reply_mentions_confirmed_major(final_reply, &structured_result, &memory);

        let diagnostics = domain::ChatDiagnostics {
            mode: "custom_runtime".to_owned(),
            route_intent: Some(route_intent),
            total_duration_ms: started_at.elapsed().as_millis(),
            model_call_count,
            llm_model,
            synthesis_used,
            tool_call_count,
            trace,
            compression: Some(compressed.diagnostics),
        };

        self.db
            .append_message(
                &conversation_id,
                "assistant",
                &final_reply,
                Some(&structured_result),
                &citations,
            )
            .await?;

        Ok(ChatReply {
            conversation_id,
            reply: final_reply,
            structured_result,
            citations,
            diagnostics: Some(diagnostics),
        })
    }

    async fn query_scores_from_memory(
        &self,
        memory: &ResolvedMemory,
    ) -> Result<ChatStructuredResult> {
        let major_slug = memory
            .major_slug
            .clone()
            .unwrap_or_else(|| memory.major_name.clone().unwrap_or_default());
        let major_name = memory
            .major_name
            .clone()
            .unwrap_or_else(|| major_slug.clone());
        self.retrieval
            .query_scores(
                memory
                    .province_name
                    .as_deref()
                    .or(memory.province_code.as_deref())
                    .unwrap(),
                &major_slug,
                &major_name,
                memory.subject_type.as_deref(),
                None,
            )
            .await
    }

    async fn handle_combined_request(
        &self,
        message: &str,
        memory: &ResolvedMemory,
        plan: CombinedRequestPlan,
    ) -> Result<(String, ChatStructuredResult, Vec<ChatCitation>, usize)> {
        let mut results = Vec::new();
        let mut citations = Vec::new();
        let mut tool_count = 0usize;
        let mut score_had_records = false;

        if asks_score_comparison(message) {
            if memory.province_name.is_none() && memory.province_code.is_none() {
                let missing = vec!["province".to_owned()];
                let structured = ChatStructuredResult::FollowUp {
                    pending_intent: ChatIntent::ScoreQuery,
                    missing_fields: missing.clone(),
                    collected_profile: memory.clone(),
                };
                return Ok((
                    render_follow_up(&missing, memory),
                    structured,
                    Vec::new(),
                    0,
                ));
            }
            let candidates = self.retrieval.search_major_candidates(message, 6).await?;
            let distinct = distinct_major_candidates(candidates, 2);
            if distinct.len() >= 2 {
                let province = memory
                    .province_name
                    .as_deref()
                    .or(memory.province_code.as_deref())
                    .unwrap()
                    .to_owned();
                let subject_type = memory.subject_type.clone();
                let first = distinct[0].clone();
                let second = distinct[1].clone();
                let (first_result, second_result) = tokio::try_join!(
                    self.retrieval.query_scores(
                        &province,
                        &first.slug,
                        &first.name,
                        subject_type.as_deref(),
                        None,
                    ),
                    self.retrieval.query_scores(
                        &province,
                        &second.slug,
                        &second.name,
                        subject_type.as_deref(),
                        None,
                    )
                )?;
                citations.extend(citations_from_structured_result(&first_result));
                citations.extend(citations_from_structured_result(&second_result));
                results.push(first_result);
                results.push(second_result);
                tool_count += 2;
            }
        }

        if (plan.include_score || plan.include_probability) && !asks_score_comparison(message) {
            let missing = if plan.include_probability {
                effective_probability_missing_fields(message, memory)
            } else {
                missing_score_fields(memory)
            };
            if !missing.is_empty() {
                let pending_intent = if plan.include_probability {
                    ChatIntent::ProbabilityAssessment
                } else {
                    ChatIntent::ScoreQuery
                };
                let structured = ChatStructuredResult::FollowUp {
                    pending_intent,
                    missing_fields: missing.clone(),
                    collected_profile: memory.clone(),
                };
                return Ok((
                    render_follow_up(&missing, memory),
                    structured,
                    Vec::new(),
                    0,
                ));
            }

            let score_result = self.query_scores_from_memory(memory).await?;
            score_had_records = score_result_has_records(&score_result);
            citations.extend(citations_from_structured_result(&score_result));
            tool_count += 1;

            if plan.include_probability {
                let probability = build_probability_from_memory(memory, &score_result);
                results.push(probability);
            }
            if plan.include_score {
                results.push(score_result);
            }
        }

        if plan.include_knowledge && (!plan.knowledge_when_score_empty || !score_had_records) {
            let knowledge_query = contextual_knowledge_query(message, memory);
            let knowledge = self.retrieval.retrieve_knowledge(&knowledge_query).await?;
            citations.extend(knowledge.citations);
            results.push(knowledge.structured_result);
            tool_count += 1;
        }

        let structured = compact_evidence_bundle(message, results);
        let citations = dedupe_citations(citations);
        let reply = render_evidence_bundle_answer(&structured);
        Ok((reply, structured, citations, tool_count))
    }

    async fn handle_province_major_list_request(
        &self,
        message: &str,
        memory: &ResolvedMemory,
    ) -> Result<(String, ChatStructuredResult, Vec<ChatCitation>, usize)> {
        let province = extract_known_province(message)
            .or_else(|| memory.province_name.clone())
            .or_else(|| memory.province_code.clone())
            .unwrap_or_default();
        let subject_type = extract_subject_type(message);
        let structured = self
            .retrieval
            .list_province_admission_majors(
                &province,
                subject_type.as_deref(),
                extract_year_from_message(message),
            )
            .await?;
        let citations = citations_from_structured_result(&structured);
        let reply = render_province_major_list_answer(&structured);
        Ok((reply, structured, citations, 1))
    }

    async fn handle_major_province_list_request(
        &self,
        message: &str,
    ) -> Result<(String, ChatStructuredResult, Vec<ChatCitation>, usize)> {
        let candidates = self.retrieval.search_major_candidates(message, 4).await?;
        let distinct = distinct_major_candidates(candidates, 2);
        let Some(candidate) = select_unambiguous_major_candidate(message, &distinct) else {
            if distinct.len() > 1 {
                let structured =
                    render_major_disambiguation(message, distinct, ChatIntent::KnowledgeAnswer);
                let reply = render_major_disambiguation_reply(&structured);
                return Ok((reply, structured, Vec::new(), 1));
            }
            let structured =
                render_major_disambiguation(message, Vec::new(), ChatIntent::KnowledgeAnswer);
            let reply = render_major_disambiguation_reply(&structured);
            return Ok((reply, structured, Vec::new(), 1));
        };

        let subject_type = extract_subject_type(message);
        let structured = self
            .retrieval
            .list_major_admission_provinces(
                &candidate.slug,
                &candidate.name,
                subject_type.as_deref(),
                extract_year_from_message(message),
            )
            .await?;
        let citations = citations_from_structured_result(&structured);
        let reply = render_major_province_list_answer(&structured);
        Ok((reply, structured, citations, 1))
    }

    async fn resolve_major_from_message(
        &self,
        memory: &mut ResolvedMemory,
        message: &str,
    ) -> Result<()> {
        if asks_major_group_without_college(message)
            || contains_policy_program_term(message)
            || asks_province_admission_major_list(message)
        {
            return Ok(());
        }
        let query = extract_switch_major_query(message).unwrap_or_else(|| message.to_owned());
        let mut candidates = self.retrieval.search_major_candidates(&query, 3).await?;
        let mut used_memory_fallback = false;
        if candidates.is_empty() {
            let Some(query) = memory
                .major_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            else {
                return Ok(());
            };
            candidates = self.retrieval.search_major_candidates(query, 3).await?;
            used_memory_fallback = true;
        }
        let Some(candidate) = candidates.first() else {
            return Ok(());
        };
        if memory.major_name.is_none()
            || message.contains(&candidate.name)
            || major_alias_matches(message, &candidate.name)
            || is_major_switch_message(message)
            || memory.major_name.as_deref().is_some_and(|value| {
                used_memory_fallback && major_alias_matches(value, &candidate.name)
            })
        {
            memory.major_slug = Some(candidate.slug.clone());
            memory.major_name = Some(candidate.name.clone());
        }
        Ok(())
    }

    async fn synthesize_reply(
        &self,
        user_message: &str,
        draft_reply: &str,
        structured_result: &ChatStructuredResult,
        citations: &[ChatCitation],
    ) -> Result<Option<String>> {
        let Some(llm) = &self.llm else {
            return Ok(None);
        };
        let structured_json = serde_json::to_string(structured_result)?;
        let citations_json = serde_json::to_string(citations)?;
        let turn_constraint = turn_synthesis_constraint(user_message, structured_result);
        let response = llm
            .complete(&[
                LlmMessage {
                    role: MessageRole::System,
                    content:
                        "你是哈尔滨师范大学招生智能顾问，回答面向学生和家长，要自然、亲切、准确，不要说“知识库命中”“有资料”“字段”等后台词。\n\n优先级规则：\n1. 系统边界最高，其次是用户本轮问题。必须先回答用户本轮真正想问的内容，不要因为模板或历史上下文转移主题。\n2. 结构化结果、证据和引用是事实来源；草稿回答只是提示，不可覆盖用户意图。\n\n事实边界分层：\n1. 高风险事实必须严格依据给定结构化结果、证据和引用回答，包括录取线、位次、录取概率、招生计划、招生政策、招生电话、官网链接、专业目录、培养方案、课程、学分、毕业要求、体检、语种、调剂和同分规则。没有证据时不要编造，要说明还需要按招生简章、培养方案、FAQ、官网或录取统计核对。\n2. 低风险泛聊可以发挥模型常识，包括城市印象、大学生活建议、备考建议、如何和家长沟通、入学前准备等；涉及学校具体安排时要用“通常/一般/建议以学校通知为准”等边界表达。\n3. 如果已经给出结构化证据，要优先用证据，不要把培养方案覆盖专业说成当年一定招生，不要把相近专业数据说成目标专业数据。\n4. 普通闲聊、身份介绍、能力介绍也要根据用户原话直接回应，不要套用无关模板；用户问“你擅长什么”时，回答能力范围即可，不要要求用户补省份分数。\n5. 用户用“和、或、以及、哪些”等方式同时问多个事实点时，要逐项回应；证据只覆盖其中一部分时，先回答有证据的部分，再明确说明其余部分在当前证据中没有直接条款支持，不能自行补充。\n6. 只有当用户正在问录取概率、分数线、志愿建议或明确需要个性化判断时，才追问省份、科类、分数、位次、意向专业；用户只是问学校介绍、校园生活或能力介绍时，不要用画像追问收尾。"
                            .to_owned(),
                },
                LlmMessage {
                    role: MessageRole::User,
                    content: format!(
                        "用户问题：{user_message}\n\n结构化结果：{}\n\n引用：{}\n\n草稿回答：{draft_reply}{turn_constraint}\n\n请输出最终中文回答，2-5段，保留关键省份、科类、分数、专业、年份和证据边界。",
                        truncate_chars(&structured_json, 6000),
                        truncate_chars(&citations_json, 1800),
                    ),
                },
            ])
            .await?;
        let content = response.content.trim().to_owned();
        if content.is_empty() {
            Ok(None)
        } else {
            Ok(Some(content))
        }
    }
}

fn should_avoid_profile_follow_up(
    user_message: &str,
    structured_result: &ChatStructuredResult,
) -> bool {
    if matches!(
        structured_result,
        ChatStructuredResult::ScoreQuery { .. }
            | ChatStructuredResult::ProbabilityAssessment { .. }
            | ChatStructuredResult::FollowUp { .. }
    ) {
        return false;
    }
    !asks_score_line(user_message)
        && !asks_probability(user_message)
        && extract_score(user_message).is_none()
}

fn turn_synthesis_constraint(
    user_message: &str,
    structured_result: &ChatStructuredResult,
) -> String {
    let mut constraints = Vec::new();
    if should_avoid_profile_follow_up(user_message, structured_result) {
        constraints.push("用户不是在请求录取概率、分数线或志愿个性化评估。请只回答本轮问题，不要在结尾主动要求用户提供省份、科类、分数、位次或意向专业。");
    }
    if is_compound_fact_question(user_message) {
        constraints.push("用户本轮同时问了多个事实点。最终回答必须逐项回应每个事实点；如果结构化证据只覆盖其中一部分，要明确说明未覆盖部分在当前证据中没有直接条款支持，不能把相近条款当成确定事实。");
    }
    if constraints.is_empty() {
        String::new()
    } else {
        format!("\n\n本轮任务约束：{}", constraints.join(" "))
    }
}

fn is_compound_fact_question(message: &str) -> bool {
    contains_any_text(message, &["或", "和", "以及", "哪些"])
        && contains_any_text(
            message,
            &[
                "限制", "要求", "规则", "安排", "课程", "学分", "实践", "实习",
            ],
        )
}

pub fn chunk_reply_text(reply: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in reply.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '\n') || current.chars().count() >= 24 {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[allow(dead_code)]
pub fn build_runtime_context(
    conversation_id: String,
    user_message: String,
    memory: ResolvedMemory,
    history: Vec<ConversationMessage>,
) -> RuntimeContext {
    RuntimeContext {
        conversation_id,
        user_message,
        memory,
        history,
        route_intent: None,
        structured_result: None,
        draft_reply: None,
        compression: None,
    }
}

#[allow(dead_code)]
pub fn finish_runtime_context(context: RuntimeContext) -> RuntimeOutput {
    RuntimeOutput {
        context,
        diagnostics: domain::ChatDiagnostics {
            mode: "custom_runtime".to_owned(),
            route_intent: None,
            total_duration_ms: 0,
            model_call_count: 0,
            llm_model: None,
            synthesis_used: false,
            tool_call_count: 0,
            trace: Vec::new(),
            compression: None,
        },
    }
}

fn to_chat_intent(intent: &RetrievalIntent) -> ChatIntent {
    match intent {
        RetrievalIntent::Greeting => ChatIntent::Greeting,
        RetrievalIntent::ScoreQuery => ChatIntent::ScoreQuery,
        RetrievalIntent::ProbabilityAssessment => ChatIntent::ProbabilityAssessment,
        RetrievalIntent::KnowledgeAnswer => ChatIntent::KnowledgeAnswer,
        RetrievalIntent::GeneralAnswer => ChatIntent::GeneralAnswer,
    }
}

fn latest_assistant_structured(history: &[ConversationMessage]) -> Option<&ChatStructuredResult> {
    history
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .and_then(|message| message.structured_payload.as_ref())
}

#[derive(Debug, Clone, Copy)]
struct CombinedRequestPlan {
    include_score: bool,
    include_probability: bool,
    include_knowledge: bool,
    knowledge_when_score_empty: bool,
}

fn combined_request_plan(
    message: &str,
    memory: &ResolvedMemory,
    route_intent: &RetrievalIntent,
    last_assistant_structured: Option<&ChatStructuredResult>,
) -> Option<CombinedRequestPlan> {
    if matches!(
        route_intent,
        RetrievalIntent::Greeting | RetrievalIntent::GeneralAnswer
    ) {
        return None;
    }

    let asks_score = asks_score_line(message) || asks_score_comparison(message);
    let asks_probability =
        asks_probability(message) || matches!(route_intent, RetrievalIntent::ProbabilityAssessment);
    let knowledge_when_score_empty = asks_score_with_likely_training_plan_followup(message)
        && !needs_parallel_knowledge_for_score_line(message);
    let asks_knowledge = asks_training_plan_context(message)
        || matches!(route_intent, RetrievalIntent::KnowledgeAnswer)
            && (asks_score || asks_probability)
        || knowledge_when_score_empty
        || needs_parallel_knowledge_for_score_line(message);
    let asks_multi_major_score = asks_score_comparison(message);
    let continues_score_probability_bundle =
        last_was_score_probability_bundle(last_assistant_structured)
            && has_score_context(memory)
            && memory.score.is_some()
            && (is_short_follow_up(message)
                || is_major_switch_message(message)
                || extract_known_province(message).is_some()
                || extract_score(message).is_some());
    let explicit_combo = (asks_score && asks_probability)
        || (asks_score && asks_knowledge)
        || (asks_probability && asks_knowledge)
        || asks_multi_major_score
        || continues_score_probability_bundle;

    if !explicit_combo {
        return None;
    }

    let has_major_context = memory.major_name.is_some() || memory.major_slug.is_some();
    let has_province_context = memory.province_name.is_some() || memory.province_code.is_some();
    if !has_major_context && !asks_multi_major_score {
        return None;
    }
    if !has_province_context && (asks_score || asks_probability) {
        return None;
    }

    Some(CombinedRequestPlan {
        include_score: asks_score || asks_multi_major_score || continues_score_probability_bundle,
        include_probability: asks_probability || continues_score_probability_bundle,
        include_knowledge: asks_knowledge,
        knowledge_when_score_empty,
    })
}

fn last_was_score_probability_bundle(result: Option<&ChatStructuredResult>) -> bool {
    let Some(ChatStructuredResult::EvidenceBundle { results, .. }) = result else {
        return false;
    };
    let has_score = results
        .iter()
        .any(|item| matches!(item, ChatStructuredResult::ScoreQuery { .. }));
    let has_probability = results
        .iter()
        .any(|item| matches!(item, ChatStructuredResult::ProbabilityAssessment { .. }));
    has_score && has_probability
}

fn asks_score_line(message: &str) -> bool {
    [
        "录取线",
        "分数线",
        "最低分",
        "录取分",
        "历年分",
        "近三年",
        "2021到2025",
        "21到25",
    ]
    .iter()
    .any(|item| message.contains(item))
}

fn asks_probability(message: &str) -> bool {
    [
        "概率",
        "能上",
        "能不能上",
        "能报",
        "稳吗",
        "冲稳保",
        "希望大吗",
    ]
    .iter()
    .any(|item| message.contains(item))
}

fn asks_score_comparison(message: &str) -> bool {
    (message.contains("哪个") || message.contains("谁") || message.contains("比较"))
        && (message.contains("分数") || message.contains("录取线") || message.contains("更高"))
}

fn asks_training_plan_context(message: &str) -> bool {
    [
        "培养方案",
        "课程",
        "毕业要求",
        "毕业条件",
        "实践环节",
        "教育实习",
        "第二课堂",
        "创新实践",
    ]
    .iter()
    .any(|item| message.contains(item))
}

fn asks_score_with_likely_training_plan_followup(message: &str) -> bool {
    asks_score_line(message)
        && !asks_probability(message)
        && (message.contains("2021到2025") || message.contains("21到25"))
}

fn needs_parallel_knowledge_for_score_line(message: &str) -> bool {
    asks_score_line(message)
        && contains_any_text(message, &["2021到2025", "21到25", "近三年", "历年"])
        && contains_any_text(
            message,
            &[
                "艺术类",
                "综合分",
                "专业课",
                "美术",
                "绘画",
                "书法",
                "设计",
                "视觉传达",
                "环境设计",
                "音乐",
                "舞蹈",
                "作曲",
                "表演",
                "播音",
                "体育",
            ],
        )
}

fn build_probability_from_memory(
    memory: &ResolvedMemory,
    score_records: &ChatStructuredResult,
) -> ChatStructuredResult {
    let major_name = memory
        .major_name
        .as_deref()
        .or(memory.major_slug.as_deref())
        .unwrap_or("目标专业");
    let (score_summary, score_history) = match score_records {
        ChatStructuredResult::ScoreQuery {
            records, summary, ..
        } => (
            json!({
                "recordCount": records.len(),
                "years": summary.years,
                "latestMinScore": records.first().map(|record| record.min_score),
                "latestYear": records.first().map(|record| record.year),
                "records": records.iter().take(5).collect::<Vec<_>>()
            }),
            records
                .iter()
                .map(|record| ProbabilityScoreHistoryItem {
                    year: record.year,
                    min_score: record.min_score,
                    min_rank: record.min_rank,
                })
                .collect::<Vec<_>>(),
        ),
        _ => (json!({}), Vec::new()),
    };
    let plan_history: Vec<ProbabilityPlanHistoryItem> = Vec::new();
    let engine = calculate_admission_probability(ProbabilityEngineInput {
        score: memory.score.unwrap_or(0.0),
        rank: memory.rank,
        score_history,
        plan_history: plan_history.clone(),
        source_mode: ProbabilitySourceMode::Major,
    });
    ChatStructuredResult::ProbabilityAssessment {
        assessment: json!({
            "province": memory.province_name.as_deref().or(memory.province_code.as_deref()),
            "subjectType": memory.subject_type,
            "score": memory.score,
            "rank": memory.rank,
            "major": major_name,
            "probability": engine.probability,
            "level": engine.level.as_str(),
            "confidence": engine.confidence.as_str(),
            "summary": engine.summary,
            "factors": engine.factors,
            "disclaimer": engine.disclaimer,
            "basis": {
                "scoreDataMode": "major",
                "scoreYearsUsed": score_summary
                    .get("recordCount")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                "planYearsUsed": plan_history.len()
            },
            "metrics": engine.metrics,
            "scoreSummary": score_summary,
            "message": "已使用确定性概率引擎，基于历年录取分数、位次和数据完整度进行参考评估。"
        }),
    }
}

fn compact_evidence_bundle(
    message: &str,
    results: Vec<ChatStructuredResult>,
) -> ChatStructuredResult {
    let mut seen = HashSet::new();
    let mut compacted = Vec::new();
    for result in results {
        let key = evidence_key(&result);
        if seen.insert(key) {
            compacted.push(result);
        }
    }
    if compacted.len() == 1 {
        return compacted.into_iter().next().unwrap();
    }
    ChatStructuredResult::EvidenceBundle {
        message: message.to_owned(),
        results: compacted,
    }
}

fn score_result_has_records(result: &ChatStructuredResult) -> bool {
    matches!(result, ChatStructuredResult::ScoreQuery { records, .. } if !records.is_empty())
}

fn evidence_key(result: &ChatStructuredResult) -> String {
    match result {
        ChatStructuredResult::ScoreQuery {
            major_name,
            province,
            subject_type,
            ..
        } => format!(
            "score:{province}:{major_name}:{}",
            subject_type.as_deref().unwrap_or("")
        ),
        ChatStructuredResult::ProbabilityAssessment { assessment } => format!(
            "probability:{}:{}:{}:{}",
            assessment
                .get("province")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            assessment
                .get("subjectType")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            assessment
                .get("score")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0),
            assessment
                .get("major")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
        ChatStructuredResult::KnowledgeAnswer { query, .. } => format!("knowledge:{query}"),
        ChatStructuredResult::ProvinceMajorList {
            province,
            subject_type,
            year,
            ..
        } => format!(
            "province-major-list:{province}:{}:{}",
            subject_type.as_deref().unwrap_or(""),
            year.map(|value| value.to_string()).unwrap_or_default()
        ),
        ChatStructuredResult::MajorProvinceList {
            major_name,
            subject_type,
            year,
            ..
        } => format!(
            "major-province-list:{major_name}:{}:{}",
            subject_type.as_deref().unwrap_or(""),
            year.map(|value| value.to_string()).unwrap_or_default()
        ),
        other => other.kind().to_owned(),
    }
}

fn dedupe_citations(citations: Vec<ChatCitation>) -> Vec<ChatCitation> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for citation in citations {
        let key = format!(
            "{}:{}:{}",
            citation
                .year
                .map(|value| value.to_string())
                .unwrap_or_default(),
            citation.source_label,
            citation.source_url.as_deref().unwrap_or_default()
        );
        if seen.insert(key) {
            deduped.push(citation);
        }
    }
    deduped
}

fn distinct_major_candidates(
    candidates: Vec<domain::MajorCandidate>,
    limit: usize,
) -> Vec<domain::MajorCandidate> {
    let mut seen_roots = HashSet::new();
    let mut distinct = Vec::new();
    for candidate in candidates {
        let key = normalize_major_alias(&candidate.name);
        if seen_roots.insert(key) {
            distinct.push(candidate);
        }
        if distinct.len() >= limit {
            break;
        }
    }
    distinct
}

fn select_unambiguous_major_candidate<'a>(
    message: &str,
    candidates: &'a [domain::MajorCandidate],
) -> Option<&'a domain::MajorCandidate> {
    let first = candidates.first()?;
    let matching = candidates
        .iter()
        .filter(|candidate| major_alias_matches(message, &candidate.name))
        .collect::<Vec<_>>();
    if matching.len() == 1 {
        return matching.first().copied();
    }

    if !major_alias_matches(message, &first.name) {
        return (candidates.len() == 1).then_some(first);
    }

    if !is_policy_variant_major(&first.name)
        && matching
            .iter()
            .skip(1)
            .all(|candidate| is_policy_variant_major(&candidate.name))
    {
        return Some(first);
    }

    (candidates.len() == 1).then_some(first)
}

fn is_policy_variant_major(name: &str) -> bool {
    contains_any_text(
        name,
        &[
            "固边",
            "公费",
            "优师",
            "专项",
            "定向",
            "省属",
            "地方",
            "少数民族",
            "实验班",
            "中美",
            "121",
        ],
    )
}

fn has_knowledge_evidence(result: &ChatStructuredResult) -> bool {
    matches!(
        result,
        ChatStructuredResult::KnowledgeAnswer {
            faq,
            policies,
            vector_chunks,
            ..
        } if !faq.is_empty() || !policies.is_empty() || !vector_chunks.is_empty()
    )
}

fn enrich_memory_from_history(memory: &mut ResolvedMemory, history: &[ConversationMessage]) {
    for message in history.iter().rev() {
        merge_memory_from_history_message(memory, message);
        let Some(structured) = &message.structured_payload else {
            continue;
        };
        merge_memory_from_structured(memory, structured);
        if has_minimum_context(memory) {
            break;
        }
    }
}

fn merge_memory_from_history_message(memory: &mut ResolvedMemory, message: &ConversationMessage) {
    if memory.major_name.is_some() || message.role != "user" {
        return;
    }
    if is_admission_policy_query(&message.content)
        || !should_update_major_from_knowledge_query(&message.content)
    {
        return;
    }
    if let Some(major) = extract_major_phrase(&message.content) {
        memory.major_name = Some(major.clone());
        memory.major_slug = Some(major);
    }
}

fn merge_memory_from_structured(memory: &mut ResolvedMemory, structured: &ChatStructuredResult) {
    match structured {
        ChatStructuredResult::FollowUp {
            pending_intent,
            collected_profile,
            ..
        } => {
            merge_memory(memory, collected_profile);
            memory
                .pending_intent
                .get_or_insert_with(|| pending_intent.clone());
        }
        ChatStructuredResult::ScoreQuery {
            major_name,
            province,
            subject_type,
            ..
        } => {
            memory.major_name.get_or_insert_with(|| major_name.clone());
            memory.major_slug.get_or_insert_with(|| major_name.clone());
            memory.province_name.get_or_insert_with(|| province.clone());
            if let Some(subject_type) = subject_type {
                memory
                    .subject_type
                    .get_or_insert_with(|| subject_type.clone());
            }
            memory.pending_intent.get_or_insert(ChatIntent::ScoreQuery);
        }
        ChatStructuredResult::ProbabilityAssessment { assessment } => {
            if memory.province_name.is_none() {
                memory.province_name = assessment
                    .get("province")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
            }
            if memory.subject_type.is_none() {
                memory.subject_type = assessment
                    .get("subjectType")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
            }
            if memory.major_name.is_none() {
                memory.major_name = assessment
                    .get("major")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned);
                memory.major_slug = memory.major_name.clone();
            }
            if memory.score.is_none() {
                memory.score = assessment.get("score").and_then(|value| value.as_f64());
            }
            if memory.rank.is_none() {
                memory.rank = assessment.get("rank").and_then(|value| value.as_f64());
            }
            memory
                .pending_intent
                .get_or_insert(ChatIntent::ProbabilityAssessment);
        }
        ChatStructuredResult::KnowledgeAnswer {
            query,
            vector_chunks,
            ..
        } => {
            memory
                .pending_intent
                .get_or_insert(ChatIntent::KnowledgeAnswer);
            if memory.major_name.is_none()
                && !is_admission_policy_query(query)
                && should_update_major_from_knowledge_query(query)
            {
                memory.major_name = major_name_from_vector_chunks(vector_chunks)
                    .or_else(|| extract_major_phrase(query));
                memory.major_slug = memory.major_name.clone();
            }
        }
        ChatStructuredResult::ProvinceMajorList {
            province,
            subject_type,
            ..
        } => {
            memory.province_name.get_or_insert_with(|| province.clone());
            if let Some(subject_type) = subject_type {
                memory
                    .subject_type
                    .get_or_insert_with(|| subject_type.clone());
            }
            memory
                .pending_intent
                .get_or_insert(ChatIntent::KnowledgeAnswer);
        }
        ChatStructuredResult::MajorProvinceList {
            major_name,
            subject_type,
            ..
        } => {
            memory.major_name.get_or_insert_with(|| major_name.clone());
            memory.major_slug.get_or_insert_with(|| major_name.clone());
            if let Some(subject_type) = subject_type {
                memory
                    .subject_type
                    .get_or_insert_with(|| subject_type.clone());
            }
            memory
                .pending_intent
                .get_or_insert(ChatIntent::KnowledgeAnswer);
        }
        ChatStructuredResult::MajorDisambiguation {
            pending_intent,
            candidates,
            ..
        } => {
            memory
                .pending_intent
                .get_or_insert_with(|| pending_intent.clone());
            if candidates.len() == 1 {
                if let Some(candidate) = candidates.first() {
                    memory.major_name = Some(candidate.name.clone());
                    memory.major_slug = Some(candidate.slug.clone());
                }
            }
        }
        ChatStructuredResult::EvidenceBundle { results, .. } => {
            let has_probability = results
                .iter()
                .any(|result| matches!(result, ChatStructuredResult::ProbabilityAssessment { .. }));
            for result in results {
                merge_memory_from_structured(memory, result);
            }
            if has_probability && memory.pending_intent.is_none() {
                memory.pending_intent = Some(ChatIntent::ProbabilityAssessment);
            }
        }
        ChatStructuredResult::GeneralAnswer {
            collected_profile, ..
        } => {
            merge_memory(memory, collected_profile);
        }
        ChatStructuredResult::Greeting { .. } | ChatStructuredResult::FallbackReply { .. } => {}
    }
}

fn merge_memory(target: &mut ResolvedMemory, source: &ResolvedMemory) {
    if target.province_code.is_none() {
        target.province_code = source.province_code.clone();
    }
    if target.province_name.is_none() {
        target.province_name = source.province_name.clone();
    }
    if target.subject_type.is_none() {
        target.subject_type = source.subject_type.clone();
    }
    if target.score.is_none() {
        target.score = source.score;
    }
    if target.rank.is_none() {
        target.rank = source.rank;
    }
    if target.major_slug.is_none() {
        target.major_slug = source.major_slug.clone();
    }
    if target.major_name.is_none() {
        target.major_name = source.major_name.clone();
    }
    if target.intended_majors.is_empty() {
        target.intended_majors = source.intended_majors.clone();
    }
    if target.pending_intent.is_none() {
        target.pending_intent = source.pending_intent.clone();
    }
}

fn has_minimum_context(memory: &ResolvedMemory) -> bool {
    memory.province_name.is_some()
        && memory.subject_type.is_some()
        && memory.major_name.is_some()
        && memory.score.is_some()
}

fn apply_contextual_route(
    route: retrieval::RouteDecision,
    message: &str,
    memory: &ResolvedMemory,
) -> retrieval::RouteDecision {
    if asks_major_admission_province_list(message) {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::KnowledgeAnswer,
            must_use_tools: true,
            reason: "专业招生省份列表需要查询录取统计覆盖关系。".to_owned(),
        };
    }

    if asks_province_admission_major_list(message) {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::KnowledgeAnswer,
            must_use_tools: true,
            reason: "省份招生专业列表需要查询分省招生计划或录取统计兜底。".to_owned(),
        };
    }

    if !matches!(route.intent, RetrievalIntent::GeneralAnswer) {
        return route;
    }

    if extract_score(message).is_some() && has_score_context(memory) {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::ProbabilityAssessment,
            must_use_tools: true,
            reason: "短句包含分数，并可从上下文继承省份、科类和专业。".to_owned(),
        };
    }

    if extract_score(message).is_some()
        && explicit_major_text(message).is_some()
        && (memory.province_name.is_some() || memory.province_code.is_some())
        && matches!(
            memory.pending_intent,
            Some(ChatIntent::ProbabilityAssessment)
        )
    {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::ProbabilityAssessment,
            must_use_tools: true,
            reason: "短句包含新专业和分数，并继承上一轮概率评估意图。".to_owned(),
        };
    }

    if extract_known_province(message).is_some()
        && memory.major_name.is_some()
        && matches!(
            memory.pending_intent,
            Some(ChatIntent::ProbabilityAssessment)
        )
        && memory.score.is_some()
        && memory.subject_type.is_some()
    {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::ProbabilityAssessment,
            must_use_tools: true,
            reason: "短句显式更换省份，并继承上一轮概率评估画像。".to_owned(),
        };
    }

    if extract_known_province(message).is_some() && memory.major_name.is_some() {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::ScoreQuery,
            must_use_tools: true,
            reason: "短句包含省份，并可从上下文继承专业。".to_owned(),
        };
    }

    if is_short_follow_up(message) {
        if let Some(intent) = &memory.pending_intent {
            let mapped = match intent {
                ChatIntent::ScoreQuery => Some(RetrievalIntent::ScoreQuery),
                ChatIntent::ProbabilityAssessment => Some(RetrievalIntent::ProbabilityAssessment),
                ChatIntent::KnowledgeAnswer => Some(RetrievalIntent::KnowledgeAnswer),
                _ => None,
            };
            if let Some(intent) = mapped {
                return retrieval::RouteDecision {
                    intent,
                    must_use_tools: true,
                    reason: "短句续问继承上一轮招生咨询意图。".to_owned(),
                };
            }
        }
    }

    if matches!(memory.pending_intent, Some(ChatIntent::KnowledgeAnswer))
        && looks_like_knowledge_follow_up(message)
    {
        return retrieval::RouteDecision {
            intent: RetrievalIntent::KnowledgeAnswer,
            must_use_tools: true,
            reason: "知识类连续追问需要结合上一轮主题继续检索。".to_owned(),
        };
    }

    route
}

fn has_score_context(memory: &ResolvedMemory) -> bool {
    (memory.province_name.is_some() || memory.province_code.is_some())
        && memory.subject_type.is_some()
        && (memory.major_name.is_some() || memory.major_slug.is_some())
}

fn is_short_follow_up(message: &str) -> bool {
    let trimmed = message.trim();
    trimmed.chars().count() <= 14
        || trimmed.ends_with("呢？")
        || trimmed.ends_with("呢")
        || trimmed.contains("继续")
        || trimmed.contains("解读")
}

fn is_major_switch_message(message: &str) -> bool {
    let trimmed = message.trim();
    (trimmed.starts_with("那") || trimmed.starts_with("换成") || trimmed.starts_with("改成"))
        && trimmed.chars().count() <= 18
        && !contains_policy_program_term(trimmed)
        && !asks_province_admission_major_list(trimmed)
}

fn extract_switch_major_query(message: &str) -> Option<String> {
    let mut text = message
        .trim()
        .trim_matches(['，', ',', '。', '？', '?', '！', '!', ' '])
        .to_owned();
    for prefix in ["换成", "改成", "那", "再看", "看看"] {
        if let Some(stripped) = text.strip_prefix(prefix) {
            text = stripped.to_owned();
            break;
        }
    }
    text = text
        .trim()
        .trim_end_matches('呢')
        .trim_end_matches("专业")
        .trim_matches(['，', ',', '。', '？', '?', '！', '!', ' '])
        .to_owned();
    if text.chars().count() >= 2
        && text.chars().count() <= 16
        && extract_score(&text).is_none()
        && !text.contains('分')
        && !text.contains("能上")
        && !text.contains("能不能")
        && !text.contains("能报")
        && !text.contains("稳吗")
        && !text.contains("稳不稳")
        && !text.contains("招生")
        && !text.contains("概率")
        && !text.contains("分数")
        && extract_known_province(&text).is_none()
    {
        Some(text)
    } else {
        None
    }
}

fn asks_major_group_without_college(message: &str) -> bool {
    let asks_group = ["有哪些专业", "有什么专业", "有啥专业", "开设哪些专业"]
        .iter()
        .any(|item| message.contains(item));
    asks_group
        && !contains_policy_program_term(message)
        && !message.contains("学院")
        && !message.contains("招生")
        && extract_known_province(message).is_none()
}

fn contains_policy_program_term(message: &str) -> bool {
    [
        "公费师范",
        "公费师范生",
        "专项计划",
        "少数民族预科",
        "地方专项",
        "国家专项",
        "优师",
    ]
    .iter()
    .any(|item| message.contains(item))
}

fn asks_province_admission_major_list(message: &str) -> bool {
    extract_known_province(message).is_some()
        && (message.contains("招生") || message.contains("招哪些") || message.contains("招什么"))
        && (message.contains("专业") || message.contains("哪些") || message.contains("什么"))
}

fn asks_major_admission_province_list(message: &str) -> bool {
    extract_known_province(message).is_none()
        && contains_any_text(
            message,
            &["哪些省", "哪些省份", "哪个省", "哪些地区", "省份"],
        )
        && contains_any_text(message, &["招生", "招收", "招", "录取记录"])
        && !asks_major_group_without_college(message)
}

fn looks_like_knowledge_follow_up(message: &str) -> bool {
    [
        "有没有",
        "有吗",
        "课程",
        "实践",
        "学分",
        "毕业",
        "培养",
        "目标",
        "要求",
        "环节",
        "换成",
        "这些课",
        "怎么安排",
        "占多少",
        "再说说",
    ]
    .iter()
    .any(|item| message.contains(item))
}

fn contextual_knowledge_query(message: &str, memory: &ResolvedMemory) -> String {
    let Some(major) = memory.major_name.as_deref() else {
        return message.to_owned();
    };
    if message.contains(major) || major_alias_matches(message, major) {
        return message.to_owned();
    }
    if extract_major_phrase(message)
        .as_deref()
        .is_some_and(|current_major| !major_alias_matches(current_major, major))
    {
        return message.to_owned();
    }
    if looks_like_knowledge_follow_up(message) || is_short_follow_up(message) {
        format!("{major} {message}")
    } else {
        message.to_owned()
    }
}

fn contextual_knowledge_query_with_history(
    message: &str,
    memory: &ResolvedMemory,
    history: &[ConversationMessage],
) -> String {
    let query = contextual_knowledge_query(message, memory);
    if query != message || !(looks_like_knowledge_follow_up(message) || is_short_follow_up(message))
    {
        return query;
    }
    if extract_major_phrase(message).is_some() {
        return query;
    }

    history
        .iter()
        .rev()
        .filter(|item| item.role == "user")
        .filter(|item| !is_admission_policy_query(&item.content))
        .filter(|item| should_update_major_from_knowledge_query(&item.content))
        .find_map(|item| extract_major_phrase(&item.content))
        .map(|major| format!("{major} {message}"))
        .unwrap_or(query)
}

fn render_major_disambiguation_reply(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::MajorDisambiguation { candidates, .. } = result else {
        return "我需要先确认你想了解的具体专业。".to_owned();
    };
    if candidates.is_empty() {
        return "我需要先确认你想了解的具体专业。你可以说专业全称，或补充学院、方向关键词。"
            .to_owned();
    }
    let names = candidates
        .iter()
        .map(|candidate| candidate.name.as_str())
        .collect::<Vec<_>>();
    format!(
        "我先帮你把可能相关的专业列出来：{}。你可以指定其中一个专业，我再继续查分数线、招生情况或培养方案。",
        names.join("、")
    )
}

fn render_probability_answer(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::ProbabilityAssessment { assessment } = result else {
        return "我已经查询录取统计，可以结合你的分数做参考判断。".to_owned();
    };
    let province = assessment
        .get("province")
        .and_then(|value| value.as_str())
        .unwrap_or("对应省份");
    let subject_type = assessment
        .get("subjectType")
        .and_then(|value| value.as_str())
        .unwrap_or("对应科类");
    let score = assessment
        .get("score")
        .and_then(|value| value.as_f64())
        .map(|value| format!("{value:.0}分"))
        .unwrap_or_else(|| "你的分数".to_owned());
    let major = assessment
        .get("major")
        .and_then(|value| value.as_str())
        .unwrap_or("目标专业");
    let probability = assessment
        .get("probability")
        .and_then(|value| value.as_i64());
    let level = assessment
        .get("level")
        .and_then(|value| value.as_str())
        .unwrap_or("reference");
    let confidence = assessment
        .get("confidence")
        .and_then(|value| value.as_str())
        .unwrap_or("low");
    let summary_text = assessment
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("该结果为历史数据推断，仅供参考。");
    let factor_text = assessment
        .get("factors")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .take(3)
                .collect::<Vec<_>>()
                .join("；")
        })
        .filter(|value| !value.is_empty());
    let score_summary = assessment.get("scoreSummary");
    let record_count = score_summary
        .and_then(|value| value.get("recordCount"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    if record_count == 0 {
        let probability_text = probability
            .map(|value| format!("系统给出的粗略参考概率为 {value}%（置信度 {confidence}）"))
            .unwrap_or_else(|| "当前只能做低置信度参考".to_owned());
        return format!(
            "我已按{province}、{subject_type}、{score}和{major}查询历年录取统计，但暂时没有找到可直接对比的分专业记录。{probability_text}，不能当成目标专业的准确录取概率。建议继续核对专业全称、科类和当年招生计划。"
        );
    }
    let latest_year = score_summary
        .and_then(|value| value.get("latestYear"))
        .and_then(|value| value.as_i64());
    let latest_min = score_summary
        .and_then(|value| value.get("latestMinScore"))
        .and_then(|value| value.as_i64());
    match (latest_year, latest_min) {
        (Some(year), Some(min_score)) => {
            let probability_text = probability
                .map(|value| {
                    format!(
                        "确定性概率引擎给出的参考概率为 {value}%（{level}，置信度 {confidence}）"
                    )
                })
                .unwrap_or_else(|| "已完成基于历年分数线的参考评估".to_owned());
            format!(
                "我按{province}、{subject_type}、{major}查询到了历年录取统计。最近可用记录是 {year} 年最低分 {min_score} 分，你的分数是{score}。{probability_text}。{summary_text}{}这只能作为历史参考，最终还要结合当年招生计划、报考热度和省级投档规则判断。",
                factor_text
                    .map(|value| format!("主要依据：{value}。"))
                    .unwrap_or_default()
            )
        }
        _ => {
            let probability_text = probability
                .map(|value| format!("参考概率为 {value}%（{level}，置信度 {confidence}）"))
                .unwrap_or_else(|| "可以作为历史分数线对比参考".to_owned());
            format!(
                "我按{province}、{subject_type}、{major}查询到了 {record_count} 条历年录取记录。你的分数是{score}，{probability_text}。最终还要结合当年招生计划和报考热度判断。"
            )
        }
    }
}

fn render_evidence_bundle_answer(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::EvidenceBundle { results, .. } = result else {
        return match result {
            ChatStructuredResult::ScoreQuery { .. } => render_score_answer(result),
            ChatStructuredResult::ProbabilityAssessment { .. } => render_probability_answer(result),
            ChatStructuredResult::KnowledgeAnswer { .. } => render_knowledge_answer(result),
            ChatStructuredResult::ProvinceMajorList { .. } => {
                render_province_major_list_answer(result)
            }
            ChatStructuredResult::MajorProvinceList { .. } => {
                render_major_province_list_answer(result)
            }
            _ => "我已经合并查询了相关招生证据。".to_owned(),
        };
    };

    let mut parts = Vec::new();
    for item in results {
        match item {
            ChatStructuredResult::ProbabilityAssessment { .. } => {
                parts.push(render_probability_answer(item));
            }
            ChatStructuredResult::ScoreQuery { .. } => {
                parts.push(render_score_answer(item));
            }
            ChatStructuredResult::KnowledgeAnswer { .. } => {
                parts.push(render_knowledge_answer(item));
            }
            ChatStructuredResult::ProvinceMajorList { .. } => {
                parts.push(render_province_major_list_answer(item));
            }
            ChatStructuredResult::MajorProvinceList { .. } => {
                parts.push(render_major_province_list_answer(item));
            }
            ChatStructuredResult::MajorDisambiguation { .. } => {
                parts.push(render_major_disambiguation_reply(item));
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        "我已经合并查询了相关招生证据。".to_owned()
    } else {
        parts.join("\n\n")
    }
}

fn ensure_reply_mentions_confirmed_major(
    reply: String,
    structured_result: &ChatStructuredResult,
    _memory: &ResolvedMemory,
) -> String {
    let major = target_major_from_structured(structured_result)
        .filter(|value| is_plausible_major_text(value));
    let Some(major) = major else {
        return reply;
    };
    if reply.contains(&major) {
        return reply;
    }
    let normalized_reply = normalize_major_alias(&reply);
    let normalized_major = normalize_major_alias(&major);
    let normalized_root =
        normalize_major_alias(major.split(['（', '(']).next().unwrap_or(major.as_str()));
    if (!normalized_major.is_empty() && normalized_reply.contains(&normalized_major))
        || (!normalized_root.is_empty() && normalized_reply.contains(&normalized_root))
    {
        return reply;
    }
    format!("关于{major}，{reply}")
}

fn target_major_from_structured(result: &ChatStructuredResult) -> Option<String> {
    match result {
        ChatStructuredResult::ScoreQuery { major_name, .. } => Some(major_name.clone()),
        ChatStructuredResult::MajorProvinceList { major_name, .. } => Some(major_name.clone()),
        ChatStructuredResult::ProbabilityAssessment { assessment } => assessment
            .get("major")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        ChatStructuredResult::KnowledgeAnswer {
            query,
            vector_chunks,
            ..
        } if should_update_major_from_knowledge_query(query) => {
            major_name_from_vector_chunks(vector_chunks).or_else(|| extract_major_phrase(query))
        }
        ChatStructuredResult::EvidenceBundle { results, .. } => {
            results.iter().find_map(target_major_from_structured)
        }
        ChatStructuredResult::MajorDisambiguation { candidates, .. } if candidates.len() == 1 => {
            candidates.first().map(|candidate| candidate.name.clone())
        }
        _ => None,
    }
}

fn major_name_from_vector_chunks(chunks: &[domain::VectorChunkEvidence]) -> Option<String> {
    chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .metadata
                .get("majorName")
                .and_then(|value| value.as_str())
        })
        .find(|value| is_plausible_major_text(value))
        .map(ToOwned::to_owned)
}

fn should_synthesize(structured_result: &ChatStructuredResult) -> bool {
    matches!(
        structured_result,
        ChatStructuredResult::Greeting { .. }
            | ChatStructuredResult::ScoreQuery { .. }
            | ChatStructuredResult::ProbabilityAssessment { .. }
            | ChatStructuredResult::KnowledgeAnswer { .. }
            | ChatStructuredResult::ProvinceMajorList { .. }
            | ChatStructuredResult::MajorProvinceList { .. }
            | ChatStructuredResult::MajorDisambiguation { .. }
            | ChatStructuredResult::EvidenceBundle { .. }
            | ChatStructuredResult::GeneralAnswer { .. }
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect::<String>()
}

fn enrich_memory_from_message(memory: &mut ResolvedMemory, message: &str) {
    if let Some(score) = extract_score(message) {
        memory.score = Some(score);
    }
    if let Some(subject_type) = extract_subject_type(message) {
        memory.subject_type = Some(subject_type);
    }
    if let Some(province) = extract_known_province(message) {
        memory.province_name = Some(province);
    }
    if should_extract_major_phrase(message) {
        if let Some(major) = extract_major_phrase(message) {
            memory.major_name = Some(major.clone());
            memory.major_slug = Some(major);
        }
    }
}

fn should_extract_major_phrase(message: &str) -> bool {
    [
        "录取线",
        "分数线",
        "近三年",
        "能上",
        "能报",
        "培养方案",
        "培养目标",
        "毕业条件",
        "毕业要求",
        "毕业需要",
        "课程",
        "学分",
        "实践环节",
        "教育实习",
        "第二课堂",
        "毕业创作",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

fn should_update_major_from_knowledge_query(query: &str) -> bool {
    if is_admission_policy_query(query) {
        return false;
    }

    if contains_any_text(
        query,
        &[
            "校园",
            "大学生活",
            "学生生活",
            "食堂",
            "宿舍",
            "住宿",
            "社团",
            "学校介绍",
            "学校简介",
            "学校情况",
            "院校介绍",
        ],
    ) && extract_major_phrase(query).is_none()
    {
        return false;
    }

    should_extract_major_phrase(query)
        || (contains_any_text(
            query,
            &[
                "专业",
                "课程",
                "培养目标",
                "学分",
                "毕业要求",
                "实践环节",
                "培养方案",
            ],
        ) && extract_major_phrase(query).is_some())
}

fn is_admission_policy_query(query: &str) -> bool {
    contains_any_text(
        query,
        &[
            "招生简章",
            "招生章程",
            "录取规则",
            "专业志愿",
            "专业级差",
            "级差",
            "服从调剂",
            "调剂",
            "退档",
            "同分",
            "体检",
            "语种",
            "外语语种",
            "单科成绩",
            "选考科目",
            "选考",
            "选科",
            "招生计划",
            "招生电话",
            "咨询电话",
            "官网",
        ],
    ) || contains_policy_program_term(query)
}

fn extract_score(message: &str) -> Option<f64> {
    let chars = message.chars().collect::<Vec<_>>();
    for index in 0..chars.len() {
        if chars.get(index + 3) == Some(&'分') {
            let candidate = chars[index..index + 3].iter().collect::<String>();
            if let Ok(score) = candidate.parse::<f64>() {
                return Some(score);
            }
        }
    }
    None
}

fn extract_subject_type(message: &str) -> Option<String> {
    for item in ["物理类", "历史类", "理科", "文科", "未区分"] {
        if message.contains(item) {
            return Some(item.to_owned());
        }
    }
    None
}

fn extract_known_province(message: &str) -> Option<String> {
    const PROVINCES: &[&str] = &[
        "北京",
        "天津",
        "河北",
        "山西",
        "内蒙古",
        "辽宁",
        "吉林",
        "黑龙江",
        "上海",
        "江苏",
        "浙江",
        "安徽",
        "福建",
        "江西",
        "山东",
        "河南",
        "湖北",
        "湖南",
        "广东",
        "广西",
        "海南",
        "重庆",
        "四川",
        "贵州",
        "云南",
        "陕西",
        "甘肃",
        "青海",
        "宁夏",
        "新疆",
    ];
    PROVINCES
        .iter()
        .find(|province| message.contains(**province))
        .map(|province| (*province).to_owned())
}

fn extract_year_from_message(message: &str) -> Option<i32> {
    for year in 2021..=2039 {
        if message.contains(&year.to_string()) {
            return Some(year);
        }
    }
    None
}

fn extract_major_phrase(message: &str) -> Option<String> {
    for marker in [
        "录取线",
        "分数线",
        "近三年",
        "能上",
        "能报",
        "培养方案",
        "培养目标",
        "毕业条件",
        "毕业要求",
        "毕业需要",
        "教育实习",
        "实践环节",
        "第二课堂",
        "毕业创作",
        "课程",
        "学分",
    ] {
        if let Some(index) = message.find(marker) {
            let before = message[..index].trim_matches(['，', ',', '。', '？', '?', ' ']);
            let without_profile = before
                .replace("物理类", "")
                .replace("历史类", "")
                .replace("理科", "")
                .replace("文科", "");
            let without_province = strip_known_provinces(&without_profile);
            let cleaned_before = clean_major_candidate_text(&without_province);
            let cleaned = clean_major_candidate_text(
                cleaned_before
                    .split(['，', ',', ' '])
                    .next_back()
                    .unwrap_or("")
                    .trim(),
            );
            if is_plausible_major_text(&cleaned) {
                return Some(cleaned);
            }
            let after = message[index + marker.len()..].trim();
            let cleaned_after = clean_major_candidate_text(after);
            if is_plausible_major_text(&cleaned_after) {
                return Some(cleaned_after);
            }
        }
    }
    explicit_major_text(message)
}

fn explicit_major_text(message: &str) -> Option<String> {
    if looks_like_knowledge_follow_up(message) && !contains_any_text(message, &["换成", "改成"])
    {
        return None;
    }
    if let Some(major) = extract_switch_major_query(message) {
        return Some(major);
    }
    let mut text = message.to_owned();
    if let Some(score) = extract_score(message) {
        text = text.replace(&format!("{score:.0}分"), "");
    }
    for token in [
        "能上",
        "能不能上",
        "能报",
        "概率",
        "录取概率",
        "稳吗",
        "录取线",
        "分数线",
        "最低分",
        "近三年",
        "历年",
        "培养方案",
        "培养目标",
        "毕业条件",
        "毕业要求",
        "毕业需要",
        "讲一下",
        "介绍一下",
        "介绍",
        "一下",
        "是什么",
        "有没有",
        "那",
        "这个专业",
    ] {
        text = text.replace(token, "");
    }
    let cleaned = clean_major_candidate_text(&text);
    if is_plausible_major_text(&cleaned) {
        Some(cleaned)
    } else {
        None
    }
}

fn clean_major_candidate_text(text: &str) -> String {
    let mut cleaned = strip_known_provinces(text)
        .replace("物理类", "")
        .replace("历史类", "")
        .replace("理科", "")
        .replace("文科", "")
        .replace("专业的", "")
        .replace("这个专业", "")
        .replace("该专业", "")
        .replace("主要", "")
        .replace("核心", "");
    for token in ["需要", "多少", "要求", "讲", "一下", "呢", "说说", "再说说"] {
        cleaned = cleaned.replace(token, "");
    }
    for year in 2021..=2039 {
        cleaned = cleaned.replace(&year.to_string(), "");
    }
    cleaned
        .replace("到", "")
        .replace("至", "")
        .trim_end_matches("专业")
        .trim_matches([
            '，', ',', '。', '？', '?', '！', '!', ' ', '呢', '吗', '啊', '：', ':',
        ])
        .to_owned()
}

fn is_plausible_major_text(text: &str) -> bool {
    text.chars().count() >= 2
        && text.chars().count() <= 24
        && !text.chars().all(|ch| ch.is_ascii_digit())
        && !text.contains('分')
        && !text.contains("能上")
        && !text.contains("能不能")
        && !text.contains("能报")
        && !text.contains("概率")
        && !text.contains("稳吗")
        && !text.contains("稳不稳")
        && !text.contains("招生")
        && !text.contains("是什么")
        && !text.contains("有没有")
        && !matches!(text, "主要" | "这个" | "该专业" | "专业" | "一下")
        && !looks_like_knowledge_follow_up(text)
        && !contains_any_text(
            text,
            &[
                "主要课程",
                "课程有哪些",
                "毕业要求",
                "毕业条件",
                "实践环节",
                "学分要求",
                "学分结构",
                "培养目标",
                "怎么安排",
            ],
        )
        && !text.contains("专业有哪些")
        && !text.contains("什么专业")
        && !text.contains("哪些专业")
        && !contains_policy_program_term(text)
}

fn contains_any_text(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn strip_known_provinces(text: &str) -> String {
    const PROVINCES: &[&str] = &[
        "北京",
        "天津",
        "河北",
        "山西",
        "内蒙古",
        "辽宁",
        "吉林",
        "黑龙江",
        "上海",
        "江苏",
        "浙江",
        "安徽",
        "福建",
        "江西",
        "山东",
        "河南",
        "湖北",
        "湖南",
        "广东",
        "广西",
        "海南",
        "重庆",
        "四川",
        "贵州",
        "云南",
        "陕西",
        "甘肃",
        "青海",
        "宁夏",
        "新疆",
    ];
    PROVINCES.iter().fold(text.to_owned(), |current, province| {
        current.replace(province, "")
    })
}

fn major_alias_matches(left: &str, right: &str) -> bool {
    let left = normalize_major_alias(left);
    let right = normalize_major_alias(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right || left.contains(&right) || right.contains(&left))
}

fn normalize_major_alias(text: &str) -> String {
    strip_known_provinces(text)
        .replace(['（', '）', '(', ')', ' ', '，', ',', '、'], "")
        .replace("物理类", "")
        .replace("历史类", "")
        .replace("理科", "")
        .replace("文科", "")
        .replace("师范类", "")
        .replace("师范", "")
        .replace("专业", "")
}

fn missing_score_fields(memory: &ResolvedMemory) -> Vec<String> {
    let mut fields = Vec::new();
    if memory.province_name.is_none() && memory.province_code.is_none() {
        fields.push("province".to_owned());
    }
    if memory.major_name.is_none() && memory.major_slug.is_none() {
        fields.push("major".to_owned());
    }
    fields
}

fn missing_probability_fields(memory: &ResolvedMemory) -> Vec<String> {
    let mut fields = missing_score_fields(memory);
    if memory.subject_type.is_none() {
        fields.push("subjectType".to_owned());
    }
    if memory.score.is_none() {
        fields.push("score".to_owned());
    }
    fields
}

fn effective_probability_missing_fields(message: &str, memory: &ResolvedMemory) -> Vec<String> {
    let mut fields = missing_probability_fields(memory);
    if fields.len() == 1
        && fields.first().is_some_and(|field| field == "subjectType")
        && (extract_score(message).is_some()
            || memory.score.is_some()
                && matches!(
                    memory.pending_intent,
                    Some(ChatIntent::ProbabilityAssessment)
                ))
        && explicit_major_text(message).is_some()
    {
        fields.clear();
    }
    fields
}

fn render_follow_up(missing: &[String], memory: &ResolvedMemory) -> String {
    let labels = missing
        .iter()
        .map(|field| match field.as_str() {
            "province" => "省份",
            "subjectType" => "科类/选科",
            "score" => "分数",
            "major" => "意向专业",
            _ => field,
        })
        .collect::<Vec<_>>();
    let mut confirmed = Vec::new();
    if let Some(province) = memory
        .province_name
        .as_deref()
        .or(memory.province_code.as_deref())
    {
        confirmed.push(format!("省份是{province}"));
    }
    if let Some(subject_type) = memory.subject_type.as_deref() {
        confirmed.push(format!("科类/选科是{subject_type}"));
    }
    if let Some(score) = memory.score {
        confirmed.push(format!("分数是{score:.0}分"));
    }
    if let Some(major) = memory
        .major_name
        .as_deref()
        .or(memory.major_slug.as_deref())
    {
        confirmed.push(format!("意向专业是{major}"));
    }

    let confirmed_text = if confirmed.is_empty() {
        String::new()
    } else {
        format!("我先记下：{}。", confirmed.join("，"))
    };
    let task_hint = if missing.iter().any(|field| field == "subjectType")
        && memory.province_name.is_some()
        && memory.major_name.is_none()
    {
        "这样我才能按对应科类继续查这个省份当年招生专业。"
    } else {
        "这样我才能继续给你查录取线、概率或专业资料。"
    };
    format!(
        "{confirmed_text}还需要你补充{}，{task_hint}",
        labels.join("、")
    )
}

fn build_redirect_prompt(memory: &ResolvedMemory) -> String {
    if memory.province_name.is_some() && memory.subject_type.is_some() && memory.score.is_some() {
        "如果你愿意，我也可以立刻回到招生咨询，结合你的省份、科类/选科和分数继续帮你筛专业或评估具体专业录取概率。".to_owned()
    } else {
        "如果你愿意，也可以告诉我省份、科类/选科、分数、位次和意向专业，我继续帮你看录取概率或近三年分数线。".to_owned()
    }
}

fn citations_from_structured_result(result: &ChatStructuredResult) -> Vec<ChatCitation> {
    match result {
        ChatStructuredResult::ScoreQuery { records, .. } => records
            .iter()
            .take(3)
            .map(|record| ChatCitation {
                year: Some(record.year),
                source_label: record.source_label.clone(),
                source_url: record.source_url.clone(),
            })
            .collect(),
        ChatStructuredResult::ProvinceMajorList { majors, .. } => {
            let mut seen = HashSet::new();
            majors
                .iter()
                .take(3)
                .filter_map(|record| {
                    let key = format!("{}:{}", record.year, record.source_label);
                    if seen.insert(key) {
                        Some(ChatCitation {
                            year: Some(record.year),
                            source_label: record.source_label.clone(),
                            source_url: None,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        }
        ChatStructuredResult::MajorProvinceList { provinces, .. } => {
            let mut seen = HashSet::new();
            provinces
                .iter()
                .take(3)
                .filter_map(|record| {
                    let key = format!("{}:{}", record.year, record.source_label);
                    if seen.insert(key) {
                        Some(ChatCitation {
                            year: Some(record.year),
                            source_label: record.source_label.clone(),
                            source_url: None,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn render_province_major_list_answer(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::ProvinceMajorList {
        province,
        subject_type,
        year,
        majors,
        note,
        ..
    } = result
    else {
        return "我已经查询了分省专业信息。".to_owned();
    };

    if majors.is_empty() {
        let subject_text = subject_type
            .as_deref()
            .map(|value| format!("{value}"))
            .unwrap_or_default();
        return format!(
            "我查了已导入的分省录取统计，暂时没有找到{province}{subject_text}对应的专业记录。这里不能直接判断为学校不在该省招生，建议以当年省级招生计划和学校招生章程为准。"
        );
    }

    let list = majors
        .iter()
        .take(60)
        .map(|item| {
            let count = item
                .admitted_count
                .map(|value| format!("，录取{value}人"))
                .unwrap_or_default();
            let score = item
                .min_score
                .map(|value| format!("，最低分{value}"))
                .unwrap_or_default();
            format!("{}（{}{}{}）", item.major_name, item.batch, count, score)
        })
        .collect::<Vec<_>>()
        .join("、");
    let more = if majors.len() > 60 {
        format!("等 {} 个专业/方向", majors.len())
    } else {
        format!("共 {} 个专业/方向", majors.len())
    };
    let subject_text = subject_type
        .as_deref()
        .map(|value| format!("{value}"))
        .unwrap_or_else(|| "未区分科类".to_owned());
    let year_text = year
        .map(|value| value.to_string())
        .unwrap_or_else(|| "最新一年".to_owned());

    format!(
        "我查到已导入录取统计中，{province}{year_text}年（{subject_text}）有录取记录的专业/方向{more}：{list}。\n\n需要说明：{note}如果你要填报志愿，最终仍要以当年山东省教育招生考试院公布的招生计划和学校官方招生章程为准。"
    )
}

fn render_major_province_list_answer(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::MajorProvinceList {
        major_name,
        subject_type,
        year,
        provinces,
        note,
        ..
    } = result
    else {
        return "我已经查询了专业覆盖省份信息。".to_owned();
    };

    if provinces.is_empty() {
        let subject_text = subject_type
            .as_deref()
            .map(|value| format!("{value}"))
            .unwrap_or_default();
        return format!(
            "我查了已导入的分省录取统计，暂时没有找到{major_name}{subject_text}对应的省份录取记录。这里不能直接判断为学校不招该专业，建议以当年省级招生计划和学校招生章程为准。"
        );
    }

    let list = provinces
        .iter()
        .take(80)
        .map(|item| {
            let count = item
                .admitted_count
                .map(|value| format!("，录取{value}人"))
                .unwrap_or_default();
            let score = item
                .min_score
                .map(|value| format!("，最低分{value}"))
                .unwrap_or_default();
            format!("{}（{}{}{}）", item.province_name, item.batch, count, score)
        })
        .collect::<Vec<_>>()
        .join("、");
    let subject_text = subject_type
        .as_deref()
        .map(|value| format!("{value}"))
        .unwrap_or_else(|| "未区分科类".to_owned());
    let year_text = year
        .map(|value| value.to_string())
        .unwrap_or_else(|| "最新一年".to_owned());

    format!(
        "我查到已导入录取统计中，{major_name}在{year_text}年（{subject_text}）有录取记录的省份/地区共 {} 个：{list}。\n\n需要说明：{note}如果你要填报志愿，最终仍要以所在省教育考试院公布的招生计划和学校官方招生章程为准。",
        provinces.len()
    )
}

fn render_score_answer(result: &ChatStructuredResult) -> String {
    let ChatStructuredResult::ScoreQuery {
        province,
        subject_type,
        records,
        ..
    } = result
    else {
        return "我已经查询了录取统计。".to_owned();
    };

    if records.is_empty() {
        return format!(
            "我查了已导入的 2021-2025 年录取统计，暂时没有找到{province}对应专业的分专业录取记录。你可以换一个专业名称，或补充专业全称我再查。"
        );
    }
    let latest = &records[0];
    format!(
        "根据已导入的历年录取统计，{}{}该专业 {} 年最低分为 {} 分。这个结果来自录取统计表，具体填报仍要结合当年招生计划和省级招生主管部门公布信息。",
        province,
        subject_type
            .as_ref()
            .map(|value| format!(" {value} "))
            .unwrap_or_else(|| " ".to_owned()),
        latest.year,
        latest.min_score
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_reply_without_losing_text() {
        let reply = "可以查到。这里是第二句。";
        assert_eq!(chunk_reply_text(reply).join(""), reply);
    }

    #[test]
    fn redirect_uses_generic_slots() {
        let memory = ResolvedMemory {
            province_name: Some("河北".to_owned()),
            subject_type: Some("历史类".to_owned()),
            score: Some(500.0),
            major_name: Some("汉语言文学".to_owned()),
            ..ResolvedMemory::default()
        };
        let prompt = build_redirect_prompt(&memory);
        assert!(prompt.contains("你的省份"));
        assert!(!prompt.contains("河北"));
        assert!(!prompt.contains("汉语言文学"));
    }

    #[test]
    fn score_probability_follow_up_is_not_a_major_switch() {
        assert_eq!(extract_switch_major_query("500分能上吗？"), None);
        assert_eq!(extract_major_phrase("500分能上吗？"), None);
        assert!(asks_province_admission_major_list(
            "哈师大在山东招生哪些专业"
        ));
        assert!(asks_province_admission_major_list(
            "我问的是哈师大在山东招哪些专业"
        ));
        assert!(asks_province_admission_major_list("山东招什么专业？"));
        assert!(asks_major_admission_province_list(
            "物联网工程在哪些省份有招生？"
        ));
        assert!(asks_major_admission_province_list(
            "英语师范类在哪些地区有录取记录？"
        ));
    }

    #[test]
    fn training_plan_follow_up_inherits_major_from_history_user_message() {
        let mut memory = ResolvedMemory::default();
        let history = vec![ConversationMessage {
            role: "user".to_owned(),
            content: "数学与应用数学培养方案讲一下".to_owned(),
            structured_payload: None,
            citations: Vec::new(),
            created_at: None,
        }];

        enrich_memory_from_history(&mut memory, &history);

        assert_eq!(memory.major_name.as_deref(), Some("数学与应用数学"));
        assert_eq!(
            contextual_knowledge_query("主要课程呢？", &memory),
            "数学与应用数学 主要课程呢？"
        );
        assert_eq!(
            contextual_knowledge_query("音乐学专业的教育实习和实践环节怎么安排？", &memory),
            "音乐学专业的教育实习和实践环节怎么安排？"
        );
        assert_eq!(
            contextual_knowledge_query_with_history(
                "毕业要求呢？",
                &ResolvedMemory::default(),
                &history
            ),
            "数学与应用数学 毕业要求呢？"
        );
        assert_eq!(
            contextual_knowledge_query_with_history(
                "音乐学专业的教育实习和实践环节怎么安排？",
                &ResolvedMemory::default(),
                &history
            ),
            "音乐学专业的教育实习和实践环节怎么安排？"
        );
        assert_eq!(
            extract_major_phrase("数学与应用数学 主要课程呢？").as_deref(),
            Some("数学与应用数学")
        );
        assert_eq!(extract_major_phrase("主要课程呢？"), None);
        assert_eq!(
            extract_major_phrase("山东数据科学与大数据技术2021到2025录取线").as_deref(),
            Some("数据科学与大数据技术")
        );
        assert_eq!(extract_major_phrase("毕业条件需要多少学分？"), None);
        assert_eq!(extract_major_phrase("这个专业培养目标讲一下"), None);
        assert_eq!(
            extract_major_phrase("数据科学与大数据技术 毕业条件需要多少学分？").as_deref(),
            Some("数据科学与大数据技术")
        );
        assert_eq!(
            extract_major_phrase("音乐学专业的教育实习和实践环节怎么安排？").as_deref(),
            Some("音乐学")
        );
    }
}
