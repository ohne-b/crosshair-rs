const repository = 'https://github.com/ohne-b/crosshair-rs';
export const releasesPage = `${repository}/releases`;
export const releasesApi =
  'https://api.github.com/repos/ohne-b/crosshair-rs/releases?per_page=100';

type Asset = {
  name?: unknown;
  state?: unknown;
  browser_download_url?: unknown;
};
type Release = { draft?: unknown; published_at?: unknown; assets?: unknown };

// Include prereleases; choose the most recently published release with an installer.
export function selectInstaller(payload: unknown): string | null {
  if (!Array.isArray(payload)) return null;
  const releases = payload
    .filter(
      (value): value is Release =>
        value !== null &&
        typeof value === 'object' &&
        value.draft === false &&
        typeof value.published_at === 'string' &&
        Number.isFinite(Date.parse(value.published_at)),
    )
    .sort(
      (a, b) =>
        Date.parse(b.published_at as string) -
        Date.parse(a.published_at as string),
    );
  for (const release of releases) {
    if (!Array.isArray(release.assets)) continue;
    for (const asset of release.assets as Asset[]) {
      if (
        !asset ||
        asset.state !== 'uploaded' ||
        typeof asset.name !== 'string' ||
        !/^crosshair-rs-.+-windows-x64-setup\.exe$/i.test(asset.name) ||
        typeof asset.browser_download_url !== 'string'
      )
        continue;
      try {
        const url = new URL(asset.browser_download_url);
        if (
          url.origin === 'https://github.com' &&
          !url.username &&
          !url.password &&
          !url.search &&
          !url.hash &&
          url.pathname.startsWith('/ohne-b/crosshair-rs/releases/download/') &&
          decodeURIComponent(url.pathname.split('/').at(-1) ?? '') ===
            asset.name
        ) {
          return url.href;
        }
      } catch {
        /* Ignore malformed release assets. */
      }
    }
  }
  return null;
}

export async function resolveDownload(
  fetcher: typeof fetch = fetch,
): Promise<string> {
  try {
    const response = await fetcher(releasesApi, {
      headers: { Accept: 'application/vnd.github+json' },
      credentials: 'omit',
      signal: AbortSignal.timeout(6000),
    });
    if (!response.ok) return releasesPage;
    return selectInstaller(await response.json()) ?? releasesPage;
  } catch {
    return releasesPage;
  }
}
