"use client";

import { useRef, useEffect, useState } from "react";
import { cn } from "@/lib/cn";

type AvatarState = "idle" | "connecting" | "speaking" | "error";

interface DigitalHumanAvatarProps {
  state: AvatarState;
  isSpeaking: boolean;
  isMuted: boolean;
  onMuteToggle: () => void;
  onInterrupt: () => void;
}

export function DigitalHumanAvatar({
  state,
  isSpeaking,
  isMuted,
  onMuteToggle,
  onInterrupt,
}: DigitalHumanAvatarProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoLoaded, setVideoLoaded] = useState(false);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.play().catch(() => {
      // Autoplay blocked, user interaction needed
    });
  }, []);

  const statusLabel = (() => {
    switch (state) {
      case "connecting":
        return "语音连接中";
      case "speaking":
        return isSpeaking ? "正在回答" : "思考中";
      case "error":
        return "服务不可用";
      default:
        return "在线";
    }
  })();

  const statusColor = (() => {
    switch (state) {
      case "connecting":
        return "dh-indicator-connecting";
      case "speaking":
        return isSpeaking ? "dh-indicator-speaking" : "dh-indicator-thinking";
      case "error":
        return "dh-indicator-error";
      default:
        return "dh-indicator-online";
    }
  })();

  return (
    <div className="dh-root">
      {/* Full-bleed video background */}
      <div className="dh-video-bg">
        <video
          ref={videoRef}
          src="/digital-human/01.webm"
          poster="/digital-human/thumbnail.jpg"
          loop
          muted
          playsInline
          onLoadedData={() => setVideoLoaded(true)}
          className={cn("dh-video", videoLoaded && "dh-video-ready")}
        />
        {!videoLoaded && (
          <div className="dh-video-skeleton">
            <div className="dh-skeleton-pulse" />
          </div>
        )}

        {/* Subtle vignette overlay */}
        <div className="dh-vignette" />
      </div>

      {/* Bottom info overlay — glassmorphism bar */}
      <div className="dh-overlay-bar">
        {/* Status badge */}
        <div className="dh-info-left">
          <div className="dh-avatar-badge">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2a4 4 0 0 1 4 4v2a4 4 0 0 1-8 0V6a4 4 0 0 1 4-4z" />
              <path d="M18 14a6 6 0 0 1-12 0" />
              <path d="M12 18v4" />
            </svg>
          </div>
          <div className="dh-info-text">
            <span className="dh-name">AI 招生顾问</span>
            <span className="dh-status-line">
              <span className={cn("dh-indicator", statusColor)} />
              {statusLabel}
            </span>
          </div>
        </div>

        {/* Controls */}
        <div className="dh-controls">
          <button
            type="button"
            onClick={onMuteToggle}
            className={cn("dh-ctrl-btn", isMuted && "dh-ctrl-active")}
            title={isMuted ? "取消静音" : "静音"}
          >
            {isMuted ? (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                <line x1="23" y1="9" x2="17" y2="15" />
                <line x1="17" y1="9" x2="23" y2="15" />
              </svg>
            ) : (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                <path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07" />
              </svg>
            )}
          </button>

          {state === "speaking" && (
            <button
              type="button"
              onClick={onInterrupt}
              className="dh-ctrl-btn dh-ctrl-stop"
              title="打断"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <rect x="6" y="4" width="4" height="16" rx="1" />
                <rect x="14" y="4" width="4" height="16" rx="1" />
              </svg>
            </button>
          )}
        </div>
      </div>

      {/* Speaking waveform — appears at bottom edge */}
      {isSpeaking && (
        <div className="dh-wave-strip">
          {Array.from({ length: 24 }).map((_, i) => (
            <span key={i} className="dh-wave-tick" style={{ animationDelay: `${i * 0.05}s` }} />
          ))}
        </div>
      )}
    </div>
  );
}
