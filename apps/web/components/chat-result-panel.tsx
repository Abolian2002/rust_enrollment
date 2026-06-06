"use client";

import type { ReactNode } from "react";

import type { ChatCitation, ChatStructuredResult } from "@/lib/api-client";

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function stringify(value: unknown) {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number") {
    return String(value);
  }
  return "";
}

function renderCitations(citations: ChatCitation[]) {
  if (citations.length === 0) {
    return null;
  }

  return (
    <div className="rounded-[24px] border border-slate-200 bg-slate-50/80 p-5">
      <p className="text-sm font-semibold text-ink-900">引用来源</p>
      <div className="mt-3 flex flex-wrap gap-2">
        {citations.map((citation, index) => (
          <span
            key={`${citation.sourceLabel}-${citation.year ?? "unknown"}-${index}`}
            className="rounded-full bg-white px-3 py-1.5 text-xs font-medium text-ink-600 ring-1 ring-slate-200"
          >
            {citation.year ? `${citation.year} · ` : ""}
            {citation.sourceLabel}
          </span>
        ))}
      </div>
    </div>
  );
}

function renderProbabilityAssessment(structuredResult: ChatStructuredResult) {
  if (!isRecord(structuredResult.assessment)) {
    return null;
  }

  const assessment = structuredResult.assessment;
  const probability = typeof assessment.probability === "number" ? assessment.probability : null;
  const summary = stringify(assessment.summary);
  const score = stringify(assessment.score);
  const rank = stringify(assessment.rank);
  const subjectType = stringify(assessment.subjectType);
  const factors = Array.isArray(assessment.factors) ? assessment.factors.filter((item): item is string => typeof item === "string") : [];
  const province = isRecord(assessment.province) ? stringify(assessment.province.name ?? assessment.province.code) : stringify(assessment.province);
  const major = isRecord(assessment.major) ? stringify(assessment.major.name ?? assessment.major.slug) : stringify(assessment.major);

  return (
    <div className="rounded-[28px] border border-school-100 bg-gradient-to-br from-white to-school-50/70 p-6 shadow-[0_18px_60px_rgba(15,23,42,0.04)]">
      <div className="flex flex-col gap-6 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <p className="text-sm font-semibold text-school-700">录取概率评估</p>
          <p className="mt-2 text-sm text-ink-600">{province} · {subjectType} · {major}</p>
          {summary ? <p className="mt-3 max-w-2xl text-[15px] leading-8 text-ink-800">{summary}</p> : null}
        </div>
        <div className="flex h-28 w-28 shrink-0 flex-col items-center justify-center rounded-full bg-school-700 text-white shadow-lg shadow-school-200/70">
          <span className="text-3xl font-bold">{probability ?? "--"}%</span>
          <span className="mt-1 text-xs tracking-[0.2em] text-white/70">PROBABILITY</span>
        </div>
      </div>

      <div className="mt-5 flex flex-wrap gap-2">
        {score ? <span className="rounded-full bg-white px-3 py-1.5 text-xs font-medium text-ink-700 ring-1 ring-school-100">分数：{score}</span> : null}
        {rank ? <span className="rounded-full bg-white px-3 py-1.5 text-xs font-medium text-ink-700 ring-1 ring-school-100">位次：{rank}</span> : null}
      </div>

      {factors.length > 0 ? (
        <div className="mt-5 grid gap-3">
          {factors.map((factor) => (
            <div key={factor} className="rounded-2xl bg-white/90 px-4 py-3 text-sm leading-7 text-ink-700 ring-1 ring-school-100">
              {factor}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function renderGeneralAnswer(structuredResult: ChatStructuredResult) {
  const answer = stringify(structuredResult.answer);
  const redirectPrompt = stringify(structuredResult.redirectPrompt);
  const collectedProfile = isRecord(structuredResult.collectedProfile) ? structuredResult.collectedProfile : null;

  return (
    <div className="rounded-[28px] border border-slate-200 bg-white p-6 shadow-[0_18px_60px_rgba(15,23,42,0.04)]">
      <p className="text-sm font-semibold text-ink-900">答复摘要</p>
      <div className="mt-4 grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
        <div className="rounded-3xl bg-school-50/70 p-5 ring-1 ring-school-100">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-school-700">简短回答</p>
          <p className="mt-3 text-[15px] leading-8 text-ink-800">{answer}</p>
        </div>
        <div className="rounded-3xl bg-slate-50 p-5 ring-1 ring-slate-200">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-slate-500">继续咨询</p>
          <p className="mt-3 text-[15px] leading-8 text-ink-700">{redirectPrompt}</p>
          {collectedProfile ? (
            <div className="mt-4 space-y-2 text-sm text-ink-600">
              {collectedProfile.province ? <p>省份：{stringify(collectedProfile.province)}</p> : null}
              {collectedProfile.subjectType ? <p>科类：{stringify(collectedProfile.subjectType)}</p> : null}
              {collectedProfile.score ? <p>分数：{stringify(collectedProfile.score)}</p> : null}
              {collectedProfile.rank ? <p>位次：{stringify(collectedProfile.rank)}</p> : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function renderScoreQuery(structuredResult: ChatStructuredResult) {
  const records = Array.isArray(structuredResult.records)
    ? structuredResult.records.filter((item): item is Record<string, unknown> => isRecord(item))
    : [];

  if (records.length === 0) {
    return null;
  }

  return (
    <div className="rounded-[28px] border border-slate-200 bg-white p-6 shadow-[0_18px_60px_rgba(15,23,42,0.04)]">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <p className="text-sm font-semibold text-ink-900">历年分数参考</p>
          <p className="mt-1 text-sm text-ink-600">
            {stringify(structuredResult.province)} · {stringify(structuredResult.subjectType)} · {stringify(structuredResult.majorName)}
          </p>
        </div>
      </div>
      <div className="mt-5 overflow-hidden rounded-3xl border border-slate-200">
        <table className="min-w-full divide-y divide-slate-200 text-left text-sm">
          <thead className="bg-slate-50 text-slate-500">
            <tr>
              <th className="px-4 py-3 font-medium">年份</th>
              <th className="px-4 py-3 font-medium">最低分</th>
              <th className="px-4 py-3 font-medium">最低位次</th>
              <th className="px-4 py-3 font-medium">科类</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100 bg-white text-ink-800">
            {records.map((record, index) => (
              <tr key={`${record.id ?? index}`}>
                <td className="px-4 py-3">{stringify(record.year)}</td>
                <td className="px-4 py-3 font-semibold text-school-700">{stringify(record.minScore)}</td>
                <td className="px-4 py-3">{stringify(record.minRank) || "—"}</td>
                <td className="px-4 py-3">{stringify(record.subjectType) || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function renderFollowUp(structuredResult: ChatStructuredResult) {
  const missingFields = Array.isArray(structuredResult.missingFields)
    ? structuredResult.missingFields.filter((item): item is string => typeof item === "string")
    : [];
  const collectedProfile = isRecord(structuredResult.collectedProfile) ? structuredResult.collectedProfile : null;

  return (
    <div className="rounded-[28px] border border-amber-100 bg-amber-50/80 p-6 shadow-[0_18px_60px_rgba(15,23,42,0.04)]">
      <p className="text-sm font-semibold text-amber-800">继续补充信息</p>
      <p className="mt-2 text-[15px] leading-8 text-amber-900">为了继续完成招生判断，请补充以下关键信息。</p>
      <div className="mt-4 flex flex-wrap gap-2">
        {missingFields.map((field) => (
          <span key={field} className="rounded-full bg-white px-3 py-1.5 text-xs font-semibold text-amber-700 ring-1 ring-amber-200">
            {field}
          </span>
        ))}
      </div>
      {collectedProfile ? (
        <div className="mt-5 rounded-3xl bg-white/90 p-4 text-sm leading-7 text-ink-700 ring-1 ring-amber-100">
          {collectedProfile.province ? <p>省份：{stringify(collectedProfile.province)}</p> : null}
          {collectedProfile.subjectType ? <p>科类：{stringify(collectedProfile.subjectType)}</p> : null}
          {collectedProfile.score ? <p>分数：{stringify(collectedProfile.score)}</p> : null}
          {collectedProfile.rank ? <p>位次：{stringify(collectedProfile.rank)}</p> : null}
          {collectedProfile.majorName ? <p>专业：{stringify(collectedProfile.majorName)}</p> : null}
        </div>
      ) : null}
    </div>
  );
}

export type ChatResultPanelProps = {
  citations: ChatCitation[];
  structuredResult: ChatStructuredResult | null;
};

export function ChatResultPanel({ citations, structuredResult }: ChatResultPanelProps) {
  if (!structuredResult) {
    return citations.length > 0 ? <div className="px-4 sm:px-8">{renderCitations(citations)}</div> : null;
  }

  let content: ReactNode = null;

  switch (structuredResult.type) {
    case "probability_assessment":
      content = renderProbabilityAssessment(structuredResult);
      break;
    case "general_answer":
      content = renderGeneralAnswer(structuredResult);
      break;
    case "score_query":
      content = renderScoreQuery(structuredResult);
      break;
    case "follow_up":
      content = renderFollowUp(structuredResult);
      break;
    default:
      content = null;
  }

  if (!content && citations.length === 0) {
    return null;
  }

  return (
    <div className="space-y-4 px-4 pb-6 sm:px-8">
      {content}
      {renderCitations(citations)}
    </div>
  );
}
