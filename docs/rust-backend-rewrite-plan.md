# Rust 重构招生 Agent 项目计划：以更高质量架构重建，而不是翻译旧项目

## 1. 目标与原则

本项目在 `/home/scm2002/Code/rust_enrollment` 中重构招生 Agent。旧项目只作为数据、接口、测试和失败经验来源，不做机械翻译，也不复刻其中不合理的 agent 行为。

核心目标：

- 前端复制当前 Next.js 应用，并保持 API 兼容。
- 后端采用 Axum、Tokio、sqlx、Postgres/pgvector。
- Agent 采用自定义 runtime，重点优化异步并发、工具强制调用、上下文压缩、证据分级、短期记忆、流式输出和可观测性。
- harness 迁移 v1-v4，并新增更普适的招生、分数、培养方案、简章、短期记忆和压缩测试。

必须坚持的设计原则：

- 分数线、录取概率、招生简章、培养方案、FAQ、学院专业目录类问题，runtime 必须保证至少走确定性工具或检索工具。
- Excel 表格、专业目录、学院专业列表、FAQ、PDF chunk、短期记忆各自有明确工具和优先级，不能全部丢给向量检索。
- Prompt 只负责推理和表达，事实边界由 runtime、tool contract、evidence grading、fallback guard 保证。
- 检索结果不能直接按前 8 条塞给 LLM，必须先去重、过滤、降噪和分级。
- 最终回答面向学生和家长，避免“有资料”“字段聚合”“知识库命中”等后台口吻。
- SSE 不做“完整回答后切块”，后续要支持阶段级和 synthesis token 级真实流式。
- 本地模型 32K 上下文下必须有快速、确定、可诊断的压缩策略。

## 2. 项目结构

```text
rust_enrollment/
  apps/
    web/                  # 复制当前前端
    api/                  # Axum 服务入口
  crates/
    domain/               # 领域类型与结构化结果
    db/                   # sqlx/Postgres/pgvector 查询层
    llm/                  # DashScope/OpenAI-compatible/local model client
    embeddings/           # local embedding client
    retrieval/            # FAQ/PDF/Excel/专业目录检索与证据分级
    agent_runtime/        # 自定义 runtime：状态机、并发、预算、trace
    admissions_agent/     # router、memory、prompts、synthesis
    eval_harness/         # v1-v4 回归与新增测试
    importers/            # 后续迁移导入脚本
```

默认技术栈：

- HTTP：`axum` + `tokio`
- DB：`sqlx` + Postgres + pgvector
- JSON/schema：`serde` / `serde_json`
- 日志：`tracing` + `tower-http`
- SSE：Axum `Sse`
- 测试：`cargo test`、`rstest`、`wiremock`、`insta`、integration harness

## 3. 分阶段实施

### Phase 0：基线冻结与资产搬迁

- 不修改旧项目。
- 复制前端到 `apps/web`。
- 复制 v1-v4 fixture 到 `crates/eval_harness/fixtures/`。
- 记录旧 API contract：`/api/v1/chat`、`/api/v1/chat/stream`、`/api/v1/chat/history/:conversationId`、admission、knowledge、health。

### Phase 1：Axum API Skeleton

- 建立 Axum 服务：
  - `GET /api/v1/health`
  - `POST /api/v1/chat`
  - `POST /api/v1/chat/stream`
  - `GET /api/v1/chat/history/:conversation_id`
- 保持前端 envelope 兼容：

```json
{
  "success": true,
  "data": {},
  "meta": {},
  "error": null
}
```

- 中间件包含 request id、CORS、timeout、body limit、错误映射、structured logging。

### Phase 2：DB 与领域模型

- 先复用当前 Postgres schema，不急着重建迁移。
- `domain` 定义 `ChatRequest`、`ChatReply`、`ChatCitation`、`ChatStructuredResult`、`ResolvedMemory`、`AgentTrace`、`ToolObservation`、`EvidenceBundle`。
- `db` 实现专业、省份、录取分数、招生计划、FAQ、policy、knowledge chunk、conversation/history/memory 查询。

### Phase 3：确定性工具优先

- 分数线查询必须查 Excel 入库表，不允许 LLM 自答。
- 录取概率必须基于真实分数记录和明确画像。
- 学院专业列表从培养方案 metadata 聚合，覆盖“某学院有哪些专业”。
- 专业候选工具支持简称、方向词、学院词、专业大类。
- PDF/FAQ 检索支持招生简章 filter、培养方案 filter、专业/学院/年份强过滤、OCR 噪声降权。

### Phase 4：自定义 Agent Runtime

Runtime 采用 typed state machine：

```text
RouterNode
  -> ContextResolutionNode
  -> RetrievalPlanNode
  -> ReActToolLoopNode
  -> EvidenceGradingNode
  -> ContextCompressionNode
  -> SynthesisNode
  -> MemoryWriteNode
```

关键约束：

- 每个 node 有 typed input/output、耗时、trace、错误状态。
- 只读工具可并发执行：FAQ、vector、policy、候选专业、相关分数组。
- 有依赖的工具串行：先解析省份/专业，再查分数/概率。
- 每个工具有 timeout、retry、max concurrency、budget。
- 如果 knowledge route 返回空证据，runtime 必须触发 fallback 检索或确定性工具，不能直接让 LLM 说“没有资料”。
- 最大 ReAct steps 默认 5；超限时进入安全 synthesis，不继续空转。

### Phase 5：上下文工程与压缩

32K 本地模型默认策略：

- 低于 72%：不压缩。
- 72%-85%：soft compression，保留最近 8 条原文。
- 高于 85%：hard compression，保留最近 6 条原文，旧上下文压到约 30%。
- 压缩摘要采用 deterministic structured summary，优先快和稳定。
- 压缩内容包含 confirmed profile、confirmed major/province/subject/score/rank、pending intent、最近证据引用、用户纠正过的信息。
- 历史 chunk 不当事实库；需要事实时重新查工具。

### Phase 6：提示词工程与回答风格

提示词分层：

- router prompt：只判意图。
- tool planning prompt：明确哪些问题必须查工具。
- evidence grading prompt：判断证据是否相关、错专业、错学院、错年份。
- synthesis prompt：自然回答，面向学生和家长。

回答原则：

- 不编造。
- 不说后台词。
- 不把“培养方案覆盖专业”说成“当年一定招生专业”。
- 不把相关专业分数线说成目标专业分数线。
- 缺省份、科类、分数、专业时自然追问。
- 有边界但不冷冰冰。

### Phase 7：真实流式与观测层

- `/chat/stream` 保持旧前端兼容事件：`chunk`、`message`、`done`。
- 新增可选状态事件：`status: resolving`、`status: retrieving`、`status: generating`。
- synthesis 支持 token/delta 真流式。
- tracing 字段：conversation_id、route_intent、node、tool_name、model、duration_ms、token_estimate、compression_level、error_class。
- 后续可新增 `agent_runs`、`agent_trace_steps`、`agent_feedback`。

### Phase 8：Harness 与质量闭环

搬迁并运行：

- v1：基础多轮回归
- v2：上下文与分数线回归
- v3：招生简章/培养方案 live 回归
- v4：课程与跨任务切换 live 回归

新增测试：

- 学院专业列表：音乐学院、教育科学学院、地理科学学院、美术学院。
- Excel 普适性：不同省份、科类、艺术类、普通类、未区分。
- PDF 培养方案：主要课程、课程有没有、学分结构、毕业要求、实践环节。
- 招生简章：调剂、同分、体检、语种、艺术类录取规则。
- 短期记忆：跨 3-5 轮继承、省份/分数/专业纠正。
- 空结果：必须说明数据边界，不能误判“没有这个专业”。
- 流式：SSE 事件顺序和最终 message 一致。
- 压缩：soft/hard 后不丢 confirmed context。

Harness 输出 JSON 报告，至少包含 case、turn、expected_type、actual_type、reply_checks、tool_calls、latency_ms、compression。

## 4. Public Interfaces

保持前端兼容：

- `POST /api/v1/chat`
  - request：`{ conversationId?, message, profile? }`
  - response：`{ conversationId, reply, structuredResult, citations, diagnostics? }`
- `POST /api/v1/chat/stream`
  - request 同 `/chat`
  - SSE 兼容旧事件，允许新增 status
- `GET /api/v1/chat/history/:conversationId`
- admission/knowledge 查询接口优先保持旧路径和 envelope。

## 5. Acceptance Criteria

- 前端复制后能直接连 Rust API 使用。
- v1-v4 harness 可运行；offline 先全绿，live 分阶段全绿。
- 分数线/概率问题一定使用表格数据工具。
- 招生简章/培养方案/FAQ 问题一定经过检索或确定性工具。
- “学院有哪些专业”不再假阴性，且回答自然。
- 长对话压缩后不丢省份、科类、分数、专业、pending intent。
- SSE 至少 synthesis 阶段真流式。
- 每轮回答可通过 trace 看到 route、tools、LLM、压缩、耗时和失败原因。

## 6. Assumptions

- 旧项目只读参考，不修改。
- Rust 后端先复用当前数据库和已导入数据。
- 前端只改必要配置。
- embedding 服务默认仍是 `http://127.0.0.1:8114/v1/embeddings`，1024 维。
- LLM 默认 OpenAI-compatible DashScope，agent/synthesis 默认非思考模式。
