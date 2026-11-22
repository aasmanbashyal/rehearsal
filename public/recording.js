import { beginRecording } from './recording-upload.js';
import { drawMeter, meterColumns } from './recorder-display.js';
import { $, fmt, formatStorage, notifyError, toast } from './ui.js';
import { modelReady, selectedModel, state, upsert } from './state.js';
import { meterStyle, updateElapsedClock } from './recorder-view.js';
import { canShareScreen, captureModes, captureOpen, chooseScreen, screenUnavailableMessage, prompterMode, usesCamera, usesScreen } from './capture-options.js';
import { createCamera } from './camera.js';
import { prefs } from './preferences.js';
import { resetPreparation } from './topics.js';
import { api } from './api.js';
import { refreshTranscript } from './review.js';

function createRecorder({ renderCapture, openSession, go }) {
  let recorder, stream, screenCameraEffect, started, timer, audioContext, animator;
  let previewVersion = 0;
  const camera = createCamera(error => {
    if (recorder?.state === 'recording') {
      recorder.stop();
      toast(`${error.message} Saving the recorded portion.`, true);
    } else if (state.phase === 'idle') previewError(error);
  });

  function previewError(error) {
    const status = $('#preview-status');
    if (!status) return;
    const messages = {
      NotAllowedError:'Allow camera access to see your preview.',
      NotFoundError:'No camera found. Connect one and try again.',
      NotReadableError:'The camera is busy. Close the other app and retry.',
    };
    status.textContent = messages[error.name] || error.message || 'Could not open the camera.';
    status.parentElement.hidden = false;
    $('[data-action="retry-camera"]').hidden = state.phase !== 'idle';
  }

  async function showPreview(video, track) {
    if (!video?.isConnected) return;
    if (track.readyState !== 'live') throw new Error('The video source disconnected.');
    video.srcObject = new MediaStream([track]);
    await video.play();
    video.parentElement?.querySelector('.preview-placeholder')?.setAttribute('hidden', '');
  }

  async function syncPreview(retry = false) {
    const version = ++previewVersion;
    if (state.phase !== 'idle') return;
    const video = $('#preview');
    if (!video || !captureOpen()) { camera.close(); return; }
    const prompter = state.view === 'teleprompter';
    try {
      const track = await camera.open(prompter ? prompterMode() : state.recorderMode, prompter ? prefs.prompterHideBackground : prefs.hideBackground, retry);
      if (version === previewVersion) await showPreview(video, track);
    } catch (error) {
      if (version === previewVersion && video.isConnected) previewError(error);
    }
  }

  window.addEventListener('pagehide', () => { ++previewVersion; if (state.phase === 'idle') camera.close(); });
  window.addEventListener('pageshow', event => { if (event.persisted) void syncPreview(); });

  function teardown() {
    clearInterval(timer); cancelAnimationFrame(animator);
    screenCameraEffect?.close();
    screenCameraEffect = null;
    stream?.getTracks().forEach(t=>t.stop()); stream=null;
    if(audioContext) { void audioContext.close().catch(()=>{}); audioContext=null; }
  }

  function animateWave() {
    const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
    try {
      audioContext = new AudioContext();
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 1024;
      audioContext.createMediaStreamSource(stream).connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      const levels = new Uint8Array(meterColumns);
      const meter = $('#wave');
      let previousTheme = '';
      let style;
      let lastUpdate = 0;
      const tick = now => {
        if (state.phase !== 'recording') return;
        animator = requestAnimationFrame(tick);
        if (now - lastUpdate < (reducedMotion ? 300 : 42)) return;
        lastUpdate = now;
        analyser.getFloatTimeDomainData(samples);
        const rms = Math.sqrt(samples.reduce((sum, value) => sum + value * value, 0) / samples.length);
        const strength = Math.min(4, Math.round(rms * 37));
        if (reducedMotion) {
          levels.fill(0);
          levels.fill(strength, 0, Math.ceil(strength * meterColumns / 4));
        } else {
          levels.copyWithin(0, 1);
          levels[levels.length - 1] = strength;
        }
        const theme = document.documentElement.dataset.theme;
        if (previousTheme !== theme) { style = meterStyle(); previousTheme = theme; }
        drawMeter(meter, { ...style, levels });
      };
      animator = requestAnimationFrame(tick);
    } catch { $('#mic-status').textContent = 'Input level unavailable'; }
  }

  async function record(preparedScreen = null) {
    if (state.phase === 'recording') { if(recorder?.state==='recording') recorder.stop(); return; }
    if (state.phase !== 'idle' || !captureOpen()) return;
    if (state.pending) { toast('Save the unsaved take before starting another.',true); return; }
    if (!navigator.mediaDevices?.getUserMedia || !window.MediaRecorder) {
      preparedScreen?.getTracks().forEach(track => track.stop());
      toast('Use a recent browser on the local Rehearsal address to record.',true); return;
    }
    const inPrompter = state.view === 'teleprompter';
    const mode = inPrompter ? prompterMode() : state.recorderMode;
    const hasCamera = usesCamera(mode), screen = usesScreen(mode);
    const combined = hasCamera && screen;
    const script = inPrompter ? state.activeScript : '';
    const hideBackground = inPrompter ? prefs.prompterHideBackground : prefs.hideBackground;
    const cue = inPrompter ? null : state.recordingCue;
    if (screen && !canShareScreen()) { toast(screenUnavailableMessage(), true); return; }
    resetPreparation();
    state.phase='permission'; renderCapture();
    const captureStatus = $('#capture-status');
    let openingCamera = false, startingCapture = false;
    try {
      const compact = Boolean(prefs.compactRecordings);
      const videoMode = hasCamera || screen;
      let screenTrack;
      if (screen) {
        stream = preparedScreen || await chooseScreen(mode);
        screenTrack = stream.getVideoTracks()[0];
        if (!screenTrack || screenTrack.readyState !== 'live') throw new Error('Screen sharing ended.');
      }
      let cameraTrack;
      if (hasCamera) {
        openingCamera = true;
        cameraTrack = await camera.open(mode, hideBackground, true);
        openingCamera = false;
        const video = $('#preview');
        void showPreview(video, cameraTrack).catch(error => { if (video?.isConnected) previewError(error); });
      }
      const input = await navigator.mediaDevices.getUserMedia({
        // WebKitGTK can report a zero sample rate without a capture constraint.
        audio: state.app.platform === 'linux' ? { sampleRate: { ideal: 48_000 } } : true,
        video: false,
      });
      if (screen) {
        for (const track of input.getTracks()) stream.addTrack(track);
      } else stream = input;
      let capture = stream;
      if (cameraTrack && !combined) capture = new MediaStream([cameraTrack, ...input.getAudioTracks()]);
      const requiredTracks = [screenTrack, cameraTrack].filter(Boolean);
      if (requiredTracks.some(track => track.readyState !== 'live')) throw new Error('A video source stopped before recording started.');
      if (combined) {
        if (captureStatus) captureStatus.textContent = 'Combining screen and camera…';
        const { composeScreenCamera } = await import('./screen-camera.js');
        screenCameraEffect = await composeScreenCamera(screenTrack, cameraTrack, compact);
        capture = new MediaStream([screenCameraEffect.track, ...input.getAudioTracks()]);
      }
      const vp8 = 'video/webm;codecs=vp8,opus', vp9 = 'video/webm;codecs=vp9,opus';
      const supported = videoMode ? [...(hasCamera ? [vp8, vp9] : [vp9, vp8]), 'video/mp4'] : ['audio/webm;codecs=opus', 'audio/mp4', 'audio/ogg;codecs=opus'];
      if (state.app.lightMedia && ['windows', 'macos'].includes(state.app.platform)) {
        // An unconstrained MP4 can contain VP9 and Opus, which the Windows reader may miss.
        // On macOS, H.264 and AAC in MP4 play and seek natively in WebKit.
        supported.unshift(videoMode ? 'video/mp4;codecs=avc1,mp4a.40.2' : 'audio/mp4;codecs=mp4a.40.2');
      }
      const mimeType=supported.find(type=>MediaRecorder.isTypeSupported(type));
      recorder = new MediaRecorder(capture, {
        ...(mimeType ? { mimeType } : {}),
        audioBitsPerSecond: compact ? 32_000 : 64_000,
        ...(videoMode ? { videoBitsPerSecond: compact ? 580_000 : 1_100_000 } : {}),
      });
      for (const track of requiredTracks) {
        if (track.readyState !== 'live') throw new Error('A video source stopped before recording started.');
        track.addEventListener('ended', () => {
          if (recorder?.state === 'recording') recorder.stop();
        }, { once: true });
      }
      const type = recorder.mimeType || (videoMode ? 'video/webm' : 'audio/webm');
      const ext = type.includes('mp4') ? 'mp4' : type.includes('ogg') ? 'ogg' : 'webm';
      const title = cue?.prompt || (script ? 'Scripted practice' : `${captureModes[mode][1]} practice`);
      startingCapture = true;
      const upload = await beginRecording({ filename:`take.${ext}`, mime:type, title, recorded:true,
        captureMode:screen ? mode : undefined, script:script || undefined, cueId:cue?.id }, message => {
        if (recorder?.state === 'recording') recorder.stop();
        toast(message, true);
      });
      startingCapture = false;
      recorder.ondataavailable = event => upload.append(event.data);
      recorder.onerror = () => {
        // MediaRecorder emits its final data and stop events after an error.
        // Keep both the committed chunks and that final data available.
        toast('The recording device stopped. Saving the recorded portion.', true);
      };
      recorder.onstop = () => {
        camera.close();
        teardown(); state.prompter?.pause();
        void persistCapture(upload, title);
      };
      recorder.start(1000); started=performance.now(); state.phase='recording'; renderCapture();
      if (inPrompter && prefs.prompterAuto !== false && !matchMedia('(prefers-reduced-motion: reduce)').matches) state.prompter.play();
      $('[data-action="record"]').focus({ preventScroll: true });
      const preview = $('#preview');
      if (preview) {
        void showPreview(preview, capture.getVideoTracks()[0]).catch(error => { if (preview.isConnected) previewError(error); });
      }
      if (state.view === 'studio') animateWave();
      const display = $('#timer'), prompterTimer = $('#prompter-timer'), progress = $('#target-progress'), targetStatus = $('#target-status');
      timer=setInterval(()=>{
        const elapsed=(performance.now()-started)/1000;
        if (display && display.textContent !== fmt(elapsed)) updateElapsedClock(elapsed);
        if (prompterTimer && prompterTimer.textContent !== fmt(elapsed)) {
          prompterTimer.textContent = fmt(elapsed);
          prompterTimer.dateTime = `PT${Math.floor(elapsed)}S`;
        }
        if (progress && elapsed < progress.max) progress.value = elapsed;
        else if (progress && progress.value < progress.max) {
          progress.value = progress.max;
          targetStatus.textContent = 'Target reached';
        }
        if(elapsed>=899.5 && recorder.state==='recording') recorder.stop();
      },150);
    } catch(err) {
      teardown(); state.phase='idle'; renderCapture();
      $('[data-action="record"]').focus({ preventScroll: true });
      if (startingCapture) notifyError(err);
      else if (openingCamera) previewError(err);
      else if (combined) toast('Share a screen or window and allow camera and microphone access to record together.', true);
      else if (screen) toast('Screen recording needs a shared screen or window and microphone access.', true);
      else toast(err.name==='NotAllowedError' ? (state.app.desktop ? 'Check microphone and camera access in your system privacy settings, then try again.' : 'Allow microphone or camera access in your browser, then try again.') : err.name==='NotFoundError' ? 'No microphone or camera was found for this mode.' : 'Could not start recording. Check that another app is not using the device.',true);
    }
  }

  async function persist(file, title, options = {}) {
    const { recorded, compress, captureMode, script, cueId } = options;
    const body = new Blob([JSON.stringify({ filename:file.name, title, recorded, compress, captureMode, script:script || undefined, cueId }) + '\n', file],
      { type:'application/octet-stream' });
    state.phase='saving'; state.pending={file,title,options}; renderCapture();
    const captureStatus = $('#capture-status');
    if (compress && captureStatus) captureStatus.textContent = 'Compressing import…';
    try {
      const { pruned = [], cleanupPending, ...session } = await api('/sessions',{method:'POST',body});
      acceptSession({ pruned, cleanupPending, ...session });
    } catch(err) { state.phase='idle'; renderCapture(); notifyError(err); }
  }

  function acceptSession({ pruned = [], cleanupPending, ...session }) {
    state.app.recordings = (state.app.recordings || []).filter(take => take.id !== session.id);
    if (pruned.length) {
      const removed = new Set(pruned);
      state.app.sessions = state.app.sessions.filter(take => !removed.has(take.id));
    }
    upsert(session); state.pending=null; state.phase='idle';
    if (state.view === 'studio') state.recordingCue = null;
    openSession(session.id);
    const message = pruned.length ? `Take saved. Removed ${pruned.length} older ${pruned.length === 1 ? 'take' : 'takes'}.` :
      session.storage ? `Take saved. Compression saved ${formatStorage(session.storage.originalBytes - session.storage.savedBytes)}.` : 'Take saved.';
    toast(cleanupPending ? `${message} Some older takes could not be deleted. Rehearsal will retry on the next save.` : message);
    if (modelReady()) void runTranscription();
  }

  async function persistCapture(capture, title, retry = false) {
    state.pending = { capture, title }; state.phase = 'saving'; renderCapture();
    try { acceptSession(await capture.finish(retry)); }
    catch (error) { state.phase = 'idle'; renderCapture(); notifyError(error); }
  }

  function retryPending() {
    const pending = state.pending;
    if (pending?.capture) return persistCapture(pending.capture, pending.title, true);
    if (pending) return persist(pending.file, pending.title, pending.options);
  }

  function recover(id) {
    if (state.phase !== 'idle' || state.pending) { toast('Save the current take first.', true); return; }
    const recording = state.app.recordings?.find(take => take.id === id);
    if (recording) return persistCapture({ id, finish:() => api(`/recordings/${id}/finish`, { method:'POST' }) }, recording.title);
  }

  async function runTranscription() {
    if (!state.active || state.active.sample) return;
    if (state.currentJob || state.app.running) { toast('Wait for the current take to finish transcribing.'); return; }
    if (!modelReady()) { go('settings?section=models'); return; }
    const id=state.active.id; state.currentJob=id; refreshTranscript();
    try {
      const session=await api(`/sessions/${id}/transcribe`,{method:'POST',body:{modelId:selectedModel()}});
      upsert(session);
      toast(session.segments.length ? 'Transcript ready.' : 'No clear speech detected.');
    } catch(err) { notifyError(err); }
    finally {
      state.currentJob = null;
      if (state.view === 'review' && state.active?.id === id) refreshTranscript();
    }
  }

  return { record, persist, retryPending, recover, runTranscription, syncPreview };
}

export { createRecorder };
