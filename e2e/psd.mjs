// A minimal flat RGB Photoshop document: the header, empty colour data,
// image resources and layer sections, then raw (uncompressed) planes. Random
// pixels keep a saved copy from compressing below the original's size.

import { randomBytes } from 'node:crypto';

export function makePsd(width, height) {
  const header = Buffer.alloc(26);
  header.write('8BPS', 0, 'ascii');
  header.writeUInt16BE(1, 4); // version
  header.writeUInt16BE(3, 12); // channels
  header.writeUInt32BE(height, 14);
  header.writeUInt32BE(width, 18);
  header.writeUInt16BE(8, 22); // bits per channel
  header.writeUInt16BE(3, 24); // RGB
  const emptySections = Buffer.alloc(12);
  const rawCompression = Buffer.alloc(2);
  return Buffer.concat([header, emptySections, rawCompression, randomBytes(width * height * 3)]);
}

/** How many layers a PSD's layer section lists (0 when it has none). */
export function layerCount(psd) {
  let offset = 26;
  offset += 4 + psd.readUInt32BE(offset); // colour mode data
  offset += 4 + psd.readUInt32BE(offset); // image resources
  if (psd.readUInt32BE(offset) === 0 || psd.readUInt32BE(offset + 4) === 0) return 0;
  return Math.abs(psd.readInt16BE(offset + 8));
}
