import { beforeEach, describe, expect, it, vi } from "vitest";

import { sendChatMessage, streamChatMessage } from "@/lib/api-client";

describe("sendChatMessage", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("posts chat requests and returns the backend payload", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        success: true,
        data: {
          conversationId: "conv_123",
          reply: "assistant reply",
          structuredResult: {
            type: "follow_up",
            pendingIntent: "probability_assessment"
          },
          citations: [{ sourceLabel: "招生章程" }]
        },
        meta: {},
        error: null
      })
    });

    vi.stubGlobal("fetch", fetchMock);

    const result = await sendChatMessage({
      message: "黑龙江 物理类 520 分能上数学与应用数学吗？",
      profile: {
        province: "HLJ",
        subjectType: "物理类",
        score: 520
      }
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:4000/api/v1/chat",
      expect.objectContaining({
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          message: "黑龙江 物理类 520 分能上数学与应用数学吗？",
          profile: {
            province: "HLJ",
            subjectType: "物理类",
            score: 520
          }
        })
      })
    );
    expect(result.conversationId).toBe("conv_123");
    expect(result.structuredResult.type).toBe("follow_up");
  });

  it("throws a useful error when the backend returns an API failure envelope", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          success: false,
          data: null,
          meta: {},
          error: {
            code: "BAD_REQUEST",
            message: "message is required"
          }
        })
      })
    );

    await expect(sendChatMessage({ message: " " })).rejects.toThrow(
      "API error: BAD_REQUEST: message is required"
    );
  });

  it("parses streaming chat responses and reports retrieval/generation progress", async () => {
    const encoder = new TextEncoder();
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('event: status\ndata: {"status":"resolving"}\n\n'));
        controller.enqueue(encoder.encode('event: status\ndata: {"status":"retrieving"}\n\n'));
        controller.enqueue(
          encoder.encode(
            'event: chunk\ndata: {"conversationId":"conv_stream","delta":"基于近年数据，"}\n\n'
          )
        );
        controller.enqueue(
          encoder.encode(
            'event: chunk\ndata: {"conversationId":"conv_stream","delta":"你的录取概率中等。"}\n\n'
          )
        );
        controller.enqueue(
          encoder.encode(
            'event: message\ndata: {"success":true,"data":{"conversationId":"conv_stream","reply":"基于近年数据，你的录取概率中等。","structuredResult":{"type":"fallback_reply"},"citations":[{"sourceLabel":"招生章程"}]},"meta":{},"error":null}\n\n'
          )
        );
        controller.enqueue(encoder.encode('event: done\ndata: {"done":true}\n\n'));
        controller.close();
      }
    });

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: stream
    });
    vi.stubGlobal("fetch", fetchMock);

    const statuses: string[] = [];
    const chunks: string[] = [];
    const result = await streamChatMessage(
      { message: "帮我看下录取概率" },
      {
        onStatus(status) {
          statuses.push(status);
        },
        onChunk(delta) {
          chunks.push(delta);
        }
      }
    );

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:4000/api/v1/chat/stream",
      expect.objectContaining({
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        }
      })
    );
    expect(statuses).toEqual(["resolving", "retrieving", "generating"]);
    expect(chunks).toEqual(["基于近年数据，", "你的录取概率中等。"]);
    expect(result.conversationId).toBe("conv_stream");
    expect(result.reply).toBe("基于近年数据，你的录取概率中等。");
  });
});
