import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ChatShell } from "@/components/chat-shell";
import { streamChatMessage } from "@/lib/api-client";

vi.mock("@/lib/api-client", () => ({
  streamChatMessage: vi.fn()
}));

describe("ChatShell", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    window.history.replaceState({}, "", "/chat");
  });

  it("renders nested probability assessment results from the chat API contract", async () => {
    vi.mocked(streamChatMessage).mockImplementation(async (_input, handlers) => {
      handlers?.onStatus?.("retrieving");
      handlers?.onStatus?.("generating");
      handlers?.onChunk?.("基于近年数据，");
      handlers?.onChunk?.("你报考数学与应用数学的录取概率中等。");

      return {
        conversationId: "conv_probability",
        reply: "基于近年数据，你报考数学与应用数学的录取概率中等。",
        structuredResult: {
          type: "probability_assessment",
          assessment: {
            probability: 64,
            level: "medium",
            confidence: "medium",
            summary: "你的分数与位次接近近年录取基线，录取概率中等。",
            factors: [
              "近 3 年最低分数加权后，你高出参考线 8 分。",
              "按可用位次数据估算，你的位次整体优于历史最低录取位次。"
            ],
            province: {
              code: "HLJ",
              name: "黑龙江"
            },
            major: {
              slug: "mathematics-and-applied-mathematics",
              name: "数学与应用数学"
            },
            score: 520,
            rank: 12000,
            subjectType: "物理类"
          }
        },
        citations: [
          {
            year: 2024,
            sourceLabel: "黑龙江省招生数据"
          }
        ]
      };
    });

    render(createElement(ChatShell));

    fireEvent.change(screen.getByLabelText("请输入你的问题"), {
      target: { value: "黑龙江物理类 520 分能上数学与应用数学吗？" }
    });
    fireEvent.submit(screen.getByRole("button", { name: "发送咨询" }).closest("form")!);

    await waitFor(() => {
      expect(streamChatMessage).toHaveBeenCalled();
    });

    expect(await screen.findByText("录取概率评估")).toBeInTheDocument();
    expect(screen.getByText("64%")).toBeInTheDocument();
    expect(screen.getByText("黑龙江 · 物理类 · 数学与应用数学")).toBeInTheDocument();
    expect(screen.getByText("近 3 年最低分数加权后，你高出参考线 8 分。")).toBeInTheDocument();
    expect(screen.queryByText("6400%")).not.toBeInTheDocument();
  });

  it("auto-submits a seeded question from the page query string", async () => {
    window.history.replaceState(
      {},
      "",
      "/chat?q=%E5%8E%BB%E5%B9%B4%E8%8B%B1%E8%AF%AD%E4%B8%93%E4%B8%9A%E5%9C%A8%E9%BB%91%E9%BE%99%E6%B1%9F%E6%9C%80%E4%BD%8E%E5%88%86%E6%98%AF%E5%A4%9A%E5%B0%91%EF%BC%9F&autosend=1"
    );

    vi.mocked(streamChatMessage).mockResolvedValue({
      conversationId: "conv_seeded",
      reply: "2024 年黑龙江英语专业最低分为 515。",
      structuredResult: {
        type: "score_query",
        province: "黑龙江",
        subjectType: "历史类",
        majorName: "英语",
        records: [
          {
            id: "score_2024",
            year: 2024,
            minScore: 515,
            minRank: 5300,
            subjectType: "历史类"
          }
        ]
      },
      citations: [{ year: 2024, sourceLabel: "黑龙江省招生数据" }]
    });

    render(createElement(ChatShell));

    await waitFor(() => {
      expect(streamChatMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          message: "去年英语专业在黑龙江最低分是多少？"
        }),
        expect.any(Object)
      );
    });
  });

  it("renders general answers with a dedicated summary card and admissions redirect", async () => {
    vi.mocked(streamChatMessage).mockResolvedValue({
      conversationId: "conv_general",
      reply:
        "高考前焦虑很常见，先把每天的复习任务拆小，并保证睡眠节奏稳定。如果你愿意，我也可以立刻回到招生咨询，结合黑龙江、物理类、520 分继续帮你看适合的专业。",
      structuredResult: {
        type: "general_answer",
        mode: "brief_then_redirect",
        answer: "高考前焦虑很常见，先把每天的复习任务拆小，并保证睡眠节奏稳定。",
        redirectPrompt:
          "如果你愿意，我也可以立刻回到招生咨询，结合黑龙江、物理类、520 分继续帮你看适合的专业。",
        collectedProfile: {
          province: "黑龙江",
          subjectType: "物理类",
          score: 520
        }
      },
      citations: []
    });

    render(createElement(ChatShell));

    fireEvent.change(screen.getByLabelText("请输入你的问题"), {
      target: { value: "高考前很焦虑怎么办？" }
    });
    fireEvent.submit(screen.getByRole("button", { name: "发送咨询" }).closest("form")!);

    await waitFor(() => {
      expect(streamChatMessage).toHaveBeenCalled();
    });

    expect(await screen.findByText("答复摘要")).toBeInTheDocument();
    expect(screen.getByText("简短回答")).toBeInTheDocument();
    expect(screen.getByText("继续咨询")).toBeInTheDocument();
    expect(screen.getByText(/省份：/)).toBeInTheDocument();
  });
});
