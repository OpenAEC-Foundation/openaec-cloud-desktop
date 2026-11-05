#!/usr/bin/env node
// node scripts/ops/ops.mjs <ops-repo> <opdracht> …   (zie ops.ts)
//
// Bundelt ops.ts met esbuild tegen de broncode van een kloon van Open Planner Studio (`<ops-repo>/src`, alias `@`)
// en draait de bundel. Er komt niets in de OPS-kloon: de bundel staat in een tijdelijke map. esbuild komt uit
// `npx` (versie zoals in OPS), of uit $ESBUILD. Pakketten die OPS gebruikt (bijv. immer) komen uit
// `<ops-repo>/node_modules` (na `npm ci` daar), of uit $NODE_PATH.
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const [ops, ...rest] = process.argv.slice(2);
if (!ops) {
  console.error('gebruik: node scripts/ops/ops.mjs <ops-repo> lees|wijzig …');
  process.exit(2);
}
const hier = dirname(fileURLToPath(import.meta.url));
const map = mkdtempSync(join(tmpdir(), 'ops-hulp-'));
try {
  const bundel = join(map, 'ops.mjs');
  const esbuild = process.env.ESBUILD ? [process.env.ESBUILD] : ['npx', '--yes', 'esbuild@0.28.1'];
  execFileSync(esbuild[0], [
    ...esbuild.slice(1), join(hier, 'ops.ts'), '--bundle', '--platform=node', '--format=esm', `--outfile=${bundel}`,
    `--alias:@=${resolve(ops, 'src')}`, '--log-level=warning',
    '--define:import.meta.env.DEV=false', '--define:import.meta.env.PROD=true',
    '--define:import.meta.env.MODE="production"', '--define:__OPS_DEV_INSTANCE__="gen"',
  ], { stdio: 'inherit', env: { ...process.env, NODE_PATH: process.env.NODE_PATH ?? resolve(ops, 'node_modules') } });
  const r = spawnSync(process.execPath, [bundel, ...rest], { stdio: 'inherit' });
  process.exitCode = r.status ?? 1;
} finally {
  rmSync(map, { recursive: true, force: true });
}
