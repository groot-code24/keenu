// SPDX-License-Identifier: GPL-3.0-or-later
// Which download to offer. No page code here, so it can be tested on its own.

/** The manifest's name for a computer, from what the browser says; `null` when it cannot tell. */
export function detectPlatform({ userAgent = '', platform = '', architecture = '' }) {
    const text = `${userAgent} ${platform}`.toLowerCase();
    const arm = architecture === 'arm' || /arm64|aarch64/.test(text);
    if (text.includes('win'))
        return arm ? 'windows-arm64' : 'windows-x64';
    if (text.includes('mac'))
        // Every Mac's browser says "Intel" and only some say more, so unless told it is Intel, this is the likelier one.
        // The page offers both.
        return architecture === 'x86' ? 'macos-x64' : 'macos-arm64';
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

/** Each platform as people call it, in the order the page lists them. */
const PLATFORM_NAMES = [
    ['windows-x64', 'Windows'],
    ['windows-arm64', 'Windows on Arm'],
    ['macos-arm64', 'macOS, Apple silicon'],
    ['macos-x64', 'macOS, Intel'],
    ['linux-x64', 'Linux x64'],
    ['linux-arm64', 'Linux Arm64'],
];

/** The release to show: the newest stable one, or the newest at all if none is stable yet. */
export function latestRelease(manifest) {
    const releases = manifest?.releases ?? [];
    return releases.find(release => release.channel === 'stable') ?? releases[0] ?? null;
}

/** The best file of `artifacts` for `platform`, or `null`. */
function bestFor(artifacts, platform) {
    const order = PREFERRED[(platform ?? '').split('-')[0]] ?? [];
    const rank = artifact => {
        const at = order.indexOf(artifact.kind);
        return at < 0 ? order.length : at;
    };
    return artifacts.filter(artifact => artifact.platform === platform).sort((a, b) => rank(a) - rank(b))[0] ?? null;
}

/** What to offer from `release`: the recommended file for `platform`, and the rest as alternatives. */
export function chooseDownloads(release, platform) {
    const artifacts = release?.artifacts ?? [];
    const recommended = bestFor(artifacts, platform);
    const others = artifacts.filter(artifact => artifact !== recommended);
    return { recommended, others };
}

/** One button per system this release has a file for: the best file for each. */
export function downloadsBySystem(release) {
    const artifacts = release?.artifacts ?? [];
    return PLATFORM_NAMES
        .map(([platform, name]) => ({ name, artifact: bestFor(artifacts, platform) }))
        .filter(({ artifact }) => artifact);
}

/** What to call a platform on the page. */
export function systemName(platform) {
    return PLATFORM_NAMES.find(([known]) => known === platform)?.[1] ?? platform;
}

/** The commands to install from this site, or `null` where its address could not work for other people. */
export function installCommands({ protocol, host } = {}) {
    if (protocol !== 'https:' || !host)
        return null;
    return {
        unix: `curl -fsSL https://${host}/install.sh | sh`,
        windows: `irm https://${host}/install.ps1 | iex`,
    };
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
