// pcm-player-worklet.js — AudioWorklet processor for PCM playback

class ChunkedAudioBuffer {
  constructor() {
    this.chunks = [];
    this.totalSamples = 0;
  }

  write(data) {
    if (data.length === 0) return;
    this.chunks.push(new Int16Array(data));
    this.totalSamples += data.length;
  }

  read(output, numSamples) {
    let read = 0;
    while (read < numSamples && this.chunks.length > 0) {
      const first = this.chunks[0];
      const toCopy = Math.min(first.length, numSamples - read);
      output.set(first.subarray(0, toCopy), read);
      read += toCopy;

      if (toCopy === first.length) {
        this.chunks.shift();
        this.totalSamples -= first.length;
      } else {
        this.chunks[0] = first.subarray(toCopy);
        this.totalSamples -= toCopy;
      }
    }
    return read;
  }

  clear() {
    this.chunks = [];
    this.totalSamples = 0;
  }

  get availableRead() {
    return this.totalSamples;
  }
}

class PCMPlayerWorklet extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buffer = null;
    this.sampleRate = 16000;
    this.isInitialized = false;
    this.isTaskFinished = false;
    this.notifiedComplete = false;

    this.port.onmessage = (event) => {
      const { type, data, sampleRate } = event.data;
      if (type === 'init') {
        this.sampleRate = sampleRate || 16000;
        this.buffer = new ChunkedAudioBuffer();
        this.isInitialized = true;
        this.isTaskFinished = false;
        this.notifiedComplete = false;
      } else if (type === 'audio') {
        if (this.buffer && data) {
          this.buffer.write(data);
        }
      } else if (type === 'clear') {
        this.buffer?.clear();
        this.notifiedComplete = false;
      } else if (type === 'task-finished') {
        this.isTaskFinished = true;
      }
    };
  }

  process(inputs, outputs, parameters) {
    const output = outputs[0];
    const channel = output[0];

    if (!this.isInitialized || !this.buffer) {
      channel.fill(0);
      return true;
    }

    const numFrames = channel.length;
    const tempBuffer = new Int16Array(numFrames);
    const readCount = this.buffer.read(tempBuffer, numFrames);

    for (let i = 0; i < numFrames; i++) {
      if (i < readCount) {
        channel[i] = tempBuffer[i] / 32768.0;
      } else {
        channel[i] = 0;
      }
    }

    // Report speaking state: has audio data being played
    if (readCount > 0) {
      this.port.postMessage({ type: 'speaking', isSpeaking: true });
    }

    const nowEmpty = this.buffer.availableRead === 0;
    if (this.isTaskFinished && nowEmpty && !this.notifiedComplete) {
      this.port.postMessage({ type: 'playbackComplete' });
      this.port.postMessage({ type: 'speaking', isSpeaking: false });
      this.notifiedComplete = true;
    }

    return true;
  }
}

registerProcessor('pcm-player-worklet', PCMPlayerWorklet);
