// Render app/icon.svg into the raster icons that browsers and phones still ask for.
// Run from showcase/: node scripts/prepare-icons.mjs
import { mkdir, readFile, writeFile } from "node:fs/promises";
import sharp from "sharp";

const svg = await readFile("app/icon.svg");
const png = size => sharp(svg, { density: 72 * size / 64 }).resize(size, size).png({ compressionLevel: 9 }).toBuffer();

await writeFile("app/apple-icon.png", await png(180));
await mkdir("public/icons", { recursive: true });
for (const size of [192, 512]) await writeFile(`public/icons/icon-${size}.png`, await png(size));

// An .ico file may hold a PNG directly: a 6-byte header, one 16-byte directory entry, then the image.
const small = await png(32);
const header = Buffer.alloc(22);
header.writeUInt16LE(1, 2);            // type: icon
header.writeUInt16LE(1, 4);            // one image
header.writeUInt8(32, 6);              // width
header.writeUInt8(32, 7);              // height
header.writeUInt16LE(1, 10);           // colour planes
header.writeUInt16LE(32, 12);          // bits per pixel
header.writeUInt32LE(small.length, 14);
header.writeUInt32LE(22, 18);          // offset of the image data
await writeFile("app/favicon.ico", Buffer.concat([header, small]));
