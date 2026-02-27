import { writeFileSync } from 'node:fs';
import { generateVplTypescript } from '../index.js';

const ts = generateVplTypescript();
writeFileSync('vpl.ts', ts);
writeFileSync('src/vpl.ts', ts);
console.log('Generated vpl.ts and src/vpl.ts');
