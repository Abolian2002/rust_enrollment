/**
 * PCMAudioPlayer — plays raw PCM audio data via an AudioWorklet.
 * Ported from digital_human/web/js/audioPlayer.js → TypeScript.
 */

export type PCMAudioPlayerCallbacks = {
  onPlaybackComplete?: () => void;
  onSpeakingChange?: (isSpeaking: boolean) => void;
};

export class PCMAudioPlayer {
  private sampleRate: number;
  private audioContext: AudioContext | null = null;
  private workletNode: AudioWorkletNode | null = null;
  private isConnected = false;
  private callbacks: PCMAudioPlayerCallbacks;
  private speakingThrottle = 0;

  constructor(sampleRate: number, callbacks: PCMAudioPlayerCallbacks = {}) {
    this.sampleRate = sampleRate;
    this.callbacks = callbacks;
  }

  async connect(): Promise<void> {
    if (this.isConnected) return;

    const AudioContextClass =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextClass) {
      throw new Error("Web Audio API not supported");
    }

    this.audioContext = new AudioContextClass({ sampleRate: this.sampleRate });

    if (this.audioContext.state === "suspended") {
      await this.audioContext.resume();
    }

    if (!this.audioContext.audioWorklet) {
      throw new Error("AudioWorklet not supported");
    }

    await this.audioContext.audioWorklet.addModule("/workers/pcm-player-worklet.js");

    this.workletNode = new AudioWorkletNode(this.audioContext, "pcm-player-worklet");
    this.workletNode.connect(this.audioContext.destination);

    this.workletNode.port.onmessage = (event: MessageEvent) => {
      if (event.data.type === "playbackComplete") {
        this.callbacks.onPlaybackComplete?.();
      } else if (event.data.type === "speaking") {
        // Throttle speaking events to avoid excessive re-renders
        const now = Date.now();
        if (now - this.speakingThrottle > 100) {
          this.speakingThrottle = now;
          this.callbacks.onSpeakingChange?.(event.data.isSpeaking);
        }
      }
    };

    this.workletNode.port.postMessage({
      type: "init",
      sampleRate: this.sampleRate,
      bufferSize: Math.ceil(this.sampleRate * 2),
    });

    this.isConnected = true;
  }

  pushPCM(arrayBuffer: ArrayBuffer): void {
    if (!this.isConnected || !this.workletNode) return;
    const int16Data = new Int16Array(arrayBuffer);
    this.workletNode.port.postMessage(
      { type: "audio", data: int16Data },
      [int16Data.buffer]
    );
  }

  sendTtsFinished(): void {
    if (!this.workletNode) return;
    this.workletNode.port.postMessage({ type: "task-finished" });
  }

  clear(): void {
    if (this.workletNode) {
      this.workletNode.port.postMessage({ type: "clear" });
    }
  }

  async stop(): Promise<void> {
    this.clear();
    if (this.workletNode) {
      this.workletNode.disconnect();
      this.workletNode = null;
    }
    if (this.audioContext) {
      await this.audioContext.close();
      this.audioContext = null;
    }
    this.isConnected = false;
  }

  get ready(): boolean {
    return this.isConnected;
  }
}
