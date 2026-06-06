"use client";

const SUGGESTED_QUESTIONS = [
  "黑龙江物理类 520 分，汉语言文学录取概率怎么样？",
  "位次 12000，想读师范类，推荐哪些专业？",
  "公费师范生有哪些报考要求和限制条件？",
  "历史类近三年在安徽省的最低录取线是多少？"
];

export type ChatLandingProps = {
  onPickQuestion: (question: string) => void;
};

export function ChatLanding({ onPickQuestion }: ChatLandingProps) {
  return (
    <div className="flex h-full flex-col justify-center px-4 py-6 sm:px-5">
      <div className="mx-auto w-full max-w-2xl text-center">
        <p className="text-sm font-medium text-school-700">你好，我是小艺学姐。</p>
        <h1 className="mt-3 text-3xl font-semibold tracking-tight text-ink-900 sm:text-[2.2rem]">想问什么，直接说。</h1>
      </div>

      <div className="mx-auto mt-6 grid w-full max-w-4xl gap-3 md:grid-cols-2">
        {SUGGESTED_QUESTIONS.map((question) => (
          <button
            key={question}
            type="button"
            onClick={() => onPickQuestion(question)}
            className="min-h-[124px] rounded-[24px] border border-slate-200/90 bg-white px-6 py-5 text-left text-[15px] font-medium leading-8 text-ink-800 shadow-[0_12px_30px_rgba(15,23,42,0.04)] transition hover:-translate-y-0.5 hover:border-school-200 hover:bg-school-50/70 hover:text-ink-900 hover:shadow-[0_18px_36px_rgba(15,23,42,0.06)]"
          >
            {question}
          </button>
        ))}
      </div>
    </div>
  );
}
