"use client";

import { useEffect, useMemo, useState, useTransition } from "react";

import {
  createAdminFaq,
  createAdminFeedback,
  createAdminPolicy,
  importAdmissionScores,
  listAdminConversations,
  listAdminFaq,
  listAdminImportBatches,
  listAdminPolicies,
  rollbackAdmissionScoreImportBatch,
  updateAdminFaq,
  updateAdminPolicy,
  type AdminConversation,
  type AdminConversationCorrectionDraft,
  type AdminConversationStatus,
  type AdminFaq,
  type AdminFaqInput,
  type AdminImportBatch,
  type AdminImportBatchSummary,
  type AdminPolicy,
  type AdminPolicyInput,
  type AdmissionScoreImportPayload,
  type AdmissionScoreImportResult,
  type AdmissionScoreImportRowInput
} from "@/lib/api-client";
import { cn } from "@/lib/cn";

type AdminShellProps = {
  title?: string;
};

type KnowledgeMode = "faq" | "policies";

type KnowledgeDraft = {
  faq: AdminFaqInput;
  policy: AdminPolicyInput;
};

type ImportFormState = {
  sourceFileName: string;
  sourceLabel: string;
  dataVersion: string;
  importedBy: string;
  rowsText: string;
};

const defaultImportRows = JSON.stringify(
  [
    {
      universityCode: "HNNU",
      majorSlug: "hanyuyanwenxue",
      provinceCode: "230000",
      year: 2025,
      batch: "本科批",
      subjectType: "历史类",
      minScore: 560,
      avgScore: 565,
      maxScore: 571,
      minRank: 7900,
      avgRank: 7700,
      maxRank: 7410,
      sourceUrl: "https://example.edu.cn/admission/2025-hlj-hanyuyan"
    }
  ],
  null,
  2
);

const defaultImportFormState: ImportFormState = {
  sourceFileName: "admission-scores-2025.json",
  sourceLabel: "2025 黑龙江录取数据",
  dataVersion: "admin-ui-preview",
  importedBy: "web-admin",
  rowsText: defaultImportRows
};

const defaultKnowledgeDraft: KnowledgeDraft = {
  faq: {
    question: "",
    answer: "",
    category: "招生咨询",
    tags: [],
    status: "draft",
    sourceLabel: "管理后台录入"
  },
  policy: {
    title: "",
    category: "章程",
    year: new Date().getFullYear(),
    sourceUrl: "",
    contentText: "",
    publishedAt: "",
    status: "active"
  }
};

const defaultCorrectionDraft: AdminConversationCorrectionDraft = {
  feedbackType: "manual-fix",
  note: "",
  resolution: ""
};

function StatCard({
  label,
  value,
  helper
}: {
  label: string;
  value: string;
  helper: string;
}) {
  return (
    <div className="rounded-[1.5rem] border border-slate-200 bg-white p-5 shadow-sm">
      <p className="text-sm text-ink-500">{label}</p>
      <p className="mt-3 text-3xl font-semibold text-ink-900">{value}</p>
      <p className="mt-2 text-sm leading-6 text-ink-600">{helper}</p>
    </div>
  );
}

function SectionCard({
  title,
  description,
  children
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-[2rem] border border-slate-200 bg-white shadow-soft">
      <div className="border-b border-slate-200 px-6 py-5 sm:px-8">
        <p className="text-xl font-semibold text-ink-900">{title}</p>
        {description ? <p className="mt-2 text-sm leading-6 text-ink-600">{description}</p> : null}
      </div>
      <div className="px-6 py-5 sm:px-8">{children}</div>
    </section>
  );
}

function InlineState({
  tone = "neutral",
  text
}: {
  tone?: "neutral" | "error" | "success";
  text: string;
}) {
  const toneClass =
    tone === "error"
      ? "border-rose-200 bg-rose-50 text-rose-800"
      : tone === "success"
        ? "border-emerald-200 bg-emerald-50 text-emerald-800"
        : "border-slate-200 bg-slate-50 text-ink-700";

  return (
    <div className={cn("rounded-2xl border px-4 py-3 text-sm", toneClass)}>
      <p>{text}</p>
    </div>
  );
}

function EmptyState({
  title,
  description
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="rounded-[1.5rem] border border-dashed border-slate-200 bg-slate-50 px-5 py-6 text-sm text-ink-600">
      <p className="font-semibold text-ink-800">{title}</p>
      <p className="mt-2 leading-6">{description}</p>
    </div>
  );
}

function formatDateTime(value: string | null | undefined) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString("zh-CN", { hour12: false });
}

function safeStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
}

function parseRowJson(text: string) {
  let parsed: unknown;

  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(error instanceof Error ? error.message : "JSON 解析失败");
  }

  if (!Array.isArray(parsed)) {
    throw new Error("导入 rows 必须是 JSON 数组");
  }

  return parsed as AdmissionScoreImportRowInput[];
}

function normalizeFaqDraft(item?: Partial<AdminFaq>): AdminFaqInput {
  return {
    question: item?.question ?? "",
    answer: item?.answer ?? "",
    category: item?.category ?? "招生咨询",
    tags: item?.tags ?? [],
    status: item?.status ?? "draft",
    sourceLabel: item?.sourceLabel ?? "管理后台录入"
  };
}

function normalizePolicyDraft(item?: Partial<AdminPolicy>): AdminPolicyInput {
  return {
    title: item?.title ?? "",
    category: item?.category ?? "章程",
    year: item?.year ?? new Date().getFullYear(),
    sourceUrl: item?.sourceUrl ?? "",
    contentText: item?.contentText ?? "",
    publishedAt: item?.publishedAt ?? "",
    status: item?.status ?? "active"
  };
}

function summaryCount(summary: AdminImportBatchSummary | null, key: keyof AdminImportBatchSummary) {
  const value = summary?.[key];
  return typeof value === "number" ? String(value) : "—";
}

export function AdminShell({ title = "管理后台" }: AdminShellProps) {
  const [importBatches, setImportBatches] = useState<AdminImportBatch[]>([]);
  const [importResult, setImportResult] = useState<AdmissionScoreImportResult | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const [importForm, setImportForm] = useState<ImportFormState>(defaultImportFormState);
  const [rollingBackBatchId, setRollingBackBatchId] = useState<string | null>(null);
  const [faqItems, setFaqItems] = useState<AdminFaq[]>([]);
  const [faqError, setFaqError] = useState<string | null>(null);
  const [faqSaving, setFaqSaving] = useState(false);
  const [selectedFaqId, setSelectedFaqId] = useState<string | null>(null);
  const [knowledgeMode, setKnowledgeMode] = useState<KnowledgeMode>("faq");
  const [knowledgeDraft, setKnowledgeDraft] = useState<KnowledgeDraft>(defaultKnowledgeDraft);
  const [policies, setPolicies] = useState<AdminPolicy[]>([]);
  const [policyError, setPolicyError] = useState<string | null>(null);
  const [selectedPolicyId, setSelectedPolicyId] = useState<string | null>(null);
  const [policySavingMessage, setPolicySavingMessage] = useState<string | null>(null);
  const [policySaving, setPolicySaving] = useState(false);
  const [conversations, setConversations] = useState<AdminConversation[]>([]);
  const [conversationError, setConversationError] = useState<string | null>(null);
  const [selectedConversationId, setSelectedConversationId] = useState<string | null>(null);
  const [conversationStatusFilter, setConversationStatusFilter] = useState<AdminConversationStatus | "all">("all");
  const [conversationProvinceFilter, setConversationProvinceFilter] = useState("");
  const [correctionDrafts, setCorrectionDrafts] = useState<Record<string, AdminConversationCorrectionDraft>>({});
  const [feedbackLog, setFeedbackLog] = useState<Record<string, string>>({});
  const [feedbackSaving, setFeedbackSaving] = useState<string | null>(null);
  const [isHydrated, setIsHydrated] = useState(false);
  const [isPending, startTransition] = useTransition();

  useEffect(() => {
    setIsHydrated(true);

    startTransition(() => {
      void Promise.allSettled([
        listAdminImportBatches()
          .then(setImportBatches)
          .catch((error) => {
            setImportError(error instanceof Error ? error.message : "导入批次加载失败");
          }),
        listAdminFaq()
          .then((items) => {
            setFaqItems(items);
            if (items[0]) {
              setSelectedFaqId((current) => current ?? items[0]?.id ?? null);
              setKnowledgeDraft((current) => ({
                ...current,
                faq: normalizeFaqDraft(items[0])
              }));
            }
          })
          .catch((error) => {
            setFaqError(error instanceof Error ? error.message : "FAQ 列表加载失败");
          }),
        listAdminPolicies()
          .then((items) => {
            setPolicies(items);
            if (items[0]) {
              setSelectedPolicyId((current) => current ?? items[0]?.id ?? null);
              setKnowledgeDraft((current) => ({
                ...current,
                policy: normalizePolicyDraft(items[0])
              }));
            }
          })
          .catch((error) => {
            setPolicyError(error instanceof Error ? error.message : "政策列表加载失败");
          }),
        listAdminConversations()
          .then((items) => {
            setConversations(items);
            setSelectedConversationId((current) => current ?? items[0]?.id ?? null);
          })
          .catch((error) => {
            setConversationError(error instanceof Error ? error.message : "咨询日志加载失败");
          })
      ]);
    });
  }, []);

  const filteredFaqItems = useMemo(() => faqItems, [faqItems]);

  const filteredPolicies = useMemo(() => policies, [policies]);

  const filteredConversations = useMemo(() => {
    return conversations.filter((item) => {
      if (conversationStatusFilter !== "all" && item.status !== conversationStatusFilter) {
        return false;
      }

      if (conversationProvinceFilter.trim() && item.provinceCode !== conversationProvinceFilter.trim()) {
        return false;
      }

      return true;
    });
  }, [conversationProvinceFilter, conversationStatusFilter, conversations]);

  const selectedBatch = importBatches[0] ?? null;
  const selectedFaq = faqItems.find((item) => item.id === selectedFaqId) ?? null;
  const selectedPolicy = policies.find((item) => item.id === selectedPolicyId) ?? null;
  const selectedConversation =
    filteredConversations.find((item) => item.id === selectedConversationId) ??
    conversations.find((item) => item.id === selectedConversationId) ??
    null;

  const statCards = useMemo(() => {
    const openConversations = conversations.filter((item) => item.status === "open").length;
    const pendingFaq = faqItems.filter((item) => item.status === "draft").length;
    const activePolicies = policies.filter((item) => item.status === "active").length;
    const latestBatchLabel = importBatches[0]
      ? `${importBatches[0].status.toLowerCase()} · ${summaryCount(importBatches[0].summary, "acceptedRows")}`
      : "暂无批次";

    return [
      {
        label: "导入批次",
        value: String(importBatches.length),
        helper: latestBatchLabel
      },
      {
        label: "FAQ 条目",
        value: String(faqItems.length),
        helper: pendingFaq > 0 ? `${pendingFaq} 条草稿待确认` : "当前无草稿积压"
      },
      {
        label: "政策条目",
        value: String(policies.length),
        helper: `${activePolicies} 条处于启用状态`
      },
      {
        label: "咨询日志",
        value: String(conversations.length),
        helper: openConversations > 0 ? `${openConversations} 条待人工处理` : "当前已全部处理"
      }
    ];
  }, [conversations, faqItems, importBatches, policies]);

  async function refreshImportBatches() {
    try {
      setImportBatches(await listAdminImportBatches());
      setImportError(null);
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "导入批次刷新失败");
    }
  }

  async function refreshFaq() {
    try {
      const items = await listAdminFaq();
      setFaqItems(items);
      setFaqError(null);
      if (selectedFaqId) {
        const match = items.find((item) => item.id === selectedFaqId);
        if (match) {
          setKnowledgeDraft((current) => ({
            ...current,
            faq: normalizeFaqDraft(match)
          }));
        }
      }
    } catch (error) {
      setFaqError(error instanceof Error ? error.message : "FAQ 刷新失败");
    }
  }

  async function refreshPolicies() {
    try {
      setPolicies(await listAdminPolicies());
      setPolicyError(null);
    } catch (error) {
      setPolicyError(error instanceof Error ? error.message : "政策列表刷新失败");
    }
  }

  async function refreshConversations() {
    try {
      setConversations(
        await listAdminConversations({
          ...(conversationProvinceFilter.trim() ? { provinceCode: conversationProvinceFilter.trim() } : {}),
          ...(conversationStatusFilter !== "all" ? { status: conversationStatusFilter } : {})
        })
      );
      setConversationError(null);
    } catch (error) {
      setConversationError(error instanceof Error ? error.message : "咨询日志刷新失败");
    }
  }

  async function handleImportSubmit(dryRun: boolean) {
    setImportError(null);
    setImportResult(null);

    let rows: AdmissionScoreImportRowInput[];
    try {
      rows = parseRowJson(importForm.rowsText);
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "导入 JSON 解析失败");
      return;
    }

    try {
      const payload: AdmissionScoreImportPayload = {
        sourceFileName: importForm.sourceFileName.trim(),
        sourceLabel: importForm.sourceLabel.trim(),
        dataVersion: importForm.dataVersion.trim(),
        importedBy: importForm.importedBy.trim(),
        dryRun,
        rows
      };

      const result = await importAdmissionScores(payload);
      setImportResult(result);
      if (!dryRun) {
        await refreshImportBatches();
      }
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "导入执行失败");
    }
  }

  async function handleRollback(batchId: string) {
    setRollingBackBatchId(batchId);
    setImportError(null);

    try {
      const result = await rollbackAdmissionScoreImportBatch(batchId);
      setImportResult({
        batchId: result.batch.id,
        persisted: true,
        status: "completed",
        summary: {
          totalRows: typeof result.batch.summary?.totalRows === "number" ? result.batch.summary.totalRows : 0,
          acceptedRows:
            typeof result.batch.summary?.acceptedRows === "number" ? result.batch.summary.acceptedRows : 0,
          rejectedRows:
            typeof result.batch.summary?.rejectedRows === "number" ? result.batch.summary.rejectedRows : 0
        },
        rowErrors: [],
        acceptedPreviewRows: [],
        batch: result.batch
      });
      await refreshImportBatches();
    } catch (error) {
      setImportError(error instanceof Error ? error.message : "回滚失败");
    } finally {
      setRollingBackBatchId(null);
    }
  }

  function handleFaqSelect(item: AdminFaq) {
    setSelectedFaqId(item.id);
    setKnowledgeDraft((current) => ({
      ...current,
      faq: normalizeFaqDraft(item)
    }));
  }

  async function handleFaqSave() {
    setFaqSaving(true);
    setFaqError(null);

    try {
      if (selectedFaqId) {
        const updated = await updateAdminFaq(selectedFaqId, knowledgeDraft.faq);
        setFaqItems((current) => current.map((item) => (item.id === updated.id ? updated : item)));
        handleFaqSelect(updated);
      } else {
        const created = await createAdminFaq(knowledgeDraft.faq);
        setFaqItems((current) => [created, ...current]);
        handleFaqSelect(created);
      }
      await refreshFaq();
    } catch (error) {
      setFaqError(error instanceof Error ? error.message : "FAQ 保存失败");
    } finally {
      setFaqSaving(false);
    }
  }

  function resetFaqDraft() {
    setSelectedFaqId(null);
    setKnowledgeDraft((current) => ({
      ...current,
      faq: normalizeFaqDraft()
    }));
  }

  function handlePolicySelect(item: AdminPolicy) {
    setSelectedPolicyId(item.id);
    setKnowledgeDraft((current) => ({
      ...current,
      policy: normalizePolicyDraft(item)
    }));
  }

  function resetPolicyDraft() {
    setSelectedPolicyId(null);
    setKnowledgeDraft((current) => ({
      ...current,
      policy: normalizePolicyDraft()
    }));
  }

  async function handlePolicySave() {
    setPolicySaving(true);
    setPolicyError(null);
    setPolicySavingMessage(null);

    try {
      if (selectedPolicyId) {
        const updated = await updateAdminPolicy(selectedPolicyId, knowledgeDraft.policy);
        setPolicies((current) => current.map((item) => (item.id === updated.id ? updated : item)));
        handlePolicySelect(updated);
        setPolicySavingMessage("政策已更新。");
      } else {
        const created = await createAdminPolicy(knowledgeDraft.policy);
        setPolicies((current) => [created, ...current]);
        handlePolicySelect(created);
        setPolicySavingMessage("政策已创建。");
      }
      await refreshPolicies();
    } catch (error) {
      setPolicyError(error instanceof Error ? error.message : "政策保存失败");
    } finally {
      setPolicySaving(false);
    }
  }

  function getCorrectionDraft(id: string) {
    return correctionDrafts[id] ?? defaultCorrectionDraft;
  }

  function updateCorrectionDraft(
    id: string,
    patch: Partial<AdminConversationCorrectionDraft>
  ) {
    setCorrectionDrafts((current) => ({
      ...current,
      [id]: {
        ...getCorrectionDraft(id),
        ...patch
      }
    }));
  }

  async function submitCorrection(id: string) {
    const draft = getCorrectionDraft(id);
    const comment = [draft.resolution.trim(), draft.note.trim()].filter(Boolean).join("\n");
    if (!comment) {
      setConversationError("请至少填写处理结论或备注后再提交。");
      return;
    }

    setFeedbackSaving(id);
    setConversationError(null);

    try {
      await createAdminFeedback({
        conversationId: id,
        feedbackType: draft.feedbackType,
        comment,
        status: draft.resolution.trim() ? "resolved" : "open"
      });
      setFeedbackLog((current) => ({
        ...current,
        [id]: "人工反馈已写入后台。"
      }));
      await refreshConversations();
    } catch (error) {
      setConversationError(error instanceof Error ? error.message : "人工反馈提交失败");
    } finally {
      setFeedbackSaving(null);
    }
  }

  return (
    <section className="space-y-6">
      <div className="rounded-[2rem] border border-slate-200 bg-white p-6 shadow-soft sm:p-8">
        <p className="text-sm font-semibold uppercase tracking-[0.18em] text-school-700">{title}</p>
        <h2 className="mt-3 text-3xl font-semibold text-ink-900">招生运营与数据维护工作台</h2>
        <p className="mt-3 max-w-3xl text-sm leading-7 text-ink-600">
          管理台已接入当前可用的后台接口，可直接执行分数导入、FAQ 与政策维护、咨询日志跟进和人工纠错提交。
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {statCards.map((metric) => (
          <StatCard key={metric.label} {...metric} />
        ))}
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.15fr)_minmax(320px,0.85fr)]">
        <SectionCard
          title="导入工作台"
          description="支持粘贴 admission score rows JSON 进行 dry-run 预检或正式导入，并可对既有批次执行回滚。"
        >
          <div className="space-y-5">
            <div className="grid gap-4 md:grid-cols-2">
              <label className="block">
                <span className="text-sm font-medium text-ink-700">源文件名</span>
                <input
                  type="text"
                  value={importForm.sourceFileName}
                  onChange={(event) =>
                    setImportForm((current) => ({ ...current, sourceFileName: event.target.value }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">来源标签</span>
                <input
                  type="text"
                  value={importForm.sourceLabel}
                  onChange={(event) =>
                    setImportForm((current) => ({ ...current, sourceLabel: event.target.value }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">数据版本</span>
                <input
                  type="text"
                  value={importForm.dataVersion}
                  onChange={(event) =>
                    setImportForm((current) => ({ ...current, dataVersion: event.target.value }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">导入人</span>
                <input
                  type="text"
                  value={importForm.importedBy}
                  onChange={(event) =>
                    setImportForm((current) => ({ ...current, importedBy: event.target.value }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
            </div>

            <label className="block">
              <span className="text-sm font-medium text-ink-700">admission score rows JSON</span>
              <textarea
                value={importForm.rowsText}
                onChange={(event) =>
                  setImportForm((current) => ({ ...current, rowsText: event.target.value }))
                }
                rows={14}
                className="mt-2 w-full rounded-[1.5rem] border border-slate-300 bg-slate-950 px-4 py-4 font-mono text-sm text-slate-100 outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
              />
            </label>

            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => void handleImportSubmit(true)}
                className="rounded-full border border-slate-300 px-5 py-3 text-sm font-semibold text-ink-800 transition hover:border-school-300 hover:bg-school-50"
              >
                Dry Run 预检
              </button>
              <button
                type="button"
                onClick={() => void handleImportSubmit(false)}
                className="rounded-full bg-school-700 px-5 py-3 text-sm font-semibold text-white transition hover:bg-school-800"
              >
                正式导入
              </button>
            </div>

            {importError ? <InlineState tone="error" text={importError} /> : null}

            {importResult ? (
              <div className="space-y-3 rounded-[1.5rem] border border-slate-200 bg-slate-50 p-4">
                <div className="grid gap-3 sm:grid-cols-3">
                  <div>
                    <p className="text-xs uppercase tracking-[0.18em] text-ink-500">总行数</p>
                    <p className="mt-2 text-2xl font-semibold text-ink-900">{importResult.summary.totalRows}</p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-[0.18em] text-ink-500">通过</p>
                    <p className="mt-2 text-2xl font-semibold text-emerald-700">
                      {importResult.summary.acceptedRows}
                    </p>
                  </div>
                  <div>
                    <p className="text-xs uppercase tracking-[0.18em] text-ink-500">拒绝</p>
                    <p className="mt-2 text-2xl font-semibold text-rose-700">
                      {importResult.summary.rejectedRows}
                    </p>
                  </div>
                </div>
                <InlineState
                  tone={importResult.persisted ? "success" : "neutral"}
                  text={
                    importResult.persisted
                      ? `导入已写入批次 ${importResult.batchId ?? "—"}。`
                      : "当前结果为 dry-run 预览，尚未写入数据库。"
                  }
                />
                {importResult.rowErrors.length > 0 ? (
                  <div className="rounded-2xl border border-rose-200 bg-white p-4 text-sm text-rose-800">
                    <p className="font-semibold">行级错误</p>
                    <ul className="mt-3 space-y-2">
                      {importResult.rowErrors.slice(0, 8).map((item) => (
                        <li key={`${item.rowIndex}-${item.messages.join("-")}`}>
                          第 {item.rowIndex} 行：{item.messages.join("；")}
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        </SectionCard>

        <SectionCard
          title="批次总览"
          description="展示当前 admission score 导入批次，支持一键回滚。"
        >
          <div className="space-y-3">
            {selectedBatch ? (
              <div className="rounded-[1.5rem] border border-slate-200 bg-slate-50 p-4">
                <p className="text-sm font-semibold text-ink-900">{selectedBatch.sourceFileName}</p>
                <p className="mt-2 text-sm text-ink-600">
                  {selectedBatch.batchNo} · {selectedBatch.importedBy ?? "未记录导入人"} ·{" "}
                  {formatDateTime(selectedBatch.createdAt)}
                </p>
                <div className="mt-4 grid grid-cols-3 gap-3 text-sm">
                  <div className="rounded-2xl bg-white px-3 py-3">
                    <p className="text-ink-500">总行数</p>
                    <p className="mt-1 font-semibold text-ink-900">
                      {summaryCount(selectedBatch.summary, "totalRows")}
                    </p>
                  </div>
                  <div className="rounded-2xl bg-white px-3 py-3">
                    <p className="text-ink-500">通过</p>
                    <p className="mt-1 font-semibold text-ink-900">
                      {summaryCount(selectedBatch.summary, "acceptedRows")}
                    </p>
                  </div>
                  <div className="rounded-2xl bg-white px-3 py-3">
                    <p className="text-ink-500">拒绝</p>
                    <p className="mt-1 font-semibold text-ink-900">
                      {summaryCount(selectedBatch.summary, "rejectedRows")}
                    </p>
                  </div>
                </div>
              </div>
            ) : (
              <EmptyState
                title="暂无导入批次"
                description="先在左侧粘贴 JSON 做一次 dry-run 或正式导入，这里会展示批次状态。"
              />
            )}

            {importBatches.length > 0 ? (
              <div className="space-y-3">
                {importBatches.map((batch) => (
                  <div
                    key={batch.id}
                    className="rounded-[1.5rem] border border-slate-200 bg-white p-4 shadow-sm"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <p className="text-sm font-semibold text-ink-900">
                          {batch.sourceLabel ?? batch.sourceFileName}
                        </p>
                        <p className="mt-1 text-sm text-ink-600">
                          {batch.sourceFileName} · {batch.dataVersion}
                        </p>
                        <p className="mt-1 text-xs uppercase tracking-[0.18em] text-ink-500">
                          {batch.status}
                        </p>
                      </div>
                      <button
                        type="button"
                        disabled={rollingBackBatchId === batch.id || batch.status === "ROLLED_BACK"}
                        onClick={() => void handleRollback(batch.id)}
                        className="rounded-full border border-slate-300 px-4 py-2 text-sm font-semibold text-ink-800 transition hover:border-school-300 hover:bg-school-50 disabled:cursor-not-allowed disabled:bg-slate-100 disabled:text-ink-400"
                      >
                        {rollingBackBatchId === batch.id ? "回滚中..." : "回滚批次"}
                      </button>
                    </div>
                    <div className="mt-4 grid gap-2 sm:grid-cols-3 text-sm text-ink-600">
                      <span>总行数：{summaryCount(batch.summary, "totalRows")}</span>
                      <span>通过：{summaryCount(batch.summary, "acceptedRows")}</span>
                      <span>拒绝：{summaryCount(batch.summary, "rejectedRows")}</span>
                    </div>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        </SectionCard>
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
        <SectionCard
          title="知识维护"
          description="FAQ 与政策都已接入真实后台接口，可直接创建和更新。"
        >
          <div className="space-y-4">
            <div className="flex flex-wrap gap-2">
              {([
                { key: "faq", label: "FAQ 维护" },
                { key: "policies", label: "政策维护" }
              ] as Array<{ key: KnowledgeMode; label: string }>).map((item) => (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => setKnowledgeMode(item.key)}
                  className={cn(
                    "rounded-full px-4 py-2 text-sm font-semibold transition",
                    knowledgeMode === item.key
                      ? "bg-school-700 text-white"
                      : "border border-slate-300 text-ink-700 hover:border-school-300 hover:bg-school-50"
                  )}
                >
                  {item.label}
                </button>
              ))}
            </div>

            {knowledgeMode === "faq" ? (
              <>
                <div className="flex flex-wrap gap-3">
                  <button
                    type="button"
                    onClick={resetFaqDraft}
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                  >
                    新建 FAQ
                  </button>
                  <button
                    type="button"
                    onClick={() => void refreshFaq()}
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                  >
                    刷新列表
                  </button>
                </div>
                {faqError ? <InlineState tone="error" text={faqError} /> : null}
                {filteredFaqItems.length === 0 ? (
                  <EmptyState title="暂无 FAQ" description="后端当前未返回 FAQ 条目，可直接新建一条。" />
                ) : (
                  <div className="space-y-3">
                    {filteredFaqItems.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        onClick={() => handleFaqSelect(item)}
                        className={cn(
                          "w-full rounded-[1.5rem] border px-4 py-4 text-left transition",
                          selectedFaqId === item.id
                            ? "border-school-300 bg-school-50"
                            : "border-slate-200 bg-white hover:border-school-200 hover:bg-school-50/60"
                        )}
                      >
                        <p className="text-sm font-semibold text-ink-900">{item.question}</p>
                        <p className="mt-1 text-sm text-ink-600">
                          {item.category} · {item.status} · {formatDateTime(item.updatedAt)}
                        </p>
                      </button>
                    ))}
                  </div>
                )}
              </>
            ) : (
              <>
                <div className="flex flex-wrap gap-3">
                  <button
                    type="button"
                    onClick={resetPolicyDraft}
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                  >
                    新建政策草稿
                  </button>
                  <button
                    type="button"
                    onClick={() => void refreshPolicies()}
                    className="rounded-full border border-slate-300 px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                  >
                    刷新列表
                  </button>
                </div>
                {policyError ? <InlineState tone="error" text={policyError} /> : null}
                {policySavingMessage ? <InlineState tone="neutral" text={policySavingMessage} /> : null}
                {filteredPolicies.length === 0 ? (
                  <EmptyState
                    title="暂无政策条目"
                    description="当前接口未返回政策数据时，可先录入前端草稿，待后端开放写接口后接入持久化。"
                  />
                ) : (
                  <div className="space-y-3">
                    {filteredPolicies.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        onClick={() => handlePolicySelect(item)}
                        className={cn(
                          "w-full rounded-[1.5rem] border px-4 py-4 text-left transition",
                          selectedPolicyId === item.id
                            ? "border-school-300 bg-school-50"
                            : "border-slate-200 bg-white hover:border-school-200 hover:bg-school-50/60"
                        )}
                      >
                        <p className="text-sm font-semibold text-ink-900">{item.title}</p>
                        <p className="mt-1 text-sm text-ink-600">
                          {item.category} · {item.year ?? "年份未填"} · {item.status}
                        </p>
                      </button>
                    ))}
                  </div>
                )}
              </>
            )}
          </div>
        </SectionCard>

        <SectionCard
          title={knowledgeMode === "faq" ? "FAQ 编辑器" : "政策编辑器"}
          description={
            knowledgeMode === "faq"
              ? "FAQ 创建和更新直接调用后台接口。"
              : "政策创建和更新直接调用后台接口。"
          }
        >
          {knowledgeMode === "faq" ? (
            <div className="space-y-4">
              {selectedFaq ? (
                <InlineState
                  text={`当前正在编辑：${selectedFaq.question}（${selectedFaq.status}）`}
                />
              ) : null}
              <label className="block">
                <span className="text-sm font-medium text-ink-700">问题</span>
                <input
                  type="text"
                  value={knowledgeDraft.faq.question}
                  onChange={(event) =>
                    setKnowledgeDraft((current) => ({
                      ...current,
                      faq: { ...current.faq, question: event.target.value }
                    }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">回答</span>
                <textarea
                  rows={8}
                  value={knowledgeDraft.faq.answer}
                  onChange={(event) =>
                    setKnowledgeDraft((current) => ({
                      ...current,
                      faq: { ...current.faq, answer: event.target.value }
                    }))
                  }
                  className="mt-2 w-full rounded-[1.5rem] border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <div className="grid gap-4 md:grid-cols-2">
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">分类</span>
                  <input
                    type="text"
                    value={knowledgeDraft.faq.category}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        faq: { ...current.faq, category: event.target.value }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">状态</span>
                  <select
                    value={knowledgeDraft.faq.status}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        faq: {
                          ...current.faq,
                          status: event.target.value as AdminFaqInput["status"]
                        }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  >
                    <option value="draft">draft</option>
                    <option value="published">published</option>
                  </select>
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">标签（逗号分隔）</span>
                  <input
                    type="text"
                    value={knowledgeDraft.faq.tags.join(", ")}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        faq: {
                          ...current.faq,
                          tags: safeStringArray(
                            event.target.value
                              .split(",")
                              .map((item) => item.trim())
                              .filter(Boolean)
                          )
                        }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">来源标签</span>
                  <input
                    type="text"
                    value={knowledgeDraft.faq.sourceLabel}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        faq: { ...current.faq, sourceLabel: event.target.value }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
              </div>
              <div className="flex flex-wrap gap-3">
                <button
                  type="button"
                  disabled={faqSaving}
                  onClick={() => void handleFaqSave()}
                  className="rounded-full bg-school-700 px-5 py-3 text-sm font-semibold text-white transition hover:bg-school-800 disabled:bg-school-300"
                >
                  {faqSaving ? "保存中..." : selectedFaqId ? "更新 FAQ" : "创建 FAQ"}
                </button>
                <button
                  type="button"
                  onClick={resetFaqDraft}
                  className="rounded-full border border-slate-300 px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                >
                  重置表单
                </button>
              </div>
            </div>
          ) : (
            <div className="space-y-4">
              {selectedPolicy ? (
                <InlineState text={`当前正在查看：${selectedPolicy.title}`} />
              ) : null}
              <label className="block">
                <span className="text-sm font-medium text-ink-700">标题</span>
                <input
                  type="text"
                  value={knowledgeDraft.policy.title}
                  onChange={(event) =>
                    setKnowledgeDraft((current) => ({
                      ...current,
                      policy: { ...current.policy, title: event.target.value }
                    }))
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <div className="grid gap-4 md:grid-cols-2">
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">分类</span>
                  <input
                    type="text"
                    value={knowledgeDraft.policy.category}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        policy: { ...current.policy, category: event.target.value }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">年份</span>
                  <input
                    type="number"
                    value={knowledgeDraft.policy.year ?? ""}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        policy: {
                          ...current.policy,
                          year: event.target.value ? Number(event.target.value) : null
                        }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">来源链接</span>
                  <input
                    type="text"
                    value={knowledgeDraft.policy.sourceUrl ?? ""}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        policy: { ...current.policy, sourceUrl: event.target.value }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink-700">状态</span>
                  <select
                    value={knowledgeDraft.policy.status}
                    onChange={(event) =>
                      setKnowledgeDraft((current) => ({
                        ...current,
                        policy: {
                          ...current.policy,
                          status: event.target.value as AdminPolicyInput["status"]
                        }
                      }))
                    }
                    className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                  >
                    <option value="active">active</option>
                    <option value="inactive">inactive</option>
                  </select>
                </label>
              </div>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">正文</span>
                <textarea
                  rows={10}
                  value={knowledgeDraft.policy.contentText}
                  onChange={(event) =>
                    setKnowledgeDraft((current) => ({
                      ...current,
                      policy: { ...current.policy, contentText: event.target.value }
                    }))
                  }
                  className="mt-2 w-full rounded-[1.5rem] border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <div className="flex flex-wrap gap-3">
                <button
                  type="button"
                  disabled={policySaving}
                  onClick={() => void handlePolicySave()}
                  className="rounded-full bg-school-700 px-5 py-3 text-sm font-semibold text-white transition hover:bg-school-800 disabled:bg-school-300"
                >
                  {policySaving ? "保存中..." : selectedPolicyId ? "更新政策" : "创建政策"}
                </button>
                <button
                  type="button"
                  onClick={resetPolicyDraft}
                  className="rounded-full border border-slate-300 px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                >
                  重置表单
                </button>
              </div>
            </div>
          )}
        </SectionCard>
      </div>

      <SectionCard
        title="咨询日志与人工纠错"
        description="列表接入当前后台 conversation 摘要接口；人工纠错会直接写入 feedback_records。"
      >
        <div className="grid gap-6 xl:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)]">
          <div className="space-y-4">
            <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_180px_auto]">
              <label className="block">
                <span className="text-sm font-medium text-ink-700">省份代码</span>
                <input
                  type="text"
                  value={conversationProvinceFilter}
                  onChange={(event) => setConversationProvinceFilter(event.target.value)}
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                />
              </label>
              <label className="block">
                <span className="text-sm font-medium text-ink-700">状态</span>
                <select
                  value={conversationStatusFilter}
                  onChange={(event) =>
                    setConversationStatusFilter(event.target.value as AdminConversationStatus | "all")
                  }
                  className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                >
                  <option value="all">all</option>
                  <option value="open">open</option>
                  <option value="resolved">resolved</option>
                </select>
              </label>
              <div className="flex items-end">
                <button
                  type="button"
                  onClick={() => void refreshConversations()}
                  className="w-full rounded-full border border-slate-300 px-4 py-3 text-sm font-semibold text-ink-700 transition hover:border-school-300 hover:bg-school-50"
                >
                  刷新
                </button>
              </div>
            </div>

            {conversationError ? <InlineState tone="error" text={conversationError} /> : null}

            {filteredConversations.length === 0 ? (
              <EmptyState
                title="暂无咨询记录"
                description="当前筛选条件下没有返回 conversation 摘要，确认 API 已启动或调整过滤条件。"
              />
            ) : (
              <div className="space-y-3">
                {filteredConversations.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setSelectedConversationId(item.id)}
                    className={cn(
                      "w-full rounded-[1.5rem] border px-4 py-4 text-left transition",
                      selectedConversationId === item.id
                        ? "border-school-300 bg-school-50"
                        : "border-slate-200 bg-white hover:border-school-200 hover:bg-school-50/60"
                    )}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="text-sm font-semibold text-ink-900">{item.lastMessage}</p>
                        <p className="mt-1 text-sm text-ink-600">
                          {item.provinceCode ?? "省份缺失"} · {item.subjectType ?? "科类缺失"} · {item.status}
                        </p>
                      </div>
                      <span className="text-xs uppercase tracking-[0.18em] text-ink-500">
                        {formatDateTime(item.updatedAt)}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="space-y-4">
            {selectedConversation ? (
              <>
                <div className="rounded-[1.5rem] border border-slate-200 bg-slate-50 p-5">
                  <p className="text-sm font-semibold text-ink-900">会话摘要</p>
                  <div className="mt-3 grid gap-3 sm:grid-cols-2 text-sm text-ink-700">
                    <span>会话 ID：{selectedConversation.id}</span>
                    <span>状态：{selectedConversation.status}</span>
                    <span>省份：{selectedConversation.provinceCode ?? "—"}</span>
                    <span>科类：{selectedConversation.subjectType ?? "—"}</span>
                    <span>分数：{selectedConversation.score ?? "—"}</span>
                    <span>位次：{selectedConversation.rank ?? "—"}</span>
                  </div>
                  <div className="mt-3 text-sm text-ink-700">
                    <p>兴趣标签：{selectedConversation.interestTags.join("、") || "—"}</p>
                    <p className="mt-1">意向专业：{selectedConversation.intendedMajors.join("、") || "—"}</p>
                  </div>
                </div>

                <div className="rounded-[1.5rem] border border-slate-200 bg-white p-5">
                  <p className="text-sm font-semibold text-ink-900">人工纠错 / 反馈</p>
                  <div className="mt-4 grid gap-4 md:grid-cols-2">
                    <label className="block">
                      <span className="text-sm font-medium text-ink-700">反馈类型</span>
                      <select
                        value={getCorrectionDraft(selectedConversation.id).feedbackType}
                        onChange={(event) =>
                          updateCorrectionDraft(selectedConversation.id, {
                            feedbackType: event.target.value as AdminConversationCorrectionDraft["feedbackType"]
                          })
                        }
                        className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                      >
                        <option value="manual-fix">manual-fix</option>
                        <option value="incorrect">incorrect</option>
                        <option value="helpful">helpful</option>
                      </select>
                    </label>
                    <label className="block">
                      <span className="text-sm font-medium text-ink-700">处理结论</span>
                      <input
                        type="text"
                        value={getCorrectionDraft(selectedConversation.id).resolution}
                        onChange={(event) =>
                          updateCorrectionDraft(selectedConversation.id, {
                            resolution: event.target.value
                          })
                        }
                        className="mt-2 w-full rounded-2xl border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                      />
                    </label>
                  </div>
                  <label className="mt-4 block">
                    <span className="text-sm font-medium text-ink-700">备注</span>
                    <textarea
                      rows={6}
                      value={getCorrectionDraft(selectedConversation.id).note}
                      onChange={(event) =>
                        updateCorrectionDraft(selectedConversation.id, {
                          note: event.target.value
                        })
                      }
                      className="mt-2 w-full rounded-[1.5rem] border border-slate-300 px-4 py-3 text-sm outline-none transition focus:border-school-400 focus:ring-4 focus:ring-school-100"
                    />
                  </label>
                  <div className="mt-4 flex flex-wrap gap-3">
                    <button
                      type="button"
                      disabled={feedbackSaving === selectedConversation.id}
                      onClick={() => void submitCorrection(selectedConversation.id)}
                      className="rounded-full bg-school-700 px-5 py-3 text-sm font-semibold text-white transition hover:bg-school-800 disabled:bg-school-300"
                    >
                      {feedbackSaving === selectedConversation.id ? "提交中..." : "记录人工反馈"}
                    </button>
                  </div>

                  {feedbackLog[selectedConversation.id] ? (
                    <div className="mt-4 rounded-2xl border border-slate-200 bg-slate-50 px-4 py-4 text-sm text-ink-700">
                      <p className="font-semibold text-ink-900">反馈已提交</p>
                      <p className="mt-2 text-ink-600">{feedbackLog[selectedConversation.id]}</p>
                    </div>
                  ) : null}
                </div>
              </>
            ) : (
              <EmptyState
                title="请选择一条咨询记录"
                description="左侧选中会话后，这里会展示摘要并提供人工纠错入口。"
              />
            )}
          </div>
        </div>
      </SectionCard>

      {!isHydrated && isPending ? (
        <InlineState text="管理台数据加载中..." />
      ) : null}
    </section>
  );
}
