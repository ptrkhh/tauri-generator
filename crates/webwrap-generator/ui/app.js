// @ts-check
// The generator's page. Everything a fetched site supplies (name, title,
// labels, descriptions) is set through textContent or value only: this page
// can write executables. Images are decoded here, by WebView2, from bytes
// Rust fetched, and leave as a 256x256 PNG that pack checks again.

/** @typedef {{ extension: string, mimeType: string, description: string }} Assoc */
/** @typedef {{ assoc: Assoc, ticked: boolean }} Row */
/** @typedef {{ label: string, url: string, contentType: string, bytes: number[] }} Candidate */
/** @typedef {{ wrappedUrl: string, appId: string, slug: string, name: string, adapter: string,
 *   associations: Row[], candidates: Candidate[], problem: string | null }} Discovery */
/** @typedef {{ path: string, replaced: 'none' | 'sameApp' | 'otherApp' }} Written */

const SIDE = 256;
const ADAPTERS = /** @type {Record<string, string>} */ ({
  none: 'None (generic site)',
  photopea: 'Photopea',
  vscodeDev: 'vscode.dev',
});
const REPLACED = { none: 'created', sameApp: 'updated', otherApp: 'replaced another generated app' };

/** @param {string} command @param {Record<string, unknown>} [args] @returns {Promise<any>} */
const invoke = (command, args) => /** @type {any} */ (window).__TAURI__.core.invoke(command, args);

/** @template {HTMLElement} T @param {string} id @returns {T} */
const $ = (id) => /** @type {T} */ (document.getElementById(id));

/** @type {Discovery | null} */
let found = null;
/** @type {{ label: string, png: Uint8Array<ArrayBuffer> }[]} */
let icons = [];
let selected = 0;

/** @param {string} text @param {boolean} [error] */
function say(text, error = false) {
  const element = $('status');
  element.textContent = text;
  element.classList.toggle('error', error);
}

/** A 256x256 transparent canvas. */
function square() {
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = SIDE;
  const context = /** @type {CanvasRenderingContext2D} */ (canvas.getContext('2d'));
  context.imageSmoothingQuality = 'high';
  return { canvas, context };
}

/** @param {HTMLCanvasElement} canvas @returns {Promise<Uint8Array<ArrayBuffer>>} */
async function pngOf(canvas) {
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
  return new Uint8Array(await /** @type {Blob} */ (blob).arrayBuffer());
}

/** Decode any image the WebView can, letterbox it into the square, and
 * return PNG bytes; null if it does not decode.
 * @param {Blob} blob @returns {Promise<Uint8Array<ArrayBuffer> | null>} */
async function toPng(blob) {
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    const { naturalWidth: width, naturalHeight: height } = image;
    if (!width || !height) return null;
    const scale = Math.min(SIDE / width, SIDE / height);
    const { canvas, context } = square();
    context.drawImage(image, (SIDE - width * scale) / 2, (SIDE - height * scale) / 2, width * scale, height * scale);
    return await pngOf(canvas);
  } catch {
    return null;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** One or two letters on a colour taken from the app id.
 * @param {string} name @param {string} appId */
async function initials(name, appId) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = (words.length > 1 ? words[0][0] + words[1][0] : (words[0] ?? '?').slice(0, 2)).toUpperCase();
  const { canvas, context } = square();
  context.fillStyle = `hsl(${parseInt(appId.slice(0, 4) || '0', 16) % 360} 55% 42%)`;
  context.beginPath();
  context.roundRect(8, 8, SIDE - 16, SIDE - 16, 48);
  context.fill();
  context.fillStyle = 'white';
  context.font = `600 ${letters.length > 1 ? 104 : 136}px "Segoe UI", system-ui, sans-serif`;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.fillText(letters, SIDE / 2, SIDE / 2 + 6);
  return pngOf(canvas);
}

/** @param {Uint8Array<ArrayBuffer>} png */
const pngUrl = (png) => URL.createObjectURL(new Blob([png], { type: 'image/png' }));

function renderGallery() {
  const gallery = $('gallery');
  gallery.replaceChildren();
  const tile = (/** @type {string} */ caption, /** @type {Node} */ face, /** @type {() => void} */ onclick, checked = false) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.setAttribute('role', 'radio');
    button.setAttribute('aria-checked', String(checked));
    const label = document.createElement('small');
    label.textContent = caption;
    button.append(face, label);
    button.onclick = onclick;
    gallery.append(button);
  };
  icons.forEach((icon, index) => {
    const image = document.createElement('img');
    image.alt = '';
    image.src = pngUrl(icon.png);
    tile(icon.label, image, () => { selected = index; renderGallery(); }, index === selected);
  });
  const plus = document.createElement('span');
  plus.className = 'blank';
  plus.textContent = '+';
  tile('Upload…', plus, () => $('upload').click());
}

/** The initials icon always sits last among the images, after any upload,
 * so it is regenerated when the name changes. */
async function refreshInitials() {
  if (!found) return;
  const png = await initials(/** @type {HTMLInputElement} */ ($('name')).value || found.name, found.appId);
  const at = icons.findIndex((icon) => icon.label === 'Initials');
  if (at >= 0) icons[at].png = png;
  else icons.push({ label: 'Initials', png });
  renderGallery();
}

/** @param {Row[]} rows */
function renderAssocs(rows) {
  const list = $('assocs');
  list.replaceChildren();
  for (const row of rows) {
    const label = document.createElement('label');
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.checked = row.ticked;
    box.onchange = () => { row.ticked = box.checked; };
    const text = document.createElement('span');
    text.textContent = `${row.assoc.extension}: ${row.assoc.description}`;
    const mime = document.createElement('span');
    mime.className = 'mime';
    mime.textContent = row.assoc.mimeType;
    label.append(box, text, mime);
    list.append(label);
  }
}

function showFiles() {
  const slug = /** @type {HTMLInputElement} */ ($('slug')).value.trim();
  $('files').textContent = slug ? `Makes ${slug}-x64.exe and ${slug}-arm64.exe` : '';
}

/** @param {Discovery} discovery */
async function show(discovery) {
  found = discovery;
  /** @type {HTMLInputElement} */ ($('name')).value = discovery.name;
  /** @type {HTMLInputElement} */ ($('slug')).value = discovery.slug;
  $('wrapped').textContent = discovery.wrappedUrl;
  $('adapter').textContent = ADAPTERS[discovery.adapter] ?? discovery.adapter;
  showFiles();
  renderAssocs(discovery.associations);
  icons = [];
  for (const candidate of discovery.candidates) {
    const png = await toPng(new Blob([new Uint8Array(candidate.bytes)], { type: candidate.contentType }));
    if (png) icons.push({ label: candidate.label, png });
  }
  selected = 0;
  await refreshInitials();
  $('details').hidden = false;
  $('result').hidden = true;
}

/** @param {SubmitEvent} event */
async function find(event) {
  event.preventDefault();
  const url = /** @type {HTMLInputElement} */ ($('url')).value.trim();
  const button = /** @type {HTMLButtonElement} */ ($('find-button'));
  button.disabled = true;
  say(`Looking at ${url}…`);
  try {
    /** @type {Discovery} */
    const discovery = await invoke('discover', { url });
    /** @type {HTMLInputElement} */ ($('url')).value = discovery.wrappedUrl;
    await show(discovery);
    say(discovery.problem
      ? `Could not read the site (${discovery.problem}). You can still set a name and an icon.`
      : `Found ${discovery.name}.`, Boolean(discovery.problem));
  } catch (error) {
    say(String(error), true);
  } finally {
    button.disabled = false;
  }
}

async function generate() {
  if (!found || !icons[selected]) return;
  const button = /** @type {HTMLButtonElement} */ ($('generate'));
  /** @type {string | null} */
  const folder = await invoke('pick_folder');
  if (!folder) return;
  button.disabled = true;
  say('Generating…');
  try {
    /** @type {Written[]} */
    const written = await invoke('generate', {
      request: {
        url: found.wrappedUrl,
        name: /** @type {HTMLInputElement} */ ($('name')).value.trim(),
        slug: /** @type {HTMLInputElement} */ ($('slug')).value.trim(),
        adapter: found.adapter,
        associations: found.associations.filter((row) => row.ticked).map((row) => row.assoc),
        iconPng: Array.from(icons[selected].png),
        outDir: folder,
      },
    });
    const list = $('outputs');
    list.replaceChildren();
    for (const output of written) {
      const item = document.createElement('li');
      const path = document.createElement('span');
      path.className = 'path';
      path.textContent = `${output.path}: ${REPLACED[output.replaced] ?? output.replaced} `;
      const reveal = document.createElement('button');
      reveal.type = 'button';
      reveal.textContent = 'Show in folder';
      reveal.onclick = () => invoke('show_in_folder', { path: output.path }).catch((error) => say(String(error), true));
      item.append(path, reveal);
      list.append(item);
    }
    $('result').hidden = false;
    say('');
  } catch (error) {
    say(String(error), true);
  } finally {
    button.disabled = false;
  }
}

/** @param {SubmitEvent} event */
function addAssoc(event) {
  event.preventDefault();
  if (!found) return;
  const value = (/** @type {string} */ id) => /** @type {HTMLInputElement} */ ($(id)).value.trim();
  let extension = value('add-ext').toLowerCase();
  if (!extension.startsWith('.')) extension = `.${extension}`;
  const row = { assoc: { extension, mimeType: value('add-mime'), description: value('add-desc') }, ticked: true };
  // The schema refuses an extension claimed twice; the new row replaces it.
  found.associations = found.associations.filter((other) => other.assoc.extension !== extension).concat(row);
  renderAssocs(found.associations);
  /** @type {HTMLFormElement} */ ($('add-assoc')).reset();
}

async function upload() {
  const input = /** @type {HTMLInputElement} */ ($('upload'));
  const file = input.files?.[0];
  input.value = '';
  if (!file) return;
  const png = await toPng(file);
  if (!png) {
    say(`${file.name} is not an image this generator can read.`, true);
    return;
  }
  icons = icons.filter((icon) => icon.label !== 'Uploaded');
  icons.splice(icons.length - 1, 0, { label: 'Uploaded', png });
  selected = icons.length - 2;
  renderGallery();
}

$('find').addEventListener('submit', find);
$('add-assoc').addEventListener('submit', addAssoc);
$('generate').addEventListener('click', generate);
$('upload').addEventListener('change', upload);
$('slug').addEventListener('input', showFiles);
$('name').addEventListener('input', refreshInitials);
