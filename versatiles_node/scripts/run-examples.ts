import { execSync } from 'child_process';
import { readdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const examplesDir = join(__dirname, '../examples');

const files = readdirSync(examplesDir).filter((f) => f.endsWith('.ts'));
let failed = false;

for (const file of files) {
	const filePath = join(examplesDir, file);
	console.log(`\n=== Running ${file} ===`);
	try {
		execSync(`npx tsx "${filePath}"`, { stdio: 'inherit' });
	} catch {
		console.error(`Failed: ${file}`);
		failed = true;
	}
}

if (failed) {
	process.exit(1);
}
