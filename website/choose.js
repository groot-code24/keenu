// SPDX-License-Identifier: GPL-3.0-or-later
// Which download to offer. No page code here, so it can be tested on its own.

/** The manifest's name for a computer, from what the browser says; `null` when it cannot tell. */
export function detectPlatform({ userAgent = '', platform = '', architecture = '' }) {
    const text = `${userAgent} ${platform}`.toLowerCase();
    const arm = architecture === 'arm' || /arm64|aarch64/.test(text);
    if (text.includes('win'))
        return arm ? 'windows-arm64' : 'windows-x64';
    if (text.includes('mac'))
        // A browser on an Intel Mac and on Apple Silicon says the same; the caller offers both.
        return arm ? 'macos-arm64' : 'macos-x64';
    if (text.includes('linux') || text.includes('x11'))
        return arm ? 'linux-arm64' : 'linux-x64';
    return null;
}

/** The kind of file to recommend on each platform, best first. */
const PREFERRED = {
    windows: ['msi', 'exe', 'zip'],
    macos: ['dmg', 'pkg'],
    linux: ['tar.gz', 'deb', 'rpm', 'appimage'],
};

/** The release to show: the newest stable one, or the newest at all if none is stable yet. */
export function latestRelease(manifest) {
    const releases = manifest?.releases ?? [];
    return releases.find(release => release.channel === 'stable') ?? releases[0] ?? null;
}

/** What to offer from `release`: the recommended file for `platform`, and the rest as alternatives. */
export function chooseDownloads(release, platform) {
    const artifacts = release?.artifacts ?? [];
    const here = artifacts.filter(artifact => artifact.platform === platform);
    const family = (platform ?? '').split('-')[0];
    const order = PREFERRED[family] ?? [];
    const rank = artifact => {
        const at = order.indexOf(artifact.kind);
        return at < 0 ? order.length : at;
    };
    const sorted = [...here].sort((a, b) => rank(a) - rank(b));
    const recommended = sorted[0] ?? null;
    const others = artifacts.filter(artifact => artifact !== recommended);
    return { recommended, others };
}

/** Only addresses that can be real: nothing else may become a link on the page. */
export function safeUrl(url) {
    return typeof url === 'string' && url.startsWith('https://');
}

/** A size as people read it. */
export function formatSize(bytes) {
    if (bytes >= 1024 * 1024)
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${Math.ceil(bytes / 1024)} KB`;
}
