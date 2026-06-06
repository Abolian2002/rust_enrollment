"use client";

/**
 * useTTS — React hook that wraps CosyVoice + PCMAudioPlayer.
 * Provides sendText / stop / interrupt and exposes isSpeaking state.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { CosyvoiceClient } from "./cosyvoice";
import { PCMAudioPlayer } from "./pcm-audio-player";

const WSS_BASE = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
const VOICE_ID = "cosyvoice-v3.5-plus-bailian-4a759f175e364696bc78368bce04252c";
const MODEL_NAME = "cosyvoice-v3.5-plus";
const SAMPLE_RATE = 16000;

type TTSState = "idle" | "connecting" | "speaking" | "error";

async function fetchTtsToken(): Promise<string> {
  const baseUrl = process.env.NEXT_PUBLIC_API_BASE_URL?.trim() || "http://localhost:4000";
  const res = await fetch(`${baseUrl}/api/v1/tts/token`, { method: "POST" });
  const data = await res.json().catch(() => null);

  if (!res.ok) {
    const message = typeof data?.error?.message === "string" ? data.error.message : "Failed to fetch TTS token";
    throw new Error(message);
  }

  const token = data?.data?.token ?? data?.token;
  if (!token) {
    throw new Error("TTS token is missing from the response");
  }

  return token;
}

export function useTTS() {
  const [state, setState] = useState<TTSState>("idle");
  const [isSpeaking, setIsSpeaking] = useState(false);
  const [isMuted, setIsMuted] = useState(false);

  const clientRef = useRef<CosyvoiceClient | null>(null);
  const playerRef = useRef<PCMAudioPlayer | null>(null);
  const sessionActive = useRef(false);
  const sessionRunId = useRef(0);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      clientRef.current?.close();
      playerRef.current?.stop();
    };
  }, []);

  /**
   * Start a new TTS session — call this when AI starts streaming response.
   * Returns quickly after the WebSocket handshake completes.
   */
  const startSession = useCallback(async () => {
    const runId = sessionRunId.current + 1;
    sessionRunId.current = runId;

    if (clientRef.current) {
      clientRef.current.close();
      clientRef.current = null;
    }
    if (playerRef.current) {
      await playerRef.current.stop();
      playerRef.current = null;
    }

    setState("connecting");
    sessionActive.current = true;

    try {
      const token = await fetchTtsToken();
      if (sessionRunId.current !== runId) {
        throw new Error("TTS session superseded");
      }
      const wssUrl = `${WSS_BASE}/?api_key=${token}`;

      const player = new PCMAudioPlayer(SAMPLE_RATE, {
        onPlaybackComplete: () => {
          setIsSpeaking(false);
          setState("idle");
          sessionActive.current = false;
        },
        onSpeakingChange: (speaking) => {
          setIsSpeaking(speaking);
        }
      });

      await player.connect();
      if (sessionRunId.current !== runId) {
        await player.stop();
        throw new Error("TTS session superseded");
      }
      playerRef.current = player;

      const client = new CosyvoiceClient(wssUrl, VOICE_ID, MODEL_NAME);
      await client.connect({
        onAudioData: (data) => {
          if (!isMuted) {
            player.pushPCM(data);
          }
        },
        onTaskFinished: () => {
          player.sendTtsFinished();
        }
      });

      if (sessionRunId.current !== runId) {
        client.close();
        await player.stop();
        throw new Error("TTS session superseded");
      }
      clientRef.current = client;
      setState("speaking");
    } catch (err) {
      if (sessionRunId.current === runId) {
        setState("error");
        sessionActive.current = false;
      }
      throw err instanceof Error ? err : new Error("TTS session failed to start");
    }
  }, [isMuted]);

  /** Send a text chunk to TTS (call per SSE delta) */
  const sendText = useCallback((text: string) => {
    if (clientRef.current?.connected) {
      clientRef.current.sendText(text);
      setIsSpeaking(true);
    }
  }, []);

  /** Signal end of text input */
  const finishText = useCallback(async () => {
    if (clientRef.current?.connected) {
      await clientRef.current.stop();
    }
  }, []);

  /** Hard interrupt — stop everything immediately */
  const interrupt = useCallback(async () => {
    sessionRunId.current += 1;
    sessionActive.current = false;
    clientRef.current?.close();
    clientRef.current = null;
    if (playerRef.current) {
      await playerRef.current.stop();
      playerRef.current = null;
    }
    setIsSpeaking(false);
    setState("idle");
  }, []);

  const toggleMute = useCallback(() => {
    setIsMuted((prev) => !prev);
  }, []);

  return {
    state,
    isSpeaking,
    isMuted,
    startSession,
    sendText,
    finishText,
    interrupt,
    toggleMute,
  };
}
