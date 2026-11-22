import { editSessionTags, libraryParams, libraryReturnRoute, libraryRoute, sessionMetadataHTML } from './library.js';
import { $, button, copyText, dialog, downloadFile, escape, fileStem, icon, notifyError, plain, toast } from './ui.js';
import { state, upsert } from './state.js';
import { captureModes, captureOpen } from './capture-options.js';
import { drawPreparation, drawnTopics, matchingTopics, promptIndex, resetPreparation, savePreparationDuration, selectTopic, togglePreparation } from './topics.js';
import { cueCards } from './cue-cards.js';
import { closeScriptEditor, editScript, readingScriptHTML } from './prompter-page.js';
import { exportTranscript, filterTranscript, playAt } from './review.js';
import { managePracticeItem, practicePageSize, renderPractice, savePracticeMarks, updatePracticeMarks } from './practice.js';
import { prefs, replacePreferences, savePrefs } from './preferences.js';
import { maxBackupBytes, parsePreferencesBackup, serializePreferencesBackup } from './preferences-backup.js';
import { collectPracticeItems, countScriptWords, scriptMaxWords } from './script-review.js';
import { api } from './api.js';
import { copyFeedback, copyPrompt, promptDefault, sendFeedback } from './feedback.js';
import { accentOptions, accentPreference, applyAppearance, themeOptions } from './appearance.js';
import { importTopics, loadKeyStatus, removeSavedKey, saveFeedbackSettings, showSettingsFields } from './settings.js';

function bindEvents({ go, render, renderCapture, showRoute, newTake, openSession, record, persist, retryPending, recover, runTranscription, applyModelState, syncPreview }) {
  let pendingBackup, pendingDeletion;
  function changeLibrary(fields) {
    const params = libraryParams();
    for (const [key, value] of Object.entries(fields)) {
      if (value) params.set(key, value); else params.delete(key);
    }
    go(libraryRoute(params.get('filter'), params));
  }

  function useTopic() {
    state.recordingCue = cueCards[promptIndex()];
    go('studio');
  }

  const actions = {
    'toggle-session-selection':()=>{
      state.selectingSessions = !state.selectingSessions;
      state.selectedSessionIds.clear();
      render(); $('[data-action="toggle-session-selection"]')?.focus();
    },
    'select-all-sessions':()=>{
      const all = state.visibleSessionIds.every(id => state.selectedSessionIds.has(id));
      state.selectedSessionIds = new Set(all ? [] : state.visibleSessionIds);
      render(); $('[data-action="select-all-sessions"]')?.focus();
    },
    'delete-selected-sessions':()=>{
      pendingDeletion = state.visibleSessionIds.filter(id => state.selectedSessionIds.has(id));
      if (!pendingDeletion.length) return;
      const count = pendingDeletion.length;
      dialog(`Delete ${count} selected ${count === 1 ? 'take' : 'takes'}?`, `<p class="info">These recordings and their transcripts will be permanently removed from this device.</p><div class="dialog-actions">${button(null, 'Cancel', 'close', 'secondary')}${button('trash', 'Delete selected takes', 'confirm-delete-selected', 'danger')}</div>`);
    },
    'confirm-delete-selected':async()=>{
      const queue = [...(pendingDeletion || [])];
      if (!queue.length) return;
      pendingDeletion = null;
      const modal = $('#dialog'), preventCancel = event => event.preventDefault();
      modal.querySelectorAll('button').forEach(button => { button.disabled = true; });
      modal.addEventListener('cancel', preventCancel);
      const deleted = new Set(), failures = [];
      await Promise.all(Array.from({ length:Math.min(4, queue.length) }, async () => {
        while (queue.length) {
          const id = queue.shift();
          try { await api(`/sessions/${id}`, { method:'DELETE' }); deleted.add(id); }
          catch (error) { failures.push(error.message); }
        }
      }));
      state.app.sessions = state.app.sessions.filter(take => !deleted.has(take.id));
      for (const id of deleted) state.selectedSessionIds.delete(id);
      if (!failures.length) state.selectingSessions = false;
      modal.removeEventListener('cancel', preventCancel); modal.close(); render();
      const message = `Deleted ${deleted.size} ${deleted.size === 1 ? 'take' : 'takes'}.`;
      toast(failures.length ? `${message} ${failures.length} could not be deleted. ${failures[0]}` : message, Boolean(failures.length));
    },
    'export-preferences':()=>{
      try {
        const blob = new Blob([serializePreferencesBackup(prefs)], { type:'application/json' });
        downloadFile(blob, 'rehearsal-practice-backup.json');
      } catch (error) { notifyError(error); }
    },
    'import-preferences':()=>$('#preferences-file').click(),
    'restore-preferences':()=>{
      if (!pendingBackup || state.phase !== 'idle') return;
      try {
        replacePreferences(pendingBackup);
        state.activeScript = typeof prefs.scriptDraft === 'string' ? prefs.scriptDraft : '';
        $('#dialog').close();
        applyAppearance();
        render();
        toast('Practice backup restored.');
      } catch (error) { notifyError(error); }
    },
    studio:()=>go('studio'), library:()=>go('library'), settings:()=>go('settings?section=models'),
    'library-view':button=>{
      changeLibrary({ view:button.dataset.view === 'calendar' ? 'calendar' : '', month:'', day:'' });
      $('[data-action="library-view"][aria-pressed="true"]')?.focus({preventScroll:true});
    },
    'calendar-month':button=>{
      changeLibrary({ month:button.dataset.month, day:'' });
      $(`[data-direction="${button.dataset.direction}"]`)?.focus({preventScroll:true});
    },
    'calendar-today':button=>{
      changeLibrary({ month:button.dataset.day.slice(0,7), day:button.dataset.day });
      $('[data-action="calendar-today"]')?.focus({preventScroll:true});
    },
    'calendar-day':button=>{
      changeLibrary({ day:button.dataset.day });
      $('#calendar-results')?.focus();
    },
    'choose-folder':()=>window.ipc?.postMessage('choose-folder'),
    mode:b=>{
      state.recorderMode = b.dataset.kind;
      renderCapture();
      $('.switch [aria-pressed="true"]').focus({ preventScroll:true });
    }, new:newTake, record:()=>record(),
    'retry-camera':()=>syncPreview(true),
    prep:()=>{
      if (state.pending) { toast('Save the unsaved take before starting another.', true); return; }
      return togglePreparation(screen => { useTopic(); void record(screen); });
    },
    'next-prompt':()=>{
      const matches = matchingTopics();
      let choices = matches.filter(id => !drawnTopics.has(id));
      if (!choices.length) {
        drawnTopics.clear();
        choices = matches.filter(id => id !== promptIndex());
      }
      selectTopic(choices[Math.floor(Math.random() * choices.length)] ?? matches[0]);
    },
    'use-topic':()=>{
      if (state.pending) { toast('Save the unsaved take before starting another.', true); return; }
      useTopic();
    },
    'remove-cue':()=>{ state.recordingCue = null; render(); $('[data-action="record"]').focus(); },
    'edit-script':()=>{ editScript(); void syncPreview(); },
    'cancel-script':()=>{ closeScriptEditor(state.activeScript ? '.prompter-edit' : '.prompter-empty button'); void syncPreview(); },
    'prompter-fullscreen':async ()=>{
      try {
        if (document.fullscreenElement) await document.exitFullscreen();
        else await $('#prompter-panel').requestFullscreen();
      } catch { toast('Full screen is unavailable in this browser.', true); }
    },
    'script-jump':b=>playAt(Math.max(0, Number(b.dataset.seconds) - .4)),
    'practice-mark':b=>managePracticeItem(b, 'practiced', 'Marked as practiced.'),
    'practice-dismiss':b=>managePracticeItem(b, 'dismissed', 'Difference dismissed.'),
    'practice-restore':b=>managePracticeItem(b, '', 'Moved back to review.'),
    'practice-clear':()=>{
      const marks = prefs.takePractice?.[state.active.id] || {};
      const ids = collectPracticeItems(state.active).filter(item => !marks[item.id]).map(item => item.id);
      const message = `${ids.length} ${ids.length === 1 ? 'difference' : 'differences'} cleared from this take.`;
      updatePracticeMarks(ids, 'dismissed', message);
      renderPractice(() => $('[data-action="practice-undo"]'), message);
    },
    'practice-undo':()=>{
      const marks = { ...prefs.takePractice?.[state.active.id] };
      for (const [id, status] of state.practiceView.undo.previous) {
        if (status) marks[id] = status;
        else delete marks[id];
      }
      savePracticeMarks(state.active.id, marks);
      savePrefs();
      state.practiceView.undo = null;
      renderPractice(() => $('#practice-status'), 'Last action undone.');
    },
    'practice-more':()=>{
      const previousLimit = state.practiceView.limit;
      state.practiceView.limit += practicePageSize;
      renderPractice(() => document.querySelectorAll('.practice-list [data-action="script-jump"]')[previousLimit]);
    },
    import:()=>{ if(state.pending) toast('Save the unsaved take before importing another.',true); else $('#import-file')?.click(); },
    retry:()=>void retryPending(),
    'recover-recording':b=>void recover(b.dataset.id),
    'download-recording':b=>downloadFile(`/api/recordings/${b.dataset.id}/media`, 'recovered-take'),
    'discard-recording':b=>{
      state.dialogTarget = b.dataset.id;
      dialog('Discard interrupted recording?', '<p>This permanently removes the saved recording chunks.</p><div class="dialog-actions">' + button('trash', 'Discard recording', 'confirm-discard-recording', 'danger') + '</div>');
    },
    'confirm-discard-recording':async()=>{
      try {
        const id = state.dialogTarget;
        await api(`/recordings/${id}/discard`, { method:'DELETE' });
        state.app.recordings = (state.app.recordings || []).filter(take => take.id !== id);
        if (state.pending?.capture?.id === id) state.pending = null;
        $('#dialog').close(); render(); toast('Interrupted recording discarded.');
      } catch (error) { notifyError(error); }
    },
    'preview-unsaved':async()=>{
      if(!state.pending) return;
      if (state.pending.capture && !state.pending.capture.preview) {
        const id = state.pending.capture.id;
        dialog('Interrupted recording', `<p class="info">The portion already written to disk is available below. Keep this page open to retry any remaining data.</p><video controls style="width:100%;max-height:260px" src="/api/recordings/${id}/media"></video><div class="dialog-actions"><button class="button" data-action="download-recording" data-id="${id}">Download saved portion</button><button class="button danger" data-action="discard-recording" data-id="${id}">Discard recording</button></div>`);
        return;
      }
      let file;
      try { file = state.pending.file || await state.pending.capture.preview(); }
      catch (error) { notifyError(error); return; }
      const url=URL.createObjectURL(file);
      const video=file.type.startsWith('video/');
      const modal=dialog('Unsaved take',`<p class="info">Save this take before closing the app.</p><${video?'video':'audio'} controls style="width:100%;max-height:260px" src="${url}"></${video?'video':'audio'}><div class="dialog-actions">${button('download','Download original','raw-unsaved')}${button('trash','Discard unsaved take','discard-unsaved','danger')}</div>`);
      const download = modal.querySelector('[data-action="raw-unsaved"]');
      download.onclick = () => downloadFile(file, file.name || state.pending.capture.filename);
      modal.addEventListener('close', () => {
        download.onclick = null;
        URL.revokeObjectURL(url);
      }, { once:true });
    },
    'discard-unsaved':()=>{
      if (state.pending?.capture) { actions['discard-recording']({ dataset:{ id:state.pending.capture.id } }); return; }
      state.pending=null; $('#dialog').close(); render(); toast('Unsaved take discarded.');
    },
    'open-session':b=>openSession(b.dataset.id),
    'play-recording':()=>{ if (state.player?.paused) playAt(); else state.player?.pause(); },
    'seek-line':b=>playAt(state.active.segments[Number(b.dataset.index)].start),
    copy:()=>state.active && copyText(plain(state.active),'Transcript copied.'),
    'save-media':()=>state.active && downloadFile(`${state.active.media}?download`,`${fileStem(state.active.title)}.${state.active.file.split('.').at(-1)}`),
    'edit-tags':editSessionTags,
    transcribe:runTranscription,
    rename:()=>{state.dialogTarget=state.active;dialog('Rename session',`<form id="rename-form"><label class="field">Name<input id="rename-input" value="${escape(state.active.title)}" maxlength="110" required></label><div class="dialog-actions"><button class="button primary">Save name</button></div></form>`);$('#rename-input').select();},
    delete:()=>{state.dialogTarget=state.active;dialog('Delete this take?',`<p class="info">The recording and transcript for “${escape(state.active.title)}” will be removed from this device.</p><div class="dialog-actions">${button(null,'Cancel','close')}${button('trash','Delete take','confirm-delete','danger')}</div>`);},
    'confirm-delete':async b=>{
      const id=state.dialogTarget.id; b.disabled=true;
      try{await api(`/sessions/${id}`,{method:'DELETE'});state.app.sessions=state.app.sessions.filter(s=>s.id!==id);$('#dialog').close();go(libraryReturnRoute());toast('Take deleted.');}
      catch(err){b.disabled=false;notifyError(err);}
    },
    close:()=>$('#dialog').close(),
    'import-topics':()=>$('#topics-file').click(),
    'reset-coach-prompt':()=>{ $('#default-question').value = promptDefault; $('#default-question').focus(); },
    'save-coach':saveFeedbackSettings,
    'check-key-storage':()=>loadKeyStatus(true),
    'remove-openai-key':()=>dialog('Remove saved OpenAI key?', `<p class="info">This removes Rehearsal’s saved key from your system credential store.</p><div class="dialog-actions">${button(null, 'Cancel', 'close', 'secondary')}${button('trash','Remove key','confirm-remove-openai-key','danger')}</div>`),
    'confirm-remove-openai-key':removeSavedKey,
    'refresh-models':()=>{
      if (!state.catalogRequest) {
        state.catalogRequest = api('/models/catalog').catch(() => ({ catalogCurrent: false })).then(payload => {
          state.catalogRequest = null;
          applyModelState(payload);
        });
      }
      applyModelState({});
    },
    install: async b => {
      b.disabled = true;
      try {
        const modelState = await api(`/models/${b.dataset.id}/install`, { method:'POST' });
        if (b.hasAttribute('data-first-run')) {
          $('#dialog').close();
          go('settings?section=models');
        }
        applyModelState(modelState);
      } catch (error) {
        b.disabled = false;
        notifyError(error);
      }
    },
    'copy-prompt':copyPrompt,
    'copy-feedback':copyFeedback,
    'send-feedback':()=>sendFeedback(),
    'retry-feedback-save':()=>sendFeedback(true),
    'coach-jump':b=>playAt(Number(b.dataset.seconds)),
  };
  window.addEventListener('rehearse-folder', event => {
    const field = $('#recording-folder');
    if (field && typeof event.detail === 'string') {
      field.value = event.detail;
      field.focus();
    }
  });
  document.addEventListener('click',e=>{
    const b=e.target.closest('[data-action]');
    const menu = $('.take-menu[open]');
    if (menu && (!menu.contains(e.target) || b)) {
      menu.open = false;
      if (b && menu.contains(b)) menu.querySelector('summary').focus();
    }
    if(b && Object.hasOwn(actions, b.dataset.action)) { void actions[b.dataset.action](b); return; }
    const nav=e.target.closest('.main-nav a, .brand');
    if(nav && state.phase!=='idle'){e.preventDefault();toast('Finish this recording first.');}
  });
  document.addEventListener('input', e => {
    if (e.target.id === 'transcript-search') filterTranscript(e.target.value);
    if (e.target.id === 'script-input') $('#script-error').hidden = true;
  });
  document.addEventListener('fullscreenchange', () => {
    const control = $('[data-action="prompter-fullscreen"]');
    if (control) {
      const full = Boolean(document.fullscreenElement);
      control.title = full ? 'Exit full screen' : 'Full screen';
      control.setAttribute('aria-label', control.title);
      control.innerHTML = icon(full ? 'collapse' : 'expand');
    }
  });
  document.addEventListener('change',async e=>{
    const el=e.target;
    if (el.id === 'settings-section') go(`settings?section=${el.value}`);
    if (el.id === 'topics-file') {
      const file = el.files[0]; el.value = '';
      if (file) await importTopics(file);
      return;
    }
    if (el.matches('[data-session-select]')) {
      const id = el.dataset.sessionSelect;
      if (el.checked) state.selectedSessionIds.add(id); else state.selectedSessionIds.delete(id);
      render(); $(`[data-session-select="${id}"]`)?.focus(); return;
    }
    if (el.id === 'preferences-file') {
      const file = el.files[0];
      el.value = '';
      if (!file || state.phase !== 'idle') return;
      try {
        if (file.size > maxBackupBytes) throw new Error('Choose a practice backup smaller than 5 MiB.');
        const restored = parsePreferencesBackup(await file.text());
        if (state.phase !== 'idle') return;
        pendingBackup = restored;
        const modal = dialog('Restore practice backup?', `<p class="info">This replaces your saved script draft, preferences, and practice marks. Recordings and transcripts stay unchanged.</p><div class="dialog-actions">${button(null, 'Cancel', 'close', 'secondary')}${button('upload', 'Restore backup', 'restore-preferences', 'primary')}</div>`);
        modal.addEventListener('close', () => { pendingBackup = undefined; }, { once:true });
      } catch (error) { notifyError(error); }
      return;
    }
    if (el.id === 'practice-status' || el.id === 'practice-type') {
      state.practiceView[el.id === 'practice-status' ? 'status' : 'type'] = el.value;
      state.practiceView.limit = practicePageSize;
      renderPractice(() => document.getElementById(el.id));
    }
    if (el.id === 'session-filter') {
      changeLibrary({ filter:el.value, day:'' });
      $('#session-filter')?.focus();
    }
    if (el.id === 'retention-limit') {
      const custom = el.value === 'custom';
      const field = $('#retention-custom-field'), input = field.querySelector('input');
      field.hidden = !custom;
      input.disabled = !custom;
      input.required = custom;
      if (custom) input.focus();
    }
    if (el.name === 'theme' && Object.hasOwn(themeOptions, el.value)) {
      prefs.theme = el.value;
      applyAppearance();
      $(`input[name="accent"][value="${accentPreference()}"]`).checked = true;
      savePrefs();
    }
    if (el.name === 'accent' && Object.hasOwn(accentOptions, el.value)) {
      prefs.accent = el.value;
      applyAppearance();
      savePrefs();
    }
    if(el.id==='import-file' && el.files[0] && state.phase === 'idle' && !state.pending) {
      const file=el.files[0];
      go('studio');
      void persist(file, file.name.replace(/\.[^.]+$/,''), { compress: !state.app.lightMedia && Boolean(prefs.compactRecordings) });
    }
    if (el.id === 'topic-select') selectTopic(Number(el.value));
    if (el.id === 'prep-seconds') savePreparationDuration(el);
    if (el.id === 'cue-category' || el.id === 'cue-difficulty') {
      prefs[el.id === 'cue-category' ? 'cueCategory' : 'cueDifficulty'] = el.value;
      drawnTopics.clear();
      selectTopic(promptIndex(), true);
    }
    if (el.id === 'prompter-speed' || el.id === 'prompter-size' || el.id === 'prompter-mirror') {
      if (el.id === 'prompter-speed') {
        el.value = String(Math.max(60, Math.min(250, Number(el.value) || 130)));
        prefs.prompterSpeed = Number(el.value);
      } else if (el.id === 'prompter-size') prefs.prompterSize = Number(el.value);
      else prefs.prompterMirror = el.checked;
      savePrefs();
    }
    if (el.id === 'prompter-mode' && state.phase === 'idle' && Object.hasOwn(captureModes, el.value)) {
      prefs.prompterMode = el.value;
      savePrefs();
      renderCapture();
      $('#prompter-mode').focus({ preventScroll:true });
    }
    if (el.id === 'prompter-hide-background' || el.id === 'hide-background') {
      prefs[el.id === 'hide-background' ? 'hideBackground' : 'prompterHideBackground'] = el.checked;
      savePrefs(); renderCapture();
      document.getElementById(el.id).focus({ preventScroll:true });
    }
    if(el.id==='provider') showSettingsFields();
    if(el.id==='export' && el.value) {exportTranscript(el.value);el.value='';}
    if (el.id === 'target' || el.id === 'default-target') { prefs.targetSeconds = Number(el.value); savePrefs(); }
    if (el.id === 'default-prep' && el.reportValidity()) { prefs.prepSeconds = el.valueAsNumber; savePrefs(); }
    if (el.id === 'compact-recordings') { prefs.compactRecordings = el.checked; savePrefs(); }
    if (el.id === 'hide-session-video') {
      prefs.hideSessionVideo = el.checked;
      $('#session-video').hidden = el.checked;
      $('#session-playback-controls').hidden = !el.checked;
      state.player.controls = !el.checked;
      savePrefs();
    }
    if(el.name==='asr') {prefs.asr=el.value;savePrefs();}
  });
  document.addEventListener('submit',async e=>{
    if (e.target.id === 'storage-form') {
      e.preventDefault();
      const form = e.target;
      const save = form.querySelector('[type="submit"]');
      if (save.disabled) return;
      const fields = new FormData(form);
      const limit = fields.get('limit');
      const keep = limit === 'custom' ? Number(fields.get('custom')) : limit ? Number(limit) : null;
      save.disabled = true;
      try {
        const result = await api('/storage', { method:'PATCH', body:{ folder:fields.get('folder'), keep } });
        state.app.storage = result.storage;
        state.app.sessions = result.sessions;
        if (form.isConnected) form.elements.folder.value = result.storage.folder;
        toast('Storage settings saved.');
      } catch (error) { notifyError(error); }
      finally { save.disabled = false; }
      return;
    }
    if (e.target.id === 'session-tags-form') {
      e.preventDefault();
      const form = e.target;
      const save = form.querySelector('[type="submit"]');
      if (save.disabled) return;
      const fields = new FormData(form);
      const tags = fields.get('tags').split(',').map(tag => tag.trim()).filter(Boolean);
      const cueId = fields.get('cueId') || null;
      const id = state.dialogTarget.id;
      const modal = $('#dialog');
      save.disabled = true;
      try {
        upsert(await api(`/sessions/${id}`, { method:'PATCH', body:{ tags, cueId } }));
        const restoreFocus = modal.open && form.isConnected;
        if (restoreFocus) modal.close();
        if (state.view === 'review' && state.active?.id === id) {
          $('.session-metadata').outerHTML = sessionMetadataHTML(state.active);
          if (restoreFocus) $('[data-action="edit-tags"]').focus();
        }
        toast('Tags saved.');
      } catch (error) {
        save.disabled = false;
        notifyError(error);
      }
      return;
    }
    if (e.target.id === 'script-form') {
      e.preventDefault();
      const input = $('#script-input');
      const script = input.value.trim();
      const count = countScriptWords(script);
      if (!count || count > scriptMaxWords) {
        const error = $('#script-error');
        error.textContent = count ? `Use at most ${scriptMaxWords.toLocaleString()} words.` : 'Add a few words to your script first.';
        error.hidden = false;
        input.focus();
        return;
      }
      state.activeScript = prefs.scriptDraft = script;
      prefs.prompterAuto = $('#prompter-auto').checked;
      savePrefs();
      $('.prompter-text').innerHTML = readingScriptHTML(script);
      $('#prompter-panel').classList.add('has-script');
      closeScriptEditor('[data-action="prompter-toggle"]');
      state.prompter.updateScript(count);
      void syncPreview();
      return;
    }
    if(e.target.id!=='rename-form')return;
    e.preventDefault(); const input=$('#rename-input'), title=input.value.trim(); if(!title) return;
    try{upsert(await api(`/sessions/${state.dialogTarget.id}`,{method:'PATCH',body:{title}}));$('#dialog').close();render();toast('Session renamed.');}
    catch(err){notifyError(err);}
  });
  window.addEventListener('hashchange', () => showRoute(true));
  window.addEventListener('pagehide', () => {
    if (state.view === 'topics') {
      resetPreparation();
      drawPreparation();
    }
  });
  window.addEventListener('keydown',e=>{
    if (e.key === 'Escape') {
      if (state.view === 'library' && state.selectingSessions && !$('#dialog').open) {
        actions['toggle-session-selection'](); return;
      }
      const menu = $('.take-menu[open]');
      if (menu) { menu.open = false; menu.querySelector('summary').focus(); }
    }
    if(e.altKey && e.code==='KeyR' && !e.repeat && captureOpen() && !$('#dialog').open && !e.target.matches('input,textarea,select')) { e.preventDefault(); void record(); }
  });
  window.addEventListener('beforeunload',e=>{if(state.phase!=='idle'||state.pending){e.preventDefault();e.returnValue='';}});
}

export { bindEvents };
