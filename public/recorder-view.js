import { $, button, escape, fmt, heading, icon, iconButton } from './ui.js';
import { modelReady, state } from './state.js';
import { cueHints, targetSeconds } from './topics.js';
import { canShareScreen, screenUnavailableMessage, captureModes, permissionStatus, usesCamera, usesScreen } from './capture-options.js';
import { drawClock } from './recorder-display.js';
import { prefs } from './preferences.js';
import { cueCards } from './cue-cards.js';

const dottedDisplay = () => document.documentElement.dataset.palette === 'dotted';

function meterStyle() {
  const style = getComputedStyle($('#wave'));
  return {
    baseline: style.getPropertyValue('--meter-base').trim(),
    signal: style.getPropertyValue('--meter-signal').trim(),
    pixel: dottedDisplay(),
  };
}

function stageHTML() {
  const live = state.phase === 'recording';
  const target = state.recordingCue ? targetSeconds() : 0;
  const status = live ? 'Recording' : state.phase === 'permission' ? permissionStatus(state.recorderMode) : state.phase === 'saving' ? 'Saving recording…' : 'Ready to record';
  return `<div class="stage">
    <div class="stage-channel">
      <h2 class="monitor-label" id="elapsed-label">${icon('clock')} Elapsed time</h2>
      <div class="timer" role="group" aria-labelledby="elapsed-label">
        <time id="timer" datetime="PT0S">00:00</time>
        <canvas id="timer-matrix" width="372" height="88" aria-hidden="true" hidden></canvas>
      </div>
      <p id="capture-status" role="status"><span class="status-dot${live ? ' live-dot' : ''}"></span>${status}</p>
      ${live && target ? `<progress id="target-progress" class="target-progress" max="${target}" value="0" aria-label="Progress toward speaking target"></progress><span id="target-status" role="status" class="target-status">Target ${fmt(target)}</span>` : ''}
    </div>
    <div class="stage-channel audio-monitor">
      <h2 class="monitor-label">${icon('waveform')} Microphone</h2>
      <div class="signal-display"><canvas id="wave" width="420" height="60" aria-hidden="true"></canvas></div>
      <p id="mic-status">${live ? 'Live input level' : 'Input level appears during recording'}</p>
    </div>
  </div>`;
}

function capturePreviewHTML(mode) {
  if (mode === 'audio' || state.phase === 'saving' || (state.phase === 'idle' && (!usesCamera(mode) || state.pending))) return '';
  const live = state.phase === 'recording';
  const title = live ? 'Recording preview' : mode === 'screen-camera' ? 'Camera inset preview' : usesCamera(mode) ? 'Camera preview' : 'Screen preview';
  return `<div class="capture-preview">
    <video id="preview" aria-label="${title}" muted autoplay playsinline></video>
    <span class="preview-caption">${title}</span>
    <div class="preview-placeholder"><p id="preview-status" role="status">${usesCamera(mode) ? 'Opening camera…' : 'Waiting for screen…'}</p>
      <button type="button" class="button secondary" data-action="retry-camera" hidden>Retry camera</button></div>
  </div>`;
}

function updateElapsedClock(seconds) {
  const timer = $('#timer');
  const matrix = $('#timer-matrix');
  const elapsed = fmt(seconds);
  timer.textContent = elapsed;
  timer.dateTime = `PT${Math.floor(seconds)}S`;
  const showMatrix = dottedDisplay() && drawClock(matrix, elapsed);
  timer.classList.toggle('visually-hidden', showMatrix);
  matrix.hidden = !showMatrix;
}

function recordButton(inPrompter = false) {
  const live = state.phase === 'recording';
  const label = live ? 'Stop and save' : state.phase === 'permission' ? 'Waiting for access…' : state.phase === 'saving' ? 'Saving…' : 'Start recording';
  const text = inPrompter && state.phase === 'idle' ? 'Record' : label;
  return `<button type="button" class="button record-button${inPrompter ? ' prompter-record' : ''}${live ? ' is-recording' : ''}" data-action="record" ${state.phase !== 'idle' && !live ? 'disabled' : ''} aria-label="${label}" aria-keyshortcuts="Alt+R" title="${label} (Alt+R)">
    <span class="record-dot${live ? ' stop' : ''}"></span>${text}
  </button>`;
}

function retryControlsHTML() {
  return state.pending && state.phase === 'idle' ? `<div class="retry-controls">${button('download', 'Retry save', 'retry', 'secondary')}${button('play', 'Preview unsaved take', 'preview-unsaved', 'secondary')}</div>` : '';
}

function recorderHTML() {
  const mode = state.recorderMode;
  const locked = state.phase !== 'idle';
  const switcher = `<div class="switch" role="group" aria-label="Recording mode">${Object.entries(captureModes).map(([value, [symbol, label]]) => {
    const unsupported = usesScreen(value) && !canShareScreen();
    return `<button type="button" data-action="mode" data-kind="${value}" aria-pressed="${mode === value}" ${locked || unsupported ? 'disabled' : ''} ${unsupported ? `title="${screenUnavailableMessage()}"` : ''}>${icon(symbol)}${label}</button>`;
  }).join('')}</div>`;
  const controls = `<div class="capture-foot">
      ${recordButton()}
      ${!modelReady() && !locked ? button('settings', 'Set up transcription', 'settings', 'quiet setup-action') : ''}
      <span class="capture-shortcut"><kbd>Alt</kbd> + <kbd>R</kbd></span>
    </div>`;
  const preview = capturePreviewHTML(mode);
  return `<section class="capture" aria-label="Recording controls">
    <div class="capture-head">${switcher}
      <div class="capture-mode-help"><p id="mode-note">${captureModes[mode][2]}</p>
        <label id="background-control" class="camera-option" ${state.app.lightMedia || !usesCamera(mode) ? 'hidden' : ''}><input id="hide-background" type="checkbox" ${prefs.hideBackground ? 'checked' : ''} ${locked ? 'disabled' : ''}>Replace background</label>
      </div>
    </div>
    ${preview ? `<div class="video-stage">${preview}${stageHTML()}</div>` : stageHTML()}${controls}
    ${retryControlsHTML()}
  </section>`;
}

function studioHTML() {
  const idle = state.phase === 'idle';
  return `${heading('Recorder', 'Capture a take and listen back to your practice.')}
    ${state.recordingCue ? `<div class="active-cue">${state.recordingCue.hints.length ? `<details open><summary>${escape(state.recordingCue.prompt)}</summary>${cueHints(state.recordingCue)}</details>` : `<strong>${escape(state.recordingCue.prompt)}</strong>`}
      ${idle ? iconButton('close', 'Remove cue card', 'remove-cue') : ''}</div>` : ''}
    ${recorderHTML()}
    ${idle && !state.pending && state.app.recordings?.length ? `<section class="panel large-panel recovery-panel" aria-label="Interrupted recordings"><h2>Interrupted recordings</h2><p>The saved portion of these takes is still on disk.</p>${state.app.recordings.map(take => `<div class="retry-controls"><strong>${escape(take.title)}</strong><button class="button secondary" data-action="recover-recording" data-id="${take.id}">Recover take</button><button class="button secondary" data-action="download-recording" data-id="${take.id}" ${take.bytes ? '' : 'disabled'}>Download saved portion</button><button class="button quiet" data-action="discard-recording" data-id="${take.id}">Discard</button></div>`).join('')}</section>` : ''}
    ${idle ? `<div class="studio-links">
      ${studioLink('shuffle', 'Cue cards', `Draw from ${cueCards.length} speaking prompts`, 'topics')}
      ${studioLink('headphones', 'Review your takes', state.app.sessions.length ? `${state.app.sessions.length} saved ${state.app.sessions.length === 1 ? 'session' : 'sessions'}` : 'Play back recordings and read transcripts', 'library')}
    </div>` : ''}`;
}

function studioLink(symbol, title, detail, route) {
  return `<a class="studio-link panel" href="#${route}"><span class="row-icon">${icon(symbol)}</span>
    <span><strong>${title}</strong><small>${detail}</small></span>${icon('chevron-right')}</a>`;
}

export { capturePreviewHTML, meterStyle, recordButton, retryControlsHTML, studioHTML, updateElapsedClock };
