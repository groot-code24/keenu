// SPDX-License-Identifier: GPL-3.0-or-later
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { chooseDownloads, detectPlatform, formatSize, latestRelease, safeUrl } from '../../website/choose.js';

const artifact = (platform, kind) => ({ platform, kind, file_name: `${platform}.${kind}`, url: `https://x.invalid/${kind}` });

test('tells systems apart from what the browser says', () => {
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }), 'windows-x64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0)', architecture: 'arm' }), 'windows-arm64');
    assert.equal(detectPlatform({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' }), 'macos-x64');
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

test('only secure addresses become links', () => {
    assert.ok(safeUrl('https://x.invalid/a'));
    for (const bad of ['http://x.invalid/a', 'javascript:alert(1)', 'data:text/html,x', '', null, undefined, 7])
        assert.equal(safeUrl(bad), false);
});

test('sizes read as people expect', () => {
    assert.equal(formatSize(1736704), '1.7 MB');
    assert.equal(formatSize(2048), '2 KB');
});
