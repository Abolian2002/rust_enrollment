"use client";

import type { FormEvent } from "react";
import { useEffect, useRef } from "react";

import { ChatComposer } from "@/components/chat-composer";
import { ChatLanding } from "@/components/chat-landing";
import { ChatResultPanel } from "@/components/chat-result-panel";
import { ChatTranscript } from "@/components/chat-transcript";
import { useChatSession } from "@/components/use-chat-session";

export type ChatShellProps = {
  externalQuestion?: string | null;
  onStreamChunk?: (delta: string) => void;
  onStreamComplete?: () => void | Promise<void>;
  onStreamError?: () => void | Promise<void>;
  onStreamStart?: () => void | Promise<void>;
  onReplyResolved?: (reply: string) => void;
  quickQuestions?: string[];
};

const DEFAULT_QUICK_QUESTIONS = [
  "黑龙江物理类 520 分，汉语言文学录取概率怎么样？",
  "公费师范生有哪些报考要求和限制条件？",
  "历史类近三年在安徽省的最低录取线是多少？"
];

export function ChatShell({
  externalQuestion,
  onReplyResolved,
  onStreamChunk,
  onStreamComplete,
  onStreamError,
  onStreamStart,
  quickQuestions = DEFAULT_QUICK_QUESTIONS
}: ChatShellProps = {}) {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const {
    citations,
    conversationId,
    error,
    input,
    isStarted,
    loading,
    messages,
    setInput,
    streamReply,
    streamStatus,
    structuredResult,
    submitMessage
  } = useChatSession({
    ...(onReplyResolved ? { onReplyResolved } : {}),
    ...(onStreamChunk ? { onStreamChunk } : {}),
    ...(onStreamComplete ? { onStreamComplete } : {}),
    ...(onStreamError ? { onStreamError } : {}),
    ...(onStreamStart ? { onStreamStart } : {})
  });

  useEffect(() => {
    if (messagesEndRef.current && typeof messagesEndRef.current.scrollIntoView === "function") {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [messages, streamReply, structuredResult]);

  useEffect(() => {
    if (externalQuestion?.trim()) {
      setInput(externalQuestion);
    }
  }, [externalQuestion, setInput]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await submitMessage(input);
  }

  return (
    <section className="flex h-full min-h-0 flex-col overflow-hidden rounded-[28px] bg-[linear-gradient(180deg,rgba(255,255,255,0.97),rgba(248,250,252,0.99))]">
      <div className="border-b border-slate-200/80 px-4 py-3 sm:px-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-xl font-semibold text-ink-900">招生问答</h2>
          <span className="inline-flex items-center gap-2 rounded-full bg-white px-3 py-1.5 text-xs font-medium text-ink-600 ring-1 ring-slate-200">
            <span className={`h-2 w-2 rounded-full ${conversationId ? "bg-emerald-400" : "bg-slate-300"}`} />
            {conversationId ? "连续追问中" : "新会话"}
          </span>
        </div>

        <div className="mt-3 flex flex-wrap gap-2">
          {quickQuestions.map((question) => (
            <button
              key={question}
              type="button"
              onClick={() => setInput(question)}
              className="rounded-full bg-slate-50 px-3 py-1.5 text-xs font-medium text-ink-600 ring-1 ring-slate-200 transition hover:bg-school-50 hover:text-school-700 hover:ring-school-100"
            >
              {question}
            </button>
          ))}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {!isStarted ? <ChatLanding onPickQuestion={setInput} /> : null}

        {isStarted ? (
          <>
            <ChatTranscript loading={loading} messages={messages} streamReply={streamReply} streamStatus={streamStatus} />
            <ChatResultPanel citations={citations} structuredResult={structuredResult} />
            {error ? (
              <div className="px-4 pb-6 sm:px-8">
                <div className="rounded-2xl border border-rose-100 bg-rose-50 px-4 py-3 text-sm leading-6 text-rose-700">
                  {error}
                </div>
              </div>
            ) : null}
          </>
        ) : null}

        <div ref={messagesEndRef} className="h-2" />
      </div>

      <ChatComposer disabled={loading} input={input} onInputChange={setInput} onSubmit={handleSubmit} />
    </section>
  );
}
