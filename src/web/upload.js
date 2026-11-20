// Image uploads straight to Discord (uploads.rs): the image is shrunk here, the bot signs a ticket,
// and the relay Worker posts the bytes to #pictures. The bot, on a Raspberry Pi, never carries them.
//
//   [file] ─► ≤1024 px WebP (JPEG where the browser cannot make WebP), EXIF dropped by the re-encode
//          ─► POST data-ticket {target, name, color}  ─► { ok, ticket }
//          ─► POST /upload, X-Upload-Ticket: ticket   ─► { ok, message }  ─► the page again
//
// Served from the bot as a file: the pages' CSP allows scripts from 'self' only, never inline.
'use strict';

(() => {
  const EDGE_MAX = 1024;
  const QUALITY = 0.85;
  const WEBP = 'image/webp';
  const JPEG = 'image/jpeg';
  // JPEG has no transparency: what was see-through comes out white, not black.
  const JPEG_BACKDROP = '#ffffff';
  const UPLOAD_PATH = '/upload';
  const TICKET_HEADER = 'X-Upload-Ticket';
  // Long enough to read "The new picture is up." before the page shows it.
  const RELOAD_MS = 1200;
  const UNREADABLE = 'That image could not be read. Try a PNG, JPEG, GIF or WebP.';
  const NO_FILE = 'Pick an image file first.';

  const say = (form, text) => {
    form.querySelector('output').textContent = text;
  };

  const encode = (canvas, type) => new Promise((resolve) => canvas.toBlob(resolve, type, QUALITY));

  // The picture redrawn at most EDGE_MAX on its longer side, on a fresh canvas: no EXIF survives.
  function draw(bitmap, backdrop) {
    const scale = Math.min(1, EDGE_MAX / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext('2d');
    if (backdrop) {
      context.fillStyle = backdrop;
      context.fillRect(0, 0, canvas.width, canvas.height);
    }
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    return canvas;
  }

  // WebP first; a browser that cannot make it (Safari) hands back PNG instead, so JPEG then.
  async function shrink(file) {
    const bitmap = await createImageBitmap(file);
    try {
      const webp = await encode(draw(bitmap, null), WEBP);
      if (webp && webp.type === WEBP) {
        return webp;
      }
      return await encode(draw(bitmap, JPEG_BACKDROP), JPEG);
    } finally {
      bitmap.close();
    }
  }

  // An answer's JSON; a page that is not JSON (the relay's offline page) becomes a plain refusal.
  async function answer(response) {
    try {
      return await response.json();
    } catch {
      return { ok: false, error: `The server answered ${response.status}. Try again shortly.` };
    }
  }

  const field = (form, name) => form.elements.namedItem(name)?.value ?? '';

  function ticket(form) {
    const asked = { target: form.dataset.upload, name: field(form, 'name'), color: field(form, 'color') };
    return fetch(form.dataset.ticket, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(asked) }).then(answer);
  }

  function send(text, image) {
    return fetch(UPLOAD_PATH, { method: 'POST', headers: { [TICKET_HEADER]: text, 'content-type': image.type }, body: image }).then(answer);
  }

  async function upload(form) {
    const file = form.querySelector('input[type=file]').files[0];
    if (!file) {
      say(form, NO_FILE);
      return;
    }

    say(form, 'Shrinking…');
    let image;
    try {
      image = await shrink(file);
    } catch {
      say(form, UNREADABLE);
      return;
    }

    say(form, 'Uploading…');
    const asked = await ticket(form);
    if (!asked.ok) {
      say(form, asked.error);
      return;
    }
    const sent = await send(asked.ticket, image);
    if (!sent.ok) {
      say(form, sent.error);
      return;
    }

    say(form, sent.message);
    setTimeout(() => location.reload(), RELOAD_MS);
  }

  for (const form of document.querySelectorAll('form[data-upload]')) {
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      const button = form.querySelector('button');
      button.disabled = true;
      try {
        await upload(form);
      } catch {
        say(form, 'The upload did not get through. Try again.');
      } finally {
        button.disabled = false;
      }
    });
  }
})();
