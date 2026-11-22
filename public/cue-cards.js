let data = { difficulties:{}, categories:{}, cueCards:[] };
export let { difficulties, categories, cueCards } = data;

export async function loadCueCards() {
  const response = await fetch(new URL('./cue-data.json', import.meta.url));
  if (!response.ok) throw new Error('Could not load the cue cards.');
  data = await response.json();
  ({ difficulties, categories, cueCards } = data);
}

export function setCustomTopics(topics = []) {
  categories = topics.length ? { ...data.categories, custom:'My topics' } : data.categories;
  cueCards = [...data.cueCards, ...topics.map(({ id, prompt }) => ({
    id, prompt, label:prompt, category:'custom', difficulty:'', hints:[],
  }))];
}

function csvRows(text) {
  const rows = [];
  let row = [], field = '', quoted = false, closed = false;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quoted) {
      if (ch !== '"') field += ch;
      else if (text[i + 1] === '"') { field += '"'; i++; }
      else { quoted = false; closed = true; }
    } else if (ch === ',' || ch === '\n') {
      row.push(field.trim());
      if (ch === '\n') { rows.push(row); row = []; }
      field = ''; closed = false;
    } else if (!closed && ch === '"' && !field.trim()) {
      field = ''; quoted = true;
    } else if (ch === '"' || (closed && ch.trim())) {
      throw new Error('Check the quotation marks in your CSV file.');
    } else field += ch;
  }
  if (quoted) throw new Error('A quoted field in your CSV is missing its closing quotation mark.');
  rows.push([...row, field.trim()]);
  return rows.filter(cells => cells.some(Boolean));
}

export function readTopicList(name, text) {
  text = text.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n');
  let prompts;
  if (/\.txt$/i.test(name)) prompts = text.split('\n').map(line => line.trim()).filter(Boolean);
  else if (/\.csv$/i.test(name)) {
    const rows = csvRows(text);
    const column = rows[0]?.findIndex(cell => /^(topic|prompt)$/i.test(cell)) ?? -1;
    if (column >= 0) rows.shift();
    else if (rows.some(row => row.length !== 1)) {
      throw new Error('For a CSV with several columns, name the topic column “topic” or “prompt”.');
    }
    prompts = rows.map((row, i) => {
      const prompt = row[Math.max(0, column)];
      if (!prompt) throw new Error(`Topic ${i + 1} is missing its prompt.`);
      return prompt;
    });
  } else throw new Error('Choose a .txt or .csv file.');
  if (!prompts.length) throw new Error('The file contains no topics.');
  return prompts;
}
