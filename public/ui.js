import { state } from './state.js';

const icons = {
  mic:'<rect x="9" y="2.75" width="6" height="12" rx="3"/><path d="M5.5 10.5v1.25a6.5 6.5 0 0 0 13 0V10.5M12 18.25v3m-3 0h6"/>',
  video:'<rect x="3" y="6" width="12" height="12" rx="2"/><path d="m15 10 6-3v10l-6-3"/>',
  screen:'<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8m-4-4v4"/>',
  layers:'<path d="m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5"/>',
  settings:'<circle cx="12" cy="12" r="3"/><path d="m10 3-.6 2.3-1.8 1-2.3-.6-2 3.5L5 11v2l-1.7 1.8 2 3.5 2.3-.6 1.8 1 .6 2.3h4l.6-2.3 1.8-1 2.3.6 2-3.5L19 13v-2l1.7-1.8-2-3.5-2.3.6-1.8-1L14 3z"/>',
  'chevron-right':'<path d="m9 6 6 6-6 6"/>', arrow:'<path d="M4 12h15m-6-6 6 6-6 6"/>', back:'<path d="M20 12H5m6 6-6-6 6-6"/>',
  waveform:'<path d="M3 10v4m4-8v12m5-16v20m5-16v12m4-8v4"/>',
  file:'<path d="M13.5 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9.5zM13.5 3v6.5H20M8 13h8m-8 4h6"/>',
  copy:'<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
  upload:'<path d="M12 16V4m-4 4 4-4 4 4M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/>',
  download:'<path d="M12 4v12m-4-4 4 4 4-4M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/>',
  play:'<path d="M9 5.5v13l10-6.5z" fill="currentColor" stroke-linejoin="round"/>', pause:'<path d="M9 5.5v13m6-13v13" stroke-width="2.7"/>',
  restart:'<path d="M3 12a9 9 0 1 1 2.8 6.5M3 4v8h8"/>',
  spark:'<path d="M12 2.5 14.1 9.9 21.5 12l-7.4 2.1-2.1 7.4-2.1-7.4L2.5 12l7.4-2.1L12 2.5ZM19 3v4m-2-2h4"/>',
  clock:'<circle cx="12" cy="12" r="8.5"/><path d="M12 7v5l3 2"/>', headphones:'<path d="M4 13v-2a8 8 0 0 1 16 0v2m-16 0v3a2 2 0 0 0 2 2h2v-7H6a2 2 0 0 0-2 2Zm16 0v3a2 2 0 0 1-2 2h-2v-7h2a2 2 0 0 1 2 2Z"/>',
  edit:'<path d="m14 6 4 4m-13 9 4-.9 11-11a2.8 2.8 0 0 0-4-4l-11 11L4 19z"/>',
  trash:'<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13m-8 3v7m4-7v7"/>',
  close:'<path d="m6 6 12 12M18 6 6 18"/>', plus:'<path d="M12 5v14M5 12h14"/>',
  expand:'<path d="M9 4H4v5m11-5h5v5M4 15v5h5m11-5v5h-5"/>',
  collapse:'<path d="M4 9h5V4m6 0v5h5M9 20v-5H4m11 5v-5h5"/>',
  link:'<path d="M10 13.5a4 4 0 0 0 5.8 0l3.2-3.2a4 4 0 0 0-5.7-5.7l-1.5 1.5m2.2 4.4a4 4 0 0 0-5.8 0L5 13.7a4 4 0 0 0 5.7 5.7l1.5-1.5"/>',
  search:'<circle cx="10.5" cy="10.5" r="5.75"/><path d="m15 15 4.5 4.5"/>',
  shuffle:'<path d="M3.5 7h2.6c5.9 0 5.9 10 11.8 10H21m-3-3 3 3-3 3M3.5 17h2.6c1.8 0 3.1-.95 4.15-2.3m3.5-5.4C14.8 7.95 16.1 7 17.9 7H21m-3-3 3 3-3 3"/>',
  more:'<circle cx="5" cy="12" r="1" fill="currentColor"/><circle cx="12" cy="12" r="1" fill="currentColor"/><circle cx="19" cy="12" r="1" fill="currentColor"/>',
  calendar:'<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M7 3v4m10-4v4M3 10h18m-13 4h1m3 0h1m3 0h1m-9 3h1m3 0h1"/>',
  list:'<path d="M8 5h12M8 12h12M8 19h12M4 5h.01M4 12h.01M4 19h.01"/>',
  'chevron-left':'<path d="m15 6-6 6 6 6"/>',
};

const icon = name => `<i data-icon="${name}" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round">${icons[name] || ''}</svg></i>`;

const $ = selector => document.querySelector(selector);

const escape = value => String(value ?? '').replace(/[&<>"']/g,ch => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));

const fmt = val => { const sec = Math.max(0, Math.floor(Number(val) || 0)); return `${String(Math.floor(sec / 60)).padStart(2,'0')}:${String(sec % 60).padStart(2,'0')}`; };

const formatStorage = bytes => bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MiB` : `${Math.ceil(bytes / 1024)} KiB`;

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle:'medium' });

const niceDate = date => date && Number.isFinite(Date.parse(date)) ? dateFormat.format(new Date(date)) : '';

const dateKey = date => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;

const sessionDay = session => niceDate(session.createdAt) ? dateKey(new Date(session.createdAt)) : '';

const iconButton = (symbol, label, action, extra='') => `<button type="button" class="icon-btn" aria-label="${label}" title="${label}" data-action="${action}" ${extra}>${icon(symbol)}</button>`;

const button = (symbol, label, action, cls='', extra='') => `<button type="button" class="button ${cls}" data-action="${action}" ${extra}>${symbol ? icon(symbol) : ''}${label}</button>`;

const heading = (title, description, action = '') => `<div class="page-heading"><div><h1 id="page-title" tabindex="-1">${title}</h1>${description ? `<p>${description}</p>` : ''}</div>${action}</div>`;

let toastTimer;

function toast(message, error=false) {
  const el = $('#dialog').open && $('#dialog-notice') || (document.fullscreenElement && $('#prompter-notice')) || $('#toast');
  $('#toast').hidden=true;
  el.textContent=message; el.classList.toggle('error', error); el.hidden=false;
  clearTimeout(toastTimer); toastTimer=setTimeout(() => { el.hidden=true; },4200);
}

const notifyError = error => toast(error.message || String(error),true);

async function copyText(text, success) {
  try { await navigator.clipboard.writeText(text); }
  catch {
    const temp=document.createElement('textarea'); temp.value=text; temp.readOnly=true;
    ( $('#dialog').open ? $('#dialog') : document.body ).append(temp);
    temp.select(); const ok=document.execCommand('copy'); temp.remove();
    if(!ok) { toast('Clipboard access was blocked.',true); return; }
  }
  toast(success);
}

function downloadFile(source, name) {
  const a = document.createElement('a');
  if (source instanceof Blob) {
    const url = URL.createObjectURL(source);
    a.href = url;
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  } else {
    const url = new URL(source, document.baseURI);
    // Wry identifies a completed macOS download by its source URL.
    if (state.app.desktop && state.app.platform === 'macos') url.searchParams.set('export', crypto.randomUUID());
    a.href = url.href;
  }
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
}

function dialog(title,content) {
  const modal=$('#dialog');
  modal.innerHTML=`<header><h2 id="dialog-title">${title}</h2>${iconButton('close','Close dialog','close')}</header>${content}<p id="dialog-notice" class="info" role="status" hidden></p>`;
  if(!modal.open) modal.showModal();
  return modal;
}

const plain = s => s.segments.map(row=>row.text).join('\n');

const srtTime = seconds => new Date(Math.round(seconds*1000)).toISOString().slice(11,23).replace('.',',');

const fileStem = title => title.replace(/[^a-z\d-]+/gi,'-').replace(/^-+|-+$/g,'').toLowerCase().slice(0,80) || 'rehearsal-take';

export { $, button, copyText, dateKey, dialog, downloadFile, escape, fileStem, fmt, formatStorage, heading, icon, iconButton, niceDate, notifyError, plain, sessionDay, srtTime, toast };
