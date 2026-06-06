"use client";

import type { FormEvent } from "react";

export type ChatComposerProps = {
  disabled?: boolean;
  input: string;
  onInputChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => Promise<void> | void;
};

export function ChatComposer({ disabled = false, input, onInputChange, onSubmit }: ChatComposerProps) {
  return (
    <div className="border-t border-slate-200/70 bg-white/90 px-4 py-4 backdrop-blur sm:px-6">
      <form onSubmit={onSubmit} className="mx-auto flex w-full max-w-4xl items-end gap-3 rounded-[28px] border border-slate-200 bg-white px-3 py-3 shadow-[0_18px_60px_rgba(15,23,42,0.08)]">
        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-school-50 text-school-700 ring-1 ring-school-100">
          <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 3a3 3 0 013 3v5a3 3 0 11-6 0V6a3 3 0 013-3zm6 8a6 6 0 01-12 0m6 6v4m-4 0h8" />
          </svg>
        </div>
        <div className="min-w-0 flex-1">
          <label htmlFor="chat-message" className="sr-only">
            请输入你的问题
          </label>
          <textarea
            id="chat-message"
            aria-label="请输入你的问题"
            value={input}
            onChange={(event) => onInputChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                if (!disabled && input.trim()) {
                  void onSubmit(event as unknown as FormEvent<HTMLFormElement>);
                }
              }
            }}
            onInput={(event) => {
              const target = event.currentTarget;
              target.style.height = "56px";
              target.style.height = `${Math.min(target.scrollHeight, 180)}px`;
            }}
            rows={1}
            style={{ minHeight: "56px" }}
            placeholder="请输入您的报考问题，例如：黑龙江物理类 520 分适合报哪些专业？"
            className="max-h-[180px] w-full resize-none border-0 bg-transparent px-1 py-3 text-[15px] leading-relaxed text-ink-900 outline-none placeholder:text-slate-400"
          />
          <p className="px-1 text-xs text-slate-400">Enter 发送，Shift + Enter 换行</p>
        </div>
        <button
          type="submit"
          aria-label="发送咨询"
          disabled={disabled || input.trim().length === 0}
          className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-ink-900 text-white transition hover:bg-ink-800 disabled:cursor-not-allowed disabled:bg-slate-200 disabled:text-slate-400"
        >
          <span className="sr-only">发送咨询</span>
          <svg className="h-5 w-5 rotate-[-90deg]" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.4} d="M12 19V5M5 12l7-7 7 7" />
          </svg>
        </button>
      </form>
    </div>
  );
}
