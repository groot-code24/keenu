// SPDX-License-Identifier: GPL-3.0-or-later
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { chooseDownloads, detectPlatform, downloadsBySystem, formatSize, installCommands, latestRelease, safeUrl } from '../../website/choose.js';

const artifact = (platform, kind) => ({ platform, kind, file_name: `${platform}.${kind}`, url: `https://x.invalid/${kind}` });

test('tells systems apart from what the browser says', () => {
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }), 'windows-x64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0)', architecture: 'arm' }), 'windows-arm64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' }), 'macos-arm64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', architecture: 'x86' }), 'macos-x64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (X11; Linux x86_64)' }), 'linux-x64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (X11; Linux aarch64)' }), 'linux-arm64');
    assert.equal(detectPlatform({ userAgent: 'Something else' }), null);
});

test('recommends the best kind for this system and offers the rest', () => {
    const release = { artifacts: [artifact('windows-x64', 'zip'), artifact('windows-x64', 'msi'), artifact('linux-x64', 'deb')] };
    const { recommended, others } = chooseDownloads(release, 'windows-x64');
    assert.equal(recommended.kind, 'msi');
    assert.equal(others.length, 2);
});

test('with nothing for this system, recommends nothing rather than something wrong', () => {
    const release = { artifacts: [artifact('linux-x64', 'deb')] };
    const { recommended, others } = chooseDownloads(release, 'windows-x64');
    assert.equal(recommended, null);
    assert.equal(others.length, 1);
    assert.equal(chooseDownloads(release, null).recommended, null);
});

test('shows the newest stable release, or a test version only if nothing is stable', () => {
    const pre = { version: '2.0.0-rc.1', channel: 'prerelease' };
    const stable = { version: '1.9.0', channel: 'stable' };
    assert.equal(latestRelease({ releases: [pre, stable] }), stable);
    assert.equal(latestRelease({ releases: [pre] }), pre);
    assert.equal(latestRelease({ releases: [] }), null);
    assert.equal(latestRelease(null), null);
});

test('offers one file for each system the release has', () => {
    const release = { artifacts: [artifact('linux-x64', 'deb'), artifact('linux-x64', 'tar.gz'), artifact('macos-x64', 'dmg'), artifact('macos-arm64', 'dmg'), artifact('windows-x64', 'zip'), artifact('windows-x64', 'msi')] };
    assert.deepEqual(downloadsBySystem(release).map(({ name, artifact: file }) => [name, file.kind]), [
        ['Windows', 'msi'],
        ['macOS, Apple silicon', 'dmg'],
        ['macOS, Intel', 'dmg'],
        ['Linux x64', 'tar.gz'],
    ]);
    assert.deepEqual(downloadsBySystem(null), []);
});

test('install commands use the address of a secure site, and no other', () => {
    assert.deepEqual(installCommands({ protocol: 'https:', host: 'keenu.vercel.app' }), {
        unix: 'curl -fsSL https://keenu.vercel.app/install.sh | sh',
        windows: 'irm https://keenu.vercel.app/install.ps1 | iex',
    });
    assert.equal(installCommands({ protocol: 'http:', host: 'localhost:3000' }), null);
    assert.equal(installCommands({ protocol: 'file:', host: '' }), null);
    assert.equal(installCommands(), null);
});

test('only secure addresses become links', () => {
    assert.ok(safeUrl('https://x.invalid/a'));
    for (const bad of ['http://x.invalid/a', 'javascript:alert(1)', 'data:text/html,x', '', null, undefined, 7])
        assert.equal(safeUrl(bad), false);
});

test('sizes read as people expect', () => {
    assert.equal(formatSize(1736704), '1.7 MB');
    assert.equal(formatSize(2048), '2 KB');
});
