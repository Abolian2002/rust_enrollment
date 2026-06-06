/**
 * CosyVoice WebSocket TTS client.
 * Ported from digital_human/web/js/cosyvoiceApi.js → TypeScript.
 */

export type CosyvoiceCallbacks = {
  onAudioData: (data: ArrayBuffer) => void;
  onTaskFinished: () => void;
};

export class CosyvoiceClient {
  private wssUrl: string;
  private voiceId: string;
  private modelName: string;
  private socket: WebSocket | null = null;
  private taskId: string | null = null;
  private isConnected = false;
  private isTaskStarted = false;
  private isTaskFinished = false;
  private resolveTaskStarted: (() => void) | null = null;
  private resolveTaskFinished: (() => void) | null = null;

  constructor(wssUrl: string, voiceId: string, modelName: string) {
    this.wssUrl = wssUrl;
    this.voiceId = voiceId;
    this.modelName = modelName;
  }

  /**
   * Connect to the WebSocket and send the run-task message.
   * Resolves when the server sends `task-started`.
   */
  connect(callbacks: CosyvoiceCallbacks): Promise<void> {
    return new Promise((resolve, reject) => {
      this.resolveTaskStarted = resolve;
      this.socket = new WebSocket(this.wssUrl);
      this.socket.binaryType = "arraybuffer";

      this.socket.onopen = () => {
        this.isConnected = true;
        this.taskId = this.generateUUID();

        const runTaskMessage = {
          header: {
            action: "run-task",
            task_id: this.taskId,
            streaming: "duplex",
          },
          payload: {
            task_group: "audio",
            task: "tts",
            function: "SpeechSynthesizer",
            model: this.modelName,
            parameters: {
              text_type: "PlainText",
              voice: this.voiceId,
              format: "pcm",
              sample_rate: 16000,
              volume: 50,
              rate: 1,
              pitch: 1,
            },
            input: {},
          },
        };

        this.socket!.send(JSON.stringify(runTaskMessage));
      };

      this.socket.onmessage = (event) => {
        const data = event.data;
        if (typeof data === "string") {
          const message = JSON.parse(data);
          if (message.header?.event === "task-started") {
            this.isTaskStarted = true;
            this.isTaskFinished = false;
            this.resolveTaskStarted?.();
          } else if (message.header?.event === "task-finished") {
            this.isTaskFinished = true;
            callbacks.onTaskFinished();
            this.resolveTaskFinished?.();
          } else if (message.header?.event === "task-failed") {
            const messageText =
              typeof message.payload?.message === "string"
                ? message.payload.message
                : typeof message.header?.error_message === "string"
                  ? message.header.error_message
                  : "CosyVoice task failed";
            reject(new Error(messageText));
          }
        } else if (data instanceof ArrayBuffer) {
          callbacks.onAudioData(data);
        }
      };

      this.socket.onerror = () => {
        reject(new Error("CosyVoice WebSocket connection failed"));
      };

      this.socket.onclose = () => {
        this.isConnected = false;
        if (!this.isTaskStarted) {
          reject(new Error("WebSocket closed before task started."));
        }
      };
    });
  }

  /** Send a text chunk for TTS synthesis */
  sendText(textChunk: string): void {
    if (!this.isConnected || !this.isTaskStarted || !this.socket) {
      return; // silently ignore if not ready
    }
    const message = {
      header: {
        action: "continue-task",
        task_id: this.taskId,
        streaming: "duplex",
      },
      payload: {
        input: { text: textChunk },
      },
    };
    this.socket.send(JSON.stringify(message));
  }

  /** Send finish-task and wait for task-finished */
  stop(): Promise<void> {
    if (!this.isConnected || !this.isTaskStarted || !this.socket) {
      return Promise.resolve();
    }
    const message = {
      header: {
        action: "finish-task",
        task_id: this.taskId,
        streaming: "duplex",
      },
      payload: { input: {} },
    };
    this.socket.send(JSON.stringify(message));

    return new Promise((resolve) => {
      this.resolveTaskFinished = resolve;
    });
  }

  /** Close the WebSocket connection */
  close(): void {
    if (this.socket) {
      this.socket.close();
      this.socket = null;
    }
    this.isConnected = false;
    this.isTaskStarted = false;
  }

  get connected(): boolean {
    return this.isConnected && this.isTaskStarted;
  }

  private generateUUID(): string {
    return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
      const r = (Math.random() * 16) | 0;
      const v = c === "x" ? r : (r & 0x3) | 0x8;
      return v.toString(16);
    });
  }
}
