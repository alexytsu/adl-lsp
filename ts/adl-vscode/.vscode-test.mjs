import { defineConfig } from '@vscode/test-cli';
import os from 'node:os';
import path from 'node:path';

export default defineConfig({
	files: 'out/test/**/*.test.js',
	// VS Code listens on a socket inside the user data directory, and socket
	// paths are limited to about 100 characters. The default directory inside
	// this repository is too deep, so use a short one.
	launchArgs: ['--user-data-dir', path.join(os.tmpdir(), 'adl-vscode-test')],
});
