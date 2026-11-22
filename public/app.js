import { state } from './state.js';
import { migratePracticeMarks, practicePageSize } from './practice.js';
import { $, icon, notifyError, toast } from './ui.js';
import { cueCards, loadCueCards, setCustomTopics } from './cue-cards.js';
import { cueCategory, cueDifficulty, drawPreparation, drawTopic, resetPreparation, targetSeconds, topicsHTML } from './topics.js';
import { prefs, savePrefs } from './preferences.js';
import { meterStyle, retryControlsHTML, studioHTML, updateElapsedClock } from './recorder-view.js';
import { prompterCaptureHTML, teleprompterPageHTML } from './prompter-page.js';
import { bindPlayer, refreshTranscript, reviewHTML } from './review.js';
import { libraryHTML, libraryParams, sessionCue } from './library.js';
import { fillSettingsFields, selectSettingsSection, settingsHTML } from './settings.js';
import { drawMeter } from './recorder-display.js';
import { bindTeleprompter } from './teleprompter.js';
import { countScriptWords } from './script-review.js';
import { api } from './api.js';
import { initializeAppearance } from './appearance.js';
import { createRecorder } from './recording.js';
import { createModelManager } from './models.js';
import { bindEvents } from './events.js';
import { offerTranscriptionSetup } from './models-view.js';
import { watchTranscription } from './transcription-status.js';

const workspaceLinks = document.querySelectorAll('.main-nav a');

const viewNames = { studio:'Recorder', topics:'Cue cards', teleprompter:'Teleprompter', review:'Review', library:'Sessions', settings:'Settings' };

function updateChrome() {
  document.title = `Rehearsal · ${viewNames[state.view]}`;
  for (const link of workspaceLinks) {
    if (link.dataset.view === state.view) link.setAttribute('aria-current', 'page');
    else if (state.view === 'review' && link.dataset.view === 'library') link.setAttribute('aria-current', 'location');
    else link.removeAttribute('aria-current');
    link.setAttribute('aria-disabled', String(state.phase !== 'idle'));
  }
}

function showRoute(focus = false) {
  let hash = location.hash;
  if (state.phase !== 'idle') {
    history.replaceState(null, '', state.shownRoute || '#studio');
    return;
  }
  if (hash.split('?')[0] === '#dashboard') {
    hash = '#library';
    history.replaceState(null, '', hash);
  }
  if (state.view === 'topics' && hash !== state.shownRoute) resetPreparation();
  state.shownRoute = hash;
  const route = hash.slice(1).split('?')[0];
  if (state.view === 'settings' && route === 'settings') {
    selectSettingsSection();
    if (focus) {
      const control = $('#settings-section').offsetWidth ? $('#settings-section') : $('.settings-nav [aria-current]');
      control.focus({ preventScroll:true });
    }
    return;
  }
  if (route.startsWith('review/')) {
    const id = route.slice('review/'.length);
    const session = id === 'example' ? state.app.sample : state.app.sessions.find(s => s.id === id);
    if (session && !state.pending) {
      if (state.active?.id !== id) state.practiceView = { status:'pending', type:'', limit:practicePageSize, undo:null };
      state.active = session;
      migratePracticeMarks(session);
      render('review');
    } else {
      history.replaceState(null, '', '#library');
      state.shownRoute = '#library';
      state.active = null;
      render('library');
      toast(state.pending ? 'Save the unsaved take before opening another.' : 'That session could not be found.', true);
    }
  } else {
    state.active = null;
    if (route.startsWith('topics/')) {
      const index = cueCards.findIndex(card => card.id === route.slice('topics/'.length));
      if (index !== -1) {
        const card = cueCards[index];
        if (cueCategory() && cueCategory() !== card.category) prefs.cueCategory = '';
        if (cueDifficulty() && cueDifficulty() !== card.difficulty) prefs.cueDifficulty = '';
        prefs.practicePrompt = index;
        savePrefs();
      }
      render('topics');
    } else if (route === 'topics' && new URLSearchParams(hash.split('?')[1]).get('collection') === 'custom' && state.app.customTopics?.length) {
      prefs.cueCategory = 'custom';
      savePrefs();
      render('topics');
    } else {
      render(Object.hasOwn(viewNames, route) && route !== 'review' ? route : 'studio');
    }
  }
  if (focus) {
    $('#page-title')?.focus({ preventScroll: true });
    window.scrollTo(0, 0);
  }
}

function go(route) {
  if (state.phase !== 'idle') {
    toast('Finish this recording first.');
    return;
  }
  if (location.hash === `#${route}`) return;
  history.pushState(null, '', `#${route}`);
  showRoute(true);
}

function render(next = state.view || 'studio') {
  if (next !== 'library') { state.selectingSessions = false; state.selectedSessionIds.clear(); }
  state.prompter?.dispose();
  state.prompter = null;
  const sameTake = next === 'review' && state.player?.dataset.id === state.active?.id;
  const position = sameTake ? state.player.currentTime : 0;
  const resume = sameTake && !state.player.paused;
  state.player?.pause();
  state.player = null;
  state.syncPlayback = null;
  if (next !== 'topics') resetPreparation();
  state.view = next;
  updateChrome();
  const pages = { studio: studioHTML, topics: topicsHTML, teleprompter: teleprompterPageHTML, review: reviewHTML, library: libraryHTML, settings: settingsHTML };
  const main = $('#main');
  main.dataset.view = next;
  main.innerHTML = pages[next]();
  if (next === 'review') bindPlayer(position, resume);
  if (next === 'settings') fillSettingsFields();
  if (next === 'topics') {
    $('#target').value = String(targetSeconds());
    drawTopic();
    drawPreparation();
  }
  if (next === 'studio') {
    updateElapsedClock(0);
    drawMeter($('#wave'), meterStyle());
  }
  if (next === 'teleprompter') {
    state.prompter = bindTeleprompter($('#prompter-panel'), countScriptWords(state.activeScript));
  }
  void syncPreview();
}

function renderCapture() {
  if (state.view !== 'teleprompter') { render(); return; }
  updateChrome();
  $('#prompter-capture').innerHTML = prompterCaptureHTML();
  $('#prompter-capture-extras').innerHTML = retryControlsHTML();
  $('.prompter-edit').disabled = state.phase !== 'idle';
  void syncPreview();
}

function newTake() {
  if(state.phase !== 'idle') return;
  if (state.pending) { toast('Save the unsaved take before starting another.',true); return; }
  if (state.active?.script) {
    state.activeScript = state.active.script;
    go('teleprompter');
    return;
  }
  state.recordingCue = sessionCue(state.active) || null;
  go('studio');
}

function openSession(id) {
  if (state.phase !== 'idle') return;
  if (state.pending) { toast('Save the unsaved take before opening another.',true); return; }
  const params = state.view === 'library' ? libraryParams() : null;
  go(`review/${id}${params?.size ? `?${params}` : ''}`);
}

async function refreshAfterTranscription() {
  const next = await api('/state');
  state.app = { ...state.app, sessions: next.sessions, running: next.running };
  if (state.view === 'review' && !state.active?.sample) {
    const updated = state.app.sessions.find(s => s.id === state.active?.id);
    if (updated) { state.active = updated; refreshTranscript(); }
    else go('library');
  } else if (state.view === 'library' && state.phase === 'idle') render();
  return state.app.running;
}

for (const link of workspaceLinks) link.insertAdjacentHTML('afterbegin', icon(link.dataset.symbol));
initializeAppearance();
const { record, persist, retryPending, recover, runTranscription, syncPreview } = createRecorder({ renderCapture, openSession, go });
const { applyModelState } = createModelManager({ updateChrome });
bindEvents({ go, render, renderCapture, showRoute, newTake, openSession, record, persist, retryPending, recover, runTranscription, applyModelState, syncPreview });

(async()=>{
  try {
    [state.app] = await Promise.all([api('/state'), loadCueCards()]);
    setCustomTopics(state.app.customTopics);
    if (Array.isArray(prefs.practicedWords) && prefs.practicedWords.length && !prefs.legacyPracticeTakes) {
      prefs.legacyPracticeTakes = state.app.sessions.filter(session => session.script && session.segments?.length).map(session => session.id);
      savePrefs();
    }
  }
  catch(err) { notifyError(err); }
  showRoute();
  applyModelState(state.app);
  offerTranscriptionSetup();
  if (state.app.running) void watchTranscription(state.app.running, () => api('/transcription'), refreshAfterTranscription);
})();
