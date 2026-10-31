#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
// For a key kept off GitHub: takes a published release's list, signs it, and puts both in website/ to commit to main.
// Only then do the website and the install scripts offer that release.
//
//   node scripts/sign-release-list.mjs <version> <file holding the base64 secret key>

import { createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';

const REPOSITORY = 'groot-code24/keenu';
// What comes before a 32-byte Ed25519 secret key in PKCS #8.
const PKCS8_PREFIX = Buffer.from('302e020100300506032b657004220420', 'hex');

const [version, keyFile] = process.argv.slice(2);
if (!version || !keyFile)
    throw new Error('Usage: sign-release-list.mjs <version> <key file>');

const seed = Buffer.from(readFileSync(keyFile, 'utf8').trim(), 'base64');
if (seed.length !== 32)
    throw new Error('That file does not hold a signing key.');
const key = createPrivateKey({ key: Buffer.concat([PKCS8_PREFIX, seed]), format: 'der', type: 'pkcs8' });

const expected = /^PUBLIC_KEY="(.*)"$/m.exec(readFileSync('scripts/install/install.sh', 'utf8'))?.[1];
const actual = createPublicKey(key).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64');
if (actual !== expected)
    throw new Error('That is not the key the install scripts trust.');

const answer = await fetch(`https://github.com/${REPOSITORY}/releases/download/v${version}/keenu-release.json`);
if (!answer.ok)
    throw new Error(`The release's list could not be fetched: ${answer.status}`);
const list = Buffer.from(await answer.arrayBuffer());
const manifest = JSON.parse(list.toString('utf8'));
if (manifest.product !== 'Keenu' || manifest.releases?.[0]?.version !== version)
    throw new Error(`That is not a list for Keenu ${version}.`);

writeFileSync('website/keenu-release.json', list);
writeFileSync('website/keenu-release.json.sig', sign(null, list, key).toString('base64'));
console.log(`Signed the list for ${version}: commit website/keenu-release.json and its .sig to main.`);
