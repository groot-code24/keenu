// SPDX-License-Identifier: GPL-3.0-or-later
// The download page: everything on it comes from the release manifest, so it can only offer what exists.

import { chooseDownloads, detectPlatform, formatSize, latestRelease, safeUrl } from './choose.js';

const MANIFEST = './keenu-release.json';

const element = id => document.getElementById(id);

/** A new element with text, never parsed as markup: a manifest's words stay words. */
function make(tag, text, attributes = {}) {
    const node = document.createElement(tag);
    if (text !== undefined)
        node.textContent = text;
    for (const [name, value] of Object.entries(attributes))
        node.setAttribute(name, value);
    return node;
}

function signatureText(artifact) {
    const { signed, kind, signer } = artifact.signature;
    if (!signed)
        return 'Not code signed: your system may warn you. Check the SHA-256 below.';
    return `Signed (${kind})${signer ? ` by ${signer}` : ''}`;
}

function row(artifact) {
    const item = make('tr');
    const name = make('td');
    if (safeUrl(artifact.url))
        name.append(make('a', artifact.file_name, { href: artifact.url }));
    else
        name.textContent = artifact.file_name;
    item.append(name, make('td', artifact.platform), make('td', formatSize(artifact.size)),
        make('td', artifact.signature.signed ? 'Signed' : 'Not signed'),
        make('td', artifact.sha256, { class: 'sum' }));
    return item;
}

async function main() {
    let manifest;
    try {
        const answer = await fetch(MANIFEST, { cache: 'no-cache' });
        if (!answer.ok)
            throw new Error(`${answer.status}`);
        manifest = await answer.json();
    } catch {
        element('status').textContent = 'No release has been published here yet.';
        return;
    }
    const release = latestRelease(manifest);
    if (!release) {
        element('status').textContent = 'No release has been published here yet.';
        return;
    }

    let architecture = '';
    try {
        // Only browsers that have it say; the others are offered every file.
        const hints = await navigator.userAgentData?.getHighEntropyValues?.(['architecture']);
        architecture = hints?.architecture ?? '';
    } catch { /* The hint is optional. */ }
    const platform = detectPlatform({ userAgent: navigator.userAgent, platform: navigator.platform, architecture });
    const { recommended, others } = chooseDownloads(release, platform);

    element('status').textContent = `Keenu ${release.version}${release.channel === 'stable' ? '' : ' (test version)'}`;
    if (recommended) {
        const link = element('recommended');
        link.textContent = `Download for ${recommended.platform} (${recommended.kind})`;
        if (safeUrl(recommended.url))
            link.href = recommended.url;
        link.hidden = false;
        element('requirements').textContent = `Needs ${recommended.min_os}.`;
        element('signature').textContent = signatureText(recommended);
        element('checksum').textContent = `SHA-256: ${recommended.sha256}`;
    } else {
        element('requirements').textContent = platform
            ? `This release has no download for ${platform} yet.`
            : 'Choose your system from the list below.';
    }

    const table = element('others');
    for (const artifact of recommended ? [recommended, ...others] : others)
        table.append(row(artifact));
    element('others-section').hidden = false;

    const links = element('links');
    if (safeUrl(release.release_notes_url))
        links.append(make('a', 'Release notes', { href: release.release_notes_url }), ' ');
    if (safeUrl(release.github_release_url))
        links.append(make('a', 'GitHub release', { href: release.github_release_url }));
}

main();
