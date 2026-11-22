import { expect, test } from 'bun:test';
import { state } from '../public/state.js';
import { downloadFile } from '../public/ui.js';

test('repeated macOS exports get distinct source URLs and retain the download flag', () => {
  const app = state.app, previousDocument = globalThis.document;
  const links = [];
  globalThis.document = {
    baseURI:'http://127.0.0.1:4318/',
    createElement:() => ({ click() {}, remove() {} }),
    body:{ append:link => links.push(link) },
  };
  try {
    state.app = { desktop:true, platform:'macos' };
    const source = '/api/sessions/one/media?download';
    downloadFile(source, 'take.mp4');
    downloadFile(source, 'take.mp4');
    const urls = links.map(link => new URL(link.href));
    expect(urls[0].href).not.toBe(urls[1].href);
    expect(urls.every(url => url.pathname === '/api/sessions/one/media' && url.searchParams.has('download'))).toBe(true);
    const file = new Blob(['backup']);
    downloadFile(file, 'backup.json');
    downloadFile(file, 'backup.json');
    expect(links[2].href).toStartWith('blob:');
    expect(links[2].href).not.toBe(links[3].href);
    expect(links[2].download).toBe('backup.json');
  } finally {
    state.app = app;
    if (previousDocument === undefined) delete globalThis.document;
    else globalThis.document = previousDocument;
  }
});
