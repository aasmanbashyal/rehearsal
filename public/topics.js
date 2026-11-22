import { categories, cueCards, difficulties } from './cue-cards.js';
import { prefs, savePrefs } from './preferences.js';
import { $, button, escape, fmt, heading } from './ui.js';
import { state } from './state.js';
import { libraryRoute } from './library.js';
import { canShareScreen, chooseScreen, screenUnavailableMessage, usesScreen } from './capture-options.js';

let preparation = null;
const maxPrepSeconds = 3600;
const prepSeconds = () => Number.isInteger(prefs.prepSeconds) && prefs.prepSeconds >= 1 && prefs.prepSeconds <= maxPrepSeconds ? prefs.prepSeconds : 60;

const cueCategory = () => Object.hasOwn(categories, prefs.cueCategory) ? prefs.cueCategory : '';

const cueDifficulty = () => cueCategory() !== 'custom' && Object.hasOwn(difficulties, prefs.cueDifficulty) ? prefs.cueDifficulty : '';

const matchingTopics = () => cueCards.flatMap((card, index) =>
  (!cueCategory() || card.category === cueCategory()) && (!cueDifficulty() || card.difficulty === cueDifficulty()) ? [index] : []);

const promptIndex = () => {
  const matches = matchingTopics();
  return matches.includes(prefs.practicePrompt) ? prefs.practicePrompt : matches[0];
};

const drawnTopics = new Set();

const targetOptions = [0, 60, 120, 180, 300];

const targetSeconds = () => targetOptions.includes(prefs.targetSeconds) ? prefs.targetSeconds : 120;
const targetOptionsHTML = () => targetOptions.map(seconds => `<option value="${seconds}" ${seconds === targetSeconds() ? 'selected' : ''}>${seconds ? `${seconds / 60} minute${seconds === 60 ? '' : 's'}` : 'Off'}</option>`).join('');

const cueHints = card => card.hints.length ? `<ul class="cue-hints">${card.hints.map(hint => `<li>${escape(hint)}</li>`).join('')}</ul>` : '';

function resetPreparation() {
  clearInterval(preparation?.timer);
  preparation?.screen?.getTracks().forEach(track => track.stop());
  preparation = null;
}

function drawTopic(updateOptions = true) {
  const index = promptIndex();
  const card = cueCards[index];
  const list = $('#topic-select');
  if (updateOptions) {
    const matches = matchingTopics();
    list.innerHTML = Object.entries(categories).map(([value, label]) => {
      const inCategory = matches.filter(id => cueCards[id].category === value);
      return inCategory.length ? `<optgroup label="${escape(label)}">${inCategory.map(id =>
        `<option value="${id}">${escape(cueCards[id].label)}</option>`).join('')}</optgroup>` : '';
    }).join('');
    $('#topic-count').textContent = `${matches.length} ${matches.length === 1 ? 'topic' : 'topics'}`;
  }
  list.value = String(index);
  $('#cue-difficulty').disabled = cueCategory() === 'custom';
  $('#cue-difficulty').value = cueDifficulty();
  drawnTopics.add(index);
  $('#practice-prompt').innerHTML = `<div class="cue-badges"><span>${escape(categories[card.category])}</span>${card.difficulty ? `<span>${difficulties[card.difficulty]}</span>` : ''}</div>
    <h2>${escape(card.prompt)}</h2>${card.hints.length ? `<div class="cue-guide"><h3>Points to cover</h3>${cueHints(card)}</div>` : ''}`;
  const related = $('#topic-sessions');
  related.hidden = !state.app.sessions.some(session => session.cueId === card.id);
  related.href = `#${libraryRoute(`cue:${card.id}`)}`;
}

function selectTopic(index, updateOptions = false) {
  if (state.shownRoute.startsWith('#topics/') || state.shownRoute.startsWith('#topics?')) {
    state.shownRoute = '#topics';
    history.replaceState(null, '', state.shownRoute);
  }
  prefs.practicePrompt = index;
  savePrefs();
  drawTopic(updateOptions);
  resetPreparation();
  drawPreparation();
}

function drawPreparation() {
  const remaining = preparation?.remaining ?? prepSeconds();
  $('#prep-duration').hidden = Boolean(preparation);
  $('#prep-clock').hidden = !preparation;
  $('#prep-clock').textContent = fmt(remaining);
  $('#prep-clock').dateTime = `PT${remaining}S`;
  $('#prep-progress').max = prepSeconds();
  $('#prep-progress').value = prepSeconds() - remaining;
  $('#prep-button').textContent = preparation ? 'Reset' : 'Start';
  $('#prep-button').setAttribute('aria-label', preparation ? 'Reset preparation timer' : 'Start preparation timer');
  $('#prep-status').textContent = preparation && !preparation.timer ? 'Choose a screen to start the timer.' : 'Recording starts when the timer ends.';
}

function savePreparationDuration(input) {
  if (!preparation && input.validity.valid) {
    prefs.prepSeconds = input.valueAsNumber;
    savePrefs();
    drawPreparation();
  }
}

async function togglePreparation(onComplete) {
  if (preparation) {
    resetPreparation();
    drawPreparation();
    return;
  }
  const input = $('#prep-seconds');
  if (!input.reportValidity()) return;
  if (usesScreen(state.recorderMode) && !canShareScreen()) {
    $('#prep-status').textContent = screenUnavailableMessage();
    return;
  }
  savePreparationDuration(input);
  const run = { remaining:prepSeconds(), screen:null, timer:null };
  preparation = run;
  if (usesScreen(state.recorderMode)) {
    drawPreparation();
    // The system picker needs the Start click's user activation.
    try {
      const screen = await chooseScreen(state.recorderMode);
      if (preparation !== run) { screen.getTracks().forEach(track => track.stop()); return; }
      run.screen = screen;
      const track = screen.getVideoTracks()[0];
      if (track?.readyState !== 'live') throw new Error('Screen sharing ended');
      track.addEventListener('ended', () => {
        if (preparation === run) { resetPreparation(); drawPreparation(); }
      }, { once:true });
    } catch {
      if (preparation === run) {
        resetPreparation(); drawPreparation();
        $('#prep-status').textContent = 'Choose a screen to prepare a screen recording.';
      }
      return;
    }
  }
  const deadline = performance.now() + run.remaining * 1000;
  run.timer = setInterval(() => {
    const remaining = Math.max(0, Math.ceil((deadline - performance.now()) / 1000));
    if (remaining === run.remaining) return;
    run.remaining = remaining;
    if (!remaining) {
      const screen = run.screen;
      run.screen = null;
      resetPreparation();
      onComplete(screen);
      return;
    }
    drawPreparation();
  }, 160);
  drawPreparation();
}

function topicsHTML() {
  const options = (values, selected) => Object.entries(values).map(([value, label]) =>
    `<option value="${value}" ${value === selected ? 'selected' : ''}>${escape(label)}</option>`).join('');
  return `${heading('Cue cards', 'Pick a topic and practice speaking without a script.')}
    <section class="topic-filters panel" aria-label="Choose a topic">
      <div class="topic-field"><label for="cue-category">Category</label><select id="cue-category">${options({ '': 'All categories', ...categories }, cueCategory())}</select></div>
      <div class="topic-field"><label for="cue-difficulty">Difficulty</label><select id="cue-difficulty">${options({ '': 'All levels', ...difficulties }, cueDifficulty())}</select></div>
      <div class="topic-picker">
        <div class="topic-field"><div class="topic-label"><label for="topic-select">Topic</label><span id="topic-count"></span></div><select id="topic-select" aria-describedby="topic-count"></select></div>
        ${button('shuffle', 'Shuffle', 'next-prompt', 'secondary')}
      </div>
    </section>
    <div class="topics-grid">
      <section class="prompt-panel" aria-label="Speaking topic">
        <div id="practice-prompt" class="prompt-content" aria-live="polite"></div>
      </section>
      <section class="prompt-timers panel" aria-label="Practice timers">
        <h2>Practice setup</h2>
        <div class="prep-block"><label for="prep-seconds">Preparation time</label><div class="prep-controls">
          <div id="prep-duration"><input id="prep-seconds" aria-label="Preparation time in seconds" type="number" min="1" max="${maxPrepSeconds}" step="1" required value="${prepSeconds()}"> sec</div>
          <time id="prep-clock" hidden></time><button type="button" id="prep-button" class="button secondary" data-action="prep" aria-describedby="prep-status">Start</button></div>
          <progress id="prep-progress" max="${prepSeconds()}" value="0" aria-label="Preparation timer progress"></progress><p id="prep-status" class="prep-status" role="status"></p>
        </div>
        <div class="topic-field"><label for="target">Speaking target</label><select id="target">${targetOptionsHTML()}</select></div>
        <p class="prep-status">The target is a guide. You choose when to stop.</p>
        ${button('arrow', 'Record this topic', 'use-topic', 'primary')}
        <a id="topic-sessions" class="topic-sessions" href="#library" hidden>Sessions for this topic</a>
      </section>
    </div>`;
}

export { cueCategory, cueDifficulty, cueHints, drawPreparation, drawTopic, drawnTopics, matchingTopics, prepSeconds, promptIndex, resetPreparation, savePreparationDuration, selectTopic, targetOptionsHTML, targetSeconds, togglePreparation, topicsHTML };
