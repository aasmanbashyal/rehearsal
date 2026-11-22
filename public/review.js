import { $, button, downloadFile, escape, fileStem, fmt, formatStorage, heading, icon, plain, srtTime, toast } from './ui.js';
import { modelReady, state } from './state.js';
import { prefs } from './preferences.js';
import { libraryReturnRoute, sessionMetadataHTML } from './library.js';
import { scriptReviewHTML } from './practice.js';
import { feedbackHTML } from './feedback.js';

function reviewMediaHTML() {
  const media = escape(state.active.media);
  const video = state.active.kind === 'video';
  const hiddenVideo = video && prefs.hideSessionVideo;
  return `<section class="review-playback" aria-label="Recording playback">
    ${video ? `<div id="session-video" class="review-preview panel" ${hiddenVideo ? 'hidden' : ''}>
        <video id="media" data-id="${state.active.id}" src="${media}" preload="metadata" ${hiddenVideo ? '' : 'controls'} playsinline></video>
      </div>` : `<audio id="media" data-id="${state.active.id}" src="${media}" preload="metadata" hidden></audio>`}
    <div id="session-playback-controls" class="review-audio panel" ${video && !hiddenVideo ? 'hidden' : ''}>
      <button type="button" class="icon-btn playback-toggle" id="play" data-action="play-recording" aria-label="Play recording" title="Play recording">${icon('play')}</button>
      <input id="playback-timeline" type="range" min="0" max="${Number(state.active.duration)}" step="any" value="0" aria-label="Playback timeline" disabled>
      <div class="playback-time"><time id="play-time" datetime="PT0S">00:00</time><span aria-hidden="true">/</span><time id="play-duration" datetime="PT${Number(state.active.duration)}S">${fmt(state.active.duration)}</time></div>
    </div>
    ${video ? `<label class="camera-option review-video-option"><input id="hide-session-video" type="checkbox" aria-controls="session-video session-playback-controls" ${hiddenVideo ? 'checked' : ''}>Hide video</label>` : ''}
  </section>`;
}

function transcriptHTML() {
  const hasText = state.active.segments.length > 0;
  const loading = state.currentJob === state.active.id || state.app.running === state.active.id;
  return `<section class="transcript panel" aria-labelledby="transcript-title">
    <div class="transcript-head"><h2 id="transcript-title">Transcript</h2>${hasText ? `<div class="transcript-tools">
      ${button('copy', 'Copy', 'copy', 'secondary')}
      <label class="visually-hidden" for="export">Export transcript</label><select id="export" class="export-select"><option value="">Export</option><option value="txt">Text (.txt)</option><option value="srt">Subtitles (.srt)</option></select>
    </div>` : ''}</div>
    ${loading ? '<p class="transcript-status" role="status">Transcribing…</p>' : ''}
    ${hasText ? `<label class="search">${icon('search')}<span class="visually-hidden">Find a phrase in the transcript</span><input type="search" id="transcript-search" placeholder="Find a phrase"></label>
      <div class="segments" id="segments">${state.active.segments.map((segment, i) => `<button class="segment" data-action="seek-line" data-index="${i}" type="button"><time>${fmt(segment.start)}</time><span>${escape(segment.text)}</span></button>`).join('')}</div>
      <p id="no-matches" class="transcript-status" role="status" hidden>No matching lines.</p>` : loading ? '' : `
      <div class="empty-transcript"><p>${state.active.transcriptDone ? 'No clear speech was detected.' : 'This take is ready to transcribe.'}</p>${button(modelReady() ? 'waveform' : 'settings', modelReady() ? 'Transcribe take' : 'Set up transcription', modelReady() ? 'transcribe' : 'settings', 'secondary')}</div>`}
  </section>`;
}

function reviewHTML() {
  const loading = state.currentJob === state.active.id || state.app.running === state.active.id;
  const tools = state.active.sample ? '' : `<details class="take-menu"><summary class="icon-btn" aria-label="Take actions" title="Take actions">${icon('more')}</summary><div class="menu-actions">
      ${button('edit', 'Rename', 'rename', 'quiet')}${button('download', 'Download recording', 'save-media', 'quiet')}
      ${button('waveform', 'Transcribe again', 'transcribe', 'quiet', loading ? 'disabled' : '')}${button('trash', 'Delete take', 'delete', 'quiet danger', loading ? 'disabled' : '')}
    </div></details>`;
  const detail = state.active.sample ? 'Example' : state.active.storage ? formatStorage(state.active.storage.savedBytes) : '';
  const metadata = state.active.sample ? '' : sessionMetadataHTML(state.active);
  const video = state.active.kind === 'video';
  return `<div class="review-layout">
    <div class="review-heading"><a class="back-link" href="#${escape(libraryReturnRoute())}">${icon('back')}Back to sessions</a>
    ${heading(escape(state.active.title), detail, `<div class="header-actions">${button('plus', 'Record again', 'new', 'secondary')}${tools}</div>`)}
    ${video ? '' : metadata}</div>
    <div class="review-grid">${reviewMediaHTML()}${video ? metadata : ''}<hr class="review-divider">${scriptReviewHTML()}<div id="review-feedback">${feedbackHTML(state.active)}</div>${transcriptHTML()}</div>
  </div>`;
}

function bindPlayer(position, resume) {
  const media = $('#media');
  const play = $('#play');
  const timeline = $('#playback-timeline');
  const elapsed = $('#play-time'), total = $('#play-duration');
  const duration = () => {
    const reported = Number.isFinite(media.duration) && media.duration > 0 ? media.duration : 0;
    // WebKit can expose only the first fragment's duration for a recorded MP4.
    if (['linux', 'macos'].includes(state.app.platform) && state.active.mime?.endsWith('/mp4')) return Math.max(reported, state.active.duration);
    return reported || state.active.duration;
  };
  state.player = media;
  let activeRow = null;
  const draw = () => {
    if (state.player !== media) return;
    const seconds = media.currentTime;
    if (timeline) {
      const length = duration();
      const ready = media.readyState >= 1 && !media.error && length > 0;
      const timestamp = value => length < 10 ? `${fmt(value)}.${Math.floor(value * 10) % 10}` : fmt(value);
      const time = timestamp(Math.min(seconds, length)), end = timestamp(length);
      if (elapsed.textContent !== time) {
        elapsed.textContent = time;
        elapsed.dateTime = `PT${Number(seconds.toFixed(2))}S`;
      }
      if (total.textContent !== end) {
        total.textContent = end;
        total.dateTime = `PT${Number(length.toFixed(2))}S`;
      }
      timeline.max = length;
      timeline.value = Math.min(seconds, length);
      timeline.setAttribute('aria-valuetext', `${time} of ${end}`);
      timeline.disabled = !ready;
      timeline.style.setProperty('--played', `${length > 0 ? Math.min(100, 100 * seconds / length) : 0}%`);
    }
    const list = $('#segments');
    if (!list) { activeRow = null; return; }
    const index = state.active.segments.findIndex(s => seconds >= s.start && seconds < s.end);
    const row = list.children[index];
    if (row === activeRow) return;
    activeRow?.classList.remove('active');
    activeRow = row;
    if (!row) return;
    row.classList.add('active');
    if (media.paused || row.hidden || $('#transcript-search').value) return;
    const listRect = list.getBoundingClientRect(), rowRect = row.getBoundingClientRect();
    if (rowRect.bottom > listRect.bottom) list.scrollTop += rowRect.bottom - listRect.bottom;
    else if (rowRect.top < listRect.top) list.scrollTop -= listRect.top - rowRect.top;
  };
  const drawPlayButton = () => {
    const playing = !media.paused && !media.ended;
    const label = playing ? 'Pause recording' : 'Play recording';
    play.title = label;
    play.setAttribute('aria-label', label);
    play.innerHTML = icon(playing ? 'pause' : 'play');
    draw();
  };
  state.syncPlayback = draw;
  for (const event of ['loadedmetadata', 'durationchange', 'timeupdate', 'seeked']) media.addEventListener(event, draw);
  if (timeline) {
    timeline.addEventListener('input', () => {
      if (state.player !== media || media.readyState < 1 || media.error) return;
      media.currentTime = timeline.valueAsNumber;
      draw();
    });
    for (const event of ['play', 'pause', 'ended']) media.addEventListener(event, drawPlayButton);
  }
  media.addEventListener('error', () => {
    if (state.player !== media) return;
    if (play) play.disabled = true;
    draw();
    toast('This browser could not play the recording format. You can download the original.', true);
  });
  if (position || resume) {
    media.addEventListener('loadedmetadata', () => {
      if (state.player !== media) return;
      media.currentTime = Math.min(position, duration());
      if (resume) void media.play().catch(() => {});
    }, { once: true });
  }
  draw();
}

function filterTranscript(query) {
  const search = query.trim().toLowerCase();
  let matches = 0;
  for (const row of $('#segments').children) {
    row.hidden = !state.active.segments[Number(row.dataset.index)].text.toLowerCase().includes(search);
    if (!row.hidden) matches++;
  }
  $('#no-matches').hidden = matches > 0;
}

function refreshTranscript() {
  const input = $('#transcript-search');
  const query = input?.value || '';
  const restoreFocus = input && document.activeElement === input;
  const list = $('#segments');
  const scroll = list?.scrollTop || 0;
  $('.transcript').outerHTML = transcriptHTML();
  const wasOpen = $('#take-feedback')?.open;
  const instructions = $('#question')?.value;
  const editing = $('#question')?.closest('details').open;
  $('#review-feedback').innerHTML = feedbackHTML(state.active);
  if (wasOpen !== undefined && $('#take-feedback')) {
    $('#take-feedback').open = wasOpen;
    $('#question').value = instructions;
    $('#question').closest('details').open = editing;
  }
  if (state.active.script) {
    const expanded = $('.full-comparison')?.open;
    $('.comparison').outerHTML = scriptReviewHTML();
    if (expanded && $('.full-comparison')) $('.full-comparison').open = true;
  }
  const nextInput = $('#transcript-search');
  if (nextInput) {
    nextInput.value = query;
    filterTranscript(query);
    if (restoreFocus) nextInput.focus();
    $('#segments').scrollTop = scroll;
  }
  state.syncPlayback?.();
  const busy = state.currentJob === state.active.id || state.app.running === state.active.id;
  for (const action of ['delete', 'transcribe']) {
    const control = $(`.take-menu [data-action="${action}"]`);
    if (control) control.disabled = busy;
  }
}

function playAt(seconds) {
  const media = state.player;
  if (!media) return;
  if (seconds !== undefined) media.currentTime = Math.max(0, Math.min(Number(seconds), state.active.duration));
  void media.play().catch(error => {
    if (media === state.player && error.name !== 'AbortError') toast('Playback could not start. Use the play button to try again.', true);
  });
}

function exportTranscript(type) {
  if(!state.active?.segments.length) return;
  const raw=type==='srt' ? state.active.segments.map((s,i)=>`${i+1}\n${srtTime(s.start)} --> ${srtTime(s.end)}\n${s.text}`).join('\n\n')+'\n' : plain(state.active);
  downloadFile(new Blob([raw],{type:'text/plain;charset=utf-8'}), `${fileStem(state.active.title)}.${type}`);
}

export { bindPlayer, exportTranscript, filterTranscript, playAt, refreshTranscript, reviewHTML };
