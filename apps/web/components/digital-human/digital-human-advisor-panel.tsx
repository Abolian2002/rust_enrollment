"use client";

import { DigitalHumanAvatar } from "@/components/digital-human/digital-human-avatar";

export type DigitalHumanAdvisorPanelProps = {
  availability: "ready" | "unavailable" | "checking";
  isMuted: boolean;
  isSpeaking: boolean;
  lastError: string | null;
  onInterrupt: () => void | Promise<void>;
  onMuteToggle: () => void;
  state: "idle" | "connecting" | "speaking" | "error";
  stateLabel: string;
};

export function DigitalHumanAdvisorPanel({
  availability,
  isMuted,
  isSpeaking,
  lastError,
  onInterrupt,
  onMuteToggle,
  state,
  stateLabel
}: DigitalHumanAdvisorPanelProps) {
  return (
    <aside className="relative flex h-[min(78vh,920px)] min-h-[720px] overflow-hidden rounded-[28px] bg-[linear-gradient(180deg,#071223_0%,#10284e_55%,#153764_100%)] text-white shadow-[0_32px_100px_rgba(8,23,46,0.28)] lg:h-full lg:min-h-0">
      <div className="absolute inset-x-0 top-0 z-10 flex items-center justify-between px-4 pt-4 sm:px-5">
        <div className="rounded-full border border-white/12 bg-slate-950/38 px-3.5 py-2 backdrop-blur-xl">
          <p className="text-[11px] font-medium tracking-[0.18em] text-white/55">数字顾问</p>
          <p className="mt-1 text-base font-semibold text-white">小艺学姐</p>
        </div>

        <span className={`inline-flex items-center gap-2 rounded-full px-3 py-1.5 text-xs font-medium backdrop-blur-xl ring-1 ${availability === "unavailable" ? "bg-rose-500/15 text-rose-100 ring-rose-300/20" : "bg-slate-950/38 text-white ring-white/10"}`}>
          <span className={`h-2 w-2 rounded-full ${availability === "unavailable" ? "bg-rose-300" : availability === "checking" ? "bg-amber-300" : "bg-emerald-300"}`} />
          {stateLabel}
        </span>
      </div>

      <div className="absolute inset-0">
        <DigitalHumanAvatar
          state={state}
          isSpeaking={isSpeaking}
          isMuted={isMuted}
          onMuteToggle={onMuteToggle}
          onInterrupt={async () => {
            await onInterrupt();
          }}
        />
      </div>

      {lastError ? (
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 bg-gradient-to-t from-[#071223] via-[#071223]/86 to-transparent px-4 pb-4 pt-12 sm:px-5">
          <div className="rounded-2xl border border-rose-300/18 bg-rose-500/10 px-4 py-3 text-sm text-rose-100 backdrop-blur-xl">
            {lastError}
          </div>
        </div>
      ) : null}
    </aside>
  );
}
